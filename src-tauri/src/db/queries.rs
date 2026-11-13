use std::collections::{HashMap, HashSet};
use chrono::{Duration, Utc};
use rusqlite::{params, OptionalExtension, Row};
use uuid::Uuid;

use crate::db::Database;
use crate::error::{AppError, AppResult};
use crate::models::{
    AppSettings, Bookmark, Chapter, Collection, DailyReadingPoint, DownloadStatus, DownloadTask,
    Highlight, HomeDashboardData, LibraryEntry, LibraryExportBundle, LibraryNovelItem,
    LibraryStatus, NovelRecord, NovelStatus, NovelStorageUsage, NovelSummary, NovelTranslation,
    ReadingHistoryItem, ReadingProgress, ReadingStats,
};

fn now_iso() -> String {
    Utc::now().to_rfc3339()
}

fn row_to_novel(row: &Row<'_>) -> rusqlite::Result<NovelRecord> {
    let status_str: String = row.get("status")?;
    let genres_json: String = row.get("genres_json")?;
    let tags_json: String = row.get("tags_json")?;
    let meta_json: String = row.get("metadata_json")?;
    let is_local_int: i64 = row.get("is_local")?;

    Ok(NovelRecord {
        id: row.get("id")?,
        source_id: row.get("source_id")?,
        external_id: row.get("external_id")?,
        title: row.get("title")?,
        author_id: row.get("author_id")?,
        author: row.get("author_name")?,
        description: row.get("description")?,
        cover_url: row.get("cover_url")?,
        local_cover_path: row.get("local_cover_path")?,
        status: NovelStatus::from_str_lossy(&status_str),
        genres: serde_json::from_str(&genres_json).unwrap_or_default(),
        tags: serde_json::from_str(&tags_json).unwrap_or_default(),
        rating: row.get("rating")?,
        popularity: row.get("popularity")?,
        chapter_count: row.get("chapter_count")?,
        last_updated_remote: row.get("last_updated_remote")?,
        metadata: serde_json::from_str(&meta_json).unwrap_or(serde_json::Value::Object(Default::default())),
        is_local: is_local_int != 0,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

fn row_to_chapter(row: &Row<'_>) -> rusqlite::Result<Chapter> {
    let is_dl: i64 = row.get("is_downloaded")?;
    Ok(Chapter {
        id: row.get("id")?,
        novel_id: row.get("novel_id")?,
        external_id: row.get("external_id")?,
        chapter_number: row.get("chapter_number")?,
        volume: row.get("volume")?,
        title: row.get("title")?,
        url: row.get("url")?,
        word_count: row.get("word_count")?,
        published_at: row.get("published_at")?,
        is_downloaded: is_dl != 0,
        local_path: row.get("local_path")?,
        size_bytes: row.get("size_bytes")?,
    })
}

fn row_to_progress(row: &Row<'_>, chapter_title: Option<String>) -> rusqlite::Result<ReadingProgress> {
    Ok(ReadingProgress {
        novel_id: row.get("novel_id")?,
        chapter_id: row.get("chapter_id")?,
        chapter_number: row.get("chapter_number")?,
        chapter_title,
        scroll_offset: row.get("scroll_offset")?,
        paragraph_index: row.get("paragraph_index")?,
        page_index: row.get("page_index")?,
        total_pages: row.get("total_pages")?,
        progress_percent: row.get("progress_percent")?,
        updated_at: row.get("updated_at")?,
    })
}

fn row_to_library_entry(row: &Row<'_>) -> rusqlite::Result<LibraryEntry> {
    let status_str: String = row.get("status")?;
    let is_bm: i64 = row.get("is_bookmarked")?;
    let follow: i64 = row.get("follow_updates")?;
    Ok(LibraryEntry {
        novel_id: row.get("novel_id")?,
        status: LibraryStatus::from_str_opt(&status_str).unwrap_or(LibraryStatus::Reading),
        is_bookmarked: is_bm != 0,
        follow_updates: follow != 0,
        last_checked_chapters: row.get("last_checked_chapters")?,
        added_at: row.get("added_at")?,
        updated_at: row.get("updated_at")?,
    })
}

#[derive(Debug, Clone)]
pub struct StoredSourceState {
    pub id: String,
    pub enabled: bool,
    pub priority: i32,
    pub config: HashMap<String, serde_json::Value>,
    pub health_status: String,
    pub last_checked_at: Option<String>,
}

impl Database {
    // -------------------------------------------------------------------------
    // Sources
    // -------------------------------------------------------------------------
    pub fn upsert_source_definition(
        &self,
        id: &str,
        name: &str,
        kind: &str,
        base_url: &str,
        description: &str,
        default_enabled: bool,
    ) -> AppResult<()> {
        self.with_conn(|conn| {
            conn.execute(
                r#"
                INSERT INTO sources (id, name, kind, base_url, description, enabled, priority, config_json)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, 100, '{}')
                ON CONFLICT(id) DO UPDATE SET
                    name = excluded.name,
                    kind = excluded.kind,
                    base_url = excluded.base_url,
                    description = excluded.description
                "#,
                params![id, name, kind, base_url, description, if default_enabled { 1 } else { 0 }],
            )?;
            Ok(())
        })
    }

    pub fn get_source_states(&self) -> AppResult<HashMap<String, StoredSourceState>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, enabled, priority, config_json, health_status, last_checked_at FROM sources",
            )?;
            let mut map = HashMap::new();
            let rows = stmt.query_map([], |row| {
                let id: String = row.get(0)?;
                let enabled: i64 = row.get(1)?;
                let priority: i32 = row.get(2)?;
                let cfg_str: String = row.get(3)?;
                let health_status: String = row.get(4)?;
                let last_checked_at: Option<String> = row.get(5)?;
                Ok(StoredSourceState {
                    id,
                    enabled: enabled != 0,
                    priority,
                    config: serde_json::from_str(&cfg_str).unwrap_or_default(),
                    health_status,
                    last_checked_at,
                })
            })?;
            for r in rows {
                let state = r?;
                map.insert(state.id.clone(), state);
            }
            Ok(map)
        })
    }

    pub fn set_source_enabled(&self, id: &str, enabled: bool) -> AppResult<()> {
        self.with_conn(|conn| {
            let changed = conn.execute(
                "UPDATE sources SET enabled = ?2 WHERE id = ?1",
                params![id, if enabled { 1 } else { 0 }],
            )?;
            if changed == 0 {
                return Err(AppError::NotFound(format!("Source {id} not found")));
            }
            Ok(())
        })
    }

    pub fn update_source_config(
        &self,
        id: &str,
        config: &HashMap<String, serde_json::Value>,
    ) -> AppResult<()> {
        let cfg_json = serde_json::to_string(config)?;
        self.with_conn(|conn| {
            let changed = conn.execute(
                "UPDATE sources SET config_json = ?2 WHERE id = ?1",
                params![id, cfg_json],
            )?;
            if changed == 0 {
                return Err(AppError::NotFound(format!("Source {id} not found")));
            }
            Ok(())
        })
    }

    pub fn update_source_health(&self, id: &str, status: &str) -> AppResult<()> {
        let now = now_iso();
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE sources SET health_status = ?2, last_checked_at = ?3 WHERE id = ?1",
                params![id, status, now],
            )?;
            Ok(())
        })
    }

    pub fn delete_source_record(&self, id: &str) -> AppResult<()> {
        self.with_conn(|conn| {
            conn.execute("DELETE FROM sources WHERE id = ?1", params![id])?;
            Ok(())
        })
    }

    // -------------------------------------------------------------------------
    // Authors & Novels
    // -------------------------------------------------------------------------
    pub fn upsert_author(&self, name: &str, url: Option<&str>) -> AppResult<String> {
        let trimmed = if name.trim().is_empty() {
            "Unknown Author"
        } else {
            name.trim()
        };
        self.with_conn(|conn| {
            if let Some(existing) = conn
                .query_row(
                    "SELECT id FROM authors WHERE name = ?1",
                    params![trimmed],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
            {
                return Ok(existing);
            }
            let id = Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO authors (id, name, url) VALUES (?1, ?2, ?3)",
                params![id, trimmed, url],
            )?;
            Ok(id)
        })
    }

    pub fn upsert_novel(&self, novel: &NovelRecord) -> AppResult<NovelRecord> {
        let author_id = self.upsert_author(&novel.author, None)?;
        let now = now_iso();
        let genres_json = serde_json::to_string(&novel.genres)?;
        let tags_json = serde_json::to_string(&novel.tags)?;
        let meta_json = serde_json::to_string(&novel.metadata)?;

        self.with_conn(|conn| {
            conn.execute(
                r#"
                INSERT INTO novels (
                    id, source_id, external_id, title, author_id, author_name,
                    description, cover_url, local_cover_path, status, genres_json,
                    tags_json, rating, popularity, chapter_count, last_updated_remote,
                    metadata_json, is_local, created_at, updated_at
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)
                ON CONFLICT(id) DO UPDATE SET
                    title = CASE WHEN excluded.title != '' THEN excluded.title ELSE novels.title END,
                    author_id = COALESCE(excluded.author_id, novels.author_id),
                    author_name = CASE WHEN excluded.author_name != '' THEN excluded.author_name ELSE novels.author_name END,
                    description = CASE WHEN excluded.description != '' THEN excluded.description ELSE novels.description END,
                    cover_url = COALESCE(excluded.cover_url, novels.cover_url),
                    local_cover_path = COALESCE(excluded.local_cover_path, novels.local_cover_path),
                    status = CASE WHEN excluded.status != 'unknown' THEN excluded.status ELSE novels.status END,
                    genres_json = CASE WHEN excluded.genres_json != '[]' THEN excluded.genres_json ELSE novels.genres_json END,
                    tags_json = CASE WHEN excluded.tags_json != '[]' THEN excluded.tags_json ELSE novels.tags_json END,
                    rating = COALESCE(excluded.rating, novels.rating),
                    popularity = MAX(excluded.popularity, novels.popularity),
                    chapter_count = CASE WHEN excluded.chapter_count > 0 THEN excluded.chapter_count ELSE novels.chapter_count END,
                    last_updated_remote = COALESCE(excluded.last_updated_remote, novels.last_updated_remote),
                    metadata_json = CASE WHEN excluded.metadata_json != '{}' THEN excluded.metadata_json ELSE novels.metadata_json END,
                    updated_at = excluded.updated_at
                "#,
                params![
                    novel.id,
                    novel.source_id,
                    novel.external_id,
                    novel.title,
                    author_id,
                    novel.author,
                    novel.description,
                    novel.cover_url,
                    novel.local_cover_path,
                    novel.status.as_str(),
                    genres_json,
                    tags_json,
                    novel.rating,
                    novel.popularity,
                    novel.chapter_count,
                    novel.last_updated_remote,
                    meta_json,
                    if novel.is_local { 1 } else { 0 },
                    if novel.created_at.is_empty() { &now } else { &novel.created_at },
                    &now,
                ],
            )?;
            let updated = conn.query_row(
                "SELECT * FROM novels WHERE id = ?1",
                params![novel.id],
                row_to_novel,
            )?;
            Ok(updated)
        })
    }

    pub fn upsert_novel_from_summary(&self, summary: &NovelSummary) -> AppResult<NovelRecord> {
        let now = now_iso();
        let record = NovelRecord {
            id: summary.id.clone(),
            source_id: summary.source_id.clone(),
            external_id: summary.external_id.clone(),
            title: summary.title.clone(),
            author_id: None,
            author: summary.author.clone(),
            description: summary.description.clone(),
            cover_url: summary.cover_url.clone(),
            local_cover_path: summary.local_cover_path.clone(),
            status: summary.status.clone(),
            genres: summary.genres.clone(),
            tags: Vec::new(),
            rating: summary.rating,
            popularity: summary.popularity,
            chapter_count: summary.chapter_count.unwrap_or(0),
            last_updated_remote: summary.last_updated.clone(),
            metadata: summary
                .language
                .as_deref()
                .map(|lang| serde_json::json!({ "language": lang }))
                .unwrap_or_else(|| serde_json::json!({})),
            is_local: summary.source_id == "local",
            created_at: now.clone(),
            updated_at: now,
        };
        self.upsert_novel(&record)
    }

    pub fn set_novel_local_cover(&self, novel_id: &str, local_cover_path: &str) -> AppResult<()> {
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE novels SET local_cover_path = ?2, updated_at = ?3 WHERE id = ?1",
                params![novel_id, local_cover_path, now_iso()],
            )?;
            Ok(())
        })
    }

    pub fn get_novel(&self, novel_id: &str) -> AppResult<Option<NovelRecord>> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT * FROM novels WHERE id = ?1",
                params![novel_id],
                row_to_novel,
            )
            .optional()
            .map_err(AppError::from)
        })
    }

    pub fn search_local_novels(&self, query: &str, limit: usize) -> AppResult<Vec<NovelRecord>> {
        let pattern = format!("%{}%", query.trim());
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                r#"
                SELECT * FROM novels
                WHERE title LIKE ?1 OR author_name LIKE ?1 OR genres_json LIKE ?1
                ORDER BY updated_at DESC
                LIMIT ?2
                "#,
            )?;
            let rows = stmt
                .query_map(params![pattern, limit as i64], row_to_novel)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    pub fn find_local_translations(&self, novel: &NovelRecord) -> AppResult<Vec<NovelTranslation>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT * FROM novels WHERE lower(trim(title)) = lower(trim(?1)) AND id != ?2",
            )?;
            let rows = stmt
                .query_map(params![novel.title, novel.id], row_to_novel)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows
                .into_iter()
                .map(|r| {
                    let lang = r
                        .metadata
                        .get("language")
                        .and_then(|v| v.as_str())
                        .unwrap_or("en")
                        .to_ascii_lowercase();
                    NovelTranslation {
                        language_label: lang.to_uppercase(),
                        language_code: lang,
                        novel_id: r.id,
                        source_id: r.source_id,
                        external_id: r.external_id,
                        title: r.title,
                    }
                })
                .collect())
        })
    }

    // -------------------------------------------------------------------------
    // Chapters
    // -------------------------------------------------------------------------
    pub fn upsert_chapters(&self, novel_id: &str, chapters: &[Chapter]) -> AppResult<()> {
        let now = now_iso();
        self.with_conn_mut(|conn| {
            let tx = conn.transaction()?;
            {
                let mut stmt = tx.prepare(
                    r#"
                    INSERT INTO chapters (
                        id, novel_id, external_id, chapter_number, volume,
                        title, url, word_count, published_at, is_downloaded,
                        local_path, size_bytes, created_at
                    )
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                    ON CONFLICT(novel_id, external_id) DO UPDATE SET
                        chapter_number = excluded.chapter_number,
                        volume = COALESCE(excluded.volume, chapters.volume),
                        title = excluded.title,
                        url = COALESCE(excluded.url, chapters.url),
                        word_count = CASE WHEN excluded.word_count > 0 THEN excluded.word_count ELSE chapters.word_count END,
                        published_at = COALESCE(excluded.published_at, chapters.published_at)
                    "#,
                )?;
                for ch in chapters {
                    stmt.execute(params![
                        ch.id,
                        novel_id,
                        ch.external_id,
                        ch.chapter_number,
                        ch.volume,
                        ch.title,
                        ch.url,
                        ch.word_count,
                        ch.published_at,
                        if ch.is_downloaded { 1 } else { 0 },
                        ch.local_path,
                        ch.size_bytes,
                        now,
                    ])?;
                }
            }
            tx.execute(
                "UPDATE novels SET chapter_count = (SELECT COUNT(*) FROM chapters WHERE novel_id = ?1), updated_at = ?2 WHERE id = ?1",
                params![novel_id, now],
            )?;
            tx.commit()?;
            Ok(())
        })
    }

    pub fn get_chapters(&self, novel_id: &str) -> AppResult<Vec<Chapter>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT * FROM chapters WHERE novel_id = ?1 ORDER BY chapter_number ASC, id ASC",
            )?;
            let rows = stmt
                .query_map(params![novel_id], row_to_chapter)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    pub fn get_chapter(&self, chapter_id: &str) -> AppResult<Option<Chapter>> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT * FROM chapters WHERE id = ?1",
                params![chapter_id],
                row_to_chapter,
            )
            .optional()
            .map_err(AppError::from)
        })
    }

    pub fn mark_chapter_downloaded(
        &self,
        chapter_id: &str,
        local_path: &str,
        size_bytes: i64,
        word_count: i64,
        content_hash: Option<&str>,
    ) -> AppResult<()> {
        self.with_conn(|conn| {
            conn.execute(
                r#"
                UPDATE chapters
                SET is_downloaded = 1,
                    local_path = ?2,
                    size_bytes = ?3,
                    word_count = CASE WHEN ?4 > 0 THEN ?4 ELSE word_count END,
                    content_hash = COALESCE(?5, content_hash)
                WHERE id = ?1
                "#,
                params![chapter_id, local_path, size_bytes, word_count, content_hash],
            )?;
            Ok(())
        })
    }

    pub fn clear_chapters_download_flags(&self, chapter_ids: &[String]) -> AppResult<Vec<String>> {
        if chapter_ids.is_empty() {
            return Ok(Vec::new());
        }
        self.with_conn_mut(|conn| {
            let tx = conn.transaction()?;
            let mut paths = Vec::new();
            for cid in chapter_ids {
                if let Some(Some(p)) = tx
                    .query_row(
                        "SELECT local_path FROM chapters WHERE id = ?1",
                        params![cid],
                        |r| r.get::<_, Option<String>>(0),
                    )
                    .optional()?
                {
                    paths.push(p);
                }
                tx.execute(
                    "UPDATE chapters SET is_downloaded = 0, local_path = NULL, size_bytes = 0 WHERE id = ?1",
                    params![cid],
                )?;
            }
            tx.commit()?;
            Ok(paths)
        })
    }

    pub fn clear_novel_downloads(&self, novel_id: &str) -> AppResult<Vec<String>> {
        self.with_conn_mut(|conn| {
            let tx = conn.transaction()?;
            let mut stmt = tx.prepare(
                "SELECT local_path FROM chapters WHERE novel_id = ?1 AND local_path IS NOT NULL",
            )?;
            let paths = stmt
                .query_map(params![novel_id], |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            drop(stmt);

            // Strictly ONLY clears download flags and file pointers;
            // reading_progress, bookmarks, library_entries, and reading_history are untouched!
            tx.execute(
                "UPDATE chapters SET is_downloaded = 0, local_path = NULL, size_bytes = 0 WHERE novel_id = ?1",
                params![novel_id],
            )?;
            tx.commit()?;
            Ok(paths)
        })
    }

    // -------------------------------------------------------------------------
    // Reading Progress
    // -------------------------------------------------------------------------
    pub fn save_reading_progress(
        &self,
        novel_id: &str,
        chapter_id: &str,
        scroll_offset: f64,
        paragraph_index: i64,
        page_index: i64,
        total_pages: i64,
    ) -> AppResult<ReadingProgress> {
        let now = now_iso();
        self.with_conn(|conn| {
            let (chapter_number, chapter_title): (f64, String) = conn
                .query_row(
                    "SELECT chapter_number, title FROM chapters WHERE id = ?1",
                    params![chapter_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map_err(|_| AppError::NotFound(format!("Chapter {chapter_id} not found")))?;

            let total_chapters: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM chapters WHERE novel_id = ?1",
                    params![novel_id],
                    |r| r.get(0),
                )
                .unwrap_or(1)
                .max(1);

            let chapter_pos: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM chapters WHERE novel_id = ?1 AND chapter_number < ?2",
                    params![novel_id, chapter_number],
                    |r| r.get(0),
                )
                .unwrap_or(0);

            let clamped_scroll = scroll_offset.clamp(0.0, 1.0);
            let progress_percent =
                (((chapter_pos as f64) + clamped_scroll) / (total_chapters as f64) * 100.0)
                    .clamp(0.0, 100.0);

            conn.execute(
                r#"
                INSERT INTO reading_progress (
                    novel_id, chapter_id, chapter_number, scroll_offset,
                    paragraph_index, page_index, total_pages, progress_percent, updated_at
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                ON CONFLICT(novel_id) DO UPDATE SET
                    chapter_id = excluded.chapter_id,
                    chapter_number = excluded.chapter_number,
                    scroll_offset = excluded.scroll_offset,
                    paragraph_index = excluded.paragraph_index,
                    page_index = excluded.page_index,
                    total_pages = excluded.total_pages,
                    progress_percent = excluded.progress_percent,
                    updated_at = excluded.updated_at
                "#,
                params![
                    novel_id,
                    chapter_id,
                    chapter_number,
                    clamped_scroll,
                    paragraph_index,
                    page_index,
                    total_pages.max(1),
                    progress_percent,
                    now,
                ],
            )?;

            // Also touch updated_at on library_entry if present
            conn.execute(
                "UPDATE library_entries SET updated_at = ?2 WHERE novel_id = ?1",
                params![novel_id, now],
            )?;

            Ok(ReadingProgress {
                novel_id: novel_id.to_string(),
                chapter_id: chapter_id.to_string(),
                chapter_number,
                chapter_title: Some(chapter_title),
                scroll_offset: clamped_scroll,
                paragraph_index,
                page_index,
                total_pages: total_pages.max(1),
                progress_percent,
                updated_at: now,
            })
        })
    }

    pub fn get_reading_progress(&self, novel_id: &str) -> AppResult<Option<ReadingProgress>> {
        self.with_conn(|conn| {
            conn.query_row(
                r#"
                SELECT rp.*, c.title AS chapter_title
                FROM reading_progress rp
                LEFT JOIN chapters c ON c.id = rp.chapter_id
                WHERE rp.novel_id = ?1
                "#,
                params![novel_id],
                |r| {
                    let ch_title: Option<String> = r.get("chapter_title")?;
                    row_to_progress(r, ch_title)
                },
            )
            .optional()
            .map_err(AppError::from)
        })
    }

    // -------------------------------------------------------------------------
    // Library Entries & Collections
    // -------------------------------------------------------------------------
    pub fn upsert_library_entry(
        &self,
        novel_id: &str,
        status: LibraryStatus,
        is_bookmarked: Option<bool>,
        follow_updates: Option<bool>,
    ) -> AppResult<LibraryEntry> {
        let now = now_iso();
        self.with_conn(|conn| {
            let existing: Option<LibraryEntry> = conn
                .query_row(
                    "SELECT * FROM library_entries WHERE novel_id = ?1",
                    params![novel_id],
                    row_to_library_entry,
                )
                .optional()?;

            let bm = is_bookmarked.unwrap_or_else(|| existing.as_ref().map(|e| e.is_bookmarked).unwrap_or(false));
            let follow = follow_updates.unwrap_or_else(|| existing.as_ref().map(|e| e.follow_updates).unwrap_or(false));
            let ch_count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM chapters WHERE novel_id = ?1",
                    params![novel_id],
                    |r| r.get(0),
                )
                .unwrap_or(0);

            conn.execute(
                r#"
                INSERT INTO library_entries (
                    novel_id, status, is_bookmarked, follow_updates, last_checked_chapters, added_at, updated_at
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
                ON CONFLICT(novel_id) DO UPDATE SET
                    status = excluded.status,
                    is_bookmarked = excluded.is_bookmarked,
                    follow_updates = excluded.follow_updates,
                    updated_at = excluded.updated_at
                "#,
                params![
                    novel_id,
                    status.as_str(),
                    if bm { 1 } else { 0 },
                    if follow { 1 } else { 0 },
                    ch_count,
                    now,
                ],
            )?;

            let entry = conn.query_row(
                "SELECT * FROM library_entries WHERE novel_id = ?1",
                params![novel_id],
                row_to_library_entry,
            )?;
            Ok(entry)
        })
    }

    pub fn get_library_entry(&self, novel_id: &str) -> AppResult<Option<LibraryEntry>> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT * FROM library_entries WHERE novel_id = ?1",
                params![novel_id],
                row_to_library_entry,
            )
            .optional()
            .map_err(AppError::from)
        })
    }

    pub fn remove_from_library(&self, novel_id: &str) -> AppResult<()> {
        self.with_conn(|conn| {
            conn.execute("DELETE FROM library_entries WHERE novel_id = ?1", params![novel_id])?;
            conn.execute("DELETE FROM collection_items WHERE novel_id = ?1", params![novel_id])?;
            Ok(())
        })
    }

    pub fn get_followed_novels(&self) -> AppResult<Vec<(NovelRecord, LibraryEntry)>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                r#"
                SELECT n.*, le.status AS le_status, le.is_bookmarked, le.follow_updates,
                       le.last_checked_chapters, le.added_at, le.updated_at AS le_updated_at
                FROM library_entries le
                JOIN novels n ON n.id = le.novel_id
                WHERE le.follow_updates = 1 AND n.is_local = 0
                "#,
            )?;
            let rows = stmt
                .query_map([], |row| {
                    let novel = row_to_novel(row)?;
                    let status_str: String = row.get("le_status")?;
                    let is_bm: i64 = row.get("is_bookmarked")?;
                    let follow: i64 = row.get("follow_updates")?;
                    let entry = LibraryEntry {
                        novel_id: novel.id.clone(),
                        status: LibraryStatus::from_str_opt(&status_str).unwrap_or(LibraryStatus::Reading),
                        is_bookmarked: is_bm != 0,
                        follow_updates: follow != 0,
                        last_checked_chapters: row.get("last_checked_chapters")?,
                        added_at: row.get("added_at")?,
                        updated_at: row.get("le_updated_at")?,
                    };
                    Ok((novel, entry))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    pub fn update_last_checked_chapters(&self, novel_id: &str, count: i64) -> AppResult<()> {
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE library_entries SET last_checked_chapters = ?2, updated_at = ?3 WHERE novel_id = ?1",
                params![novel_id, count, now_iso()],
            )?;
            Ok(())
        })
    }

    pub fn list_library_items(&self) -> AppResult<Vec<LibraryNovelItem>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                r#"
                SELECT n.*,
                       le.status AS le_status,
                       le.is_bookmarked,
                       le.follow_updates,
                       le.last_checked_chapters,
                       le.added_at,
                       le.updated_at AS le_updated_at
                FROM library_entries le
                JOIN novels n ON n.id = le.novel_id
                ORDER BY le.updated_at DESC
                "#,
            )?;
            let pairs: Vec<(NovelRecord, LibraryEntry)> = stmt
                .query_map([], |row| {
                    let novel = row_to_novel(row)?;
                    let status_str: String = row.get("le_status")?;
                    let is_bm: i64 = row.get("is_bookmarked")?;
                    let follow: i64 = row.get("follow_updates")?;
                    let entry = LibraryEntry {
                        novel_id: novel.id.clone(),
                        status: LibraryStatus::from_str_opt(&status_str).unwrap_or(LibraryStatus::Reading),
                        is_bookmarked: is_bm != 0,
                        follow_updates: follow != 0,
                        last_checked_chapters: row.get("last_checked_chapters")?,
                        added_at: row.get("added_at")?,
                        updated_at: row.get("le_updated_at")?,
                    };
                    Ok((novel, entry))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            drop(stmt);

            let mut items = Vec::with_capacity(pairs.len());
            for (novel, entry) in pairs {
                let progress = conn
                    .query_row(
                        r#"
                        SELECT rp.*, c.title AS chapter_title
                        FROM reading_progress rp
                        LEFT JOIN chapters c ON c.id = rp.chapter_id
                        WHERE rp.novel_id = ?1
                        "#,
                        params![novel.id],
                        |r| {
                            let ch_title: Option<String> = r.get("chapter_title")?;
                            row_to_progress(r, ch_title)
                        },
                    )
                    .optional()?;

                let latest_chapter = conn
                    .query_row(
                        "SELECT * FROM chapters WHERE novel_id = ?1 ORDER BY chapter_number DESC LIMIT 1",
                        params![novel.id],
                        row_to_chapter,
                    )
                    .optional()?;

                let (downloaded_chapters, total_size_bytes): (i64, i64) = conn
                    .query_row(
                        "SELECT COALESCE(SUM(CASE WHEN is_downloaded = 1 THEN 1 ELSE 0 END), 0), COALESCE(SUM(size_bytes), 0) FROM chapters WHERE novel_id = ?1",
                        params![novel.id],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .unwrap_or((0, 0));

                let mut col_stmt = conn.prepare(
                    "SELECT collection_id FROM collection_items WHERE novel_id = ?1",
                )?;
                let collection_ids = col_stmt
                    .query_map(params![novel.id], |r| r.get::<_, String>(0))?
                    .collect::<Result<Vec<_>, _>>()?;

                items.push(LibraryNovelItem {
                    novel,
                    entry,
                    progress,
                    latest_chapter,
                    downloaded_chapters,
                    total_size_bytes,
                    collection_ids,
                });
            }
            Ok(items)
        })
    }

    pub fn get_novel_collection_ids(&self, novel_id: &str) -> AppResult<Vec<String>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT collection_id FROM collection_items WHERE novel_id = ?1",
            )?;
            let ids = stmt
                .query_map(params![novel_id], |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(ids)
        })
    }

    // -------------------------------------------------------------------------
    // Collections
    // -------------------------------------------------------------------------
    pub fn list_collections(&self) -> AppResult<Vec<Collection>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                r#"
                SELECT c.id, c.name, c.description, c.color, c.sort_order, c.created_at,
                       COUNT(ci.novel_id) AS item_count
                FROM collections c
                LEFT JOIN collection_items ci ON ci.collection_id = c.id
                GROUP BY c.id
                ORDER BY c.sort_order ASC, c.name ASC
                "#,
            )?;
            let rows = stmt
                .query_map([], |row| {
                    Ok(Collection {
                        id: row.get("id")?,
                        name: row.get("name")?,
                        description: row.get("description")?,
                        color: row.get("color")?,
                        sort_order: row.get("sort_order")?,
                        item_count: row.get("item_count")?,
                        created_at: row.get("created_at")?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    pub fn create_collection(
        &self,
        name: &str,
        description: Option<&str>,
        color: Option<&str>,
    ) -> AppResult<Collection> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(AppError::InvalidInput("Collection name cannot be empty".into()));
        }
        let id = Uuid::new_v4().to_string();
        let now = now_iso();
        self.with_conn(|conn| {
            let max_order: i32 = conn
                .query_row("SELECT COALESCE(MAX(sort_order), 0) FROM collections", [], |r| r.get(0))
                .unwrap_or(0);
            conn.execute(
                "INSERT INTO collections (id, name, description, color, sort_order, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![id, trimmed, description, color.unwrap_or("#6366F1"), max_order + 1, now],
            )?;
            Ok(Collection {
                id,
                name: trimmed.to_string(),
                description: description.map(ToString::to_string),
                color: color.map(ToString::to_string).or(Some("#6366F1".to_string())),
                sort_order: max_order + 1,
                item_count: 0,
                created_at: now,
            })
        })
    }

    pub fn delete_collection(&self, id: &str) -> AppResult<()> {
        self.with_conn(|conn| {
            conn.execute("DELETE FROM collections WHERE id = ?1", params![id])?;
            Ok(())
        })
    }

    pub fn set_novel_in_collection(
        &self,
        collection_id: &str,
        novel_id: &str,
        present: bool,
    ) -> AppResult<()> {
        self.with_conn(|conn| {
            if present {
                conn.execute(
                    "INSERT OR IGNORE INTO collection_items (collection_id, novel_id, added_at) VALUES (?1, ?2, ?3)",
                    params![collection_id, novel_id, now_iso()],
                )?;
            } else {
                conn.execute(
                    "DELETE FROM collection_items WHERE collection_id = ?1 AND novel_id = ?2",
                    params![collection_id, novel_id],
                )?;
            }
            Ok(())
        })
    }

    // -------------------------------------------------------------------------
    // Bookmarks
    // -------------------------------------------------------------------------
    #[allow(clippy::too_many_arguments)]
    pub fn add_bookmark(
        &self,
        novel_id: &str,
        chapter_id: Option<&str>,
        paragraph_index: Option<i64>,
        excerpt: Option<&str>,
        note: Option<&str>,
        color: Option<&str>,
        is_novel_bookmark: bool,
    ) -> AppResult<Bookmark> {
        let id = Uuid::new_v4().to_string();
        let now = now_iso();
        let resolved_color = color
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("#818CF8");
        self.with_conn(|conn| {
            conn.execute(
                r#"
                INSERT INTO bookmarks (id, novel_id, chapter_id, paragraph_index, excerpt, note, color, is_novel_bookmark, created_at)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                "#,
                params![
                    id,
                    novel_id,
                    chapter_id,
                    paragraph_index,
                    excerpt,
                    note,
                    resolved_color,
                    if is_novel_bookmark { 1 } else { 0 },
                    now
                ],
            )?;
            let bms = Self::query_bookmarks_inner(conn, Some(&id), None)?;
            bms.into_iter()
                .next()
                .ok_or_else(|| AppError::Other("Failed to read created bookmark".into()))
        })
    }

    pub fn list_bookmarks(&self, novel_id: Option<&str>) -> AppResult<Vec<Bookmark>> {
        self.with_conn(|conn| Self::query_bookmarks_inner(conn, None, novel_id))
    }

    fn query_bookmarks_inner(
        conn: &rusqlite::Connection,
        bookmark_id: Option<&str>,
        novel_id: Option<&str>,
    ) -> AppResult<Vec<Bookmark>> {
        let mut sql = String::from(
            r#"
            SELECT b.*, n.title AS novel_title, n.author_name, n.cover_url, n.local_cover_path,
                   c.chapter_number, c.title AS chapter_title
            FROM bookmarks b
            LEFT JOIN novels n ON n.id = b.novel_id
            LEFT JOIN chapters c ON c.id = b.chapter_id
            WHERE 1=1
            "#,
        );
        if bookmark_id.is_some() {
            sql.push_str(" AND b.id = ?1");
        } else if novel_id.is_some() {
            sql.push_str(" AND b.novel_id = ?1");
        }
        sql.push_str(" ORDER BY b.created_at DESC");

        let mut stmt = conn.prepare(&sql)?;
        let mapper = |row: &Row<'_>| -> rusqlite::Result<Bookmark> {
            let is_nb: i64 = row.get("is_novel_bookmark")?;
            let color: String = row
                .get::<_, Option<String>>("color")?
                .unwrap_or_else(|| "#818CF8".into());
            Ok(Bookmark {
                id: row.get("id")?,
                novel_id: row.get("novel_id")?,
                novel_title: row.get("novel_title")?,
                author: row.get("author_name")?,
                cover_url: row.get("cover_url")?,
                local_cover_path: row.get("local_cover_path")?,
                chapter_id: row.get("chapter_id")?,
                chapter_number: row.get("chapter_number")?,
                chapter_title: row.get("chapter_title")?,
                paragraph_index: row.get("paragraph_index")?,
                excerpt: row.get("excerpt")?,
                note: row.get("note")?,
                color,
                is_novel_bookmark: is_nb != 0,
                created_at: row.get("created_at")?,
            })
        };

        let items = if let Some(bid) = bookmark_id {
            stmt.query_map(params![bid], mapper)?
                .collect::<Result<Vec<_>, _>>()?
        } else if let Some(nid) = novel_id {
            stmt.query_map(params![nid], mapper)?
                .collect::<Result<Vec<_>, _>>()?
        } else {
            stmt.query_map([], mapper)?
                .collect::<Result<Vec<_>, _>>()?
        };
        Ok(items)
    }

    pub fn delete_bookmark(&self, bookmark_id: &str) -> AppResult<()> {
        self.with_conn(|conn| {
            conn.execute("DELETE FROM bookmarks WHERE id = ?1", params![bookmark_id])?;
            Ok(())
        })
    }

    // -------------------------------------------------------------------------
    // Highlights
    // -------------------------------------------------------------------------
    #[allow(clippy::too_many_arguments)]
    pub fn add_highlight(
        &self,
        novel_id: &str,
        chapter_id: &str,
        paragraph_index: i64,
        start_offset: i64,
        end_offset: i64,
        text: &str,
        color: Option<&str>,
        note: Option<&str>,
    ) -> AppResult<Highlight> {
        let id = Uuid::new_v4().to_string();
        let now = now_iso();
        let resolved_color = color
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("#FDE047");
        self.with_conn(|conn| {
            conn.execute(
                r#"
                INSERT INTO highlights (id, novel_id, chapter_id, paragraph_index, start_offset, end_offset, text, color, note, created_at)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                "#,
                params![
                    id,
                    novel_id,
                    chapter_id,
                    paragraph_index,
                    start_offset.max(0),
                    end_offset.max(0),
                    text,
                    resolved_color,
                    note,
                    now
                ],
            )?;
            Ok(Highlight {
                id,
                novel_id: novel_id.to_string(),
                chapter_id: chapter_id.to_string(),
                paragraph_index,
                start_offset: start_offset.max(0),
                end_offset: end_offset.max(0),
                text: text.to_string(),
                color: resolved_color.to_string(),
                note: note.map(ToString::to_string),
                created_at: now,
            })
        })
    }

    pub fn list_highlights(
        &self,
        novel_id: &str,
        chapter_id: Option<&str>,
    ) -> AppResult<Vec<Highlight>> {
        self.with_conn(|conn| {
            let sql = if chapter_id.is_some() {
                "SELECT * FROM highlights WHERE novel_id = ?1 AND chapter_id = ?2 ORDER BY paragraph_index ASC, start_offset ASC, created_at ASC"
            } else {
                "SELECT * FROM highlights WHERE novel_id = ?1 ORDER BY chapter_id ASC, paragraph_index ASC, start_offset ASC"
            };
            let mut stmt = conn.prepare(sql)?;
            let mapper = |r: &Row<'_>| -> rusqlite::Result<Highlight> {
                Ok(Highlight {
                    id: r.get("id")?,
                    novel_id: r.get("novel_id")?,
                    chapter_id: r.get("chapter_id")?,
                    paragraph_index: r.get("paragraph_index")?,
                    start_offset: r.get("start_offset")?,
                    end_offset: r.get("end_offset")?,
                    text: r.get("text")?,
                    color: r
                        .get::<_, Option<String>>("color")?
                        .unwrap_or_else(|| "#FDE047".into()),
                    note: r.get("note")?,
                    created_at: r.get("created_at")?,
                })
            };
            let rows = if let Some(cid) = chapter_id {
                stmt.query_map(params![novel_id, cid], mapper)?
                    .collect::<Result<Vec<_>, _>>()?
            } else {
                stmt.query_map(params![novel_id], mapper)?
                    .collect::<Result<Vec<_>, _>>()?
            };
            Ok(rows)
        })
    }

    pub fn delete_highlight(&self, highlight_id: &str) -> AppResult<()> {
        self.with_conn(|conn| {
            conn.execute("DELETE FROM highlights WHERE id = ?1", params![highlight_id])?;
            Ok(())
        })
    }

    // -------------------------------------------------------------------------
    // Reading History & Stats
    // -------------------------------------------------------------------------
    pub fn record_reading_history(
        &self,
        novel_id: &str,
        chapter_id: &str,
        duration_seconds: i64,
        words_read: i64,
    ) -> AppResult<()> {
        let now = now_iso();
        let id = Uuid::new_v4().to_string();
        self.with_conn(|conn| {
            // Coalesce recent consecutive reads of the same chapter within 15 minutes
            // using RFC3339 format so 'T' separators compare consistently with read_at.
            let cutoff = (Utc::now() - Duration::minutes(15)).to_rfc3339();
            let recent_id: Option<String> = conn
                .query_row(
                    r#"
                    SELECT id FROM reading_history
                    WHERE novel_id = ?1 AND chapter_id = ?2
                      AND read_at >= ?3
                    ORDER BY read_at DESC LIMIT 1
                    "#,
                    params![novel_id, chapter_id, cutoff],
                    |r| r.get(0),
                )
                .optional()?;

            if let Some(rid) = recent_id {
                conn.execute(
                    r#"
                    UPDATE reading_history
                    SET duration_seconds = duration_seconds + ?2,
                        words_read = MAX(words_read, ?3),
                        read_at = ?4
                    WHERE id = ?1
                    "#,
                    params![rid, duration_seconds.max(0), words_read.max(0), now],
                )?;
            } else {
                conn.execute(
                    r#"
                    INSERT INTO reading_history (id, novel_id, chapter_id, duration_seconds, words_read, read_at)
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                    "#,
                    params![id, novel_id, chapter_id, duration_seconds.max(0), words_read.max(0), now],
                )?;
            }
            Ok(())
        })
    }

    pub fn list_reading_history(&self, limit: usize) -> AppResult<Vec<ReadingHistoryItem>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                r#"
                SELECT rh.id, rh.novel_id, n.title AS novel_title, n.author_name,
                       n.cover_url, n.local_cover_path, rh.chapter_id,
                       c.chapter_number, c.title AS chapter_title,
                       rh.duration_seconds, rh.words_read, rh.read_at
                FROM reading_history rh
                JOIN novels n ON n.id = rh.novel_id
                JOIN chapters c ON c.id = rh.chapter_id
                ORDER BY rh.read_at DESC
                LIMIT ?1
                "#,
            )?;
            let rows = stmt
                .query_map(params![limit as i64], |r| {
                    Ok(ReadingHistoryItem {
                        id: r.get("id")?,
                        novel_id: r.get("novel_id")?,
                        novel_title: r.get("novel_title")?,
                        author: r.get("author_name")?,
                        cover_url: r.get("cover_url")?,
                        local_cover_path: r.get("local_cover_path")?,
                        chapter_id: r.get("chapter_id")?,
                        chapter_number: r.get("chapter_number")?,
                        chapter_title: r.get("chapter_title")?,
                        duration_seconds: r.get("duration_seconds")?,
                        words_read: r.get("words_read")?,
                        read_at: r.get("read_at")?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    pub fn clear_reading_history(&self) -> AppResult<()> {
        self.with_conn(|conn| {
            conn.execute("DELETE FROM reading_history", [])?;
            Ok(())
        })
    }

    pub fn get_reading_stats(&self) -> AppResult<ReadingStats> {
        self.with_conn(|conn| {
            let (total_reading_seconds, total_words_read, chapters_read_count): (i64, i64, i64) =
                conn.query_row(
                    r#"
                    SELECT COALESCE(SUM(duration_seconds), 0),
                           COALESCE(SUM(words_read), 0),
                           COUNT(DISTINCT chapter_id)
                    FROM reading_history
                    "#,
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )?;

            let novels_finished_count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM library_entries WHERE status = 'finished'",
                    [],
                    |r| r.get(0),
                )
                .unwrap_or(0);

            let mut stmt = conn.prepare(
                "SELECT substr(read_at, 1, 10) AS day, COALESCE(SUM(duration_seconds), 0), COALESCE(SUM(words_read), 0) FROM reading_history GROUP BY day ORDER BY day DESC",
            )?;
            let mut by_day: HashMap<String, (i64, i64)> = HashMap::new();
            let mut active_days: HashSet<String> = HashSet::new();
            let day_rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            })?;
            for dr in day_rows {
                let (d, secs, words) = dr?;
                by_day.insert(d.clone(), (secs, words));
                active_days.insert(d);
            }

            let today = Utc::now().date_naive();
            let mut streak = 0i64;
            let mut cursor = if active_days.contains(&today.format("%Y-%m-%d").to_string()) {
                today
            } else {
                today - Duration::days(1)
            };
            while active_days.contains(&cursor.format("%Y-%m-%d").to_string()) {
                streak += 1;
                cursor -= Duration::days(1);
            }

            let mut daily_7d = Vec::with_capacity(7);
            for offset in (0..7).rev() {
                let d = today - Duration::days(offset);
                let key = d.format("%Y-%m-%d").to_string();
                let (secs, words) = by_day.get(&key).copied().unwrap_or((0, 0));
                daily_7d.push(DailyReadingPoint {
                    date: key,
                    minutes: (secs + 59) / 60,
                    words,
                });
            }

            Ok(ReadingStats {
                total_reading_seconds,
                total_words_read,
                chapters_read_count,
                novels_finished_count,
                current_streak_days: streak,
                daily_7d,
            })
        })
    }

    pub fn get_home_dashboard(&self) -> AppResult<HomeDashboardData> {
        let all = self.list_library_items()?;
        let stats = self.get_reading_stats()?;

        let mut continue_reading: Vec<LibraryNovelItem> = all
            .iter()
            .filter(|i| i.progress.is_some() && i.entry.status != LibraryStatus::Finished)
            .cloned()
            .collect();
        continue_reading.sort_by(|a, b| {
            let ta = a.progress.as_ref().map(|p| p.updated_at.as_str()).unwrap_or("");
            let tb = b.progress.as_ref().map(|p| p.updated_at.as_str()).unwrap_or("");
            tb.cmp(ta)
        });
        continue_reading.truncate(8);

        let mut recently_updated = all.clone();
        recently_updated.sort_by(|a, b| b.novel.updated_at.cmp(&a.novel.updated_at));
        recently_updated.truncate(10);

        let mut recently_added = all.clone();
        recently_added.sort_by(|a, b| b.entry.added_at.cmp(&a.entry.added_at));
        recently_added.truncate(10);

        let mut your_library = all;
        your_library.truncate(12);

        Ok(HomeDashboardData {
            continue_reading,
            recently_updated,
            recently_added,
            your_library,
            stats,
        })
    }

    // -------------------------------------------------------------------------
    // Downloads
    // -------------------------------------------------------------------------
    pub fn insert_download_task(&self, task: &DownloadTask) -> AppResult<()> {
        let chapter_ids_json = serde_json::to_string(&task.chapter_ids)?;
        self.with_conn(|conn| {
            conn.execute(
                r#"
                INSERT INTO downloads (
                    id, novel_id, chapter_ids_json, total_chapters,
                    completed_chapters, failed_chapters, bytes_downloaded,
                    status, error_message, created_at, updated_at
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                "#,
                params![
                    task.id,
                    task.novel_id,
                    chapter_ids_json,
                    task.total_chapters,
                    task.completed_chapters,
                    task.failed_chapters,
                    task.bytes_downloaded,
                    task.status.as_str(),
                    task.error_message,
                    task.created_at,
                    task.updated_at,
                ],
            )?;
            Ok(())
        })
    }

    pub fn update_download_task(&self, task: &DownloadTask) -> AppResult<()> {
        let now = now_iso();
        self.with_conn(|conn| {
            conn.execute(
                r#"
                UPDATE downloads
                SET completed_chapters = ?2,
                    failed_chapters = ?3,
                    bytes_downloaded = ?4,
                    status = ?5,
                    error_message = ?6,
                    updated_at = ?7
                WHERE id = ?1
                "#,
                params![
                    task.id,
                    task.completed_chapters,
                    task.failed_chapters,
                    task.bytes_downloaded,
                    task.status.as_str(),
                    task.error_message,
                    now,
                ],
            )?;
            Ok(())
        })
    }

    pub fn list_download_tasks(&self) -> AppResult<Vec<DownloadTask>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                r#"
                SELECT d.*, n.title AS novel_title, n.cover_url, n.local_cover_path, n.source_id
                FROM downloads d
                JOIN novels n ON n.id = d.novel_id
                ORDER BY d.created_at DESC
                LIMIT 100
                "#,
            )?;
            let rows = stmt
                .query_map([], |r| {
                    let cids_json: String = r.get("chapter_ids_json")?;
                    let status_str: String = r.get("status")?;
                    Ok(DownloadTask {
                        id: r.get("id")?,
                        novel_id: r.get("novel_id")?,
                        novel_title: r.get("novel_title")?,
                        cover_url: r.get("cover_url")?,
                        local_cover_path: r.get("local_cover_path")?,
                        source_id: r.get("source_id")?,
                        chapter_ids: serde_json::from_str(&cids_json).unwrap_or_default(),
                        total_chapters: r.get("total_chapters")?,
                        completed_chapters: r.get("completed_chapters")?,
                        failed_chapters: r.get("failed_chapters")?,
                        bytes_downloaded: r.get("bytes_downloaded")?,
                        speed_bytes_per_sec: 0.0,
                        status: DownloadStatus::from_str_lossy(&status_str),
                        error_message: r.get("error_message")?,
                        created_at: r.get("created_at")?,
                        updated_at: r.get("updated_at")?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    pub fn delete_download_task_record(&self, task_id: &str) -> AppResult<()> {
        self.with_conn(|conn| {
            conn.execute("DELETE FROM downloads WHERE id = ?1", params![task_id])?;
            Ok(())
        })
    }

    pub fn get_novel_storage_usages(&self) -> AppResult<Vec<NovelStorageUsage>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                r#"
                SELECT n.id, n.title, n.author_name, n.cover_url, n.local_cover_path,
                       SUM(CASE WHEN c.is_downloaded = 1 THEN 1 ELSE 0 END) AS dl_count,
                       COUNT(c.id) AS total_count,
                       COALESCE(SUM(c.size_bytes), 0) AS total_bytes
                FROM novels n
                JOIN chapters c ON c.novel_id = n.id
                GROUP BY n.id
                HAVING dl_count > 0 OR total_bytes > 0
                ORDER BY total_bytes DESC
                "#,
            )?;
            let rows = stmt
                .query_map([], |r| {
                    Ok(NovelStorageUsage {
                        novel_id: r.get(0)?,
                        title: r.get(1)?,
                        author: r.get(2)?,
                        cover_url: r.get(3)?,
                        local_cover_path: r.get(4)?,
                        downloaded_chapters: r.get(5)?,
                        total_chapters: r.get(6)?,
                        size_bytes: r.get(7)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    // -------------------------------------------------------------------------
    // Settings
    // -------------------------------------------------------------------------
    pub fn load_settings(&self, default_download_dir: &str) -> AppResult<AppSettings> {
        let default = AppSettings::default_with_dir(default_download_dir.to_string());
        self.with_conn(|conn| {
            let stored: Option<String> = conn
                .query_row(
                    "SELECT value_json FROM settings WHERE key = 'app_settings'",
                    [],
                    |r| r.get(0),
                )
                .optional()?;
            match stored {
                Some(json_str) => match serde_json::from_str::<AppSettings>(&json_str) {
                    Ok(mut s) => {
                        if s.download_dir.trim().is_empty() {
                            s.download_dir = default_download_dir.to_string();
                        }
                        Ok(s)
                    }
                    Err(_) => Ok(default),
                },
                None => Ok(default),
            }
        })
    }

    pub fn save_settings(&self, settings: &AppSettings) -> AppResult<()> {
        let json_str = serde_json::to_string(settings)?;
        let now = now_iso();
        self.with_conn(|conn| {
            conn.execute(
                r#"
                INSERT INTO settings (key, value_json, updated_at)
                VALUES ('app_settings', ?1, ?2)
                ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json, updated_at = excluded.updated_at
                "#,
                params![json_str, now],
            )?;
            Ok(())
        })
    }

    // -------------------------------------------------------------------------
    // Export / Import Library Bundle
    // -------------------------------------------------------------------------
    pub fn export_bundle(&self, default_download_dir: &str) -> AppResult<LibraryExportBundle> {
        let settings = self.load_settings(default_download_dir)?;
        let collections = self.list_collections()?;
        let bookmarks = self.list_bookmarks(None)?;
        let reading_history = self.list_reading_history(1000)?;

        self.with_conn(|conn| {
            let mut n_stmt = conn.prepare("SELECT * FROM novels")?;
            let novels = n_stmt
                .query_map([], row_to_novel)?
                .collect::<Result<Vec<_>, _>>()?;

            let mut c_stmt = conn.prepare("SELECT * FROM chapters ORDER BY novel_id, chapter_number")?;
            let chapters = c_stmt
                .query_map([], row_to_chapter)?
                .collect::<Result<Vec<_>, _>>()?;

            let mut l_stmt = conn.prepare("SELECT * FROM library_entries")?;
            let library_entries = l_stmt
                .query_map([], row_to_library_entry)?
                .collect::<Result<Vec<_>, _>>()?;

            let mut rp_stmt = conn.prepare(
                "SELECT rp.*, c.title AS chapter_title FROM reading_progress rp LEFT JOIN chapters c ON c.id = rp.chapter_id",
            )?;
            let reading_progress = rp_stmt
                .query_map([], |r| {
                    let t: Option<String> = r.get("chapter_title")?;
                    row_to_progress(r, t)
                })?
                .collect::<Result<Vec<_>, _>>()?;

            let mut ci_stmt = conn.prepare("SELECT collection_id, novel_id, added_at FROM collection_items")?;
            let collection_items = ci_stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect::<Result<Vec<_>, _>>()?;

            let mut hl_stmt = conn.prepare("SELECT * FROM highlights ORDER BY created_at ASC")?;
            let highlights = hl_stmt
                .query_map([], |r| {
                    Ok(Highlight {
                        id: r.get("id")?,
                        novel_id: r.get("novel_id")?,
                        chapter_id: r.get("chapter_id")?,
                        paragraph_index: r.get("paragraph_index")?,
                        start_offset: r.get("start_offset")?,
                        end_offset: r.get("end_offset")?,
                        text: r.get("text")?,
                        color: r
                            .get::<_, Option<String>>("color")?
                            .unwrap_or_else(|| "#FDE047".into()),
                        note: r.get("note")?,
                        created_at: r.get("created_at")?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;

            Ok(LibraryExportBundle {
                version: 1,
                exported_at: now_iso(),
                novels,
                chapters,
                library_entries,
                reading_progress,
                bookmarks,
                highlights,
                collections,
                collection_items,
                reading_history,
                settings: Some(settings),
            })
        })
    }

    pub fn import_bundle(&self, bundle: &LibraryExportBundle) -> AppResult<(usize, usize)> {
        let mut novels_count = 0usize;
        for novel in &bundle.novels {
            let mut sanitized = novel.clone();
            if let Some(ref cp) = sanitized.local_cover_path {
                if !std::path::Path::new(cp).exists() {
                    sanitized.local_cover_path = None;
                }
            }
            self.upsert_novel(&sanitized)?;
            novels_count += 1;
        }

        let mut chapters_by_novel: HashMap<&str, Vec<Chapter>> = HashMap::new();
        for ch in &bundle.chapters {
            let mut sanitized = ch.clone();
            let has_local_file = sanitized
                .local_path
                .as_deref()
                .map(|p| std::path::Path::new(p).exists())
                .unwrap_or(false);
            if !has_local_file {
                sanitized.is_downloaded = false;
                sanitized.local_path = None;
                sanitized.size_bytes = 0;
            }
            chapters_by_novel
                .entry(ch.novel_id.as_str())
                .or_default()
                .push(sanitized);
        }
        for (nid, chs) in &chapters_by_novel {
            self.upsert_chapters(nid, chs)?;
        }

        let mut entries_count = 0usize;
        self.with_conn_mut(|conn| {
            let tx = conn.transaction()?;

            for col in &bundle.collections {
                tx.execute(
                    "INSERT OR IGNORE INTO collections (id, name, description, color, sort_order, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![col.id, col.name, col.description, col.color, col.sort_order, col.created_at],
                )?;
            }

            let novel_exists = |tx: &rusqlite::Transaction<'_>, nid: &str| -> rusqlite::Result<bool> {
                Ok(tx
                    .query_row("SELECT 1 FROM novels WHERE id = ?1", params![nid], |_| Ok(()))
                    .optional()?
                    .is_some())
            };
            let chapter_exists = |tx: &rusqlite::Transaction<'_>, cid: &str| -> rusqlite::Result<bool> {
                Ok(tx
                    .query_row("SELECT 1 FROM chapters WHERE id = ?1", params![cid], |_| Ok(()))
                    .optional()?
                    .is_some())
            };

            for (cid, nid, added_at) in &bundle.collection_items {
                let col_ok = tx
                    .query_row("SELECT 1 FROM collections WHERE id = ?1", params![cid], |_| Ok(()))
                    .optional()?
                    .is_some();
                if col_ok && novel_exists(&tx, nid)? {
                    tx.execute(
                        "INSERT OR IGNORE INTO collection_items (collection_id, novel_id, added_at) VALUES (?1, ?2, ?3)",
                        params![cid, nid, added_at],
                    )?;
                }
            }

            for entry in &bundle.library_entries {
                if !novel_exists(&tx, &entry.novel_id)? {
                    continue;
                }
                tx.execute(
                    r#"
                    INSERT INTO library_entries (novel_id, status, is_bookmarked, follow_updates, last_checked_chapters, added_at, updated_at)
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                    ON CONFLICT(novel_id) DO UPDATE SET
                        status = excluded.status,
                        is_bookmarked = excluded.is_bookmarked,
                        follow_updates = excluded.follow_updates,
                        updated_at = excluded.updated_at
                    "#,
                    params![
                        entry.novel_id,
                        entry.status.as_str(),
                        if entry.is_bookmarked { 1 } else { 0 },
                        if entry.follow_updates { 1 } else { 0 },
                        entry.last_checked_chapters,
                        entry.added_at,
                        entry.updated_at,
                    ],
                )?;
                entries_count += 1;
            }

            for rp in &bundle.reading_progress {
                if !novel_exists(&tx, &rp.novel_id)? || !chapter_exists(&tx, &rp.chapter_id)? {
                    continue;
                }
                tx.execute(
                    r#"
                    INSERT INTO reading_progress (novel_id, chapter_id, chapter_number, scroll_offset, paragraph_index, page_index, total_pages, progress_percent, updated_at)
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                    ON CONFLICT(novel_id) DO UPDATE SET
                        chapter_id = excluded.chapter_id,
                        chapter_number = excluded.chapter_number,
                        scroll_offset = excluded.scroll_offset,
                        paragraph_index = excluded.paragraph_index,
                        page_index = excluded.page_index,
                        total_pages = excluded.total_pages,
                        progress_percent = excluded.progress_percent,
                        updated_at = excluded.updated_at
                    "#,
                    params![
                        rp.novel_id,
                        rp.chapter_id,
                        rp.chapter_number,
                        rp.scroll_offset,
                        rp.paragraph_index,
                        rp.page_index,
                        rp.total_pages,
                        rp.progress_percent,
                        rp.updated_at,
                    ],
                )?;
            }

            for bm in &bundle.bookmarks {
                if !novel_exists(&tx, &bm.novel_id)? {
                    continue;
                }
                let valid_chapter_id = match bm.chapter_id.as_deref() {
                    Some(cid) if chapter_exists(&tx, cid)? => Some(cid.to_string()),
                    _ => None,
                };
                tx.execute(
                    "INSERT OR IGNORE INTO bookmarks (id, novel_id, chapter_id, paragraph_index, excerpt, note, color, is_novel_bookmark, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![
                        bm.id,
                        bm.novel_id,
                        valid_chapter_id,
                        bm.paragraph_index,
                        bm.excerpt,
                        bm.note,
                        bm.color,
                        if bm.is_novel_bookmark { 1 } else { 0 },
                        bm.created_at,
                    ],
                )?;
            }

            for hl in &bundle.highlights {
                if !novel_exists(&tx, &hl.novel_id)? || !chapter_exists(&tx, &hl.chapter_id)? {
                    continue;
                }
                tx.execute(
                    "INSERT OR IGNORE INTO highlights (id, novel_id, chapter_id, paragraph_index, start_offset, end_offset, text, color, note, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                    params![
                        hl.id,
                        hl.novel_id,
                        hl.chapter_id,
                        hl.paragraph_index,
                        hl.start_offset,
                        hl.end_offset,
                        hl.text,
                        hl.color,
                        hl.note,
                        hl.created_at,
                    ],
                )?;
            }

            for rh in &bundle.reading_history {
                if !novel_exists(&tx, &rh.novel_id)? || !chapter_exists(&tx, &rh.chapter_id)? {
                    continue;
                }
                tx.execute(
                    "INSERT OR IGNORE INTO reading_history (id, novel_id, chapter_id, duration_seconds, words_read, read_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![rh.id, rh.novel_id, rh.chapter_id, rh.duration_seconds, rh.words_read, rh.read_at],
                )?;
            }

            tx.commit()?;
            Ok(())
        })?;

        if let Some(settings) = &bundle.settings {
            self.save_settings(settings)?;
        }

        Ok((novels_count, entries_count))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_novel(id: &str) -> NovelRecord {
        NovelRecord {
            id: id.to_string(),
            source_id: "gutenberg".to_string(),
            external_id: "1342".to_string(),
            title: "Pride and Prejudice".to_string(),
            author_id: None,
            author: "Jane Austen".to_string(),
            description: "Classic English novel.".to_string(),
            cover_url: Some("https://example.org/cover.jpg".to_string()),
            local_cover_path: None,
            status: NovelStatus::Completed,
            genres: vec!["Romance".into(), "Classics".into()],
            tags: vec!["Regency".into()],
            rating: Some(4.8),
            popularity: 9000,
            chapter_count: 2,
            last_updated_remote: None,
            metadata: serde_json::json!({}),
            is_local: false,
            created_at: now_iso(),
            updated_at: now_iso(),
        }
    }

    #[test]
    fn test_library_progress_preserved_when_clearing_downloads() {
        let db = Database::open_in_memory().unwrap();
        let novel = db.upsert_novel(&sample_novel("gut:1342")).unwrap();

        let chapters = vec![
            Chapter {
                id: "gut:1342:1".into(),
                novel_id: novel.id.clone(),
                external_id: "1".into(),
                chapter_number: 1.0,
                volume: None,
                title: "Chapter 1".into(),
                url: None,
                word_count: 1200,
                published_at: None,
                is_downloaded: false,
                local_path: None,
                size_bytes: 0,
            },
            Chapter {
                id: "gut:1342:2".into(),
                novel_id: novel.id.clone(),
                external_id: "2".into(),
                chapter_number: 2.0,
                volume: None,
                title: "Chapter 2".into(),
                url: None,
                word_count: 1500,
                published_at: None,
                is_downloaded: false,
                local_path: None,
                size_bytes: 0,
            },
        ];
        db.upsert_chapters(&novel.id, &chapters).unwrap();
        db.upsert_library_entry(&novel.id, LibraryStatus::Reading, Some(true), Some(true))
            .unwrap();
        db.mark_chapter_downloaded("gut:1342:1", "/tmp/ch1.json", 4096, 1200, None)
            .unwrap();
        db.save_reading_progress(&novel.id, "gut:1342:1", 0.65, 14, 2, 5)
            .unwrap();
        db.add_bookmark(&novel.id, Some("gut:1342:1"), Some(14), Some("It is a truth..."), Some("Great opening"), Some("#F59E0B"), false)
            .unwrap();

        // Clear all downloaded files for the novel
        let removed_paths = db.clear_novel_downloads(&novel.id).unwrap();
        assert_eq!(removed_paths, vec!["/tmp/ch1.json".to_string()]);

        // Verify chapter is marked not downloaded, yet reading_progress + bookmarks + library remain intact!
        let ch1 = db.get_chapter("gut:1342:1").unwrap().unwrap();
        assert!(!ch1.is_downloaded);
        assert!(ch1.local_path.is_none());
        assert_eq!(ch1.size_bytes, 0);

        let progress = db.get_reading_progress(&novel.id).unwrap().unwrap();
        assert_eq!(progress.chapter_id, "gut:1342:1");
        assert!((progress.scroll_offset - 0.65).abs() < 1e-6);
        assert_eq!(progress.paragraph_index, 14);

        let bms = db.list_bookmarks(Some(&novel.id)).unwrap();
        assert_eq!(bms.len(), 1);

        let entry = db.get_library_entry(&novel.id).unwrap().unwrap();
        assert_eq!(entry.status, LibraryStatus::Reading);
        assert!(entry.is_bookmarked);
    }

    #[test]
    fn test_export_import_roundtrip() {
        let db1 = Database::open_in_memory().unwrap();
        let novel = db1.upsert_novel(&sample_novel("gut:99")).unwrap();
        db1.upsert_chapters(
            &novel.id,
            &[Chapter {
                id: "gut:99:1".into(),
                novel_id: novel.id.clone(),
                external_id: "1".into(),
                chapter_number: 1.0,
                volume: None,
                title: "Chapter 1".into(),
                url: None,
                word_count: 500,
                published_at: None,
                is_downloaded: false,
                local_path: None,
                size_bytes: 0,
            }],
        )
        .unwrap();
        db1.upsert_library_entry(&novel.id, LibraryStatus::Reading, Some(true), Some(false))
            .unwrap();
        let col = db1.create_collection("SciFi Gems", None, Some("#38BDF8")).unwrap();
        db1.set_novel_in_collection(&col.id, &novel.id, true).unwrap();
        db1.save_reading_progress(&novel.id, "gut:99:1", 0.4, 8, 1, 3)
            .unwrap();
        db1.add_bookmark(&novel.id, Some("gut:99:1"), Some(2), Some("Opening"), None, Some("#F59E0B"), false)
            .unwrap();
        db1.add_highlight(&novel.id, "gut:99:1", 2, 0, 7, "Opening", Some("#6EE7B7"), None)
            .unwrap();

        let bundle = db1.export_bundle("/tmp/dl").unwrap();
        let db2 = Database::open_in_memory().unwrap();
        let (novels_n, entries_n) = db2.import_bundle(&bundle).unwrap();
        assert_eq!(novels_n, 1);
        assert_eq!(entries_n, 1);

        let items = db2.list_library_items().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].novel.title, "Pride and Prejudice");
        assert!(items[0].collection_ids.contains(&col.id));
        assert!((items[0].progress.as_ref().unwrap().scroll_offset - 0.4).abs() < 1e-6);
        assert_eq!(db2.list_bookmarks(Some(&novel.id)).unwrap()[0].color, "#F59E0B");
        assert_eq!(db2.list_highlights(&novel.id, Some("gut:99:1")).unwrap()[0].color, "#6EE7B7");
    }

    #[test]
    fn test_reading_history_coalescing_stats_and_streak() {
        let db = Database::open_in_memory().unwrap();
        let novel = db.upsert_novel(&sample_novel("gut:42")).unwrap();
        db.upsert_chapters(
            &novel.id,
            &[
                Chapter {
                    id: "gut:42:1".into(),
                    novel_id: novel.id.clone(),
                    external_id: "1".into(),
                    chapter_number: 1.0,
                    volume: None,
                    title: "Chapter 1".into(),
                    url: None,
                    word_count: 600,
                    published_at: None,
                    is_downloaded: false,
                    local_path: None,
                    size_bytes: 0,
                },
                Chapter {
                    id: "gut:42:2".into(),
                    novel_id: novel.id.clone(),
                    external_id: "2".into(),
                    chapter_number: 2.0,
                    volume: None,
                    title: "Chapter 2".into(),
                    url: None,
                    word_count: 900,
                    published_at: None,
                    is_downloaded: false,
                    local_path: None,
                    size_bytes: 0,
                },
            ],
        )
        .unwrap();
        db.upsert_library_entry(&novel.id, LibraryStatus::Finished, Some(false), Some(false))
            .unwrap();

        // Recording two rapid sessions on chapter 1 coalesces into one history row (sum duration, max words)
        db.record_reading_history(&novel.id, "gut:42:1", 40, 300).unwrap();
        db.record_reading_history(&novel.id, "gut:42:1", 80, 600).unwrap();
        db.record_reading_history(&novel.id, "gut:42:2", 60, 900).unwrap();

        let history = db.list_reading_history(10).unwrap();
        assert_eq!(history.len(), 2);
        let ch1_row = history.iter().find(|h| h.chapter_id == "gut:42:1").unwrap();
        assert_eq!(ch1_row.duration_seconds, 120);
        assert_eq!(ch1_row.words_read, 600);

        let stats = db.get_reading_stats().unwrap();
        assert_eq!(stats.total_reading_seconds, 180);
        assert_eq!(stats.total_words_read, 1500);
        assert_eq!(stats.chapters_read_count, 2);
        assert_eq!(stats.novels_finished_count, 1);
        assert_eq!(stats.current_streak_days, 1);
        assert_eq!(stats.daily_7d.len(), 7);
        assert_eq!(stats.daily_7d.last().unwrap().minutes, 3);
    }

    #[test]
    fn test_record_reading_history_cutoff_respects_rfc3339_t_separator() {
        let db = Database::open_in_memory().unwrap();
        let novel = db.upsert_novel(&sample_novel("gut:99")).unwrap();
        db.upsert_chapters(
            &novel.id,
            &[Chapter {
                id: "gut:99:1".into(),
                novel_id: novel.id.clone(),
                external_id: "1".into(),
                chapter_number: 1.0,
                volume: None,
                title: "Chapter 1".into(),
                url: None,
                word_count: 500,
                published_at: None,
                is_downloaded: false,
                local_path: None,
                size_bytes: 0,
            }],
        )
        .unwrap();

        // Insert a same-day session from 2 hours ago using RFC3339 ('T' separator).
        // Previously `datetime('now', '-15 minutes')` (space separator) compared < '...T...' and coalesced!
        let old_ts = (Utc::now() - Duration::hours(2)).to_rfc3339();
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO reading_history (id, novel_id, chapter_id, duration_seconds, words_read, read_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params!["old-hist-1", novel.id, "gut:99:1", 100, 400, old_ts],
            )?;
            Ok(())
        })
        .unwrap();

        db.record_reading_history(&novel.id, "gut:99:1", 45, 500).unwrap();
        let history = db.list_reading_history(10).unwrap();
        assert_eq!(
            history.len(),
            2,
            "2-hour-old RFC3339 session must NOT coalesce with a new reading session"
        );
    }

    #[test]
    fn test_import_bundle_sanitizes_missing_paths_and_orphan_chapters() {
        let db = Database::open_in_memory().unwrap();
        let mut novel = sample_novel("gut:555");
        novel.local_cover_path = Some("/nonexistent/machine/cover.jpg".into());

        let chapter = Chapter {
            id: "gut:555:1".into(),
            novel_id: novel.id.clone(),
            external_id: "1".into(),
            chapter_number: 1.0,
            volume: None,
            title: "Chapter 1".into(),
            url: None,
            word_count: 400,
            published_at: None,
            is_downloaded: true,
            local_path: Some("/nonexistent/machine/ch1.json".into()),
            size_bytes: 2048,
        };

        let bundle = LibraryExportBundle {
            exported_at: now_iso(),
            version: 1,
            novels: vec![novel.clone()],
            chapters: vec![chapter],
            library_entries: vec![LibraryEntry {
                novel_id: novel.id.clone(),
                status: LibraryStatus::Reading,
                is_bookmarked: true,
                follow_updates: false,
                last_checked_chapters: 1,
                added_at: now_iso(),
                updated_at: now_iso(),
            }],
            reading_progress: vec![ReadingProgress {
                novel_id: novel.id.clone(),
                chapter_id: "gut:555:missing_ch".into(),
                chapter_number: 99.0,
                chapter_title: None,
                scroll_offset: 0.5,
                paragraph_index: 3,
                page_index: 0,
                total_pages: 1,
                progress_percent: 50.0,
                updated_at: now_iso(),
            }],
            bookmarks: vec![Bookmark {
                id: "bm-orphan".into(),
                novel_id: novel.id.clone(),
                novel_title: None,
                author: None,
                cover_url: None,
                local_cover_path: None,
                chapter_id: Some("gut:555:missing_ch".into()),
                chapter_number: None,
                chapter_title: None,
                paragraph_index: None,
                excerpt: Some("Quote".into()),
                note: None,
                color: "#818CF8".into(),
                is_novel_bookmark: true,
                created_at: now_iso(),
            }],
            highlights: vec![],
            collections: vec![],
            collection_items: vec![],
            reading_history: vec![],
            settings: None,
        };

        let (novels_cnt, entries_cnt) = db.import_bundle(&bundle).unwrap();
        assert_eq!(novels_cnt, 1);
        assert_eq!(entries_cnt, 1);

        let stored_novel = db.get_novel(&novel.id).unwrap().unwrap();
        assert_eq!(stored_novel.local_cover_path, None);

        let stored_ch = db.get_chapter("gut:555:1").unwrap().unwrap();
        assert!(!stored_ch.is_downloaded);
        assert_eq!(stored_ch.local_path, None);
        assert_eq!(stored_ch.size_bytes, 0);

        let bms = db.list_bookmarks(Some(&novel.id)).unwrap();
        assert_eq!(bms.len(), 1);
        assert_eq!(bms[0].chapter_id, None);
    }
}
