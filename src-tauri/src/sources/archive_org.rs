use std::collections::HashMap;

use async_trait::async_trait;
use chrono::Utc;
use serde::Deserialize;
use serde_json::Value;

use crate::error::{AppError, AppResult};
use crate::formats::epub::parse_epub_bytes;
use crate::formats::text_html::parse_txt_bytes;
use crate::models::{
    Chapter, DiscoveryQuery, NovelRecord, NovelStatus, NovelSummary, SourceCapabilities,
};
use crate::net::SourceHttpClient;
use crate::sources::{make_chapter_id, make_novel_id, CachedParsedBook, NovelSource, ParsedBookCache};

pub struct ArchiveOrgSource {
    http: SourceHttpClient,
    cache: ParsedBookCache,
}

impl ArchiveOrgSource {
    pub fn new(http: SourceHttpClient, cache: ParsedBookCache) -> Self {
        Self { http, cache }
    }

    async fn ensure_parsed(&self, identifier: &str) -> AppResult<CachedParsedBook> {
        let novel_id = make_novel_id(self.id(), identifier);
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

        let meta_url = format!("https://archive.org/metadata/{identifier}");
        let meta: IaMetadataResp = self.http.get_json(&meta_url).await?;

        const MAX_EPUB_BYTES: u64 = 25 * 1024 * 1024;
        const MAX_TXT_BYTES: u64 = 15 * 1024 * 1024;

        let mut epub_candidates: Vec<(&IaFile, u64)> = meta
            .files
            .iter()
            .filter(|f| {
                let low = f.name.to_ascii_lowercase();
                low.ends_with(".epub")
                    && !low.contains("_encrypted")
                    && f.private.as_deref() != Some("true")
            })
            .map(|f| (f, f.size_bytes().unwrap_or(0)))
            .filter(|(_, sz)| *sz <= MAX_EPUB_BYTES)
            .collect();
        epub_candidates.sort_by_key(|(_, sz)| *sz);

        let mut txt_candidates: Vec<(&IaFile, u64)> = meta
            .files
            .iter()
            .filter(|f| {
                let low = f.name.to_ascii_lowercase();
                low.ends_with(".txt")
                    && !low.ends_with("_files.xml")
                    && f.private.as_deref() != Some("true")
            })
            .map(|f| (f, f.size_bytes().unwrap_or(0)))
            .filter(|(_, sz)| *sz <= MAX_TXT_BYTES)
            .collect();
        // Prefer clean non-djvu plain text first, then smallest _djvu.txt
        txt_candidates.sort_by_key(|(f, sz)| {
            let is_djvu = f.name.to_ascii_lowercase().ends_with("_djvu.txt");
            (if is_djvu { 1u8 } else { 0u8 }, *sz)
        });

        let mut parsed = None;
        if let Some((f, _)) = epub_candidates.first() {
            let dl_url = build_download_url(identifier, &f.name)?;
            let bytes = self.http.get_bytes(&dl_url).await?;
            if let Ok(book) = parse_epub_bytes(&bytes, identifier) {
                if !book.chapters.is_empty() {
                    parsed = Some(book);
                }
            }
        }
        if parsed.is_none() {
            if let Some((f, _)) = txt_candidates.first() {
                let dl_url = build_download_url(identifier, &f.name)?;
                let bytes = self.http.get_bytes(&dl_url).await?;
                parsed = Some(parse_txt_bytes(&bytes, identifier)?);
            }
        }

        let parsed = parsed.ok_or_else(|| {
            AppError::NotFound(format!(
                "No readable open EPUB (<=25MB) or TXT file available for Internet Archive item {identifier}"
            ))
        })?;

        self.cache.put(&novel_id, &parsed)
    }
}

fn build_download_url(identifier: &str, fname: &str) -> AppResult<String> {
    let mut url = url::Url::parse("https://archive.org/download")?;
    {
        let mut segs = url
            .path_segments_mut()
            .map_err(|_| AppError::InvalidInput("Invalid Archive.org URL".into()))?;
        segs.push(identifier);
        for part in fname.split('/').filter(|s| !s.is_empty()) {
            segs.push(part);
        }
    }
    Ok(url.into())
}

#[derive(Debug, Deserialize)]
struct IaSearchResp {
    response: IaSearchInner,
}

#[derive(Debug, Deserialize)]
struct IaSearchInner {
    #[serde(rename = "numFound")]
    num_found: u64,
    docs: Vec<IaDoc>,
}

#[derive(Debug, Deserialize)]
struct IaDoc {
    identifier: String,
    #[serde(default)]
    title: Option<Value>,
    #[serde(default)]
    creator: Option<Value>,
    #[serde(default)]
    description: Option<Value>,
    #[serde(default)]
    subject: Option<Value>,
    #[serde(default)]
    language: Option<Value>,
    #[serde(default)]
    downloads: Option<i64>,
    #[serde(default)]
    date: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IaMetadataResp {
    #[serde(default)]
    metadata: Option<IaDocMeta>,
    #[serde(default)]
    files: Vec<IaFile>,
}

#[derive(Debug, Deserialize)]
struct IaDocMeta {
    #[serde(default)]
    title: Option<Value>,
    #[serde(default)]
    creator: Option<Value>,
    #[serde(default)]
    description: Option<Value>,
    #[serde(default)]
    subject: Option<Value>,
    #[serde(default)]
    language: Option<Value>,
    #[serde(default)]
    date: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IaFile {
    name: String,
    #[serde(default)]
    private: Option<String>,
    #[serde(default)]
    size: Option<Value>,
}

impl IaFile {
    fn size_bytes(&self) -> Option<u64> {
        match self.size.as_ref()? {
            Value::Number(n) => n.as_u64(),
            Value::String(s) => s.trim().parse::<u64>().ok(),
            _ => None,
        }
    }
}

fn val_to_string(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|x| x.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        _ => String::new(),
    }
}

fn val_to_vec(v: Option<&Value>) -> Vec<String> {
    match v {
        Some(Value::String(s)) => s
            .split([';', ','])
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .take(5)
            .collect(),
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|x| x.as_str().map(|s| s.trim().to_string()))
            .filter(|s| !s.is_empty())
            .take(5)
            .collect(),
        _ => Vec::new(),
    }
}

fn normalize_ia_lang_str(raw: &str) -> Option<String> {
    let first = raw
        .split([',', ';'])
        .next()
        .map(str::trim)
        .unwrap_or("")
        .to_ascii_lowercase();
    if first.is_empty() {
        return None;
    }
    let code = match first.as_str() {
        "eng" | "english" | "en" => "en",
        "pol" | "polish" | "polski" | "pl" => "pl",
        "fre" | "fra" | "french" | "français" | "fr" => "fr",
        "ger" | "deu" | "german" | "deutsch" | "de" => "de",
        "spa" | "spanish" | "español" | "es" => "es",
        "ita" | "italian" | "italiano" | "it" => "it",
        "por" | "portuguese" | "português" | "pt" => "pt",
        "rus" | "russian" | "ru" => "ru",
        "jpn" | "japanese" | "ja" => "ja",
        "chi" | "zho" | "chinese" | "zh" => "zh",
        "ind" | "indonesian" | "id" => "id",
        other if other.len() == 2 => other,
        _ => return Some(first),
    };
    Some(code.to_string())
}

fn normalize_ia_lang_val(v: Option<&Value>) -> Option<String> {
    let s = val_to_string(v);
    normalize_ia_lang_str(&s)
}

fn ia_language_clause(code: &str) -> String {
    match code.trim().to_ascii_lowercase().as_str() {
        "en" => "(language:eng OR language:English OR language:en)".into(),
        "pl" => "(language:pol OR language:Polish OR language:pl)".into(),
        "fr" => "(language:fre OR language:fra OR language:French OR language:fr)".into(),
        "de" => "(language:ger OR language:deu OR language:German OR language:de)".into(),
        "es" => "(language:spa OR language:Spanish OR language:es)".into(),
        "it" => "(language:ita OR language:Italian OR language:it)".into(),
        "pt" => "(language:por OR language:Portuguese OR language:pt)".into(),
        "ru" => "(language:rus OR language:Russian OR language:ru)".into(),
        "ja" => "(language:jpn OR language:Japanese OR language:ja)".into(),
        "zh" => "(language:chi OR language:zho OR language:Chinese OR language:zh)".into(),
        "id" => "(language:ind OR language:Indonesian OR language:id)".into(),
        other => {
            let safe: String = other
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
                .collect();
            format!("language:({safe})")
        }
    }
}

#[async_trait]
impl NovelSource for ArchiveOrgSource {
    fn id(&self) -> &str {
        "archive_org"
    }

    fn name(&self) -> &str {
        "Internet Archive (Open Texts)"
    }

    fn kind(&self) -> &str {
        "builtin"
    }

    fn base_url(&self) -> &str {
        "https://archive.org"
    }

    fn description(&self) -> &str {
        "Public domain & open-access fiction EPUBs from Internet Archive's open text collections."
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

        let mut q_parts = vec![
            "mediatype:texts".to_string(),
            "format:EPUB".to_string(),
            "(collection:americana OR collection:library_of_congress OR collection:universallibrary OR collection:bostonpubliclibrary OR collection:toronto OR collection:gutenberg)".to_string(),
            "-collection:inlibrary".to_string(),
            "-collection:printdisabled".to_string(),
            "-collection:folkscanomy".to_string(),
        ];

        if let Some(q) = query.query.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            let safe = q.replace('"', "");
            q_parts.push(format!("(title:(\"{safe}\") OR creator:(\"{safe}\") OR \"{safe}\")"));
        }
        if let Some(g) = query.genre.as_deref().map(str::trim).filter(|s| !s.is_empty() && *s != "All") {
            let safe_g = g.replace('"', "");
            q_parts.push(format!("subject:(\"{safe_g}\")"));
        } else if query.query.as_deref().map(str::trim).unwrap_or("").is_empty() {
            q_parts.push("subject:(\"fiction\")".to_string());
        }
        let req_lang = query
            .language
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("all"))
            .map(str::to_ascii_lowercase);
        if let Some(ref lang_code) = req_lang {
            q_parts.push(ia_language_clause(lang_code));
        }

        let sort = match query.sort.as_deref() {
            Some("latest") => "addeddate desc",
            Some("title") => "titleSorter asc",
            _ => "downloads desc",
        };
        let rows = query.limit.clamp(1, 30);
        let page = query.page.max(1);

        let mut url = url::Url::parse("https://archive.org/advancedsearch.php")?;
        url.query_pairs_mut()
            .append_pair("q", &q_parts.join(" AND "))
            .append_pair("fl[]", "identifier")
            .append_pair("fl[]", "title")
            .append_pair("fl[]", "creator")
            .append_pair("fl[]", "description")
            .append_pair("fl[]", "subject")
            .append_pair("fl[]", "language")
            .append_pair("fl[]", "downloads")
            .append_pair("fl[]", "date")
            .append_pair("sort[]", sort)
            .append_pair("rows", &rows.to_string())
            .append_pair("page", &page.to_string())
            .append_pair("output", "json");

        let resp: IaSearchResp = self.http.get_json(url.as_str()).await?;
        let num_found = resp.response.num_found;
        let has_more = (page as u64) * (rows as u64) < num_found;

        let items = resp
            .response
            .docs
            .into_iter()
            .map(|doc| {
                let title = {
                    let s = val_to_string(doc.title.as_ref());
                    if s.is_empty() {
                        doc.identifier.clone()
                    } else {
                        s
                    }
                };
                let author = {
                    let a = val_to_string(doc.creator.as_ref());
                    if a.is_empty() {
                        "Unknown Author".into()
                    } else {
                        a
                    }
                };
                let desc = val_to_string(doc.description.as_ref());
                let genres = val_to_vec(doc.subject.as_ref());
                let lang = normalize_ia_lang_val(doc.language.as_ref())
                    .or_else(|| req_lang.clone());

                NovelSummary {
                    id: make_novel_id(self.id(), &doc.identifier),
                    source_id: self.id().to_string(),
                    external_id: doc.identifier.clone(),
                    title,
                    author,
                    description: if desc.is_empty() {
                        "Public domain EPUB hosted at the Internet Archive.".into()
                    } else {
                        desc
                    },
                    cover_url: Some(format!("https://archive.org/services/img/{}", doc.identifier)),
                    local_cover_path: None,
                    status: NovelStatus::Completed,
                    genres,
                    rating: None,
                    popularity: doc.downloads.unwrap_or(0),
                    chapter_count: None,
                    last_updated: doc.date,
                    language: lang,
                }
            })
            .collect();

        Ok((items, has_more, Some(num_found)))
    }

    async fn get_novel(
        &self,
        external_id: &str,
        _config: &HashMap<String, Value>,
    ) -> AppResult<NovelRecord> {
        let meta_url = format!("https://archive.org/metadata/{external_id}");
        let meta: IaMetadataResp = self.http.get_json(&meta_url).await?;
        let dm = meta.metadata.as_ref();
        let title = val_to_string(dm.and_then(|m| m.title.as_ref()));
        let author = val_to_string(dm.and_then(|m| m.creator.as_ref()));
        let desc = val_to_string(dm.and_then(|m| m.description.as_ref()));
        let genres = val_to_vec(dm.and_then(|m| m.subject.as_ref()));
        let now = Utc::now().to_rfc3339();

        Ok(NovelRecord {
            id: make_novel_id(self.id(), external_id),
            source_id: self.id().to_string(),
            external_id: external_id.to_string(),
            title: if title.is_empty() {
                external_id.to_string()
            } else {
                title
            },
            author_id: None,
            author: if author.is_empty() {
                "Unknown Author".into()
            } else {
                author
            },
            description: desc,
            cover_url: Some(format!("https://archive.org/services/img/{external_id}")),
            local_cover_path: None,
            status: NovelStatus::Completed,
            genres,
            tags: vec!["Internet Archive".into()],
            rating: None,
            popularity: 0,
            chapter_count: 0,
            last_updated_remote: dm.and_then(|m| m.date.clone()),
            metadata: serde_json::json!({
                "identifier": external_id,
                "language": normalize_ia_lang_val(dm.and_then(|m| m.language.as_ref())).unwrap_or_default(),
            }),
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
        let novel_id = make_novel_id(self.id(), external_id);

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
            .ok_or_else(|| AppError::NotFound(format!("Chapter {external_chapter_id} not found")))?;
        Ok((ch.html, ch.paragraphs, ch.word_count))
    }

    async fn health_check(&self, _config: &HashMap<String, Value>) -> AppResult<u64> {
        self.http
            .probe_latency("https://archive.org/advancedsearch.php?q=mediatype:texts&rows=1&output=json")
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_download_url_percent_encodes_spaces_and_special_chars() {
        let u = build_download_url("sherlock_01", "sub dir/My Book #1 & Notes.epub").unwrap();
        assert_eq!(
            u,
            "https://archive.org/download/sherlock_01/sub%20dir/My%20Book%20%231%20&%20Notes.epub"
        );
    }

    #[test]
    fn test_ia_file_size_parses_string_or_number() {
        let f_str = IaFile {
            name: "book.epub".into(),
            private: None,
            size: Some(Value::String("123456".into())),
        };
        let f_num = IaFile {
            name: "book2.epub".into(),
            private: None,
            size: Some(serde_json::json!(654321)),
        };
        assert_eq!(f_str.size_bytes(), Some(123456));
        assert_eq!(f_num.size_bytes(), Some(654321));
    }

    #[tokio::test]
    #[ignore = "live network check against archive.org; run manually with --ignored"]
    async fn test_live_archive_org_discover_and_download() {
        let dir = std::env::temp_dir().join(format!("novelium_ia_live_{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&dir);
        let http = SourceHttpClient::new().unwrap();
        let cache = ParsedBookCache::new(dir.clone());
        let src = ArchiveOrgSource::new(http, cache);
        let cfg = HashMap::new();

        let (items, _has_more, total) = src
            .discover(
                &DiscoveryQuery {
                    source_id: Some("archive_org".into()),
                    query: Some("sherlock".into()),
                    genre: None,
                    status: None,
                    language: None,
                    sort: Some("popular".into()),
                    page: 1,
                    limit: 5,
                },
                &cfg,
            )
            .await
            .expect("live archive.org discover failed");
        eprintln!("archive_org discover returned {} items (total={total:?})", items.len());
        assert!(!items.is_empty(), "expected at least one archive_org search result");

        let mut loaded_ch = None;
        for item in &items {
            eprintln!("trying identifier={} title={}", item.external_id, item.title);
            let Ok(chapters) = src.get_chapters(&item.external_id, &cfg).await else {
                continue;
            };
            let Some(first) = chapters.first() else {
                continue;
            };
            if let Ok((_html, paras, words)) = src
                .get_chapter_content(&item.external_id, &first.external_id, None, &cfg)
                .await
            {
                eprintln!(
                    "loaded identifier={} ch='{}' paras={} words={}",
                    item.external_id,
                    first.title,
                    paras.len(),
                    words
                );
                loaded_ch = Some((paras.len(), words));
                break;
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
        let (paras_len, words) = loaded_ch.expect("failed to download & parse any live archive.org novel");
        assert!(paras_len > 0 && words > 0);
    }
}
