export type NovelStatus = "ongoing" | "completed" | "hiatus" | "cancelled" | "unknown";
export type LibraryStatus = "reading" | "plan_to_read" | "finished" | "on_hold" | "dropped";
export type DownloadStatus = "queued" | "downloading" | "paused" | "completed" | "failed" | "cancelled";

export interface SourceCapabilities {
  search: boolean;
  browse_popular: boolean;
  browse_trending: boolean;
  browse_latest: boolean;
  browse_genre: boolean;
  filter_status: boolean;
  genres: string[];
}

export interface SourceConfigField {
  key: string;
  label: string;
  field_type: "text" | "number" | "boolean" | "select";
  default_value: unknown;
  description: string;
  options?: string[];
}

export interface SourceInfo {
  id: string;
  name: string;
  kind: string;
  base_url: string;
  description: string;
  enabled: boolean;
  priority: number;
  health_status: string;
  last_checked_at: string | null;
  capabilities: SourceCapabilities;
  config: Record<string, unknown>;
  config_schema: SourceConfigField[];
  is_custom: boolean;
}

export interface SourceHealthResult {
  source_id: string;
  ok: boolean;
  latency_ms: number;
  status: string;
  message: string;
}

export interface DeclarativeSourceManifest {
  id: string;
  name: string;
  kind: "opds" | "html" | "auto";
  base_url: string;
  description: string;
  genres: string[];
  feed_url?: string;
  search_url_template?: string;
  browse_url_template?: string;
  novel_list_selector?: string;
  title_selector?: string;
  author_selector?: string;
  cover_selector?: string;
  link_selector?: string;
  description_selector?: string;
  chapter_list_selector?: string;
  chapter_title_selector?: string;
  chapter_link_selector?: string;
  content_selector?: string;
  remove_selectors: string[];
  language?: string;
}

export interface NovelSummary {
  id: string;
  source_id: string;
  external_id: string;
  title: string;
  author: string;
  description: string;
  cover_url: string | null;
  local_cover_path: string | null;
  status: NovelStatus;
  genres: string[];
  rating: number | null;
  popularity: number;
  chapter_count: number | null;
  last_updated: string | null;
  language?: string | null;
}

export interface NovelRecord {
  id: string;
  source_id: string;
  external_id: string;
  title: string;
  author_id: string | null;
  author: string;
  description: string;
  cover_url: string | null;
  local_cover_path: string | null;
  status: NovelStatus;
  genres: string[];
  tags: string[];
  rating: number | null;
  popularity: number;
  chapter_count: number;
  last_updated_remote: string | null;
  metadata: Record<string, unknown>;
  is_local: boolean;
  created_at: string;
  updated_at: string;
}

export interface Chapter {
  id: string;
  novel_id: string;
  external_id: string;
  chapter_number: number;
  volume: string | null;
  title: string;
  url: string | null;
  word_count: number;
  published_at: string | null;
  is_downloaded: boolean;
  local_path: string | null;
  size_bytes: number;
}

export interface ChapterContent {
  chapter_id: string;
  novel_id: string;
  chapter_number: number;
  title: string;
  html: string;
  paragraphs: string[];
  word_count: number;
  is_downloaded: boolean;
  prev_chapter_id: string | null;
  next_chapter_id: string | null;
}

export interface ReadingProgress {
  novel_id: string;
  chapter_id: string;
  chapter_number: number;
  chapter_title: string | null;
  scroll_offset: number;
  paragraph_index: number;
  page_index: number;
  total_pages: number;
  progress_percent: number;
  updated_at: string;
}

export interface LibraryEntry {
  novel_id: string;
  status: LibraryStatus;
  is_bookmarked: boolean;
  follow_updates: boolean;
  last_checked_chapters: number;
  added_at: string;
  updated_at: string;
}

export interface LibraryNovelItem {
  novel: NovelRecord;
  entry: LibraryEntry;
  progress: ReadingProgress | null;
  latest_chapter: Chapter | null;
  downloaded_chapters: number;
  total_size_bytes: number;
  collection_ids: string[];
}

export interface Bookmark {
  id: string;
  novel_id: string;
  novel_title: string | null;
  author: string | null;
  cover_url: string | null;
  local_cover_path: string | null;
  chapter_id: string | null;
  chapter_number: number | null;
  chapter_title: string | null;
  paragraph_index: number | null;
  excerpt: string | null;
  note: string | null;
  color: string;
  is_novel_bookmark: boolean;
  created_at: string;
}

export interface Highlight {
  id: string;
  novel_id: string;
  chapter_id: string;
  paragraph_index: number;
  start_offset: number;
  end_offset: number;
  text: string;
  color: string;
  note: string | null;
  created_at: string;
}

export interface NovelTranslation {
  language_code: string;
  language_label: string;
  novel_id: string;
  source_id: string;
  external_id: string;
  title: string;
}

export interface NovelDetailsResponse {
  novel: NovelRecord;
  chapters: Chapter[];
  library_entry: LibraryEntry | null;
  reading_progress: ReadingProgress | null;
  bookmarks: Bookmark[];
  highlights: Highlight[];
  translations: NovelTranslation[];
  collection_ids: string[];
  downloaded_chapters: number;
  total_size_bytes: number;
}

export interface Collection {
  id: string;
  name: string;
  description: string | null;
  color: string | null;
  sort_order: number;
  item_count: number;
  created_at: string;
}

export interface ReadingHistoryItem {
  id: string;
  novel_id: string;
  novel_title: string;
  author: string;
  cover_url: string | null;
  local_cover_path: string | null;
  chapter_id: string;
  chapter_number: number;
  chapter_title: string;
  duration_seconds: number;
  words_read: number;
  read_at: string;
}

export interface DailyReadingPoint {
  date: string;
  minutes: number;
  words: number;
}

export interface ReadingStats {
  total_reading_seconds: number;
  total_words_read: number;
  chapters_read_count: number;
  novels_finished_count: number;
  current_streak_days: number;
  daily_7d: DailyReadingPoint[];
}

export interface HomeDashboardData {
  continue_reading: LibraryNovelItem[];
  recently_updated: LibraryNovelItem[];
  recently_added: LibraryNovelItem[];
  your_library: LibraryNovelItem[];
  stats: ReadingStats;
}

export interface DiscoveryQuery {
  source_id: string | null;
  query: string | null;
  genre: string | null;
  sort: string | null;
  status: string | null;
  language?: string | null;
  page: number;
  limit: number;
}

export interface SourceErrorBanner {
  source_id: string;
  source_name: string;
  message: string;
}

export interface DiscoveryPage {
  items: NovelSummary[];
  page: number;
  has_more: boolean;
  total_estimate: number | null;
  source_errors: SourceErrorBanner[];
}

export interface DownloadTask {
  id: string;
  novel_id: string;
  novel_title: string;
  cover_url: string | null;
  local_cover_path: string | null;
  source_id: string;
  chapter_ids: string[];
  total_chapters: number;
  completed_chapters: number;
  failed_chapters: number;
  bytes_downloaded: number;
  speed_bytes_per_sec: number;
  status: DownloadStatus;
  error_message: string | null;
  created_at: string;
  updated_at: string;
}

export interface NovelStorageUsage {
  novel_id: string;
  title: string;
  author: string;
  cover_url: string | null;
  local_cover_path: string | null;
  downloaded_chapters: number;
  total_chapters: number;
  size_bytes: number;
}

export interface StorageStats {
  total_bytes: number;
  chapters_bytes: number;
  covers_bytes: number;
  database_bytes: number;
  download_dir: string;
  novels: NovelStorageUsage[];
}

export interface UpdateCheckResult {
  novel_id: string;
  novel_title: string;
  previous_chapters: number;
  current_chapters: number;
  new_chapters: number;
  auto_download_queued: boolean;
}

export interface AppSettings {
  theme: "dark" | "light" | "system";
  accent_color: "indigo" | "amber" | "emerald" | "rose" | "cyan";
  density: "comfortable" | "compact";
  sidebar_sections: string[];
  reader_font: string;
  reader_font_size: number;
  reader_line_height: number;
  reader_paragraph_spacing: number;
  reader_width: number;
  reader_theme: "obsidian" | "amoled" | "paper" | "sepia" | "forest" | "custom";
  reader_custom_bg: string;
  reader_custom_fg: string;
  reader_custom_accent: string;
  reader_mode: "scroll" | "paginated";
  reader_flip_animation: boolean;
  reader_text_align: "left" | "justify";
  reader_auto_load_next: boolean;
  default_language: string;
  download_dir: string;
  max_concurrent_downloads: number;
  auto_download_followed: boolean;
  startup_page: string;
  check_updates_on_launch: boolean;
}

export interface DiagnosticsInfo {
  app_data_dir: string;
  db_path: string;
  custom_sources_dir: string;
  logs: string[];
}

export interface ImportBookResult {
  novel: NovelRecord;
  chapters_imported: number;
}

export interface ImportBundleResult {
  novels_imported: number;
  entries_imported: number;
}

export interface GoogleDriveSyncConfigPatch {
  enabled: boolean;
  client_id: string;
  client_secret_update: string | null;
  folder_input: string;
  sync_downloaded_books: boolean;
  sync_settings: boolean;
  auto_sync_on_launch: boolean;
}

export interface GoogleDriveSyncStatus {
  enabled: boolean;
  client_id: string;
  has_client_secret: boolean;
  folder_input: string;
  folder_id: string;
  folder_name: string | null;
  sync_downloaded_books: boolean;
  sync_settings: boolean;
  auto_sync_on_launch: boolean;
  connected: boolean;
  connected_email: string | null;
  token_expires_at: string | null;
  last_sync_at: string | null;
  last_sync_summary: string | null;
}

export interface GoogleDriveOAuthStartResult {
  auth_url: string;
  redirect_uri: string;
  browser_opened: boolean;
}

export interface GoogleDriveSyncReport {
  folder_id: string;
  folder_name: string | null;
  novels_merged: number;
  library_entries_merged: number;
  books_downloaded: number;
  chapters_downloaded: number;
  books_uploaded: number;
  settings_applied: boolean;
  synced_at: string;
}

export interface GoogleDriveCloudBook {
  file_id: string;
  filename: string;
  folder_name: string;
  novel_id: string;
  title: string;
  author: string;
  chapter_count: number;
  size_bytes: number;
  updated_at: string | null;
  is_downloaded_locally: boolean;
}

