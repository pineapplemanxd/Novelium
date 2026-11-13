use tauri::{AppHandle, State};

use crate::error::AppResult;
use crate::models::{
    GoogleDriveCloudBook, GoogleDriveOAuthStartResult, GoogleDriveSyncConfigPatch,
    GoogleDriveSyncReport, GoogleDriveSyncStatus,
};
use crate::state::AppState;
use crate::sync::{parse_google_drive_folder_id, GoogleDriveSyncCoordinator};

#[tauri::command]
pub async fn get_gdrive_sync_status(state: State<'_, AppState>) -> AppResult<GoogleDriveSyncStatus> {
    let cfg = state.db.load_gdrive_config()?;
    Ok(GoogleDriveSyncCoordinator::redact_status(&cfg))
}

#[tauri::command]
pub async fn save_gdrive_sync_config(
    state: State<'_, AppState>,
    patch: GoogleDriveSyncConfigPatch,
) -> AppResult<GoogleDriveSyncStatus> {
    let mut cfg = state.db.load_gdrive_config()?;
    cfg.enabled = patch.enabled;
    cfg.client_id = patch.client_id.trim().to_string();
    if let Some(secret_update) = patch.client_secret_update {
        cfg.client_secret = secret_update.trim().to_string();
    }

    let trimmed_folder = patch.folder_input.trim().to_string();
    if trimmed_folder.is_empty() {
        cfg.folder_input.clear();
        cfg.folder_id.clear();
        cfg.folder_name = None;
    } else {
        let parsed_id = parse_google_drive_folder_id(&trimmed_folder)?;
        if cfg.folder_id != parsed_id {
            cfg.folder_name = None;
        }
        cfg.folder_input = trimmed_folder;
        cfg.folder_id = parsed_id;
    }

    cfg.sync_downloaded_books = patch.sync_downloaded_books;
    cfg.sync_settings = patch.sync_settings;
    cfg.auto_sync_on_launch = patch.auto_sync_on_launch;
    state.db.save_gdrive_config(&cfg)?;
    state
        .log_event(format!(
            "Saved Google Drive sync settings (folder_id={})",
            if cfg.folder_id.is_empty() {
                "<unset>"
            } else {
                &cfg.folder_id
            }
        ))
        .await;
    Ok(GoogleDriveSyncCoordinator::redact_status(&cfg))
}

#[tauri::command]
pub async fn start_gdrive_oauth(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<GoogleDriveOAuthStartResult> {
    state
        .log_event("Started Google Drive OAuth authorization flow")
        .await;
    state
        .gdrive
        .start_oauth(state.db.clone(), Some(app))
        .await
}

#[tauri::command]
pub async fn complete_gdrive_oauth_manual(
    app: AppHandle,
    state: State<'_, AppState>,
    callback_input: String,
) -> AppResult<GoogleDriveSyncStatus> {
    let status = state
        .gdrive
        .complete_oauth_manual(&state.db, &callback_input, Some(&app))
        .await?;
    state
        .log_event(format!(
            "Connected Google Drive account {}",
            status.connected_email.as_deref().unwrap_or("(unknown)")
        ))
        .await;
    Ok(status)
}

#[tauri::command]
pub async fn cancel_gdrive_oauth(state: State<'_, AppState>) -> AppResult<()> {
    state.gdrive.cancel_oauth().await;
    Ok(())
}

#[tauri::command]
pub async fn disconnect_gdrive(state: State<'_, AppState>) -> AppResult<GoogleDriveSyncStatus> {
    let status = state.gdrive.disconnect(&state.db).await?;
    state.log_event("Disconnected Google Drive account").await;
    Ok(status)
}

#[tauri::command]
pub async fn verify_gdrive_folder(state: State<'_, AppState>) -> AppResult<GoogleDriveSyncStatus> {
    let status = state.gdrive.verify_configured_folder(&state.db).await?;
    state
        .log_event(format!(
            "Verified Google Drive sync folder '{}' ({})",
            status.folder_name.as_deref().unwrap_or("Folder"),
            status.folder_id
        ))
        .await;
    Ok(status)
}

#[tauri::command]
pub async fn sync_with_gdrive(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<GoogleDriveSyncReport> {
    let report = state
        .gdrive
        .run_sync(
            &state.db,
            &state.downloads,
            &state.default_download_dir,
            Some(&app),
        )
        .await?;
    state
        .log_event(format!(
            "Google Drive sync finished: merged {} books, downloaded {} books ({} chapters), uploaded {} books",
            report.novels_merged,
            report.books_downloaded,
            report.chapters_downloaded,
            report.books_uploaded
        ))
        .await;
    Ok(report)
}

#[tauri::command]
pub async fn list_gdrive_cloud_books(
    state: State<'_, AppState>,
) -> AppResult<Vec<GoogleDriveCloudBook>> {
    state
        .gdrive
        .list_cloud_books(&state.db, &state.downloads)
        .await
}

#[tauri::command]
pub async fn download_gdrive_book_to_device(
    app: AppHandle,
    state: State<'_, AppState>,
    file_id: String,
    novel_id: Option<String>,
) -> AppResult<GoogleDriveSyncReport> {
    let report = state
        .gdrive
        .download_cloud_book_to_device(
            &state.db,
            &state.downloads,
            &file_id,
            novel_id.as_deref(),
            Some(&app),
        )
        .await?;
    state
        .log_event(format!(
            "Downloaded cloud book from Google Drive ({} ch restored)",
            report.chapters_downloaded
        ))
        .await;
    Ok(report)
}
