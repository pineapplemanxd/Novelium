use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::RwLock;

use crate::error::{AppError, AppResult};
use crate::models::{
    Chapter, DiscoveryQuery, NovelRecord, NovelStatus, NovelSummary, SourceCapabilities,
};
use crate::net::SourceHttpClient;
use crate::sources::archive_org::{
    ensure_ia_item_parsed, iso2_to_ia_code, normalize_ia_lang_str,
};
use crate::sources::{
    make_chapter_id, make_novel_id, CachedParsedBook, NovelSource, ParsedBookCache,
};

pub struct OpenLibrarySource {
    http: SourceHttpClient,
    // Open Library's official JSON Search REST API (/search.json) shares the /search
    // path prefix that robots.txt disallows for HTML crawlers; rate limits remain active.
    search_api_http: SourceHttpClient,
    cache: ParsedBookCache,
    ol_base_url: String,
    covers_base_url: String,
    ia_base_url: String,
    ia_candidates_cache: Arc<RwLock<HashMap<String, Vec<String>>>>,
}

impl OpenLibrarySource {
    pub fn new(http: SourceHttpClient, cache: ParsedBookCache) -> Self {
        Self::with_base_urls(
            http,
            cache,
            "https://openlibrary.org",
            "https://covers.openlibrary.org",
            "https://archive.org",
        )
    }

    pub fn with_base_urls(
        http: SourceHttpClient,
        cache: ParsedBookCache,
        ol_base_url: &str,
        covers_base_url: &str,
        ia_base_url: &str,
    ) -> Self {
        let search_api_http = http.clone().with_robots_enforcement(false);
        Self {
            http,
            search_api_http,
            cache,
            ol_base_url: ol_base_url.trim_end_matches('/').to_string(),
            covers_base_url: covers_base_url.trim_end_matches('/').to_string(),
            ia_base_url: ia_base_url.trim_end_matches('/').to_string(),
            ia_candidates_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    fn normalize_work_id(raw: &str) -> String {
        raw.trim()
            .trim_start_matches('/')
            .strip_prefix("works/")
            .unwrap_or_else(|| raw.trim().trim_start_matches('/'))
            .trim_end_matches(".json")
            .to_string()
    }

    fn cover_url_from_id(&self, cover_id: Option<i64>) -> Option<String> {
        cover_id
            .filter(|id| *id > 0)
            .map(|id| format!("{}/b/id/{id}-L.jpg", self.covers_base_url))
    }

    fn extract_description(val: Option<&Value>) -> String {
        match val {
            Some(Value::String(s)) => s.trim().to_string(),
            Some(Value::Object(obj)) => obj
                .get("value")
                .and_then(Value::as_str)
                .map(|s| s.trim().to_string())
                .unwrap_or_default(),
            _ => String::new(),
        }
    }

    fn pick_summary_language(langs: &[String], req_lang: Option<&str>) -> Option<String> {
        let normalized: Vec<String> = langs
            .iter()
            .filter_map(|l| normalize_ia_lang_str(l))
            .collect();
        if let Some(req) = req_lang {
            if normalized.iter().any(|l| l == req) {
                return Some(req.to_string());
            }
        }
        if normalized.iter().any(|l| l == "en") {
            return Some("en".to_string());
        }
        normalized.into_iter().next().or_else(|| req_lang.map(str::to_string))
    }

    fn prioritize_ia_candidates(raw: &[String]) -> Vec<String> {
        let mut items: Vec<(u8, String)> = raw
            .iter()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .filter(|s| {
                let low = s.to_ascii_lowercase();
                !low.starts_with("bwb_")
                    && !low.starts_with("isbn_")
                    && !low.contains("_librivox")
            })
            .map(|s| {
                let low = s.to_ascii_lowercase();
                let score = if low.ends_with("gut") {
                    0u8
                } else if !low.contains("0000") {
                    1u8
                } else {
                    2u8
                };
                (score, s.to_string())
            })
            .collect();
        items.sort_by_key(|(score, _)| *score);
        items.into_iter().map(|(_, s)| s).take(25).collect()
    }

    async fn fetch_work_doc(&self, work_id: &str) -> AppResult<Option<OlSearchDoc>> {
        let mut url = url::Url::parse(&format!("{}/search.json", self.ol_base_url))?;
        url.query_pairs_mut()
            .append_pair("q", &format!("key:\"/works/{work_id}\""))
            .append_pair(
                "fields",
                "key,title,author_name,cover_i,ia,first_publish_year,subject,language,edition_count,ebook_access",
            )
            .append_pair("limit", "1");
        let resp: OlSearchResp = self.search_api_http.get_json(url.as_str()).await?;
        Ok(resp.docs.into_iter().next())
    }

    async fn collect_ia_candidates(&self, work_id: &str) -> AppResult<Vec<String>> {
        {
            let guard = self.ia_candidates_cache.read().await;
            if let Some(list) = guard.get(work_id) {
                if !list.is_empty() {
                    return Ok(list.clone());
                }
            }
        }

        let mut raw = Vec::new();
        if let Ok(Some(doc)) = self.fetch_work_doc(work_id).await {
            raw.extend(doc.ia);
        }
        if raw.is_empty() {
            let editions_url = format!("{}/works/{work_id}/editions.json?limit=25", self.ol_base_url);
            if let Ok(editions) = self.http.get_json::<OlEditionsResp>(&editions_url).await {
                for e in editions.entries {
                    if let Some(ocaid) = e.ocaid {
                        raw.push(ocaid);
                    }
                }
            }
        }

        let prioritized = Self::prioritize_ia_candidates(&raw);
        if !prioritized.is_empty() {
            let mut guard = self.ia_candidates_cache.write().await;
            if guard.len() > 1000 {
                guard.clear();
            }
            guard.insert(work_id.to_string(), prioritized.clone());
        }
        Ok(prioritized)
    }

    async fn ensure_parsed(&self, external_id: &str) -> AppResult<CachedParsedBook> {
        let work_id = Self::normalize_work_id(external_id);
        let novel_id = make_novel_id(self.id(), &work_id);
        if let Some(cached) = self.cache.get(&novel_id) {
            if !cached.chapters.is_empty() {
                return Ok(cached);
            }
        }

        let candidates = self.collect_ia_candidates(&work_id).await?;
        if candidates.is_empty() {
            return Err(AppError::NotFound(format!(
                "No public-domain Archive.org editions found for Open Library work {work_id}"
            )));
        }

        let id_expr = candidates
            .iter()
            .map(|id| format!("\"{}\"", id.replace('"', "")))
            .collect::<Vec<_>>()
            .join(" OR ");
        let q = format!(
            "identifier:({id_expr}) AND mediatype:texts AND (format:EPUB OR format:Text) AND -collection:inlibrary AND -collection:printdisabled"
        );
        let mut search_url = url::Url::parse(&format!("{}/advancedsearch.php", self.ia_base_url))?;
        search_url
            .query_pairs_mut()
            .append_pair("q", &q)
            .append_pair("fl[]", "identifier")
            .append_pair("fl[]", "format")
            .append_pair("sort[]", "downloads desc")
            .append_pair("rows", "10")
            .append_pair("output", "json");

        let ia_hits: IaResolvedResp = self.http.get_json(search_url.as_str()).await?;
        let mut docs = ia_hits.response.docs;
        docs.sort_by_key(|d| {
            let has_epub = d.formats().iter().any(|f| f.eq_ignore_ascii_case("EPUB"));
            if has_epub { 0u8 } else { 1u8 }
        });

        for doc in docs {
            if let Ok(parsed) = ensure_ia_item_parsed(
                &self.http,
                &self.cache,
                &self.ia_base_url,
                &novel_id,
                &doc.identifier,
            )
            .await
            {
                return Ok(parsed);
            }
        }

        Err(AppError::NotFound(format!(
            "This Open Library work ({work_id}) has no freely readable public-domain EPUB/TXT copy available"
        )))
    }
}

#[derive(Debug, Deserialize)]
struct OlSearchResp {
    #[serde(default, rename = "numFound")]
    num_found: u64,
    #[serde(default)]
    docs: Vec<OlSearchDoc>,
}

#[derive(Debug, Deserialize)]
struct OlSearchDoc {
    key: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    author_name: Vec<String>,
    #[serde(default)]
    cover_i: Option<i64>,
    #[serde(default)]
    ia: Vec<String>,
    #[serde(default)]
    first_publish_year: Option<i64>,
    #[serde(default)]
    subject: Vec<String>,
    #[serde(default)]
    language: Vec<String>,
    #[serde(default)]
    edition_count: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct OlWorkJson {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    description: Option<Value>,
    #[serde(default)]
    subjects: Vec<String>,
    #[serde(default)]
    covers: Vec<i64>,
}

#[derive(Debug, Deserialize)]
struct OlEditionsResp {
    #[serde(default)]
    entries: Vec<OlEditionEntry>,
}

#[derive(Debug, Deserialize)]
struct OlEditionEntry {
    #[serde(default)]
    ocaid: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IaResolvedResp {
    response: IaResolvedInner,
}

#[derive(Debug, Deserialize)]
struct IaResolvedInner {
    #[serde(default)]
    docs: Vec<IaResolvedDoc>,
}

#[derive(Debug, Deserialize)]
struct IaResolvedDoc {
    identifier: String,
    #[serde(default)]
    format: Option<Value>,
}

impl IaResolvedDoc {
    fn formats(&self) -> Vec<String> {
        match self.format.as_ref() {
            Some(Value::String(s)) => vec![s.clone()],
            Some(Value::Array(arr)) => arr
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect(),
            _ => Vec::new(),
        }
    }
}

#[async_trait]
impl NovelSource for OpenLibrarySource {
    fn id(&self) -> &str {
        "openlibrary"
    }

    fn name(&self) -> &str {
        "Open Library"
    }

    fn kind(&self) -> &str {
        "builtin"
    }

    fn base_url(&self) -> &str {
        "https://openlibrary.org"
    }

    fn description(&self) -> &str {
        "Open-access public domain & readable books cataloged on Open Library."
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
                "Adventure".into(),
                "Romance".into(),
                "Horror".into(),
                "Historical".into(),
                "Philosophy".into(),
                "Poetry".into(),
                "Short Stories".into(),
            ],
        }
    }

    async fn discover(
        &self,
        query: &DiscoveryQuery,
        _config: &HashMap<String, Value>,
    ) -> AppResult<(Vec<NovelSummary>, bool, Option<u64>)> {
        if matches!(query.status.as_deref(), Some("ongoing")) {
            return Ok((Vec::new(), false, Some(0)));
        }

        let mut parts = vec!["ebook_access:public".to_string()];
        if let Some(q) = query.query.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            parts.push(q.replace('"', ""));
        }
        if let Some(g) = query
            .genre
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty() && *s != "All")
        {
            let safe_g = g.replace('"', "");
            if safe_g.contains(' ') {
                parts.push(format!("subject:\"{safe_g}\""));
            } else {
                parts.push(format!("subject:{}", safe_g.to_ascii_lowercase()));
            }
        } else if query.query.as_deref().map(str::trim).unwrap_or("").is_empty() {
            parts.push("subject:fiction".to_string());
        }

        let req_lang = query
            .language
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("all"))
            .map(str::to_ascii_lowercase);
        if let Some(ref lang_code) = req_lang {
            let iso3 = iso2_to_ia_code(lang_code);
            if !iso3.is_empty() {
                parts.push(format!("language:{iso3}"));
            }
        }

        let limit = query.limit.clamp(1, 30);
        let page = query.page.max(1);
        let sort = match query.sort.as_deref() {
            Some("latest") => "new",
            Some("title") => "title",
            _ => "editions",
        };

        let mut url = url::Url::parse(&format!("{}/search.json", self.ol_base_url))?;
        url.query_pairs_mut()
            .append_pair("q", &parts.join(" "))
            .append_pair(
                "fields",
                "key,title,author_name,cover_i,ia,first_publish_year,subject,language,edition_count,ebook_access",
            )
            .append_pair("sort", sort)
            .append_pair("limit", &limit.to_string())
            .append_pair("page", &page.to_string());

        let resp: OlSearchResp = self.search_api_http.get_json(url.as_str()).await?;
        let has_more = (page as u64) * (limit as u64) < resp.num_found;

        let mut candidate_updates = Vec::new();
        let items = resp
            .docs
            .into_iter()
            .filter_map(|doc| {
                let work_id = Self::normalize_work_id(&doc.key);
                if work_id.is_empty() {
                    return None;
                }
                let ia_list = Self::prioritize_ia_candidates(&doc.ia);
                if !ia_list.is_empty() {
                    candidate_updates.push((work_id.clone(), ia_list));
                }
                let author = if doc.author_name.is_empty() {
                    "Unknown Author".to_string()
                } else {
                    doc.author_name.join(", ")
                };
                let genres: Vec<String> = doc.subject.into_iter().take(5).collect();
                let lang = Self::pick_summary_language(&doc.language, req_lang.as_deref());
                let title = doc
                    .title
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .unwrap_or(&work_id)
                    .to_string();

                Some(NovelSummary {
                    id: make_novel_id(self.id(), &work_id),
                    source_id: self.id().to_string(),
                    external_id: work_id,
                    title,
                    author,
                    description: "Public-domain edition cataloged by Open Library.".to_string(),
                    cover_url: self.cover_url_from_id(doc.cover_i),
                    local_cover_path: None,
                    status: NovelStatus::Completed,
                    genres,
                    rating: None,
                    popularity: doc.edition_count.unwrap_or(0),
                    chapter_count: None,
                    last_updated: doc.first_publish_year.map(|y| y.to_string()),
                    language: lang,
                })
            })
            .collect();

        if !candidate_updates.is_empty() {
            let mut guard = self.ia_candidates_cache.write().await;
            if guard.len() > 1000 {
                guard.clear();
            }
            for (k, v) in candidate_updates {
                guard.insert(k, v);
            }
        }

        Ok((items, has_more, Some(resp.num_found)))
    }

    async fn get_novel(
        &self,
        external_id: &str,
        _config: &HashMap<String, Value>,
    ) -> AppResult<NovelRecord> {
        let work_id = Self::normalize_work_id(external_id);
        let work_url = format!("{}/works/{work_id}.json", self.ol_base_url);
        let work = self.http.get_json::<OlWorkJson>(&work_url).await.ok();
        let doc = self.fetch_work_doc(&work_id).await.ok().flatten();

        if work.is_none() && doc.is_none() {
            return Err(AppError::NotFound(format!(
                "Open Library work {work_id} not found"
            )));
        }

        if let Some(ref d) = doc {
            let ia_list = Self::prioritize_ia_candidates(&d.ia);
            if !ia_list.is_empty() {
                let mut guard = self.ia_candidates_cache.write().await;
                guard.insert(work_id.clone(), ia_list);
            }
        }

        let title = work
            .as_ref()
            .and_then(|w| w.title.as_deref())
            .or_else(|| doc.as_ref().and_then(|d| d.title.as_deref()))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(&work_id)
            .to_string();

        let author = doc
            .as_ref()
            .filter(|d| !d.author_name.is_empty())
            .map(|d| d.author_name.join(", "))
            .unwrap_or_else(|| "Unknown Author".to_string());

        let description = work
            .as_ref()
            .map(|w| Self::extract_description(w.description.as_ref()))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Public-domain edition cataloged by Open Library.".to_string());

        let cover_id = doc
            .as_ref()
            .and_then(|d| d.cover_i)
            .or_else(|| work.as_ref().and_then(|w| w.covers.iter().copied().find(|c| *c > 0)));

        let genres: Vec<String> = work
            .as_ref()
            .filter(|w| !w.subjects.is_empty())
            .map(|w| w.subjects.iter().take(5).cloned().collect())
            .or_else(|| doc.as_ref().map(|d| d.subject.iter().take(5).cloned().collect()))
            .unwrap_or_default();

        let lang = doc
            .as_ref()
            .and_then(|d| Self::pick_summary_language(&d.language, None))
            .unwrap_or_else(|| "en".to_string());

        let now = Utc::now().to_rfc3339();
        Ok(NovelRecord {
            id: make_novel_id(self.id(), &work_id),
            source_id: self.id().to_string(),
            external_id: work_id,
            title,
            author_id: None,
            author,
            description,
            cover_url: self.cover_url_from_id(cover_id),
            local_cover_path: None,
            status: NovelStatus::Completed,
            genres,
            tags: vec!["Open Library".into()],
            rating: None,
            popularity: doc.as_ref().and_then(|d| d.edition_count).unwrap_or(0),
            chapter_count: 0,
            last_updated_remote: doc
                .as_ref()
                .and_then(|d| d.first_publish_year.map(|y| y.to_string())),
            metadata: serde_json::json!({ "language": lang }),
            is_local: false,
            created_at: now.clone(),
            updated_at: now,
        })
    }

    async fn get_chapters(
        &self,
        external_id: &str,
        _config: &HashMap<String, Value>,
    ) -> AppResult<Vec<Chapter>> {
        let parsed = self.ensure_parsed(external_id).await?;
        let work_id = Self::normalize_work_id(external_id);
        let novel_id = make_novel_id(self.id(), &work_id);

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
        let parsed = self.ensure_parsed(external_novel_id).await?;
        let ch = parsed
            .chapters
            .into_iter()
            .find(|c| c.external_id == external_chapter_id)
            .ok_or_else(|| {
                AppError::NotFound(format!("Chapter {external_chapter_id} not found"))
            })?;
        Ok((ch.html, ch.paragraphs, ch.word_count))
    }

    async fn health_check(&self, _config: &HashMap<String, Value>) -> AppResult<u64> {
        let url = format!("{}/subjects/fiction.json?limit=1", self.ol_base_url);
        self.http.probe_latency(&url).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[test]
    fn test_prioritize_ia_candidates_prefers_clean_gutenberg_and_skips_bwb() {
        let raw = vec![
            "bwb_KS-179-237".into(),
            "pride_librivox".into(),
            "pride0000jane".into(),
            "prideandprejudic42671gut".into(),
            "pride00aust".into(),
        ];
        assert_eq!(
            OpenLibrarySource::prioritize_ia_candidates(&raw),
            vec!["prideandprejudic42671gut", "pride00aust", "pride0000jane"]
        );
    }

    #[tokio::test]
    async fn test_openlibrary_discover_details_and_txt_chapters() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());

        let srv = tokio::spawn(async move {
            loop {
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

                    let (ctype, body) = if path.starts_with("/robots.txt") {
                        ("text/plain", "User-agent: *\nAllow: /\n".to_string())
                    } else if path.starts_with("/search.json") {
                        (
                            "application/json",
                            serde_json::json!({
                                "numFound": 1,
                                "docs": [{
                                    "key": "/works/OL66554W",
                                    "title": "Pride and Prejudice",
                                    "author_name": ["Jane Austen"],
                                    "cover_i": 14348537,
                                    "ia": ["bwb_X1", "pride42671gut"],
                                    "first_publish_year": 1813,
                                    "subject": ["Fiction", "Romance"],
                                    "language": ["eng", "pol"],
                                    "edition_count": 4042
                                }]
                            })
                            .to_string(),
                        )
                    } else if path.starts_with("/works/OL66554W.json") {
                        (
                            "application/json",
                            serde_json::json!({
                                "title": "Pride and Prejudice",
                                "description": { "type": "/type/text", "value": "Classic Regency novel." },
                                "subjects": ["Fiction", "Romance"],
                                "covers": [14348537]
                            })
                            .to_string(),
                        )
                    } else if path.starts_with("/advancedsearch.php") {
                        (
                            "application/json",
                            serde_json::json!({
                                "response": {
                                    "docs": [{
                                        "identifier": "pride42671gut",
                                        "format": ["Text", "Metadata"]
                                    }]
                                }
                            })
                            .to_string(),
                        )
                    } else if path.starts_with("/metadata/pride42671gut") {
                        (
                            "application/json",
                            serde_json::json!({
                                "files": [{
                                    "name": "pride.txt",
                                    "size": "120"
                                }]
                            })
                            .to_string(),
                        )
                    } else if path.starts_with("/download/pride42671gut/pride.txt") {
                        (
                            "text/plain",
                            "Chapter 1\n\nIt is a truth universally acknowledged.\n\nChapter 2\n\nMr. Bennet was among the earliest.\n".to_string(),
                        )
                    } else {
                        ("application/json", "{}".to_string())
                    };

                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = sock.write_all(resp.as_bytes()).await;
                });
            }
        });

        let tmp = tempfile::tempdir().unwrap();
        let cache = ParsedBookCache::new(tmp.path().to_path_buf());
        let http = SourceHttpClient::new().unwrap();
        let src = OpenLibrarySource::with_base_urls(http, cache, &base, &base, &base);
        let cfg = HashMap::new();

        let (items, has_more, total) = src
            .discover(
                &DiscoveryQuery {
                    source_id: Some("openlibrary".into()),
                    query: None,
                    genre: None,
                    sort: Some("popular".into()),
                    status: None,
                    language: Some("pl".into()),
                    page: 1,
                    limit: 20,
                },
                &cfg,
            )
            .await
            .unwrap();

        assert!(!has_more);
        assert_eq!(total, Some(1));
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "openlibrary:OL66554W");
        assert_eq!(items[0].author, "Jane Austen");
        assert_eq!(items[0].language.as_deref(), Some("pl"));

        let novel = src.get_novel("OL66554W", &cfg).await.unwrap();
        assert_eq!(novel.description, "Classic Regency novel.");
        assert_eq!(novel.author, "Jane Austen");

        let chapters = src.get_chapters("OL66554W", &cfg).await.unwrap();
        assert!(chapters.len() >= 2);
        let (html, paras, wc) = src
            .get_chapter_content("OL66554W", &chapters[0].external_id, None, &cfg)
            .await
            .unwrap();
        assert!(!html.is_empty() && !paras.is_empty() && wc > 0);

        srv.abort();
    }
}
