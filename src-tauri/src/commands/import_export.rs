use std::path::Path;
use chrono::Utc;
use serde::Serialize;
use tauri::State;
use uuid::Uuid;

use crate::downloads::{DownloadManager, StoredChapterPayload};
use crate::error::{AppError, AppResult};
use crate::formats::epub::parse_epub_bytes;
use crate::formats::text_html::{parse_html_bytes, parse_txt_bytes};
use crate::models::{Chapter, LibraryStatus, NovelRecord, NovelStatus};
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct ImportBookResult {
    pub novel: NovelRecord,
    pub chapters_imported: usize,
}

#[derive(Debug, Serialize)]
pub struct ImportBundleResult {
    pub novels_imported: usize,
    pub entries_imported: usize,
}

#[tauri::command]
pub async fn import_local_book(
    state: State<'_, AppState>,
    file_path: String,
) -> AppResult<ImportBookResult> {
    let path = Path::new(&file_path);
    if !path.exists() {
        return Err(AppError::NotFound(format!("File not found: {file_path}")));
    }

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();

    let fallback_title = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Imported Book")
        .to_string();

    let bytes = std::fs::read(path)?;
    let parsed = match ext.as_str() {
        "epub" => parse_epub_bytes(&bytes, &fallback_title)?,
        "txt" => parse_txt_bytes(&bytes, &fallback_title)?,
        "html" | "htm" | "xhtml" => parse_html_bytes(&bytes, &fallback_title)?,
        other => {
            return Err(AppError::InvalidInput(format!(
                "Unsupported book format '.{other}'. Supported formats: EPUB, TXT, HTML."
            )))
        }
    };

    let external_id = Uuid::new_v4().to_string();
    let novel_id = format!("local:{external_id}");
    let dl_dir = state.downloads.current_download_dir();

    // Persist extracted cover if present
    let local_cover_path = if let Some(ref cbytes) = parsed.cover_bytes {
        let cext = parsed.cover_ext.as_deref().unwrap_or("jpg");
        let cpath = DownloadManager::cover_file_path(&dl_dir, &novel_id, cext);
        if let Some(parent) = cpath.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&cpath, cbytes)?;
        Some(cpath.to_string_lossy().to_string())
    } else {
        None
    };

    let now = Utc::now().to_rfc3339();
    let novel_record = state.db.upsert_novel(&NovelRecord {
        id: novel_id.clone(),
        source_id: "local".into(),
        external_id,
        title: parsed.title,
        author_id: None,
        author: parsed.author,
        description: parsed.description,
        cover_url: None,
        local_cover_path,
        status: NovelStatus::Completed,
        genres: parsed.genres,
        tags: parsed.tags,
        rating: parsed.rating,
        popularity: 0,
        chapter_count: parsed.chapters.len() as i64,
        last_updated_remote: None,
        metadata: serde_json::json!({
            "source_file": file_path,
            "series": parsed.series,
            "series_index": parsed.series_index,
            "language": parsed.language,
        }),
        is_local: true,
        created_at: now.clone(),
        updated_at: now,
    })?;

    // Save all chapters locally on disk so imported books work 100% offline immediately
    let mut chapters = Vec::with_capacity(parsed.chapters.len());
    for (idx, pch) in parsed.chapters.into_iter().enumerate() {
        let ch_ext_id = format!("ch_{}", idx + 1);
        let ch_id = format!("{novel_id}::{ch_ext_id}");
        let payload = StoredChapterPayload {
            chapter_id: ch_id.clone(),
            novel_id: novel_id.clone(),
            chapter_number: pch.chapter_number,
            title: pch.title.clone(),
            html: pch.html,
            paragraphs: pch.paragraphs,
            word_count: pch.word_count,
        };
        let json = serde_json::to_vec(&payload)?;
        let size_bytes = json.len() as i64;
        let ch_path = DownloadManager::chapter_file_path(&dl_dir, &novel_id, &ch_id, pch.chapter_number);
        if let Some(parent) = ch_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&ch_path, &json)?;

        chapters.push(Chapter {
            id: ch_id,
            novel_id: novel_id.clone(),
            external_id: ch_ext_id,
            chapter_number: pch.chapter_number,
            volume: None,
            title: pch.title,
            url: None,
            word_count: pch.word_count,
            published_at: None,
            is_downloaded: true,
            local_path: Some(ch_path.to_string_lossy().to_string()),
            size_bytes,
        });
    }

    let chapters_imported = chapters.len();
    state.db.upsert_chapters(&novel_id, &chapters)?;
    state
        .db
        .upsert_library_entry(&novel_id, LibraryStatus::Reading, Some(false), Some(false))?;

    state
        .log_event(format!(
            "Imported '{}' ({} chapters) from {}",
            novel_record.title, chapters_imported, file_path
        ))
        .await;

    Ok(ImportBookResult {
        novel: novel_record,
        chapters_imported,
    })
}

#[tauri::command]
pub async fn export_library_json(
    state: State<'_, AppState>,
    dest_path: String,
) -> AppResult<u64> {
    let bundle = state
        .db
        .export_bundle(&state.default_download_dir.to_string_lossy())?;
    let json = serde_json::to_string_pretty(&bundle)?;
    let path = Path::new(&dest_path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, json.as_bytes())?;
    state
        .log_event(format!("Exported library metadata backup to {dest_path}"))
        .await;
    Ok(json.len() as u64)
}

#[tauri::command]
pub async fn import_library_json(
    state: State<'_, AppState>,
    file_path: String,
) -> AppResult<ImportBundleResult> {
    let raw = std::fs::read_to_string(&file_path)?;
    let bundle = serde_json::from_str(&raw)?;
    let (novels_imported, entries_imported) = state.db.import_bundle(&bundle)?;
    state
        .log_event(format!(
            "Restored {novels_imported} novel(s) and {entries_imported} library entries from {file_path}"
        ))
        .await;
    Ok(ImportBundleResult {
        novels_imported,
        entries_imported,
    })
}

#[tauri::command]
pub async fn backup_database_file(
    state: State<'_, AppState>,
    dest_path: String,
) -> AppResult<u64> {
    let bytes = state.db.backup_to(&dest_path)?;
    state
        .log_event(format!("Created SQLite backup at {dest_path} ({bytes} bytes)"))
        .await;
    Ok(bytes)
}
