use std::collections::HashMap;
use serde_json::Value;
use tauri::State;

use crate::error::AppResult;
use crate::models::{SourceHealthResult, SourceInfo};
use crate::sources::DeclarativeSourceManifest;
use crate::state::AppState;

#[tauri::command]
pub async fn list_sources(state: State<'_, AppState>) -> AppResult<Vec<SourceInfo>> {
    state.sources.list_sources(&state.db).await
}

#[tauri::command]
pub async fn set_source_enabled(
    state: State<'_, AppState>,
    source_id: String,
    enabled: bool,
) -> AppResult<()> {
    state.db.set_source_enabled(&source_id, enabled)?;
    state
        .log_event(format!("Source '{source_id}' set enabled={enabled}"))
        .await;
    Ok(())
}

#[tauri::command]
pub async fn update_source_config(
    state: State<'_, AppState>,
    source_id: String,
    config: HashMap<String, Value>,
) -> AppResult<()> {
    state.db.update_source_config(&source_id, &config)?;
    state
        .log_event(format!("Updated configuration for source '{source_id}'"))
        .await;
    Ok(())
}

#[tauri::command]
pub async fn test_source_connection(
    state: State<'_, AppState>,
    source_id: String,
) -> AppResult<SourceHealthResult> {
    let res = state
        .sources
        .test_source_connection(&state.db, &source_id)
        .await?;
    state
        .log_event(format!(
            "Tested connection for '{}': ok={} ({})",
            source_id, res.ok, res.message
        ))
        .await;
    Ok(res)
}

#[tauri::command]
pub async fn install_custom_source(
    state: State<'_, AppState>,
    manifest: DeclarativeSourceManifest,
) -> AppResult<SourceInfo> {
    let info = state
        .sources
        .install_custom_source(&state.db, manifest)
        .await?;
    state
        .log_event(format!("Installed custom source '{}' ({})", info.name, info.id))
        .await;
    Ok(info)
}

#[tauri::command]
pub async fn remove_custom_source(
    state: State<'_, AppState>,
    source_id: String,
) -> AppResult<()> {
    state
        .sources
        .remove_custom_source(&state.db, &source_id)
        .await?;
    state
        .log_event(format!("Removed custom source '{source_id}'"))
        .await;
    Ok(())
}
