use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::Utc;
use flate2::read::GzDecoder;
use scraper::{Html, Selector};
use serde_json::Value;
use tokio::sync::{Mutex, RwLock};

use crate::error::{AppError, AppResult};
use crate::formats::epub::parse_epub_bytes;
use crate::formats::text_html::{parse_html_bytes, parse_txt_bytes};
use crate::models::{
    Chapter, DiscoveryQuery, NovelRecord, NovelStatus, NovelSummary, NovelTranslation,
    SourceCapabilities, SourceConfigField,
};
use crate::net::SourceHttpClient;
use crate::sources::{
    make_chapter_id, make_novel_id, CachedParsedBook, NovelSource, ParsedBookCache,
};

#[derive(Debug, Clone)]
struct TopScoreEntry {
    id: u64,
    title: String,
    author: String,
    downloads: i64,
}

#[derive(Debug, Clone)]
struct TopScoresCache {
    fetched_at: Instant,
    last_30d: Vec<TopScoreEntry>,
    last_7d: Vec<TopScoreEntry>,
    downloads_by_id: HashMap<u64, i64>,
}

#[derive(Debug, Clone)]
struct CatalogBook {
    id: u64,
    issued: String,
    title: String,
    title_lower: String,
    languages: Vec<String>,
    author: String,
    author_lower: String,
    subjects: Vec<String>,
    bookshelves: Vec<String>,
    search_blob_lower: String,
}

type CachedCatalog = Option<(Instant, Vec<CatalogBook>)>;

pub struct GutenbergSource {
    http: SourceHttpClient,
    cache: ParsedBookCache,
    base_url: String,
    top_cache: Arc<Mutex<Option<TopScoresCache>>>,
    catalog: Arc<RwLock<CachedCatalog>>,
    catalog_load_lock: Arc<Mutex<()>>,
}

impl GutenbergSource {
    pub fn new(http: SourceHttpClient, cache: ParsedBookCache) -> Self {
        Self::with_base_url(http, cache, "https://www.gutenberg.org")
    }

    pub fn with_base_url(http: SourceHttpClient, cache: ParsedBookCache, base_url: &str) -> Self {
        Self {
            http,
            cache,
            base_url: base_url.trim_end_matches('/').to_string(),
            top_cache: Arc::new(Mutex::new(None)),
            catalog: Arc::new(RwLock::new(None)),
            catalog_load_lock: Arc::new(Mutex::new(())),
        }
    }

    fn format_author(raw: &str) -> String {
        let parts: Vec<&str> = raw
            .split(',')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .collect();
        // Strip life-year tokens like "1797-1851", "ca. 1800-", "active 19th century"
        let non_date: Vec<&str> = parts
            .iter()
            .copied()
            .filter(|p| !p.chars().all(|c| c.is_ascii_digit() || c == '-' || c == '?'))
            .filter(|p| !p.starts_with("fl.") && !p.starts_with("ca."))
            .collect();
        match non_date.as_slice() {
            [last, first] => format!("{first} {last}"),
            [first] => (*first).to_string(),
            _ if parts.len() >= 2 => format!("{} {}", parts[1], parts[0]),
            _ => raw.trim().to_string(),
        }
    }

    fn format_authors_field(raw: &str) -> String {
        let list: Vec<String> = raw
            .split(';')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(Self::format_author)
            .collect();
        if list.is_empty() {
            "Unknown Author".to_string()
        } else {
            list.join(", ")
        }
    }

    fn extract_genres(bookshelves: &[String], subjects: &[String]) -> Vec<String> {
        let mut out = Vec::new();
        for b in bookshelves {
            let clean = b
                .strip_prefix("Category: ")
                .or_else(|| b.strip_prefix("Browsing: "))
                .unwrap_or(b)
                .trim();
            if !clean.is_empty()
                && !clean.eq_ignore_ascii_case("Best Books Ever Listings")
                && !out.iter().any(|x: &String| x.eq_ignore_ascii_case(clean))
            {
                out.push(clean.to_string());
            }
            if out.len() >= 4 {
                return out;
            }
        }
        for s in subjects {
            let clean = s.split("--").next().unwrap_or(s).trim();
            if !clean.is_empty() && !out.iter().any(|x| x.eq_ignore_ascii_case(clean)) {
                out.push(clean.to_string());
            }
            if out.len() >= 4 {
                break;
            }
        }
        out
    }

    fn language_name_to_code(raw: &str) -> String {
        match raw.trim().to_ascii_lowercase().as_str() {
            "english" | "en" => "en".into(),
            "polish" | "polski" | "pl" => "pl".into(),
            "french" | "français" | "fr" => "fr".into(),
            "german" | "deutsch" | "de" => "de".into(),
            "spanish" | "español" | "es" => "es".into(),
            "italian" | "italiano" | "it" => "it".into(),
            "portuguese" | "português" | "pt" => "pt".into(),
            "russian" | "ru" => "ru".into(),
            "japanese" | "ja" => "ja".into(),
            "chinese" | "zh" => "zh".into(),
            other => other.chars().take(2).collect(),
        }
    }

    fn normalize_work_key(s: &str) -> String {
        let base = s.split([':', ';', '(']).next().unwrap_or(s);
        base.chars()
            .map(|c| {
                if c.is_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    ' '
                }
            })
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn cover_url_for_id(&self, id: u64) -> String {
        format!("{}/cache/epub/{id}/pg{id}.cover.medium.jpg", self.base_url)
    }

    fn parse_top_scores(html: &str) -> (Vec<TopScoreEntry>, Vec<TopScoreEntry>) {
        let doc = Html::parse_document(html);
        let h2_sel = Selector::parse("h2").unwrap();
        let ol_sel = Selector::parse("ol").unwrap();
        let a_sel = Selector::parse("li a[href*='/ebooks/']").unwrap();

        let parse_ol = |ol: scraper::ElementRef<'_>| -> Vec<TopScoreEntry> {
            let mut list = Vec::new();
            let mut seen = HashSet::new();
            for a in ol.select(&a_sel) {
                let Some(href) = a.value().attr("href") else {
                    continue;
                };
                let Some(id_str) = href.trim_end_matches('/').rsplit('/').next() else {
                    continue;
                };
                let Ok(id) = id_str.parse::<u64>() else {
                    continue;
                };
                if !seen.insert(id) {
                    continue;
                }
                let text = a.text().collect::<Vec<_>>().join(" ");
                let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
                let (head, downloads) = if let Some((left, right)) = text.rsplit_once(" (") {
                    let count = right
                        .trim_end_matches(')')
                        .replace(',', "")
                        .parse::<i64>()
                        .unwrap_or(0);
                    (left.trim(), count)
                } else {
                    (text.as_str(), 0)
                };
                let (title, author) = if let Some((t, auth)) = head.rsplit_once(" by ") {
                    (t.trim().to_string(), auth.trim().to_string())
                } else {
                    (head.trim().to_string(), "Unknown Author".to_string())
                };
                if !title.is_empty() {
                    list.push(TopScoreEntry {
                        id,
                        title,
                        author,
                        downloads,
                    });
                }
            }
            list
        };

        let mut last_30d = Vec::new();
        let mut last_7d = Vec::new();

        // Find <ol> following #books-last30 and #books-last7
        for h2 in doc.select(&h2_sel) {
            let id_attr = h2.value().attr("id").unwrap_or("");
            if id_attr == "books-last30" || id_attr == "books-last7" || id_attr == "books-last1" {
                let mut sib = h2.next_sibling();
                while let Some(node) = sib {
                    if let Some(el_ref) = scraper::ElementRef::wrap(node) {
                        if el_ref.value().name() == "ol" {
                            let parsed = parse_ol(el_ref);
                            if id_attr == "books-last30" {
                                last_30d = parsed;
                            } else if id_attr == "books-last7"
                                || (id_attr == "books-last1" && last_7d.is_empty())
                            {
                                last_7d = parsed;
                            }
                            break;
                        }
                        if el_ref.value().name() == "h2" {
                            break;
                        }
                    }
                    sib = node.next_sibling();
                }
            }
        }

        if last_30d.is_empty() {
            if let Some(first_ol) = doc.select(&ol_sel).next() {
                last_30d = parse_ol(first_ol);
            }
        }
        if last_7d.is_empty() {
            last_7d = last_30d.clone();
        }

        (last_30d, last_7d)
    }

    async fn get_top_scores(&self) -> AppResult<TopScoresCache> {
        {
            let guard = self.top_cache.lock().await;
            if let Some(ref cached) = *guard {
                if cached.fetched_at.elapsed() < Duration::from_secs(1800) {
                    return Ok(cached.clone());
                }
            }
        }

        let url = format!("{}/browse/scores/top", self.base_url);
        let html = self.http.get_text(&url).await.map_err(|e| AppError::Source {
            source_id: self.id().to_string(),
            message: e.to_string(),
        })?;

        let (last_30d, last_7d) = Self::parse_top_scores(&html);
        let mut downloads_by_id = HashMap::new();
        for e in last_30d.iter().chain(last_7d.iter()) {
            downloads_by_id
                .entry(e.id)
                .and_modify(|v: &mut i64| {
                    if e.downloads > *v {
                        *v = e.downloads;
                    }
                })
                .or_insert(e.downloads);
        }

        let built = TopScoresCache {
            fetched_at: Instant::now(),
            last_30d,
            last_7d,
            downloads_by_id,
        };
        let mut guard = self.top_cache.lock().await;
        *guard = Some(built.clone());
        Ok(built)
    }

    fn parse_pg_catalog_csv(csv_text: &str) -> Vec<CatalogBook> {
        let records = parse_csv_records(csv_text);
        let mut rows = Vec::with_capacity(records.len().saturating_sub(1));
        for (idx, cols) in records.into_iter().enumerate() {
            if idx == 0 || cols.len() < 9 {
                continue;
            }
            if cols[1].trim() != "Text" {
                continue;
            }
            let Ok(id) = cols[0].trim().parse::<u64>() else {
                continue;
            };
            let issued = cols[2].trim().to_string();
            let title = cols[3]
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            if title.is_empty() {
                continue;
            }
            let languages: Vec<String> = cols[4]
                .split(',')
                .map(|s| s.trim().to_ascii_lowercase())
                .filter(|s| !s.is_empty())
                .collect();
            let author = Self::format_authors_field(&cols[5]);
            let subjects: Vec<String> = cols[6]
                .split(';')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToString::to_string)
                .collect();
            let bookshelves: Vec<String> = cols[8]
                .split(';')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToString::to_string)
                .collect();

            let title_lower = title.to_ascii_lowercase();
            let author_lower = author.to_ascii_lowercase();
            let search_blob_lower = format!(
                "{title_lower} {author_lower} {} {}",
                subjects.join(" ").to_ascii_lowercase(),
                bookshelves.join(" ").to_ascii_lowercase()
            );

            rows.push(CatalogBook {
                id,
                issued,
                title,
                title_lower,
                languages,
                author,
                author_lower,
                subjects,
                bookshelves,
                search_blob_lower,
            });
        }
        rows
    }

    async fn load_catalog(&self) -> AppResult<Vec<CatalogBook>> {
        {
            let guard = self.catalog.read().await;
            if let Some((at, ref books)) = *guard {
                if at.elapsed() < Duration::from_secs(6 * 3600) {
                    return Ok(books.clone());
                }
            }
        }

        let _load_guard = self.catalog_load_lock.lock().await;
        {
            let guard = self.catalog.read().await;
            if let Some((at, ref books)) = *guard {
                if at.elapsed() < Duration::from_secs(6 * 3600) {
                    return Ok(books.clone());
                }
            }
        }

        let gz_path = self.cache.root_dir().join("pg_catalog.csv.gz");
        let gz_bytes = if let Ok(meta) = std::fs::metadata(&gz_path) {
            let fresh = meta
                .modified()
                .ok()
                .and_then(|m| m.elapsed().ok())
                .map(|age| age < Duration::from_secs(24 * 3600))
                .unwrap_or(false);
            if fresh && meta.len() > 100 {
                std::fs::read(&gz_path)?
            } else {
                self.fetch_and_cache_catalog_gz(&gz_path).await?
            }
        } else {
            self.fetch_and_cache_catalog_gz(&gz_path).await?
        };

        let csv_text = decode_maybe_gzip(&gz_bytes)?;
        let books = tokio::task::spawn_blocking(move || Self::parse_pg_catalog_csv(&csv_text))
            .await
            .map_err(|e| AppError::Other(e.to_string()))?;

        let mut guard = self.catalog.write().await;
        *guard = Some((Instant::now(), books.clone()));
        Ok(books)
    }

    async fn fetch_and_cache_catalog_gz(&self, gz_path: &std::path::Path) -> AppResult<Vec<u8>> {
        let gz_url = format!("{}/cache/epub/feeds/pg_catalog.csv.gz", self.base_url);
        let bytes = match self.http.get_bytes(&gz_url).await {
            Ok(b) => b,
            Err(_) => {
                let csv_url = format!("{}/cache/epub/feeds/pg_catalog.csv", self.base_url);
                self.http.get_bytes(&csv_url).await.map_err(|e| AppError::Source {
                    source_id: self.id().to_string(),
                    message: e.to_string(),
                })?
            }
        };
        if let Some(parent) = gz_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let tmp = gz_path.with_extension("tmp");
        if std::fs::write(&tmp, &bytes).is_ok() {
            let _ = std::fs::rename(&tmp, gz_path);
        }
        Ok(bytes)
    }

    fn genre_matches(book: &CatalogBook, genre: &str) -> bool {
        let g = genre.trim().to_ascii_lowercase();
        if g.is_empty() || g == "all" {
            return true;
        }
        let aliases: &[&str] = match g.as_str() {
            "historical" => &["historical", "history"],
            "mystery" => &["mystery", "detective"],
            "horror" => &["horror", "gothic", "ghost"],
            "fiction" => &["fiction", "novel"],
            _ => &[],
        };
        let check = |hay: &str| -> bool {
            let h = hay.to_ascii_lowercase();
            if h.contains(&g) {
                return true;
            }
            aliases.iter().any(|a| h.contains(a))
        };
        book.bookshelves.iter().any(|s| check(s)) || book.subjects.iter().any(|s| check(s))
    }

    fn catalog_book_to_summary(&self, book: &CatalogBook, downloads: i64) -> NovelSummary {
        let genres = Self::extract_genres(&book.bookshelves, &book.subjects);
        let desc = if !book.subjects.is_empty() {
            format!("Subjects: {}", book.subjects.join(", "))
        } else {
            "Public domain classic from Project Gutenberg.".to_string()
        };
        NovelSummary {
            id: make_novel_id(self.id(), &book.id.to_string()),
            source_id: self.id().to_string(),
            external_id: book.id.to_string(),
            title: book.title.clone(),
            author: book.author.clone(),
            description: desc,
            cover_url: Some(self.cover_url_for_id(book.id)),
            local_cover_path: None,
            status: NovelStatus::Completed,
            genres,
            rating: None,
            popularity: downloads,
            chapter_count: None,
            last_updated: if book.issued.is_empty() {
                None
            } else {
                Some(book.issued.clone())
            },
        }
    }

    fn parse_ebook_page_metadata(
        &self,
        id: u64,
        html: &str,
    ) -> NovelRecord {
        let doc = Html::parse_document(html);
        let meta_sel = Selector::parse("meta").unwrap();
        let row_sel = Selector::parse("table.bibrec tr").unwrap();
        let th_sel = Selector::parse("th").unwrap();
        let td_sel = Selector::parse("td").unwrap();

        let mut og_title = String::new();
        let mut og_desc = String::new();
        let mut og_image = None;

        for m in doc.select(&meta_sel) {
            let prop = m.value().attr("property").unwrap_or("");
            let content = m.value().attr("content").unwrap_or("").trim();
            if content.is_empty() {
                continue;
            }
            match prop {
                "og:title" => og_title = content.to_string(),
                "og:description" => og_desc = content.to_string(),
                "og:image" => og_image = Some(content.to_string()),
                _ => {}
            }
        }

        let mut title = String::new();
        let mut authors = Vec::new();
        let mut subjects = Vec::new();
        let mut bookshelves = Vec::new();
        let mut languages = Vec::new();
        let mut release_date = None;
        let mut downloads = 0i64;
        let mut summary_notes = Vec::new();

        for tr in doc.select(&row_sel) {
            let key = tr
                .select(&th_sel)
                .next()
                .map(|e| e.text().collect::<Vec<_>>().join(" "))
                .unwrap_or_default()
                .trim()
                .to_ascii_lowercase();
            let val = tr
                .select(&td_sel)
                .next()
                .map(|e| e.text().collect::<Vec<_>>().join(" "))
                .unwrap_or_default()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            if key.is_empty() || val.is_empty() {
                continue;
            }
            match key.as_str() {
                "author" => authors.push(Self::format_author(&val)),
                "title" if title.is_empty() => title = val,
                "subject" => subjects.push(val),
                "category" | "bookshelf" => bookshelves.push(val),
                "language" => languages.push(val),
                "release date" => release_date = Some(val),
                "summary" => summary_notes.push(val),
                "downloads" => {
                    downloads = val
                        .split_whitespace()
                        .next()
                        .unwrap_or("0")
                        .replace(',', "")
                        .parse::<i64>()
                        .unwrap_or(0);
                }
                _ => {}
            }
        }

        if title.is_empty() {
            title = og_title
                .split(" by ")
                .next()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(&format!("Project Gutenberg #{id}"))
                .to_string();
        }
        let author = if authors.is_empty() {
            "Unknown Author".to_string()
        } else {
            authors.join(", ")
        };
        let description = if !summary_notes.is_empty() {
            summary_notes.join("\n\n")
        } else if !og_desc.is_empty() {
            og_desc
        } else if !subjects.is_empty() {
            format!("Subjects: {}", subjects.join(", "))
        } else {
            "Public domain classic from Project Gutenberg.".to_string()
        };

        let primary_lang = languages
            .first()
            .map(|l| Self::language_name_to_code(l))
            .unwrap_or_else(|| "en".to_string());
        let genres = Self::extract_genres(&bookshelves, &subjects);
        let now = Utc::now().to_rfc3339();
        NovelRecord {
            id: make_novel_id(self.id(), &id.to_string()),
            source_id: self.id().to_string(),
            external_id: id.to_string(),
            title,
            author_id: None,
            author,
            description,
            cover_url: og_image.or_else(|| Some(self.cover_url_for_id(id))),
            local_cover_path: None,
            status: NovelStatus::Completed,
            genres,
            tags: if bookshelves.is_empty() {
                subjects.into_iter().take(6).collect()
            } else {
                bookshelves
            },
            rating: None,
            popularity: downloads,
            chapter_count: 0,
            last_updated_remote: release_date,
            metadata: serde_json::json!({
                "language": primary_lang,
                "languages": languages,
                "gutenberg_id": id,
            }),
            is_local: false,
            created_at: now.clone(),
            updated_at: now,
        }
    }

    async fn ensure_parsed(&self, external_id: &str) -> AppResult<CachedParsedBook> {
        let id_clean = external_id.trim();
        let novel_id = make_novel_id(self.id(), id_clean);
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

        let fallback = format!("Book #{id_clean}");
        let epub_candidates = [
            format!("{}/cache/epub/{id_clean}/pg{id_clean}.epub", self.base_url),
            format!("{}/cache/epub/{id_clean}/pg{id_clean}-images.epub", self.base_url),
        ];

        let mut parsed = None;
        for url in &epub_candidates {
            if let Ok(bytes) = self.http.get_bytes(url).await {
                if let Ok(book) = parse_epub_bytes(&bytes, &fallback) {
                    if !book.chapters.is_empty() {
                        parsed = Some(book);
                        break;
                    }
                }
            }
        }

        if parsed.is_none() {
            let html_candidates = [
                format!("{}/cache/epub/{id_clean}/pg{id_clean}-images.html", self.base_url),
                format!("{}/cache/epub/{id_clean}/pg{id_clean}.html", self.base_url),
                format!("{}/files/{id_clean}/{id_clean}-h/{id_clean}-h.htm", self.base_url),
            ];
            for url in &html_candidates {
                if let Ok(bytes) = self.http.get_bytes(url).await {
                    if let Ok(book) = parse_html_bytes(&bytes, &fallback) {
                        if !book.chapters.is_empty() {
                            parsed = Some(book);
                            break;
                        }
                    }
                }
            }
        }

        let parsed = match parsed {
            Some(book) => book,
            None => {
                let txt_url = format!("{}/cache/epub/{id_clean}/pg{id_clean}.txt", self.base_url);
                let bytes = self.http.get_bytes(&txt_url).await?;
                parse_txt_bytes(&bytes, &fallback)?
            }
        };

        self.cache.put(&novel_id, &parsed)
    }
}

fn decode_maybe_gzip(bytes: &[u8]) -> AppResult<String> {
    if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut dec = GzDecoder::new(bytes);
        let mut s = String::new();
        dec.read_to_string(&mut s)?;
        Ok(s)
    } else {
        Ok(String::from_utf8_lossy(bytes).into_owned())
    }
}

fn parse_csv_records(input: &str) -> Vec<Vec<String>> {
    let text = input.strip_prefix('\u{feff}').unwrap_or(input);
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    in_quotes = false;
                }
            } else if c != '\r' {
                field.push(c);
            }
        } else {
            match c {
                '"' if field.is_empty() => in_quotes = true,
                ',' => {
                    row.push(std::mem::take(&mut field));
                }
                '\r' => {}
                '\n' => {
                    row.push(std::mem::take(&mut field));
                    if row.len() > 1 || (row.len() == 1 && !row[0].is_empty()) {
                        rows.push(std::mem::take(&mut row));
                    } else {
                        row.clear();
                    }
                }
                _ => field.push(c),
            }
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

#[async_trait]
impl NovelSource for GutenbergSource {
    fn id(&self) -> &str {
        "gutenberg"
    }

    fn name(&self) -> &str {
        "Project Gutenberg"
    }

    fn kind(&self) -> &str {
        "builtin"
    }

    fn base_url(&self) -> &str {
        &self.base_url
    }

    fn description(&self) -> &str {
        "Over 70,000 free public domain classics from Project Gutenberg's official catalog and EPUB archives."
    }

    fn capabilities(&self) -> SourceCapabilities {
        SourceCapabilities {
            search: true,
            browse_popular: true,
            browse_trending: true,
            browse_latest: true,
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
                "Philosophy".into(),
                "Poetry".into(),
            ],
        }
    }

    fn config_schema(&self) -> Vec<SourceConfigField> {
        vec![SourceConfigField {
            key: "language".into(),
            label: "Catalog Language".into(),
            field_type: "select".into(),
            default_value: Value::String("en".into()),
            description: "ISO 639-1 language code to filter Project Gutenberg books.".into(),
            options: Some(vec![
                "en".into(),
                "pl".into(),
                "fr".into(),
                "de".into(),
                "es".into(),
                "it".into(),
                "pt".into(),
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

        let lang = config
            .get("language")
            .and_then(|v| v.as_str())
            .unwrap_or("en")
            .to_ascii_lowercase();
        let q_str = query.query.as_deref().map(str::trim).unwrap_or("");
        let genre_str = query
            .genre
            .as_deref()
            .map(str::trim)
            .filter(|g| !g.is_empty() && *g != "All");
        let sort = query.sort.as_deref().unwrap_or("popular");
        let page = query.page.max(1) as usize;
        let limit = query.limit.clamp(1, 30) as usize;
        let offset = (page - 1) * limit;

        let top_scores = self.get_top_scores().await.ok();

        // Fast path for top popular / trending English browse when within top 100
        let can_use_top_fast_path = q_str.is_empty()
            && genre_str.is_none()
            && lang == "en"
            && matches!(sort, "popular" | "trending")
            && offset + limit <= 100;

        if can_use_top_fast_path {
            if let Some(ref ts) = top_scores {
                let list = if sort == "trending" {
                    &ts.last_7d
                } else {
                    &ts.last_30d
                };
                if !list.is_empty() {
                    let mem_catalog = self.catalog.read().await.as_ref().map(|(_, v)| v.clone());
                    let by_id: HashMap<u64, &CatalogBook> = mem_catalog
                        .as_ref()
                        .map(|v| v.iter().map(|b| (b.id, b)).collect())
                        .unwrap_or_default();

                    let items: Vec<NovelSummary> = list
                        .iter()
                        .skip(offset)
                        .take(limit)
                        .map(|entry| {
                            if let Some(cat) = by_id.get(&entry.id) {
                                self.catalog_book_to_summary(cat, entry.downloads)
                            } else {
                                NovelSummary {
                                    id: make_novel_id(self.id(), &entry.id.to_string()),
                                    source_id: self.id().to_string(),
                                    external_id: entry.id.to_string(),
                                    title: entry.title.clone(),
                                    author: entry.author.clone(),
                                    description: "Popular public domain work from Project Gutenberg."
                                        .to_string(),
                                    cover_url: Some(self.cover_url_for_id(entry.id)),
                                    local_cover_path: None,
                                    status: NovelStatus::Completed,
                                    genres: vec!["Classic Literature".into()],
                                    rating: None,
                                    popularity: entry.downloads,
                                    chapter_count: None,
                                    last_updated: None,
                                }
                            }
                        })
                        .collect();
                    let has_more = !items.is_empty();
                    return Ok((items, has_more, None));
                }
            }
        }

        let catalog = self.load_catalog().await?;
        let downloads_map = top_scores
            .as_ref()
            .map(|ts| &ts.downloads_by_id);

        let tokens: Vec<String> = q_str
            .to_ascii_lowercase()
            .split_whitespace()
            .map(ToString::to_string)
            .collect();
        let exact_id = q_str.parse::<u64>().ok();

        let mut matched: Vec<(&CatalogBook, i64)> = catalog
            .iter()
            .filter(|b| b.languages.is_empty() || b.languages.iter().any(|l| l == &lang))
            .filter(|b| match genre_str {
                Some(g) => Self::genre_matches(b, g),
                None => true,
            })
            .filter(|b| {
                if tokens.is_empty() {
                    return true;
                }
                if exact_id == Some(b.id) {
                    return true;
                }
                tokens.iter().all(|tok| b.search_blob_lower.contains(tok))
            })
            .map(|b| {
                let dl = downloads_map.and_then(|m| m.get(&b.id).copied()).unwrap_or(0);
                (b, dl)
            })
            .collect();

        match sort {
            "latest" => matched.sort_by(|a, b| b.0.issued.cmp(&a.0.issued).then_with(|| b.0.id.cmp(&a.0.id))),
            "title" => matched.sort_by(|a, b| a.0.title_lower.cmp(&b.0.title_lower)),
            _ => matched.sort_by(|a, b| {
                b.1.cmp(&a.1)
                    .then_with(|| {
                        if !tokens.is_empty() {
                            let a_exact = a.0.title_lower.contains(&q_str.to_ascii_lowercase())
                                || a.0.author_lower.contains(&q_str.to_ascii_lowercase());
                            let b_exact = b.0.title_lower.contains(&q_str.to_ascii_lowercase())
                                || b.0.author_lower.contains(&q_str.to_ascii_lowercase());
                            b_exact.cmp(&a_exact)
                        } else {
                            std::cmp::Ordering::Equal
                        }
                    })
                    .then_with(|| a.0.id.cmp(&b.0.id))
            }),
        }

        let total = matched.len() as u64;
        let has_more = offset + limit < matched.len();
        let items: Vec<NovelSummary> = matched
            .into_iter()
            .skip(offset)
            .take(limit)
            .map(|(b, dl)| self.catalog_book_to_summary(b, dl))
            .collect();

        Ok((items, has_more, Some(total)))
    }

    async fn get_novel(
        &self,
        external_id: &str,
        _config: &HashMap<String, Value>,
    ) -> AppResult<NovelRecord> {
        let id_clean = external_id.trim();
        let id = id_clean
            .parse::<u64>()
            .map_err(|_| AppError::InvalidInput(format!("Invalid Gutenberg ID: {id_clean}")))?;

        let url = format!("{}/ebooks/{id}", self.base_url);
        let html = self.http.get_text(&url).await.map_err(|e| AppError::Source {
            source_id: self.id().to_string(),
            message: e.to_string(),
        })?;
        Ok(self.parse_ebook_page_metadata(id, &html))
    }

    async fn get_chapters(
        &self,
        external_id: &str,
        _config: &HashMap<String, Value>,
    ) -> AppResult<Vec<Chapter>> {
        let parsed = self.ensure_parsed(external_id).await?;
        let novel_id = make_novel_id(self.id(), external_id.trim());

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

    async fn get_chapter_content(
        &self,
        external_novel_id: &str,
        external_chapter_id: &str,
        _chapter_url: Option<&str>,
        _config: &HashMap<String, Value>,
    ) -> AppResult<(String, Vec<String>, i64)> {
        let parsed = self.ensure_parsed(external_novel_id.trim()).await?;
        let ch = parsed
            .chapters
            .into_iter()
            .find(|c| c.external_id == external_chapter_id)
            .ok_or_else(|| {
                AppError::NotFound(format!("Chapter {external_chapter_id} not found in Gutenberg book"))
            })?;
        Ok((ch.html, ch.paragraphs, ch.word_count))
    }

    async fn get_translations(
        &self,
        external_id: &str,
        _config: &HashMap<String, Value>,
    ) -> AppResult<Vec<NovelTranslation>> {
        let Ok(id) = external_id.trim().parse::<u64>() else {
            return Ok(Vec::new());
        };
        let Ok(catalog) = self.load_catalog().await else {
            return Ok(Vec::new());
        };
        let Some(current) = catalog.iter().find(|b| b.id == id) else {
            return Ok(Vec::new());
        };
        let title_key = Self::normalize_work_key(&current.title);
        let author_key = Self::normalize_work_key(&current.author);
        if title_key.is_empty() {
            return Ok(Vec::new());
        }

        let cur_lang = current
            .languages
            .first()
            .cloned()
            .unwrap_or_else(|| "en".to_string());
        let mut out = vec![NovelTranslation {
            language_code: cur_lang.clone(),
            language_label: cur_lang.to_uppercase(),
            novel_id: make_novel_id(self.id(), &current.id.to_string()),
            source_id: self.id().to_string(),
            external_id: current.id.to_string(),
            title: current.title.clone(),
        }];
        let mut seen_langs = HashSet::from([cur_lang]);

        for b in &catalog {
            let Some(b_lang) = b.languages.first() else {
                continue;
            };
            if seen_langs.contains(b_lang) {
                continue;
            }
            if Self::normalize_work_key(&b.title) != title_key {
                continue;
            }
            if !author_key.is_empty()
                && author_key != "unknown author"
                && Self::normalize_work_key(&b.author) != author_key
            {
                continue;
            }
            seen_langs.insert(b_lang.clone());
            out.push(NovelTranslation {
                language_code: b_lang.clone(),
                language_label: b_lang.to_uppercase(),
                novel_id: make_novel_id(self.id(), &b.id.to_string()),
                source_id: self.id().to_string(),
                external_id: b.id.to_string(),
                title: b.title.clone(),
            });
        }
        Ok(out)
    }

    async fn health_check(&self, _config: &HashMap<String, Value>) -> AppResult<u64> {
        let url = format!("{}/browse/scores/top", self.base_url);
        self.http.probe_latency(&url).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[test]
    fn test_parse_pg_catalog_csv_multiline_and_author() {
        let csv = concat!(
            "Text#,Type,Issued,Title,Language,Authors,Subjects,LoCC,Bookshelves\n",
            "84,Text,1993-10-01,\"Frankenstein; or,\nThe Modern Prometheus\",en,\"Shelley, Mary Wollstonecraft, 1797-1851\",\"Science fiction; Horror tales\",PR,\"Gothic Fiction; Category: Science-Fiction & Fantasy\"\n",
            "99999,Sound,2020-01-01,Audio Book,en,Someone,Audio,PR,Audio\n"
        );
        let rows = GutenbergSource::parse_pg_catalog_csv(csv);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, 84);
        assert_eq!(rows[0].title, "Frankenstein; or, The Modern Prometheus");
        assert_eq!(rows[0].author, "Mary Wollstonecraft Shelley");
        assert!(rows[0].bookshelves.iter().any(|b| b.contains("Gothic Fiction")));
    }

    #[tokio::test]
    async fn test_gutenberg_official_feeds_respect_robots_and_read_chapters() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base = format!("http://{addr}");

        let csv_raw = concat!(
            "Text#,Type,Issued,Title,Language,Authors,Subjects,LoCC,Bookshelves\n",
            "84,Text,1993-10-01,\"Frankenstein; or, The Modern Prometheus\",en,\"Shelley, Mary Wollstonecraft, 1797-1851\",\"Science fiction; Horror tales\",PR,\"Science-Fiction & Fantasy\"\n",
            "1342,Text,1998-06-01,Pride and Prejudice,en,\"Austen, Jane, 1775-1817\",Romance fiction,PR,Best Books Ever Listings\n"
        );
        let mut gz = GzEncoder::new(Vec::new(), Compression::default());
        gz.write_all(csv_raw.as_bytes()).unwrap();
        let csv_gz = gz.finish().unwrap();

        let srv = tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                let csv_gz = csv_gz.clone();
                tokio::spawn(async move {
                    let mut buf = [0u8; 2048];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let path = req
                        .lines()
                        .next()
                        .and_then(|l| l.split_whitespace().nth(1))
                        .unwrap_or("/");

                    let (status, ctype, body): (&str, &str, Vec<u8>) = if path == "/robots.txt" {
                        (
                            "200 OK",
                            "text/plain",
                            b"User-agent: *\nDisallow: /ebooks/search\n".to_vec(),
                        )
                    } else if path == "/browse/scores/top" {
                        (
                            "200 OK",
                            "text/html",
                            br#"<h2 id="books-last30">Top</h2><ol><li><a href="/ebooks/1342">Pride and Prejudice by Jane Austen (234413)</a></li></ol>"#.to_vec(),
                        )
                    } else if path == "/cache/epub/feeds/pg_catalog.csv.gz" {
                        ("200 OK", "application/octet-stream", csv_gz)
                    } else if path == "/ebooks/84" {
                        (
                            "200 OK",
                            "text/html",
                            br#"<html><head><meta property="og:description" content="Victor Frankenstein's creation."></head><body><table class="bibrec"><tr><th>Author</th><td>Shelley, Mary Wollstonecraft, 1797-1851</td></tr><tr><th>Title</th><td>Frankenstein; or, The Modern Prometheus</td></tr><tr><th>Subject</th><td>Science fiction</td></tr><tr><th>Downloads</th><td>213735 downloads in the last 30 days.</td></tr></table></body></html>"#.to_vec(),
                        )
                    } else if path == "/cache/epub/84/pg84.txt" {
                        (
                            "200 OK",
                            "text/plain",
                            b"Title: Frankenstein\nAuthor: Mary Shelley\n\nChapter 1\n\nIt was on a dreary night of November.\n".to_vec(),
                        )
                    } else {
                        ("404 Not Found", "text/plain", b"Not Found".to_vec())
                    };

                    let header = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = sock.write_all(header.as_bytes()).await;
                    let _ = sock.write_all(&body).await;
                });
            }
        });

        let tmp = tempfile::tempdir().unwrap();
        let http = SourceHttpClient::new().unwrap();
        let cache = ParsedBookCache::new(tmp.path().to_path_buf());
        let src = GutenbergSource::with_base_url(http, cache, &base);

        // Popular browse uses /browse/scores/top
        let (pop, _, _) = src
            .discover(
                &DiscoveryQuery {
                    source_id: None,
                    query: None,
                    genre: None,
                    sort: Some("popular".into()),
                    status: None,
                    page: 1,
                    limit: 10,
                },
                &HashMap::new(),
            )
            .await
            .unwrap();
        assert_eq!(pop.len(), 1);
        assert_eq!(pop[0].external_id, "1342");
        assert_eq!(pop[0].popularity, 234413);

        // Search uses pg_catalog.csv.gz (never hits disallowed /ebooks/search)
        let (res, _, est) = src
            .discover(
                &DiscoveryQuery {
                    source_id: None,
                    query: Some("frankenstein".into()),
                    genre: Some("Science Fiction".into()),
                    sort: None,
                    status: None,
                    page: 1,
                    limit: 10,
                },
                &HashMap::new(),
            )
            .await
            .unwrap();
        assert_eq!(est, Some(1));
        assert_eq!(res[0].external_id, "84");

        let novel = src.get_novel("84", &HashMap::new()).await.unwrap();
        assert_eq!(novel.author, "Mary Wollstonecraft Shelley");
        assert_eq!(novel.popularity, 213735);

        let chs = src.get_chapters("84", &HashMap::new()).await.unwrap();
        assert_eq!(chs.len(), 2);
        assert_eq!(chs[1].title, "Chapter 1");
        let (_, paras, _) = src
            .get_chapter_content("84", &chs[1].external_id, None, &HashMap::new())
            .await
            .unwrap();
        assert!(paras.join(" ").contains("dreary night of November"));

        assert!(src.health_check(&HashMap::new()).await.is_ok());
        srv.abort();
    }
}
