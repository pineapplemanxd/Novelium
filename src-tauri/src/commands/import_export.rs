use std::io::{Read, Write};
use std::path::PathBuf;
use chrono::Utc;
use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_fs::{FilePath, FsExt, OpenOptions};
use url::Url;
use uuid::Uuid;

use crate::downloads::{DownloadManager, StoredChapterPayload};
use crate::error::{AppError, AppResult};
use crate::formats::epub::parse_epub_bytes;
use crate::formats::text_html::{
    parse_fb2_bytes, parse_html_bytes, parse_markdown_bytes, parse_pdf_bytes, parse_txt_bytes,
};
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

/// Parses a file path or `file://` URI returned by native file pickers (including iOS
/// `UIDocumentPickerViewController`) and returns `(FilePath, lowercase_ext, fallback_stem)`.
fn parse_picked_path(raw: &str) -> AppResult<(FilePath, String, String)> {
    if raw.starts_with("file://") {
        let parsed = Url::parse(raw)?;
        let decoded = parsed
            .to_file_path()
            .unwrap_or_else(|()| PathBuf::from(parsed.path()));
        let ext = decoded
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();
        let stem = decoded
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("Imported Book")
            .to_string();
        return Ok((FilePath::Url(parsed), ext, stem));
    }

    let path = PathBuf::from(raw);
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("Imported Book")
        .to_string();
    Ok((FilePath::Path(path), ext, stem))
}

fn read_picked_bytes(app: &AppHandle, fp: &FilePath) -> AppResult<Vec<u8>> {
    if let FilePath::Path(ref p) = fp {
        if !p.exists() {
            return Err(AppError::NotFound(format!("File not found: {}", p.display())));
        }
    }
    let mut opts = OpenOptions::new();
    opts.read(true);
    let mut file = app.fs().open(fp.clone(), opts).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            AppError::NotFound(format!("File not found: {e}"))
        } else {
            AppError::Io(e)
        }
    })?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    #[cfg(target_os = "ios")]
    {
        let _ = app.fs().stop_accessing_security_scoped_resource(fp.clone());
    }
    Ok(bytes)
}

fn write_picked_bytes(app: &AppHandle, dest_raw: &str, bytes: &[u8]) -> AppResult<()> {
    let (fp, _, _) = parse_picked_path(dest_raw)?;
    if let FilePath::Path(ref p) = fp {
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let mut opts = OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    let mut file = app.fs().open(fp.clone(), opts)?;
    file.write_all(bytes)?;
    file.flush()?;
    #[cfg(target_os = "ios")]
    {
        let _ = app.fs().stop_accessing_security_scoped_resource(fp);
    }
    Ok(())
}

#[tauri::command]
pub async fn import_local_book(
    app: AppHandle,
    state: State<'_, AppState>,
    file_path: String,
) -> AppResult<ImportBookResult> {
    let (fp, ext, fallback_title) = parse_picked_path(&file_path)?;
    let bytes = read_picked_bytes(&app, &fp)?;
    let effective_ext = if ext.is_empty() {
        if bytes.starts_with(b"%PDF") {
            "pdf".to_string()
        } else if bytes.starts_with(b"PK") {
            "epub".to_string()
        } else {
            ext
        }
    } else {
        ext
    };
    let parsed = match effective_ext.as_str() {
        "epub" => parse_epub_bytes(&bytes, &fallback_title)?,
        "pdf" => parse_pdf_bytes(&bytes, &fallback_title)?,
        "fb2" => parse_fb2_bytes(&bytes, &fallback_title)?,
        "md" | "markdown" => parse_markdown_bytes(&bytes, &fallback_title)?,
        "txt" | "text" => parse_txt_bytes(&bytes, &fallback_title)?,
        "html" | "htm" | "xhtml" => parse_html_bytes(&bytes, &fallback_title)?,
        other => {
            return Err(AppError::InvalidInput(format!(
                "Unsupported book format '.{other}'. Supported formats: EPUB, PDF, FB2, MD, TXT, HTML."
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
    app: AppHandle,
    state: State<'_, AppState>,
    dest_path: String,
) -> AppResult<u64> {
    let bundle = state
        .db
        .export_bundle(&state.default_download_dir.to_string_lossy())?;
    let json = serde_json::to_string_pretty(&bundle)?;
    write_picked_bytes(&app, &dest_path, json.as_bytes())?;
    state
        .log_event(format!("Exported library metadata backup to {dest_path}"))
        .await;
    Ok(json.len() as u64)
}

#[tauri::command]
pub async fn import_library_json(
    app: AppHandle,
    state: State<'_, AppState>,
    file_path: String,
) -> AppResult<ImportBundleResult> {
    let (fp, _, _) = parse_picked_path(&file_path)?;
    let bytes = read_picked_bytes(&app, &fp)?;
    let raw = String::from_utf8(bytes)
        .map_err(|e| AppError::InvalidInput(format!("Backup file is not valid UTF-8: {e}")))?;
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
    app: AppHandle,
    state: State<'_, AppState>,
    dest_path: String,
) -> AppResult<u64> {
    // Stage through a sandbox temp file so SQLite backup works identically on
    // desktop paths and iOS security-scoped file:// destinations.
    let tmp_backup = state
        .app_data_dir
        .join(format!("novelium_backup_{}.db", Uuid::new_v4()));
    let bytes = state.db.backup_to(&tmp_backup)?;
    let data = std::fs::read(&tmp_backup)?;
    let _ = std::fs::remove_file(&tmp_backup);
    write_picked_bytes(&app, &dest_path, &data)?;
    state
        .log_event(format!("Created SQLite backup at {dest_path} ({bytes} bytes)"))
        .await;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_picked_path_handles_plain_paths_and_ios_file_urls() {
        let (fp1, ext1, stem1) = parse_picked_path("/Users/test/Books/Dune.epub").unwrap();
        assert!(matches!(fp1, FilePath::Path(_)));
        assert_eq!(ext1, "epub");
        assert_eq!(stem1, "Dune");

        let (fp2, ext2, stem2) =
            parse_picked_path("file:///private/var/mobile/Library/Inbox/The%20Hobbit%20(1).EPUB")
                .unwrap();
        assert!(matches!(fp2, FilePath::Url(_)));
        assert_eq!(ext2, "epub");
        assert_eq!(stem2, "The Hobbit (1)");

        let (fp3, ext3, stem3) =
            parse_picked_path("file:///var/mobile/Containers/Shared/AppGroup/backup.json").unwrap();
        assert!(matches!(fp3, FilePath::Url(_)));
        assert_eq!(ext3, "json");
        assert_eq!(stem3, "backup");
    }
}
