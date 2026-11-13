use tauri::State;

use crate::error::{AppError, AppResult};
use crate::models::{
    Bookmark, Collection, Highlight, HomeDashboardData, LibraryEntry, LibraryNovelItem,
    LibraryStatus, ReadingHistoryItem, ReadingProgress, ReadingStats, UpdateCheckResult,
};
use crate::state::AppState;

#[tauri::command]
pub async fn get_home_dashboard(state: State<'_, AppState>) -> AppResult<HomeDashboardData> {
    state.db.get_home_dashboard()
}

#[tauri::command]
pub async fn list_library(state: State<'_, AppState>) -> AppResult<Vec<LibraryNovelItem>> {
    state.db.list_library_items()
}

#[tauri::command]
pub async fn update_library_entry(
    state: State<'_, AppState>,
    novel_id: String,
    status: String,
    is_bookmarked: Option<bool>,
    follow_updates: Option<bool>,
) -> AppResult<LibraryEntry> {
    let parsed_status = LibraryStatus::from_str_opt(&status)
        .ok_or_else(|| AppError::InvalidInput(format!("Unknown library status '{status}'")))?;
    let entry = state
        .db
        .upsert_library_entry(&novel_id, parsed_status, is_bookmarked, follow_updates)?;
    Ok(entry)
}

#[tauri::command]
pub async fn remove_from_library(state: State<'_, AppState>, novel_id: String) -> AppResult<()> {
    state.db.remove_from_library(&novel_id)
}

#[tauri::command]
pub async fn save_reading_progress(
    state: State<'_, AppState>,
    novel_id: String,
    chapter_id: String,
    scroll_offset: f64,
    paragraph_index: i64,
    page_index: i64,
    total_pages: i64,
) -> AppResult<ReadingProgress> {
    state.db.save_reading_progress(
        &novel_id,
        &chapter_id,
        scroll_offset,
        paragraph_index,
        page_index,
        total_pages,
    )
}

#[tauri::command]
pub async fn get_reading_progress(
    state: State<'_, AppState>,
    novel_id: String,
) -> AppResult<Option<ReadingProgress>> {
    state.db.get_reading_progress(&novel_id)
}

#[tauri::command]
pub async fn record_reading_session(
    state: State<'_, AppState>,
    novel_id: String,
    chapter_id: String,
    duration_seconds: i64,
    words_read: i64,
) -> AppResult<()> {
    state
        .db
        .record_reading_history(&novel_id, &chapter_id, duration_seconds, words_read)
}

#[tauri::command]
pub async fn list_reading_history(
    state: State<'_, AppState>,
    limit: Option<usize>,
) -> AppResult<Vec<ReadingHistoryItem>> {
    state.db.list_reading_history(limit.unwrap_or(100))
}

#[tauri::command]
pub async fn clear_reading_history(state: State<'_, AppState>) -> AppResult<()> {
    state.db.clear_reading_history()
}

#[tauri::command]
pub async fn get_reading_stats(state: State<'_, AppState>) -> AppResult<ReadingStats> {
    state.db.get_reading_stats()
}

#[tauri::command]
pub async fn list_bookmarks(
    state: State<'_, AppState>,
    novel_id: Option<String>,
) -> AppResult<Vec<Bookmark>> {
    state.db.list_bookmarks(novel_id.as_deref())
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn add_bookmark(
    state: State<'_, AppState>,
    novel_id: String,
    chapter_id: Option<String>,
    paragraph_index: Option<i64>,
    excerpt: Option<String>,
    note: Option<String>,
    color: Option<String>,
    is_novel_bookmark: Option<bool>,
) -> AppResult<Bookmark> {
    state.db.add_bookmark(
        &novel_id,
        chapter_id.as_deref(),
        paragraph_index,
        excerpt.as_deref(),
        note.as_deref(),
        color.as_deref(),
        is_novel_bookmark.unwrap_or(false),
    )
}

#[tauri::command]
pub async fn delete_bookmark(state: State<'_, AppState>, bookmark_id: String) -> AppResult<()> {
    state.db.delete_bookmark(&bookmark_id)
}

#[tauri::command]
pub async fn list_highlights(
    state: State<'_, AppState>,
    novel_id: String,
    chapter_id: Option<String>,
) -> AppResult<Vec<Highlight>> {
    state.db.list_highlights(&novel_id, chapter_id.as_deref())
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn add_highlight(
    state: State<'_, AppState>,
    novel_id: String,
    chapter_id: String,
    paragraph_index: i64,
    start_offset: i64,
    end_offset: i64,
    text: String,
    color: Option<String>,
    note: Option<String>,
) -> AppResult<Highlight> {
    state.db.add_highlight(
        &novel_id,
        &chapter_id,
        paragraph_index,
        start_offset,
        end_offset,
        &text,
        color.as_deref(),
        note.as_deref(),
    )
}

#[tauri::command]
pub async fn delete_highlight(state: State<'_, AppState>, highlight_id: String) -> AppResult<()> {
    state.db.delete_highlight(&highlight_id)
}

#[tauri::command]
pub async fn list_collections(state: State<'_, AppState>) -> AppResult<Vec<Collection>> {
    state.db.list_collections()
}

#[tauri::command]
pub async fn create_collection(
    state: State<'_, AppState>,
    name: String,
    description: Option<String>,
    color: Option<String>,
) -> AppResult<Collection> {
    state
        .db
        .create_collection(&name, description.as_deref(), color.as_deref())
}

#[tauri::command]
pub async fn delete_collection(state: State<'_, AppState>, collection_id: String) -> AppResult<()> {
    state.db.delete_collection(&collection_id)
}

#[tauri::command]
pub async fn set_novel_in_collection(
    state: State<'_, AppState>,
    collection_id: String,
    novel_id: String,
    present: bool,
) -> AppResult<()> {
    state
        .db
        .set_novel_in_collection(&collection_id, &novel_id, present)
}

#[tauri::command]
pub async fn check_followed_updates(
    state: State<'_, AppState>,
) -> AppResult<Vec<UpdateCheckResult>> {
    let followed = state.db.get_followed_novels()?;
    let settings = state
        .db
        .load_settings(&state.default_download_dir.to_string_lossy())?;
    let mut results = Vec::new();

    for (novel, entry) in followed {
        let Some(src) = state.sources.get_source(&novel.source_id).await else {
            continue;
        };
        let cfg = state
            .sources
            .get_source_info(&state.db, &novel.source_id)
            .await?
            .map(|i| i.config)
            .unwrap_or_default();

        if let Ok(remote_chapters) = src.get_chapters(&novel.external_id, &cfg).await {
            let prev_chs = state.db.get_chapters(&novel.id)?;
            let prev_ids: std::collections::HashSet<String> =
                prev_chs.iter().map(|c| c.id.clone()).collect();
            let prev_count = prev_chs.len() as i64;
            let new_ids: Vec<String> = remote_chapters
                .iter()
                .filter(|c| !prev_ids.contains(&c.id))
                .map(|c| c.id.clone())
                .collect();

            if !remote_chapters.is_empty() {
                state.db.upsert_chapters(&novel.id, &remote_chapters)?;
            }
            let curr_count = remote_chapters.len() as i64;
            let new_count = (curr_count - entry.last_checked_chapters.max(prev_count)).max(0);
            state.db.update_last_checked_chapters(&novel.id, curr_count)?;

            let mut auto_dl = false;
            if !new_ids.is_empty()
                && settings.auto_download_followed
                && state.downloads.enqueue_chapters(&novel.id, new_ids).await.is_ok()
            {
                auto_dl = true;
            }

            results.push(UpdateCheckResult {
                novel_id: novel.id,
                novel_title: novel.title,
                previous_chapters: prev_count,
                current_chapters: curr_count,
                new_chapters: new_count,
                auto_download_queued: auto_dl,
            });
        }
    }

    state
        .log_event(format!("Checked updates for {} followed novel(s)", results.len()))
        .await;
    Ok(results)
}
