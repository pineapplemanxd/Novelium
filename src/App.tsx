import React, { useCallback, useEffect, useState } from "react";
import {
  Bookmark,
  BookMarked,
  BookOpen,
  Clock,
  Compass,
  Download,
  Globe,
  Home,
  Library,
  Menu,
  Moon,
  MoreHorizontal,
  Search,
  Settings,
  Sun,
  X,
} from "lucide-react";
import { api, extractErrorMessage } from "./api/tauri";
import { IS_IOS, IS_MAC_DESKTOP, MOD_KEY, useDeviceLayout } from "./utils/format";
import { CommandPalette } from "./components/CommandPalette";
import { BookmarksPage } from "./pages/BookmarksPage";
import { DiscoverPage } from "./pages/DiscoverPage";
import { DownloadsPage } from "./pages/DownloadsPage";
import { ExplorePage } from "./pages/ExplorePage";
import { HistoryPage } from "./pages/HistoryPage";
import { HomePage, type HomeFeedCache } from "./pages/HomePage";
import { LibraryPage } from "./pages/LibraryPage";
import { NovelDetailsPage } from "./pages/NovelDetailsPage";
import { ReaderPage } from "./pages/ReaderPage";
import { SettingsPage } from "./pages/SettingsPage";
import type { AppSettings, NovelSummary } from "./types/novel";

type MainTab =
  | "home"
  | "discover"
  | "books"
  | "explore"
  | "library"
  | "downloads"
  | "history"
  | "bookmarks"
  | "settings";

const DEFAULT_SETTINGS: AppSettings = {
  theme: "dark",
  accent_color: "indigo",
  density: "comfortable",
  sidebar_sections: [
    "home",
    "discover",
    "books",
    "explore",
    "library",
    "downloads",
    "history",
    "bookmarks",
    "settings",
  ],
  reader_font: "Literata",
  reader_font_size: 18,
  reader_line_height: 1.72,
  reader_paragraph_spacing: 1.15,
  reader_width: 720,
  reader_theme: "obsidian",
  reader_custom_bg: "#121318",
  reader_custom_fg: "#E2E4EA",
  reader_custom_accent: "#818CF8",
  reader_mode: "scroll",
  reader_flip_animation: true,
  reader_text_align: "left",
  reader_auto_load_next: true,
  default_language: "en",
  download_dir: "",
  max_concurrent_downloads: 3,
  auto_download_followed: false,
  startup_page: "home",
  check_updates_on_launch: false,
};

const NAV_ITEMS: { id: MainTab; label: string; icon: React.ElementType }[] = [
  { id: "home", label: "Home", icon: Home },
  { id: "discover", label: "Discover", icon: Globe },
  { id: "books", label: "Books", icon: BookMarked },
  { id: "explore", label: "Explore", icon: Compass },
  { id: "library", label: "Library", icon: Library },
  { id: "downloads", label: "Downloads", icon: Download },
  { id: "history", label: "Reading History", icon: Clock },
  { id: "bookmarks", label: "Bookmarks", icon: Bookmark },
  { id: "settings", label: "Settings", icon: Settings },
];

export function App() {
  const [settings, setSettings] = useState<AppSettings>(DEFAULT_SETTINGS);
  const [activeTab, setActiveTab] = useState<MainTab>("home");
  const [selectedNovelId, setSelectedNovelId] = useState<string | null>(null);
  const [readerState, setReaderState] = useState<{
    novelId: string;
    chapterId: string;
    paragraphIndex?: number | null;
  } | null>(null);

  const [discoverFilters, setDiscoverFilters] = useState<{
    query?: string;
    genre?: string;
    sort?: string;
  }>({});
  const [homeFeedCache, setHomeFeedCache] = useState<HomeFeedCache | null>(null);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const deviceLayout = useDeviceLayout();
  const isPhone = deviceLayout === "phone";
  const isTablet = deviceLayout === "tablet";
  const [sidebarOpen, setSidebarOpen] = useState(() =>
    IS_IOS ? (typeof window !== "undefined" ? window.innerWidth >= 1024 : false) : true,
  );
  const [moreOpen, setMoreOpen] = useState(false);
  const [activeDownloadsCount, setActiveDownloadsCount] = useState(0);
  const [toast, setToast] = useState<{ msg: string; isError: boolean } | null>(null);

  const notify = useCallback((msg: string, isError = false) => {
    setToast({ msg, isError });
  }, []);

  useEffect(() => {
    if (!toast) return;
    const t = window.setTimeout(() => setToast(null), 4000);
    return () => window.clearTimeout(t);
  }, [toast]);

  // Apply UI theme, accent & density
  useEffect(() => {
    const root = document.documentElement;
    let resolvedTheme = settings.theme;
    if (resolvedTheme === "system") {
      resolvedTheme = window.matchMedia("(prefers-color-scheme: dark)").matches
        ? "dark"
        : "light";
    }
    root.setAttribute("data-theme", resolvedTheme);
    root.setAttribute("data-accent", settings.accent_color);
    root.setAttribute("data-density", settings.density);
  }, [settings.theme, settings.accent_color, settings.density]);

  // Initial settings + startup page + background update check + download event badge
  useEffect(() => {
    api
      .getSettings()
      .then((loaded) => {
        setSettings(loaded);
        if (
          loaded.startup_page &&
          ["home", "library", "discover"].includes(loaded.startup_page)
        ) {
          setActiveTab(loaded.startup_page as MainTab);
        }
        if (loaded.check_updates_on_launch) {
          api.checkFollowedUpdates().catch(() => {});
        }
        Promise.resolve(api.getGdriveSyncStatus?.())
          .then(async (syncSt) => {
            if (syncSt?.enabled && syncSt.connected && syncSt.auto_sync_on_launch && syncSt.folder_id) {
              const report = await api.syncWithGdrive();
              if (report.settings_applied) {
                const updatedSettings = await api.getSettings();
                setSettings(updatedSettings);
              }
              if (
                report.books_downloaded > 0 ||
                report.books_uploaded > 0 ||
                report.novels_merged > 0
              ) {
                notify(
                  `Google Drive synced: ${report.novels_merged} books merged, ${report.books_downloaded} downloaded, ${report.books_uploaded} uploaded`,
                );
              }
            }
          })
          .catch(() => {});
      })
      .catch(() => {});

    api
      .listDownloads()
      .then((tasks) => {
        setActiveDownloadsCount(
          tasks.filter((t) => t.status === "downloading" || t.status === "queued").length,
        );
      })
      .catch(() => {});

    let unlisten: (() => void) | undefined;
    api
      .onDownloadUpdated(() => {
        api
          .listDownloads()
          .then((tasks) => {
            setActiveDownloadsCount(
              tasks.filter((t) => t.status === "downloading" || t.status === "queued").length,
            );
          })
          .catch(() => {});
      })
      .then((fn) => {
        unlisten = fn;
      });
    return () => unlisten?.();
  }, []);

  // Global Cmd+K / Ctrl+K shortcut & mobile background suspension pause
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPaletteOpen((v) => !v);
      }
    };
    const onSuspend = () => {
      if (!IS_IOS) return;
      if (document.visibilityState === "hidden") {
        api.pauseActiveDownloads().catch(() => {});
      }
    };
    const onPageHide = () => {
      if (IS_IOS) api.pauseActiveDownloads().catch(() => {});
    };
    window.addEventListener("keydown", onKeyDown);
    document.addEventListener("visibilitychange", onSuspend);
    window.addEventListener("pagehide", onPageHide);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      document.removeEventListener("visibilitychange", onSuspend);
      window.removeEventListener("pagehide", onPageHide);
    };
  }, []);

  const handleUpdateSettings = useCallback(
    async (next: AppSettings) => {
      setSettings(next);
      try {
        await api.saveSettings(next);
      } catch (e) {
        notify(extractErrorMessage(e), true);
      }
    },
    [notify],
  );

  const handleOpenRemoteNovel = async (summary: NovelSummary) => {
    try {
      await api.cacheNovelSummary(summary);
      setSelectedNovelId(summary.id);
    } catch (e) {
      notify(extractErrorMessage(e), true);
    }
  };

  const handleStartReading = async (
    novelId: string,
    chapterId?: string,
    paragraphIndex?: number | null,
  ) => {
    if (chapterId) {
      setReaderState({ novelId, chapterId, paragraphIndex });
      return;
    }
    try {
      const det = await api.getNovelDetails(novelId, false);
      const target = det.reading_progress?.chapter_id || det.chapters[0]?.id;
      if (!target) {
        notify("No readable chapters found for this novel", true);
        return;
      }
      setReaderState({ novelId, chapterId: target, paragraphIndex: null });
    } catch (e) {
      notify(extractErrorMessage(e), true);
    }
  };

  const handleNavigate = (
    tab: string,
    opts?: { query?: string; genre?: string; sort?: string },
  ) => {
    setSelectedNovelId(null);
    if (opts) setDiscoverFilters(opts);
    setActiveTab(tab as MainTab);
  };

  // Fullscreen distraction-free Reader completely hides application chrome & sidebar
  if (readerState) {
    return (
      <>
        <ReaderPage
          novelId={readerState.novelId}
          initialChapterId={readerState.chapterId}
          initialParagraphIndex={readerState.paragraphIndex}
          settings={settings}
          onUpdateSettings={handleUpdateSettings}
          onSwitchNovel={(nid) => handleStartReading(nid)}
          onExit={() => setReaderState(null)}
          onNotify={notify}
        />
        {toast && (
          <div
            className={`fixed bottom-[calc(1.25rem+var(--safe-bottom))] right-4 left-4 sm:left-auto sm:right-5 z-50 px-4 py-2.5 rounded-xl text-xs font-medium shadow-xl border ${
              toast.isError
                ? "bg-rose-950/95 border-rose-500/40 text-rose-200"
                : "bg-[var(--bg-elevated)] border-[var(--border-strong)] text-[var(--text-primary)]"
            }`}
          >
            {toast.msg}
          </div>
        )}
      </>
    );
  }

  const visibleNavItems = NAV_ITEMS.filter(
    (item) =>
      item.id === "settings" ||
      item.id === "books" ||
      settings.sidebar_sections.includes(item.id),
  );

  const primaryPhoneIds: MainTab[] = ["home", "discover", "books", "library"];
  const phonePrimaryNav = primaryPhoneIds
    .map((id) => NAV_ITEMS.find((item) => item.id === id)!)
    .filter(Boolean);
  const phoneMoreNav = visibleNavItems.filter(
    (item) => !primaryPhoneIds.includes(item.id),
  );
  const isTabletOverlay =
    isTablet && typeof window !== "undefined" && window.innerWidth < 1024;

  const selectTab = (id: MainTab) => {
    setSelectedNovelId(null);
    setActiveTab(id);
    setMoreOpen(false);
    if (isTabletOverlay) setSidebarOpen(false);
  };

  return (
    <div
      style={{ height: "var(--app-height, 100dvh)" }}
      className="w-screen flex overflow-hidden bg-[var(--bg-app)] text-[var(--text-primary)]"
    >
      {/* Tablet Portrait Overlay Backdrop */}
      {isTabletOverlay && sidebarOpen && (
        <div
          aria-hidden="true"
          onClick={() => setSidebarOpen(false)}
          className="fixed inset-0 z-30 bg-black/50 backdrop-blur-[1px]"
        />
      )}

      {/* Sidebar (Desktop + iPad; hidden on iPhone in favor of Bottom Tab Bar) */}
      {!isPhone && sidebarOpen && (
        <aside
          className={`w-56 shrink-0 flex flex-col border-r border-[var(--border-subtle)] bg-[var(--bg-sidebar)] select-none ${
            isTabletOverlay ? "fixed inset-y-0 left-0 z-40 shadow-2xl" : ""
          } ${IS_IOS ? "pt-[var(--safe-top)] pb-[var(--safe-bottom)]" : ""}`}
        >
          {/* macOS Traffic Lights Spacer + Brand */}
          <div
            data-tauri-drag-region={IS_MAC_DESKTOP ? "deep" : undefined}
            className={`${IS_MAC_DESKTOP ? "macos-drag pt-2 justify-end" : "justify-between"} h-12 px-4 flex items-center`}
          >
            <div className="flex items-center gap-2 text-xs font-semibold tracking-tight text-[var(--text-primary)]">
              <BookOpen className="w-4 h-4 text-[var(--accent-hover)]" />
              <span>Novelium</span>
            </div>
            {isTabletOverlay && (
              <button
                type="button"
                aria-label="Close navigation"
                onClick={() => setSidebarOpen(false)}
                className="p-2 rounded-lg text-[var(--text-muted)] hover:text-[var(--text-primary)]"
              >
                <X className="w-4 h-4" />
              </button>
            )}
          </div>

          <nav className="flex-1 px-2.5 py-2 space-y-0.5 overflow-y-auto">
            {visibleNavItems.map((item) => {
              const Icon = item.icon;
              const isActive = !selectedNovelId && activeTab === item.id;
              return (
                <button
                  key={item.id}
                  type="button"
                  onClick={() => selectTab(item.id)}
                  className={`w-full flex items-center justify-between px-3 ${
                    isTablet ? "py-2.5 min-h-[44px]" : "py-2"
                  } rounded-lg text-[13px] transition-colors ${
                    isActive
                      ? "bg-[var(--accent-soft)] text-[var(--accent-hover)] font-medium"
                      : "text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]"
                  }`}
                >
                  <span className="flex items-center gap-2.5 truncate">
                    <Icon className="w-4 h-4 shrink-0" />
                    <span className="truncate">{item.label}</span>
                  </span>
                  {item.id === "downloads" && activeDownloadsCount > 0 && (
                    <span
                      className="px-1.5 py-0.5 rounded-full text-[10px] font-semibold text-white"
                      style={{ backgroundColor: "var(--accent)" }}
                    >
                      {activeDownloadsCount}
                    </span>
                  )}
                </button>
              );
            })}
          </nav>

          <div className="p-3 border-t border-[var(--border-subtle)] text-[11px] text-[var(--text-muted)] flex items-center justify-between gap-2">
            <span>Novelium v0.1</span>
            <div className="flex items-center gap-2">
              <button
                type="button"
                aria-label="Toggle Light/Dark Theme"
                title={
                  settings.theme === "light"
                    ? "Switch to Dark theme"
                    : "Switch to White theme"
                }
                onClick={() =>
                  handleUpdateSettings({
                    ...settings,
                    theme: settings.theme === "light" ? "dark" : "light",
                  })
                }
                className="p-1.5 rounded-md hover:bg-[var(--bg-hover)] text-[var(--text-secondary)] hover:text-[var(--text-primary)] transition-colors"
              >
                {settings.theme === "light" ? (
                  <Moon className="w-3.5 h-3.5" />
                ) : (
                  <Sun className="w-3.5 h-3.5" />
                )}
              </button>
              {!IS_IOS && <span>{MOD_KEY}K</span>}
            </div>
          </div>
        </aside>
      )}

      {/* Main Content Shell */}
      <div className="flex-1 min-w-0 flex flex-col overflow-hidden">
        {/* Top Bar with macOS Drag Region & Global Search */}
        <header
          data-tauri-drag-region={IS_MAC_DESKTOP ? "deep" : undefined}
          className={`${IS_MAC_DESKTOP ? "macos-drag" : ""} shrink-0 px-3 sm:px-4 border-b border-[var(--border-subtle)] bg-[var(--bg-app)]/95 backdrop-blur-md flex items-center justify-between gap-2 sm:gap-4 ${
            IS_IOS || isPhone
              ? "pt-[var(--safe-top)] h-[calc(3.25rem+var(--safe-top))]"
              : "h-12"
          } ${IS_MAC_DESKTOP && !sidebarOpen ? "pl-20" : ""}`}
        >
          <div className="macos-no-drag flex items-center gap-2">
            {isPhone ? (
              <button
                type="button"
                onClick={() => selectTab("home")}
                className="flex items-center gap-1.5 px-1 py-1.5 min-h-[40px] text-xs font-semibold tracking-tight text-[var(--text-primary)]"
              >
                <BookOpen className="w-4 h-4 text-[var(--accent-hover)]" />
                <span>Novelium</span>
              </button>
            ) : (
              <button
                type="button"
                data-tauri-drag-region="false"
                aria-label="Toggle sidebar"
                onClick={() => setSidebarOpen((v) => !v)}
                className="p-2 rounded-lg text-[var(--text-secondary)] hover:text-[var(--text-primary)] hover:bg-[var(--bg-hover)]"
              >
                <Menu className="w-4 h-4" />
              </button>
            )}
          </div>

          <button
            type="button"
            data-tauri-drag-region="false"
            onClick={() => setPaletteOpen(true)}
            className="macos-no-drag flex items-center justify-between gap-3 flex-1 max-w-md px-3 py-2 sm:py-1.5 min-h-[38px] rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-surface)] hover:border-[var(--border-strong)] text-xs text-[var(--text-muted)] transition-colors"
          >
            <span className="flex items-center gap-2 truncate">
              <Search className="w-3.5 h-3.5 shrink-0" />
              <span className="truncate">
                {isPhone ? "Search novels..." : "Search library or online novels..."}
              </span>
            </span>
            {!isPhone && !IS_IOS && (
              <kbd className="px-1.5 py-0.5 rounded bg-[var(--bg-elevated)] text-[10px] border border-[var(--border-subtle)]">
                {MOD_KEY}K
              </kbd>
            )}
          </button>

          {isPhone ? (
            <button
              type="button"
              aria-label="Toggle Light/Dark Theme"
              onClick={() =>
                handleUpdateSettings({
                  ...settings,
                  theme: settings.theme === "light" ? "dark" : "light",
                })
              }
              className="p-2 min-w-[40px] min-h-[40px] flex items-center justify-center rounded-lg text-[var(--text-secondary)] hover:text-[var(--text-primary)] hover:bg-[var(--bg-hover)]"
            >
              {settings.theme === "light" ? (
                <Moon className="w-4 h-4" />
              ) : (
                <Sun className="w-4 h-4" />
              )}
            </button>
          ) : (
            <div className="w-8" />
          )}
        </header>

        {/* Active View */}
        <main className="flex-1 overflow-y-auto overscroll-y-contain">
          {selectedNovelId ? (
            <NovelDetailsPage
              novelId={selectedNovelId}
              defaultLanguage={settings.default_language}
              onBack={() => setSelectedNovelId(null)}
              onSelectNovel={(nid) => setSelectedNovelId(nid)}
              onStartReading={(nid, cid) => handleStartReading(nid, cid)}
              onSearchAuthor={(author) =>
                handleNavigate("discover", { query: author, genre: "All" })
              }
              onNotify={notify}
            />
          ) : activeTab === "home" ? (
            <HomePage
              feedCache={homeFeedCache}
              onUpdateFeedCache={setHomeFeedCache}
              onOpenNovel={(id) => setSelectedNovelId(id)}
              onOpenRemoteNovel={handleOpenRemoteNovel}
              onStartReading={(nid, cid) => handleStartReading(nid, cid)}
              onNavigate={handleNavigate}
            />
          ) : activeTab === "discover" ? (
            <DiscoverPage
              key="discover"
              initialQuery={discoverFilters.query}
              initialGenre={discoverFilters.genre}
              initialSort={discoverFilters.sort}
              onOpenNovel={handleOpenRemoteNovel}
            />
          ) : activeTab === "books" ? (
            <DiscoverPage
              key="books"
              mode="books"
              onOpenNovel={handleOpenRemoteNovel}
            />
          ) : activeTab === "explore" ? (
            <ExplorePage
              onOpenGenreInDiscover={(g) =>
                handleNavigate("discover", { genre: g, query: "" })
              }
              onOpenNovel={handleOpenRemoteNovel}
            />
          ) : activeTab === "library" ? (
            <LibraryPage
              onOpenNovel={(id) => setSelectedNovelId(id)}
              onStartReading={(nid, cid) => handleStartReading(nid, cid)}
              onNotify={notify}
            />
          ) : activeTab === "downloads" ? (
            <DownloadsPage
              onOpenNovel={(id) => setSelectedNovelId(id)}
              onNotify={notify}
            />
          ) : activeTab === "history" ? (
            <HistoryPage
              onOpenNovel={(id) => setSelectedNovelId(id)}
              onStartReading={(nid, cid) => handleStartReading(nid, cid)}
              onNotify={notify}
            />
          ) : activeTab === "bookmarks" ? (
            <BookmarksPage
              onOpenNovel={(id) => setSelectedNovelId(id)}
              onJumpToBookmark={(nid, cid, pIdx) => handleStartReading(nid, cid, pIdx)}
              onNotify={notify}
            />
          ) : (
            <SettingsPage
              settings={settings}
              onUpdateSettings={handleUpdateSettings}
              onNotify={notify}
            />
          )}
        </main>

        {/* iPhone Bottom Tab Bar */}
        {isPhone && (
          <nav
            aria-label="Primary Navigation"
            className="shrink-0 border-t border-[var(--border-subtle)] bg-[var(--bg-sidebar)]/98 backdrop-blur-xl grid grid-cols-5 pt-1 pb-[max(var(--safe-bottom),0.5rem)] z-30"
          >
            {phonePrimaryNav.map((item) => {
              const Icon = item.icon;
              const isActive = !moreOpen && !selectedNovelId && activeTab === item.id;
              return (
                <button
                  key={item.id}
                  type="button"
                  onClick={() => selectTab(item.id)}
                  className={`flex flex-col items-center justify-center gap-0.5 py-2 min-h-[52px] text-[10px] transition-colors ${
                    isActive
                      ? "text-[var(--accent-hover)] font-semibold"
                      : "text-[var(--text-secondary)]"
                  }`}
                >
                  <Icon className="w-5 h-5" />
                  <span className="truncate max-w-full px-1">{item.label}</span>
                </button>
              );
            })}
            <button
              type="button"
              aria-label="More navigation tabs"
              aria-expanded={moreOpen}
              onClick={() => setMoreOpen((v) => !v)}
              className={`relative flex flex-col items-center justify-center gap-0.5 py-2 min-h-[52px] text-[10px] transition-colors ${
                moreOpen ||
                (!selectedNovelId && !primaryPhoneIds.includes(activeTab))
                  ? "text-[var(--accent-hover)] font-semibold"
                  : "text-[var(--text-secondary)]"
              }`}
            >
              <MoreHorizontal className="w-5 h-5" />
              <span>More</span>
              {activeDownloadsCount > 0 && (
                <span
                  className="absolute top-1.5 right-4 w-2 h-2 rounded-full"
                  style={{ backgroundColor: "var(--accent)" }}
                />
              )}
            </button>
          </nav>
        )}
      </div>

      {/* iPhone "More" Bottom Sheet */}
      {isPhone && moreOpen && (
        <div
          className="fixed inset-0 z-40 flex items-end bg-black/55 backdrop-blur-[1px]"
          onClick={() => setMoreOpen(false)}
        >
          <div
            onClick={(e) => e.stopPropagation()}
            className="w-full rounded-t-2xl border-t border-[var(--border-strong)] bg-[var(--bg-surface)] p-4 pb-[calc(1rem+var(--safe-bottom))] space-y-2 shadow-2xl"
          >
            <div className="flex items-center justify-between pb-2 border-b border-[var(--border-subtle)]">
              <span className="text-sm font-semibold">More Sections</span>
              <button
                type="button"
                aria-label="Close menu"
                onClick={() => setMoreOpen(false)}
                className="p-2 rounded-lg text-[var(--text-muted)]"
              >
                <X className="w-4 h-4" />
              </button>
            </div>
            <div className="grid grid-cols-1 gap-1">
              {phoneMoreNav.map((item) => {
                const Icon = item.icon;
                const isActive = !selectedNovelId && activeTab === item.id;
                return (
                  <button
                    key={item.id}
                    type="button"
                    onClick={() => selectTab(item.id)}
                    className={`w-full flex items-center justify-between px-3.5 py-3 min-h-[44px] rounded-xl text-sm transition-colors ${
                      isActive
                        ? "bg-[var(--accent-soft)] text-[var(--accent-hover)] font-medium"
                        : "text-[var(--text-primary)] hover:bg-[var(--bg-hover)]"
                    }`}
                  >
                    <span className="flex items-center gap-3">
                      <Icon className="w-4 h-4" />
                      <span>{item.label}</span>
                    </span>
                    {item.id === "downloads" && activeDownloadsCount > 0 && (
                      <span
                        className="px-2 py-0.5 rounded-full text-[11px] font-semibold text-white"
                        style={{ backgroundColor: "var(--accent)" }}
                      >
                        {activeDownloadsCount}
                      </span>
                    )}
                  </button>
                );
              })}
            </div>
          </div>
        </div>
      )}

      {/* Global Search Command Palette */}
      <CommandPalette
        open={paletteOpen}
        onClose={() => setPaletteOpen(false)}
        onOpenLocalNovel={(id) => setSelectedNovelId(id)}
        onOpenRemoteNovel={handleOpenRemoteNovel}
        onSearchAllSources={(q) =>
          handleNavigate("discover", { query: q, genre: "All" })
        }
      />

      {/* Toast Notification */}
      {toast && (
        <div
          className={`fixed ${
            isPhone
              ? "bottom-[calc(4.75rem+var(--safe-bottom))] left-3 right-3"
              : "bottom-5 right-5"
          } z-50 flex items-center justify-between gap-3 px-4 py-2.5 rounded-xl text-xs font-medium shadow-xl border ${
            toast.isError
              ? "bg-rose-950/95 border-rose-500/40 text-rose-200"
              : "bg-[var(--bg-elevated)] border-[var(--border-strong)] text-[var(--text-primary)]"
          }`}
        >
          <span>{toast.msg}</span>
          <button type="button" onClick={() => setToast(null)} className="opacity-70 hover:opacity-100">
            <X className="w-3.5 h-3.5" />
          </button>
        </div>
      )}
    </div>
  );
}

export default App;
