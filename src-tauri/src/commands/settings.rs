use base64::Engine;
use serde::Serialize;
use tauri::State;

use crate::error::AppResult;
use crate::models::AppSettings;
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct DiagnosticsInfo {
    pub app_data_dir: String,
    pub db_path: String,
    pub custom_sources_dir: String,
    pub logs: Vec<String>,
}

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> AppResult<AppSettings> {
    state
        .db
        .load_settings(&state.default_download_dir.to_string_lossy())
}

#[tauri::command]
pub async fn save_settings(
    state: State<'_, AppState>,
    settings: AppSettings,
) -> AppResult<AppSettings> {
    if !settings.download_dir.trim().is_empty() {
        let _ = std::fs::create_dir_all(&settings.download_dir);
    }
    state.db.save_settings(&settings)?;
    state.log_event("Saved application settings").await;
    Ok(settings)
}

#[tauri::command]
pub async fn reset_settings(state: State<'_, AppState>) -> AppResult<AppSettings> {
    let defaults =
        AppSettings::default_with_dir(state.default_download_dir.to_string_lossy().to_string());
    state.db.save_settings(&defaults)?;
    state.log_event("Reset settings to defaults").await;
    Ok(defaults)
}

#[tauri::command]
pub async fn get_diagnostics(state: State<'_, AppState>) -> AppResult<DiagnosticsInfo> {
    let logs = state.logs.lock().await.clone();
    Ok(DiagnosticsInfo {
        app_data_dir: state.app_data_dir.to_string_lossy().to_string(),
        db_path: state.db.db_path().to_string_lossy().to_string(),
        custom_sources_dir: state
            .sources
            .custom_sources_dir()
            .to_string_lossy()
            .to_string(),
        logs,
    })
}

/// Fallback helper to load local cover files cleanly as data URLs if needed
#[tauri::command]
pub async fn read_local_image_data_url(
    state: State<'_, AppState>,
    path: String,
) -> AppResult<String> {
    let resolved = state.downloads.resolve_stored_path(&path);
    let bytes = std::fs::read(&resolved)?;
    let lower = resolved.to_string_lossy().to_ascii_lowercase();
    let mime = if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".webp") {
        "image/webp"
    } else {
        "image/jpeg"
    };
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    Ok(format!("data:{mime};base64,{encoded}"))
}
