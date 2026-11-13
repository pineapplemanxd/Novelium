use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tauri::Emitter;
use tokio::sync::{Mutex, Notify, Semaphore};
use uuid::Uuid;

use crate::db::Database;
use crate::error::{AppError, AppResult};
use crate::models::{DownloadStatus, DownloadTask, StorageStats};
use crate::net::SourceHttpClient;
use crate::sources::SourceRegistry;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredChapterPayload {
    pub chapter_id: String,
    pub novel_id: String,
    pub chapter_number: f64,
    pub title: String,
    pub html: String,
    pub paragraphs: Vec<String>,
    pub word_count: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ControlSignal {
    Run,
    Pause,
    Cancel,
}

struct ActiveTaskControl {
    signal: Arc<Mutex<ControlSignal>>,
    notify: Arc<Notify>,
    speed_bps: Arc<Mutex<f64>>,
}

#[derive(Clone)]
pub struct DownloadManager {
    db: Database,
    sources: SourceRegistry,
    http: SourceHttpClient,
    default_download_dir: PathBuf,
    controls: Arc<Mutex<HashMap<String, ActiveTaskControl>>>,
    app_handle: Arc<Mutex<Option<tauri::AppHandle>>>,
}

impl DownloadManager {
    pub fn new(
        db: Database,
        sources: SourceRegistry,
        http: SourceHttpClient,
        default_download_dir: PathBuf,
    ) -> Self {
        let _ = std::fs::create_dir_all(&default_download_dir);
        if let Ok(tasks) = db.list_download_tasks() {
            for mut t in tasks {
                if matches!(t.status, DownloadStatus::Queued | DownloadStatus::Downloading) {
                    t.status = DownloadStatus::Paused;
                    t.speed_bytes_per_sec = 0.0;
                    t.error_message = Some("Interrupted when application closed; resume to continue.".into());
                    t.updated_at = Utc::now().to_rfc3339();
                    let _ = db.update_download_task(&t);
                }
            }
        }
        Self {
            db,
            sources,
            http,
            default_download_dir,
            controls: Arc::new(Mutex::new(HashMap::new())),
            app_handle: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn set_app_handle(&self, handle: tauri::AppHandle) {
        let mut g = self.app_handle.lock().await;
        *g = Some(handle);
    }

    pub fn current_download_dir(&self) -> PathBuf {
        #[cfg(target_os = "ios")]
        {
            return self.default_download_dir.clone();
        }
        #[cfg(not(target_os = "ios"))]
        {
            self.db
                .load_settings(&self.default_download_dir.to_string_lossy())
                .map(|s| PathBuf::from(s.download_dir))
                .unwrap_or_else(|_| self.default_download_dir.clone())
        }
    }

    /// Resolves a stored offline chapter or cover path, relocating files inside
    /// `/novels/` or `/covers/` onto `download_dir` if an app container UUID changed (iOS updates)
    /// or metadata was imported from another device.
    pub fn relocate_library_path(stored: &str, download_dir: &Path) -> PathBuf {
        let direct = Path::new(stored);
        if direct.exists() {
            return direct.to_path_buf();
        }
        let normalized = stored.replace('\\', "/");
        for marker in ["/novels/", "/covers/"] {
            if let Some(idx) = normalized.rfind(marker) {
                let rel = &normalized[idx + 1..];
                let candidate = download_dir.join(rel);
                if candidate.exists() {
                    return candidate;
                }
            }
        }
        direct.to_path_buf()
    }

    pub fn resolve_stored_path(&self, stored: &str) -> PathBuf {
        Self::relocate_library_path(stored, &self.current_download_dir())
    }

    pub fn safe_id(id: &str) -> String {
        id.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect()
    }

    pub fn chapter_file_path(download_dir: &Path, novel_id: &str, chapter_id: &str, chapter_num: f64) -> PathBuf {
        let n_dir = Self::safe_id(novel_id);
        let c_file = Self::safe_id(chapter_id);
        download_dir
            .join("novels")
            .join(n_dir)
            .join("chapters")
            .join(format!("{:05}_{}.json", chapter_num.max(0.0) as u64, c_file))
    }

    pub fn cover_file_path(download_dir: &Path, novel_id: &str, ext: &str) -> PathBuf {
        let n_dir = Self::safe_id(novel_id);
        download_dir
            .join("covers")
            .join(format!("{n_dir}.{ext}"))
    }

    async fn emit_task_update(&self, task: &DownloadTask) {
        let guard = self.app_handle.lock().await;
        if let Some(app) = guard.as_ref() {
            let _ = app.emit("download-updated", task);
        }
    }

    fn cover_ext(url: &str) -> &'static str {
        let low = url.to_ascii_lowercase();
        if low.contains(".png") {
            "png"
        } else if low.contains(".webp") {
            "webp"
        } else if low.contains(".gif") {
            "gif"
        } else {
            "jpg"
        }
    }

    fn hash_str_hex(input: &str) -> String {
        let mut h1: u64 = 0xcbf29ce484222325;
        let mut h2: u64 = 0x84222325cbf29ce4;
        for b in input.as_bytes() {
            h1 ^= *b as u64;
            h1 = h1.wrapping_mul(0x100000001b3);
            h2 = h2.wrapping_add(*b as u64).wrapping_mul(0x9e3779b97f4a7c15);
        }
        format!("{h1:016x}{h2:016x}")
    }

    async fn fetch_and_store_cover(&self, stem: &str, cover_url: &str) -> Option<String> {
        if cover_url.trim().is_empty() || cover_url.starts_with("data:") {
            return None;
        }
        let dl_dir = self.current_download_dir();
        let covers_dir = dl_dir.join("covers");
        let _ = std::fs::create_dir_all(&covers_dir);
        let path = covers_dir.join(format!("{stem}.{}", Self::cover_ext(cover_url)));
        if path.exists() {
            return Some(path.to_string_lossy().to_string());
        }
        if let Ok(bytes) = self.http.get_bytes(cover_url).await {
            if !bytes.is_empty() && std::fs::write(&path, &bytes).is_ok() {
                return Some(path.to_string_lossy().to_string());
            }
        }
        None
    }

    pub async fn cache_novel_cover(&self, novel_id: &str, cover_url: &str) -> Option<String> {
        let path_str = self
            .fetch_and_store_cover(&Self::safe_id(novel_id), cover_url)
            .await?;
        let _ = self.db.set_novel_local_cover(novel_id, &path_str);
        Some(path_str)
    }

    pub async fn cache_image_url(&self, cover_url: &str) -> Option<String> {
        let stem = format!("url_{}", Self::hash_str_hex(cover_url));
        self.fetch_and_store_cover(&stem, cover_url).await
    }

    fn cached_chapter_path(&self, novel_id: &str, chapter_id: &str) -> PathBuf {
        self.current_download_dir()
            .join("cache")
            .join("chapters")
            .join(Self::safe_id(novel_id))
            .join(format!("{}.json", Self::safe_id(chapter_id)))
    }

    pub fn read_cached_chapter(&self, novel_id: &str, chapter_id: &str) -> Option<StoredChapterPayload> {
        let path = self.cached_chapter_path(novel_id, chapter_id);
        let raw = std::fs::read_to_string(path).ok()?;
        serde_json::from_str::<StoredChapterPayload>(&raw).ok()
    }

    pub fn write_cached_chapter(&self, payload: &StoredChapterPayload) {
        let path = self.cached_chapter_path(&payload.novel_id, &payload.chapter_id);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string(payload) {
            let _ = std::fs::write(path, json);
        }
    }

    pub async fn enqueue_chapters(
        &self,
        novel_id: &str,
        chapter_ids: Vec<String>,
    ) -> AppResult<DownloadTask> {
        let novel = self
            .db
            .get_novel(novel_id)?
            .ok_or_else(|| AppError::NotFound(format!("Novel {novel_id} not found")))?;

        let all_chapters = self.db.get_chapters(novel_id)?;
        let target_ids: Vec<String> = if chapter_ids.is_empty() {
            all_chapters
                .iter()
                .filter(|c| !c.is_downloaded)
                .map(|c| c.id.clone())
                .collect()
        } else {
            let wanted: std::collections::HashSet<String> = chapter_ids.into_iter().collect();
            all_chapters
                .iter()
                .filter(|c| wanted.contains(&c.id) && !c.is_downloaded)
                .map(|c| c.id.clone())
                .collect()
        };

        if target_ids.is_empty() {
            return Err(AppError::InvalidInput(
                "All selected chapters are already downloaded".into(),
            ));
        }

        let now = Utc::now().to_rfc3339();
        let task = DownloadTask {
            id: Uuid::new_v4().to_string(),
            novel_id: novel.id.clone(),
            novel_title: novel.title.clone(),
            cover_url: novel.cover_url.clone(),
            local_cover_path: novel.local_cover_path.clone(),
            source_id: novel.source_id.clone(),
            total_chapters: target_ids.len() as i64,
            chapter_ids: target_ids,
            completed_chapters: 0,
            failed_chapters: 0,
            bytes_downloaded: 0,
            speed_bytes_per_sec: 0.0,
            status: DownloadStatus::Queued,
            error_message: None,
            created_at: now.clone(),
            updated_at: now,
        };

        let _ = self.db.ensure_novel_in_library(&novel.id);
        self.db.insert_download_task(&task)?;
        self.spawn_task_worker(task.clone()).await;
        Ok(task)
    }

    async fn spawn_task_worker(&self, mut task: DownloadTask) {
        let signal = Arc::new(Mutex::new(ControlSignal::Run));
        let notify = Arc::new(Notify::new());
        let speed_bps = Arc::new(Mutex::new(0.0));

        {
            let mut controls = self.controls.lock().await;
            controls.insert(
                task.id.clone(),
                ActiveTaskControl {
                    signal: signal.clone(),
                    notify: notify.clone(),
                    speed_bps: speed_bps.clone(),
                },
            );
        }

        let mgr = self.clone();
        tokio::spawn(async move {
            let settings = mgr
                .db
                .load_settings(&mgr.default_download_dir.to_string_lossy())
                .unwrap_or_else(|_| {
                    crate::models::AppSettings::default_with_dir(
                        mgr.default_download_dir.to_string_lossy().to_string(),
                    )
                });
            let max_conc = settings.max_concurrent_downloads.clamp(1, 8) as usize;
            let dl_dir = PathBuf::from(&settings.download_dir);

            let novel = match mgr.db.get_novel(&task.novel_id) {
                Ok(Some(n)) => n,
                _ => {
                    task.status = DownloadStatus::Failed;
                    task.error_message = Some("Novel record missing".into());
                    let _ = mgr.db.update_download_task(&task);
                    mgr.emit_task_update(&task).await;
                    mgr.controls.lock().await.remove(&task.id);
                    return;
                }
            };

            if novel.local_cover_path.is_none() {
                if let Some(ref curl) = novel.cover_url {
                    let _ = mgr.cache_novel_cover(&novel.id, curl).await;
                }
            }

            let source = match mgr.sources.get_source(&novel.source_id).await {
                Some(s) => s,
                None => {
                    task.status = DownloadStatus::Failed;
                    task.error_message = Some(format!("Source '{}' unavailable", novel.source_id));
                    let _ = mgr.db.update_download_task(&task);
                    mgr.emit_task_update(&task).await;
                    mgr.controls.lock().await.remove(&task.id);
                    return;
                }
            };
            let source_cfg = mgr
                .sources
                .get_source_info(&mgr.db, &novel.source_id)
                .await
                .ok()
                .flatten()
                .map(|i| i.config)
                .unwrap_or_default();

            task.status = DownloadStatus::Downloading;
            let _ = mgr.db.update_download_task(&task);
            mgr.emit_task_update(&task).await;

            let semaphore = Arc::new(Semaphore::new(max_conc));
            let started_at = Instant::now();
            let mut last_error: Option<String> = None;

            // Process in batches up to concurrency while checking pause/cancel cleanly
            let mut idx = 0usize;
            while idx < task.chapter_ids.len() {
                loop {
                    let sig = *signal.lock().await;
                    match sig {
                        ControlSignal::Cancel => {
                            task.status = DownloadStatus::Cancelled;
                            task.speed_bytes_per_sec = 0.0;
                            let _ = mgr.db.update_download_task(&task);
                            mgr.emit_task_update(&task).await;
                            mgr.controls.lock().await.remove(&task.id);
                            return;
                        }
                        ControlSignal::Pause => {
                            task.status = DownloadStatus::Paused;
                            task.speed_bytes_per_sec = 0.0;
                            let _ = mgr.db.update_download_task(&task);
                            mgr.emit_task_update(&task).await;
                            notify.notified().await;
                            let new_sig = *signal.lock().await;
                            if new_sig == ControlSignal::Run {
                                task.status = DownloadStatus::Downloading;
                                let _ = mgr.db.update_download_task(&task);
                                mgr.emit_task_update(&task).await;
                            }
                        }
                        ControlSignal::Run => break,
                    }
                }

                let batch_end = (idx + max_conc).min(task.chapter_ids.len());
                let batch = &task.chapter_ids[idx..batch_end];
                let mut handles = Vec::with_capacity(batch.len());

                for ch_id in batch {
                    let ch_id = ch_id.clone();
                    let permit = semaphore.clone().acquire_owned().await;
                    let db = mgr.db.clone();
                    let src = source.clone();
                    let cfg = source_cfg.clone();
                    let ext_novel_id = novel.external_id.clone();
                    let novel_id = novel.id.clone();
                    let dl_dir = dl_dir.clone();

                    handles.push(tokio::spawn(async move {
                        let _permit = permit;
                        let ch = db
                            .get_chapter(&ch_id)?
                            .ok_or_else(|| AppError::NotFound(format!("Chapter {ch_id} missing")))?;

                        if ch.is_downloaded {
                            if let Some(ref lp) = ch.local_path {
                                if Path::new(lp).exists() {
                                    return Ok::<i64, AppError>(0);
                                }
                            }
                        }

                        let (html, paragraphs, word_count) = src
                            .get_chapter_content(
                                &ext_novel_id,
                                &ch.external_id,
                                ch.url.as_deref(),
                                &cfg,
                            )
                            .await?;

                        let payload = StoredChapterPayload {
                            chapter_id: ch.id.clone(),
                            novel_id: novel_id.clone(),
                            chapter_number: ch.chapter_number,
                            title: ch.title.clone(),
                            html,
                            paragraphs,
                            word_count,
                        };
                        let json = serde_json::to_vec(&payload)?;
                        let size_bytes = json.len() as i64;
                        let path = Self::chapter_file_path(
                            &dl_dir,
                            &novel_id,
                            &ch.id,
                            ch.chapter_number,
                        );
                        if let Some(parent) = path.parent() {
                            std::fs::create_dir_all(parent)?;
                        }
                        std::fs::write(&path, &json)?;
                        let path_str = path.to_string_lossy().to_string();
                        db.mark_chapter_downloaded(&ch.id, &path_str, size_bytes, word_count, None)?;
                        Ok(size_bytes)
                    }));
                }

                for h in handles {
                    match h.await {
                        Ok(Ok(bytes)) => {
                            task.completed_chapters += 1;
                            task.bytes_downloaded += bytes;
                        }
                        Ok(Err(e)) => {
                            task.failed_chapters += 1;
                            last_error = Some(e.to_string());
                        }
                        Err(e) => {
                            task.failed_chapters += 1;
                            last_error = Some(e.to_string());
                        }
                    }
                }

                let elapsed = started_at.elapsed().as_secs_f64().max(0.1);
                let bps = (task.bytes_downloaded as f64) / elapsed;
                task.speed_bytes_per_sec = bps;
                *speed_bps.lock().await = bps;
                task.updated_at = Utc::now().to_rfc3339();
                let _ = mgr.db.update_download_task(&task);
                mgr.emit_task_update(&task).await;

                idx = batch_end;
            }

            task.speed_bytes_per_sec = 0.0;
            if task.failed_chapters > 0 && task.completed_chapters == 0 {
                task.status = DownloadStatus::Failed;
                task.error_message = last_error;
            } else if task.failed_chapters > 0 {
                task.status = DownloadStatus::Failed;
                task.error_message = Some(format!(
                    "{} of {} chapters failed{}",
                    task.failed_chapters,
                    task.total_chapters,
                    last_error
                        .map(|e| format!(": {e}"))
                        .unwrap_or_default()
                ));
            } else {
                task.status = DownloadStatus::Completed;
                task.error_message = None;
            }
            task.updated_at = Utc::now().to_rfc3339();
            let _ = mgr.db.update_download_task(&task);
            mgr.emit_task_update(&task).await;
            mgr.controls.lock().await.remove(&task.id);
        });
    }

    pub async fn pause_task(&self, task_id: &str) -> AppResult<()> {
        let controls = self.controls.lock().await;
        if let Some(ctrl) = controls.get(task_id) {
            *ctrl.signal.lock().await = ControlSignal::Pause;
            ctrl.notify.notify_waiters();
            return Ok(());
        }
        Err(AppError::NotFound(format!("Active download task {task_id} not found")))
    }

    /// Pauses all actively running/queued download tasks (e.g. before mobile app suspension).
    pub async fn pause_all_active(&self) -> usize {
        let active_ids: Vec<String> = {
            let controls = self.controls.lock().await;
            controls.keys().cloned().collect()
        };
        let mut paused = 0usize;
        for tid in active_ids {
            if self.pause_task(&tid).await.is_ok() {
                paused += 1;
            }
        }
        paused
    }

    pub async fn resume_task(&self, task_id: &str) -> AppResult<()> {
        {
            let controls = self.controls.lock().await;
            if let Some(ctrl) = controls.get(task_id) {
                *ctrl.signal.lock().await = ControlSignal::Run;
                ctrl.notify.notify_waiters();
                return Ok(());
            }
        }
        // If task wasn't actively spawned (e.g. app restarted while paused), respawn remaining chapters
        let tasks = self.db.list_download_tasks()?;
        if let Some(mut task) = tasks.into_iter().find(|t| t.id == task_id) {
            task.completed_chapters = 0;
            task.failed_chapters = 0;
            task.status = DownloadStatus::Queued;
            task.error_message = None;
            self.db.update_download_task(&task)?;
            self.spawn_task_worker(task).await;
            return Ok(());
        }
        Err(AppError::NotFound(format!("Download task {task_id} not found")))
    }

    pub async fn cancel_task(&self, task_id: &str) -> AppResult<()> {
        let controls = self.controls.lock().await;
        if let Some(ctrl) = controls.get(task_id) {
            *ctrl.signal.lock().await = ControlSignal::Cancel;
            ctrl.notify.notify_waiters();
            return Ok(());
        }
        drop(controls);

        let tasks = self.db.list_download_tasks()?;
        if let Some(mut task) = tasks.into_iter().find(|t| t.id == task_id) {
            task.status = DownloadStatus::Cancelled;
            self.db.update_download_task(&task)?;
            self.emit_task_update(&task).await;
            return Ok(());
        }
        Err(AppError::NotFound(format!("Download task {task_id} not found")))
    }

    pub async fn retry_task(&self, task_id: &str) -> AppResult<DownloadTask> {
        let tasks = self.db.list_download_tasks()?;
        let task = tasks
            .into_iter()
            .find(|t| t.id == task_id)
            .ok_or_else(|| AppError::NotFound(format!("Download task {task_id} not found")))?;

        let all_chs = self.db.get_chapters(&task.novel_id)?;
        let wanted: std::collections::HashSet<String> = task.chapter_ids.into_iter().collect();
        let remaining: Vec<String> = all_chs
            .into_iter()
            .filter(|c| wanted.contains(&c.id) && !c.is_downloaded)
            .map(|c| c.id)
            .collect();

        if remaining.is_empty() {
            return Err(AppError::InvalidInput("No failed/remaining chapters to retry".into()));
        }
        self.db.delete_download_task_record(task_id)?;
        self.enqueue_chapters(&task.novel_id, remaining).await
    }

    pub async fn list_tasks(&self) -> AppResult<Vec<DownloadTask>> {
        let mut tasks = self.db.list_download_tasks()?;
        let controls = self.controls.lock().await;
        for t in &mut tasks {
            if let Some(ctrl) = controls.get(&t.id) {
                t.speed_bytes_per_sec = *ctrl.speed_bps.lock().await;
            }
        }
        Ok(tasks)
    }

    pub fn delete_downloaded_chapters(&self, chapter_ids: &[String]) -> AppResult<usize> {
        let paths = self.db.clear_chapters_download_flags(chapter_ids)?;
        let mut removed = 0usize;
        for p in paths {
            let path = self.resolve_stored_path(&p);
            if path.exists() && std::fs::remove_file(path).is_ok() {
                removed += 1;
            }
        }
        Ok(removed)
    }

    pub async fn delete_downloaded_novel(&self, novel_id: &str) -> AppResult<usize> {
        let active_ids: Vec<String> = self
            .db
            .list_download_tasks()?
            .into_iter()
            .filter(|t| {
                t.novel_id == novel_id
                    && matches!(
                        t.status,
                        DownloadStatus::Queued | DownloadStatus::Downloading | DownloadStatus::Paused
                    )
            })
            .map(|t| t.id)
            .collect();
        for tid in &active_ids {
            let _ = self.cancel_task(tid).await;
        }
        for _ in 0..80 {
            let controls = self.controls.lock().await;
            if active_ids.iter().all(|tid| !controls.contains_key(tid)) {
                break;
            }
            drop(controls);
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }

        let paths = self.db.clear_novel_downloads(novel_id)?;
        let mut removed = 0usize;
        for p in paths {
            let path = self.resolve_stored_path(&p);
            if path.exists() && std::fs::remove_file(path).is_ok() {
                removed += 1;
            }
        }
        // Clean empty chapter directory if present
        let dl_dir = self.current_download_dir();
        let novel_dir = dl_dir.join("novels").join(Self::safe_id(novel_id));
        if novel_dir.exists() {
            let _ = std::fs::remove_dir_all(novel_dir);
        }
        Ok(removed)
    }

    pub fn get_storage_stats(&self) -> AppResult<StorageStats> {
        let dl_dir = self.current_download_dir();
        let novels = self.db.get_novel_storage_usages()?;
        let chapters_bytes: i64 = novels.iter().map(|n| n.size_bytes).sum();
        let covers_bytes = dir_size(&dl_dir.join("covers"));
        let database_bytes = std::fs::metadata(self.db.db_path())
            .map(|m| m.len() as i64)
            .unwrap_or(0);
        let total_bytes = chapters_bytes + covers_bytes + database_bytes;

        Ok(StorageStats {
            total_bytes,
            chapters_bytes,
            covers_bytes,
            database_bytes,
            download_dir: dl_dir.to_string_lossy().to_string(),
            novels,
        })
    }

    pub fn read_offline_chapter(&self, local_path: &str) -> AppResult<StoredChapterPayload> {
        let resolved = self.resolve_stored_path(local_path);
        let raw = std::fs::read_to_string(resolved)?;
        let payload = serde_json::from_str::<StoredChapterPayload>(&raw)?;
        Ok(payload)
    }
}

fn dir_size(path: &Path) -> i64 {
    if !path.exists() {
        return 0;
    }
    let mut total = 0i64;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                total += dir_size(&p);
            } else if let Ok(meta) = entry.metadata() {
                total += meta.len() as i64;
            }
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AppSettings, Chapter, LibraryStatus, NovelRecord, NovelStatus};
    use crate::sources::declarative::{DeclarativeKind, DeclarativeSourceManifest};

    #[tokio::test]
    async fn test_download_manager_downloads_and_deletes_keeping_progress() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base = format!("http://{addr}");

        let srv = tokio::spawn(async move {
            for _ in 0..8 {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let mut buf = [0u8; 2048];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let path = req
                        .lines()
                        .next()
                        .and_then(|l| l.split_whitespace().nth(1))
                        .unwrap_or("/");
                    let (status, body) = if path == "/robots.txt" {
                        ("404 Not Found", "".to_string())
                    } else if path == "/ch1" {
                        ("200 OK", "<div id='c'><p>Offline chapter one content.</p></div>".to_string())
                    } else if path == "/ch2" {
                        ("200 OK", "<div id='c'><p>Offline chapter two content.</p></div>".to_string())
                    } else {
                        ("404 Not Found", "Not found".to_string())
                    };
                    let resp = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = sock.write_all(resp.as_bytes()).await;
                });
            }
        });

        let tmp = tempfile::tempdir().unwrap();
        let db = Database::open_in_memory().unwrap();
        let http = SourceHttpClient::new().unwrap();
        let dl_dir = tmp.path().join("downloads");
        let sources_dir = tmp.path().join("sources");
        let cache_dir = tmp.path().join("cache");
        let registry = SourceRegistry::new(&db, http.clone(), sources_dir, cache_dir).unwrap();

        let manifest = DeclarativeSourceManifest {
            id: "test_dl_src".into(),
            name: "Test DL".into(),
            kind: DeclarativeKind::Html,
            base_url: base.clone(),
            description: "Test".into(),
            genres: vec![],
            feed_url: None,
            search_url_template: None,
            browse_url_template: None,
            novel_list_selector: None,
            title_selector: None,
            author_selector: None,
            cover_selector: None,
            link_selector: None,
            description_selector: None,
            chapter_list_selector: None,
            chapter_title_selector: None,
            chapter_link_selector: None,
            content_selector: Some("#c".into()),
            remove_selectors: vec![],
            language: None,
        };
        registry.install_custom_source(&db, manifest).await.unwrap();

        let mut settings = AppSettings::default_with_dir(dl_dir.to_string_lossy().to_string());
        settings.max_concurrent_downloads = 2;
        db.save_settings(&settings).unwrap();

        let novel = db
            .upsert_novel(&NovelRecord {
                id: "test_dl_src:book1".into(),
                source_id: "test_dl_src".into(),
                external_id: "book1".into(),
                title: "Offline Book".into(),
                author_id: None,
                author: "Test Author".into(),
                description: "Desc".into(),
                cover_url: None,
                local_cover_path: None,
                status: NovelStatus::Ongoing,
                genres: vec![],
                tags: vec![],
                rating: None,
                popularity: 0,
                chapter_count: 2,
                last_updated_remote: None,
                metadata: serde_json::json!({}),
                is_local: false,
                created_at: Utc::now().to_rfc3339(),
                updated_at: Utc::now().to_rfc3339(),
            })
            .unwrap();

        db.upsert_chapters(
            &novel.id,
            &[
                Chapter {
                    id: "test_dl_src:book1::1".into(),
                    novel_id: novel.id.clone(),
                    external_id: format!("{base}/ch1"),
                    chapter_number: 1.0,
                    volume: None,
                    title: "Ch 1".into(),
                    url: Some(format!("{base}/ch1")),
                    word_count: 0,
                    published_at: None,
                    is_downloaded: false,
                    local_path: None,
                    size_bytes: 0,
                },
                Chapter {
                    id: "test_dl_src:book1::2".into(),
                    novel_id: novel.id.clone(),
                    external_id: format!("{base}/ch2"),
                    chapter_number: 2.0,
                    volume: None,
                    title: "Ch 2".into(),
                    url: Some(format!("{base}/ch2")),
                    word_count: 0,
                    published_at: None,
                    is_downloaded: false,
                    local_path: None,
                    size_bytes: 0,
                },
            ],
        )
        .unwrap();

        db.upsert_library_entry(&novel.id, LibraryStatus::Reading, Some(true), Some(false))
            .unwrap();
        db.save_reading_progress(&novel.id, "test_dl_src:book1::1", 0.5, 3, 1, 2)
            .unwrap();

        let mgr = DownloadManager::new(db.clone(), registry, http, dl_dir.clone());
        let task = mgr.enqueue_chapters(&novel.id, vec![]).await.unwrap();

        // Wait up to 3s for background worker to finish
        for _ in 0..30 {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let list = mgr.list_tasks().await.unwrap();
            if list.iter().any(|t| t.id == task.id && t.status == DownloadStatus::Completed) {
                break;
            }
        }

        let chs = db.get_chapters(&novel.id).unwrap();
        assert!(chs[0].is_downloaded && chs[1].is_downloaded);
        let offline = mgr
            .read_offline_chapter(chs[0].local_path.as_deref().unwrap())
            .unwrap();
        assert_eq!(offline.paragraphs, vec!["Offline chapter one content."]);

        let stats = mgr.get_storage_stats().unwrap();
        assert!(stats.chapters_bytes > 0);
        assert_eq!(stats.novels.len(), 1);

        // Delete downloaded novel files and verify reading_progress stays intact!
        let removed = mgr.delete_downloaded_novel(&novel.id).await.unwrap();
        assert_eq!(removed, 2);
        let chs_after = db.get_chapters(&novel.id).unwrap();
        assert!(!chs_after[0].is_downloaded && !chs_after[1].is_downloaded);
        let rp = db.get_reading_progress(&novel.id).unwrap().unwrap();
        assert!((rp.scroll_offset - 0.5).abs() < 1e-6);

        srv.abort();
    }

    #[tokio::test]
    async fn test_download_pause_resume_cancel_and_single_chapter_delete() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base = format!("http://{addr}");

        let srv = tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let mut buf = [0u8; 2048];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let path = req
                        .lines()
                        .next()
                        .and_then(|l| l.split_whitespace().nth(1))
                        .unwrap_or("/");
                    if path != "/robots.txt" {
                        tokio::time::sleep(std::time::Duration::from_millis(80)).await;
                    }
                    let (status, body) = if path == "/robots.txt" {
                        ("404 Not Found", "".to_string())
                    } else if path == "/ch1" || path == "/ch2" || path == "/ch3" {
                        ("200 OK", format!("<div id='c'><p>Content for {path}</p></div>"))
                    } else {
                        ("404 Not Found", "Not found".to_string())
                    };
                    let resp = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = sock.write_all(resp.as_bytes()).await;
                });
            }
        });

        let tmp = tempfile::tempdir().unwrap();
        let db = Database::open_in_memory().unwrap();
        let http = SourceHttpClient::new().unwrap();
        let dl_dir = tmp.path().join("downloads");
        let sources_dir = tmp.path().join("sources");
        let cache_dir = tmp.path().join("cache");
        let registry = SourceRegistry::new(&db, http.clone(), sources_dir, cache_dir).unwrap();

        let manifest = DeclarativeSourceManifest {
            id: "test_ctrl_src".into(),
            name: "Test Control".into(),
            kind: DeclarativeKind::Html,
            base_url: base.clone(),
            description: "Test".into(),
            genres: vec![],
            feed_url: None,
            search_url_template: None,
            browse_url_template: None,
            novel_list_selector: None,
            title_selector: None,
            author_selector: None,
            cover_selector: None,
            link_selector: None,
            description_selector: None,
            chapter_list_selector: None,
            chapter_title_selector: None,
            chapter_link_selector: None,
            content_selector: Some("#c".into()),
            remove_selectors: vec![],
            language: None,
        };
        registry.install_custom_source(&db, manifest).await.unwrap();

        let mut settings = AppSettings::default_with_dir(dl_dir.to_string_lossy().to_string());
        settings.max_concurrent_downloads = 1;
        db.save_settings(&settings).unwrap();

        let novel = db
            .upsert_novel(&NovelRecord {
                id: "test_ctrl_src:book1".into(),
                source_id: "test_ctrl_src".into(),
                external_id: "book1".into(),
                title: "Pause Cancel Book".into(),
                author_id: None,
                author: "Tester".into(),
                description: "".into(),
                cover_url: None,
                local_cover_path: None,
                status: NovelStatus::Ongoing,
                genres: vec![],
                tags: vec![],
                rating: None,
                popularity: 0,
                chapter_count: 3,
                last_updated_remote: None,
                metadata: serde_json::json!({}),
                is_local: false,
                created_at: Utc::now().to_rfc3339(),
                updated_at: Utc::now().to_rfc3339(),
            })
            .unwrap();

        db.upsert_chapters(
            &novel.id,
            &[
                Chapter {
                    id: "test_ctrl_src:book1::1".into(),
                    novel_id: novel.id.clone(),
                    external_id: format!("{base}/ch1"),
                    chapter_number: 1.0,
                    volume: None,
                    title: "Ch 1".into(),
                    url: Some(format!("{base}/ch1")),
                    word_count: 0,
                    published_at: None,
                    is_downloaded: false,
                    local_path: None,
                    size_bytes: 0,
                },
                Chapter {
                    id: "test_ctrl_src:book1::2".into(),
                    novel_id: novel.id.clone(),
                    external_id: format!("{base}/ch2"),
                    chapter_number: 2.0,
                    volume: None,
                    title: "Ch 2".into(),
                    url: Some(format!("{base}/ch2")),
                    word_count: 0,
                    published_at: None,
                    is_downloaded: false,
                    local_path: None,
                    size_bytes: 0,
                },
                Chapter {
                    id: "test_ctrl_src:book1::3".into(),
                    novel_id: novel.id.clone(),
                    external_id: format!("{base}/ch3"),
                    chapter_number: 3.0,
                    volume: None,
                    title: "Ch 3".into(),
                    url: Some(format!("{base}/ch3")),
                    word_count: 0,
                    published_at: None,
                    is_downloaded: false,
                    local_path: None,
                    size_bytes: 0,
                },
            ],
        )
        .unwrap();

        let mgr = DownloadManager::new(db.clone(), registry, http, dl_dir.clone());
        let task = mgr
            .enqueue_chapters(
                &novel.id,
                vec![
                    "test_ctrl_src:book1::1".into(),
                    "test_ctrl_src:book1::2".into(),
                ],
            )
            .await
            .unwrap();

        // Pause immediately, verify task transitions to Paused, then resume to Completed
        mgr.pause_task(&task.id).await.unwrap();
        let mut saw_paused = false;
        for _ in 0..20 {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            let list = mgr.list_tasks().await.unwrap();
            if list.iter().any(|t| t.id == task.id && t.status == DownloadStatus::Paused) {
                saw_paused = true;
                break;
            }
        }
        assert!(saw_paused, "expected task to enter Paused status");

        mgr.resume_task(&task.id).await.unwrap();
        let mut completed = false;
        for _ in 0..40 {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            let list = mgr.list_tasks().await.unwrap();
            if list.iter().any(|t| t.id == task.id && t.status == DownloadStatus::Completed) {
                completed = true;
                break;
            }
        }
        assert!(completed, "expected resumed task to complete");

        // Enqueue chapter 3 and cancel it right away
        let cancel_task = mgr
            .enqueue_chapters(&novel.id, vec!["test_ctrl_src:book1::3".into()])
            .await
            .unwrap();
        mgr.cancel_task(&cancel_task.id).await.unwrap();
        let mut cancelled = false;
        for _ in 0..20 {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            let list = mgr.list_tasks().await.unwrap();
            if list.iter().any(|t| t.id == cancel_task.id && t.status == DownloadStatus::Cancelled) {
                cancelled = true;
                break;
            }
        }
        assert!(cancelled, "expected task to transition to Cancelled");

        // Delete only chapter 1, keeping chapter 2 downloaded
        let deleted = mgr
            .delete_downloaded_chapters(&["test_ctrl_src:book1::1".into()])
            .unwrap();
        assert_eq!(deleted, 1);

        let ch1 = db.get_chapter("test_ctrl_src:book1::1").unwrap().unwrap();
        let ch2 = db.get_chapter("test_ctrl_src:book1::2").unwrap().unwrap();
        assert!(!ch1.is_downloaded);
        assert!(ch2.is_downloaded);

        srv.abort();
    }

    #[tokio::test]
    async fn test_startup_reconciles_stale_downloading_tasks_to_paused() {
        let tmp = tempfile::tempdir().unwrap();
        let db = Database::open_in_memory().unwrap();
        let http = SourceHttpClient::new().unwrap();
        let dl_dir = tmp.path().join("downloads");
        let sources_dir = tmp.path().join("sources");
        let cache_dir = tmp.path().join("cache");
        let registry = SourceRegistry::new(&db, http.clone(), sources_dir, cache_dir).unwrap();

        let novel = db
            .upsert_novel(&NovelRecord {
                id: "gutenberg:1342".into(),
                source_id: "gutenberg".into(),
                external_id: "1342".into(),
                title: "Pride and Prejudice".into(),
                author_id: None,
                author: "Jane Austen".into(),
                description: "".into(),
                cover_url: None,
                local_cover_path: None,
                status: NovelStatus::Completed,
                genres: vec![],
                tags: vec![],
                rating: None,
                popularity: 0,
                chapter_count: 1,
                last_updated_remote: None,
                metadata: serde_json::json!({}),
                is_local: false,
                created_at: Utc::now().to_rfc3339(),
                updated_at: Utc::now().to_rfc3339(),
            })
            .unwrap();

        let stale_task = DownloadTask {
            id: "stale-task-1".into(),
            novel_id: novel.id.clone(),
            novel_title: novel.title.clone(),
            cover_url: None,
            local_cover_path: None,
            source_id: "gutenberg".into(),
            total_chapters: 2,
            chapter_ids: vec!["ch1".into(), "ch2".into()],
            completed_chapters: 1,
            failed_chapters: 0,
            bytes_downloaded: 1024,
            speed_bytes_per_sec: 512.0,
            status: DownloadStatus::Downloading,
            error_message: None,
            created_at: Utc::now().to_rfc3339(),
            updated_at: Utc::now().to_rfc3339(),
        };
        db.insert_download_task(&stale_task).unwrap();

        let mgr = DownloadManager::new(db.clone(), registry, http, dl_dir);
        let list = mgr.list_tasks().await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].status, DownloadStatus::Paused);
        assert_eq!(list[0].speed_bytes_per_sec, 0.0);
    }

    #[test]
    fn test_relocate_library_path_handles_container_uuid_change() {
        let tmp = tempfile::tempdir().unwrap();
        let new_dl_dir = tmp.path().join("Application Support/library");
        let ch_path = DownloadManager::chapter_file_path(&new_dl_dir, "src:novel1", "ch1", 1.0);
        std::fs::create_dir_all(ch_path.parent().unwrap()).unwrap();
        std::fs::write(&ch_path, "{}").unwrap();

        let cover_path = DownloadManager::cover_file_path(&new_dl_dir, "src:novel1", "jpg");
        std::fs::create_dir_all(cover_path.parent().unwrap()).unwrap();
        std::fs::write(&cover_path, "img").unwrap();

        let old_ch = format!(
            "/var/mobile/Containers/Data/Application/OLD-UUID/Library/Application Support/com.novelium.reader/library/novels/src_novel1/chapters/{}",
            ch_path.file_name().unwrap().to_string_lossy()
        );
        let old_cover = "/var/mobile/Containers/Data/Application/OLD-UUID/Library/Application Support/com.novelium.reader/library/covers/src_novel1.jpg";

        assert_eq!(DownloadManager::relocate_library_path(&old_ch, &new_dl_dir), ch_path);
        assert_eq!(DownloadManager::relocate_library_path(old_cover, &new_dl_dir), cover_path);
    }
}
