use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NovelStatus {
    Ongoing,
    Completed,
    Hiatus,
    Cancelled,
    Unknown,
}

impl NovelStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            NovelStatus::Ongoing => "ongoing",
            NovelStatus::Completed => "completed",
            NovelStatus::Hiatus => "hiatus",
            NovelStatus::Cancelled => "cancelled",
            NovelStatus::Unknown => "unknown",
        }
    }

    pub fn from_str_lossy(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "ongoing" | "in progress" => NovelStatus::Ongoing,
            "completed" | "complete" | "finished" | "public_domain" => NovelStatus::Completed,
            "hiatus" => NovelStatus::Hiatus,
            "cancelled" | "canceled" => NovelStatus::Cancelled,
            _ => NovelStatus::Unknown,
        }
    }
}

impl std::str::FromStr for NovelStatus {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::from_str_lossy(s))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LibraryStatus {
    Reading,
    PlanToRead,
    Finished,
    OnHold,
    Dropped,
}

impl LibraryStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            LibraryStatus::Reading => "reading",
            LibraryStatus::PlanToRead => "plan_to_read",
            LibraryStatus::Finished => "finished",
            LibraryStatus::OnHold => "on_hold",
            LibraryStatus::Dropped => "dropped",
        }
    }

    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "reading" => Some(LibraryStatus::Reading),
            "plan_to_read" => Some(LibraryStatus::PlanToRead),
            "finished" => Some(LibraryStatus::Finished),
            "on_hold" => Some(LibraryStatus::OnHold),
            "dropped" => Some(LibraryStatus::Dropped),
            _ => None,
        }
    }
}

impl std::str::FromStr for LibraryStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_str_opt(s).ok_or_else(|| format!("Unknown library status '{s}'"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceCapabilities {
    pub search: bool,
    pub browse_popular: bool,
    pub browse_trending: bool,
    pub browse_latest: bool,
    pub browse_genre: bool,
    pub filter_status: bool,
    pub genres: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceConfigField {
    pub key: String,
    pub label: String,
    pub field_type: String, // "text" | "number" | "boolean" | "select"
    pub default_value: serde_json::Value,
    pub description: String,
    pub options: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceInfo {
    pub id: String,
    pub name: String,
    pub kind: String, // "builtin" | "opds" | "declarative_html" | "local"
    pub base_url: String,
    pub description: String,
    pub enabled: bool,
    pub priority: i32,
    pub health_status: String, // "online" | "degraded" | "offline" | "unknown"
    pub last_checked_at: Option<String>,
    pub capabilities: SourceCapabilities,
    pub config: HashMap<String, serde_json::Value>,
    pub config_schema: Vec<SourceConfigField>,
    pub is_custom: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceHealthResult {
    pub source_id: String,
    pub ok: bool,
    pub latency_ms: u64,
    pub status: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Author {
    pub id: String,
    pub name: String,
    pub bio: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NovelSummary {
    pub id: String, // deterministic: "{source_id}:{external_id}" or uuid for local
    pub source_id: String,
    pub external_id: String,
    pub title: String,
    pub author: String,
    pub description: String,
    pub cover_url: Option<String>,
    pub local_cover_path: Option<String>,
    pub status: NovelStatus,
    pub genres: Vec<String>,
    pub rating: Option<f64>,
    pub popularity: i64,
    pub chapter_count: Option<i64>,
    pub last_updated: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NovelRecord {
    pub id: String,
    pub source_id: String,
    pub external_id: String,
    pub title: String,
    pub author_id: Option<String>,
    pub author: String,
    pub description: String,
    pub cover_url: Option<String>,
    pub local_cover_path: Option<String>,
    pub status: NovelStatus,
    pub genres: Vec<String>,
    pub tags: Vec<String>,
    pub rating: Option<f64>,
    pub popularity: i64,
    pub chapter_count: i64,
    pub last_updated_remote: Option<String>,
    pub metadata: serde_json::Value,
    pub is_local: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chapter {
    pub id: String,
    pub novel_id: String,
    pub external_id: String,
    pub chapter_number: f64,
    pub volume: Option<String>,
    pub title: String,
    pub url: Option<String>,
    pub word_count: i64,
    pub published_at: Option<String>,
    pub is_downloaded: bool,
    pub local_path: Option<String>,
    pub size_bytes: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChapterContent {
    pub chapter_id: String,
    pub novel_id: String,
    pub chapter_number: f64,
    pub title: String,
    pub html: String,
    pub paragraphs: Vec<String>,
    pub word_count: i64,
    pub is_downloaded: bool,
    pub prev_chapter_id: Option<String>,
    pub next_chapter_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadingProgress {
    pub novel_id: String,
    pub chapter_id: String,
    pub chapter_number: f64,
    pub chapter_title: Option<String>,
    pub scroll_offset: f64,
    pub paragraph_index: i64,
    pub page_index: i64,
    pub total_pages: i64,
    pub progress_percent: f64,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryEntry {
    pub novel_id: String,
    pub status: LibraryStatus,
    pub is_bookmarked: bool,
    pub follow_updates: bool,
    pub last_checked_chapters: i64,
    pub added_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryNovelItem {
    pub novel: NovelRecord,
    pub entry: LibraryEntry,
    pub progress: Option<ReadingProgress>,
    pub latest_chapter: Option<Chapter>,
    pub downloaded_chapters: i64,
    pub total_size_bytes: i64,
    pub collection_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NovelTranslation {
    pub language_code: String,
    pub language_label: String,
    pub novel_id: String,
    pub source_id: String,
    pub external_id: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NovelDetailsResponse {
    pub novel: NovelRecord,
    pub chapters: Vec<Chapter>,
    pub library_entry: Option<LibraryEntry>,
    pub reading_progress: Option<ReadingProgress>,
    pub bookmarks: Vec<Bookmark>,
    #[serde(default)]
    pub highlights: Vec<Highlight>,
    #[serde(default)]
    pub translations: Vec<NovelTranslation>,
    pub collection_ids: Vec<String>,
    pub downloaded_chapters: i64,
    pub total_size_bytes: i64,
}

fn default_bookmark_color() -> String {
    "#818CF8".to_string()
}

fn default_highlight_color() -> String {
    "#FDE047".to_string()
}

fn default_true() -> bool {
    true
}

fn default_language() -> String {
    "en".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    pub id: String,
    pub novel_id: String,
    pub novel_title: Option<String>,
    pub author: Option<String>,
    pub cover_url: Option<String>,
    pub local_cover_path: Option<String>,
    pub chapter_id: Option<String>,
    pub chapter_number: Option<f64>,
    pub chapter_title: Option<String>,
    pub paragraph_index: Option<i64>,
    pub excerpt: Option<String>,
    pub note: Option<String>,
    #[serde(default = "default_bookmark_color")]
    pub color: String,
    pub is_novel_bookmark: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Highlight {
    pub id: String,
    pub novel_id: String,
    pub chapter_id: String,
    pub paragraph_index: i64,
    pub start_offset: i64,
    pub end_offset: i64,
    pub text: String,
    #[serde(default = "default_highlight_color")]
    pub color: String,
    pub note: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Collection {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub color: Option<String>,
    pub sort_order: i32,
    pub item_count: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadingHistoryItem {
    pub id: String,
    pub novel_id: String,
    pub novel_title: String,
    pub author: String,
    pub cover_url: Option<String>,
    pub local_cover_path: Option<String>,
    pub chapter_id: String,
    pub chapter_number: f64,
    pub chapter_title: String,
    pub duration_seconds: i64,
    pub words_read: i64,
    pub read_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyReadingPoint {
    pub date: String,
    pub minutes: i64,
    pub words: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadingStats {
    pub total_reading_seconds: i64,
    pub total_words_read: i64,
    pub chapters_read_count: i64,
    pub novels_finished_count: i64,
    pub current_streak_days: i64,
    pub daily_7d: Vec<DailyReadingPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DownloadStatus {
    Queued,
    Downloading,
    Paused,
    Completed,
    Failed,
    Cancelled,
}

impl DownloadStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            DownloadStatus::Queued => "queued",
            DownloadStatus::Downloading => "downloading",
            DownloadStatus::Paused => "paused",
            DownloadStatus::Completed => "completed",
            DownloadStatus::Failed => "failed",
            DownloadStatus::Cancelled => "cancelled",
        }
    }

    pub fn from_str_lossy(s: &str) -> Self {
        match s {
            "queued" => DownloadStatus::Queued,
            "downloading" => DownloadStatus::Downloading,
            "paused" => DownloadStatus::Paused,
            "completed" => DownloadStatus::Completed,
            "failed" => DownloadStatus::Failed,
            "cancelled" => DownloadStatus::Cancelled,
            _ => DownloadStatus::Failed,
        }
    }
}

impl std::str::FromStr for DownloadStatus {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::from_str_lossy(s))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadTask {
    pub id: String,
    pub novel_id: String,
    pub novel_title: String,
    pub cover_url: Option<String>,
    pub local_cover_path: Option<String>,
    pub source_id: String,
    pub chapter_ids: Vec<String>,
    pub total_chapters: i64,
    pub completed_chapters: i64,
    pub failed_chapters: i64,
    pub bytes_downloaded: i64,
    pub speed_bytes_per_sec: f64,
    pub status: DownloadStatus,
    pub error_message: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NovelStorageUsage {
    pub novel_id: String,
    pub title: String,
    pub author: String,
    pub cover_url: Option<String>,
    pub local_cover_path: Option<String>,
    pub downloaded_chapters: i64,
    pub total_chapters: i64,
    pub size_bytes: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageStats {
    pub total_bytes: i64,
    pub chapters_bytes: i64,
    pub covers_bytes: i64,
    pub database_bytes: i64,
    pub download_dir: String,
    pub novels: Vec<NovelStorageUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryQuery {
    pub source_id: Option<String>,
    pub query: Option<String>,
    pub genre: Option<String>,
    pub sort: Option<String>,   // "popular" | "trending" | "latest" | "title"
    pub status: Option<String>, // "all" | "ongoing" | "completed"
    #[serde(default)]
    pub language: Option<String>, // ISO 639-1 code e.g. "en", "pl", "es", or "all"
    pub page: u32,
    pub limit: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceErrorBanner {
    pub source_id: String,
    pub source_name: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryPage {
    pub items: Vec<NovelSummary>,
    pub page: u32,
    pub has_more: bool,
    pub total_estimate: Option<u64>,
    pub source_errors: Vec<SourceErrorBanner>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HomeDashboardData {
    pub continue_reading: Vec<LibraryNovelItem>,
    pub recently_updated: Vec<LibraryNovelItem>,
    pub recently_added: Vec<LibraryNovelItem>,
    pub your_library: Vec<LibraryNovelItem>,
    pub stats: ReadingStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCheckResult {
    pub novel_id: String,
    pub novel_title: String,
    pub previous_chapters: i64,
    pub current_chapters: i64,
    pub new_chapters: i64,
    pub auto_download_queued: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    // Appearance
    pub theme: String,             // "dark" | "light" | "system"
    pub accent_color: String,      // "indigo" | "amber" | "emerald" | "rose" | "cyan"
    pub density: String,           // "comfortable" | "compact"
    pub sidebar_sections: Vec<String>,
    // Reader
    pub reader_font: String,
    pub reader_font_size: u32,
    pub reader_line_height: f64,
    pub reader_paragraph_spacing: f64,
    pub reader_width: u32,
    pub reader_theme: String, // "obsidian" | "paper" | "sepia" | "forest" | "custom"
    pub reader_custom_bg: String,
    pub reader_custom_fg: String,
    pub reader_custom_accent: String,
    pub reader_mode: String, // "scroll" | "paginated"
    #[serde(default = "default_true")]
    pub reader_flip_animation: bool,
    pub reader_text_align: String, // "left" | "justify"
    pub reader_auto_load_next: bool,
    // Downloads
    pub download_dir: String,
    pub max_concurrent_downloads: u32,
    pub auto_download_followed: bool,
    // General
    pub startup_page: String,
    pub check_updates_on_launch: bool,
    #[serde(default = "default_language")]
    pub default_language: String,
}

/// Returns true when an author label is empty or a generic unscraped placeholder.
pub fn is_placeholder_author(author: &str) -> bool {
    let norm = author.trim().to_ascii_lowercase();
    norm.is_empty()
        || matches!(
            norm.as_str(),
            "unknown"
                | "unknown author"
                | "wikisource archive"
                | "wikisource edition"
                | "anonymous"
        )
}

impl AppSettings {
    pub fn default_with_dir(download_dir: String) -> Self {
        Self {
            theme: "dark".to_string(),
            accent_color: "indigo".to_string(),
            density: "comfortable".to_string(),
            sidebar_sections: vec![
                "home".into(),
                "discover".into(),
                "books".into(),
                "explore".into(),
                "library".into(),
                "downloads".into(),
                "history".into(),
                "bookmarks".into(),
                "settings".into(),
            ],
            reader_font: "Literata".to_string(),
            reader_font_size: 18,
            reader_line_height: 1.72,
            reader_paragraph_spacing: 1.15,
            reader_width: 720,
            reader_theme: "obsidian".to_string(),
            reader_custom_bg: "#121318".to_string(),
            reader_custom_fg: "#E2E4EA".to_string(),
            reader_custom_accent: "#818CF8".to_string(),
            reader_mode: "scroll".to_string(),
            reader_flip_animation: true,
            reader_text_align: "left".to_string(),
            reader_auto_load_next: true,
            download_dir,
            max_concurrent_downloads: 3,
            auto_download_followed: false,
            startup_page: "home".to_string(),
            check_updates_on_launch: false,
            default_language: "en".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryExportBundle {
    pub version: u32,
    pub exported_at: String,
    pub novels: Vec<NovelRecord>,
    pub chapters: Vec<Chapter>,
    pub library_entries: Vec<LibraryEntry>,
    pub reading_progress: Vec<ReadingProgress>,
    pub bookmarks: Vec<Bookmark>,
    #[serde(default)]
    pub highlights: Vec<Highlight>,
    pub collections: Vec<Collection>,
    pub collection_items: Vec<(String, String, String)>,
    pub reading_history: Vec<ReadingHistoryItem>,
    pub settings: Option<AppSettings>,
}
