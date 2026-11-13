pub mod archive_org;
pub mod declarative;
pub mod gutenberg;
pub mod wikisource;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{Mutex, RwLock};

use crate::db::Database;
use crate::error::{AppError, AppResult};
use crate::formats::ParsedBook;
use crate::models::{
    Chapter, DiscoveryQuery, NovelRecord, NovelSummary, NovelTranslation, SourceCapabilities,
    SourceConfigField, SourceHealthResult, SourceInfo,
};
use crate::net::SourceHttpClient;

pub use declarative::DeclarativeSourceManifest;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedChapterEntry {
    pub chapter_number: f64,
    pub external_id: String,
    pub title: String,
    pub html: String,
    pub paragraphs: Vec<String>,
    pub word_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedParsedBook {
    pub novel_id: String,
    pub chapters: Vec<CachedChapterEntry>,
}

/// Shared disk/in-flight lock cache for EPUB/HTML books fetched from sources
/// that distribute a single archive per book (Gutenberg, Archive.org, OPDS).
#[derive(Clone)]
pub struct ParsedBookCache {
    cache_dir: PathBuf,
    locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
}

impl ParsedBookCache {
    pub fn new(cache_dir: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(&cache_dir);
        Self {
            cache_dir,
            locks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn root_dir(&self) -> &Path {
        &self.cache_dir
    }

    pub async fn novel_lock(&self, novel_id: &str) -> Arc<Mutex<()>> {
        let mut map = self.locks.lock().await;
        map.entry(novel_id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    fn cache_file(&self, novel_id: &str) -> PathBuf {
        let safe: String = novel_id
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
            .collect();
        self.cache_dir.join(format!("{safe}.json"))
    }

    pub fn get(&self, novel_id: &str) -> Option<CachedParsedBook> {
        let path = self.cache_file(novel_id);
        let data = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&data).ok()
    }

    pub fn put(&self, novel_id: &str, parsed: &ParsedBook) -> AppResult<CachedParsedBook> {
        std::fs::create_dir_all(&self.cache_dir)?;
        let cached = CachedParsedBook {
            novel_id: novel_id.to_string(),
            chapters: parsed
                .chapters
                .iter()
                .enumerate()
                .map(|(i, c)| CachedChapterEntry {
                    chapter_number: c.chapter_number,
                    external_id: format!("ch_{}", i + 1),
                    title: c.title.clone(),
                    html: c.html.clone(),
                    paragraphs: c.paragraphs.clone(),
                    word_count: c.word_count,
                })
                .collect(),
        };
        let json = serde_json::to_string(&cached)?;
        std::fs::write(self.cache_file(novel_id), json)?;
        Ok(cached)
    }
}

#[async_trait]
pub trait NovelSource: Send + Sync {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn kind(&self) -> &str;
    fn base_url(&self) -> &str;
    fn description(&self) -> &str;
    fn is_custom(&self) -> bool {
        false
    }
    fn default_enabled(&self) -> bool {
        true
    }
    fn capabilities(&self) -> SourceCapabilities;
    fn config_schema(&self) -> Vec<SourceConfigField> {
        Vec::new()
    }

    async fn discover(
        &self,
        query: &DiscoveryQuery,
        config: &HashMap<String, Value>,
    ) -> AppResult<(Vec<NovelSummary>, bool, Option<u64>)>;

    async fn get_novel(
        &self,
        external_id: &str,
        config: &HashMap<String, Value>,
    ) -> AppResult<NovelRecord>;

    async fn get_chapters(
        &self,
        external_id: &str,
        config: &HashMap<String, Value>,
    ) -> AppResult<Vec<Chapter>>;

    async fn get_chapter_content(
        &self,
        external_novel_id: &str,
        external_chapter_id: &str,
        chapter_url: Option<&str>,
        config: &HashMap<String, Value>,
    ) -> AppResult<(String, Vec<String>, i64)>;

    async fn get_translations(
        &self,
        _external_id: &str,
        _config: &HashMap<String, Value>,
    ) -> AppResult<Vec<NovelTranslation>> {
        Ok(Vec::new())
    }

    async fn health_check(&self, config: &HashMap<String, Value>) -> AppResult<u64>;
}

#[derive(Clone)]
pub struct SourceRegistry {
    sources: Arc<RwLock<Vec<Arc<dyn NovelSource>>>>,
    custom_sources_dir: PathBuf,
    http: SourceHttpClient,
    book_cache: ParsedBookCache,
}

struct HtmlPreset<'a> {
    id: &'a str,
    name: &'a str,
    base_url: &'a str,
    description: &'a str,
    browse_url_template: &'a str,
    search_url_template: Option<&'a str>,
    novel_list_selector: &'a str,
    title_selector: &'a str,
    author_selector: Option<&'a str>,
    link_selector: Option<&'a str>,
    description_selector: Option<&'a str>,
    chapter_list_selector: &'a str,
    content_selector: &'a str,
    remove_selectors: &'a [&'a str],
}

impl SourceRegistry {
    fn html_preset(&self, spec: HtmlPreset<'_>) -> Arc<dyn NovelSource> {
        let manifest = DeclarativeSourceManifest {
            id: spec.id.to_string(),
            name: spec.name.to_string(),
            kind: declarative::DeclarativeKind::Html,
            base_url: spec.base_url.to_string(),
            description: spec.description.to_string(),
            genres: vec!["Fantasy".into(), "Action".into(), "Romance".into()],
            feed_url: None,
            search_url_template: spec.search_url_template.map(str::to_string),
            browse_url_template: Some(spec.browse_url_template.to_string()),
            novel_list_selector: Some(spec.novel_list_selector.to_string()),
            title_selector: Some(spec.title_selector.to_string()),
            author_selector: spec.author_selector.map(str::to_string),
            cover_selector: Some("img".to_string()),
            link_selector: spec.link_selector.map(str::to_string),
            description_selector: spec.description_selector.map(str::to_string),
            chapter_list_selector: Some(spec.chapter_list_selector.to_string()),
            chapter_title_selector: None,
            chapter_link_selector: None,
            content_selector: Some(spec.content_selector.to_string()),
            remove_selectors: spec
                .remove_selectors
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
        };
        Arc::new(declarative::DeclarativeSource::new_builtin(
            manifest,
            self.http.clone(),
            self.book_cache.clone(),
        ))
    }

    fn builtin_sources(&self) -> Vec<Arc<dyn NovelSource>> {
        vec![
            Arc::new(gutenberg::GutenbergSource::new(
                self.http.clone(),
                self.book_cache.clone(),
            )),
            Arc::new(wikisource::WikisourceSource::new(self.http.clone())),
            Arc::new(archive_org::ArchiveOrgSource::new(
                self.http.clone(),
                self.book_cache.clone(),
            )),
            self.html_preset(HtmlPreset {
                id: "fanmtl",
                name: "FanMTL",
                base_url: "https://www.fanmtl.com",
                description: "Fan translations and web novel updates hosted on fanmtl.com.",
                browse_url_template: "https://www.fanmtl.com/list/all/all-newstime-{page0}.html",
                search_url_template: None,
                novel_list_selector: "li.novel-item",
                title_selector: "h1.novel-title, h4.novel-title",
                author_selector: Some(".author span[itemprop='author']"),
                link_selector: Some("a[href^='/novel/']"),
                description_selector: Some(".summary .content"),
                chapter_list_selector: "ul.chapter-list li a",
                content_selector: ".chapter-content",
                remove_selectors: &["script", ".ads", "div[align='center']"],
            }),
            self.html_preset(HtmlPreset {
                id: "wuxiabox",
                name: "WuxiaBox",
                base_url: "https://www.wuxiabox.com",
                description: "Translated fantasy, cultivation, and action novels from wuxiabox.com.",
                browse_url_template: "https://www.wuxiabox.com/list/all/all-newstime-{page0}.html",
                search_url_template: None,
                novel_list_selector: "li.novel-item",
                title_selector: "h1.novel-title, h4.novel-title",
                author_selector: Some(".author span[itemprop='author']"),
                link_selector: Some("a[href^='/novel/']"),
                description_selector: Some(".summary .content"),
                chapter_list_selector: "ul.chapter-list li a",
                content_selector: ".chapter-content",
                remove_selectors: &["script", ".ads", "div[align='center']"],
            }),
            self.html_preset(HtmlPreset {
                id: "nobadnovel",
                name: "NoBadNovel",
                base_url: "https://www.nobadnovel.com",
                description: "Serialized web novels and translations from nobadnovel.com.",
                browse_url_template: "https://www.nobadnovel.com/series?page={page}",
                search_url_template: Some(
                    "https://www.nobadnovel.com/series?search={query}&page={page}",
                ),
                novel_list_selector: ".grid > div",
                title_selector: "h4 a, h1",
                author_selector: None,
                link_selector: Some("h4 a, a[href*='/series/']"),
                description_selector: Some("#intro .content"),
                chapter_list_selector: "#table a[href*='/chapter-']",
                content_selector: "main div.grow, main article, main",
                remove_selectors: &["script", "header", "nav"],
            }),
            self.html_preset(HtmlPreset {
                id: "wuxia_click",
                name: "Wuxia.Click",
                base_url: "https://wuxia.click",
                description: "Web novels and multi-chapter series hosted on wuxia.click.",
                browse_url_template: "https://wuxia.click/browse?page={page}",
                search_url_template: Some("https://wuxia.click/browse?search={query}&page={page}"),
                novel_list_selector: "main a[href^='/novel/']",
                title_selector: "h3, h1",
                author_selector: None,
                link_selector: None,
                description_selector: Some("main p"),
                chapter_list_selector: "li a[href^='/chapter/']",
                content_selector: "#chapter-content",
                remove_selectors: &["script", ".ads"],
            }),
        ]
    }

    pub fn new(
        db: &Database,
        http: SourceHttpClient,
        custom_sources_dir: PathBuf,
        cache_dir: PathBuf,
    ) -> AppResult<Self> {
        let _ = std::fs::create_dir_all(&custom_sources_dir);
        let book_cache = ParsedBookCache::new(cache_dir);

        let registry = Self {
            sources: Arc::new(RwLock::new(Vec::new())),
            custom_sources_dir,
            http,
            book_cache,
        };

        let mut list = registry.builtin_sources();
        if let Ok(customs) = Self::load_custom_manifests(&registry.custom_sources_dir) {
            for manifest in customs {
                list.push(Arc::new(declarative::DeclarativeSource::new(
                    manifest,
                    registry.http.clone(),
                    registry.book_cache.clone(),
                )));
            }
        }

        for src in &list {
            let _ = db.upsert_source_definition(
                src.id(),
                src.name(),
                src.kind(),
                src.base_url(),
                src.description(),
                src.default_enabled(),
            );
        }

        // Blocking write during startup init
        if let Ok(mut w) = registry.sources.try_write() {
            *w = list;
        }

        Ok(registry)
    }

    pub fn custom_sources_dir(&self) -> &Path {
        &self.custom_sources_dir
    }

    fn load_custom_manifests(dir: &Path) -> AppResult<Vec<DeclarativeSourceManifest>> {
        let mut manifests = Vec::new();
        if !dir.exists() {
            return Ok(manifests);
        }
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                match std::fs::read_to_string(&path)
                    .map_err(AppError::from)
                    .and_then(|s| serde_json::from_str::<DeclarativeSourceManifest>(&s).map_err(AppError::from))
                {
                    Ok(m) => manifests.push(m),
                    Err(err) => {
                        tracing::warn!("Skipping invalid custom source file {:?}: {}", path, err);
                    }
                }
            }
        }
        Ok(manifests)
    }

    pub async fn reload_custom_sources(&self, db: &Database) -> AppResult<()> {
        let mut list = self.builtin_sources();

        for manifest in Self::load_custom_manifests(&self.custom_sources_dir)? {
            list.push(Arc::new(declarative::DeclarativeSource::new(
                manifest,
                self.http.clone(),
                self.book_cache.clone(),
            )));
        }

        for src in &list {
            db.upsert_source_definition(
                src.id(),
                src.name(),
                src.kind(),
                src.base_url(),
                src.description(),
                src.default_enabled(),
            )?;
        }

        let mut w = self.sources.write().await;
        *w = list;
        Ok(())
    }

    pub async fn install_custom_source(
        &self,
        db: &Database,
        manifest: DeclarativeSourceManifest,
    ) -> AppResult<SourceInfo> {
        let id = manifest.id.trim().to_string();
        if id.is_empty()
            || !id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(AppError::InvalidInput(
                "Source ID must be non-empty alphanumeric, '-' or '_'".into(),
            ));
        }
        if matches!(
            id.as_str(),
            "gutenberg"
                | "wikisource"
                | "archive_org"
                | "local"
                | "fanmtl"
                | "wuxiabox"
                | "nobadnovel"
                | "wuxia_click"
        ) {
            return Err(AppError::InvalidInput(
                "Cannot overwrite built-in source ID".into(),
            ));
        }
        std::fs::create_dir_all(&self.custom_sources_dir)?;
        let file_path = self.custom_sources_dir.join(format!("{id}.json"));
        let json = serde_json::to_string_pretty(&manifest)?;
        std::fs::write(&file_path, json)?;
        self.reload_custom_sources(db).await?;
        self.get_source_info(db, &id)
            .await?
            .ok_or_else(|| AppError::Other("Failed to load installed source".into()))
    }

    pub async fn remove_custom_source(&self, db: &Database, id: &str) -> AppResult<()> {
        let file_path = self.custom_sources_dir.join(format!("{id}.json"));
        if file_path.exists() {
            std::fs::remove_file(file_path)?;
        }
        db.delete_source_record(id)?;
        self.reload_custom_sources(db).await?;
        Ok(())
    }

    pub async fn get_source(&self, id: &str) -> Option<Arc<dyn NovelSource>> {
        let r = self.sources.read().await;
        r.iter().find(|s| s.id() == id).cloned()
    }

    pub async fn list_sources(&self, db: &Database) -> AppResult<Vec<SourceInfo>> {
        let states = db.get_source_states()?;
        let r = self.sources.read().await;
        let mut out = Vec::with_capacity(r.len());
        for s in r.iter() {
            let st = states.get(s.id());
            let mut config = HashMap::new();
            for f in s.config_schema() {
                config.insert(f.key, f.default_value);
            }
            if let Some(st) = st {
                for (k, v) in &st.config {
                    config.insert(k.clone(), v.clone());
                }
            }
            out.push(SourceInfo {
                id: s.id().to_string(),
                name: s.name().to_string(),
                kind: s.kind().to_string(),
                base_url: s.base_url().to_string(),
                description: s.description().to_string(),
                enabled: st.map(|x| x.enabled).unwrap_or_else(|| s.default_enabled()),
                priority: st.map(|x| x.priority).unwrap_or(100),
                health_status: st
                    .map(|x| x.health_status.clone())
                    .unwrap_or_else(|| "unknown".into()),
                last_checked_at: st.and_then(|x| x.last_checked_at.clone()),
                capabilities: s.capabilities(),
                config,
                config_schema: s.config_schema(),
                is_custom: s.is_custom(),
            });
        }
        out.sort_by_key(|x| x.priority);
        Ok(out)
    }

    pub async fn get_source_info(&self, db: &Database, id: &str) -> AppResult<Option<SourceInfo>> {
        let all = self.list_sources(db).await?;
        Ok(all.into_iter().find(|s| s.id == id))
    }

    pub async fn enabled_sources_with_config(
        &self,
        db: &Database,
        filter_source_id: Option<&str>,
    ) -> AppResult<Vec<(Arc<dyn NovelSource>, HashMap<String, Value>)>> {
        let infos = self.list_sources(db).await?;
        let r = self.sources.read().await;
        let mut out = Vec::new();
        for info in infos {
            if let Some(target_id) = filter_source_id {
                if info.id != target_id {
                    continue;
                }
            } else if !info.enabled {
                continue;
            }
            if let Some(src) = r.iter().find(|s| s.id() == info.id) {
                out.push((src.clone(), info.config));
            }
        }
        Ok(out)
    }

    pub async fn test_source_connection(
        &self,
        db: &Database,
        source_id: &str,
    ) -> AppResult<SourceHealthResult> {
        let src = self
            .get_source(source_id)
            .await
            .ok_or_else(|| AppError::NotFound(format!("Source {source_id} not found")))?;
        let info = self.get_source_info(db, source_id).await?;
        let cfg = info.map(|i| i.config).unwrap_or_default();

        match src.health_check(&cfg).await {
            Ok(ms) => {
                db.update_source_health(source_id, "online")?;
                Ok(SourceHealthResult {
                    source_id: source_id.to_string(),
                    ok: true,
                    latency_ms: ms,
                    status: "online".into(),
                    message: format!("Connected in {ms}ms"),
                })
            }
            Err(e) => {
                db.update_source_health(source_id, "offline")?;
                Ok(SourceHealthResult {
                    source_id: source_id.to_string(),
                    ok: false,
                    latency_ms: 0,
                    status: "offline".into(),
                    message: e.to_string(),
                })
            }
        }
    }
}

pub fn make_novel_id(source_id: &str, external_id: &str) -> String {
    format!("{source_id}:{external_id}")
}

pub fn make_chapter_id(novel_id: &str, external_chapter_id: &str) -> String {
    format!("{novel_id}::{external_chapter_id}")
}
