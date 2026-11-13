use tauri::State;

use crate::error::AppResult;
use crate::models::{DownloadTask, StorageStats};
use crate::state::AppState;

#[tauri::command]
pub async fn enqueue_download(
    state: State<'_, AppState>,
    novel_id: String,
    chapter_ids: Vec<String>,
) -> AppResult<DownloadTask> {
    let task = state
        .downloads
        .enqueue_chapters(&novel_id, chapter_ids)
        .await?;
    state
        .log_event(format!(
            "Queued download of {} chapter(s) for '{}'",
            task.total_chapters, task.novel_title
        ))
        .await;
    Ok(task)
}

#[tauri::command]
pub async fn list_downloads(state: State<'_, AppState>) -> AppResult<Vec<DownloadTask>> {
    state.downloads.list_tasks().await
}

#[tauri::command]
pub async fn pause_download(state: State<'_, AppState>, task_id: String) -> AppResult<()> {
    state.downloads.pause_task(&task_id).await
}

#[tauri::command]
pub async fn resume_download(state: State<'_, AppState>, task_id: String) -> AppResult<()> {
    state.downloads.resume_task(&task_id).await
}

#[tauri::command]
pub async fn cancel_download(state: State<'_, AppState>, task_id: String) -> AppResult<()> {
    state.downloads.cancel_task(&task_id).await
}

#[tauri::command]
pub async fn retry_download(state: State<'_, AppState>, task_id: String) -> AppResult<DownloadTask> {
    state.downloads.retry_task(&task_id).await
}

#[tauri::command]
pub async fn delete_downloaded_chapters(
    state: State<'_, AppState>,
    chapter_ids: Vec<String>,
) -> AppResult<usize> {
    let count = state.downloads.delete_downloaded_chapters(&chapter_ids)?;
    state
        .log_event(format!("Deleted {count} downloaded chapter file(s) (progress kept)"))
        .await;
    Ok(count)
}

#[tauri::command]
pub async fn delete_downloaded_novel(
    state: State<'_, AppState>,
    novel_id: String,
) -> AppResult<usize> {
    let count = state.downloads.delete_downloaded_novel(&novel_id).await?;
    state
        .log_event(format!("Deleted offline files for '{novel_id}' ({count} files removed, progress kept)"))
        .await;
    Ok(count)
}

#[tauri::command]
pub async fn get_storage_stats(state: State<'_, AppState>) -> AppResult<StorageStats> {
    state.downloads.get_storage_stats()
}

#[tauri::command]
pub async fn pause_active_downloads(state: State<'_, AppState>) -> AppResult<usize> {
    Ok(state.downloads.pause_all_active().await)
}
