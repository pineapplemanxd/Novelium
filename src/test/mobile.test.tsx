import { describe, expect, it, vi, beforeEach } from "vitest";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type {
  AppSettings,
  ChapterContent,
  HomeDashboardData,
  NovelDetailsResponse,
} from "../types/novel";

const mockApi = vi.hoisted(() => ({
  listSources: vi.fn(),
  setSourceEnabled: vi.fn(),
  updateSourceConfig: vi.fn(),
  testSourceConnection: vi.fn(),
  installCustomSource: vi.fn(),
  removeCustomSource: vi.fn(),
  discoverNovels: vi.fn(),
  cacheNovelSummary: vi.fn(),
  getNovelDetails: vi.fn(),
  getChapterContent: vi.fn(),
  searchLocalNovels: vi.fn(),
  cacheCoverImage: vi.fn(),
  getHomeDashboard: vi.fn(),
  listLibrary: vi.fn(),
  updateLibraryEntry: vi.fn(),
  removeFromLibrary: vi.fn(),
  saveReadingProgress: vi.fn(),
  getReadingProgress: vi.fn(),
  recordReadingSession: vi.fn(),
  listReadingHistory: vi.fn(),
  clearReadingHistory: vi.fn(),
  getReadingStats: vi.fn(),
  listBookmarks: vi.fn(),
  addBookmark: vi.fn(),
  deleteBookmark: vi.fn(),
  listHighlights: vi.fn(),
  addHighlight: vi.fn(),
  deleteHighlight: vi.fn(),
  listCollections: vi.fn(),
  createCollection: vi.fn(),
  deleteCollection: vi.fn(),
  setNovelInCollection: vi.fn(),
  checkFollowedUpdates: vi.fn(),
  enqueueDownload: vi.fn(),
  listDownloads: vi.fn(),
  pauseDownload: vi.fn(),
  pauseActiveDownloads: vi.fn(),
  resumeDownload: vi.fn(),
  cancelDownload: vi.fn(),
  retryDownload: vi.fn(),
  deleteDownloadedChapters: vi.fn(),
  deleteDownloadedNovel: vi.fn(),
  getStorageStats: vi.fn(),
  importLocalBook: vi.fn(),
  exportLibraryJson: vi.fn(),
  importLibraryJson: vi.fn(),
  backupDatabaseFile: vi.fn(),
  getSettings: vi.fn(),
  saveSettings: vi.fn(),
  resetSettings: vi.fn(),
  getDiagnostics: vi.fn(),
  readLocalImageDataUrl: vi.fn(),
  onDownloadUpdated: vi.fn(),
  toggleFullscreen: vi.fn(),
  setFullscreen: vi.fn(),
  pickBookFiles: vi.fn(),
  pickDirectory: vi.fn(),
  pickSavePath: vi.fn(),
  pickOpenJsonFile: vi.fn(),
}));

vi.mock("../api/tauri", () => ({
  api: mockApi,
  isTauriEnv: () => false,
  resolveCoverSrc: (coverUrl?: string | null, localCoverPath?: string | null) =>
    localCoverPath ? `asset://localhost/${localCoverPath}` : coverUrl || null,
  extractErrorMessage: (e: unknown) =>
    e instanceof Error ? e.message : String(e),
}));

vi.mock("../utils/format", async () => {
  const actual = await vi.importActual<typeof import("../utils/format")>(
    "../utils/format",
  );
  const React = await import("react");
  return {
    ...actual,
    IS_IOS: true,
    IS_MAC: true,
    IS_MAC_DESKTOP: false,
    MOD_KEY: "⌘",
    useDeviceLayout: () => {
      const [layout, setLayout] = React.useState(() =>
        actual.getDeviceLayout(window.innerWidth, true),
      );
      React.useEffect(() => {
        const update = () =>
          setLayout(actual.getDeviceLayout(window.innerWidth, true));
        update();
        window.addEventListener("resize", update);
        return () => window.removeEventListener("resize", update);
      }, []);
      return layout;
    },
  };
});

import { App } from "../App";
import { ReaderPage } from "../pages/ReaderPage";
import { SettingsPage } from "../pages/SettingsPage";

const IOS_BASE_SETTINGS: AppSettings = {
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
  download_dir: "/var/mobile/Containers/Data/Application/UUID/Library/Application Support/com.novelium.desktop/downloads",
  max_concurrent_downloads: 3,
  auto_download_followed: false,
  startup_page: "home",
  check_updates_on_launch: false,
};

const EMPTY_DASHBOARD: HomeDashboardData = {
  continue_reading: [],
  recently_updated: [],
  recently_added: [],
  your_library: [],
  stats: {
    total_reading_seconds: 0,
    total_words_read: 0,
    chapters_read_count: 0,
    novels_finished_count: 0,
    current_streak_days: 0,
    daily_7d: [],
  },
};

function makeReaderDetails(): NovelDetailsResponse {
  return {
    novel: {
      id: "n1",
      source_id: "gutenberg",
      external_id: "ext-1",
      title: "Mobile Test Novel",
      author_id: null,
      author: "Author Name",
      description: "Desc",
      cover_url: null,
      local_cover_path: null,
      status: "ongoing",
      genres: ["Fantasy"],
      tags: [],
      rating: 4.5,
      popularity: 100,
      chapter_count: 2,
      last_updated_remote: null,
      metadata: {},
      is_local: false,
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
    },
    chapters: [
      {
        id: "c1",
        novel_id: "n1",
        external_id: "ext-c1",
        chapter_number: 1,
        volume: null,
        title: "Chapter 1",
        url: null,
        word_count: 20,
        published_at: null,
        is_downloaded: true,
        local_path: null,
        size_bytes: 0,
      },
      {
        id: "c2",
        novel_id: "n1",
        external_id: "ext-c2",
        chapter_number: 2,
        volume: null,
        title: "Chapter 2",
        url: null,
        word_count: 20,
        published_at: null,
        is_downloaded: true,
        local_path: null,
        size_bytes: 0,
      },
    ],
    library_entry: {
      novel_id: "n1",
      status: "reading",
      is_bookmarked: false,
      follow_updates: false,
      last_checked_chapters: 2,
      added_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
    },
    translations: [
      {
        novel_id: "n1",
        source_id: "gutenberg",
        external_id: "ext-1",
        language_code: "en",
        language_label: "English",
        title: "Mobile Test Novel",
      },
      {
        novel_id: "n1-pl",
        source_id: "gutenberg",
        external_id: "ext-1-pl",
        language_code: "pl",
        language_label: "Polish",
        title: "Mobilna Powiesc",
      },
    ],
    reading_progress: null,
    bookmarks: [],
    highlights: [],
    collection_ids: [],
    downloaded_chapters: 2,
    total_size_bytes: 0,
  };
}

function makeChapter(id: string, num: number, text: string): ChapterContent {
  return {
    chapter_id: id,
    novel_id: "n1",
    chapter_number: num,
    title: `Chapter ${num}`,
    html: `<p>${text}</p>`,
    paragraphs: [text],
    word_count: 10,
    is_downloaded: true,
    prev_chapter_id: num > 1 ? `c${num - 1}` : null,
    next_chapter_id: num < 2 ? `c${num + 1}` : null,
  };
}

function setViewportWidth(width: number) {
  Object.defineProperty(window, "innerWidth", {
    configurable: true,
    writable: true,
    value: width,
  });
  act(() => {
    window.dispatchEvent(new Event("resize"));
  });
}

beforeEach(() => {
  vi.clearAllMocks();
  mockApi.getSettings.mockResolvedValue(IOS_BASE_SETTINGS);
  mockApi.saveSettings.mockResolvedValue(IOS_BASE_SETTINGS);
  mockApi.listDownloads.mockResolvedValue([]);
  mockApi.pauseActiveDownloads.mockResolvedValue(undefined);
  mockApi.onDownloadUpdated.mockResolvedValue(() => {});
  mockApi.getHomeDashboard.mockResolvedValue(EMPTY_DASHBOARD);
  mockApi.listSources.mockResolvedValue([]);
  mockApi.listBookmarks.mockResolvedValue([]);
  mockApi.listHighlights.mockResolvedValue([]);
  mockApi.saveReadingProgress.mockResolvedValue(undefined);
  mockApi.recordReadingSession.mockResolvedValue(undefined);
  mockApi.getDiagnostics.mockResolvedValue({
    db_path: "/var/mobile/.../novelium.db",
    download_dir: IOS_BASE_SETTINGS.download_dir,
    novel_count: 0,
    chapter_count: 0,
    downloaded_chapters: 0,
    source_count: 0,
    cache_bytes: 0,
  });
  mockApi.getStorageStats.mockResolvedValue({
    download_dir: IOS_BASE_SETTINGS.download_dir,
    total_bytes: 0,
    cover_cache_bytes: 0,
    chapters_downloaded: 0,
  });
});

describe("iOS and iPadOS platform detection", () => {
  it("detects iPhone, iPad, and iPadOS desktop-class mode accurately", async () => {
    const actual = await vi.importActual<typeof import("../utils/format")>(
      "../utils/format",
    );
    expect(actual.detectIosPlatform({ platform: "iPhone", userAgent: "Mobile", maxTouchPoints: 5 })).toBe(true);
    expect(actual.detectIosPlatform({ platform: "iPad", userAgent: "Mobile", maxTouchPoints: 5 })).toBe(true);
    expect(actual.detectIosPlatform({ platform: "MacIntel", userAgent: "Macintosh", maxTouchPoints: 5 })).toBe(true);
    expect(actual.detectIosPlatform({ platform: "MacIntel", userAgent: "Macintosh", maxTouchPoints: 0 })).toBe(false);

    expect(actual.getDeviceLayout(390, true)).toBe("phone");
    expect(actual.getDeviceLayout(820, true)).toBe("tablet");
    expect(actual.getDeviceLayout(1024, true)).toBe("tablet");
    expect(actual.getDeviceLayout(1024, false)).toBe("desktop");
  });
});

describe("iPhone & iPadOS App Shell", () => {
  it("renders iPhone bottom tab bar + More sheet and pauses downloads on backgrounding", async () => {
    setViewportWidth(390);
    render(<App />);

    const nav = await screen.findByRole("navigation", { name: "Primary Navigation" });
    expect(nav).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Toggle sidebar" })).not.toBeInTheDocument();

    const moreBtn = screen.getByRole("button", { name: "More navigation tabs" });
    fireEvent.click(moreBtn);
    expect(screen.getByRole("button", { name: "Close menu" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Downloads/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Settings/i })).toBeInTheDocument();

    Object.defineProperty(document, "visibilityState", {
      configurable: true,
      value: "hidden",
    });
    act(() => {
      document.dispatchEvent(new Event("visibilitychange"));
    });
    expect(mockApi.pauseActiveDownloads).toHaveBeenCalledTimes(1);
  });

  it("renders iPad portrait drawer toggle without phone bottom bar", async () => {
    setViewportWidth(820);
    render(<App />);

    const toggleBtn = await screen.findByRole("button", { name: "Toggle sidebar" });
    expect(toggleBtn).toBeInTheDocument();
    expect(screen.queryByRole("navigation", { name: "Primary Navigation" })).not.toBeInTheDocument();

    fireEvent.click(toggleBtn);
    expect(screen.getByRole("button", { name: "Close navigation" })).toBeInTheDocument();
  });
});

describe("iOS ReaderPage AMOLED, touch swipe, HUD tap, and Settings sandbox UI", () => {
  it("supports AMOLED black theme, center-tap HUD toggle, and mobile translation menu", async () => {
    setViewportWidth(390);
    const onUpdateSettings = vi.fn();
    mockApi.getNovelDetails.mockResolvedValue(makeReaderDetails());
    mockApi.getChapterContent.mockResolvedValue(
      makeChapter("c1", 1, "Tap center to hide or show controls."),
    );

    const { container } = render(
      <ReaderPage
        novelId="n1"
        initialChapterId="c1"
        settings={IOS_BASE_SETTINGS}
        onUpdateSettings={onUpdateSettings}
        onExit={vi.fn()}
        onNotify={vi.fn()}
        onSwitchNovel={vi.fn()}
      />,
    );

    const bodyText = await screen.findByText("Tap center to hide or show controls.");
    expect(container.querySelector("header")?.hasAttribute("data-tauri-drag-region")).toBe(false);

    // Open Appearance drawer ("Aa") and select AMOLED theme
    fireEvent.click(screen.getByRole("button", { name: "Aa" }));
    fireEvent.click(screen.getByRole("button", { name: "AMOLED" }));
    expect(onUpdateSettings).toHaveBeenCalledWith(
      expect.objectContaining({ reader_theme: "amoled" }),
    );
    expect(screen.getByLabelText("Reader translation language menu")).toBeInTheDocument();

    // Tap reading surface twice: first closes Appearance drawer, second hides HUD
    fireEvent.click(bodyText);
    fireEvent.click(bodyText);
    const header = container.querySelector("header");
    expect(header?.className).toContain("opacity-0");
  });

  it("navigates paginated chapter on horizontal touch swipe", async () => {
    setViewportWidth(390);
    mockApi.getNovelDetails.mockResolvedValue(makeReaderDetails());
    mockApi.getChapterContent.mockImplementation(async (_nid: string, cid: string) =>
      cid === "c2"
        ? makeChapter("c2", 2, "Second chapter on iOS.")
        : makeChapter("c1", 1, "First chapter on iOS."),
    );

    const { container } = render(
      <ReaderPage
        novelId="n1"
        initialChapterId="c1"
        settings={{ ...IOS_BASE_SETTINGS, reader_mode: "paginated" }}
        onUpdateSettings={vi.fn()}
        onExit={vi.fn()}
        onNotify={vi.fn()}
      />,
    );

    await screen.findByText("First chapter on iOS.");
    const pageSurface = container.querySelector(".flex-1.flex.flex-col.overflow-hidden") as HTMLElement;
    expect(pageSurface).not.toBeNull();

    // Wait a tick for effects, then swipe left (dx < -48) on 1-page chapter -> moves to Chapter 2
    fireEvent.touchStart(pageSurface, { touches: [{ clientX: 280, clientY: 300 }] });
    fireEvent.touchEnd(pageSurface, { changedTouches: [{ clientX: 140, clientY: 302 }] });

    await waitFor(() => {
      expect(mockApi.getChapterContent).toHaveBeenCalledWith("n1", "c2");
    });
  });

  it("hides desktop download folder Change button on iOS SettingsPage", async () => {
    render(
      <SettingsPage
        settings={IOS_BASE_SETTINGS}
        onUpdateSettings={vi.fn()}
        onNotify={vi.fn()}
      />,
    );

    fireEvent.click(await screen.findByRole("button", { name: /^Reader$/i }));
    expect(await screen.findByText(/AMOLED Pure Black/i)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /^Downloads$/i }));
    expect(screen.queryByRole("button", { name: /Change\.\.\./i })).not.toBeInTheDocument();
    expect(
      await screen.findByText(/sandboxed Application Support container/i),
    ).toBeInTheDocument();
  });
});
