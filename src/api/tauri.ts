import { convertFileSrc, invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open as tauriOpen, save as tauriSave } from "@tauri-apps/plugin-dialog";
import { IS_IOS } from "../utils/format";
import type {
  AppSettings,
  Bookmark,
  ChapterContent,
  Collection,
  DeclarativeSourceManifest,
  DiagnosticsInfo,
  DiscoveryPage,
  DiscoveryQuery,
  DownloadTask,
  GoogleDriveOAuthStartResult,
  GoogleDriveSyncConfigPatch,
  GoogleDriveSyncReport,
  GoogleDriveSyncStatus,
  Highlight,
  HomeDashboardData,
  ImportBookResult,
  ImportBundleResult,
  LibraryEntry,
  LibraryNovelItem,
  LibraryStatus,
  NovelDetailsResponse,
  NovelRecord,
  NovelSummary,
  ReadingHistoryItem,
  ReadingProgress,
  ReadingStats,
  SourceHealthResult,
  SourceInfo,
  StorageStats,
  UpdateCheckResult,
} from "../types/novel";

export function isTauriEnv(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

const TAURI_UNAVAILABLE_ERROR =
  "Novelium native Tauri runtime is unavailable in plain browser preview.";

function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauriEnv()) {
    return Promise.reject(new Error(TAURI_UNAVAILABLE_ERROR));
  }
  return tauriInvoke<T>(cmd, args);
}

function open(options?: Parameters<typeof tauriOpen>[0]): ReturnType<typeof tauriOpen> {
  if (!isTauriEnv()) {
    return Promise.reject(new Error(TAURI_UNAVAILABLE_ERROR));
  }
  return tauriOpen(options);
}

function save(options?: Parameters<typeof tauriSave>[0]): ReturnType<typeof tauriSave> {
  if (!isTauriEnv()) {
    return Promise.reject(new Error(TAURI_UNAVAILABLE_ERROR));
  }
  return tauriSave(options);
}

export function resolveCoverSrc(
  coverUrl?: string | null,
  localCoverPath?: string | null,
): string | null {
  if (localCoverPath) {
    if (isTauriEnv()) {
      try {
        return convertFileSrc(localCoverPath);
      } catch {
        // fallback below
      }
    }
  }
  return coverUrl || null;
}

export function extractErrorMessage(err: unknown): string {
  if (typeof err === "string") return err;
  if (err && typeof err === "object") {
    const rec = err as Record<string, unknown>;
    if (typeof rec.message === "string") return rec.message;
  }
  if (err instanceof Error) return err.message;
  return "An unexpected error occurred";
}

export const api = {
  // Sources
  listSources: () => invoke<SourceInfo[]>("list_sources"),
  setSourceEnabled: (sourceId: string, enabled: boolean) =>
    invoke<void>("set_source_enabled", { sourceId, enabled }),
  updateSourceConfig: (sourceId: string, config: Record<string, unknown>) =>
    invoke<void>("update_source_config", { sourceId, config }),
  testSourceConnection: (sourceId: string) =>
    invoke<SourceHealthResult>("test_source_connection", { sourceId }),
  installCustomSource: (manifest: DeclarativeSourceManifest) =>
    invoke<SourceInfo>("install_custom_source", { manifest }),
  removeCustomSource: (sourceId: string) =>
    invoke<void>("remove_custom_source", { sourceId }),

  // Discovery & Novel Details & Reader Content
  discoverNovels: (query: DiscoveryQuery) =>
    invoke<DiscoveryPage>("discover_novels", { query }),
  cacheNovelSummary: (summary: NovelSummary) =>
    invoke<NovelRecord>("cache_novel_summary", { summary }),
  getNovelDetails: (novelId: string, refreshRemote = false) =>
    invoke<NovelDetailsResponse>("get_novel_details", { novelId, refreshRemote }),
  getChapterContent: (novelId: string, chapterId: string) =>
    invoke<ChapterContent>("get_chapter_content", { novelId, chapterId }),
  searchLocalNovels: (query: string) =>
    invoke<NovelRecord[]>("search_local_novels", { query }),
  cacheCoverImage: (url: string, novelId?: string | null) =>
    invoke<string | null>("cache_cover_image", {
      url,
      novelId: novelId ?? null,
    }),

  // Library, Collections, Bookmarks, History & Stats
  getHomeDashboard: () => invoke<HomeDashboardData>("get_home_dashboard"),
  listLibrary: () => invoke<LibraryNovelItem[]>("list_library"),
  updateLibraryEntry: (
    novelId: string,
    status: LibraryStatus,
    isBookmarked?: boolean,
    followUpdates?: boolean,
  ) =>
    invoke<LibraryEntry>("update_library_entry", {
      novelId,
      status,
      isBookmarked: isBookmarked ?? null,
      followUpdates: followUpdates ?? null,
    }),
  removeFromLibrary: (novelId: string) =>
    invoke<void>("remove_from_library", { novelId }),
  saveReadingProgress: (
    novelId: string,
    chapterId: string,
    scrollOffset: number,
    paragraphIndex: number,
    pageIndex: number,
    totalPages: number,
  ) =>
    invoke<ReadingProgress>("save_reading_progress", {
      novelId,
      chapterId,
      scrollOffset,
      paragraphIndex,
      pageIndex,
      totalPages,
    }),
  getReadingProgress: (novelId: string) =>
    invoke<ReadingProgress | null>("get_reading_progress", { novelId }),
  recordReadingSession: (
    novelId: string,
    chapterId: string,
    durationSeconds: number,
    wordsRead: number,
  ) =>
    invoke<void>("record_reading_session", {
      novelId,
      chapterId,
      durationSeconds,
      wordsRead,
    }),
  listReadingHistory: (limit = 100) =>
    invoke<ReadingHistoryItem[]>("list_reading_history", { limit }),
  clearReadingHistory: () => invoke<void>("clear_reading_history"),
  getReadingStats: () => invoke<ReadingStats>("get_reading_stats"),
  listBookmarks: (novelId?: string) =>
    invoke<Bookmark[]>("list_bookmarks", { novelId: novelId ?? null }),
  addBookmark: (params: {
    novelId: string;
    chapterId?: string | null;
    paragraphIndex?: number | null;
    excerpt?: string | null;
    note?: string | null;
    color?: string | null;
    isNovelBookmark?: boolean;
  }) =>
    invoke<Bookmark>("add_bookmark", {
      novelId: params.novelId,
      chapterId: params.chapterId ?? null,
      paragraphIndex: params.paragraphIndex ?? null,
      excerpt: params.excerpt ?? null,
      note: params.note ?? null,
      color: params.color ?? null,
      isNovelBookmark: params.isNovelBookmark ?? false,
    }),
  deleteBookmark: (bookmarkId: string) =>
    invoke<void>("delete_bookmark", { bookmarkId }),
  listHighlights: (novelId: string, chapterId?: string | null) =>
    invoke<Highlight[]>("list_highlights", {
      novelId,
      chapterId: chapterId ?? null,
    }),
  addHighlight: (params: {
    novelId: string;
    chapterId: string;
    paragraphIndex: number;
    startOffset: number;
    endOffset: number;
    text: string;
    color?: string | null;
    note?: string | null;
  }) =>
    invoke<Highlight>("add_highlight", {
      novelId: params.novelId,
      chapterId: params.chapterId,
      paragraphIndex: params.paragraphIndex,
      startOffset: params.startOffset,
      endOffset: params.endOffset,
      text: params.text,
      color: params.color ?? null,
      note: params.note ?? null,
    }),
  deleteHighlight: (highlightId: string) =>
    invoke<void>("delete_highlight", { highlightId }),
  listCollections: () => invoke<Collection[]>("list_collections"),
  createCollection: (name: string, description?: string, color?: string) =>
    invoke<Collection>("create_collection", {
      name,
      description: description ?? null,
      color: color ?? null,
    }),
  deleteCollection: (collectionId: string) =>
    invoke<void>("delete_collection", { collectionId }),
  setNovelInCollection: (collectionId: string, novelId: string, present: boolean) =>
    invoke<void>("set_novel_in_collection", { collectionId, novelId, present }),
  checkFollowedUpdates: () =>
    invoke<UpdateCheckResult[]>("check_followed_updates"),

  // Downloads & Storage
  enqueueDownload: (novelId: string, chapterIds: string[] = []) =>
    invoke<DownloadTask>("enqueue_download", { novelId, chapterIds }),
  listDownloads: () => invoke<DownloadTask[]>("list_downloads"),
  pauseDownload: (taskId: string) => invoke<void>("pause_download", { taskId }),
  resumeDownload: (taskId: string) => invoke<void>("resume_download", { taskId }),
  cancelDownload: (taskId: string) => invoke<void>("cancel_download", { taskId }),
  retryDownload: (taskId: string) => invoke<DownloadTask>("retry_download", { taskId }),
  deleteDownloadedChapters: (chapterIds: string[]) =>
    invoke<number>("delete_downloaded_chapters", { chapterIds }),
  deleteDownloadedNovel: (novelId: string) =>
    invoke<number>("delete_downloaded_novel", { novelId }),
  getStorageStats: () => invoke<StorageStats>("get_storage_stats"),
  pauseActiveDownloads: () =>
    isTauriEnv() ? invoke<number>("pause_active_downloads") : Promise.resolve(0),

  // Import / Export / Backup
  importLocalBook: (filePath: string) =>
    invoke<ImportBookResult>("import_local_book", { filePath }),
  exportLibraryJson: (destPath: string) =>
    invoke<number>("export_library_json", { destPath }),
  importLibraryJson: (filePath: string) =>
    invoke<ImportBundleResult>("import_library_json", { filePath }),
  backupDatabaseFile: (destPath: string) =>
    invoke<number>("backup_database_file", { destPath }),

  // Settings & Diagnostics
  getSettings: () => invoke<AppSettings>("get_settings"),
  saveSettings: (settings: AppSettings) =>
    invoke<AppSettings>("save_settings", { settings }),
  resetSettings: () => invoke<AppSettings>("reset_settings"),
  getDiagnostics: () => invoke<DiagnosticsInfo>("get_diagnostics"),
  readLocalImageDataUrl: (path: string) =>
    invoke<string>("read_local_image_data_url", { path }),

  // Google Drive Cloud Sync
  getGdriveSyncStatus: () =>
    invoke<GoogleDriveSyncStatus>("get_gdrive_sync_status"),
  saveGdriveSyncConfig: (patch: GoogleDriveSyncConfigPatch) =>
    invoke<GoogleDriveSyncStatus>("save_gdrive_sync_config", { patch }),
  startGdriveOAuth: () =>
    invoke<GoogleDriveOAuthStartResult>("start_gdrive_oauth"),
  completeGdriveOAuthManual: (callbackInput: string) =>
    invoke<GoogleDriveSyncStatus>("complete_gdrive_oauth_manual", { callbackInput }),
  cancelGdriveOAuth: () => invoke<void>("cancel_gdrive_oauth"),
  disconnectGdrive: () =>
    invoke<GoogleDriveSyncStatus>("disconnect_gdrive"),
  verifyGdriveFolder: () =>
    invoke<GoogleDriveSyncStatus>("verify_gdrive_folder"),
  syncWithGdrive: () =>
    invoke<GoogleDriveSyncReport>("sync_with_gdrive"),

  // Events & Native Window/Dialogs
  onGdriveSyncUpdated: (cb: (status: GoogleDriveSyncStatus) => void): Promise<UnlistenFn> => {
    if (!isTauriEnv()) return Promise.resolve(() => {});
    return listen<GoogleDriveSyncStatus>("gdrive-sync-updated", (event) => cb(event.payload));
  },
  onGdriveOAuthError: (cb: (message: string) => void): Promise<UnlistenFn> => {
    if (!isTauriEnv()) return Promise.resolve(() => {});
    return listen<string>("gdrive-oauth-error", (event) => cb(event.payload));
  },
  onDownloadUpdated: (cb: (task: DownloadTask) => void): Promise<UnlistenFn> => {
    if (!isTauriEnv()) return Promise.resolve(() => {});
    return listen<DownloadTask>("download-updated", (event) => cb(event.payload));
  },
  toggleFullscreen: async (): Promise<boolean> => {
    if (IS_IOS) {
      return false;
    }
    if (!isTauriEnv()) {
      try {
        if (!document.fullscreenElement) {
          await document.documentElement.requestFullscreen?.();
          return true;
        } else {
          await document.exitFullscreen?.();
          return false;
        }
      } catch {
        return false;
      }
    }
    try {
      const win = getCurrentWindow();
      const current = await win.isFullscreen();
      await win.setFullscreen(!current);
      return !current;
    } catch {
      return false;
    }
  },
  setFullscreen: async (fs: boolean): Promise<void> => {
    if (IS_IOS || !isTauriEnv()) return;
    try {
      await getCurrentWindow().setFullscreen(fs);
    } catch {
      // ignore unsupported mobile platforms
    }
  },
  pickBookFiles: async (): Promise<string[]> => {
    const selected = await open({
      multiple: true,
      title: "Import Books (EPUB, PDF, FB2, MD, TXT, HTML)",
      filters: [
        {
          name: "Ebooks & Documents",
          extensions: ["epub", "pdf", "fb2", "md", "markdown", "txt", "text", "html", "htm", "xhtml"],
        },
      ],
    });
    if (!selected) return [];
    return Array.isArray(selected) ? selected : [selected];
  },
  pickDirectory: async (title = "Select Folder"): Promise<string | null> => {
    const selected = await open({ directory: true, multiple: false, title });
    return typeof selected === "string" ? selected : null;
  },
  pickSavePath: async (defaultPath: string, ext: string, name: string): Promise<string | null> => {
    return save({
      defaultPath,
      filters: [{ name, extensions: [ext] }],
    });
  },
  pickOpenJsonFile: async (): Promise<string | null> => {
    const selected = await open({
      multiple: false,
      title: "Select Library Backup JSON",
      filters: [{ name: "JSON Backup", extensions: ["json"] }],
    });
    return typeof selected === "string" ? selected : null;
  },
};
