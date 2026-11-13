import { describe, expect, it, vi, beforeEach } from "vitest";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import {
  estimateReadingMinutes,
  formatBytes,
  formatDuration,
  formatNumber,
  formatRelativeDate,
  formatSpeed,
  libraryStatusLabel,
  sourceLabel,
} from "../utils/format";
import { CoverImage } from "../components/CoverImage";
import { DiscoverPage } from "../pages/DiscoverPage";
import { LibraryPage } from "../pages/LibraryPage";
import { NovelDetailsPage } from "../pages/NovelDetailsPage";
import { ReaderPage } from "../pages/ReaderPage";
import { SettingsPage } from "../pages/SettingsPage";
import type {
  AppSettings,
  ChapterContent,
  DownloadTask,
  HomeDashboardData,
  NovelDetailsResponse,
  SourceInfo,
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

import { App } from "../App";

const BASE_SETTINGS: AppSettings = {
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
  download_dir: "/tmp/novelium-downloads",
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
    total_reading_seconds: 1200,
    total_words_read: 5400,
    chapters_read_count: 3,
    novels_finished_count: 1,
    current_streak_days: 2,
    daily_7d: [],
  },
};

const SAMPLE_SOURCE: SourceInfo = {
  id: "gutenberg",
  name: "Project Gutenberg",
  kind: "api",
  base_url: "https://gutendex.com",
  description: "Public domain literature",
  enabled: true,
  priority: 1,
  health_status: "healthy",
  last_checked_at: null,
  capabilities: {
    search: true,
    browse_popular: true,
    browse_trending: true,
    browse_latest: true,
    browse_genre: true,
    filter_status: false,
    genres: ["Fiction"],
  },
  config: {},
  config_schema: [],
  is_custom: false,
};

beforeEach(() => {
  vi.clearAllMocks();
  mockApi.getSettings.mockResolvedValue(BASE_SETTINGS);
  mockApi.saveSettings.mockResolvedValue(BASE_SETTINGS);
  mockApi.listDownloads.mockResolvedValue([]);
  mockApi.onDownloadUpdated.mockResolvedValue(() => {});
  mockApi.getHomeDashboard.mockResolvedValue(EMPTY_DASHBOARD);
  mockApi.discoverNovels.mockResolvedValue({
    items: [],
    page: 1,
    has_more: false,
    total_estimate: 0,
    source_errors: [],
  });
  mockApi.listSources.mockResolvedValue([SAMPLE_SOURCE]);
  mockApi.getDiagnostics.mockResolvedValue({
    app_data_dir: "/tmp/novelium",
    db_path: "/tmp/novelium/novelium.db",
    custom_sources_dir: "/tmp/novelium/sources",
    logs: [],
  });
  mockApi.searchLocalNovels.mockResolvedValue([]);
  mockApi.cacheCoverImage.mockResolvedValue(null);
  mockApi.listHighlights.mockResolvedValue([]);
  mockApi.deleteHighlight.mockResolvedValue(undefined);
  mockApi.recordReadingSession.mockResolvedValue(undefined);
  mockApi.saveReadingProgress.mockResolvedValue({
    novel_id: "n1",
    chapter_id: "c1",
    chapter_number: 1,
    chapter_title: "Chapter 1",
    scroll_offset: 0,
    paragraph_index: 0,
    page_index: 0,
    total_pages: 1,
    progress_percent: 100,
    updated_at: "2026-01-01T00:00:00Z",
  });
});

describe("format utilities", () => {
  it("handles bytes, speeds, durations, and edge cases", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(-42)).toBe("0 B");
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1536)).toBe("1.5 KB");
    expect(formatBytes(1024 * 1024)).toBe("1.0 MB");

    expect(formatSpeed(0)).toBe("—");
    expect(formatSpeed(2048)).toBe("2.0 KB/s");

    expect(formatDuration(0)).toBe("0m");
    expect(formatDuration(45)).toBe("45s");
    expect(formatDuration(180)).toBe("3m");
    expect(formatDuration(3720)).toBe("1h 2m");

    expect(formatNumber(950)).toBe("950");
    expect(formatNumber(12500)).toBe("12.5K");
    expect(formatNumber(2_500_000)).toBe("2.5M");

    expect(estimateReadingMinutes(0)).toBe(1);
    expect(estimateReadingMinutes(720)).toBe(3);

    expect(formatRelativeDate(null)).toBe("—");
    expect(formatRelativeDate("not-a-date")).toBe("not-a-date");

    expect(sourceLabel("gutenberg")).toBe("Project Gutenberg");
    expect(sourceLabel("custom_opds")).toBe("custom_opds");
    expect(libraryStatusLabel("plan_to_read")).toBe("Plan to Read");
    expect(libraryStatusLabel("unknown")).toBe("unknown");
  });
});

describe("CoverImage", () => {
  it("renders a typographic cover when no cover URL is provided", () => {
    render(<CoverImage title="The Starlight Archive" author="Ada Lovelace" />);
    expect(screen.getByText("The Starlight Archive")).toBeInTheDocument();
    expect(screen.getByText("Ada Lovelace")).toBeInTheDocument();
  });

  it("falls back cleanly when cover image fails to load", async () => {
    render(
      <CoverImage
        title="Broken Cover Novel"
        author="Jane Doe"
        coverUrl="https://example.invalid/missing.jpg"
      />,
    );
    const img = screen.getByRole("img", { name: "Broken Cover Novel" });
    fireEvent.error(img);

    await waitFor(() => {
      expect(screen.getByText("Broken Cover Novel")).toBeInTheDocument();
    });
  });
});

describe("App shell and CommandPalette", () => {
  it("renders navigation sidebar and toggles global Cmd+K search modal", async () => {
    render(<App />);

    await waitFor(() => {
      expect(screen.getByText("Novelium")).toBeInTheDocument();
    });

    expect(screen.getByRole("button", { name: /^Discover$/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^Library$/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^Settings$/i })).toBeInTheDocument();

    fireEvent.keyDown(window, { key: "k", metaKey: true });

    const input = await screen.findByPlaceholderText(
      /Search your library, or press Enter to search online sources/i,
    );
    expect(input).toBeInTheDocument();

    fireEvent.keyDown(input, { key: "Escape" });
    await waitFor(() => {
      expect(
        screen.queryByPlaceholderText(
          /Search your library, or press Enter to search online sources/i,
        ),
      ).not.toBeInTheDocument();
    });
  });
});

describe("ReaderPage", () => {
  it("loads chapter text and persists typography/theme/mode changes", async () => {
    const details: NovelDetailsResponse = {
      novel: {
        id: "n1",
        source_id: "local",
        external_id: "ext-1",
        title: "A Voyage to Arcturus",
        author_id: null,
        author: "David Lindsay",
        description: "Classic sci-fi",
        cover_url: null,
        local_cover_path: null,
        status: "completed",
        genres: ["Science Fiction"],
        tags: [],
        rating: null,
        popularity: 0,
        chapter_count: 2,
        last_updated_remote: null,
        metadata: {},
        is_local: true,
        created_at: "2026-01-01T00:00:00Z",
        updated_at: "2026-01-01T00:00:00Z",
      },
      chapters: [
        {
          id: "c1",
          novel_id: "n1",
          external_id: "1",
          chapter_number: 1,
          volume: null,
          title: "The Seance",
          url: null,
          word_count: 24,
          published_at: null,
          is_downloaded: true,
          local_path: "/tmp/c1.json",
          size_bytes: 120,
        },
        {
          id: "c2",
          novel_id: "n1",
          external_id: "2",
          chapter_number: 2,
          volume: null,
          title: "In the Street",
          url: null,
          word_count: 20,
          published_at: null,
          is_downloaded: false,
          local_path: null,
          size_bytes: 0,
        },
      ],
      library_entry: null,
      reading_progress: null,
      bookmarks: [],
      highlights: [],
      translations: [],
      collection_ids: [],
      downloaded_chapters: 1,
      total_size_bytes: 120,
    };

    const chapter: ChapterContent = {
      chapter_id: "c1",
      novel_id: "n1",
      chapter_number: 1,
      title: "The Seance",
      html: "<p>First paragraph in the dark room.</p><p>Second paragraph follows cleanly.</p>",
      paragraphs: [
        "First paragraph in the dark room.",
        "Second paragraph follows cleanly.",
      ],
      word_count: 10,
      is_downloaded: true,
      prev_chapter_id: null,
      next_chapter_id: "c2",
    };

    mockApi.getNovelDetails.mockResolvedValue(details);
    mockApi.getChapterContent.mockResolvedValue(chapter);

    const onUpdateSettings = vi.fn();
    const onExit = vi.fn();
    const onNotify = vi.fn();

    render(
      <ReaderPage
        novelId="n1"
        initialChapterId="c1"
        settings={BASE_SETTINGS}
        onUpdateSettings={onUpdateSettings}
        onExit={onExit}
        onNotify={onNotify}
      />,
    );

    await waitFor(() => {
      expect(screen.getByText("First paragraph in the dark room.")).toBeInTheDocument();
    });
    expect(screen.getByText("Offline")).toBeInTheDocument();

    fireEvent.click(screen.getByTitle("Typography & Theme (A)"));
    expect(screen.getByText("Reading Studio")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Sepia" }));
    expect(onUpdateSettings).toHaveBeenCalledWith(
      expect.objectContaining({ reader_theme: "sepia" }),
    );

    fireEvent.click(screen.getByRole("button", { name: "Book Flip" }));
    expect(onUpdateSettings).toHaveBeenCalledWith(
      expect.objectContaining({ reader_mode: "paginated" }),
    );
  });
});

describe("SettingsPage custom source installation", () => {
  it("submits custom declarative OPDS source manifest", async () => {
    mockApi.installCustomSource.mockResolvedValue({
      ...SAMPLE_SOURCE,
      id: "calibre_lan",
      name: "Calibre Home",
      kind: "opds",
      is_custom: true,
    });

    const onUpdateSettings = vi.fn();
    const onNotify = vi.fn();

    render(
      <SettingsPage
        settings={BASE_SETTINGS}
        onUpdateSettings={onUpdateSettings}
        onNotify={onNotify}
      />,
    );

    await waitFor(() => {
      expect(mockApi.listSources).toHaveBeenCalled();
    });

    fireEvent.click(screen.getByRole("button", { name: "Sources" }));
    expect(await screen.findByText("Project Gutenberg")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /Add Custom Source/i }));

    fireEvent.change(screen.getByPlaceholderText("ID (e.g. my_calibre)"), {
      target: { value: "calibre_lan" },
    });
    fireEvent.change(screen.getByPlaceholderText("Display Name"), {
      target: { value: "Calibre Home" },
    });
    fireEvent.change(screen.getByPlaceholderText("Base URL (https://...)"), {
      target: { value: "http://127.0.0.1:8080" },
    });
    fireEvent.change(screen.getByPlaceholderText("OPDS Atom Feed URL (supports {page})"), {
      target: { value: "http://127.0.0.1:8080/opds" },
    });

    fireEvent.click(screen.getByRole("button", { name: /Save & Activate Source/i }));

    await waitFor(() => {
      expect(mockApi.installCustomSource).toHaveBeenCalledWith(
        expect.objectContaining({
          id: "calibre_lan",
          name: "Calibre Home",
          kind: "opds",
          base_url: "http://127.0.0.1:8080",
          feed_url: "http://127.0.0.1:8080/opds",
        }),
      );
    });
  });
});

describe("ReaderPage HTML rendering, startAtEnd pagination, and unmount flush", () => {
  const mkDetails = (): NovelDetailsResponse => ({
    novel: {
      id: "n1",
      source_id: "local",
      external_id: "ext-1",
      title: "Rich Novel",
      author_id: null,
      author: "Author",
      description: "",
      cover_url: null,
      local_cover_path: null,
      status: "completed",
      genres: [],
      tags: [],
      rating: null,
      popularity: 0,
      chapter_count: 2,
      last_updated_remote: null,
      metadata: {},
      is_local: true,
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
    },
    chapters: [
      {
        id: "c1",
        novel_id: "n1",
        external_id: "1",
        chapter_number: 1,
        volume: null,
        title: "Chapter One",
        url: null,
        word_count: 15,
        published_at: null,
        is_downloaded: true,
        local_path: null,
        size_bytes: 0,
      },
      {
        id: "c2",
        novel_id: "n1",
        external_id: "2",
        chapter_number: 2,
        volume: null,
        title: "Chapter Two",
        url: null,
        word_count: 10,
        published_at: null,
        is_downloaded: true,
        local_path: null,
        size_bytes: 0,
      },
    ],
    library_entry: null,
    reading_progress: null,
    bookmarks: [],
    highlights: [],
    translations: [],
    collection_ids: [],
    downloaded_chapters: 2,
    total_size_bytes: 0,
  });

  it("renders inline HTML formatting, blockquotes, and hr while stripping scripts", async () => {
    const details = mkDetails();
    const richChapter: ChapterContent = {
      chapter_id: "c1",
      novel_id: "n1",
      chapter_number: 1,
      title: "Chapter One",
      html: "<p>She saw an <em>italicized clue</em> and a <strong>bold mark</strong>.</p><hr/><blockquote><p>Quoted inscription.</p></blockquote><script>window.__xss=1</script>",
      paragraphs: [
        "She saw an italicized clue and a bold mark.",
        "Quoted inscription.",
      ],
      word_count: 11,
      is_downloaded: true,
      prev_chapter_id: null,
      next_chapter_id: "c2",
    };

    mockApi.getNovelDetails.mockResolvedValue(details);
    mockApi.getChapterContent.mockResolvedValue(richChapter);

    const { container } = render(
      <ReaderPage
        novelId="n1"
        initialChapterId="c1"
        settings={BASE_SETTINGS}
        onUpdateSettings={vi.fn()}
        onExit={vi.fn()}
        onNotify={vi.fn()}
      />,
    );

    const emEl = await screen.findByText("italicized clue");
    expect(emEl.tagName.toLowerCase()).toBe("em");
    expect(screen.getByText("bold mark").tagName.toLowerCase()).toBe("strong");
    expect(container.querySelector("blockquote")).toBeInTheDocument();
    expect(container.querySelector("hr")).toBeInTheDocument();
    expect(container.querySelector("script")).toBeNull();
  });

  it("lands on the last page when stepping backward into previous chapter in paginated mode", async () => {
    const details = mkDetails();
    const ch2: ChapterContent = {
      chapter_id: "c2",
      novel_id: "n1",
      chapter_number: 2,
      title: "Chapter Two",
      html: "<p>Second chapter body.</p>",
      paragraphs: ["Second chapter body."],
      word_count: 3,
      is_downloaded: true,
      prev_chapter_id: "c1",
      next_chapter_id: null,
    };
    const ch1: ChapterContent = {
      chapter_id: "c1",
      novel_id: "n1",
      chapter_number: 1,
      title: "Chapter One",
      html: "<p>First chapter spanning two pages.</p>",
      paragraphs: ["First chapter spanning two pages."],
      word_count: 5,
      is_downloaded: true,
      prev_chapter_id: null,
      next_chapter_id: "c2",
    };

    mockApi.getNovelDetails.mockResolvedValue(details);
    mockApi.getChapterContent.mockImplementation(async (_nid: string, cid: string) =>
      cid === "c2" ? ch2 : ch1,
    );

    const origClientW = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "clientWidth");
    const origScrollW = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "scrollWidth");
    Object.defineProperty(HTMLElement.prototype, "clientWidth", {
      configurable: true,
      get() {
        return 720;
      },
    });
    Object.defineProperty(HTMLElement.prototype, "scrollWidth", {
      configurable: true,
      get() {
        // CSS multi-column width for 2 pages at colW=720, gap=64 is 2*720 + 64 = 1504
        return this.textContent?.includes("Chapter One") ? 1504 : 720;
      },
    });

    try {
      render(
        <ReaderPage
          novelId="n1"
          initialChapterId="c2"
          settings={{ ...BASE_SETTINGS, reader_mode: "paginated", reader_width: 720 }}
          onUpdateSettings={vi.fn()}
          onExit={vi.fn()}
          onNotify={vi.fn()}
        />,
      );

      expect(await screen.findByText(/Page 1 of 1 · Chapter Two/)).toBeInTheDocument();

      fireEvent.click(screen.getByRole("button", { name: "Previous page" }));

      expect(await screen.findByText(/Page 2 of 2 · Chapter One/)).toBeInTheDocument();
      expect(mockApi.saveReadingProgress).toHaveBeenLastCalledWith("n1", "c1", 1, 0, 1, 2);
    } finally {
      if (origClientW) {
        Object.defineProperty(HTMLElement.prototype, "clientWidth", origClientW);
      } else {
        delete (HTMLElement.prototype as { clientWidth?: unknown }).clientWidth;
      }
      if (origScrollW) {
        Object.defineProperty(HTMLElement.prototype, "scrollWidth", origScrollW);
      } else {
        delete (HTMLElement.prototype as { scrollWidth?: unknown }).scrollWidth;
      }
    }
  });

  it("flushes debounced scroll progress immediately on unmount", async () => {
    const details = mkDetails();
    const ch1: ChapterContent = {
      chapter_id: "c1",
      novel_id: "n1",
      chapter_number: 1,
      title: "Chapter One",
      html: "<p>Scroll paragraph one.</p>",
      paragraphs: ["Scroll paragraph one."],
      word_count: 3,
      is_downloaded: true,
      prev_chapter_id: null,
      next_chapter_id: "c2",
    };
    mockApi.getNovelDetails.mockResolvedValue(details);
    mockApi.getChapterContent.mockResolvedValue(ch1);

    const { container, unmount } = render(
      <ReaderPage
        novelId="n1"
        initialChapterId="c1"
        settings={BASE_SETTINGS}
        onUpdateSettings={vi.fn()}
        onExit={vi.fn()}
        onNotify={vi.fn()}
      />,
    );

    expect(await screen.findByText("Scroll paragraph one.")).toBeInTheDocument();

    const scroller = container.querySelector(".overflow-y-auto");
    expect(scroller).not.toBeNull();
    fireEvent.scroll(scroller!);

    expect(mockApi.saveReadingProgress).not.toHaveBeenCalled();
    unmount();
    expect(mockApi.saveReadingProgress).toHaveBeenCalledTimes(1);
    expect(mockApi.saveReadingProgress).toHaveBeenCalledWith("n1", "c1", expect.any(Number), 0, 0, 1);
  });

  it("marks all ReaderPage HUD header buttons with data-tauri-drag-region='false'", async () => {
    const details = mkDetails();
    const ch1: ChapterContent = {
      chapter_id: "c1",
      novel_id: "n1",
      chapter_number: 1,
      title: "Chapter One",
      html: "<p>HUD button check.</p>",
      paragraphs: ["HUD button check."],
      word_count: 3,
      is_downloaded: true,
      prev_chapter_id: null,
      next_chapter_id: null,
    };
    mockApi.getNovelDetails.mockResolvedValue(details);
    mockApi.getChapterContent.mockResolvedValue(ch1);

    const { container } = render(
      <ReaderPage
        novelId="n1"
        initialChapterId="c1"
        settings={BASE_SETTINGS}
        onUpdateSettings={vi.fn()}
        onExit={vi.fn()}
        onNotify={vi.fn()}
      />,
    );

    expect(await screen.findByText("HUD button check.")).toBeInTheDocument();
    const header = container.querySelector("header[data-tauri-drag-region='deep']");
    expect(header).not.toBeNull();
    const buttons = Array.from(header!.querySelectorAll("button"));
    expect(buttons.length).toBeGreaterThanOrEqual(6);
    for (const btn of buttons) {
      expect(btn.getAttribute("data-tauri-drag-region")).toBe("false");
    }
  });

  it("restores saved scroll offset instantly without scroll-smooth or progress overwrite", async () => {
    const details: NovelDetailsResponse = {
      ...mkDetails(),
      reading_progress: {
        novel_id: "n1",
        chapter_id: "c1",
        chapter_number: 1,
        chapter_title: "Chapter One",
        scroll_offset: 0.45,
        paragraph_index: 0,
        page_index: 0,
        total_pages: 1,
        progress_percent: 45,
        updated_at: "2026-01-01T00:00:00Z",
      },
    };
    const ch1: ChapterContent = {
      chapter_id: "c1",
      novel_id: "n1",
      chapter_number: 1,
      title: "Chapter One",
      html: "<p>Restored paragraph.</p>",
      paragraphs: ["Restored paragraph."],
      word_count: 2,
      is_downloaded: true,
      prev_chapter_id: null,
      next_chapter_id: null,
    };

    mockApi.getNovelDetails.mockResolvedValue(details);
    mockApi.getChapterContent.mockResolvedValue(ch1);

    const origScrollH = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "scrollHeight");
    const origClientH = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "clientHeight");
    Object.defineProperty(HTMLElement.prototype, "scrollHeight", {
      configurable: true,
      get() {
        return 2000;
      },
    });
    Object.defineProperty(HTMLElement.prototype, "clientHeight", {
      configurable: true,
      get() {
        return 1000;
      },
    });

    try {
      const { container } = render(
        <ReaderPage
          novelId="n1"
          initialChapterId="c1"
          settings={BASE_SETTINGS}
          onUpdateSettings={vi.fn()}
          onExit={vi.fn()}
          onNotify={vi.fn()}
        />,
      );

      expect(await screen.findByText("Restored paragraph.")).toBeInTheDocument();
      const scroller = container.querySelector(".overflow-y-auto") as HTMLDivElement;
      expect(scroller).not.toBeNull();
      expect(scroller.className).not.toContain("scroll-smooth");

      await waitFor(() => {
        expect(scroller.scrollTop).toBe(450);
      });
      // Guarded programmatic restoration must not overwrite saved progress via handleScroll
      fireEvent.scroll(scroller);
      expect(mockApi.saveReadingProgress).not.toHaveBeenCalled();
    } finally {
      if (origScrollH) {
        Object.defineProperty(HTMLElement.prototype, "scrollHeight", origScrollH);
      } else {
        delete (HTMLElement.prototype as { scrollHeight?: unknown }).scrollHeight;
      }
      if (origClientH) {
        Object.defineProperty(HTMLElement.prototype, "clientHeight", origClientH);
      } else {
        delete (HTMLElement.prototype as { clientHeight?: unknown }).clientHeight;
      }
    }
  });

  it("does not add a phantom blank page when paginated scrollWidth includes trailing column gap", async () => {
    const details = mkDetails();
    const ch1: ChapterContent = {
      chapter_id: "c1",
      novel_id: "n1",
      chapter_number: 1,
      title: "Chapter One",
      html: "<p>Two columns with trailing gap.</p>",
      paragraphs: ["Two columns with trailing gap."],
      word_count: 5,
      is_downloaded: true,
      prev_chapter_id: null,
      next_chapter_id: null,
    };

    mockApi.getNovelDetails.mockResolvedValue(details);
    mockApi.getChapterContent.mockResolvedValue(ch1);

    const origClientW = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "clientWidth");
    const origScrollW = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "scrollWidth");
    Object.defineProperty(HTMLElement.prototype, "clientWidth", {
      configurable: true,
      get() {
        return 720;
      },
    });
    Object.defineProperty(HTMLElement.prototype, "scrollWidth", {
      configurable: true,
      get() {
        // Trailing gap included: 2 * (720 + 64) = 1568
        return 1568;
      },
    });

    try {
      render(
        <ReaderPage
          novelId="n1"
          initialChapterId="c1"
          settings={{ ...BASE_SETTINGS, reader_mode: "paginated", reader_width: 720 }}
          onUpdateSettings={vi.fn()}
          onExit={vi.fn()}
          onNotify={vi.fn()}
        />,
      );

      expect(await screen.findByText(/Page 1 of 2 · Chapter One/)).toBeInTheDocument();
    } finally {
      if (origClientW) {
        Object.defineProperty(HTMLElement.prototype, "clientWidth", origClientW);
      } else {
        delete (HTMLElement.prototype as { clientWidth?: unknown }).clientWidth;
      }
      if (origScrollW) {
        Object.defineProperty(HTMLElement.prototype, "scrollWidth", origScrollW);
      } else {
        delete (HTMLElement.prototype as { scrollWidth?: unknown }).scrollWidth;
      }
    }
  });
});

describe("NovelDetailsPage bookmark deletion and download progress updates", () => {
  const mkDetails = (downloadedCh2: boolean, isBookmarked: boolean): NovelDetailsResponse => ({
    novel: {
      id: "n1",
      source_id: "gutenberg",
      external_id: "1342",
      title: "Pride and Prejudice",
      author_id: null,
      author: "Jane Austen",
      description: "Classic novel.",
      cover_url: null,
      local_cover_path: null,
      status: "completed",
      genres: ["Classic"],
      tags: [],
      rating: null,
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
        external_id: "1",
        chapter_number: 1,
        volume: null,
        title: "Chapter 1",
        url: null,
        word_count: 100,
        published_at: null,
        is_downloaded: true,
        local_path: "/tmp/c1.json",
        size_bytes: 512,
      },
      {
        id: "c2",
        novel_id: "n1",
        external_id: "2",
        chapter_number: 2,
        volume: null,
        title: "Chapter 2",
        url: null,
        word_count: 100,
        published_at: null,
        is_downloaded: downloadedCh2,
        local_path: downloadedCh2 ? "/tmp/c2.json" : null,
        size_bytes: downloadedCh2 ? 512 : 0,
      },
    ],
    library_entry: {
      novel_id: "n1",
      status: "reading",
      is_bookmarked: isBookmarked,
      follow_updates: false,
      last_checked_chapters: 2,
      added_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
    },
    reading_progress: null,
    bookmarks: [
      {
        id: "bm-novel-1",
        novel_id: "n1",
        novel_title: "Pride and Prejudice",
        author: "Jane Austen",
        cover_url: null,
        local_cover_path: null,
        chapter_id: null,
        chapter_number: null,
        chapter_title: null,
        paragraph_index: 0,
        excerpt: null,
        note: null,
        color: "amber",
        is_novel_bookmark: true,
        created_at: "2026-01-01T00:00:00Z",
      },
      {
        id: "bm-para-1",
        novel_id: "n1",
        novel_title: "Pride and Prejudice",
        author: "Jane Austen",
        cover_url: null,
        local_cover_path: null,
        chapter_id: "c1",
        chapter_number: 1,
        chapter_title: "Chapter 1",
        paragraph_index: 3,
        excerpt: "It is a truth...",
        note: null,
        color: "amber",
        is_novel_bookmark: false,
        created_at: "2026-01-01T00:00:00Z",
      },
    ],
    highlights: [],
    translations: [],
    collection_ids: [],
    downloaded_chapters: downloadedCh2 ? 2 : 1,
    total_size_bytes: downloadedCh2 ? 1024 : 512,
  });

  it("deletes novel-level bookmark rows when un-bookmarking and refreshes chapter badges on download-updated", async () => {
    let downloadCb: ((task: DownloadTask) => void) | null = null;
    mockApi.onDownloadUpdated.mockImplementation(async (cb: (task: DownloadTask) => void) => {
      downloadCb = cb;
      return () => {};
    });
    mockApi.listCollections.mockResolvedValue([]);
    mockApi.updateLibraryEntry.mockResolvedValue({
      novel_id: "n1",
      status: "reading",
      is_bookmarked: false,
      follow_updates: false,
      last_checked_chapters: 2,
      added_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
    });
    mockApi.deleteBookmark.mockResolvedValue(undefined);
    mockApi.getNovelDetails
      .mockResolvedValueOnce(mkDetails(false, true))
      .mockResolvedValueOnce(mkDetails(false, false))
      .mockResolvedValueOnce(mkDetails(true, false));

    render(
      <NovelDetailsPage
        novelId="n1"
        onBack={vi.fn()}
        onStartReading={vi.fn()}
        onSearchAuthor={vi.fn()}
        onNotify={vi.fn()}
      />,
    );

    expect(await screen.findByText(/1\s*\/\s*2 offline/i)).toBeInTheDocument();
    expect(screen.getAllByTitle(/Downloaded \(/i)).toHaveLength(1);

    fireEvent.click(screen.getByRole("button", { name: "Bookmarked" }));
    await waitFor(() => {
      expect(mockApi.deleteBookmark).toHaveBeenCalledTimes(1);
      expect(mockApi.deleteBookmark).toHaveBeenCalledWith("bm-novel-1");
    });

    expect(downloadCb).not.toBeNull();
    act(() => {
      downloadCb!({
        id: "dl-1",
        novel_id: "n1",
        novel_title: "Pride and Prejudice",
        cover_url: null,
        local_cover_path: null,
        source_id: "gutenberg",
        chapter_ids: ["c2"],
        total_chapters: 1,
        completed_chapters: 1,
        failed_chapters: 0,
        bytes_downloaded: 512,
        speed_bytes_per_sec: 0,
        status: "completed",
        error_message: null,
        created_at: "2026-01-01T00:00:00Z",
        updated_at: "2026-01-01T00:00:00Z",
      });
    });

    expect(await screen.findByText(/2\s*\/\s*2 offline/i)).toBeInTheDocument();
    expect(screen.getAllByTitle(/Downloaded \(/i)).toHaveLength(2);
  });
});

describe("SettingsPage Reader defaults", () => {
  it("exposes reader text alignment and custom reader color pickers", async () => {
    const onUpdateSettings = vi.fn();

    render(
      <SettingsPage
        settings={{ ...BASE_SETTINGS, reader_theme: "custom" }}
        onUpdateSettings={onUpdateSettings}
        onNotify={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Reader" }));

    const alignSelect = await screen.findByLabelText("Text Alignment");
    fireEvent.change(alignSelect, { target: { value: "justify" } });
    expect(onUpdateSettings).toHaveBeenCalledWith(
      expect.objectContaining({ reader_text_align: "justify" }),
    );

    fireEvent.change(screen.getByLabelText("Page Background Color"), {
      target: { value: "#112233" },
    });
    expect(onUpdateSettings).toHaveBeenCalledWith(
      expect.objectContaining({ reader_theme: "custom", reader_custom_bg: "#112233" }),
    );

    fireEvent.change(screen.getByLabelText("Font Color"), {
      target: { value: "#eeeeee" },
    });
    expect(onUpdateSettings).toHaveBeenCalledWith(
      expect.objectContaining({ reader_theme: "custom", reader_custom_fg: "#eeeeee" }),
    );

    fireEvent.change(screen.getByLabelText("Custom Accent Color"), {
      target: { value: "#ff8800" },
    });
    expect(onUpdateSettings).toHaveBeenCalledWith(
      expect.objectContaining({ reader_custom_accent: "#ff8800" }),
    );
  });
});


describe("Regression guards for NovelDetailsPage and ReaderPage fixes", () => {
  const mkTestDetails = (overrides?: Partial<NovelDetailsResponse>): NovelDetailsResponse => ({
    novel: {
      id: "n1",
      source_id: "gutenberg",
      external_id: "1342",
      title: "Pride and Prejudice",
      author_id: null,
      author: "Jane Austen",
      description: "A classic novel.",
      cover_url: null,
      local_cover_path: null,
      status: "completed",
      genres: ["Classic"],
      tags: [],
      rating: null,
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
        external_id: "1",
        chapter_number: 1,
        volume: null,
        title: "Chapter One",
        url: null,
        word_count: 10,
        published_at: null,
        is_downloaded: true,
        local_path: null,
        size_bytes: 120,
      },
      {
        id: "c2",
        novel_id: "n1",
        external_id: "2",
        chapter_number: 2,
        volume: null,
        title: "Chapter Two",
        url: null,
        word_count: 10,
        published_at: null,
        is_downloaded: true,
        local_path: null,
        size_bytes: 120,
      },
    ],
    library_entry: {
      novel_id: "n1",
      status: "reading",
      is_bookmarked: true,
      follow_updates: false,
      last_checked_chapters: 2,
      added_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
    },
    reading_progress: null,
    bookmarks: [],
    highlights: [],
    translations: [],
    collection_ids: [],
    downloaded_chapters: 2,
    total_size_bytes: 240,
    ...overrides,
  });

  it("queries fresh bookmarks via listBookmarks when un-bookmarking despite stale details.bookmarks", async () => {
    const initial = mkTestDetails({ bookmarks: [] });
    mockApi.getNovelDetails.mockResolvedValue(initial);
    mockApi.listCollections.mockResolvedValue([]);
    mockApi.listBookmarks.mockResolvedValue([
      {
        id: "bm-stale-novel",
        novel_id: "n1",
        chapter_id: null,
        paragraph_index: null,
        excerpt: null,
        note: null,
        is_novel_bookmark: true,
        created_at: "2026-01-01T00:00:00Z",
      },
      {
        id: "bm-passage",
        novel_id: "n1",
        chapter_id: "c1",
        paragraph_index: 0,
        excerpt: "Quote",
        note: null,
        is_novel_bookmark: false,
        created_at: "2026-01-01T00:00:00Z",
      },
    ]);
    mockApi.updateLibraryEntry.mockResolvedValue({
      ...initial.library_entry!,
      is_bookmarked: false,
    });
    mockApi.deleteBookmark.mockResolvedValue(undefined);

    render(
      <NovelDetailsPage
        novelId="n1"
        onBack={vi.fn()}
        onStartReading={vi.fn()}
        onSearchAuthor={vi.fn()}
        onNotify={vi.fn()}
      />,
    );

    const bmBtn = await screen.findByRole("button", { name: /Bookmarked/i });
    fireEvent.click(bmBtn);

    await waitFor(() => {
      expect(mockApi.listBookmarks).toHaveBeenCalledWith("n1");
      expect(mockApi.deleteBookmark).toHaveBeenCalledWith("bm-stale-novel");
    });
    expect(mockApi.deleteBookmark).not.toHaveBeenCalledWith("bm-passage");
  });

  it("does not let silent download-updated refreshes clear refreshing while remote refresh is pending", async () => {
    const initial = mkTestDetails();
    let downloadCb: ((task: DownloadTask) => void) | null = null;
    mockApi.onDownloadUpdated.mockImplementation(async (cb: (task: DownloadTask) => void) => {
      downloadCb = cb;
      return () => {};
    });
    mockApi.listCollections.mockResolvedValue([]);

    let resolveRemote!: (val: NovelDetailsResponse) => void;
    mockApi.getNovelDetails.mockImplementation(async (_id: string, refreshRemote?: boolean) => {
      if (refreshRemote) {
        return new Promise<NovelDetailsResponse>((res) => {
          resolveRemote = res;
        });
      }
      return initial;
    });

    render(
      <NovelDetailsPage
        novelId="n1"
        onBack={vi.fn()}
        onStartReading={vi.fn()}
        onSearchAuthor={vi.fn()}
        onNotify={vi.fn()}
      />,
    );

    const refreshBtn = await screen.findByRole("button", {
      name: /Refresh Metadata & Chapters/i,
    });
    fireEvent.click(refreshBtn);

    await waitFor(() => expect(refreshBtn).toBeDisabled());
    expect(downloadCb).not.toBeNull();

    await act(async () => {
      downloadCb?.({
        id: "dl-1",
        novel_id: "n1",
        novel_title: "Pride and Prejudice",
        cover_url: null,
        local_cover_path: null,
        source_id: "gutenberg",
        chapter_ids: ["c1", "c2"],
        total_chapters: 2,
        completed_chapters: 2,
        failed_chapters: 0,
        bytes_downloaded: 240,
        speed_bytes_per_sec: 120,
        status: "completed",
        error_message: null,
        created_at: "2026-01-01T00:00:00Z",
        updated_at: "2026-01-01T00:00:00Z",
      });
    });

    await waitFor(() => expect(mockApi.getNovelDetails).toHaveBeenCalledTimes(3));
    expect(refreshBtn).toBeDisabled();

    await act(async () => {
      resolveRemote(initial);
    });
    await waitFor(() => expect(refreshBtn).not.toBeDisabled());
  });

  it("navigates to the target column page when jumping to a bookmark in paginated mode", async () => {
    const details = mkTestDetails();
    const ch1: ChapterContent = {
      chapter_id: "c1",
      novel_id: "n1",
      chapter_number: 1,
      title: "Chapter One",
      html: "<p>Page one lead.</p><p>Page two target.</p>",
      paragraphs: ["Page one lead.", "Page two target."],
      word_count: 6,
      is_downloaded: true,
      prev_chapter_id: null,
      next_chapter_id: "c2",
    };
    mockApi.getNovelDetails.mockResolvedValue(details);
    mockApi.getChapterContent.mockResolvedValue(ch1);

    const origClientW = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "clientWidth");
    const origScrollW = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "scrollWidth");
    const origGetRect = Element.prototype.getBoundingClientRect;
    const scrollIntoViewSpy = vi.fn();
    const origScrollIntoView = Element.prototype.scrollIntoView;
    Element.prototype.scrollIntoView = scrollIntoViewSpy;

    Object.defineProperty(HTMLElement.prototype, "clientWidth", {
      configurable: true,
      get() {
        return 720;
      },
    });
    Object.defineProperty(HTMLElement.prototype, "scrollWidth", {
      configurable: true,
      get() {
        return this.textContent?.includes("Page two target.") ? 1504 : 720;
      },
    });
    Element.prototype.getBoundingClientRect = function () {
      const pAttr = this.getAttribute("data-p");
      if (pAttr === "1") {
        return {
          left: 784,
          top: 0,
          right: 1504,
          bottom: 40,
          width: 720,
          height: 40,
          x: 784,
          y: 0,
          toJSON: () => ({}),
        } as DOMRect;
      }
      return {
        left: 0,
        top: 0,
        right: 720,
        bottom: 600,
        width: 720,
        height: 600,
        x: 0,
        y: 0,
        toJSON: () => ({}),
      } as DOMRect;
    };

    try {
      render(
        <ReaderPage
          novelId="n1"
          initialChapterId="c1"
          initialParagraphIndex={1}
          settings={{ ...BASE_SETTINGS, reader_mode: "paginated", reader_width: 720 }}
          onUpdateSettings={vi.fn()}
          onExit={vi.fn()}
          onNotify={vi.fn()}
        />,
      );

      expect(await screen.findByText(/Page 2 of 2 · Chapter One/)).toBeInTheDocument();
      expect(scrollIntoViewSpy).not.toHaveBeenCalled();
    } finally {
      Element.prototype.getBoundingClientRect = origGetRect;
      Element.prototype.scrollIntoView = origScrollIntoView;
      if (origClientW) {
        Object.defineProperty(HTMLElement.prototype, "clientWidth", origClientW);
      } else {
        delete (HTMLElement.prototype as { clientWidth?: unknown }).clientWidth;
      }
      if (origScrollW) {
        Object.defineProperty(HTMLElement.prototype, "scrollWidth", origScrollW);
      } else {
        delete (HTMLElement.prototype as { scrollWidth?: unknown }).scrollWidth;
      }
    }
  });

  it("renders jumped chapter immediately even while saveReadingProgress is still pending", async () => {
    const details = mkTestDetails();
    const ch2: ChapterContent = {
      chapter_id: "c2",
      novel_id: "n1",
      chapter_number: 2,
      title: "Chapter Two",
      html: "<p>Second chapter text.</p>",
      paragraphs: ["Second chapter text."],
      word_count: 3,
      is_downloaded: true,
      prev_chapter_id: "c1",
      next_chapter_id: null,
    };
    const ch1: ChapterContent = {
      chapter_id: "c1",
      novel_id: "n1",
      chapter_number: 1,
      title: "Chapter One",
      html: "<p>First chapter spanning two columns.</p>",
      paragraphs: ["First chapter spanning two columns."],
      word_count: 5,
      is_downloaded: true,
      prev_chapter_id: null,
      next_chapter_id: "c2",
    };

    mockApi.getNovelDetails.mockResolvedValue(details);
    mockApi.getChapterContent.mockImplementation(async (_nid: string, cid: string) =>
      cid === "c2" ? ch2 : ch1,
    );

    let resolveSave!: () => void;
    mockApi.saveReadingProgress.mockImplementation(
      () =>
        new Promise((res) => {
          resolveSave = () =>
            res({
              novel_id: "n1",
              chapter_id: "c1",
              chapter_number: 1,
              chapter_title: "Chapter One",
              scroll_offset: 1,
              paragraph_index: 0,
              page_index: 1,
              total_pages: 2,
              progress_percent: 50,
              updated_at: "2026-01-01T00:00:00Z",
            });
        }),
    );

    const origClientW = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "clientWidth");
    const origScrollW = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "scrollWidth");
    Object.defineProperty(HTMLElement.prototype, "clientWidth", {
      configurable: true,
      get() {
        return 720;
      },
    });
    Object.defineProperty(HTMLElement.prototype, "scrollWidth", {
      configurable: true,
      get() {
        return this.textContent?.includes("Chapter One") ? 1504 : 720;
      },
    });

    try {
      render(
        <ReaderPage
          novelId="n1"
          initialChapterId="c2"
          settings={{ ...BASE_SETTINGS, reader_mode: "paginated", reader_width: 720 }}
          onUpdateSettings={vi.fn()}
          onExit={vi.fn()}
          onNotify={vi.fn()}
        />,
      );

      expect(await screen.findByText(/Page 1 of 1 · Chapter Two/)).toBeInTheDocument();
      fireEvent.click(screen.getByRole("button", { name: "Previous page" }));

      expect(await screen.findByText(/Page 2 of 2 · Chapter One/)).toBeInTheDocument();
      resolveSave?.();
    } finally {
      if (origClientW) {
        Object.defineProperty(HTMLElement.prototype, "clientWidth", origClientW);
      } else {
        delete (HTMLElement.prototype as { clientWidth?: unknown }).clientWidth;
      }
      if (origScrollW) {
        Object.defineProperty(HTMLElement.prototype, "scrollWidth", origScrollW);
      } else {
        delete (HTMLElement.prototype as { scrollWidth?: unknown }).scrollWidth;
      }
    }
  });
});

describe("New features: seamless scroll, flip toggle, colored annotations, and translations", () => {
  const mkFeatureDetails = (overrides?: Partial<NovelDetailsResponse>): NovelDetailsResponse => ({
    novel: {
      id: "wikisource:Pride_and_Prejudice",
      source_id: "wikisource",
      external_id: "Pride_and_Prejudice",
      title: "Pride and Prejudice",
      author_id: null,
      author: "Jane Austen",
      description: "Classic novel.",
      cover_url: null,
      local_cover_path: null,
      status: "completed",
      genres: ["Classic"],
      tags: [],
      rating: null,
      popularity: 100,
      chapter_count: 2,
      last_updated_remote: null,
      metadata: { language: "en" },
      is_local: false,
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
    },
    chapters: [
      {
        id: "c1",
        novel_id: "wikisource:Pride_and_Prejudice",
        external_id: "1",
        chapter_number: 1,
        volume: null,
        title: "Chapter One",
        url: null,
        word_count: 20,
        published_at: null,
        is_downloaded: false,
        local_path: null,
        size_bytes: 0,
      },
      {
        id: "c2",
        novel_id: "wikisource:Pride_and_Prejudice",
        external_id: "2",
        chapter_number: 2,
        volume: null,
        title: "Chapter Two",
        url: null,
        word_count: 20,
        published_at: null,
        is_downloaded: false,
        local_path: null,
        size_bytes: 0,
      },
    ],
    library_entry: null,
    reading_progress: null,
    bookmarks: [],
    highlights: [],
    translations: [
      {
        language_code: "en",
        language_label: "English",
        novel_id: "wikisource:Pride_and_Prejudice",
        source_id: "wikisource",
        external_id: "Pride_and_Prejudice",
        title: "Pride and Prejudice",
      },
      {
        language_code: "pl",
        language_label: "Polski",
        novel_id: "wikisource:pl|Duma_i_uprzedzenie",
        source_id: "wikisource",
        external_id: "pl|Duma_i_uprzedzenie",
        title: "Duma i uprzedzenie",
      },
    ],
    collection_ids: [],
    downloaded_chapters: 0,
    total_size_bytes: 0,
    ...overrides,
  });

  it("keeps scroll position when advancing into auto-loaded Chapter 2 without snapping back to 0", async () => {
    const det = mkFeatureDetails();
    const ch1: ChapterContent = {
      chapter_id: "c1",
      novel_id: det.novel.id,
      chapter_number: 1,
      title: "Chapter One",
      html: "<p>First chapter prose.</p>",
      paragraphs: ["First chapter prose."],
      word_count: 3,
      is_downloaded: false,
      prev_chapter_id: null,
      next_chapter_id: "c2",
    };
    const ch2: ChapterContent = {
      chapter_id: "c2",
      novel_id: det.novel.id,
      chapter_number: 2,
      title: "Chapter Two",
      html: "<p>Second chapter prose.</p>",
      paragraphs: ["Second chapter prose."],
      word_count: 3,
      is_downloaded: false,
      prev_chapter_id: "c1",
      next_chapter_id: null,
    };
    mockApi.getNovelDetails.mockResolvedValue(det);
    mockApi.getChapterContent.mockImplementation(async (_nid: string, cid: string) =>
      cid === "c2" ? ch2 : ch1,
    );

    const { container } = render(
      <ReaderPage
        novelId={det.novel.id}
        initialChapterId="c1"
        settings={{ ...BASE_SETTINGS, reader_mode: "scroll", reader_auto_load_next: true }}
        onUpdateSettings={vi.fn()}
        onExit={vi.fn()}
        onNotify={vi.fn()}
      />,
    );

    await screen.findByText("First chapter prose.");
    const scrollBox = container.querySelector(".overflow-y-auto") as HTMLElement;
    expect(scrollBox).toBeTruthy();

    let fakeScrollTop = 900;
    Object.defineProperty(scrollBox, "scrollHeight", { configurable: true, get: () => 2000 });
    Object.defineProperty(scrollBox, "clientHeight", { configurable: true, get: () => 1000 });
    Object.defineProperty(scrollBox, "scrollTop", {
      configurable: true,
      get: () => fakeScrollTop,
      set: (v: number) => {
        fakeScrollTop = v;
      },
    });
    vi.spyOn(scrollBox, "getBoundingClientRect").mockReturnValue({
      top: 0,
      bottom: 1000,
      left: 0,
      right: 800,
      width: 800,
      height: 1000,
      x: 0,
      y: 0,
      toJSON: () => ({}),
    });

    // Scroll to 90% -> triggers Ch 2 append
    await act(async () => {
      fireEvent.scroll(scrollBox);
    });
    expect(await screen.findByText("Second chapter prose.")).toBeInTheDocument();

    // Now simulate Chapter 2 entering viewport top
    const sec1 = container.querySelector('[data-chapter-section="c1"]') as HTMLElement;
    const sec2 = container.querySelector('[data-chapter-section="c2"]') as HTMLElement;
    vi.spyOn(sec1, "getBoundingClientRect").mockReturnValue({
      top: -900,
      bottom: 20,
      left: 0,
      right: 800,
      width: 800,
      height: 920,
      x: 0,
      y: -900,
      toJSON: () => ({}),
    });
    vi.spyOn(sec2, "getBoundingClientRect").mockReturnValue({
      top: 40,
      bottom: 900,
      left: 0,
      right: 800,
      width: 800,
      height: 860,
      x: 0,
      y: 40,
      toJSON: () => ({}),
    });
    fakeScrollTop = 960;

    await act(async () => {
      fireEvent.scroll(scrollBox);
    });

    await waitFor(() => {
      expect(mockApi.saveReadingProgress).toHaveBeenCalledWith(
        det.novel.id,
        "c2",
        expect.any(Number),
        expect.any(Number),
        0,
        1,
      );
    });
    // Must stay at 960, never reset back to 0
    expect(fakeScrollTop).toBe(960);
  });

  it("renders saved inline highlights and saves colored bookmarks with flip animation toggle", async () => {
    const det = mkFeatureDetails({
      highlights: [
        {
          id: "hl-1",
          novel_id: "wikisource:Pride_and_Prejudice",
          chapter_id: "c1",
          paragraph_index: 0,
          start_offset: 6,
          end_offset: 13,
          text: "chapter",
          color: "emerald",
          note: null,
          created_at: "2026-01-01T00:00:00Z",
        },
      ],
    });
    mockApi.getNovelDetails.mockResolvedValue(det);
    mockApi.getChapterContent.mockResolvedValue({
      chapter_id: "c1",
      novel_id: det.novel.id,
      chapter_number: 1,
      title: "Chapter One",
      html: "<p>First chapter prose.</p>",
      paragraphs: ["First chapter prose."],
      word_count: 3,
      is_downloaded: false,
      prev_chapter_id: null,
      next_chapter_id: null,
    });
    mockApi.addBookmark.mockResolvedValue({
      id: "bm-rose",
      novel_id: det.novel.id,
      novel_title: det.novel.title,
      author: det.novel.author,
      cover_url: null,
      local_cover_path: null,
      chapter_id: "c1",
      chapter_number: 1,
      chapter_title: "Chapter One",
      paragraph_index: 0,
      excerpt: "First chapter prose.",
      note: null,
      color: "rose",
      is_novel_bookmark: false,
      created_at: "2026-01-01T00:00:00Z",
    });

    const onUpdateSettings = vi.fn();
    render(
      <ReaderPage
        novelId={det.novel.id}
        initialChapterId="c1"
        settings={{ ...BASE_SETTINGS, reader_mode: "paginated", reader_flip_animation: true }}
        onUpdateSettings={onUpdateSettings}
        onExit={vi.fn()}
        onNotify={vi.fn()}
      />,
    );

    const mark = await screen.findByText("chapter");
    expect(mark.tagName).toBe("MARK");
    fireEvent.click(mark);
    await waitFor(() => {
      expect(mockApi.deleteHighlight).toHaveBeenCalledWith("hl-1");
    });

    fireEvent.click(screen.getByTitle("Typography & Theme (A)"));
    fireEvent.click(screen.getByLabelText("Page Flip Animation"));
    expect(onUpdateSettings).toHaveBeenCalledWith(
      expect.objectContaining({ reader_flip_animation: false }),
    );

    fireEvent.click(screen.getByTitle("Bookmark Position (B)"));
    fireEvent.click(screen.getByLabelText("Bookmark color Rose"));
    fireEvent.click(screen.getByRole("button", { name: "Save Bookmark" }));
    await waitFor(() => {
      expect(mockApi.addBookmark).toHaveBeenCalledWith(
        expect.objectContaining({ color: "rose" }),
      );
    });
  });

  it("shows translation selector menu and switches to default_language when available", async () => {
    const onSelectNovel = vi.fn();
    mockApi.listCollections.mockResolvedValue([]);
    mockApi.getNovelDetails.mockResolvedValue(mkFeatureDetails());

    render(
      <NovelDetailsPage
        novelId="wikisource:Pride_and_Prejudice"
        defaultLanguage="pl"
        onBack={vi.fn()}
        onSelectNovel={onSelectNovel}
        onStartReading={vi.fn()}
        onSearchAuthor={vi.fn()}
        onNotify={vi.fn()}
      />,
    );

    const langSelect = await screen.findByLabelText("Translation language");
    expect(langSelect).toBeInTheDocument();
    await waitFor(() => {
      expect(onSelectNovel).toHaveBeenCalledWith("wikisource:pl|Duma_i_uprzedzenie");
    });
  });

  it("filters novels by language in Discover and Library", async () => {
    mockApi.listSources.mockResolvedValue([]);
    mockApi.discoverNovels.mockResolvedValue({
      items: [
        {
          id: "gutenberg:10",
          source_id: "gutenberg",
          external_id: "10",
          title: "Pan Tadeusz",
          author: "Adam Mickiewicz",
          description: "Polish epic",
          cover_url: null,
          local_cover_path: null,
          status: "completed",
          genres: ["Poetry"],
          rating: null,
          popularity: 100,
          chapter_count: null,
          last_updated: null,
          language: "pl",
        },
      ],
      page: 1,
      has_more: false,
      total_estimate: 1,
      source_errors: [],
    });

    const { unmount } = render(<DiscoverPage onOpenNovel={vi.fn()} />);
    const discoverLang = await screen.findByLabelText("Filter language");
    fireEvent.change(discoverLang, { target: { value: "pl" } });
    await waitFor(() => {
      expect(mockApi.discoverNovels).toHaveBeenLastCalledWith(
        expect.objectContaining({ language: "pl" }),
      );
    });
    expect(screen.getAllByText("Pan Tadeusz").length).toBeGreaterThan(0);
    unmount();

    mockApi.listCollections.mockResolvedValue([]);
    mockApi.listLibrary.mockResolvedValue([
      {
        novel: {
          ...mkFeatureDetails().novel,
          id: "gutenberg:en1",
          title: "English Tale",
          metadata: { language: "en" },
        },
        entry: {
          novel_id: "gutenberg:en1",
          status: "reading",
          is_bookmarked: false,
          follow_updates: false,
          last_checked_chapters: 0,
          added_at: "2026-05-01T00:00:00Z",
          updated_at: "2026-05-01T00:00:00Z",
        },
        progress: null,
        latest_chapter: null,
        downloaded_chapters: 0,
        total_size_bytes: 0,
        collection_ids: [],
      },
      {
        novel: {
          ...mkFeatureDetails().novel,
          id: "gutenberg:pl1",
          title: "Polish Chronicle",
          metadata: { language: "pl" },
        },
        entry: {
          novel_id: "gutenberg:pl1",
          status: "reading",
          is_bookmarked: false,
          follow_updates: false,
          last_checked_chapters: 0,
          added_at: "2026-05-01T00:00:00Z",
          updated_at: "2026-05-01T00:00:00Z",
        },
        progress: null,
        latest_chapter: null,
        downloaded_chapters: 0,
        total_size_bytes: 0,
        collection_ids: [],
      },
    ]);

    render(
      <LibraryPage
        onOpenNovel={vi.fn()}
        onStartReading={vi.fn()}
        onNotify={vi.fn()}
      />,
    );
    expect((await screen.findAllByText("English Tale")).length).toBeGreaterThan(0);
    expect(screen.getAllByText("Polish Chronicle").length).toBeGreaterThan(0);

    fireEvent.change(screen.getByLabelText("Filter library by language"), {
      target: { value: "pl" },
    });
    expect(screen.queryByText("English Tale")).not.toBeInTheDocument();
    expect(screen.getAllByText("Polish Chronicle").length).toBeGreaterThan(0);
  });

  it("opens Books section scoped to legal open-access catalogs and renders authors", async () => {
    mockApi.listSources.mockResolvedValue([
      {
        id: "gutenberg",
        name: "Project Gutenberg",
        kind: "builtin",
        base_url: "https://www.gutenberg.org",
        description: "Public domain books",
        enabled: true,
        priority: 1,
        health_status: "online",
        last_checked_at: null,
        capabilities: {
          search: true,
          browse_popular: true,
          browse_trending: true,
          browse_latest: true,
          browse_genre: true,
          filter_status: true,
          genres: ["Fiction"],
        },
        config: {},
        config_schema: [],
        is_custom: false,
      },
      {
        id: "standardebooks",
        name: "Standard Ebooks",
        kind: "builtin",
        base_url: "https://standardebooks.org",
        description: "Open domain editions",
        enabled: true,
        priority: 4,
        health_status: "online",
        last_checked_at: null,
        capabilities: {
          search: true,
          browse_popular: true,
          browse_trending: false,
          browse_latest: false,
          browse_genre: false,
          filter_status: false,
          genres: ["Fiction"],
        },
        config: {},
        config_schema: [],
        is_custom: false,
      },
      {
        id: "fanmtl",
        name: "FanMTL",
        kind: "declarative_html",
        base_url: "https://www.fanmtl.com",
        description: "Web novels",
        enabled: true,
        priority: 10,
        health_status: "online",
        last_checked_at: null,
        capabilities: {
          search: false,
          browse_popular: true,
          browse_trending: false,
          browse_latest: true,
          browse_genre: false,
          filter_status: false,
          genres: ["Fantasy"],
        },
        config: {},
        config_schema: [],
        is_custom: false,
      },
    ]);
    mockApi.discoverNovels.mockResolvedValue({
      items: [
        {
          id: "standardebooks:arthur-conan-doyle/sir-nigel",
          source_id: "standardebooks",
          external_id: "arthur-conan-doyle/sir-nigel",
          title: "Sir Nigel",
          author: "Arthur Conan Doyle",
          description: "Public domain historical romance.",
          cover_url: null,
          local_cover_path: null,
          status: "completed",
          genres: ["Fiction"],
          rating: null,
          popularity: 500,
          chapter_count: null,
          last_updated: null,
          language: "en",
        },
      ],
      page: 1,
      has_more: false,
      total_estimate: 1,
      source_errors: [],
    });

    render(<App />);
    const booksNavBtn = await screen.findByRole("button", { name: /^Books$/i });
    fireEvent.click(booksNavBtn);

    await waitFor(() => {
      expect(mockApi.discoverNovels).toHaveBeenCalledWith(
        expect.objectContaining({
          source_id: "gutenberg,standardebooks,archive_org,wikisource",
        }),
      );
    });
    expect((await screen.findAllByText("Sir Nigel")).length).toBeGreaterThan(0);
    expect(screen.getAllByText("Arthur Conan Doyle").length).toBeGreaterThan(0);
    expect(screen.getByRole("button", { name: "Standard Ebooks" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "FanMTL" })).not.toBeInTheDocument();
  });
});
