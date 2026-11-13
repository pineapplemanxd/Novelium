pub mod gdrive;

use std::collections::HashMap;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::{watch, Mutex};
use url::Url;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::db::Database;
use crate::downloads::DownloadManager;
use crate::error::{AppError, AppResult};
use crate::models::{Chapter, GoogleDriveSyncConfig, GoogleDriveSyncStatus, NovelRecord};

const MAX_ARCHIVE_ENTRY_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookArchiveChapterEntry {
    pub chapter_id: String,
    pub filename: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookArchiveManifest {
    pub version: u32,
    pub novel_id: String,
    pub cover_filename: Option<String>,
    pub cover_sha256: Option<String>,
    pub chapters: Vec<BookArchiveChapterEntry>,
}

#[derive(Debug, Clone, Default)]
pub struct UnpackBookResult {
    pub chapters_restored: usize,
    pub cover_restored: bool,
}

pub struct PendingOAuthSession {
    pub state: String,
    pub code_verifier: String,
    pub redirect_uri: String,
    pub client_id: String,
    pub client_secret: String,
    pub cancel_tx: watch::Sender<bool>,
}

#[derive(Clone)]
pub struct GoogleDriveSyncCoordinator {
    pub http: reqwest::Client,
    pub pending_oauth: Arc<Mutex<Option<PendingOAuthSession>>>,
    pub sync_lock: Arc<Mutex<()>>,
}

impl GoogleDriveSyncCoordinator {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .user_agent("NoveliumSync/0.1.0")
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            http,
            pending_oauth: Arc::new(Mutex::new(None)),
            sync_lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn redact_status(cfg: &GoogleDriveSyncConfig) -> GoogleDriveSyncStatus {
        let connected = cfg
            .refresh_token
            .as_deref()
            .map(|t| !t.trim().is_empty())
            .unwrap_or(false)
            || cfg
                .access_token
                .as_deref()
                .map(|t| !t.trim().is_empty())
                .unwrap_or(false);
        GoogleDriveSyncStatus {
            enabled: cfg.enabled,
            client_id: cfg.client_id.clone(),
            has_client_secret: !cfg.client_secret.trim().is_empty(),
            folder_input: cfg.folder_input.clone(),
            folder_id: cfg.folder_id.clone(),
            folder_name: cfg.folder_name.clone(),
            sync_downloaded_books: cfg.sync_downloaded_books,
            sync_settings: cfg.sync_settings,
            auto_sync_on_launch: cfg.auto_sync_on_launch,
            connected,
            connected_email: cfg.connected_email.clone(),
            token_expires_at: cfg.token_expires_at.clone(),
            last_sync_at: cfg.last_sync_at.clone(),
            last_sync_summary: cfg.last_sync_summary.clone(),
        }
    }
}

impl Default for GoogleDriveSyncCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

pub fn is_valid_drive_id(raw: &str) -> bool {
    let trimmed = raw.trim();
    (10..=128).contains(&trimmed.len())
        && trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Extracts a Google Drive folder ID from a full Drive URL (such as
/// `https://drive.google.com/drive/folders/<ID>?hl=pl`) or validates a bare ID.
pub fn parse_google_drive_folder_id(input: &str) -> AppResult<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(AppError::InvalidInput(
            "Google Drive folder URL or folder ID cannot be empty.".into(),
        ));
    }

    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        let parsed = Url::parse(trimmed).map_err(|e| {
            AppError::InvalidInput(format!("Invalid Google Drive URL '{trimmed}': {e}"))
        })?;
        let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
        if !host.ends_with("drive.google.com") && !host.ends_with("docs.google.com") {
            return Err(AppError::InvalidInput(
                "Please enter a valid drive.google.com folder link or folder ID.".into(),
            ));
        }

        if let Some(mut segs) = parsed.path_segments() {
            while let Some(seg) = segs.next() {
                if seg == "folders" {
                    if let Some(candidate) = segs.next() {
                        if is_valid_drive_id(candidate) {
                            return Ok(candidate.to_string());
                        }
                    }
                }
            }
        }

        for (k, v) in parsed.query_pairs() {
            if k == "id" && is_valid_drive_id(&v) {
                return Ok(v.into_owned());
            }
        }

        return Err(AppError::InvalidInput(
            "Could not find a folder ID in that Google Drive URL. Expected a URL like https://drive.google.com/drive/folders/<FOLDER_ID>."
                .into(),
        ));
    }

    if is_valid_drive_id(trimmed) {
        return Ok(trimmed.to_string());
    }

    Err(AppError::InvalidInput(
        "Invalid Google Drive folder ID. Paste a folder URL (https://drive.google.com/drive/folders/...) or valid folder ID."
            .into(),
    ))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

fn sanitize_archive_basename(raw: &str) -> AppResult<String> {
    if raw.contains("..") || raw.contains('/') || raw.contains('\\') || raw.contains('\0') {
        return Err(AppError::InvalidInput(format!(
            "Invalid archive filename '{raw}'"
        )));
    }
    let name = Path::new(raw)
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty() && *s != "." && *s != "..")
        .ok_or_else(|| AppError::InvalidInput(format!("Empty archive filename '{raw}'")))?;
    Ok(name.to_string())
}

pub fn novel_archive_filename(novel_id: &str) -> String {
    format!("{}.novelium.zip", DownloadManager::safe_id(novel_id))
}

struct PreparedChapterFile {
    chapter_id: String,
    filename: String,
    path: PathBuf,
    size_bytes: u64,
    sha256: String,
}

struct PreparedCoverFile {
    filename: String,
    path: PathBuf,
    sha256: String,
}

struct PreparedNovelFiles {
    cover: Option<PreparedCoverFile>,
    chapters: Vec<PreparedChapterFile>,
    digest: String,
}

fn collect_novel_archive_inputs(
    novel: &NovelRecord,
    chapters: &[Chapter],
    download_dir: &Path,
) -> Option<PreparedNovelFiles> {
    let cover = novel.local_cover_path.as_deref().and_then(|raw| {
        let resolved = DownloadManager::relocate_library_path(raw, download_dir);
        let bytes = std::fs::read(&resolved).ok()?;
        let ext = resolved
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .filter(|e| matches!(e.as_str(), "jpg" | "jpeg" | "png" | "webp" | "gif"))
            .unwrap_or_else(|| "jpg".to_string());
        Some(PreparedCoverFile {
            filename: format!("{}.{}", DownloadManager::safe_id(&novel.id), ext),
            path: resolved,
            sha256: sha256_hex(&bytes),
        })
    });

    let mut prep_chapters = Vec::new();
    for ch in chapters {
        if !ch.is_downloaded {
            continue;
        }
        let Some(ref raw_lp) = ch.local_path else {
            continue;
        };
        let resolved = DownloadManager::relocate_library_path(raw_lp, download_dir);
        if !resolved.exists() {
            continue;
        }
        let Ok(bytes) = std::fs::read(&resolved) else {
            continue;
        };
        let filename = resolved
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                DownloadManager::chapter_file_path(
                    download_dir,
                    &novel.id,
                    &ch.id,
                    ch.chapter_number,
                )
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("chapter.json")
                .to_string()
            });
        prep_chapters.push(PreparedChapterFile {
            chapter_id: ch.id.clone(),
            filename,
            path: resolved,
            size_bytes: bytes.len() as u64,
            sha256: sha256_hex(&bytes),
        });
    }

    if prep_chapters.is_empty() && cover.is_none() {
        return None;
    }

    prep_chapters.sort_by(|a, b| a.filename.cmp(&b.filename).then(a.chapter_id.cmp(&b.chapter_id)));

    let mut hasher = Sha256::new();
    hasher.update(novel.id.as_bytes());
    hasher.update(b"\n");
    if let Some(ref c) = cover {
        hasher.update(b"cover:");
        hasher.update(c.filename.as_bytes());
        hasher.update(b":");
        hasher.update(c.sha256.as_bytes());
        hasher.update(b"\n");
    }
    for ch in &prep_chapters {
        hasher.update(b"ch:");
        hasher.update(ch.chapter_id.as_bytes());
        hasher.update(b":");
        hasher.update(ch.filename.as_bytes());
        hasher.update(b":");
        hasher.update(ch.sha256.as_bytes());
        hasher.update(b"\n");
    }
    let raw_digest = hasher.finalize();
    let mut digest = String::with_capacity(raw_digest.len() * 2);
    for b in raw_digest {
        digest.push_str(&format!("{b:02x}"));
    }

    Some(PreparedNovelFiles {
        cover,
        chapters: prep_chapters,
        digest,
    })
}

pub fn compute_novel_archive_digest(
    novel: &NovelRecord,
    chapters: &[Chapter],
    download_dir: &Path,
) -> Option<String> {
    collect_novel_archive_inputs(novel, chapters, download_dir).map(|p| p.digest)
}

/// Packages a novel's cover and all downloaded chapters into a zip archive with
/// a deterministic content digest.
pub fn pack_novel_archive(
    novel: &NovelRecord,
    chapters: &[Chapter],
    download_dir: &Path,
) -> AppResult<Option<(Vec<u8>, String, usize)>> {
    let Some(prepared) = collect_novel_archive_inputs(novel, chapters, download_dir) else {
        return Ok(None);
    };

    let manifest = BookArchiveManifest {
        version: 1,
        novel_id: novel.id.clone(),
        cover_filename: prepared.cover.as_ref().map(|c| c.filename.clone()),
        cover_sha256: prepared.cover.as_ref().map(|c| c.sha256.clone()),
        chapters: prepared
            .chapters
            .iter()
            .map(|c| BookArchiveChapterEntry {
                chapter_id: c.chapter_id.clone(),
                filename: c.filename.clone(),
                size_bytes: c.size_bytes,
                sha256: c.sha256.clone(),
            })
            .collect(),
    };

    let mut buf = Cursor::new(Vec::new());
    {
        let mut zip = ZipWriter::new(&mut buf);
        let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

        let manifest_json = serde_json::to_vec_pretty(&manifest)?;
        zip.start_file("manifest.json", opts)
            .map_err(|e| AppError::InvalidInput(format!("Failed creating zip manifest: {e}")))?;
        zip.write_all(&manifest_json)?;

        if let Some(ref c) = prepared.cover {
            let bytes = std::fs::read(&c.path)?;
            zip.start_file(format!("cover/{}", c.filename), opts)
                .map_err(|e| AppError::InvalidInput(format!("Failed packing cover: {e}")))?;
            zip.write_all(&bytes)?;
        }

        for ch in &prepared.chapters {
            let bytes = std::fs::read(&ch.path)?;
            zip.start_file(format!("chapters/{}", ch.filename), opts)
                .map_err(|e| AppError::InvalidInput(format!("Failed packing chapter: {e}")))?;
            zip.write_all(&bytes)?;
        }

        zip.finish()
            .map_err(|e| AppError::InvalidInput(format!("Failed finalizing book archive: {e}")))?;
    }

    let chapter_count = prepared.chapters.len();
    Ok(Some((buf.into_inner(), prepared.digest, chapter_count)))
}

fn read_zip_entry_capped(archive: &mut ZipArchive<Cursor<&[u8]>>, name: &str) -> AppResult<Vec<u8>> {
    let entry = archive
        .by_name(name)
        .map_err(|e| AppError::InvalidInput(format!("Missing zip entry '{name}': {e}")))?;
    if entry.size() > MAX_ARCHIVE_ENTRY_BYTES {
        return Err(AppError::InvalidInput(format!(
            "Archive entry '{name}' exceeds size limit"
        )));
    }
    let mut out = Vec::new();
    entry.take(MAX_ARCHIVE_ENTRY_BYTES + 1).read_to_end(&mut out)?;
    if out.len() as u64 > MAX_ARCHIVE_ENTRY_BYTES {
        return Err(AppError::InvalidInput(format!(
            "Archive entry '{name}' exceeds uncompressed size limit"
        )));
    }
    Ok(out)
}

/// Restores missing offline chapters and cover from a `.novelium.zip` archive into
/// the target device's `download_dir` and updates SQLite paths.
pub fn unpack_novel_archive(
    zip_bytes: &[u8],
    db: &Database,
    download_dir: &Path,
) -> AppResult<UnpackBookResult> {
    let cursor = Cursor::new(zip_bytes);
    let mut archive = ZipArchive::new(cursor)
        .map_err(|e| AppError::InvalidInput(format!("Corrupted book archive: {e}")))?;

    let manifest_bytes = read_zip_entry_capped(&mut archive, "manifest.json")?;
    let manifest: BookArchiveManifest = serde_json::from_slice(&manifest_bytes)?;
    let novel_id = manifest.novel_id.clone();

    let Some(novel) = db.get_novel(&novel_id)? else {
        return Ok(UnpackBookResult::default());
    };

    let mut res = UnpackBookResult::default();
    let safe_novel_id = DownloadManager::safe_id(&novel_id);

    if let Some(ref raw_cover) = manifest.cover_filename {
        let cover_name = sanitize_archive_basename(raw_cover)?;
        let covers_dir = download_dir.join("covers");
        let dest = covers_dir.join(&cover_name);
        if !dest.exists() {
            let entry_path = format!("cover/{cover_name}");
            if let Ok(cover_bytes) = read_zip_entry_capped(&mut archive, &entry_path) {
                std::fs::create_dir_all(&covers_dir)?;
                std::fs::write(&dest, &cover_bytes)?;
                db.set_novel_local_cover(&novel_id, &dest.to_string_lossy())?;
                res.cover_restored = true;
            }
        } else if novel.local_cover_path.as_deref() != Some(dest.to_string_lossy().as_ref()) {
            db.set_novel_local_cover(&novel_id, &dest.to_string_lossy())?;
        }
    }

    let existing_chapters: HashMap<String, Chapter> = db
        .get_chapters(&novel_id)?
        .into_iter()
        .map(|c| (c.id.clone(), c))
        .collect();

    let chapters_dir = download_dir
        .join("novels")
        .join(&safe_novel_id)
        .join("chapters");
    if !manifest.chapters.is_empty() {
        std::fs::create_dir_all(&chapters_dir)?;
    }

    for entry in &manifest.chapters {
        let safe_file = sanitize_archive_basename(&entry.filename)?;
        let dest = chapters_dir.join(&safe_file);

        let already_present = existing_chapters
            .get(&entry.chapter_id)
            .map(|c| {
                c.is_downloaded
                    && c.local_path
                        .as_deref()
                        .map(|lp| DownloadManager::relocate_library_path(lp, download_dir).exists())
                        .unwrap_or(false)
            })
            .unwrap_or(false)
            && dest.exists();

        if already_present {
            continue;
        }

        let bytes = read_zip_entry_capped(&mut archive, &format!("chapters/{safe_file}"))?;
        let tmp_dest = chapters_dir.join(format!(".{safe_file}.tmp"));
        std::fs::write(&tmp_dest, &bytes)?;
        std::fs::rename(&tmp_dest, &dest)?;

        db.mark_chapter_downloaded(
            &entry.chapter_id,
            &dest.to_string_lossy(),
            bytes.len() as i64,
            0,
            Some(&entry.sha256),
        )?;
        res.chapters_restored += 1;
    }

    Ok(res)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::downloads::StoredChapterPayload;
    use crate::models::NovelStatus;
    use tempfile::TempDir;

    fn mk_novel(id: &str, cover_path: Option<String>) -> NovelRecord {
        NovelRecord {
            id: id.into(),
            source_id: "gutenberg".into(),
            external_id: "1342".into(),
            title: "Pride and Prejudice".into(),
            author_id: None,
            author: "Jane Austen".into(),
            description: "Classic".into(),
            cover_url: None,
            local_cover_path: cover_path,
            status: NovelStatus::Completed,
            genres: vec![],
            tags: vec![],
            rating: None,
            popularity: 0,
            chapter_count: 2,
            last_updated_remote: None,
            metadata: serde_json::json!({}),
            is_local: false,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    #[test]
    fn test_parse_google_drive_folder_id_variants() {
        assert_eq!(
            parse_google_drive_folder_id(
                "https://drive.google.com/drive/folders/1JoF3DeGpjMRY__lN-nY-OITpT6Dma2Yd?hl=pl"
            )
            .unwrap(),
            "1JoF3DeGpjMRY__lN-nY-OITpT6Dma2Yd"
        );
        assert_eq!(
            parse_google_drive_folder_id(
                "https://drive.google.com/drive/u/0/folders/1JoF3DeGpjMRY__lN-nY-OITpT6Dma2Yd"
            )
            .unwrap(),
            "1JoF3DeGpjMRY__lN-nY-OITpT6Dma2Yd"
        );
        assert_eq!(
            parse_google_drive_folder_id(
                "https://drive.google.com/open?id=1JoF3DeGpjMRY__lN-nY-OITpT6Dma2Yd"
            )
            .unwrap(),
            "1JoF3DeGpjMRY__lN-nY-OITpT6Dma2Yd"
        );
        assert_eq!(
            parse_google_drive_folder_id("1JoF3DeGpjMRY__lN-nY-OITpT6Dma2Yd").unwrap(),
            "1JoF3DeGpjMRY__lN-nY-OITpT6Dma2Yd"
        );

        assert!(parse_google_drive_folder_id("").is_err());
        assert!(parse_google_drive_folder_id("https://example.com/folders/123456789012").is_err());
        assert!(parse_google_drive_folder_id("https://drive.google.com/drive/my-drive").is_err());
    }

    #[test]
    fn test_pack_and_unpack_novel_archive_roundtrip_and_digest_match() {
        let dir_a = TempDir::new().unwrap();
        let dir_b = TempDir::new().unwrap();

        let db_a = Database::open_in_memory().unwrap();
        let cover_a = DownloadManager::cover_file_path(dir_a.path(), "gut:1342", "jpg");
        std::fs::create_dir_all(cover_a.parent().unwrap()).unwrap();
        std::fs::write(&cover_a, b"fake-jpeg-bytes").unwrap();

        let novel_a = mk_novel("gut:1342", Some(cover_a.to_string_lossy().into_owned()));
        db_a.upsert_novel(&novel_a).unwrap();

        let ch_path_a =
            DownloadManager::chapter_file_path(dir_a.path(), "gut:1342", "gut:1342:1", 1.0);
        std::fs::create_dir_all(ch_path_a.parent().unwrap()).unwrap();
        let payload = StoredChapterPayload {
            chapter_id: "gut:1342:1".into(),
            novel_id: "gut:1342".into(),
            chapter_number: 1.0,
            title: "Chapter 1".into(),
            html: "<p>It is a truth universally acknowledged.</p>".into(),
            paragraphs: vec!["It is a truth universally acknowledged.".into()],
            word_count: 6,
        };
        let payload_bytes = serde_json::to_vec(&payload).unwrap();
        std::fs::write(&ch_path_a, &payload_bytes).unwrap();

        let ch_a = Chapter {
            id: "gut:1342:1".into(),
            novel_id: "gut:1342".into(),
            external_id: "1".into(),
            chapter_number: 1.0,
            volume: None,
            title: "Chapter 1".into(),
            url: None,
            word_count: 6,
            published_at: None,
            is_downloaded: true,
            local_path: Some(ch_path_a.to_string_lossy().into_owned()),
            size_bytes: payload_bytes.len() as i64,
        };
        db_a.upsert_chapters("gut:1342", std::slice::from_ref(&ch_a)).unwrap();

        let (zip_bytes, digest_a, packed_n) =
            pack_novel_archive(&novel_a, &[ch_a], dir_a.path())
                .unwrap()
                .unwrap();
        assert_eq!(packed_n, 1);

        // Simulate syncing onto Device B with empty local files and a different download_dir
        let bundle = db_a.export_bundle(&dir_a.path().to_string_lossy()).unwrap();
        let db_b = Database::open_in_memory().unwrap();
        db_b.merge_bundle(&bundle, false).unwrap();

        let unpack_res = unpack_novel_archive(&zip_bytes, &db_b, dir_b.path()).unwrap();
        assert!(unpack_res.cover_restored);
        assert_eq!(unpack_res.chapters_restored, 1);

        let novel_b = db_b.get_novel("gut:1342").unwrap().unwrap();
        let chapters_b = db_b.get_chapters("gut:1342").unwrap();
        assert!(chapters_b[0].is_downloaded);
        let lp_b = PathBuf::from(chapters_b[0].local_path.as_ref().unwrap());
        assert!(lp_b.starts_with(dir_b.path()));
        assert!(lp_b.exists());
        assert!(Path::new(novel_b.local_cover_path.as_ref().unwrap()).starts_with(dir_b.path()));

        // Both devices now compute the exact same archive digest -> idempotent future syncs
        let digest_b = compute_novel_archive_digest(&novel_b, &chapters_b, dir_b.path()).unwrap();
        assert_eq!(digest_a, digest_b);
    }

    #[test]
    fn test_unpack_rejects_path_traversal() {
        let dir = TempDir::new().unwrap();
        let db = Database::open_in_memory().unwrap();
        db.upsert_novel(&mk_novel("gut:1", None)).unwrap();

        let manifest = BookArchiveManifest {
            version: 1,
            novel_id: "gut:1".into(),
            cover_filename: None,
            cover_sha256: None,
            chapters: vec![BookArchiveChapterEntry {
                chapter_id: "gut:1:1".into(),
                filename: "../../evil.json".into(),
                size_bytes: 2,
                sha256: "abc".into(),
            }],
        };

        let mut buf = Cursor::new(Vec::new());
        {
            let mut zip = ZipWriter::new(&mut buf);
            let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
            zip.start_file("manifest.json", opts).unwrap();
            zip.write_all(&serde_json::to_vec(&manifest).unwrap()).unwrap();
            zip.finish().unwrap();
        }

        assert!(unpack_novel_archive(&buf.into_inner(), &db, dir.path()).is_err());
    }
}
