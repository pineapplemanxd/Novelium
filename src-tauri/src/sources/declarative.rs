use std::collections::{HashMap, HashSet};

use async_trait::async_trait;
use chrono::Utc;
use scraper::{ElementRef, Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

use crate::error::{AppError, AppResult};
use crate::formats::epub::parse_epub_bytes;
use crate::formats::text_html::{clean_html_fragment_excluding, parse_html_bytes};
use crate::formats::ParsedBook;
use crate::models::{
    is_placeholder_author, Chapter, DiscoveryQuery, NovelRecord, NovelStatus, NovelSummary,
    SourceCapabilities,
};
use crate::net::SourceHttpClient;
use crate::sources::archive_org::normalize_ia_lang_str;
use crate::sources::{make_chapter_id, make_novel_id, CachedParsedBook, NovelSource, ParsedBookCache};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeclarativeKind {
    Opds,
    Html,
    Auto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeclarativeSourceManifest {
    pub id: String,
    pub name: String,
    pub kind: DeclarativeKind,
    pub base_url: String,
    pub description: String,
    #[serde(default)]
    pub genres: Vec<String>,
    // OPDS options
    #[serde(default)]
    pub feed_url: Option<String>,
    #[serde(default)]
    pub search_url_template: Option<String>,
    // HTML CSS selector options
    #[serde(default)]
    pub browse_url_template: Option<String>,
    #[serde(default)]
    pub novel_list_selector: Option<String>,
    #[serde(default)]
    pub title_selector: Option<String>,
    #[serde(default)]
    pub author_selector: Option<String>,
    #[serde(default)]
    pub cover_selector: Option<String>,
    #[serde(default)]
    pub link_selector: Option<String>,
    #[serde(default)]
    pub description_selector: Option<String>,
    #[serde(default)]
    pub chapter_list_selector: Option<String>,
    #[serde(default)]
    pub chapter_title_selector: Option<String>,
    #[serde(default)]
    pub chapter_link_selector: Option<String>,
    #[serde(default)]
    pub content_selector: Option<String>,
    #[serde(default)]
    pub remove_selectors: Vec<String>,
    #[serde(default)]
    pub language: Option<String>,
}

impl DeclarativeSourceManifest {
    /// Builds a minimal Auto-parsed manifest from a single website or feed URL.
    pub fn from_url(raw_url: &str) -> Self {
        let mut m = Self {
            id: String::new(),
            name: String::new(),
            kind: DeclarativeKind::Auto,
            base_url: raw_url.trim().to_string(),
            description: String::new(),
            genres: vec!["All".into()],
            feed_url: None,
            search_url_template: None,
            browse_url_template: None,
            novel_list_selector: None,
            title_selector: None,
            author_selector: None,
            cover_selector: None,
            link_selector: None,
            description_selector: None,
            chapter_list_selector: None,
            chapter_title_selector: None,
            chapter_link_selector: None,
            content_selector: None,
            remove_selectors: Vec::new(),
            language: None,
        };
        let _ = m.fill_defaults_from_url();
        m
    }

    /// Validates `base_url` and fills missing `id`, `name`, and `description` from the URL.
    pub fn fill_defaults_from_url(&mut self) -> AppResult<()> {
        let base = self.base_url.trim().to_string();
        let parsed = Url::parse(&base)
            .map_err(|e| AppError::InvalidInput(format!("Invalid source URL '{base}': {e}")))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(AppError::InvalidInput(
                "Source URL must start with http:// or https://".into(),
            ));
        }
        let host = parsed
            .host_str()
            .filter(|h| !h.is_empty())
            .ok_or_else(|| AppError::InvalidInput("Source URL is missing a host name".into()))?;
        let clean_host = host.strip_prefix("www.").unwrap_or(host);
        self.base_url = base;

        if self.id.trim().is_empty() {
            let path_tail = parsed.path().trim_matches('/');
            let raw = if path_tail.is_empty() {
                format!("auto_{clean_host}")
            } else {
                format!("auto_{clean_host}_{path_tail}")
            };
            let mut slug = String::with_capacity(raw.len());
            let mut prev_underscore = false;
            for ch in raw.chars() {
                if ch.is_ascii_alphanumeric() || ch == '-' {
                    slug.push(ch.to_ascii_lowercase());
                    prev_underscore = false;
                } else if !prev_underscore {
                    slug.push('_');
                    prev_underscore = true;
                }
                if slug.len() >= 48 {
                    break;
                }
            }
            self.id = slug.trim_matches('_').to_string();
        }
        if self.name.trim().is_empty() {
            let path_tail = parsed.path().trim_matches('/');
            self.name = if path_tail.is_empty() {
                clean_host.to_string()
            } else {
                format!("{clean_host}/{path_tail}")
            };
        }
        if self.description.trim().is_empty() {
            self.description = format!("Auto-parsed source ({})", self.base_url);
        }
        self.normalize_optional_fields();
        Ok(())
    }

    /// Converts blank/whitespace-only optional selectors & templates to `None`.
    pub fn normalize_optional_fields(&mut self) {
        for field in [
            &mut self.feed_url,
            &mut self.search_url_template,
            &mut self.browse_url_template,
            &mut self.novel_list_selector,
            &mut self.title_selector,
            &mut self.author_selector,
            &mut self.cover_selector,
            &mut self.link_selector,
            &mut self.description_selector,
            &mut self.chapter_list_selector,
            &mut self.chapter_title_selector,
            &mut self.chapter_link_selector,
            &mut self.content_selector,
            &mut self.language,
        ] {
            if let Some(val) = field {
                let trimmed = val.trim();
                if trimmed.is_empty() {
                    *field = None;
                } else if trimmed.len() != val.len() {
                    *field = Some(trimmed.to_string());
                }
            }
        }
        self.remove_selectors.retain(|s| !s.trim().is_empty());
    }
}

pub struct DeclarativeSource {
    manifest: DeclarativeSourceManifest,
    http: SourceHttpClient,
    cache: ParsedBookCache,
    is_builtin_preset: bool,
}

impl DeclarativeSource {
    pub fn new(
        mut manifest: DeclarativeSourceManifest,
        http: SourceHttpClient,
        cache: ParsedBookCache,
    ) -> Self {
        manifest.normalize_optional_fields();
        Self {
            manifest,
            http,
            cache,
            is_builtin_preset: false,
        }
    }

    pub fn new_builtin(
        mut manifest: DeclarativeSourceManifest,
        http: SourceHttpClient,
        cache: ParsedBookCache,
    ) -> Self {
        manifest.normalize_optional_fields();
        Self {
            manifest,
            http,
            cache,
            is_builtin_preset: true,
        }
    }

    fn extract_cover_attr(&self, base: &str, el: scraper::ElementRef<'_>) -> Option<String> {
        let data_src = el
            .value()
            .attr("data-src")
            .or_else(|| el.value().attr("data-original"));
        let raw_src = el.value().attr("src");
        let chosen = match (data_src, raw_src) {
            (Some(ds), Some(src))
                if src.contains("placeholder") || src.starts_with("data:") =>
            {
                Some(ds)
            }
            (ds, Some(src)) if !src.trim().is_empty() => Some(src).or(ds),
            (ds, _) => ds,
        }?;
        let trimmed = chosen.trim();
        if trimmed.is_empty() || trimmed.starts_with("data:") {
            None
        } else {
            Some(self.resolve_url(base, trimmed))
        }
    }

    fn resolve_url(&self, base: &str, rel: &str) -> String {
        if rel.starts_with("http://") || rel.starts_with("https://") {
            return rel.to_string();
        }
        Url::parse(base)
            .and_then(|b| b.join(rel))
            .map(|u| u.to_string())
            .unwrap_or_else(|_| rel.to_string())
    }

    fn apply_template(&self, tpl: &str, query: &DiscoveryQuery) -> String {
        let q_encoded: String = url::form_urlencoded::byte_serialize(
            query.query.as_deref().unwrap_or("").trim().as_bytes(),
        )
        .collect();
        let g_encoded: String = url::form_urlencoded::byte_serialize(
            query.genre.as_deref().unwrap_or("").trim().as_bytes(),
        )
        .collect();
        let page1 = query.page.max(1);
        let page0 = page1.saturating_sub(1);
        tpl.replace("{query}", &q_encoded)
            .replace("{genre}", &g_encoded)
            .replace("{page0}", &page0.to_string())
            .replace("{page}", &page1.to_string())
    }

    fn parse_opds_feed(&self, feed_url: &str, xml: &str) -> AppResult<(Vec<NovelSummary>, bool)> {
        let doc = roxmltree::Document::parse(xml)?;
        let mut items = Vec::new();
        let mut has_next = false;

        for node in doc.descendants().filter(|n| n.is_element() && n.tag_name().name() == "link") {
            if node.parent().map(|p| p.tag_name().name() == "feed").unwrap_or(false)
                && node.attribute("rel") == Some("next")
            {
                has_next = true;
            }
        }

        for entry in doc.descendants().filter(|n| n.is_element() && n.tag_name().name() == "entry") {
            let title = entry
                .children()
                .find(|c| c.tag_name().name() == "title")
                .and_then(|c| c.text())
                .map(str::trim)
                .unwrap_or("")
                .to_string();
            if title.is_empty() {
                continue;
            }
            let author = entry
                .children()
                .find(|c| c.tag_name().name() == "author")
                .and_then(|a| a.children().find(|c| c.tag_name().name() == "name"))
                .and_then(|n| n.text())
                .map(str::trim)
                .unwrap_or("Unknown Author")
                .to_string();
            let description = entry
                .children()
                .find(|c| c.tag_name().name() == "summary" || c.tag_name().name() == "content")
                .and_then(|c| c.text())
                .map(str::trim)
                .unwrap_or("")
                .to_string();

            let mut genres = Vec::new();
            for cat in entry.children().filter(|c| c.tag_name().name() == "category") {
                if let Some(term) = cat.attribute("label").or_else(|| cat.attribute("term")) {
                    if !term.trim().is_empty() {
                        genres.push(term.trim().to_string());
                    }
                }
            }

            let mut cover_url = None;
            let mut acquisition_url = None;
            for link in entry.children().filter(|c| c.tag_name().name() == "link") {
                let rel = link.attribute("rel").unwrap_or("");
                let href = link.attribute("href").unwrap_or("");
                let mime = link.attribute("type").unwrap_or("");
                if href.is_empty() {
                    continue;
                }
                if rel.contains("image") || rel.contains("thumbnail") {
                    if cover_url.is_none() || rel.ends_with("/image") {
                        cover_url = Some(self.resolve_url(feed_url, href));
                    }
                } else if (rel.contains("acquisition") || mime.contains("epub"))
                    && (acquisition_url.is_none() || mime.contains("epub"))
                {
                    acquisition_url = Some(self.resolve_url(feed_url, href));
                }
            }

            if let Some(acq) = acquisition_url {
                let external_id = acq;
                items.push(NovelSummary {
                    id: make_novel_id(self.id(), &external_id),
                    source_id: self.id().to_string(),
                    external_id,
                    title,
                    author,
                    description,
                    cover_url,
                    local_cover_path: None,
                    status: NovelStatus::Completed,
                    genres,
                    rating: None,
                    popularity: 0,
                    chapter_count: None,
                    last_updated: None,
                    language: None,
                });
            }
        }

        Ok((items, has_next))
    }

    fn parse_html_list(&self, page_url: &str, html: &str) -> AppResult<Vec<NovelSummary>> {
        let doc = Html::parse_document(html);
        let list_sel_str = self
            .manifest
            .novel_list_selector
            .as_deref()
            .ok_or_else(|| AppError::InvalidInput("Missing novel_list_selector in HTML source".into()))?;
        let list_sel = Selector::parse(list_sel_str)
            .map_err(|e| AppError::InvalidInput(format!("Invalid CSS selector '{list_sel_str}': {e}")))?;

        let title_sel = self
            .manifest
            .title_selector
            .as_deref()
            .and_then(|s| Selector::parse(s).ok());
        let author_sel = self
            .manifest
            .author_selector
            .as_deref()
            .and_then(|s| Selector::parse(s).ok());
        let cover_sel = self
            .manifest
            .cover_selector
            .as_deref()
            .and_then(|s| Selector::parse(s).ok());
        let link_sel = self
            .manifest
            .link_selector
            .as_deref()
            .and_then(|s| Selector::parse(s).ok());
        let desc_sel = self
            .manifest
            .description_selector
            .as_deref()
            .and_then(|s| Selector::parse(s).ok());

        let mut out = Vec::new();
        for card in doc.select(&list_sel) {
            let title = title_sel
                .as_ref()
                .and_then(|s| card.select(s).next())
                .map(|e| e.text().collect::<String>().split_whitespace().collect::<Vec<_>>().join(" "))
                .unwrap_or_default();
            if title.is_empty() {
                continue;
            }
            let href = link_sel
                .as_ref()
                .and_then(|s| card.select(s).next())
                .and_then(|e| e.value().attr("href"))
                .or_else(|| card.value().attr("href"));
            let Some(href) = href else { continue };
            let novel_url = self.resolve_url(page_url, href);

            let author = extract_html_card_author(&card, author_sel.as_ref())
                .unwrap_or_else(|| "Unknown Author".into());

            let cover_url = cover_sel
                .as_ref()
                .and_then(|s| card.select(s).next())
                .and_then(|e| self.extract_cover_attr(page_url, e));

            let description = desc_sel
                .as_ref()
                .and_then(|s| card.select(s).next())
                .map(|e| e.text().collect::<String>().trim().to_string())
                .unwrap_or_default();

            out.push(NovelSummary {
                id: make_novel_id(self.id(), &novel_url),
                source_id: self.id().to_string(),
                external_id: novel_url,
                title,
                author,
                description,
                cover_url,
                local_cover_path: None,
                status: NovelStatus::Unknown,
                genres: Vec::new(),
                rating: None,
                popularity: 0,
                chapter_count: None,
                last_updated: None,
                language: self.manifest.language.clone(),
            });
        }
        Ok(out)
    }

    fn parse_and_cache_book_bytes(
        &self,
        novel_id: &str,
        url: &str,
        bytes: &[u8],
    ) -> AppResult<(ParsedBook, CachedParsedBook)> {
        let parsed = if bytes.starts_with(b"PK") {
            parse_epub_bytes(bytes, url)?
        } else {
            parse_html_bytes(bytes, url)?
        };
        let cached = self.cache.put(novel_id, &parsed)?;
        Ok((parsed, cached))
    }

    async fn ensure_opds_parsed(&self, acquisition_url: &str) -> AppResult<CachedParsedBook> {
        let novel_id = make_novel_id(self.id(), acquisition_url);
        if let Some(cached) = self.cache.get(&novel_id) {
            if !cached.chapters.is_empty() {
                return Ok(cached);
            }
        }
        let lock = self.cache.novel_lock(&novel_id).await;
        let _guard = lock.lock().await;
        if let Some(cached) = self.cache.get(&novel_id) {
            if !cached.chapters.is_empty() {
                return Ok(cached);
            }
        }
        let bytes = self.http.get_bytes(acquisition_url).await?;
        Ok(self.parse_and_cache_book_bytes(&novel_id, acquisition_url, &bytes)?.1)
    }

    fn parse_rss_feed(&self, feed_url: &str, xml: &str) -> AppResult<Vec<NovelSummary>> {
        let doc = roxmltree::Document::parse(xml)?;
        let mut items = Vec::new();
        for item in doc.descendants().filter(|n| n.is_element() && n.tag_name().name() == "item") {
            let child_text = |names: &[&str]| -> Option<String> {
                item.children()
                    .find(|c| c.is_element() && names.contains(&c.tag_name().name()))
                    .and_then(|c| c.text())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
            };
            let Some(title) = child_text(&["title"]) else {
                continue;
            };
            let link = child_text(&["link"]).or_else(|| {
                item.children()
                    .find(|c| c.tag_name().name() == "link")
                    .and_then(|l| l.attribute("href"))
                    .map(str::to_string)
            });
            let Some(raw_link) = link else { continue };
            let external_id = self.resolve_url(feed_url, &raw_link);
            let author = child_text(&["creator", "author"]).unwrap_or_else(|| "Unknown Author".into());
            let description = child_text(&["description", "encoded"]).unwrap_or_default();
            items.push(NovelSummary {
                id: make_novel_id(self.id(), &external_id),
                source_id: self.id().to_string(),
                external_id,
                title,
                author,
                description,
                cover_url: None,
                local_cover_path: None,
                status: NovelStatus::Unknown,
                genres: Vec::new(),
                rating: None,
                popularity: 0,
                chapter_count: None,
                last_updated: None,
                language: self.manifest.language.clone(),
            });
        }
        Ok(items)
    }

    fn auto_detect_search_url(&self, page_url: &str, html: &str, q: &str) -> Option<String> {
        let doc = Html::parse_document(html);
        let form_sel = Selector::parse("form").ok()?;
        let input_sel = Selector::parse("input[name]").ok()?;
        let base_parsed = Url::parse(page_url).ok();

        for form in doc.select(&form_sel) {
            let method = form.value().attr("method").unwrap_or("get").to_ascii_lowercase();
            if method != "get" {
                continue;
            }
            let input_name = form
                .select(&input_sel)
                .filter_map(|inp| {
                    let typ = inp.value().attr("type").unwrap_or("text").to_ascii_lowercase();
                    let name = inp.value().attr("name")?;
                    if matches!(typ.as_str(), "search" | "text" | "")
                        && matches!(
                            name.to_ascii_lowercase().as_str(),
                            "q" | "query" | "s" | "search" | "keyword" | "keywords" | "term"
                        )
                    {
                        Some(name.to_string())
                    } else {
                        None
                    }
                })
                .next()
                .or_else(|| {
                    form.select(&input_sel)
                        .find(|inp| {
                            inp.value()
                                .attr("type")
                                .is_some_and(|t| t.eq_ignore_ascii_case("search"))
                        })
                        .and_then(|inp| inp.value().attr("name").map(str::to_string))
                });
            let Some(param) = input_name else { continue };
            let action = form.value().attr("action").unwrap_or(page_url);
            let action_abs = self.resolve_url(page_url, action);
            if let (Some(bp), Ok(mut u)) = (&base_parsed, Url::parse(&action_abs)) {
                if u.host_str() == bp.host_str() {
                    u.query_pairs_mut().append_pair(&param, q);
                    return Some(u.to_string());
                }
            }
        }
        None
    }

    fn auto_parse_html_list(&self, page_url: &str, html: &str) -> (Vec<NovelSummary>, bool, Option<String>) {
        let doc = Html::parse_document(html);
        let html_lang = extract_html_lang(&doc);
        let effective_lang = self.manifest.language.clone().or(html_lang.clone());
        let page_base = Url::parse(page_url).ok();

        let is_valid_card_link = |href: &str, text: &str| -> bool {
            let h = href.trim();
            if h.is_empty() || h.starts_with('#') || h.starts_with("javascript:") || h.starts_with("mailto:") {
                return false;
            }
            let tl = text.trim();
            if tl.len() < 2 || tl.len() > 160 {
                return false;
            }
            let low = h.to_ascii_lowercase();
            let t_low = tl.to_ascii_lowercase();
            let skip_href = [
                "/login", "/register", "/signup", "/privacy", "/terms", "/about",
                "/contact", "/tag/", "/genre/", "/author/", "/search?", "?page=",
            ];
            let skip_text = ["next", "prev", "previous", "home", "login", "sign in", "register"];
            !skip_href.iter().any(|p| low.contains(p))
                && !skip_text.contains(&t_low.as_str())
                && page_base
                    .as_ref()
                    .and_then(|b| b.join(h).ok())
                    .map(|u| page_base.as_ref().is_none_or(|b| u.host_str() == b.host_str()))
                    .unwrap_or(true)
        };

        let link_sel = Selector::parse("a[href]").unwrap();
        let heading_sel = Selector::parse("h1, h2, h3, h4, .title, .book-title, .post-title, .novel-title").unwrap();
        let desc_sel = Selector::parse(".desc, .description, .summary, p").unwrap();
        let img_sel = Selector::parse("img").unwrap();

        let card_selectors = [
            "li.novel-item",
            ".page-item-detail",
            ".list-novel .row",
            ".novel-list li",
            ".book-list li",
            ".search-item",
            "article",
            ".card",
            ".grid > div",
            "main li",
        ];

        let mut best_items: Vec<NovelSummary> = Vec::new();
        for sel_str in card_selectors {
            let Ok(sel) = Selector::parse(sel_str) else { continue };
            let mut items = Vec::new();
            let mut seen = HashSet::new();
            for card in doc.select(&sel) {
                let Some(a) = card.select(&link_sel).find(|a| {
                    let href = a.value().attr("href").unwrap_or("");
                    let txt = collapse_ws(&a.text().collect::<String>());
                    !href.is_empty() && !href.starts_with('#') && (!txt.is_empty() || card.select(&heading_sel).next().is_some())
                }) else {
                    continue;
                };
                let href = a.value().attr("href").unwrap_or("");
                let heading_title = card
                    .select(&heading_sel)
                    .next()
                    .map(|e| collapse_ws(&e.text().collect::<String>()))
                    .unwrap_or_default();
                let link_title = collapse_ws(&a.text().collect::<String>());
                let title = if !heading_title.is_empty() {
                    heading_title
                } else {
                    link_title
                };
                if !is_valid_card_link(href, &title) {
                    continue;
                }
                let novel_url = self.resolve_url(page_url, href);
                if !seen.insert(novel_url.clone()) {
                    continue;
                }
                let author = extract_html_card_author(&card, None)
                    .unwrap_or_else(|| "Unknown Author".into());
                let cover_url = card
                    .select(&img_sel)
                    .next()
                    .and_then(|e| self.extract_cover_attr(page_url, e));
                let description = card
                    .select(&desc_sel)
                    .next()
                    .map(|e| collapse_ws(&e.text().collect::<String>()))
                    .unwrap_or_default();
                items.push(NovelSummary {
                    id: make_novel_id(self.id(), &novel_url),
                    source_id: self.id().to_string(),
                    external_id: novel_url,
                    title,
                    author,
                    description,
                    cover_url,
                    local_cover_path: None,
                    status: NovelStatus::Unknown,
                    genres: Vec::new(),
                    rating: None,
                    popularity: 0,
                    chapter_count: None,
                    last_updated: None,
                    language: effective_lang.clone(),
                });
            }
            if items.len() >= 2 {
                best_items = items;
                break;
            } else if best_items.is_empty() && !items.is_empty() {
                best_items = items;
            }
        }

        if best_items.is_empty() {
            let container_sel = Selector::parse("main a[href], #content a[href], .content a[href], body a[href]").unwrap();
            let mut seen = HashSet::new();
            for a in doc.select(&container_sel) {
                let in_chrome = a.ancestors().any(|anc| {
                    anc.value()
                        .as_element()
                        .is_some_and(|el| matches!(el.name(), "nav" | "header" | "footer" | "aside"))
                });
                if in_chrome {
                    continue;
                }
                let Some(href) = a.value().attr("href") else { continue };
                let title = collapse_ws(&a.text().collect::<String>());
                if !is_valid_card_link(href, &title) {
                    continue;
                }
                let novel_url = self.resolve_url(page_url, href);
                if novel_url.trim_end_matches('/') == page_url.trim_end_matches('/')
                    || !seen.insert(novel_url.clone())
                {
                    continue;
                }
                best_items.push(NovelSummary {
                    id: make_novel_id(self.id(), &novel_url),
                    source_id: self.id().to_string(),
                    external_id: novel_url,
                    title,
                    author: "Unknown Author".into(),
                    description: String::new(),
                    cover_url: None,
                    local_cover_path: None,
                    status: NovelStatus::Unknown,
                    genres: Vec::new(),
                    rating: None,
                    popularity: 0,
                    chapter_count: None,
                    last_updated: None,
                    language: effective_lang.clone(),
                });
                if best_items.len() >= 60 {
                    break;
                }
            }
        }

        let has_next = Selector::parse("link[rel='next'], a[rel='next'], a.next, .pagination .next a")
            .ok()
            .is_some_and(|sel| doc.select(&sel).next().is_some());
        (best_items, has_next, html_lang)
    }

    fn auto_extract_prose(&self, html: &str) -> (String, Vec<String>, i64) {
        let doc = Html::parse_document(html);
        let custom_rm: Vec<&str> = self.manifest.remove_selectors.iter().map(String::as_str).collect();
        let default_rm = [
            "script", "style", "noscript", "nav", "header", "footer", "aside",
            ".ads", ".advertisement", ".comments", "#comments", ".share", ".social",
        ];
        let mut all_rm: Vec<&str> = default_rm.to_vec();
        all_rm.extend(custom_rm);

        if let Some(ref sel_str) = self.manifest.content_selector {
            if let Ok(sel) = Selector::parse(sel_str) {
                if let Some(el) = doc.select(&sel).next() {
                    return clean_html_fragment_excluding(&el.inner_html(), &all_rm);
                }
            }
        }

        let candidates = [
            "#chapter-content",
            ".chapter-content",
            "#chr-content",
            ".reading-content .text-left",
            ".reading-content",
            ".chapter-inner",
            ".entry-content",
            ".post-content",
            "article",
            "main",
            "body",
        ];
        let p_sel = Selector::parse("p").unwrap();
        let mut best_html: Option<String> = None;
        let mut best_score = 0usize;

        for sel_str in candidates {
            let Ok(sel) = Selector::parse(sel_str) else { continue };
            for el in doc.select(&sel) {
                let p_len: usize = el
                    .select(&p_sel)
                    .map(|p| p.text().collect::<String>().trim().len())
                    .sum();
                let score = if p_len > 0 {
                    p_len
                } else {
                    el.text().collect::<String>().trim().len()
                };
                if score > best_score {
                    best_score = score;
                    best_html = Some(el.inner_html());
                }
            }
            if best_score >= 200 && sel_str != "article" && sel_str != "main" && sel_str != "body" {
                break;
            }
        }

        clean_html_fragment_excluding(best_html.as_deref().unwrap_or(html), &all_rm)
    }
}

fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn extract_html_lang(doc: &Html) -> Option<String> {
    let sel = Selector::parse("html[lang]").ok()?;
    let raw = doc.select(&sel).next()?.value().attr("lang")?;
    normalize_ia_lang_str(raw)
}

fn first_selector_text(doc: &Html, selectors: &[&str]) -> Option<String> {
    for sel_str in selectors {
        let Ok(sel) = Selector::parse(sel_str) else { continue };
        for el in doc.select(&sel) {
            let txt = collapse_ws(&el.text().collect::<String>());
            if !txt.is_empty() {
                return Some(txt);
            }
        }
    }
    None
}

fn first_meta_content(doc: &Html, selectors: &[&str]) -> Option<String> {
    for sel_str in selectors {
        let Ok(sel) = Selector::parse(sel_str) else { continue };
        if let Some(val) = doc
            .select(&sel)
            .next()
            .and_then(|e| e.value().attr("content"))
            .map(collapse_ws)
            .filter(|s| !s.is_empty())
        {
            return Some(val);
        }
    }
    None
}

fn sort_chapters_ascending(chapters: &mut [Chapter]) {
    if chapters.len() < 2 {
        return;
    }
    let first_num = extract_chapter_order_num(
        &chapters[0].title,
        chapters[0].url.as_deref().unwrap_or(""),
    );
    let last = chapters.len() - 1;
    let last_num = extract_chapter_order_num(
        &chapters[last].title,
        chapters[last].url.as_deref().unwrap_or(""),
    );
    if let (Some(a), Some(b)) = (first_num, last_num) {
        if a > b {
            chapters.reverse();
            for (idx, ch) in chapters.iter_mut().enumerate() {
                ch.chapter_number = (idx + 1) as f64;
            }
        }
    }
}

#[async_trait]
impl NovelSource for DeclarativeSource {
    fn id(&self) -> &str {
        &self.manifest.id
    }

    fn name(&self) -> &str {
        &self.manifest.name
    }

    fn kind(&self) -> &str {
        match self.manifest.kind {
            DeclarativeKind::Opds => "opds",
            DeclarativeKind::Html => "declarative_html",
            DeclarativeKind::Auto => "auto",
        }
    }

    fn base_url(&self) -> &str {
        &self.manifest.base_url
    }

    fn description(&self) -> &str {
        &self.manifest.description
    }

    fn is_custom(&self) -> bool {
        !self.is_builtin_preset
    }

    fn capabilities(&self) -> SourceCapabilities {
        SourceCapabilities {
            search: matches!(self.manifest.kind, DeclarativeKind::Auto)
                || self.manifest.search_url_template.is_some()
                || self.manifest.feed_url.is_some()
                || self.manifest.browse_url_template.is_some(),
            browse_popular: matches!(self.manifest.kind, DeclarativeKind::Auto)
                || self.manifest.feed_url.is_some()
                || self.manifest.browse_url_template.is_some(),
            browse_trending: false,
            browse_latest: false,
            browse_genre: !self.manifest.genres.is_empty(),
            filter_status: false,
            genres: self.manifest.genres.clone(),
        }
    }

    async fn discover(
        &self,
        query: &DiscoveryQuery,
        _config: &HashMap<String, Value>,
    ) -> AppResult<(Vec<NovelSummary>, bool, Option<u64>)> {
        let req_lang = query
            .language
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("all"));
        if let Some(req_lang) = req_lang {
            if let Some(ref src_lang) = self.manifest.language {
                if !src_lang.eq_ignore_ascii_case(req_lang) {
                    return Ok((Vec::new(), false, None));
                }
            } else if !matches!(self.manifest.kind, DeclarativeKind::Auto) {
                return Ok((Vec::new(), false, None));
            }
        }
        let trimmed_q = query.query.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let (target_url, used_tpl) = if trimmed_q.is_some() {
            if let Some(tpl) = &self.manifest.search_url_template {
                (self.apply_template(tpl, query), Some(tpl.as_str()))
            } else if let Some(tpl) = self
                .manifest
                .feed_url
                .as_deref()
                .or(self.manifest.browse_url_template.as_deref())
            {
                (self.apply_template(tpl, query), Some(tpl))
            } else if matches!(self.manifest.kind, DeclarativeKind::Auto) {
                (self.manifest.base_url.clone(), None)
            } else {
                return Err(AppError::Source {
                    source_id: self.id().into(),
                    message: "Search is not configured for this custom source".into(),
                });
            }
        } else if let Some(tpl) = self
            .manifest
            .browse_url_template
            .as_deref()
            .or(self.manifest.feed_url.as_deref())
        {
            (self.apply_template(tpl, query), Some(tpl))
        } else {
            (self.manifest.base_url.clone(), None)
        };

        let mut final_url = target_url;
        let mut body = self.http.get_text(&final_url).await?;
        let mut used_remote_search = self.manifest.search_url_template.is_some();

        // For Auto sources without an explicit search template, try any GET search form on the base page.
        if matches!(self.manifest.kind, DeclarativeKind::Auto) && !used_remote_search {
            if let Some(q) = trimmed_q {
                if let Some(search_url) = self.auto_detect_search_url(&final_url, &body, q) {
                    if let Ok(search_body) = self.http.get_text(&search_url).await {
                        final_url = search_url;
                        body = search_body;
                        used_remote_search = true;
                    }
                }
            }
        }

        let filter_local = |items: &mut Vec<NovelSummary>| {
            if let Some(q) = trimmed_q {
                if !used_remote_search {
                    let q_low = q.to_ascii_lowercase();
                    items.retain(|it| {
                        it.title.to_ascii_lowercase().contains(&q_low)
                            || it.author.to_ascii_lowercase().contains(&q_low)
                    });
                }
            }
        };

        match self.manifest.kind {
            DeclarativeKind::Opds => {
                let (mut items, has_next) = self.parse_opds_feed(&final_url, &body)?;
                filter_local(&mut items);
                Ok((items, has_next, None))
            }
            DeclarativeKind::Html => {
                let mut items = self.parse_html_list(&final_url, &body)?;
                filter_local(&mut items);
                let has_next = !items.is_empty()
                    && used_tpl.is_some_and(|tpl| tpl.contains("{page"));
                Ok((items, has_next, None))
            }
            DeclarativeKind::Auto => {
                let head = body.trim_start();
                let is_atom = (head.starts_with("<?xml") || head.starts_with("<feed"))
                    && body.contains("<entry");
                let is_rss = (head.starts_with("<?xml") || head.starts_with("<rss"))
                    && body.contains("<channel")
                    && body.contains("<item");

                let (mut items, mut has_next, detected_lang) = if is_atom {
                    let (it, next) = self.parse_opds_feed(&final_url, &body)?;
                    (it, next, self.manifest.language.clone())
                } else if is_rss {
                    (self.parse_rss_feed(&final_url, &body)?, false, self.manifest.language.clone())
                } else if self.manifest.novel_list_selector.is_some() {
                    let it = self.parse_html_list(&final_url, &body)?;
                    let next = !it.is_empty()
                        && used_tpl.is_some_and(|tpl| tpl.contains("{page"));
                    (it, next, self.manifest.language.clone())
                } else {
                    self.auto_parse_html_list(&final_url, &body)
                };

                if let Some(req_l) = req_lang {
                    if let Some(ref dl) = detected_lang {
                        if !dl.eq_ignore_ascii_case(req_l) {
                            return Ok((Vec::new(), false, None));
                        }
                    }
                }
                filter_local(&mut items);
                if used_tpl.is_some_and(|tpl| tpl.contains("{page")) && !items.is_empty() {
                    has_next = true;
                }
                Ok((items, has_next, None))
            }
        }
    }

    async fn get_novel(
        &self,
        external_id: &str,
        _config: &HashMap<String, Value>,
    ) -> AppResult<NovelRecord> {
        let now = Utc::now().to_rfc3339();
        let make_epub_record = |novel_id: String, parsed: ParsedBook| NovelRecord {
            id: novel_id,
            source_id: self.id().to_string(),
            external_id: external_id.to_string(),
            title: parsed.title,
            author_id: None,
            author: parsed.author,
            description: parsed.description,
            cover_url: None,
            local_cover_path: None,
            status: NovelStatus::Completed,
            genres: parsed.genres,
            tags: parsed.tags,
            rating: parsed.rating,
            popularity: 0,
            chapter_count: parsed.chapters.len() as i64,
            last_updated_remote: None,
            metadata: self
                .manifest
                .language
                .as_deref()
                .map(|lang| serde_json::json!({ "language": lang }))
                .unwrap_or_else(|| serde_json::json!({})),
            is_local: false,
            created_at: now.clone(),
            updated_at: now.clone(),
        };

        match self.manifest.kind {
            DeclarativeKind::Opds => {
                let bytes = self.http.get_bytes(external_id).await?;
                let novel_id = make_novel_id(self.id(), external_id);
                let parsed = self
                    .parse_and_cache_book_bytes(&novel_id, external_id, &bytes)?
                    .0;
                Ok(make_epub_record(novel_id, parsed))
            }
            DeclarativeKind::Html => {
                let html = self.http.get_text(external_id).await?;
                let doc = Html::parse_document(&html);
                let title = self
                    .manifest
                    .title_selector
                    .as_deref()
                    .and_then(|s| Selector::parse(s).ok())
                    .and_then(|sel| doc.select(&sel).next())
                    .map(|e| e.text().collect::<String>().trim().to_string())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| external_id.to_string());
                let author = extract_html_detail_author(
                    &doc,
                    self.manifest.author_selector.as_deref(),
                )
                .unwrap_or_else(|| "Unknown Author".into());
                let description = self
                    .manifest
                    .description_selector
                    .as_deref()
                    .and_then(|s| Selector::parse(s).ok())
                    .and_then(|sel| doc.select(&sel).next())
                    .map(|e| e.text().collect::<String>().trim().to_string())
                    .unwrap_or_default();
                let cover_url = self
                    .manifest
                    .cover_selector
                    .as_deref()
                    .and_then(|s| Selector::parse(s).ok())
                    .and_then(|sel| doc.select(&sel).next())
                    .and_then(|e| self.extract_cover_attr(external_id, e));

                Ok(NovelRecord {
                    id: make_novel_id(self.id(), external_id),
                    source_id: self.id().to_string(),
                    external_id: external_id.to_string(),
                    title,
                    author_id: None,
                    author,
                    description,
                    cover_url,
                    local_cover_path: None,
                    status: NovelStatus::Unknown,
                    genres: Vec::new(),
                    tags: vec![self.name().to_string()],
                    rating: None,
                    popularity: 0,
                    chapter_count: 0,
                    last_updated_remote: None,
                    metadata: self
                        .manifest
                        .language
                        .as_deref()
                        .map(|lang| serde_json::json!({ "language": lang }))
                        .unwrap_or_else(|| serde_json::json!({})),
                    is_local: false,
                    created_at: now.clone(),
                    updated_at: now,
                })
            }
            DeclarativeKind::Auto => {
                let bytes = self.http.get_bytes(external_id).await?;
                let novel_id = make_novel_id(self.id(), external_id);
                if bytes.starts_with(b"PK") {
                    let parsed = self
                        .parse_and_cache_book_bytes(&novel_id, external_id, &bytes)?
                        .0;
                    return Ok(make_epub_record(novel_id, parsed));
                }
                let html = String::from_utf8_lossy(&bytes).into_owned();
                let doc = Html::parse_document(&html);
                let title = self
                    .manifest
                    .title_selector
                    .as_deref()
                    .and_then(|s| Selector::parse(s).ok())
                    .and_then(|sel| doc.select(&sel).next())
                    .map(|e| collapse_ws(&e.text().collect::<String>()))
                    .filter(|s| !s.is_empty())
                    .or_else(|| {
                        first_selector_text(
                            &doc,
                            &["h1", ".novel-title", ".book-title", ".post-title", ".entry-title"],
                        )
                    })
                    .or_else(|| first_meta_content(&doc, &["meta[property='og:title']"]))
                    .or_else(|| first_selector_text(&doc, &["title"]))
                    .unwrap_or_else(|| external_id.to_string());
                let author = extract_html_detail_author(
                    &doc,
                    self.manifest.author_selector.as_deref(),
                )
                .unwrap_or_else(|| "Unknown Author".into());
                let description = self
                    .manifest
                    .description_selector
                    .as_deref()
                    .and_then(|s| Selector::parse(s).ok())
                    .and_then(|sel| doc.select(&sel).next())
                    .map(|e| collapse_ws(&e.text().collect::<String>()))
                    .filter(|s| !s.is_empty())
                    .or_else(|| {
                        first_selector_text(
                            &doc,
                            &[
                                ".summary__content",
                                ".summary .content",
                                ".synopsis",
                                ".description",
                                ".book-description",
                                "#description",
                            ],
                        )
                    })
                    .or_else(|| {
                        first_meta_content(
                            &doc,
                            &["meta[name='description']", "meta[property='og:description']"],
                        )
                    })
                    .unwrap_or_default();
                let cover_url = self
                    .manifest
                    .cover_selector
                    .as_deref()
                    .and_then(|s| Selector::parse(s).ok())
                    .and_then(|sel| doc.select(&sel).next())
                    .and_then(|e| self.extract_cover_attr(external_id, e))
                    .or_else(|| first_meta_content(&doc, &["meta[property='og:image']"]).map(|u| self.resolve_url(external_id, &u)))
                    .or_else(|| {
                        Selector::parse(".summary_image img, .book-cover img, .cover img, .novel-cover img, article img, main img")
                            .ok()
                            .and_then(|sel| doc.select(&sel).next())
                            .and_then(|e| self.extract_cover_attr(external_id, e))
                    });
                let lang = self
                    .manifest
                    .language
                    .clone()
                    .or_else(|| extract_html_lang(&doc));

                Ok(NovelRecord {
                    id: novel_id,
                    source_id: self.id().to_string(),
                    external_id: external_id.to_string(),
                    title,
                    author_id: None,
                    author,
                    description,
                    cover_url,
                    local_cover_path: None,
                    status: NovelStatus::Unknown,
                    genres: Vec::new(),
                    tags: vec![self.name().to_string()],
                    rating: None,
                    popularity: 0,
                    chapter_count: 0,
                    last_updated_remote: None,
                    metadata: lang
                        .map(|l| serde_json::json!({ "language": l }))
                        .unwrap_or_else(|| serde_json::json!({})),
                    is_local: false,
                    created_at: now.clone(),
                    updated_at: now,
                })
            }
        }
    }

    async fn get_chapters(
        &self,
        external_id: &str,
        _config: &HashMap<String, Value>,
    ) -> AppResult<Vec<Chapter>> {
        let novel_id = make_novel_id(self.id(), external_id);
        let chapters_from_cached = |cached: CachedParsedBook| -> Vec<Chapter> {
            cached
                .chapters
                .into_iter()
                .map(|c| Chapter {
                    id: make_chapter_id(&novel_id, &c.external_id),
                    novel_id: novel_id.clone(),
                    external_id: c.external_id,
                    chapter_number: c.chapter_number,
                    volume: None,
                    title: c.title,
                    url: None,
                    word_count: c.word_count,
                    published_at: None,
                    is_downloaded: false,
                    local_path: None,
                    size_bytes: 0,
                })
                .collect()
        };

        match self.manifest.kind {
            DeclarativeKind::Opds => {
                Ok(chapters_from_cached(self.ensure_opds_parsed(external_id).await?))
            }
            DeclarativeKind::Html => {
                let html = self.http.get_text(external_id).await?;
                let doc = Html::parse_document(&html);
                let ch_sel_str = self
                    .manifest
                    .chapter_list_selector
                    .as_deref()
                    .ok_or_else(|| {
                        AppError::InvalidInput("Missing chapter_list_selector in HTML source".into())
                    })?;
                let ch_sel = Selector::parse(ch_sel_str)
                    .map_err(|e| AppError::InvalidInput(format!("Invalid selector '{ch_sel_str}': {e}")))?;
                let title_sel = self
                    .manifest
                    .chapter_title_selector
                    .as_deref()
                    .and_then(|s| Selector::parse(s).ok());
                let link_sel = self
                    .manifest
                    .chapter_link_selector
                    .as_deref()
                    .and_then(|s| Selector::parse(s).ok());

                let mut chapters = Vec::new();
                let mut seen_urls = HashSet::new();
                for node in doc.select(&ch_sel) {
                    let href = link_sel
                        .as_ref()
                        .and_then(|s| node.select(s).next())
                        .and_then(|e| e.value().attr("href"))
                        .or_else(|| node.value().attr("href"));
                    let Some(href) = href else { continue };
                    let ch_url = self.resolve_url(external_id, href);
                    if !seen_urls.insert(ch_url.clone()) {
                        continue;
                    }
                    let num = (chapters.len() + 1) as f64;
                    let title = title_sel
                        .as_ref()
                        .and_then(|s| node.select(s).next())
                        .map(|e| e.text().collect::<String>().trim().to_string())
                        .unwrap_or_else(|| node.text().collect::<String>().trim().to_string());
                    let title = if title.is_empty() {
                        format!("Chapter {}", chapters.len() + 1)
                    } else {
                        title
                    };
                    chapters.push(Chapter {
                        id: make_chapter_id(&novel_id, &ch_url),
                        novel_id: novel_id.clone(),
                        external_id: ch_url.clone(),
                        chapter_number: num,
                        volume: None,
                        title,
                        url: Some(ch_url),
                        word_count: 0,
                        published_at: None,
                        is_downloaded: false,
                        local_path: None,
                        size_bytes: 0,
                    });
                }
                sort_chapters_ascending(&mut chapters);
                Ok(chapters)
            }
            DeclarativeKind::Auto => {
                if let Some(cached) = self.cache.get(&novel_id) {
                    if !cached.chapters.is_empty() {
                        return Ok(chapters_from_cached(cached));
                    }
                }
                let bytes = self.http.get_bytes(external_id).await?;
                if bytes.starts_with(b"PK") {
                    let cached = self
                        .parse_and_cache_book_bytes(&novel_id, external_id, &bytes)?
                        .1;
                    return Ok(chapters_from_cached(cached));
                }
                let html = String::from_utf8_lossy(&bytes).into_owned();
                let doc = Html::parse_document(&html);
                let base_parsed = Url::parse(external_id).ok();
                let novel_path = base_parsed
                    .as_ref()
                    .map(|u| u.path().trim_end_matches('/').to_string())
                    .unwrap_or_default();

                let mut chapters = Vec::new();
                let mut seen_urls = HashSet::new();
                let custom_sels: Vec<&str> = self
                    .manifest
                    .chapter_list_selector
                    .iter()
                    .map(String::as_str)
                    .chain([
                        "ul.chapter-list li a[href]",
                        "li.wp-manga-chapter a[href]",
                        "#list-chapter a[href]",
                        ".list-chapter a[href]",
                        ".chapter-list a[href]",
                        ".chapters a[href]",
                        "ol.toc a[href]",
                        ".toc a[href]",
                        "ul.chapters li a[href]",
                    ])
                    .collect();

                for sel_str in custom_sels {
                    let Ok(sel) = Selector::parse(sel_str) else { continue };
                    for a in doc.select(&sel) {
                        let Some(href) = a.value().attr("href") else { continue };
                        let ch_url = self.resolve_url(external_id, href);
                        if ch_url == external_id || !seen_urls.insert(ch_url.clone()) {
                            continue;
                        }
                        let title = collapse_ws(&a.text().collect::<String>());
                        let title = if title.is_empty() {
                            format!("Chapter {}", chapters.len() + 1)
                        } else {
                            title
                        };
                        chapters.push(Chapter {
                            id: make_chapter_id(&novel_id, &ch_url),
                            novel_id: novel_id.clone(),
                            external_id: ch_url.clone(),
                            chapter_number: (chapters.len() + 1) as f64,
                            volume: None,
                            title,
                            url: Some(ch_url),
                            word_count: 0,
                            published_at: None,
                            is_downloaded: false,
                            local_path: None,
                            size_bytes: 0,
                        });
                    }
                    if !chapters.is_empty() {
                        break;
                    }
                }

                if chapters.is_empty() {
                    if let Ok(a_sel) = Selector::parse("main a[href], #content a[href], article a[href], body a[href]") {
                        for a in doc.select(&a_sel) {
                            let Some(href) = a.value().attr("href") else { continue };
                            if href.starts_with('#') || href.starts_with("javascript:") {
                                continue;
                            }
                            let ch_url = self.resolve_url(external_id, href);
                            if ch_url.trim_end_matches('/') == external_id.trim_end_matches('/') {
                                continue;
                            }
                            let title = collapse_ws(&a.text().collect::<String>());
                            let t_low = title.to_ascii_lowercase();
                            let u_low = ch_url.to_ascii_lowercase();
                            let same_host = Url::parse(&ch_url)
                                .ok()
                                .zip(base_parsed.as_ref())
                                .is_some_and(|(u, b)| u.host_str() == b.host_str());
                            let sub_path = !novel_path.is_empty()
                                && u_low.contains(&novel_path.to_ascii_lowercase())
                                && u_low.len() > novel_path.len() + 1;
                            let looks_chapter = sub_path
                                || ["chapter", "chap", "ch-", "/ch/", "episode", "part-"]
                                    .iter()
                                    .any(|kw| t_low.contains(kw) || u_low.contains(kw));
                            if !same_host || !looks_chapter || title.is_empty() || !seen_urls.insert(ch_url.clone()) {
                                continue;
                            }
                            chapters.push(Chapter {
                                id: make_chapter_id(&novel_id, &ch_url),
                                novel_id: novel_id.clone(),
                                external_id: ch_url.clone(),
                                chapter_number: (chapters.len() + 1) as f64,
                                volume: None,
                                title,
                                url: Some(ch_url),
                                word_count: 0,
                                published_at: None,
                                is_downloaded: false,
                                local_path: None,
                                size_bytes: 0,
                            });
                        }
                    }
                }

                // Single-page story / essay fallback if no TOC links exist on the page.
                if chapters.is_empty() {
                    let (_, _, wc) = self.auto_extract_prose(&html);
                    if wc >= 8 {
                        let title = first_selector_text(&doc, &["h1", "title"])
                            .unwrap_or_else(|| "Chapter 1".into());
                        chapters.push(Chapter {
                            id: make_chapter_id(&novel_id, external_id),
                            novel_id,
                            external_id: external_id.to_string(),
                            chapter_number: 1.0,
                            volume: None,
                            title,
                            url: Some(external_id.to_string()),
                            word_count: wc,
                            published_at: None,
                            is_downloaded: false,
                            local_path: None,
                            size_bytes: 0,
                        });
                    }
                }

                sort_chapters_ascending(&mut chapters);
                Ok(chapters)
            }
        }
    }

    async fn get_chapter_content(
        &self,
        external_novel_id: &str,
        external_chapter_id: &str,
        chapter_url: Option<&str>,
        _config: &HashMap<String, Value>,
    ) -> AppResult<(String, Vec<String>, i64)> {
        match self.manifest.kind {
            DeclarativeKind::Opds => {
                let parsed = self.ensure_opds_parsed(external_novel_id).await?;
                let ch = parsed
                    .chapters
                    .into_iter()
                    .find(|c| c.external_id == external_chapter_id)
                    .ok_or_else(|| AppError::NotFound("OPDS chapter not found".into()))?;
                Ok((ch.html, ch.paragraphs, ch.word_count))
            }
            DeclarativeKind::Html => {
                let target = chapter_url.unwrap_or(external_chapter_id);
                let html = self.http.get_text(target).await?;
                let doc = Html::parse_document(&html);
                let content_sel_str = self
                    .manifest
                    .content_selector
                    .as_deref()
                    .unwrap_or("article, main, body");
                let content_sel = Selector::parse(content_sel_str)
                    .map_err(|e| AppError::InvalidInput(format!("Invalid content_selector: {e}")))?;
                let inner = doc
                    .select(&content_sel)
                    .next()
                    .map(|e| e.inner_html())
                    .unwrap_or_else(|| html.clone());

                let rm_refs: Vec<&str> = self
                    .manifest
                    .remove_selectors
                    .iter()
                    .map(|s| s.as_str())
                    .collect();
                Ok(clean_html_fragment_excluding(&inner, &rm_refs))
            }
            DeclarativeKind::Auto => {
                let novel_id = make_novel_id(self.id(), external_novel_id);
                if let Some(cached) = self.cache.get(&novel_id) {
                    if let Some(ch) = cached
                        .chapters
                        .into_iter()
                        .find(|c| c.external_id == external_chapter_id)
                    {
                        return Ok((ch.html, ch.paragraphs, ch.word_count));
                    }
                }
                let target = chapter_url.unwrap_or(external_chapter_id);
                let html = self.http.get_text(target).await?;
                Ok(self.auto_extract_prose(&html))
            }
        }
    }

    async fn health_check(&self, _config: &HashMap<String, Value>) -> AppResult<u64> {
        let raw_probe = self
            .manifest
            .feed_url
            .as_deref()
            .or(self.manifest.browse_url_template.as_deref())
            .unwrap_or(&self.manifest.base_url);
        let probe = raw_probe
            .replace("{page}", "1")
            .replace("{query}", "")
            .replace("{genre}", "");
        self.http.probe_latency(&probe).await
    }
}

fn extract_chapter_order_num(title: &str, url: &str) -> Option<f64> {
    let parse_after_kw = |s: &str| -> Option<f64> {
        let lower = s.to_ascii_lowercase();
        for kw in ["chapter", "chap", "ch-", "ch ", "bab"] {
            if let Some(pos) = lower.find(kw) {
                let tail = &lower[pos + kw.len()..];
                let digits: String = tail
                    .chars()
                    .skip_while(|c| !c.is_ascii_digit())
                    .take_while(|c| c.is_ascii_digit() || *c == '.')
                    .collect();
                if let Ok(n) = digits.trim_matches('.').parse::<f64>() {
                    return Some(n);
                }
            }
        }
        None
    };
    parse_after_kw(title).or_else(|| parse_after_kw(url))
}

/// Collapses whitespace and strips common HTML label prefixes ("Author:", "By ").
fn clean_author_text(raw: &str) -> Option<String> {
    let mut s = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let prefixes = ["author(s):", "authors:", "author:", "written by ", "by "];
    for p in prefixes {
        if s.len() >= p.len() && s[..p.len()].eq_ignore_ascii_case(p) {
            s = s[p.len()..].trim().to_string();
            break;
        }
    }
    let trimmed = s.trim_matches(|c: char| c.is_whitespace() || matches!(c, ':' | '-' | '|'));
    if trimmed.is_empty() || trimmed.len() > 140 || is_placeholder_author(trimmed) {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn json_ld_author_value(val: &Value) -> Option<String> {
    match val {
        Value::String(s) => clean_author_text(s),
        Value::Object(map) => {
            if let Some(auth) = map.get("author").and_then(json_ld_author_value) {
                return Some(auth);
            }
            if let Some(name) = map.get("name").and_then(|n| n.as_str()) {
                return clean_author_text(name);
            }
            if let Some(graph) = map.get("@graph").and_then(json_ld_author_value) {
                return Some(graph);
            }
            None
        }
        Value::Array(arr) => {
            let names: Vec<String> = arr
                .iter()
                .filter_map(|item| match item {
                    Value::String(s) => clean_author_text(s),
                    Value::Object(m) => m
                        .get("name")
                        .and_then(|n| n.as_str())
                        .and_then(clean_author_text)
                        .or_else(|| json_ld_author_value(item)),
                    _ => json_ld_author_value(item),
                })
                .collect();
            if names.is_empty() {
                None
            } else {
                clean_author_text(&names.join(", "))
            }
        }
        _ => None,
    }
}

fn extract_json_ld_author(doc: &Html) -> Option<String> {
    let sel = Selector::parse("script[type='application/ld+json']").ok()?;
    for script in doc.select(&sel) {
        let raw = script.text().collect::<String>();
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(val) = serde_json::from_str::<Value>(trimmed) {
            if let Some(author) = json_ld_author_value(&val) {
                return Some(author);
            }
        }
    }
    None
}

fn extract_html_card_author(card: &ElementRef<'_>, explicit: Option<&Selector>) -> Option<String> {
    if let Some(sel) = explicit {
        for el in card.select(sel) {
            if let Some(cleaned) = clean_author_text(&el.text().collect::<String>()) {
                return Some(cleaned);
            }
        }
    }
    const CARD_FALLBACKS: &[&str] = &[
        "[itemprop='author'] [itemprop='name']",
        "[itemprop='author']",
        ".author a",
        ".author",
        ".book-author",
        ".novel-author",
        ".byline a",
        ".byline",
        "[rel='author']",
        "a[href*='/author']",
        "a[href*='author=']",
    ];
    for css in CARD_FALLBACKS {
        if let Ok(sel) = Selector::parse(css) {
            for el in card.select(&sel) {
                if let Some(cleaned) = clean_author_text(&el.text().collect::<String>()) {
                    return Some(cleaned);
                }
            }
        }
    }
    None
}

fn extract_html_detail_author(doc: &Html, explicit: Option<&str>) -> Option<String> {
    if let Some(sel_str) = explicit {
        if let Ok(sel) = Selector::parse(sel_str) {
            for el in doc.select(&sel) {
                let candidate = el
                    .value()
                    .attr("content")
                    .map(str::to_string)
                    .unwrap_or_else(|| el.text().collect::<String>());
                if let Some(cleaned) = clean_author_text(&candidate) {
                    return Some(cleaned);
                }
            }
        }
    }

    const DETAIL_FALLBACKS: &[&str] = &[
        "[itemprop='author'] [itemprop='name']",
        "[itemprop='author']",
        "meta[name='author']",
        "a[href*='/author/']",
        "a[href*='/authors/']",
        "a[href*='author=']",
        "a[href*='novel-author']",
        ".author-content a",
        ".author a",
        ".author",
        ".book-author",
    ];
    for css in DETAIL_FALLBACKS {
        if let Ok(sel) = Selector::parse(css) {
            for el in doc.select(&sel) {
                let raw = if el.value().name() == "meta" {
                    el.value().attr("content").unwrap_or("").to_string()
                } else {
                    el.text().collect::<String>()
                };
                if let Some(cleaned) = clean_author_text(&raw) {
                    return Some(cleaned);
                }
            }
        }
    }
    extract_json_ld_author(doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn test_declarative_html_source_with_mock_server_and_robots() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base = format!("http://{addr}");

        let server_handle = tokio::spawn(async move {
            for _ in 0..12 {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let path = req
                        .lines()
                        .next()
                        .and_then(|l| l.split_whitespace().nth(1))
                        .unwrap_or("/");

                    let (status, ctype, body) = if path == "/robots.txt" {
                        ("200 OK", "text/plain", "User-agent: *\nDisallow: /secret\n".to_string())
                    } else if path.starts_with("/catalog") {
                        (
                            "200 OK",
                            "text/html",
                            r#"<html><body>
                                <div class="book-card">
                                    <a class="book-link" href="/novel/moon"></a>
                                    <h3 class="book-title">Moonlight Vale</h3>
                                    <span class="book-author">Clara Reed</span>
                                    <p class="book-desc">An atmospheric mystery.</p>
                                </div>
                            </body></html>"#
                                .to_string(),
                        )
                    } else if path == "/novel/moon" {
                        (
                            "200 OK",
                            "text/html",
                            r#"<html><body>
                                <h1 class="book-title">Moonlight Vale</h1>
                                <span class="book-author">Clara Reed</span>
                                <ul class="chapters">
                                    <li><a href="/novel/moon/ch1">Chapter 1: Fog</a></li>
                                </ul>
                            </body></html>"#
                                .to_string(),
                        )
                    } else if path == "/novel/moon/ch1" {
                        (
                            "200 OK",
                            "text/html",
                            r#"<html><body><div id="prose"><p>Mist drifted over the valley.</p><div class="ads">Buy now</div></div></body></html>"#
                                .to_string(),
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

        let manifest = DeclarativeSourceManifest {
            id: "mock_html".into(),
            name: "Mock HTML Catalog".into(),
            kind: DeclarativeKind::Html,
            base_url: base.clone(),
            description: "Test source".into(),
            genres: vec!["Mystery".into()],
            feed_url: None,
            search_url_template: Some(format!("{base}/catalog?q={{query}}")),
            browse_url_template: Some(format!("{base}/catalog?page={{page}}")),
            novel_list_selector: Some(".book-card".into()),
            title_selector: Some(".book-title".into()),
            author_selector: Some(".book-author".into()),
            cover_selector: None,
            link_selector: Some(".book-link".into()),
            description_selector: Some(".book-desc".into()),
            chapter_list_selector: Some(".chapters li a".into()),
            chapter_title_selector: None,
            chapter_link_selector: None,
            content_selector: Some("#prose".into()),
            remove_selectors: vec![".ads".into()],
            language: Some("en".into()),
        };

        let tmp = tempfile::tempdir().unwrap();
        let http = SourceHttpClient::new().unwrap();
        let cache = ParsedBookCache::new(tmp.path().to_path_buf());
        let src = DeclarativeSource::new(manifest, http.clone(), cache);

        // Discovery works
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
                    limit: 20,
                },
                &HashMap::new(),
            )
            .await
            .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Moonlight Vale");
        assert_eq!(items[0].author, "Clara Reed");

        // Chapter list & content parsing (including remove_selectors)
        let chs = src
            .get_chapters(&items[0].external_id, &HashMap::new())
            .await
            .unwrap();
        assert_eq!(chs.len(), 1);
        assert_eq!(chs[0].title, "Chapter 1: Fog");

        let (html, paras, wc) = src
            .get_chapter_content(
                &items[0].external_id,
                &chs[0].external_id,
                chs[0].url.as_deref(),
                &HashMap::new(),
            )
            .await
            .unwrap();
        assert!(html.contains("Mist drifted over the valley."));
        assert!(!html.contains("Buy now"));
        assert_eq!(paras, vec!["Mist drifted over the valley."]);
        assert_eq!(wc, 5);

        // Robots.txt checks are disabled by default; opt-in enforcement still rejects /secret
        assert!(!matches!(
            http.get_text(&format!("{base}/secret/page")).await,
            Err(AppError::RobotsDisallowed(_))
        ));
        let strict_http = http.clone().with_robots_enforcement(true);
        let res = strict_http.get_text(&format!("{base}/secret/page")).await;
        assert!(matches!(res, Err(AppError::RobotsDisallowed(_))));

        server_handle.abort();
    }

    #[tokio::test]
    async fn test_opds_feed_discovery_search_and_acquisition() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base = format!("http://{addr}");

        let server_handle = tokio::spawn(async move {
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
                        ("404 Not Found", "text/plain", "".to_string())
                    } else if path.starts_with("/opds") {
                        let feed = r#"<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>Calibre Test OPDS</title>
  <link rel="next" href="/opds?page=2"/>
  <entry>
    <title>Clockwork Harbour</title>
    <author><name>M. Verne</name></author>
    <summary>Steampunk maritime chronicle.</summary>
    <category term="Steampunk" label="Steampunk"/>
    <link rel="http://opds-spec.org/image" href="/covers/harbour.jpg"/>
    <link rel="http://opds-spec.org/acquisition" type="text/html" href="/books/harbour.html"/>
  </entry>
</feed>"#;
                        ("200 OK", "application/atom+xml", feed.to_string())
                    } else if path == "/books/harbour.html" {
                        (
                            "200 OK",
                            "text/html",
                            "<html><head><title>Clockwork Harbour</title><meta name=\"author\" content=\"M. Verne\"></head><body><h2>Chapter 1: Brass Bells</h2><p>Gears turned under the misty pier.</p><p>The airship cast off.</p><h2>Chapter 2: Open Sky</h2><p>Clouds parted before dawn.</p></body></html>".to_string(),
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

        let manifest = DeclarativeSourceManifest {
            id: "mock_opds".into(),
            name: "Mock OPDS Feed".into(),
            kind: DeclarativeKind::Opds,
            base_url: base.clone(),
            description: "OPDS Atom test".into(),
            genres: vec!["Steampunk".into()],
            feed_url: Some(format!("{base}/opds?page={{page}}")),
            search_url_template: Some(format!("{base}/opds?q={{query}}&page={{page}}")),
            browse_url_template: None,
            novel_list_selector: None,
            title_selector: None,
            author_selector: None,
            cover_selector: None,
            link_selector: None,
            description_selector: None,
            chapter_list_selector: None,
            chapter_title_selector: None,
            chapter_link_selector: None,
            content_selector: None,
            remove_selectors: vec![],
            language: None,
        };

        let tmp = tempfile::tempdir().unwrap();
        let http = SourceHttpClient::new().unwrap();
        let cache = ParsedBookCache::new(tmp.path().to_path_buf());
        let src = DeclarativeSource::new(manifest, http, cache);

        let (items, has_more, _) = src
            .discover(
                &DiscoveryQuery {
                    source_id: None,
                    query: Some("clockwork".into()),
                    genre: None,
                    sort: None,
                    status: None,
                    language: None,
                    page: 1,
                    limit: 20,
                },
                &HashMap::new(),
            )
            .await
            .unwrap();

        assert!(has_more);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Clockwork Harbour");
        assert_eq!(items[0].author, "M. Verne");
        assert_eq!(
            items[0].cover_url.as_deref(),
            Some(format!("{base}/covers/harbour.jpg").as_str())
        );

        let novel = src.get_novel(&items[0].external_id, &HashMap::new()).await.unwrap();
        assert_eq!(novel.chapter_count, 2);

        let chs = src.get_chapters(&items[0].external_id, &HashMap::new()).await.unwrap();
        assert_eq!(chs.len(), 2);
        assert_eq!(chs[0].title, "Chapter 1: Brass Bells");
        assert_eq!(chs[1].title, "Chapter 2: Open Sky");

        let (html, paras, wc) = src
            .get_chapter_content(
                &items[0].external_id,
                &chs[0].external_id,
                chs[0].url.as_deref(),
                &HashMap::new(),
            )
            .await
            .unwrap();
        assert!(html.contains("Gears turned under the misty pier."));
        assert_eq!(paras.len(), 2);
        assert_eq!(wc, 10);

        server_handle.abort();
    }

    #[tokio::test]
    async fn html_preset_supports_page0_lazy_cover_and_deduped_chapters() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base = format!("http://{addr}");
        let server_handle = tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else { break };
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let path = req
                        .lines()
                        .next()
                        .and_then(|l| l.split_whitespace().nth(1))
                        .unwrap_or("/");
                    let (status, ctype, body) = if path == "/robots.txt" {
                        ("200 OK", "text/plain", "User-agent: *\nAllow: /\n".to_string())
                    } else if path == "/list/all-0.html" {
                        (
                            "200 OK",
                            "text/html",
                            r#"<html><body>
                                <li class="novel-item"><a href="/novel/sky"><h4 class="novel-title">Sky Sword</h4><img src="/placeholder.gif" data-src="/covers/sky.jpg"></a></li>
                                <li class="novel-item"><a href="/novel/other"><h4 class="novel-title">Unrelated Book</h4><img src="/covers/other.jpg"></a></li>
                            </body></html>"#.to_string(),
                        )
                    } else if path == "/novel/sky" {
                        (
                            "200 OK",
                            "text/html",
                            r#"<html><body>
                                <h1 class="novel-title">Sky Sword</h1>
                                <ul class="chapter-list">
                                    <li><a href="/ch/2">Chapter 2</a></li>
                                    <li><a href="/ch/2">Chapter 2</a></li>
                                    <li><a href="/ch/1">Chapter 1</a></li>
                                </ul>
                            </body></html>"#.to_string(),
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

        let manifest = DeclarativeSourceManifest {
            id: "preset_html".into(),
            name: "Preset HTML".into(),
            kind: DeclarativeKind::Html,
            base_url: base.clone(),
            description: "HTML preset test".into(),
            genres: vec![],
            feed_url: None,
            search_url_template: None,
            browse_url_template: Some(format!("{base}/list/all-{{page0}}.html")),
            novel_list_selector: Some("li.novel-item".into()),
            title_selector: Some("h1.novel-title, h4.novel-title".into()),
            author_selector: None,
            cover_selector: Some("img".into()),
            link_selector: Some("a[href^='/novel/']".into()),
            description_selector: None,
            chapter_list_selector: Some("ul.chapter-list li a".into()),
            chapter_title_selector: None,
            chapter_link_selector: None,
            content_selector: Some(".chapter-content".into()),
            remove_selectors: vec![],
            language: Some("en".into()),
        };

        let tmp = tempfile::tempdir().unwrap();
        let src = DeclarativeSource::new_builtin(
            manifest,
            SourceHttpClient::new().unwrap(),
            ParsedBookCache::new(tmp.path().to_path_buf()),
        );
        assert!(!src.is_custom());
        let (items, has_more, _) = src
            .discover(
                &DiscoveryQuery {
                    source_id: None,
                    query: Some("sky".into()),
                    genre: None,
                    sort: None,
                    status: None,
                    language: Some("en".into()),
                    page: 1,
                    limit: 20,
                },
                &HashMap::new(),
            )
            .await
            .unwrap();
        assert!(has_more);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].language.as_deref(), Some("en"));
        assert_eq!(
            items[0].cover_url.as_deref(),
            Some(format!("{base}/covers/sky.jpg").as_str())
        );

        let (pl_items, _, _) = src
            .discover(
                &DiscoveryQuery {
                    source_id: None,
                    query: None,
                    genre: None,
                    sort: None,
                    status: None,
                    language: Some("pl".into()),
                    page: 1,
                    limit: 20,
                },
                &HashMap::new(),
            )
            .await
            .unwrap();
        assert!(pl_items.is_empty());

        let chapters = src.get_chapters(&items[0].external_id, &HashMap::new()).await.unwrap();
        assert_eq!(chapters.len(), 2);
        assert_eq!(chapters[0].title, "Chapter 1");
        assert_eq!(chapters[1].title, "Chapter 2");
        assert_eq!(chapters[1].chapter_number, 2.0);
        server_handle.abort();
    }

    #[test]
    fn test_extract_html_author_fallbacks_and_cleanup() {
        let jsonld_html = Html::parse_document(
            r#"<html><head><script type="application/ld+json">{"@type":"Book","name":"Mysteries","author":{"@type":"Person","name":"Cuttlefish That Loves Diving"}}</script></head><body><h1>Mysteries</h1></body></html>"#,
        );
        assert_eq!(
            extract_html_detail_author(&jsonld_html, None).as_deref(),
            Some("Cuttlefish That Loves Diving")
        );

        let nobad_html = Html::parse_document(
            r#"<html><body><span class="mr-2">Author:</span><a href="/series?author=abc">  Dancing   raindrops </a></body></html>"#,
        );
        assert_eq!(
            extract_html_detail_author(&nobad_html, None).as_deref(),
            Some("Dancing raindrops")
        );

        let dirty_span = Html::parse_document(
            r#"<html><body><span class="author"><span class="icon"></span>
            Author:  Aravind_S  </span></body></html>"#,
        );
        assert_eq!(
            extract_html_detail_author(&dirty_span, Some(".author")).as_deref(),
            Some("Aravind_S")
        );
    }

    #[tokio::test]
    async fn test_auto_url_parser_discovers_lists_opds_chapters_and_single_page() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base = format!("http://{addr}");

        let server_handle = tokio::spawn(async move {
            for _ in 0..16 {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let path = req
                        .lines()
                        .next()
                        .and_then(|l| l.split_whitespace().nth(1))
                        .unwrap_or("/");

                    let (status, ctype, body) = if path == "/robots.txt" {
                        ("200 OK", "text/plain", "User-agent: *\nAllow: /\n".to_string())
                    } else if path == "/" {
                        (
                            "200 OK",
                            "text/html",
                            r#"<html lang="en"><body>
                                <form method="get" action="/find"><input type="search" name="q" /></form>
                                <article>
                                    <a href="/book/starlight"><h3>Starlight</h3></a>
                                    <span class="byline">By Vera Holt</span>
                                    <p>Among quiet constellations.</p>
                                </article>
                                <article>
                                    <a href="/essay/single"><h3>Brief Essay</h3></a>
                                    <span class="byline">By Sam Moss</span>
                                </article>
                                <a rel="next" href="/?page=2">Next</a>
                            </body></html>"#
                                .to_string(),
                        )
                    } else if path.starts_with("/find?q=star") {
                        (
                            "200 OK",
                            "text/html",
                            r#"<html lang="en"><body>
                                <article>
                                    <a href="/book/starlight"><h3>Starlight</h3></a>
                                    <span class="byline">By Vera Holt</span>
                                </article>
                            </body></html>"#
                                .to_string(),
                        )
                    } else if path == "/book/starlight" {
                        (
                            "200 OK",
                            "text/html",
                            r#"<html lang="en"><body>
                                <h1>Starlight</h1>
                                <span class="author">Vera Holt</span>
                                <div class="synopsis">Far above earth.</div>
                                <ul class="chapters">
                                    <li><a href="/book/starlight/ch-2">Chapter 2</a></li>
                                    <li><a href="/book/starlight/ch-1">Chapter 1</a></li>
                                </ul>
                            </body></html>"#
                                .to_string(),
                        )
                    } else if path == "/book/starlight/ch-1" {
                        (
                            "200 OK",
                            "text/html",
                            r#"<html><body>
                                <nav>Site menu</nav>
                                <div id="chapter-content">
                                    <p>The lantern flickered in the cold observatory dome.</p>
                                    <div class="ads">Sponsored link</div>
                                </div>
                            </body></html>"#
                                .to_string(),
                        )
                    } else if path == "/essay/single" {
                        (
                            "200 OK",
                            "text/html",
                            r#"<html><body><article><h1>Brief Essay</h1><p>This short stand-alone essay has plenty of readable prose for automatic chapter detection.</p></article></body></html>"#
                                .to_string(),
                        )
                    } else if path == "/feed.opds" {
                        (
                            "200 OK",
                            "application/atom+xml",
                            r#"<?xml version="1.0" encoding="utf-8"?>
                            <feed xmlns="http://www.w3.org/2005/Atom">
                                <title>Auto OPDS</title>
                                <entry>
                                    <title>Auto Feed Novel</title>
                                    <author><name>Ada Doe</name></author>
                                    <link rel="http://opds-spec.org/acquisition" type="text/html" href="/essay/single"/>
                                </entry>
                            </feed>"#
                                .to_string(),
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
        let cache_dir = tempfile::tempdir().unwrap();
        let cache = ParsedBookCache::new(cache_dir.path().to_path_buf());
        let manifest = DeclarativeSourceManifest::from_url(&base);
        assert!(manifest.id.starts_with("auto_127_0_0_1"));
        assert_eq!(manifest.kind, DeclarativeKind::Auto);
        let src = DeclarativeSource::new(manifest, http.clone(), cache.clone());

        let (items, has_more, _) = src
            .discover(
                &DiscoveryQuery {
                    source_id: None,
                    query: None,
                    genre: None,
                    sort: None,
                    status: None,
                    language: Some("en".into()),
                    page: 1,
                    limit: 20,
                },
                &HashMap::new(),
            )
            .await
            .unwrap();
        assert!(has_more);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].title, "Starlight");
        assert_eq!(items[0].author, "Vera Holt");
        assert_eq!(items[0].language.as_deref(), Some("en"));

        let (searched, _, _) = src
            .discover(
                &DiscoveryQuery {
                    source_id: None,
                    query: Some("star".into()),
                    genre: None,
                    sort: None,
                    status: None,
                    language: None,
                    page: 1,
                    limit: 20,
                },
                &HashMap::new(),
            )
            .await
            .unwrap();
        assert_eq!(searched.len(), 1);

        let rec = src.get_novel(&items[0].external_id, &HashMap::new()).await.unwrap();
        assert_eq!(rec.title, "Starlight");
        assert_eq!(rec.author, "Vera Holt");

        let chs = src.get_chapters(&items[0].external_id, &HashMap::new()).await.unwrap();
        assert_eq!(chs.len(), 2);
        assert_eq!(chs[0].title, "Chapter 1");

        let (_, paras, wc) = src
            .get_chapter_content(&items[0].external_id, &chs[0].external_id, chs[0].url.as_deref(), &HashMap::new())
            .await
            .unwrap();
        assert!(wc > 0);
        assert!(paras[0].contains("lantern flickered"));
        assert!(!paras.join(" ").contains("Sponsored"));

        // Single-page story fallback
        let single_chs = src.get_chapters(&items[1].external_id, &HashMap::new()).await.unwrap();
        assert_eq!(single_chs.len(), 1);

        // Auto OPDS detection from a feed URL
        let opds_src = DeclarativeSource::new(
            DeclarativeSourceManifest::from_url(&format!("{base}/feed.opds")),
            http.clone(),
            cache.clone(),
        );
        let (feed_items, _, _) = opds_src
            .discover(
                &DiscoveryQuery {
                    source_id: None,
                    query: None,
                    genre: None,
                    sort: None,
                    status: None,
                    language: None,
                    page: 1,
                    limit: 20,
                },
                &HashMap::new(),
            )
            .await
            .unwrap();
        assert_eq!(feed_items.len(), 1);
        assert_eq!(feed_items[0].title, "Auto Feed Novel");

        // Ensure blank string selectors (e.g. "" from UI forms) never trigger EmptySelector
        let mut blank_sel_manifest = DeclarativeSourceManifest::from_url(&base);
        blank_sel_manifest.novel_list_selector = Some("".into());
        blank_sel_manifest.chapter_list_selector = Some("   ".into());
        blank_sel_manifest.content_selector = Some("".into());
        blank_sel_manifest.fill_defaults_from_url().unwrap();
        assert!(blank_sel_manifest.novel_list_selector.is_none());
        let blank_sel_src = DeclarativeSource::new(blank_sel_manifest, http, cache);
        let (blank_items, _, _) = blank_sel_src
            .discover(
                &DiscoveryQuery {
                    source_id: None,
                    query: None,
                    genre: None,
                    sort: None,
                    status: None,
                    language: None,
                    page: 1,
                    limit: 20,
                },
                &HashMap::new(),
            )
            .await
            .unwrap();
        assert_eq!(blank_items.len(), 2);

        server_handle.abort();
    }
}
