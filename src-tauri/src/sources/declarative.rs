use std::collections::{HashMap, HashSet};

use async_trait::async_trait;
use chrono::Utc;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

use crate::error::{AppError, AppResult};
use crate::formats::epub::parse_epub_bytes;
use crate::formats::text_html::{clean_html_fragment_excluding, parse_html_bytes};
use crate::models::{
    Chapter, DiscoveryQuery, NovelRecord, NovelStatus, NovelSummary, SourceCapabilities,
};
use crate::net::SourceHttpClient;
use crate::sources::{make_chapter_id, make_novel_id, CachedParsedBook, NovelSource, ParsedBookCache};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeclarativeKind {
    Opds,
    Html,
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

pub struct DeclarativeSource {
    manifest: DeclarativeSourceManifest,
    http: SourceHttpClient,
    cache: ParsedBookCache,
    is_builtin_preset: bool,
}

impl DeclarativeSource {
    pub fn new(
        manifest: DeclarativeSourceManifest,
        http: SourceHttpClient,
        cache: ParsedBookCache,
    ) -> Self {
        Self {
            manifest,
            http,
            cache,
            is_builtin_preset: false,
        }
    }

    pub fn new_builtin(
        manifest: DeclarativeSourceManifest,
        http: SourceHttpClient,
        cache: ParsedBookCache,
    ) -> Self {
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

            let author = author_sel
                .as_ref()
                .and_then(|s| card.select(s).next())
                .map(|e| e.text().collect::<String>().trim().to_string())
                .filter(|s| !s.is_empty())
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
        let parsed = if bytes.starts_with(b"PK") {
            parse_epub_bytes(&bytes, acquisition_url)?
        } else {
            parse_html_bytes(&bytes, acquisition_url)?
        };
        self.cache.put(&novel_id, &parsed)
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
            search: self.manifest.search_url_template.is_some()
                || self.manifest.feed_url.is_some()
                || self.manifest.browse_url_template.is_some(),
            browse_popular: self.manifest.feed_url.is_some()
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
        if let Some(req_lang) = query
            .language
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("all"))
        {
            if let Some(ref src_lang) = self.manifest.language {
                if !src_lang.eq_ignore_ascii_case(req_lang) {
                    return Ok((Vec::new(), false, None));
                }
            } else {
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

        let body = self.http.get_text(&target_url).await?;
        let filter_local = |items: &mut Vec<NovelSummary>| {
            if let Some(q) = trimmed_q {
                if self.manifest.search_url_template.is_none() {
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
                let (mut items, has_next) = self.parse_opds_feed(&target_url, &body)?;
                filter_local(&mut items);
                Ok((items, has_next, None))
            }
            DeclarativeKind::Html => {
                let mut items = self.parse_html_list(&target_url, &body)?;
                filter_local(&mut items);
                let has_next = !items.is_empty()
                    && used_tpl.is_some_and(|tpl| tpl.contains("{page"));
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
        match self.manifest.kind {
            DeclarativeKind::Opds => {
                let bytes = self.http.get_bytes(external_id).await?;
                let parsed = if bytes.starts_with(b"PK") {
                    parse_epub_bytes(&bytes, external_id)?
                } else {
                    parse_html_bytes(&bytes, external_id)?
                };
                let novel_id = make_novel_id(self.id(), external_id);
                self.cache.put(&novel_id, &parsed)?;
                Ok(NovelRecord {
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
                    updated_at: now,
                })
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
                let author = self
                    .manifest
                    .author_selector
                    .as_deref()
                    .and_then(|s| Selector::parse(s).ok())
                    .and_then(|sel| doc.select(&sel).next())
                    .map(|e| e.text().collect::<String>().trim().to_string())
                    .filter(|s| !s.is_empty())
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
        }
    }

    async fn get_chapters(
        &self,
        external_id: &str,
        _config: &HashMap<String, Value>,
    ) -> AppResult<Vec<Chapter>> {
        let novel_id = make_novel_id(self.id(), external_id);
        match self.manifest.kind {
            DeclarativeKind::Opds => {
                let parsed = self.ensure_opds_parsed(external_id).await?;
                Ok(parsed
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
                    .collect())
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
                if chapters.len() >= 2 {
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

        // Robots.txt enforcement rejects /secret
        let res = http.get_text(&format!("{base}/secret/page")).await;
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
}
