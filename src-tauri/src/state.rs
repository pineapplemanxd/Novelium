use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::db::Database;
use crate::downloads::DownloadManager;
use crate::net::SourceHttpClient;
use crate::sources::SourceRegistry;

#[derive(Clone)]
pub struct AppState {
    pub db: Database,
    pub sources: SourceRegistry,
    pub downloads: DownloadManager,
    pub http: SourceHttpClient,
    pub app_data_dir: PathBuf,
    pub default_download_dir: PathBuf,
    pub logs: Arc<Mutex<Vec<String>>>,
}

impl AppState {
    pub async fn log_event(&self, line: impl Into<String>) {
        let entry = format!("[{}] {}", chrono::Utc::now().format("%H:%M:%S"), line.into());
        tracing::info!("{}", entry);
        let mut logs = self.logs.lock().await;
        logs.push(entry);
        if logs.len() > 200 {
            let drain_count = logs.len() - 200;
            logs.drain(0..drain_count);
        }
    }
}
