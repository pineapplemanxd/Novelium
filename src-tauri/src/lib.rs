pub mod commands;
pub mod db;
pub mod downloads;
pub mod error;
pub mod formats;
pub mod models;
pub mod net;
pub mod sources;
pub mod state;
pub mod sync;

use std::sync::Arc;
use tauri::Manager;
use tokio::sync::Mutex;

use crate::db::Database;
use crate::downloads::DownloadManager;
use crate::net::SourceHttpClient;
use crate::sources::SourceRegistry;
use crate::state::AppState;
use crate::sync::GoogleDriveSyncCoordinator;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "novelium_lib=info,warn".into()),
        )
        .try_init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .setup(|app| {
            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("failed to resolve app data directory");
            std::fs::create_dir_all(&app_data_dir)?;

            let db_path = app_data_dir.join("novelium.db");
            let default_download_dir = app_data_dir.join("library");
            let custom_sources_dir = app_data_dir.join("sources");
            #[cfg(target_os = "ios")]
            let cache_dir = app
                .path()
                .app_cache_dir()
                .unwrap_or_else(|_| app_data_dir.join("cache"))
                .join("parsed_books");
            #[cfg(not(target_os = "ios"))]
            let cache_dir = app_data_dir.join("cache").join("parsed_books");
            std::fs::create_dir_all(&default_download_dir)?;
            std::fs::create_dir_all(&custom_sources_dir)?;
            std::fs::create_dir_all(&cache_dir)?;

            let db = Database::open(&db_path)?;
            let http = SourceHttpClient::new()?;
            let sources = SourceRegistry::new(
                &db,
                http.clone(),
                custom_sources_dir,
                cache_dir,
            )?;
            let downloads = DownloadManager::new(
                db.clone(),
                sources.clone(),
                http.clone(),
                default_download_dir.clone(),
            );

            let handle = app.handle().clone();
            let dl_clone = downloads.clone();
            tauri::async_runtime::spawn(async move {
                dl_clone.set_app_handle(handle).await;
            });

            let state = AppState {
                db,
                sources,
                downloads,
                http,
                gdrive: GoogleDriveSyncCoordinator::new(),
                app_data_dir,
                default_download_dir,
                logs: Arc::new(Mutex::new(vec![
                    format!(
                        "[{}] Novelium initialized cleanly",
                        chrono::Utc::now().format("%H:%M:%S")
                    ),
                ])),
            };

            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Sources
            commands::sources::list_sources,
            commands::sources::set_source_enabled,
            commands::sources::update_source_config,
            commands::sources::test_source_connection,
            commands::sources::install_custom_source,
            commands::sources::remove_custom_source,
            // Discovery & Reader content
            commands::discovery::discover_novels,
            commands::discovery::cache_novel_summary,
            commands::discovery::get_novel_details,
            commands::discovery::get_chapter_content,
            commands::discovery::search_local_novels,
            commands::discovery::cache_cover_image,
            // Library, Collections, Bookmarks, History & Progress
            commands::library::get_home_dashboard,
            commands::library::list_library,
            commands::library::update_library_entry,
            commands::library::remove_from_library,
            commands::library::save_reading_progress,
            commands::library::get_reading_progress,
            commands::library::record_reading_session,
            commands::library::list_reading_history,
            commands::library::clear_reading_history,
            commands::library::get_reading_stats,
            commands::library::list_bookmarks,
            commands::library::add_bookmark,
            commands::library::delete_bookmark,
            commands::library::list_highlights,
            commands::library::add_highlight,
            commands::library::delete_highlight,
            commands::library::list_collections,
            commands::library::create_collection,
            commands::library::delete_collection,
            commands::library::set_novel_in_collection,
            commands::library::check_followed_updates,
            // Downloads & Storage
            commands::downloads::enqueue_download,
            commands::downloads::list_downloads,
            commands::downloads::pause_download,
            commands::downloads::resume_download,
            commands::downloads::cancel_download,
            commands::downloads::retry_download,
            commands::downloads::pause_active_downloads,
            commands::downloads::delete_downloaded_chapters,
            commands::downloads::delete_downloaded_novel,
            commands::downloads::get_storage_stats,
            // Import & Export
            commands::import_export::import_local_book,
            commands::import_export::export_library_json,
            commands::import_export::import_library_json,
            commands::import_export::backup_database_file,
            // Settings & Diagnostics
            commands::settings::get_settings,
            commands::settings::save_settings,
            commands::settings::reset_settings,
            commands::settings::get_diagnostics,
            commands::settings::read_local_image_data_url,
            // Google Drive Cloud Sync
            commands::sync::get_gdrive_sync_status,
            commands::sync::save_gdrive_sync_config,
            commands::sync::start_gdrive_oauth,
            commands::sync::complete_gdrive_oauth_manual,
            commands::sync::cancel_gdrive_oauth,
            commands::sync::disconnect_gdrive,
            commands::sync::verify_gdrive_folder,
            commands::sync::sync_with_gdrive,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Novelium Tauri application");
}
