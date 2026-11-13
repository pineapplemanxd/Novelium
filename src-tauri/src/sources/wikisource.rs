use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::Utc;
use regex::Regex;
use scraper::{Html, Selector};
use serde_json::Value;
use tokio::sync::Mutex;
use url::Url;

use crate::error::{AppError, AppResult};
use crate::formats::text_html::clean_html_fragment_excluding;
use crate::models::{
    Chapter, DiscoveryQuery, NovelRecord, NovelStatus, NovelSummary, NovelTranslation,
    SourceCapabilities, SourceConfigField,
};
use crate::net::SourceHttpClient;
use crate::sources::{make_chapter_id, make_novel_id, NovelSource};

#[derive(Debug, Clone)]
struct WikiCatItem {
    title: String,
    external_id: String,
    genre: String,
}

type CategoryCache = HashMap<String, (Instant, Vec<WikiCatItem>)>;

pub struct WikisourceSource {
    http: SourceHttpClient,
    base_url_override: Option<String>,
    cat_cache: Arc<Mutex<CategoryCache>>,
}

impl WikisourceSource {
    pub fn new(http: SourceHttpClient) -> Self {
        Self {
            http,
            base_url_override: None,
            cat_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn with_base_url(http: SourceHttpClient, base_url: &str) -> Self {
        Self {
            http,
            base_url_override: Some(base_url.trim_end_matches('/').to_string()),
            cat_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn base_origin_for_lang(&self, lang: &str) -> String {
        if let Some(ref ov) = self.base_url_override {
            return ov.clone();
        }
        format!("https://{lang}.wikisource.org")
    }

    fn resolve_lang_and_title<'a>(
        config: &HashMap<String, Value>,
        raw_id: &'a str,
    ) -> (String, &'a str) {
        if let Some((prefix, rest)) = raw_id.split_once('|') {
            let p = prefix.trim();
            if !p.is_empty() && p.len() <= 8 && p.chars().all(|c| c.is_ascii_alphabetic()) {
                return (p.to_ascii_lowercase(), rest.trim());
            }
        }
        (Self::wiki_lang(config), raw_id.trim())
    }

    fn wiki_lang(config: &HashMap<String, Value>) -> String {
        let lang = config
            .get("wiki_lang")
            .and_then(|v| v.as_str())
            .unwrap_or("en")
            .trim();
        if lang.chars().all(|c| c.is_ascii_alphabetic()) && !lang.is_empty() {
            lang.to_ascii_lowercase()
        } else {
            "en".to_string()
        }
    }

    fn category_for_lang_and_genre(lang: &str, genre: Option<&str>) -> &'static str {
        match lang {
            "fr" => "Catégorie:Romans",
            "de" => "Kategorie:Roman",
            "es" => "Categoría:Novelas",
            "it" => "Categoria:Romanzi",
            "pl" => "Kategoria:Powieści",
            "pt" => "Categoria:Romances",
            _ => match genre.unwrap_or("All").to_ascii_lowercase().as_str() {
                "science fiction" => "Category:Science_fiction_novels",
                "fantasy" => "Category:Fantasy_novels",
                "mystery" => "Category:Mystery_novels",
                "romance" => "Category:Romance_novels",
                "adventure" => "Category:Adventure_novels",
                "horror" | "gothic" => "Category:Horror_novels",
                "historical" => "Category:Historical_fiction_novels",
                "short stories" => "Category:Short_stories",
                _ => "Category:Novels",
            },
        }
    }

    fn extra_search_categories(lang: &str) -> &'static [&'static str] {
        if lang == "en" {
            &[
                "Category:Novels",
                "Category:Science_fiction_novels",
                "Category:Adventure_novels",
                "Category:Mystery_novels",
                "Category:Horror_novels",
                "Category:Romance_novels",
                "Category:Fantasy_novels",
                "Category:Historical_fiction_novels",
            ]
        } else {
            &[]
        }
    }

    fn page_url(&self, config: &HashMap<String, Value>, title_or_slug: &str) -> AppResult<String> {
        let (lang, slug) = Self::resolve_lang_and_title(config, title_or_slug);
        let origin = self.base_origin_for_lang(&lang);
        let mut url = Url::parse(&origin)?;
        {
            let mut segs = url
                .path_segments_mut()
                .map_err(|_| AppError::InvalidInput("Invalid Wikisource base URL".into()))?;
            segs.push("wiki");
            let normalized = slug.trim().replace(' ', "_");
            for part in normalized.split('/').filter(|p| !p.is_empty()) {
                segs.push(part);
            }
        }
        Ok(url.into())
    }

    fn slug_from_wiki_url(url_str: &str) -> Option<String> {
        let parsed = Url::parse(url_str).ok()?;
        let path = parsed.path().strip_prefix("/wiki/")?;
        if path.is_empty() {
            return None;
        }
        let decoded = decode_percent_lossy(path).replace('_', " ");
        Some(decoded)
    }

    fn natural_chapter_rank(sub_title: &str) -> (u8, i64, String) {
        let lower = sub_title.to_ascii_lowercase();
        if lower.contains("preface") || lower.contains("intro") || lower.contains("foreword") {
            return (0, 0, lower);
        }
        if lower.contains("prologue") {
            return (1, 0, lower);
        }
        if lower.contains("epilogue") || lower.contains("afterword") || lower.contains("appendix") {
            return (3, 0, lower);
        }
        for token in lower.split(|c: char| !c.is_ascii_alphanumeric()) {
            if let Ok(n) = token.parse::<i64>() {
                return (2, n, lower);
            }
            if let Some(rn) = parse_roman(token) {
                return (2, rn, lower);
            }
        }
        (2, i64::MAX / 2, lower)
    }

    fn clean_wikisource_html(raw_html: &str) -> (String, Vec<String>, i64) {
        let doc = Html::parse_document(raw_html);
        let prose_sel =
            Selector::parse("#mw-content-text .mw-parser-output, .mw-parser-output").unwrap();
        let target_html = doc
            .select(&prose_sel)
            .next()
            .map(|e| e.inner_html())
            .unwrap_or_else(|| raw_html.to_string());

        clean_html_fragment_excluding(
            &target_html,
            &[
                ".ws-noexport",
                "#ws-data",
                "#headerContainer",
                ".ws-header",
                ".wst-header",
                ".headertemplate",
                ".footertemplate",
                ".gen_header_block",
                ".mw-editsection",
                "table.ambox",
                ".toc",
                ".prp-page-qualityheader",
                ".pagenum",
                ".ws-pagenum",
                ".subNote",
                ".noprint",
                "sup.reference",
                "style",
                "script",
            ],
        )
    }

    async fn fetch_category_items(
        &self,
        lang: &str,
        category: &str,
        genre_label: &str,
    ) -> AppResult<Vec<WikiCatItem>> {
        let origin = self.base_origin_for_lang(lang);
        let cache_key = format!("{origin}|{category}");
        {
            let guard = self.cat_cache.lock().await;
            if let Some((at, items)) = guard.get(&cache_key) {
                if at.elapsed() < Duration::from_secs(1800) {
                    return Ok(items.clone());
                }
            }
        }

        let empty_cfg = HashMap::new();
        let url = self.page_url(&empty_cfg, &format!("{lang}|{category}"))?;
        let html = self.http.get_text(&url).await?;
        let items = {
            let base_url = Url::parse(&url)?;
            let doc = Html::parse_document(&html);
            let link_sel = Selector::parse("#mw-pages a[href], .mw-category a[href]").unwrap();

            let mut seen = HashSet::new();
            let mut items = Vec::new();
            for a in doc.select(&link_sel) {
                let Some(href) = a.value().attr("href") else {
                    continue;
                };
                if href.contains('?') || href.starts_with('#') {
                    continue;
                }
                let Ok(abs) = base_url.join(href) else {
                    continue;
                };
                let Some(decoded_title) = Self::slug_from_wiki_url(abs.as_str()) else {
                    continue;
                };
                if decoded_title.contains(':') || decoded_title.contains('/') {
                    continue;
                }
                let label = a
                    .text()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                let title = if label.is_empty() {
                    decoded_title.clone()
                } else {
                    label
                };
                if seen.insert(decoded_title.clone()) {
                    items.push(WikiCatItem {
                        title,
                        external_id: decoded_title,
                        genre: genre_label.to_string(),
                    });
                }
            }
            items
        };

        let mut guard = self.cat_cache.lock().await;
        guard.insert(cache_key, (Instant::now(), items.clone()));
        Ok(items)
    }

    fn parse_novel_page(&self, external_id: &str, lang: &str, slug: &str, html: &str) -> NovelRecord {
        let doc = Html::parse_document(html);

        let text_of = |sel_str: &str| -> String {
            Selector::parse(sel_str)
                .ok()
                .and_then(|sel| doc.select(&sel).next())
                .map(|el| {
                    el.text()
                        .collect::<Vec<_>>()
                        .join(" ")
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_default()
        };

        let ws_title = text_of("#ws-title");
        let h1_title = text_of("#firstHeading");
        let title = if !ws_title.is_empty() {
            ws_title
        } else if !h1_title.is_empty() {
            h1_title
        } else {
            slug.replace('_', " ")
        };

        let ws_author = text_of("#ws-author");
        let hdr_author = text_of(
            ".wst-header .fn a, .wst-header .fn, .wst-header .contributor-text a, .wst-header a[href*='Author:'], .gen_header_foreword a, #headerContainer .fn",
        );
        let raw_author = if !ws_author.is_empty() {
            ws_author
        } else {
            hdr_author
        };
        let cleaned_author = raw_author
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let author = if !cleaned_author.is_empty() {
            cleaned_author
        } else {
            "Wikisource Edition".to_string()
        };

        let cover_url = Selector::parse(".mw-parser-output img")
            .ok()
            .and_then(|sel| {
                doc.select(&sel).find_map(|img| {
                    let src = img.value().attr("src")?;
                    let width = img
                        .value()
                        .attr("width")
                        .and_then(|w| w.parse::<u32>().ok())
                        .unwrap_or(100);
                    let lower_src = src.to_ascii_lowercase();
                    if width < 60
                        || lower_src.contains("red_copyright")
                        || lower_src.contains("oojs_ui")
                        || lower_src.contains("rule_segment")
                        || lower_src.contains("fleuron")
                        || lower_src.contains("wikisource-logo")
                        || lower_src.contains("pd-icon")
                        || lower_src.contains(".svg")
                    {
                        return None;
                    }
                    if src.starts_with("//") {
                        Some(format!("https:{src}"))
                    } else if src.starts_with("http") {
                        Some(src.to_string())
                    } else {
                        None
                    }
                })
            });

        let mut genres = Vec::new();
        if let Ok(cat_sel) = Selector::parse("#mw-normal-catlinks ul li a") {
            for a in doc.select(&cat_sel) {
                let c = a.text().collect::<Vec<_>>().join(" ").trim().to_string();
                if !c.is_empty()
                    && !c.contains("Wikisource")
                    && !c.contains("Pages")
                    && !c.contains("DefaultSort")
                    && !genres.contains(&c)
                {
                    genres.push(c);
                }
                if genres.len() >= 5 {
                    break;
                }
            }
        }

        let (_, paras, _) = Self::clean_wikisource_html(html);
        let desc = paras.into_iter().take(3).collect::<Vec<_>>().join("\n\n");
        let now = Utc::now().to_rfc3339();

        NovelRecord {
            id: make_novel_id(self.id(), external_id),
            source_id: self.id().to_string(),
            external_id: external_id.to_string(),
            title,
            author_id: None,
            author,
            description: if desc.is_empty() {
                "Public domain work hosted on Wikisource.".to_string()
            } else {
                desc
            },
            cover_url,
            local_cover_path: None,
            status: NovelStatus::Completed,
            genres,
            tags: vec!["Wikisource".into()],
            rating: None,
            popularity: 0,
            chapter_count: 0,
            last_updated_remote: None,
            metadata: serde_json::json!({ "language": lang }),
            is_local: false,
            created_at: now.clone(),
            updated_at: now,
        }
    }

    fn extract_translations(
        &self,
        current_external_id: &str,
        current_lang: &str,
        html: &str,
    ) -> Vec<NovelTranslation> {
        let doc = Html::parse_document(html);
        let mut out = Vec::new();
        let mut seen = HashSet::new();

        let current_title = Selector::parse("#ws-title, #firstHeading")
            .ok()
            .and_then(|s| doc.select(&s).next())
            .map(|e| e.text().collect::<String>().trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                current_external_id
                    .split_once('|')
                    .map(|(_, s)| s)
                    .unwrap_or(current_external_id)
                    .replace('_', " ")
            });
        out.push(NovelTranslation {
            language_code: current_lang.to_string(),
            language_label: current_lang.to_uppercase(),
            novel_id: make_novel_id(self.id(), current_external_id),
            source_id: self.id().to_string(),
            external_id: current_external_id.to_string(),
            title: current_title,
        });
        seen.insert(current_lang.to_string());

        let Ok(link_sel) = Selector::parse("li.interlanguage-link a") else {
            return out;
        };
        for a in doc.select(&link_sel) {
            let Some(href) = a.value().attr("href") else {
                continue;
            };
            let Some(slug) = Self::slug_from_wiki_url(href) else {
                continue;
            };
            let lang_code = a
                .value()
                .attr("lang")
                .or_else(|| a.value().attr("hreflang"))
                .map(|s| s.trim().to_ascii_lowercase())
                .or_else(|| {
                    Url::parse(href)
                        .ok()
                        .and_then(|u| u.host_str().and_then(|h| h.split('.').next()).map(str::to_ascii_lowercase))
                });
            let Some(lang_code) = lang_code.filter(|l| !l.is_empty()) else {
                continue;
            };
            if !seen.insert(lang_code.clone()) {
                continue;
            }
            let autonym = a
                .value()
                .attr("data-language-autonym")
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| {
                    let txt = a.text().collect::<String>().trim().to_string();
                    if txt.is_empty() {
                        lang_code.to_uppercase()
                    } else {
                        txt
                    }
                });
            let ext_id = format!("{lang_code}|{slug}");
            out.push(NovelTranslation {
                language_code: lang_code,
                language_label: autonym,
                novel_id: make_novel_id(self.id(), &ext_id),
                source_id: self.id().to_string(),
                external_id: ext_id,
                title: slug.replace('_', " "),
            });
        }
        out
    }
}

fn decode_percent_lossy(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Ok(hi), Ok(lo)) = (
                u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 2]).unwrap_or(""), 16),
                u8::from_str_radix(std::str::from_utf8(&bytes[i + 2..i + 3]).unwrap_or(""), 16),
            ) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn parse_roman(s: &str) -> Option<i64> {
    static ROMAN_RE: OnceLock<Regex> = OnceLock::new();
    let re = ROMAN_RE.get_or_init(|| {
        Regex::new(r"(?i)^m{0,3}(cm|cd|d?c{0,3})(xc|xl|l?x{0,3})(ix|iv|v?i{0,3})$").unwrap()
    });
    if s.is_empty() || s.len() > 15 || !re.is_match(s) {
        return None;
    }
    let val = |c: char| -> Option<i64> {
        match c.to_ascii_lowercase() {
            'i' => Some(1),
            'v' => Some(5),
            'x' => Some(10),
            'l' => Some(50),
            'c' => Some(100),
            'd' => Some(500),
            'm' => Some(1000),
            _ => None,
        }
    };
    let chars: Vec<char> = s.chars().collect();
    let mut total = 0i64;
    for i in 0..chars.len() {
        let cur = val(chars[i])?;
        let next = chars.get(i + 1).and_then(|c| val(*c)).unwrap_or(0);
        if cur < next {
            total -= cur;
        } else {
            total += cur;
        }
    }
    if total > 0 { Some(total) } else { None }
}

#[async_trait]
impl NovelSource for WikisourceSource {
    fn id(&self) -> &str {
        "wikisource"
    }

    fn name(&self) -> &str {
        "Wikisource"
    }

    fn kind(&self) -> &str {
        "builtin"
    }

    fn base_url(&self) -> &str {
        self.base_url_override
            .as_deref()
            .unwrap_or("https://en.wikisource.org")
    }

    fn description(&self) -> &str {
        "Free digital library of public domain & openly licensed novels on Wikisource."
    }

    fn capabilities(&self) -> SourceCapabilities {
        SourceCapabilities {
            search: true,
            browse_popular: true,
            browse_trending: true,
            browse_latest: false,
            browse_genre: true,
            filter_status: true,
            genres: vec![
                "Fiction".into(),
                "Science Fiction".into(),
                "Fantasy".into(),
                "Mystery".into(),
                "Romance".into(),
                "Adventure".into(),
                "Horror".into(),
                "Historical".into(),
                "Short Stories".into(),
            ],
        }
    }

    fn config_schema(&self) -> Vec<SourceConfigField> {
        vec![SourceConfigField {
            key: "wiki_lang".into(),
            label: "Wikisource Language".into(),
            field_type: "select".into(),
            default_value: Value::String("en".into()),
            description: "Wikisource subdomain language code.".into(),
            options: Some(vec![
                "en".into(),
                "pl".into(),
                "fr".into(),
                "de".into(),
                "es".into(),
                "it".into(),
                "pt".into(),
                "ru".into(),
                "ja".into(),
                "zh".into(),
            ]),
        }]
    }

    async fn discover(
        &self,
        query: &DiscoveryQuery,
        config: &HashMap<String, Value>,
    ) -> AppResult<(Vec<NovelSummary>, bool, Option<u64>)> {
        if matches!(query.status.as_deref(), Some("ongoing")) {
            return Ok((Vec::new(), false, Some(0)));
        }

        let cfg_lang = Self::wiki_lang(config);
        let lang = query
            .language
            .as_deref()
            .map(str::trim)
            .filter(|s| {
                !s.is_empty()
                    && !s.eq_ignore_ascii_case("all")
                    && s.len() <= 8
                    && s.chars().all(|c| c.is_ascii_alphabetic())
            })
            .map(str::to_ascii_lowercase)
            .unwrap_or_else(|| cfg_lang.clone());
        let genre_label = query
            .genre
            .as_deref()
            .filter(|g| !g.trim().is_empty() && *g != "All")
            .unwrap_or("Classic Literature");
        let primary_cat = Self::category_for_lang_and_genre(&lang, query.genre.as_deref());

        let mut pool = self
            .fetch_category_items(&lang, primary_cat, genre_label)
            .await?;

        let q_str = query.query.as_deref().map(str::trim).unwrap_or("");
        if !q_str.is_empty() && query.genre.as_deref().unwrap_or("All") == "All" {
            let mut seen: HashSet<String> = pool.iter().map(|i| i.external_id.clone()).collect();
            for cat in Self::extra_search_categories(&lang) {
                if *cat == primary_cat {
                    continue;
                }
                if let Ok(extra) = self.fetch_category_items(&lang, cat, "Fiction").await {
                    for item in extra {
                        if seen.insert(item.external_id.clone()) {
                            pool.push(item);
                        }
                    }
                }
            }
        }

        if !q_str.is_empty() {
            let tokens: Vec<String> = q_str
                .to_ascii_lowercase()
                .split_whitespace()
                .map(ToString::to_string)
                .collect();
            pool.retain(|item| {
                let t = item.title.to_ascii_lowercase();
                tokens.iter().all(|tok| t.contains(tok))
            });
        }

        if query.sort.as_deref() == Some("title") {
            pool.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
        }

        let limit = query.limit.clamp(1, 30) as usize;
        let page = query.page.max(1) as usize;
        let offset = (page - 1) * limit;
        let total = pool.len() as u64;
        let has_more = offset + limit < pool.len();

        let items = pool
            .into_iter()
            .skip(offset)
            .take(limit)
            .map(|entry| {
                let ext = if lang == cfg_lang {
                    entry.external_id
                } else {
                    format!("{lang}|{}", entry.external_id)
                };
                NovelSummary {
                    id: make_novel_id(self.id(), &ext),
                    source_id: self.id().to_string(),
                    external_id: ext,
                    title: entry.title,
                    author: "Wikisource Archive".to_string(),
                    description: "Public domain work hosted on Wikisource.".to_string(),
                    cover_url: None,
                    local_cover_path: None,
                    status: NovelStatus::Completed,
                    genres: vec![entry.genre],
                    rating: None,
                    popularity: 0,
                    chapter_count: None,
                    last_updated: None,
                    language: Some(lang.clone()),
                }
            })
            .collect();

        Ok((items, has_more, Some(total)))
    }

    async fn get_novel(
        &self,
        external_id: &str,
        config: &HashMap<String, Value>,
    ) -> AppResult<NovelRecord> {
        let (lang, slug) = Self::resolve_lang_and_title(config, external_id);
        let url = self.page_url(config, external_id)?;
        let html = self.http.get_text(&url).await?;
        Ok(self.parse_novel_page(external_id, &lang, slug, &html))
    }

    async fn get_chapters(
        &self,
        external_id: &str,
        config: &HashMap<String, Value>,
    ) -> AppResult<Vec<Chapter>> {
        let url = self.page_url(config, external_id)?;
        let html = self.http.get_text(&url).await?;
        let base_url = Url::parse(&url)?;
        let doc = Html::parse_document(&html);
        let prose_sel =
            Selector::parse("#mw-content-text .mw-parser-output a[href], .mw-parser-output a[href], body a[href]")
                .unwrap();

        let (lang, root_slug) = Self::resolve_lang_and_title(config, external_id);
        let has_lang_prefix = external_id.contains('|');
        let novel_id = make_novel_id(self.id(), external_id);
        let normalized_root = root_slug.replace('_', " ");
        let prefix_space = format!("{normalized_root}/");

        let mut seen = HashSet::new();
        let mut subpages: Vec<(String, String)> = Vec::new();

        for a in doc.select(&prose_sel) {
            let Some(href) = a.value().attr("href") else {
                continue;
            };
            if href.contains('?') || href.starts_with('#') {
                continue;
            }
            let Ok(abs) = base_url.join(href) else {
                continue;
            };
            let Some(decoded_title) = Self::slug_from_wiki_url(abs.as_str()) else {
                continue;
            };
            if decoded_title.starts_with(&prefix_space) && seen.insert(decoded_title.clone()) {
                subpages.push((decoded_title, abs.to_string()));
            }
        }

        if subpages.is_empty() {
            return Ok(vec![Chapter {
                id: make_chapter_id(&novel_id, external_id),
                novel_id,
                external_id: external_id.to_string(),
                chapter_number: 1.0,
                volume: None,
                title: normalized_root,
                url: Some(url),
                word_count: 0,
                published_at: None,
                is_downloaded: false,
                local_path: None,
                size_bytes: 0,
            }]);
        }

        subpages.sort_by_key(|(full, _)| {
            let sub = full.strip_prefix(&prefix_space).unwrap_or(full);
            Self::natural_chapter_rank(sub)
        });

        Ok(subpages
            .into_iter()
            .enumerate()
            .map(|(i, (full_title, ch_url))| {
                let ch_title = full_title
                    .strip_prefix(&prefix_space)
                    .unwrap_or(&full_title)
                    .replace('_', " ");
                let ch_ext = if has_lang_prefix {
                    format!("{lang}|{full_title}")
                } else {
                    full_title
                };
                Chapter {
                    id: make_chapter_id(&novel_id, &ch_ext),
                    novel_id: novel_id.clone(),
                    external_id: ch_ext,
                    chapter_number: (i + 1) as f64,
                    volume: None,
                    title: ch_title,
                    url: Some(ch_url),
                    word_count: 0,
                    published_at: None,
                    is_downloaded: false,
                    local_path: None,
                    size_bytes: 0,
                }
            })
            .collect())
    }

    async fn get_translations(
        &self,
        external_id: &str,
        config: &HashMap<String, Value>,
    ) -> AppResult<Vec<NovelTranslation>> {
        let (lang, _) = Self::resolve_lang_and_title(config, external_id);
        let url = self.page_url(config, external_id)?;
        let html = self.http.get_text(&url).await?;
        Ok(self.extract_translations(external_id, &lang, &html))
    }

    async fn get_chapter_content(
        &self,
        _external_novel_id: &str,
        external_chapter_id: &str,
        chapter_url: Option<&str>,
        config: &HashMap<String, Value>,
    ) -> AppResult<(String, Vec<String>, i64)> {
        let url = match chapter_url {
            Some(u) if !u.trim().is_empty() => u.to_string(),
            _ => self.page_url(config, external_chapter_id)?,
        };
        let html = self.http.get_text(&url).await?;
        Ok(Self::clean_wikisource_html(&html))
    }

    async fn health_check(&self, config: &HashMap<String, Value>) -> AppResult<u64> {
        let lang = Self::wiki_lang(config);
        let cat = Self::category_for_lang_and_genre(&lang, None);
        let probe = self.page_url(config, cat)?;
        self.http.probe_latency(&probe).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[test]
    fn test_strict_roman_numeral_validation() {
        assert_eq!(parse_roman("I"), Some(1));
        assert_eq!(parse_roman("iv"), Some(4));
        assert_eq!(parse_roman("XIV"), Some(14));
        assert_eq!(parse_roman("XLII"), Some(42));
        // English words composed of Roman letters must NOT parse
        assert_eq!(parse_roman("mild"), None);
        assert_eq!(parse_roman("civil"), None);
        assert_eq!(parse_roman("dim"), None);
        assert_eq!(parse_roman("did"), None);
        assert_eq!(parse_roman("ill"), None);
    }

    #[test]
    fn test_wikisource_dom_cleanup_removes_noexport_and_pagenum() {
        let html = r#"
        <div id="mw-content-text"><div class="mw-parser-output">
          <div class="ws-header ws-noexport" data-x="a > b"><p>Header Navigation</p></div>
          <p>First sentence.<span class="pagenum ws-pagenum">[p. 12]</span> Second sentence.<span class="mw-editsection">[edit]</span></p>
          <p>Final <em>italic</em> line.</p>
        </div></div>"#;
        let (out_html, paras, wc) = WikisourceSource::clean_wikisource_html(html);
        assert_eq!(
            paras,
            vec!["First sentence. Second sentence.", "Final italic line."]
        );
        assert!(!out_html.contains("Header Navigation"));
        assert!(!out_html.contains("[p. 12]"));
        assert!(!out_html.contains("[edit]"));
        assert!(out_html.contains("<em>italic</em>"));
        assert_eq!(wc, 7);
    }

    #[tokio::test]
    async fn test_wikisource_wiki_pages_respect_robots_and_order_chapters() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base = format!("http://{addr}");

        let srv = tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let mut buf = [0u8; 2048];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let path = req
                        .lines()
                        .next()
                        .and_then(|l| l.split_whitespace().nth(1))
                        .unwrap_or("/");

                    let (status, ctype, body) = if path == "/robots.txt" {
                        (
                            "200 OK",
                            "text/plain",
                            "User-agent: *\nDisallow: /w/\nDisallow: /api/\nDisallow: /wiki/Special:\n".to_string(),
                        )
                    } else if path == "/wiki/Category:Novels" {
                        (
                            "200 OK",
                            "text/html",
                            r#"<div id="mw-pages"><div class="mw-category"><ul>
                              <li><a href="/wiki/Sample_Novel">Sample Novel</a></li>
                            </ul></div></div>"#.to_string(),
                        )
                    } else if path == "/wiki/Sample_Novel" {
                        (
                            "200 OK",
                            "text/html",
                            r#"<html><body>
                              <div id="ws-data" class="ws-noexport"><span id="ws-title">Sample Novel</span><span id="ws-author">Jane Writer</span></div>
                              <ul><li class="interlanguage-link"><a href="https://pl.wikisource.org/wiki/Przyk%C5%82adowa_Powie%C5%9B%C4%87" lang="pl" data-language-autonym="Polski">Polski</a></li></ul>
                              <div id="mw-content-text"><div class="mw-parser-output">
                                <p>Classic tale.</p>
                                <ul>
                                  <li><a href="/wiki/Sample_Novel/Chapter_X">Chapter X</a></li>
                                  <li><a href="/wiki/Sample_Novel/Chapter_II">Chapter II</a></li>
                                  <li><a href="/wiki/Sample_Novel/Chapter_I">Chapter I</a></li>
                                </ul>
                              </div></div>
                            </body></html>"#.to_string(),
                        )
                    } else if path.starts_with("/wiki/Sample_Novel/Chapter_") {
                        (
                            "200 OK",
                            "text/html",
                            r#"<div id="mw-content-text"><div class="mw-parser-output"><p>Chapter body prose.</p></div></div>"#.to_string(),
                        )
                    } else {
                        ("404 Not Found", "text/plain", "Not Found".to_string())
                    };

                    let resp = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = sock.write_all(resp.as_bytes()).await;
                });
            }
        });

        let http = SourceHttpClient::new().unwrap();
        let src = WikisourceSource::with_base_url(http, &base);

        let (items, _, _) = src
            .discover(
                &DiscoveryQuery {
                    source_id: None,
                    query: None,
                    genre: None,
                    sort: None,
                    status: None,
                    language: None,
                    page: 1,
                    limit: 10,
                },
                &HashMap::new(),
            )
            .await
            .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Sample Novel");

        let novel = src.get_novel("Sample Novel", &HashMap::new()).await.unwrap();
        assert_eq!(novel.author, "Jane Writer");

        let chs = src.get_chapters("Sample Novel", &HashMap::new()).await.unwrap();
        assert_eq!(
            chs.iter().map(|c| c.title.as_str()).collect::<Vec<_>>(),
            vec!["Chapter I", "Chapter II", "Chapter X"]
        );

        let (_, paras, _) = src
            .get_chapter_content("Sample Novel", &chs[0].external_id, chs[0].url.as_deref(), &HashMap::new())
            .await
            .unwrap();
        assert_eq!(paras, vec!["Chapter body prose."]);

        let translations = src.get_translations("Sample Novel", &HashMap::new()).await.unwrap();
        assert_eq!(translations.len(), 2);
        assert_eq!(translations[0].language_code, "en");
        assert_eq!(translations[1].language_code, "pl");
        assert_eq!(translations[1].language_label, "Polski");
        assert!(translations[1].external_id.starts_with("pl|"));

        assert!(src.health_check(&HashMap::new()).await.is_ok());

        srv.abort();
    }
}
