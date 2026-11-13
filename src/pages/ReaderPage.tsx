import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  ArrowLeft,
  Bookmark,
  ChevronLeft,
  ChevronRight,
  Copy,
  Download,
  EyeOff,
  Languages,
  List,
  Loader2,
  Maximize2,
  Minimize2,
  Settings2,
  Sparkles,
  Trash2,
  X,
} from "lucide-react";
import { api, extractErrorMessage } from "../api/tauri";
import type {
  AppSettings,
  Bookmark as BookmarkItem,
  Chapter,
  ChapterContent,
  Highlight,
  NovelDetailsResponse,
} from "../types/novel";
import {
  ANNOTATION_COLORS,
  IS_IOS,
  IS_MAC_DESKTOP,
  estimateReadingMinutes,
  languageLabel,
  swatchForColor,
} from "../utils/format";

interface ReaderPageProps {
  novelId: string;
  initialChapterId: string;
  initialParagraphIndex?: number | null;
  settings: AppSettings;
  onUpdateSettings: (next: AppSettings) => void;
  onSwitchNovel?: (novelId: string) => void;
  onExit: () => void;
  onNotify: (msg: string, isError?: boolean) => void;
}

const READER_THEMES: Record<
  string,
  { label: string; bg: string; fg: string; muted: string; border: string; accent: string }
> = {
  obsidian: {
    label: "Obsidian",
    bg: "#0D0F13",
    fg: "#D9DDE6",
    muted: "#7D8596",
    border: "rgba(255,255,255,0.09)",
    accent: "#818CF8",
  },
  amoled: {
    label: "AMOLED",
    bg: "#000000",
    fg: "#E4E6EB",
    muted: "#717682",
    border: "rgba(255,255,255,0.12)",
    accent: "#818CF8",
  },
  paper: {
    label: "Paper",
    bg: "#FAFAFA",
    fg: "#1E222A",
    muted: "#6B7280",
    border: "rgba(15,23,42,0.1)",
    accent: "#4F46E5",
  },
  sepia: {
    label: "Sepia",
    bg: "#F4ECD8",
    fg: "#3B2F21",
    muted: "#7A6855",
    border: "rgba(92,70,45,0.16)",
    accent: "#B45309",
  },
  forest: {
    label: "Forest",
    bg: "#101916",
    fg: "#CAD8CF",
    muted: "#738B7E",
    border: "rgba(180,220,195,0.1)",
    accent: "#34D399",
  },
};

const FONT_OPTIONS = [
  "Literata",
  "Newsreader",
  "Instrument Serif",
  "Charter",
  "Iowan Old Style",
  "Georgia",
  "Palatino Linotype",
  "Palatino",
  "Baskerville",
  "Times New Roman",
  "Segoe UI",
  "-apple-system",
  "Menlo",
];

function resolveFontCss(fontName: string): string {
  if (fontName === "-apple-system") {
    return "-apple-system, BlinkMacSystemFont, 'SF Pro Text', sans-serif";
  }
  if (fontName === "Menlo") {
    return "Menlo, 'SF Mono', monospace";
  }
  return `'${fontName}', Georgia, serif`;
}

type ParsedBlock =
  | { kind: "hr"; key: string }
  | {
      kind: "p" | "h2" | "h3" | "h4" | "blockquote" | "pre" | "ul" | "ol";
      key: string;
      pIndex: number;
      plainText: string;
      content: React.ReactNode;
    };

const BLOCK_TAGS = new Set([
  "p",
  "h1",
  "h2",
  "h3",
  "h4",
  "h5",
  "h6",
  "blockquote",
  "pre",
  "hr",
  "ul",
  "ol",
]);

const CONTAINER_TAGS = new Set(["div", "section", "article", "main", "header", "footer"]);
const SKIP_TAGS = new Set(["script", "style", "iframe", "object", "embed", "noscript"]);

function renderSafeInlineNodes(nodes: Iterable<ChildNode>, keyPrefix: string): React.ReactNode[] {
  const out: React.ReactNode[] = [];
  let idx = 0;
  for (const node of nodes) {
    const k = `${keyPrefix}-${idx++}`;
    if (node.nodeType === Node.TEXT_NODE) {
      if (node.textContent) out.push(node.textContent);
      continue;
    }
    if (node.nodeType !== Node.ELEMENT_NODE) continue;
    const el = node as Element;
    const tag = el.tagName.toLowerCase();
    if (SKIP_TAGS.has(tag)) continue;
    if (tag === "br") {
      out.push(<br key={k} />);
      continue;
    }
    const children = renderSafeInlineNodes(Array.from(el.childNodes), k);
    switch (tag) {
      case "em":
      case "i":
        out.push(<em key={k}>{children}</em>);
        break;
      case "strong":
      case "b":
        out.push(<strong key={k}>{children}</strong>);
        break;
      case "u":
        out.push(<u key={k}>{children}</u>);
        break;
      case "s":
      case "del":
        out.push(<s key={k}>{children}</s>);
        break;
      case "code":
        out.push(
          <code key={k} className="px-1 py-0.5 rounded bg-black/15 font-mono text-[0.9em]">
            {children}
          </code>,
        );
        break;
      case "sup":
        out.push(<sup key={k}>{children}</sup>);
        break;
      case "sub":
        out.push(<sub key={k}>{children}</sub>);
        break;
      case "small":
        out.push(<small key={k}>{children}</small>);
        break;
      case "p":
        // Inside blockquote or list items, separate stacked inner paragraphs cleanly
        out.push(
          <span key={k} className="block mb-2 last:mb-0">
            {children}
          </span>,
        );
        break;
      default:
        out.push(<React.Fragment key={k}>{children}</React.Fragment>);
        break;
    }
  }
  return out;
}

function parseChapterBlocks(ch: ChapterContent): ParsedBlock[] {
  const blocks: ParsedBlock[] = [];
  let nextIdx = 0;

  if (ch.html && ch.html.trim().length > 0 && typeof DOMParser !== "undefined") {
    const doc = new DOMParser().parseFromString(ch.html, "text/html");
    const walk = (nodes: Iterable<ChildNode>) => {
      for (const node of nodes) {
        if (node.nodeType === Node.TEXT_NODE) {
          const text = (node.textContent || "").trim();
          if (text.length > 0) {
            const pIndex = nextIdx++;
            blocks.push({
              kind: "p",
              key: `${ch.chapter_id}-b-${pIndex}`,
              pIndex,
              plainText: text,
              content: text,
            });
          }
          continue;
        }
        if (node.nodeType !== Node.ELEMENT_NODE) continue;
        const el = node as Element;
        const tag = el.tagName.toLowerCase();
        if (SKIP_TAGS.has(tag)) continue;

        if (tag === "hr") {
          blocks.push({ kind: "hr", key: `${ch.chapter_id}-hr-${blocks.length}` });
          continue;
        }

        if (CONTAINER_TAGS.has(tag)) {
          const hasChildBlock = Array.from(el.children).some((c) => {
            const ct = c.tagName.toLowerCase();
            return BLOCK_TAGS.has(ct) || CONTAINER_TAGS.has(ct);
          });
          if (hasChildBlock) {
            walk(Array.from(el.childNodes));
            continue;
          }
        }

        const plainText = (el.textContent || "").replace(/\s+/g, " ").trim();
        if (!plainText) continue;

        const pIndex = nextIdx++;
        const key = `${ch.chapter_id}-b-${pIndex}`;
        if (tag === "ul" || tag === "ol") {
          const liNodes = Array.from(el.children)
            .filter((c) => c.tagName.toLowerCase() === "li")
            .map((li, liIdx) => (
              <li key={`${key}-li-${liIdx}`}>
                {renderSafeInlineNodes(Array.from(li.childNodes), `${key}-li-${liIdx}`)}
              </li>
            ));
          blocks.push({
            kind: tag,
            key,
            pIndex,
            plainText,
            content: liNodes.length > 0 ? liNodes : renderSafeInlineNodes(Array.from(el.childNodes), key),
          });
        } else if (tag === "pre") {
          blocks.push({
            kind: "pre",
            key,
            pIndex,
            plainText,
            content: el.textContent || plainText,
          });
        } else {
          const mappedKind: ParsedBlock["kind"] =
            tag === "h1" || tag === "h2"
              ? "h2"
              : tag === "h3"
                ? "h3"
                : tag === "h4" || tag === "h5" || tag === "h6"
                  ? "h4"
                  : tag === "blockquote"
                    ? "blockquote"
                    : "p";
          blocks.push({
            kind: mappedKind,
            key,
            pIndex,
            plainText,
            content: renderSafeInlineNodes(Array.from(el.childNodes), key),
          });
        }
      }
    };
    walk(Array.from(doc.body.childNodes));
  }

  if (blocks.length === 0 && ch.paragraphs.length > 0) {
    ch.paragraphs.forEach((para, idx) => {
      blocks.push({
        kind: "p",
        key: `${ch.chapter_id}-fallback-${idx}`,
        pIndex: idx,
        plainText: para,
        content: para,
      });
    });
  }

  return blocks;
}

function renderHighlightedText(
  plainText: string,
  paraHighlights: Highlight[],
  onDeleteHighlight: (id: string) => void,
): React.ReactNode {
  if (!plainText || paraHighlights.length === 0) return plainText;
  const ranges: { start: number; end: number; hl: Highlight }[] = [];
  for (const hl of paraHighlights) {
    let start = hl.start_offset;
    let end = hl.end_offset;
    if (
      start < 0 ||
      end <= start ||
      end > plainText.length ||
      plainText.slice(start, end) !== hl.text
    ) {
      const idx = plainText.indexOf(hl.text);
      if (idx === -1) continue;
      start = idx;
      end = idx + hl.text.length;
    }
    ranges.push({ start, end, hl });
  }
  if (ranges.length === 0) return plainText;
  ranges.sort((a, b) => a.start - b.start);
  const parts: React.ReactNode[] = [];
  let cursor = 0;
  for (const r of ranges) {
    if (r.start < cursor) continue;
    if (r.start > cursor) {
      parts.push(plainText.slice(cursor, r.start));
    }
    const sw = swatchForColor(r.hl.color);
    parts.push(
      <mark
        key={r.hl.id}
        title="Click to remove highlight"
        onClick={(e) => {
          e.stopPropagation();
          onDeleteHighlight(r.hl.id);
        }}
        className="rounded-sm px-0.5 cursor-pointer hover:opacity-75 transition-opacity"
        style={{
          backgroundColor: sw.markBg,
          borderBottom: `2px solid ${sw.markBorder}`,
          color: "inherit",
        }}
      >
        {plainText.slice(r.start, r.end)}
      </mark>,
    );
    cursor = r.end;
  }
  if (cursor < plainText.length) {
    parts.push(plainText.slice(cursor));
  }
  return parts;
}

export const ReaderPage: React.FC<ReaderPageProps> = ({
  novelId,
  initialChapterId,
  initialParagraphIndex = null,
  settings,
  onUpdateSettings,
  onSwitchNovel,
  onExit,
  onNotify,
}) => {
  const [details, setDetails] = useState<NovelDetailsResponse | null>(null);
  const [chaptersQueue, setChaptersQueue] = useState<ChapterContent[]>([]);
  const [activeChapterId, setActiveChapterId] = useState(initialChapterId);
  const [loading, setLoading] = useState(true);
  const [loadingNext, setLoadingNext] = useState(false);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);

  const [hudVisible, setHudVisible] = useState(true);
  const [zenMode, setZenMode] = useState(false);
  const [showToc, setShowToc] = useState(false);
  const [tocTab, setTocTab] = useState<"chapters" | "notes">("chapters");
  const [showAppearance, setShowAppearance] = useState(false);
  const [isFullscreen, setIsFullscreen] = useState(false);
  const [customFontInput, setCustomFontInput] = useState(settings.reader_font);

  // Progress & Pagination state
  const [scrollPct, setScrollPct] = useState(0);
  const [currentParagraphIdx, setCurrentParagraphIdx] = useState(0);
  const [pageIndex, setPageIndex] = useState(0);
  const [totalPages, setTotalPages] = useState(1);
  const [pageTurnDir, setPageTurnDir] = useState<"next" | "prev">("next");
  const [pageTurnKey, setPageTurnKey] = useState(0);
  const [paginatedColWidth, setPaginatedColWidth] = useState(Math.min(settings.reader_width, 960));
  const [pageStride, setPageStride] = useState(Math.min(settings.reader_width, 960) + 64);

  // Bookmarks, Highlights & note dialog
  const [bookmarks, setBookmarks] = useState<BookmarkItem[]>([]);
  const [highlights, setHighlights] = useState<Highlight[]>([]);
  const [bookmarkModal, setBookmarkModal] = useState<{
    chapterId: string;
    paragraphIndex: number;
    excerpt: string;
  } | null>(null);
  const [bookmarkNote, setBookmarkNote] = useState("");
  const [bookmarkColor, setBookmarkColor] = useState("amber");
  const [selectionPopup, setSelectionPopup] = useState<{
    text: string;
    startOffset: number;
    endOffset: number;
    x: number;
    y: number;
    chapterId: string;
    paragraphIndex: number;
  } | null>(null);

  const scrollContainerRef = useRef<HTMLDivElement>(null);
  const paginatedViewportRef = useRef<HTMLDivElement>(null);
  const paginatedContentRef = useRef<HTMLDivElement>(null);
  const prefetchedChaptersRef = useRef<Map<string, ChapterContent>>(new Map());
  const sessionStartRef = useRef<number>(Date.now());
  const pendingSaveTimer = useRef<number | null>(null);
  const pendingSavePayloadRef = useRef<{
    chId: string;
    scrollOffset: number;
    paraIdx: number;
    pIdx: number;
    totPages: number;
  } | null>(null);
  const pendingStartAtEndRef = useRef(false);
  const pendingJumpParagraphRef = useRef<{
    chapterId: string;
    paragraphIndex: number;
  } | null>(
    initialParagraphIndex !== null && initialParagraphIndex !== undefined
      ? { chapterId: initialChapterId, paragraphIndex: initialParagraphIndex }
      : null,
  );
  const restoredForChapterRef = useRef<string | null>(null);
  const restoringScrollRef = useRef(false);
  const restoreGuardTimerRef = useRef<number | null>(null);
  const touchGestureRef = useRef<{ x: number; y: number } | null>(null);

  const setInstantScrollTop = useCallback((container: HTMLElement, top: number) => {
    const target = Math.max(0, top);
    if (Math.abs(container.scrollTop - target) < 1) {
      restoringScrollRef.current = false;
      return;
    }
    restoringScrollRef.current = true;
    const prevBehavior = container.style.scrollBehavior;
    container.style.scrollBehavior = "auto";
    container.scrollTop = target;
    container.style.scrollBehavior = prevBehavior;
    if (restoreGuardTimerRef.current) {
      window.clearTimeout(restoreGuardTimerRef.current);
    }
    restoreGuardTimerRef.current = window.setTimeout(() => {
      restoringScrollRef.current = false;
    }, 20);
  }, []);

  useEffect(() => {
    return () => {
      if (restoreGuardTimerRef.current) {
        window.clearTimeout(restoreGuardTimerRef.current);
      }
    };
  }, []);

  const themePalette = useMemo(() => {
    if (settings.reader_theme === "custom") {
      return {
        label: "Custom",
        bg: settings.reader_custom_bg || "#121318",
        fg: settings.reader_custom_fg || "#E2E4EA",
        muted: "#8C93A4",
        border: "rgba(148,163,184,0.18)",
        accent: settings.reader_custom_accent || "#818CF8",
      };
    }
    return READER_THEMES[settings.reader_theme] || READER_THEMES.obsidian;
  }, [settings]);

  const activeChapter = useMemo(
    () =>
      chaptersQueue.find((c) => c.chapter_id === activeChapterId) ||
      chaptersQueue[0] ||
      null,
    [chaptersQueue, activeChapterId],
  );

  // Load novel metadata & initial chapter
  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setErrorMsg(null);
    restoredForChapterRef.current = null;
    pendingJumpParagraphRef.current =
      initialParagraphIndex !== null && initialParagraphIndex !== undefined
        ? { chapterId: initialChapterId, paragraphIndex: initialParagraphIndex }
        : null;
    prefetchedChaptersRef.current.clear();

    Promise.all([
      api.getNovelDetails(novelId, false),
      api.getChapterContent(novelId, initialChapterId),
    ])
      .then(([det, ch]) => {
        if (cancelled) return;
        prefetchedChaptersRef.current.set(ch.chapter_id, ch);
        setDetails(det);
        setBookmarks(det.bookmarks || []);
        setHighlights(det.highlights || []);
        setChaptersQueue([ch]);
        setActiveChapterId(ch.chapter_id);
        sessionStartRef.current = Date.now();
        setLoading(false);
      })
      .catch((e) => {
        if (!cancelled) {
          setErrorMsg(extractErrorMessage(e));
          setLoading(false);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [novelId, initialChapterId]);

  // Flush reading session duration & words periodically and on unmount
  const flushSession = useCallback(
    (ch: ChapterContent | null) => {
      if (!ch) return;
      const secs = Math.max(1, Math.round((Date.now() - sessionStartRef.current) / 1000));
      sessionStartRef.current = Date.now();
      if (secs >= 2) {
        api
          .recordReadingSession(novelId, ch.chapter_id, secs, ch.word_count)
          .catch(() => {});
      }
    },
    [novelId],
  );

  useEffect(() => {
    const interval = window.setInterval(() => {
      flushSession(activeChapter);
    }, 30000);
    return () => {
      window.clearInterval(interval);
      flushSession(activeChapter);
    };
  }, [activeChapter, flushSession]);

  // Save reading progress (debounced, flushed on unmount or chapter jump)
  const flushPendingProgress = useCallback(() => {
    if (pendingSaveTimer.current !== null) {
      window.clearTimeout(pendingSaveTimer.current);
      pendingSaveTimer.current = null;
    }
    const payload = pendingSavePayloadRef.current;
    if (payload) {
      pendingSavePayloadRef.current = null;
      api
        .saveReadingProgress(
          novelId,
          payload.chId,
          payload.scrollOffset,
          payload.paraIdx,
          payload.pIdx,
          payload.totPages,
        )
        .catch(() => {});
    }
  }, [novelId]);

  useEffect(() => {
    return () => {
      flushPendingProgress();
    };
  }, [flushPendingProgress]);

  const scheduleSaveProgress = useCallback(
    (
      chId: string,
      scrollOffset: number,
      paraIdx: number,
      pIdx: number,
      totPages: number,
    ) => {
      if (pendingSaveTimer.current !== null) {
        window.clearTimeout(pendingSaveTimer.current);
      }
      pendingSavePayloadRef.current = { chId, scrollOffset, paraIdx, pIdx, totPages };
      pendingSaveTimer.current = window.setTimeout(() => {
        pendingSaveTimer.current = null;
        pendingSavePayloadRef.current = null;
        api
          .saveReadingProgress(novelId, chId, scrollOffset, paraIdx, pIdx, totPages)
          .catch(() => {});
      }, 350);
    },
    [novelId],
  );

  const jumpToParagraphInScroll = useCallback((chapterId: string, paragraphIndex: number) => {
    setCurrentParagraphIdx(paragraphIndex);
    setTimeout(() => {
      const el = document.querySelector(`[data-ch="${chapterId}"][data-p="${paragraphIndex}"]`);
      if (el && typeof el.scrollIntoView === "function") {
        el.scrollIntoView({ behavior: "smooth", block: "center" });
      }
    }, 60);
  }, []);

  // Paginated measurement (also resolves any pending paragraph jump once column width settles)
  const recomputePages = useCallback(() => {
    if (settings.reader_mode !== "paginated" || loading) return;
    const vp = paginatedViewportRef.current;
    const inner = paginatedContentRef.current;
    if (!vp || !inner) return;
    const gap = 64;
    const rectW = vp.getBoundingClientRect().width;
    const vpWidth =
      rectW > 0
        ? rectW
        : vp.clientWidth > 0
          ? vp.clientWidth
          : Math.min(settings.reader_width, 960);
    const colW = Math.max(240, Math.min(settings.reader_width, vpWidth));
    if (Math.abs(colW - paginatedColWidth) > 0.25) {
      setPaginatedColWidth(colW);
      return;
    }
    const stride = colW + gap;
    setPageStride(stride);
    const totalW = inner.scrollWidth;
    const count = Math.max(1, Math.ceil((totalW - 8) / stride));
    setTotalPages(count);

    const pendingPara = pendingJumpParagraphRef.current;
    if (pendingPara && activeChapter && pendingPara.chapterId === activeChapter.chapter_id) {
      pendingJumpParagraphRef.current = null;
      const el = document.querySelector(
        `[data-ch="${activeChapter.chapter_id}"][data-p="${pendingPara.paragraphIndex}"]`,
      );
      let targetPage = 0;
      if (el) {
        vp.scrollLeft = 0;
        const innerRect = inner.getBoundingClientRect();
        const elRect = el.getBoundingClientRect();
        const offsetLeft = Math.max(0, elRect.left - innerRect.left);
        const rawPage = Math.floor((offsetLeft + 8) / stride);
        targetPage = Math.max(0, Math.min(count - 1, rawPage));
      }
      setCurrentParagraphIdx(pendingPara.paragraphIndex);
      setPageIndex(targetPage);
      setScrollPct(count > 1 ? Math.round((targetPage / (count - 1)) * 100) : 0);
      return;
    }

    if (pendingStartAtEndRef.current) {
      pendingStartAtEndRef.current = false;
      const lastPage = count - 1;
      setPageIndex(lastPage);
      setScrollPct(100);
      if (activeChapter) {
        api
          .saveReadingProgress(novelId, activeChapter.chapter_id, 1, 0, lastPage, count)
          .catch(() => {});
      }
    } else {
      setPageIndex((prev) => Math.min(prev, count - 1));
    }
  }, [
    novelId,
    activeChapter,
    loading,
    paginatedColWidth,
    settings.reader_mode,
    settings.reader_width,
  ]);

  useEffect(() => {
    if (initialParagraphIndex !== null && initialParagraphIndex !== undefined) {
      pendingJumpParagraphRef.current = {
        chapterId: initialChapterId,
        paragraphIndex: initialParagraphIndex,
      };
      if (restoredForChapterRef.current === initialChapterId) {
        restoredForChapterRef.current = null;
      }
    }
  }, [initialChapterId, initialParagraphIndex]);

  // Restore saved position when active chapter first renders
  useEffect(() => {
    if (loading || !activeChapter || !details) return;
    if (restoredForChapterRef.current === activeChapter.chapter_id) return;
    restoredForChapterRef.current = activeChapter.chapter_id;

    // Jump to explicit paragraph if navigated from a Bookmark or Highlights drawer
    const pendingPara =
      pendingJumpParagraphRef.current &&
      pendingJumpParagraphRef.current.chapterId === activeChapter.chapter_id
        ? pendingJumpParagraphRef.current.paragraphIndex
        : initialParagraphIndex;
    if (pendingPara !== null && pendingPara !== undefined) {
      if (settings.reader_mode === "paginated") {
        pendingJumpParagraphRef.current = {
          chapterId: activeChapter.chapter_id,
          paragraphIndex: pendingPara,
        };
        setCurrentParagraphIdx(pendingPara);
        return;
      }
      pendingJumpParagraphRef.current = null;
      jumpToParagraphInScroll(activeChapter.chapter_id, pendingPara);
      return;
    }

    const saved = details.reading_progress;
    if (saved && saved.chapter_id === activeChapter.chapter_id) {
      setScrollPct(Math.round((saved.scroll_offset || 0) * 100));
      setCurrentParagraphIdx(Math.max(0, saved.paragraph_index || 0));
      if (settings.reader_mode === "paginated") {
        setPageIndex(Math.max(0, saved.page_index));
      } else if (scrollContainerRef.current) {
        restoringScrollRef.current = true;
        setTimeout(() => {
          const container = scrollContainerRef.current;
          const el = document.querySelector(
            `[data-ch="${activeChapter.chapter_id}"][data-p="${saved.paragraph_index}"]`,
          );
          if (
            el &&
            !(saved.paragraph_index === 0 && saved.scroll_offset > 0.05) &&
            typeof el.scrollIntoView === "function" &&
            container
          ) {
            const prevBehavior = container.style.scrollBehavior;
            container.style.scrollBehavior = "auto";
            el.scrollIntoView({ behavior: "instant" as ScrollBehavior, block: "start" });
            container.style.scrollBehavior = prevBehavior;
            if (restoreGuardTimerRef.current) {
              window.clearTimeout(restoreGuardTimerRef.current);
            }
            restoreGuardTimerRef.current = window.setTimeout(() => {
              restoringScrollRef.current = false;
            }, 60);
          } else if (container) {
            const maxScroll = Math.max(0, container.scrollHeight - container.clientHeight);
            setInstantScrollTop(container, Math.max(0, maxScroll * saved.scroll_offset));
          } else {
            restoringScrollRef.current = false;
          }
        }, 60);
      }
    } else if (settings.reader_mode === "scroll" && scrollContainerRef.current) {
      setInstantScrollTop(scrollContainerRef.current, 0);
      setScrollPct(0);
    } else {
      setPageIndex(0);
      setScrollPct(0);
    }
  }, [
    loading,
    activeChapter,
    details,
    initialParagraphIndex,
    settings.reader_mode,
    setInstantScrollTop,
    jumpToParagraphInScroll,
  ]);

  const pendingScrollEndChapterRef = useRef<string | null>(null);

  useEffect(() => {
    if (loading || settings.reader_mode !== "scroll" || !activeChapter) return;
    if (pendingScrollEndChapterRef.current !== activeChapter.chapter_id) return;
    pendingScrollEndChapterRef.current = null;
    const applyBottom = () => {
      const c = scrollContainerRef.current;
      if (c) {
        restoringScrollRef.current = true;
        setInstantScrollTop(c, Math.max(0, c.scrollHeight - c.clientHeight));
      } else {
        restoringScrollRef.current = false;
      }
    };
    applyBottom();
    const timer = setTimeout(() => {
      const c = scrollContainerRef.current;
      if (!c) return;
      const targetTop = Math.max(0, c.scrollHeight - c.clientHeight);
      if (Math.abs(c.scrollTop - targetTop) > 1) {
        restoringScrollRef.current = true;
        setInstantScrollTop(c, targetTop);
      }
    }, 40);
    return () => clearTimeout(timer);
  }, [loading, activeChapter?.chapter_id, settings.reader_mode]);

  // Preload adjacent next chapter into memory for seamless chapter transitions
  useEffect(() => {
    const nextId = activeChapter?.next_chapter_id;
    if (!nextId || prefetchedChaptersRef.current.has(nextId)) return;
    let cancelled = false;
    api
      .getChapterContent(novelId, nextId)
      .then((ch) => {
        if (!cancelled) {
          prefetchedChaptersRef.current.set(ch.chapter_id, ch);
        }
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [novelId, activeChapter?.next_chapter_id]);

  const jumpToChapter = useCallback(
    async (chapterId: string, startAtEnd = false, targetParagraph: number | null = null) => {
      flushPendingProgress();
      flushSession(activeChapter);

      // Fast path when target chapter is already rendered (avoids wiping scroll buffer/re-fetching)
      const alreadyLoaded =
        !errorMsg &&
        !loading &&
        !startAtEnd &&
        (targetParagraph !== null && targetParagraph !== undefined
          ? (activeChapter && activeChapter.chapter_id === chapterId) ||
            (settings.reader_mode === "scroll" &&
              chaptersQueue.some((c) => c.chapter_id === chapterId))
          : activeChapter &&
            activeChapter.chapter_id === chapterId &&
            chaptersQueue[0]?.chapter_id === chapterId);
      if (alreadyLoaded) {
        restoredForChapterRef.current = chapterId;
        pendingStartAtEndRef.current = false;
        setActiveChapterId(chapterId);
        setShowToc(false);
        if (targetParagraph !== null && targetParagraph !== undefined) {
          if (settings.reader_mode === "paginated") {
            pendingJumpParagraphRef.current = { chapterId, paragraphIndex: targetParagraph };
            recomputePages();
          } else {
            pendingJumpParagraphRef.current = null;
            jumpToParagraphInScroll(chapterId, targetParagraph);
          }
        } else {
          pendingJumpParagraphRef.current = null;
          setPageIndex(0);
          setScrollPct(0);
          setCurrentParagraphIdx(0);
          if (settings.reader_mode === "scroll" && scrollContainerRef.current) {
            setInstantScrollTop(scrollContainerRef.current, 0);
          }
        }
        return;
      }

      const cached = prefetchedChaptersRef.current.get(chapterId);
      if (!cached) {
        setLoading(true);
      }
      setErrorMsg(null);
      try {
        const ch = cached ?? (await api.getChapterContent(novelId, chapterId));
        prefetchedChaptersRef.current.set(ch.chapter_id, ch);
        // Prevent initial-load restore effect from resetting pageIndex/scroll position
        restoredForChapterRef.current = ch.chapter_id;
        pendingStartAtEndRef.current = startAtEnd;
        const hasTargetPara = !startAtEnd && targetParagraph !== null && targetParagraph !== undefined;
        pendingJumpParagraphRef.current =
          hasTargetPara && settings.reader_mode === "paginated"
            ? { chapterId: ch.chapter_id, paragraphIndex: targetParagraph }
            : null;

        setChaptersQueue([ch]);
        setActiveChapterId(ch.chapter_id);
        setShowToc(false);

        if (startAtEnd && settings.reader_mode === "paginated") {
          pendingScrollEndChapterRef.current = null;
          setPageIndex(9999);
          setScrollPct(100);
          setCurrentParagraphIdx(0);
          setLoading(false);
          await api
            .saveReadingProgress(
              novelId,
              ch.chapter_id,
              1,
              Math.max(0, ch.paragraphs.length - 1),
              0,
              1,
            )
            .catch(() => {});
        } else if (startAtEnd && settings.reader_mode === "scroll") {
          const lastPara = Math.max(0, ch.paragraphs.length - 1);
          pendingScrollEndChapterRef.current = ch.chapter_id;
          restoringScrollRef.current = true;
          setPageIndex(0);
          setScrollPct(100);
          setCurrentParagraphIdx(lastPara);
          setLoading(false);
          await api
            .saveReadingProgress(novelId, ch.chapter_id, 1, lastPara, 0, 1)
            .catch(() => {});
        } else {
          pendingScrollEndChapterRef.current = null;
          const nextPara = hasTargetPara ? targetParagraph : 0;
          setPageIndex(0);
          setScrollPct(0);
          setCurrentParagraphIdx(nextPara);
          setLoading(false);
          if (hasTargetPara && settings.reader_mode === "scroll") {
            jumpToParagraphInScroll(ch.chapter_id, nextPara);
          }
          await api
            .saveReadingProgress(novelId, ch.chapter_id, 0, nextPara, 0, 1)
            .catch(() => {});
        }
      } catch (e) {
        setErrorMsg(extractErrorMessage(e));
      } finally {
        setLoading(false);
      }
    },
    [
      novelId,
      activeChapter,
      chaptersQueue,
      errorMsg,
      loading,
      settings.reader_mode,
      flushPendingProgress,
      flushSession,
      setInstantScrollTop,
      jumpToParagraphInScroll,
      recomputePages,
    ],
  );

  // Continuous scroll auto-loader & scroll progress tracker
  const appendNextChapterInScroll = useCallback(async () => {
    if (loadingNext || chaptersQueue.length === 0) return;
    const last = chaptersQueue[chaptersQueue.length - 1];
    if (!last.next_chapter_id) return;
    if (chaptersQueue.some((c) => c.chapter_id === last.next_chapter_id)) return;

    const cached = prefetchedChaptersRef.current.get(last.next_chapter_id);
    if (cached) {
      setChaptersQueue((prev) => {
        if (prev.some((c) => c.chapter_id === cached.chapter_id)) return prev;
        return [...prev, cached];
      });
      return;
    }

    setLoadingNext(true);
    try {
      const nextCh = await api.getChapterContent(novelId, last.next_chapter_id);
      prefetchedChaptersRef.current.set(nextCh.chapter_id, nextCh);
      setChaptersQueue((prev) => {
        if (prev.some((c) => c.chapter_id === nextCh.chapter_id)) return prev;
        return [...prev, nextCh];
      });
    } catch {
      // ignore silent preload failure; user has explicit Next Chapter button
    } finally {
      setLoadingNext(false);
    }
  }, [chaptersQueue, loadingNext, novelId]);

  const handleScroll = useCallback(() => {
    if (settings.reader_mode !== "scroll" || restoringScrollRef.current) return;
    const container = scrollContainerRef.current;
    if (!container || !activeChapter) return;

    const maxScroll = Math.max(1, container.scrollHeight - container.clientHeight);
    const ratio = Math.min(1, Math.max(0, container.scrollTop / maxScroll));
    setScrollPct(Math.round(ratio * 100));

    // Find which chapter & paragraph is currently at top of viewport
    const containerTop = container.getBoundingClientRect().top + 90;
    const chapterSections = container.querySelectorAll<HTMLElement>("[data-chapter-section]");
    let visibleChapterId = activeChapter.chapter_id;
    let visibleChapterRatio = ratio;

    chapterSections.forEach((sec) => {
      const rect = sec.getBoundingClientRect();
      if (rect.top <= containerTop && rect.bottom > containerTop) {
        const cid = sec.getAttribute("data-chapter-section");
        if (cid) {
          visibleChapterId = cid;
          const secProgress = (containerTop - rect.top) / Math.max(1, rect.height);
          visibleChapterRatio = Math.min(1, Math.max(0, secProgress));
        }
      }
    });

    if (visibleChapterId !== activeChapterId) {
      flushSession(activeChapter);
      // Mark already restored so scrolling smoothly into an appended chapter never snaps scrollTop to 0
      restoredForChapterRef.current = visibleChapterId;
      setActiveChapterId(visibleChapterId);
    }

    const paras = container.querySelectorAll<HTMLElement>(
      `[data-ch="${visibleChapterId}"][data-p]`,
    );
    let topPara = 0;
    for (const p of paras) {
      const r = p.getBoundingClientRect();
      if (r.bottom >= containerTop) {
        topPara = Number(p.getAttribute("data-p") || "0");
        break;
      }
    }
    setCurrentParagraphIdx(topPara);
    scheduleSaveProgress(visibleChapterId, visibleChapterRatio, topPara, 0, 1);

    // Auto-load next chapter near bottom if enabled
    if (settings.reader_auto_load_next && ratio > 0.72) {
      appendNextChapterInScroll();
    }
  }, [
    settings.reader_mode,
    settings.reader_auto_load_next,
    activeChapter,
    activeChapterId,
    appendNextChapterInScroll,
    flushSession,
    scheduleSaveProgress,
  ]);

  useEffect(() => {
    if (settings.reader_mode !== "paginated" || loading) return;
    const t = setTimeout(recomputePages, 40);
    window.addEventListener("resize", recomputePages);
    return () => {
      clearTimeout(t);
      window.removeEventListener("resize", recomputePages);
    };
  }, [
    loading,
    settings.reader_mode,
    settings.reader_font,
    settings.reader_font_size,
    settings.reader_line_height,
    settings.reader_paragraph_spacing,
    settings.reader_width,
    settings.reader_text_align,
    paginatedColWidth,
    activeChapter?.chapter_id,
    activeChapter?.paragraphs.length,
    recomputePages,
  ]);

  const goPage = useCallback(
    (delta: number) => {
      if (!activeChapter) return;
      const dir = delta >= 0 ? "next" : "prev";
      const next = pageIndex + delta;
      if (next >= 0 && next < totalPages) {
        setPageTurnDir(dir);
        setPageTurnKey((k) => k + 1);
        setPageIndex(next);
        const pct = totalPages <= 1 ? 1 : next / (totalPages - 1);
        setScrollPct(Math.round(pct * 100));
        scheduleSaveProgress(activeChapter.chapter_id, pct, 0, next, totalPages);
      } else if (delta > 0 && activeChapter.next_chapter_id) {
        setPageTurnDir("next");
        setPageTurnKey((k) => k + 1);
        jumpToChapter(activeChapter.next_chapter_id, false);
      } else if (delta < 0 && activeChapter.prev_chapter_id) {
        setPageTurnDir("prev");
        setPageTurnKey((k) => k + 1);
        jumpToChapter(activeChapter.prev_chapter_id, true);
      }
    },
    [activeChapter, pageIndex, totalPages, jumpToChapter, scheduleSaveProgress],
  );

  const handleToggleFullscreen = useCallback(async () => {
    if (IS_IOS) {
      setZenMode((prev) => {
        const next = !prev;
        setHudVisible(!next);
        if (next) {
          setShowToc(false);
          setShowAppearance(false);
        }
        return next;
      });
      return;
    }
    const next = await api.toggleFullscreen();
    setIsFullscreen(next);
  }, []);

  const handleReadingSurfaceClick = useCallback(
    (e: React.MouseEvent<HTMLElement>) => {
      const target = e.target as HTMLElement | null;
      if (target?.closest("button, a, input, select, textarea, label")) return;
      if (typeof window !== "undefined" && window.getSelection()?.toString().trim()) return;
      if (showToc || showAppearance) {
        setShowToc(false);
        setShowAppearance(false);
        return;
      }
      if (zenMode) {
        setZenMode(false);
        setHudVisible(true);
      } else {
        setHudVisible((v) => !v);
      }
    },
    [showToc, showAppearance, zenMode],
  );

  const handlePaginatedTouchStart = useCallback((e: React.TouchEvent<HTMLElement>) => {
    const t = e.touches[0];
    if (!t) return;
    touchGestureRef.current = { x: t.clientX, y: t.clientY };
  }, []);

  const handlePaginatedTouchEnd = useCallback(
    (e: React.TouchEvent<HTMLElement>) => {
      const start = touchGestureRef.current;
      touchGestureRef.current = null;
      const t = e.changedTouches[0];
      if (!start || !t) return;
      if (start.x < 20) return; // keep edge free for system navigation gesture
      if (typeof window !== "undefined" && window.getSelection()?.toString().trim()) return;
      const dx = t.clientX - start.x;
      const dy = t.clientY - start.y;
      if (Math.abs(dx) > 45 && Math.abs(dx) > Math.abs(dy) * 1.4) {
        goPage(dx < 0 ? 1 : -1);
      }
    },
    [goPage],
  );

  // Keyboard shortcuts
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement | null;
      if (
        target &&
        (target.tagName === "INPUT" ||
          target.tagName === "TEXTAREA" ||
          target.tagName === "SELECT")
      ) {
        return;
      }

      if (e.key === "Escape") {
        if (bookmarkModal) {
          setBookmarkModal(null);
        } else if (showToc || showAppearance) {
          setShowToc(false);
          setShowAppearance(false);
        } else if (zenMode) {
          setZenMode(false);
          setHudVisible(true);
        } else {
          if (isFullscreen) api.setFullscreen(false);
          onExit();
        }
        return;
      }

      if (e.key === "t" || e.key === "T") {
        e.preventDefault();
        setShowToc((v) => !v);
        setShowAppearance(false);
      } else if (e.key === "a" || e.key === "A") {
        e.preventDefault();
        setShowAppearance((v) => !v);
        setShowToc(false);
      } else if (e.key === "h" || e.key === "H") {
        e.preventDefault();
        setZenMode((z) => {
          setHudVisible(z);
          return !z;
        });
      } else if (e.key === "f" || e.key === "F") {
        e.preventDefault();
        handleToggleFullscreen();
      } else if (e.key === "b" || e.key === "B") {
        e.preventDefault();
        if (activeChapter) {
          setBookmarkModal({
            chapterId: activeChapter.chapter_id,
            paragraphIndex: currentParagraphIdx,
            excerpt:
              activeChapter.paragraphs[currentParagraphIdx]?.slice(0, 180) ||
              activeChapter.title,
          });
        }
      } else if (e.key === "ArrowRight" || e.key === "j" || e.key === "J" || e.key === "]") {
        if (settings.reader_mode === "paginated") {
          e.preventDefault();
          goPage(1);
        } else if (e.key === "]" && activeChapter?.next_chapter_id) {
          e.preventDefault();
          jumpToChapter(activeChapter.next_chapter_id);
        }
      } else if (e.key === "ArrowLeft" || e.key === "k" || e.key === "K" || e.key === "[") {
        if (settings.reader_mode === "paginated") {
          e.preventDefault();
          goPage(-1);
        } else if (e.key === "[" && activeChapter?.prev_chapter_id) {
          e.preventDefault();
          jumpToChapter(activeChapter.prev_chapter_id);
        }
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [
    activeChapter,
    bookmarkModal,
    currentParagraphIdx,
    goPage,
    handleToggleFullscreen,
    isFullscreen,
    jumpToChapter,
    onExit,
    settings.reader_mode,
    showAppearance,
    showToc,
    zenMode,
  ]);

  const updateReaderSetting = <K extends keyof AppSettings>(key: K, value: AppSettings[K]) => {
    const next = { ...settings, [key]: value };
    onUpdateSettings(next);
  };

  const updateCustomColors = (patch: Partial<AppSettings>) => {
    onUpdateSettings({
      ...settings,
      reader_theme: "custom",
      ...patch,
    });
  };

  const handleSaveBookmark = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!bookmarkModal) return;
    try {
      const bm = await api.addBookmark({
        novelId,
        chapterId: bookmarkModal.chapterId,
        paragraphIndex: bookmarkModal.paragraphIndex,
        excerpt: bookmarkModal.excerpt,
        note: bookmarkNote.trim() || null,
        color: bookmarkColor,
        isNovelBookmark: false,
      });
      setBookmarks((prev) => [bm, ...prev]);
      setBookmarkModal(null);
      setBookmarkNote("");
      onNotify("Passage bookmarked");
    } catch (err) {
      onNotify(extractErrorMessage(err), true);
    }
  };

  const handleAddHighlight = async (color: string) => {
    if (!selectionPopup) return;
    try {
      const hl = await api.addHighlight({
        novelId,
        chapterId: selectionPopup.chapterId,
        paragraphIndex: selectionPopup.paragraphIndex,
        startOffset: selectionPopup.startOffset,
        endOffset: selectionPopup.endOffset,
        text: selectionPopup.text,
        color,
      });
      setHighlights((prev) => [...prev, hl]);
      setSelectionPopup(null);
      window.getSelection()?.removeAllRanges();
      onNotify("Passage highlighted");
    } catch (err) {
      onNotify(extractErrorMessage(err), true);
    }
  };

  const handleDeleteHighlight = async (highlightId: string) => {
    try {
      await api.deleteHighlight(highlightId);
      setHighlights((prev) => prev.filter((h) => h.id !== highlightId));
      onNotify("Highlight removed");
    } catch (err) {
      onNotify(extractErrorMessage(err), true);
    }
  };

  const handleTextSelection = (fallbackChapterId: string) => {
    const sel = window.getSelection();
    const text = sel?.toString().trim() || "";
    if (text.length < 2) {
      setSelectionPopup(null);
      return;
    }
    const range = sel?.rangeCount ? sel.getRangeAt(0) : null;
    const rect = range?.getBoundingClientRect();
    let paraIdx = currentParagraphIdx;
    let resolvedChapterId = fallbackChapterId;
    const anchorNode = sel?.anchorNode;
    const anchorElement =
      anchorNode instanceof Element ? anchorNode : anchorNode?.parentElement || null;
    const pEl = anchorElement?.closest("[data-p]");
    if (pEl) {
      paraIdx = Number(pEl.getAttribute("data-p") || "0");
    }
    const chEl = anchorElement?.closest("[data-ch]") || anchorElement?.closest("[data-chapter-section]");
    const attrCh = chEl?.getAttribute("data-ch") || chEl?.getAttribute("data-chapter-section");
    if (attrCh) {
      resolvedChapterId = attrCh;
    }
    let startOffset = 0;
    let endOffset = text.length;
    if (pEl && range) {
      try {
        const preRange = document.createRange();
        preRange.selectNodeContents(pEl);
        preRange.setEnd(range.startContainer, range.startOffset);
        const preLen = preRange.toString().replace(/\s+/g, " ").trimStart().length;
        const pNorm = (pEl.textContent || "").replace(/\s+/g, " ").trim();
        const selNorm = text.replace(/\s+/g, " ").trim();
        const found = pNorm.indexOf(selNorm, Math.max(0, preLen - 4));
        const idx = found >= 0 ? found : pNorm.indexOf(selNorm);
        if (idx >= 0) {
          startOffset = idx;
          endOffset = idx + selNorm.length;
        }
      } catch {
        // keep defaults
      }
    }
    if (rect) {
      setSelectionPopup({
        text: text.slice(0, 260),
        startOffset,
        endOffset,
        x: Math.min(window.innerWidth - 320, Math.max(16, rect.left + rect.width / 2 - 140)),
        y: Math.max(64, rect.top - 48),
        chapterId: resolvedChapterId,
        paragraphIndex: paraIdx,
      });
    }
  };

  const chaptersList: Chapter[] = details?.chapters || [];
  const bookmarkedParaColors = useMemo(() => {
    const map = new Map<string, string>();
    for (const bm of bookmarks) {
      if (bm.chapter_id && bm.paragraph_index !== null) {
        const k = `${bm.chapter_id}:${bm.paragraph_index}`;
        if (!map.has(k)) {
          map.set(k, swatchForColor(bm.color).hex);
        }
      }
    }
    return map;
  }, [bookmarks]);

  const highlightsByPara = useMemo(() => {
    const map = new Map<string, Highlight[]>();
    for (const hl of highlights) {
      const k = `${hl.chapter_id}:${hl.paragraph_index}`;
      const arr = map.get(k);
      if (arr) arr.push(hl);
      else map.set(k, [hl]);
    }
    return map;
  }, [highlights]);

  const parsedBlocksByChapter = useMemo(() => {
    const map = new Map<string, ParsedBlock[]>();
    for (const ch of chaptersQueue) {
      map.set(ch.chapter_id, parseChapterBlocks(ch));
    }
    return map;
  }, [chaptersQueue]);

  const renderBlockElement = useCallback(
    (chapterId: string, block: ParsedBlock) => {
      if (block.kind === "hr") {
        return (
          <hr
            className="my-7 border-t opacity-30"
            style={{ borderColor: themePalette.border }}
          />
        );
      }
      const paraHighlights = highlightsByPara.get(`${chapterId}:${block.pIndex}`);
      const contentNode =
        paraHighlights && paraHighlights.length > 0 && block.kind !== "ul" && block.kind !== "ol"
          ? renderHighlightedText(block.plainText, paraHighlights, handleDeleteHighlight)
          : block.content;
      switch (block.kind) {
        case "h2":
          return <h2 className="text-xl font-semibold mt-7 mb-3 tracking-tight">{contentNode}</h2>;
        case "h3":
          return <h3 className="text-lg font-semibold mt-6 mb-2.5">{contentNode}</h3>;
        case "h4":
          return <h4 className="text-base font-semibold mt-5 mb-2">{contentNode}</h4>;
        case "blockquote":
          return (
            <blockquote
              className="my-4 pl-4 border-l-2 italic opacity-90"
              style={{ borderColor: themePalette.accent }}
            >
              {contentNode}
            </blockquote>
          );
        case "pre":
          return (
            <pre className="my-4 p-4 rounded-xl bg-black/15 overflow-x-auto text-[0.88em] font-mono whitespace-pre-wrap">
              {contentNode}
            </pre>
          );
        case "ul":
          return <ul className="my-4 pl-6 list-disc space-y-1.5">{contentNode}</ul>;
        case "ol":
          return <ol className="my-4 pl-6 list-decimal space-y-1.5">{contentNode}</ol>;
        default:
          return <p>{contentNode}</p>;
      }
    },
    [highlightsByPara, themePalette.accent, themePalette.border],
  );

  return (
    <div
      className="relative h-[100dvh] w-screen overflow-hidden flex flex-col select-text transition-colors duration-200"
      style={{
        backgroundColor: themePalette.bg,
        color: themePalette.fg,
        ["--reader-accent" as string]: themePalette.accent,
      }}
    >
      {/* Top Progress Hairline */}
      <div className="fixed top-0 inset-x-0 h-[2px] z-40 bg-black/15">
        <div
          className="h-full transition-all duration-150"
          style={{
            width: `${scrollPct}%`,
            backgroundColor: themePalette.accent,
          }}
        />
      </div>

      {/* Top HUD */}
      <header
        data-tauri-drag-region={!IS_IOS ? "deep" : undefined}
        className={`${IS_MAC_DESKTOP ? "macos-drag pl-20" : ""} ${
          IS_IOS ? "pt-[var(--safe-top)] min-h-[3.25rem]" : "h-13"
        } shrink-0 px-3 sm:px-4 flex items-center justify-between border-b transition-all duration-200 z-30 ${
          hudVisible && !zenMode
            ? "opacity-100 translate-y-0"
            : "opacity-0 -translate-y-full pointer-events-none absolute inset-x-0 top-0"
        }`}
        style={{
          backgroundColor: themePalette.bg,
          borderColor: themePalette.border,
        }}
      >
        <div
          data-tauri-drag-region="false"
          className="macos-no-drag flex items-center gap-2 sm:gap-3 min-w-0"
        >
          <button
            type="button"
            data-tauri-drag-region="false"
            onClick={() => {
              if (isFullscreen) api.setFullscreen(false);
              onExit();
            }}
            className="inline-flex items-center gap-1.5 px-2.5 py-2 min-h-[40px] rounded-lg text-xs font-medium hover:bg-white/10 transition-colors shrink-0"
            style={{ color: themePalette.fg }}
          >
            <ArrowLeft className="w-4 h-4" />
            <span className="hidden sm:inline">Exit</span>
          </button>
          <div className="h-4 w-px opacity-20 bg-current" />
          <div className="min-w-0">
            <div className="text-xs font-semibold truncate">
              {details?.novel.title || "Loading..."}
            </div>
            <div className="text-[11px] truncate" style={{ color: themePalette.muted }}>
              {activeChapter?.title || ""}
            </div>
          </div>
        </div>

        <div
          data-tauri-drag-region="false"
          className="macos-no-drag flex items-center gap-1.5"
        >
          {activeChapter?.is_downloaded && (
            <span
              title="Reading offline copy"
              className="hidden sm:inline-flex items-center gap-1 px-2 py-1 rounded-md text-[10px] font-medium bg-emerald-500/15 text-emerald-400 mr-1"
            >
              <Download className="w-3 h-3" /> Offline
            </span>
          )}

          {details && details.translations.length > 1 && onSwitchNovel && (
            <label
              data-tauri-drag-region="false"
              title="Switch translation language"
              className="hidden md:inline-flex items-center gap-1 px-2 py-1 rounded-lg border text-[11px] mr-1"
              style={{ borderColor: themePalette.border }}
            >
              <Languages className="w-3.5 h-3.5 opacity-80" />
              <select
                aria-label="Reader translation language"
                value={novelId}
                onChange={(e) => {
                  const target = e.target.value;
                  if (target && target !== novelId) {
                    onSwitchNovel(target);
                  }
                }}
                className="bg-transparent focus:outline-none cursor-pointer"
              >
                {details.translations.map((tr) => (
                  <option
                    key={tr.novel_id}
                    value={tr.novel_id}
                    className="bg-zinc-900 text-zinc-100"
                  >
                    {languageLabel(tr.language_code, tr.language_label)}
                  </option>
                ))}
              </select>
            </label>
          )}

          <button
            type="button"
            data-tauri-drag-region="false"
            title="Table of Contents (T)"
            onClick={() => {
              setShowToc(!showToc);
              setShowAppearance(false);
            }}
            className="p-2 rounded-lg hover:bg-white/10 transition-colors"
          >
            <List className="w-4 h-4" />
          </button>

          <button
            type="button"
            data-tauri-drag-region="false"
            title="Bookmark Position (B)"
            onClick={() => {
              if (!activeChapter) return;
              setBookmarkModal({
                chapterId: activeChapter.chapter_id,
                paragraphIndex: currentParagraphIdx,
                excerpt:
                  activeChapter.paragraphs[currentParagraphIdx]?.slice(0, 180) ||
                  activeChapter.title,
              });
            }}
            className="p-2 rounded-lg hover:bg-white/10 transition-colors"
          >
            <Bookmark className="w-4 h-4" />
          </button>

          <button
            type="button"
            data-tauri-drag-region="false"
            title="Typography & Theme (A)"
            onClick={() => {
              setShowAppearance(!showAppearance);
              setShowToc(false);
            }}
            className="px-2.5 py-1.5 rounded-lg hover:bg-white/10 text-xs font-serif font-semibold transition-colors"
          >
            Aa
          </button>

          <button
            type="button"
            data-tauri-drag-region="false"
            title="Zen Distraction-Free Mode (H)"
            onClick={() => {
              setZenMode(true);
              setHudVisible(false);
              setShowToc(false);
              setShowAppearance(false);
            }}
            className="p-2 rounded-lg hover:bg-white/10 transition-colors"
          >
            <EyeOff className="w-4 h-4" />
          </button>

          {!IS_IOS && (
            <button
              type="button"
              data-tauri-drag-region="false"
              title="Toggle Fullscreen (F)"
              onClick={handleToggleFullscreen}
              className="p-2 rounded-lg hover:bg-white/10 transition-colors"
            >
              {isFullscreen ? (
                <Minimize2 className="w-4 h-4" />
              ) : (
                <Maximize2 className="w-4 h-4" />
              )}
            </button>
          )}
        </div>
      </header>

      {/* Zen Floating Exit Pill */}
      {zenMode && (
        <button
          type="button"
          onClick={() => {
            setZenMode(false);
            setHudVisible(true);
          }}
          className="fixed top-[calc(0.75rem+var(--safe-top))] right-4 z-40 px-3 py-1.5 rounded-full text-[11px] bg-black/45 text-white/80 hover:text-white backdrop-blur-md border border-white/10 transition-opacity opacity-50 sm:opacity-30 hover:opacity-100"
        >
          {IS_IOS ? "Show Controls" : "Show Controls (H)"}
        </button>
      )}

      {/* Main Reading Area */}
      {loading ? (
        <div className="flex-1 flex items-center justify-center gap-2 text-sm opacity-75">
          <Loader2 className="w-5 h-5 animate-spin" />
          Loading chapter...
        </div>
      ) : errorMsg ? (
        <div className="flex-1 flex flex-col items-center justify-center p-8 text-center space-y-4">
          <div className="max-w-md text-sm text-rose-300">{errorMsg}</div>
          <div className="flex gap-2">
            <button
              type="button"
              onClick={() => jumpToChapter(activeChapterId)}
              className="px-4 py-2 rounded-lg text-xs font-medium bg-white/10 hover:bg-white/15"
            >
              Retry
            </button>
            <button
              type="button"
              onClick={onExit}
              className="px-4 py-2 rounded-lg text-xs font-medium bg-white/5 hover:bg-white/10"
            >
              Return to Novel
            </button>
          </div>
        </div>
      ) : settings.reader_mode === "scroll" ? (
        /* ---------------- CONTINUOUS SCROLL MODE ---------------- */
        <div
          ref={scrollContainerRef}
          onScroll={handleScroll}
          onClick={handleReadingSurfaceClick}
          onMouseUp={() => activeChapter && handleTextSelection(activeChapter.chapter_id)}
          onTouchEnd={() => {
            if (!activeChapter) return;
            window.setTimeout(() => handleTextSelection(activeChapter.chapter_id), 60);
          }}
          className="flex-1 overflow-y-auto overscroll-y-contain px-4 sm:px-8 md:px-10 pt-6 sm:pt-10 pb-[calc(2.5rem+var(--safe-bottom))]"
        >
          <div
            className="mx-auto space-y-16"
            style={{ maxWidth: `${settings.reader_width}px` }}
          >
            {chaptersQueue.map((ch) => (
              <article
                key={ch.chapter_id}
                data-chapter-section={ch.chapter_id}
                className="novel-prose"
                style={{
                  fontFamily: resolveFontCss(settings.reader_font),
                  fontSize: `${settings.reader_font_size}px`,
                  lineHeight: settings.reader_line_height,
                  textAlign: settings.reader_text_align,
                  ["--reader-para-spacing" as string]: `${settings.reader_paragraph_spacing}em`,
                }}
              >
                <div className="mb-8 pb-4 border-b" style={{ borderColor: themePalette.border }}>
                  <div
                    className="text-xs uppercase tracking-widest mb-1.5"
                    style={{ color: themePalette.muted, fontFamily: "sans-serif" }}
                  >
                    Chapter {Math.round(ch.chapter_number)} ·{" "}
                    {estimateReadingMinutes(ch.word_count)} min read
                  </div>
                  <h1 className="text-2xl sm:text-3xl font-semibold tracking-tight leading-tight">
                    {ch.title}
                  </h1>
                </div>

                <div>
                  {(parsedBlocksByChapter.get(ch.chapter_id) || []).map((block) => {
                    if (block.kind === "hr") {
                      return (
                        <React.Fragment key={block.key}>
                          {renderBlockElement(ch.chapter_id, block)}
                        </React.Fragment>
                      );
                    }
                    const bmColor = bookmarkedParaColors.get(`${ch.chapter_id}:${block.pIndex}`);
                    return (
                      <div
                        key={block.key}
                        data-ch={ch.chapter_id}
                        data-p={block.pIndex}
                        className={`group relative rounded-md transition-colors ${
                          bmColor ? "pl-3 border-l-2" : ""
                        }`}
                        style={bmColor ? { borderColor: bmColor } : undefined}
                      >
                        {renderBlockElement(ch.chapter_id, block)}
                        <button
                          type="button"
                          title="Bookmark this paragraph"
                          onClick={() =>
                            setBookmarkModal({
                              chapterId: ch.chapter_id,
                              paragraphIndex: block.pIndex,
                              excerpt: block.plainText.slice(0, 180),
                            })
                          }
                          className="opacity-0 group-hover:opacity-75 hover:!opacity-100 absolute -left-7 top-1 p-1 rounded transition-opacity"
                          style={{ color: bmColor || themePalette.accent }}
                        >
                          <Bookmark className="w-3.5 h-3.5" />
                        </button>
                      </div>
                    );
                  })}
                </div>
              </article>
            ))}

            {/* Chapter Navigation Footer */}
            {chaptersQueue.length > 0 && (
              <div
                className="pt-8 pb-12 border-t flex items-center justify-between gap-4"
                style={{ borderColor: themePalette.border }}
              >
                <button
                  type="button"
                  disabled={!chaptersQueue[0].prev_chapter_id}
                  onClick={() =>
                    chaptersQueue[0].prev_chapter_id &&
                    jumpToChapter(chaptersQueue[0].prev_chapter_id)
                  }
                  className="inline-flex items-center gap-2 px-4 py-2.5 rounded-xl text-xs font-medium border disabled:opacity-30 hover:bg-white/5 transition-colors"
                  style={{ borderColor: themePalette.border }}
                >
                  <ChevronLeft className="w-4 h-4" />
                  Previous Chapter
                </button>

                {loadingNext ? (
                  <span className="inline-flex items-center gap-1.5 text-xs opacity-65">
                    <Loader2 className="w-3.5 h-3.5 animate-spin" />
                    Loading next chapter...
                  </span>
                ) : (
                  <button
                    type="button"
                    disabled={!chaptersQueue[chaptersQueue.length - 1].next_chapter_id}
                    onClick={() => {
                      const nextId =
                        chaptersQueue[chaptersQueue.length - 1].next_chapter_id;
                      if (nextId) jumpToChapter(nextId);
                    }}
                    className="inline-flex items-center gap-2 px-4 py-2.5 rounded-xl text-xs font-medium text-white disabled:opacity-30 transition-colors"
                    style={{ backgroundColor: themePalette.accent }}
                  >
                    Next Chapter
                    <ChevronRight className="w-4 h-4" />
                  </button>
                )}
              </div>
            )}
          </div>
        </div>
      ) : (
        /* ---------------- PAGINATED MODE ---------------- */
        <div
          className="flex-1 flex flex-col overflow-hidden px-3 sm:px-6 pt-3 sm:pt-6 pb-[calc(0.75rem+var(--safe-bottom))]"
          onClick={handleReadingSurfaceClick}
          onTouchStart={handlePaginatedTouchStart}
          onTouchEnd={(e) => {
            handlePaginatedTouchEnd(e);
            if (activeChapter) {
              window.setTimeout(() => handleTextSelection(activeChapter.chapter_id), 60);
            }
          }}
          onMouseUp={() => activeChapter && handleTextSelection(activeChapter.chapter_id)}
        >
          <div
            ref={paginatedViewportRef}
            className="relative flex-1 mx-auto w-full overflow-hidden"
            style={{ maxWidth: `${settings.reader_width}px` }}
          >
            {/* Left/Right Click Zones for effortless page turning */}
            <button
              type="button"
              aria-label="Previous page"
              onClick={() => goPage(-1)}
              className="absolute inset-y-0 left-0 w-1/5 z-10 cursor-w-resize opacity-0"
            />
            <button
              type="button"
              aria-label="Next page"
              onClick={() => goPage(1)}
              className="absolute inset-y-0 right-0 w-1/5 z-10 cursor-e-resize opacity-0"
            />

            {settings.reader_flip_animation && pageTurnKey > 0 && (
              <div
                key={pageTurnKey}
                className={`pointer-events-none absolute inset-0 z-20 rounded-xl ${
                  pageTurnDir === "next" ? "page-flip-next" : "page-flip-prev"
                }`}
                style={{
                  boxShadow:
                    pageTurnDir === "next"
                      ? "inset -28px 0 36px -24px rgba(0,0,0,0.35)"
                      : "inset 28px 0 36px -24px rgba(0,0,0,0.35)",
                }}
              />
            )}

            <div
              ref={paginatedContentRef}
              className={`h-full novel-prose ${
                settings.reader_flip_animation
                  ? "transition-transform duration-300 ease-out"
                  : ""
              }`}
              style={{
                columnWidth: `${paginatedColWidth}px`,
                columnGap: "64px",
                columnFill: "auto",
                height: "100%",
                transform: `translateX(-${pageIndex * pageStride}px)`,
                fontFamily: resolveFontCss(settings.reader_font),
                fontSize: `${settings.reader_font_size}px`,
                lineHeight: settings.reader_line_height,
                textAlign: settings.reader_text_align,
                ["--reader-para-spacing" as string]: `${settings.reader_paragraph_spacing}em`,
              }}
            >
              {activeChapter && (
                <>
                  <h1 className="text-2xl font-semibold mb-6">{activeChapter.title}</h1>
                  {(parsedBlocksByChapter.get(activeChapter.chapter_id) || []).map((block) =>
                    block.kind === "hr" ? (
                      <React.Fragment key={block.key}>
                        {renderBlockElement(activeChapter.chapter_id, block)}
                      </React.Fragment>
                    ) : (
                      <div key={block.key} data-ch={activeChapter.chapter_id} data-p={block.pIndex}>
                        {renderBlockElement(activeChapter.chapter_id, block)}
                      </div>
                    ),
                  )}
                </>
              )}
            </div>
          </div>

          <div
            className="shrink-0 pt-3 flex items-center justify-between max-w-2xl mx-auto w-full text-xs"
            style={{ color: themePalette.muted }}
          >
            <button
              type="button"
              onClick={() => goPage(-1)}
              disabled={pageIndex === 0 && !activeChapter?.prev_chapter_id}
              className="inline-flex items-center gap-1 px-3 py-2 min-h-[40px] rounded-lg hover:bg-white/10 disabled:opacity-30"
            >
              <ChevronLeft className="w-4 h-4" /> Prev
            </button>
            <span className="tabular-nums truncate px-2 text-center">
              Page {pageIndex + 1} of {totalPages} · {activeChapter?.title}
            </span>
            <button
              type="button"
              onClick={() => goPage(1)}
              disabled={pageIndex >= totalPages - 1 && !activeChapter?.next_chapter_id}
              className="inline-flex items-center gap-1 px-3 py-2 min-h-[40px] rounded-lg hover:bg-white/10 disabled:opacity-30"
            >
              Next <ChevronRight className="w-4 h-4" />
            </button>
          </div>
        </div>
      )}

      {/* Text Selection Quick Action Bar */}
      {selectionPopup && (
        <div
          className="fixed z-50 flex items-center gap-1 p-1.5 rounded-xl border shadow-xl backdrop-blur-md"
          style={{
            left: selectionPopup.x,
            top: selectionPopup.y,
            backgroundColor: themePalette.bg,
            borderColor: themePalette.border,
          }}
        >
          <div
            className="flex items-center gap-1 pr-1.5 mr-1 border-r"
            style={{ borderColor: themePalette.border }}
          >
            {ANNOTATION_COLORS.map((c) => (
              <button
                key={c.id}
                type="button"
                title={`Highlight ${c.label}`}
                aria-label={`Highlight ${c.label}`}
                onClick={() => handleAddHighlight(c.id)}
                className="w-5 h-5 rounded-full border border-white/25 hover:scale-110 transition-transform"
                style={{ backgroundColor: c.hex }}
              />
            ))}
          </div>
          <button
            type="button"
            onClick={() => {
              navigator.clipboard?.writeText(selectionPopup.text);
              setSelectionPopup(null);
              onNotify("Copied selection");
            }}
            className="inline-flex items-center gap-1 px-2 py-1 rounded-lg text-xs hover:bg-white/10"
          >
            <Copy className="w-3.5 h-3.5" /> Copy
          </button>
          <button
            type="button"
            onClick={() => {
              setBookmarkModal({
                chapterId: selectionPopup.chapterId,
                paragraphIndex: selectionPopup.paragraphIndex,
                excerpt: selectionPopup.text,
              });
              setSelectionPopup(null);
            }}
            className="inline-flex items-center gap-1 px-2 py-1 rounded-lg text-xs hover:bg-white/10"
            style={{ color: themePalette.accent }}
          >
            <Bookmark className="w-3.5 h-3.5" /> Bookmark
          </button>
        </div>
      )}

      {/* Table of Contents & Annotations Slide-Over Drawer */}
      {showToc && (
        <aside
          className="fixed inset-y-0 right-0 w-full max-w-full sm:w-96 z-40 border-l shadow-2xl flex flex-col pt-[var(--safe-top)] pb-[var(--safe-bottom)]"
          style={{
            backgroundColor: themePalette.bg,
            borderColor: themePalette.border,
          }}
        >
          <div
            className="p-3 border-b flex items-center justify-between gap-2"
            style={{ borderColor: themePalette.border }}
          >
            <div className="flex items-center gap-1">
              <button
                type="button"
                onClick={() => setTocTab("chapters")}
                className="px-2.5 py-1 rounded-lg text-xs font-semibold transition-colors"
                style={
                  tocTab === "chapters"
                    ? { backgroundColor: `${themePalette.accent}22`, color: themePalette.accent }
                    : undefined
                }
              >
                Chapters ({chaptersList.length})
              </button>
              <button
                type="button"
                onClick={() => setTocTab("notes")}
                className="px-2.5 py-1 rounded-lg text-xs font-semibold transition-colors"
                style={
                  tocTab === "notes"
                    ? { backgroundColor: `${themePalette.accent}22`, color: themePalette.accent }
                    : undefined
                }
              >
                Highlights & Marks ({highlights.length + bookmarks.length})
              </button>
            </div>
            <button
              type="button"
              onClick={() => setShowToc(false)}
              className="p-1 rounded-md hover:bg-white/10"
            >
              <X className="w-4 h-4" />
            </button>
          </div>
          {tocTab === "chapters" ? (
            <div className="flex-1 overflow-y-auto p-2 space-y-0.5">
              {chaptersList.map((ch) => {
                const current = ch.id === activeChapter?.chapter_id;
                return (
                  <button
                    key={ch.id}
                    type="button"
                    onClick={() => jumpToChapter(ch.id)}
                    className={`w-full text-left px-3 py-2 rounded-lg text-xs flex items-center justify-between gap-2 transition-colors ${
                      current ? "font-semibold" : "hover:bg-white/5"
                    }`}
                    style={
                      current
                        ? { backgroundColor: `${themePalette.accent}22`, color: themePalette.accent }
                        : undefined
                    }
                  >
                    <span className="truncate">{ch.title}</span>
                    {ch.is_downloaded && (
                      <Download className="w-3 h-3 text-emerald-400 shrink-0" />
                    )}
                  </button>
                );
              })}
            </div>
          ) : (
            <div className="flex-1 overflow-y-auto p-3 space-y-3">
              {highlights.length === 0 && bookmarks.length === 0 ? (
                <div className="text-xs opacity-60 text-center py-8">
                  Select text to highlight passages, or press B to add colored bookmarks.
                </div>
              ) : (
                <>
                  {highlights.map((hl) => {
                    const sw = swatchForColor(hl.color);
                    return (
                      <div
                        key={hl.id}
                        className="p-2.5 rounded-lg border text-xs space-y-1"
                        style={{
                          borderColor: themePalette.border,
                          borderLeftWidth: 3,
                          borderLeftColor: sw.hex,
                        }}
                      >
                        <div className="flex items-start justify-between gap-2">
                          <button
                            type="button"
                            onClick={() => jumpToChapter(hl.chapter_id, false, hl.paragraph_index)}
                            className="text-left italic line-clamp-3 hover:underline"
                          >
                            “{hl.text}”
                          </button>
                          <button
                            type="button"
                            title="Delete highlight"
                            onClick={() => handleDeleteHighlight(hl.id)}
                            className="p-1 opacity-60 hover:opacity-100 hover:text-rose-400"
                          >
                            <Trash2 className="w-3.5 h-3.5" />
                          </button>
                        </div>
                      </div>
                    );
                  })}
                  {bookmarks.map((bm) => {
                    const sw = swatchForColor(bm.color);
                    return (
                      <div
                        key={bm.id}
                        className="p-2.5 rounded-lg border text-xs space-y-1"
                        style={{
                          borderColor: themePalette.border,
                          borderLeftWidth: 3,
                          borderLeftColor: sw.hex,
                        }}
                      >
                        <div className="flex items-center justify-between gap-2">
                          <button
                            type="button"
                            onClick={() =>
                              bm.chapter_id &&
                              jumpToChapter(bm.chapter_id, false, bm.paragraph_index ?? null)
                            }
                            className="font-medium truncate hover:underline"
                          >
                            {bm.chapter_title || "Bookmark"}
                          </button>
                          <span
                            className="w-2.5 h-2.5 rounded-full shrink-0"
                            style={{ backgroundColor: sw.hex }}
                          />
                        </div>
                        {bm.excerpt && (
                          <div className="opacity-75 italic line-clamp-2">“{bm.excerpt}”</div>
                        )}
                        {bm.note && <div className="opacity-90">{bm.note}</div>}
                      </div>
                    );
                  })}
                </>
              )}
            </div>
          )}
        </aside>
      )}

      {/* Typography & Appearance Drawer */}
      {showAppearance && (
        <aside
          className="fixed top-[calc(3.5rem+var(--safe-top))] right-2 left-2 sm:left-auto sm:right-4 sm:w-96 max-h-[78dvh] overflow-y-auto z-40 rounded-2xl border shadow-2xl p-4 space-y-5"
          style={{
            backgroundColor: themePalette.bg,
            borderColor: themePalette.border,
          }}
        >
          <div className="flex items-center justify-between">
            <span className="text-sm font-semibold flex items-center gap-1.5">
              <Settings2 className="w-4 h-4" /> Reading Studio
            </span>
            <button
              type="button"
              onClick={() => setShowAppearance(false)}
              className="p-1 rounded-md hover:bg-white/10"
            >
              <X className="w-4 h-4" />
            </button>
          </div>

          {/* Reading Mode */}
          <div className="space-y-1.5">
            <label className="text-[11px] uppercase tracking-wider opacity-60">Mode</label>
            <div className="grid grid-cols-2 gap-2">
              {(["scroll", "paginated"] as const).map((m) => (
                <button
                  key={m}
                  type="button"
                  onClick={() => updateReaderSetting("reader_mode", m)}
                  className="py-1.5 px-3 rounded-lg text-xs font-medium border transition-colors capitalize"
                  style={{
                    borderColor:
                      settings.reader_mode === m ? themePalette.accent : themePalette.border,
                    backgroundColor:
                      settings.reader_mode === m ? `${themePalette.accent}22` : "transparent",
                  }}
                >
                  {m === "scroll" ? "Continuous Scroll" : "Book Flip"}
                </button>
              ))}
            </div>
            {settings.reader_mode === "paginated" && (
              <label className="flex items-center justify-between text-xs pt-1.5 cursor-pointer">
                <span>Page Flip Animation</span>
                <input
                  type="checkbox"
                  checked={settings.reader_flip_animation}
                  onChange={(e) =>
                    updateReaderSetting("reader_flip_animation", e.target.checked)
                  }
                  className="accent-[var(--reader-accent)]"
                />
              </label>
            )}
          </div>

          {/* Reading Themes */}
          <div className="space-y-1.5">
            <label className="text-[11px] uppercase tracking-wider opacity-60">Theme</label>
            <div className="grid grid-cols-3 sm:grid-cols-6 gap-1.5">
              {Object.entries(READER_THEMES).map(([key, t]) => (
                <button
                  key={key}
                  type="button"
                  onClick={() => updateReaderSetting("reader_theme", key as AppSettings["reader_theme"])}
                  className="flex flex-col items-center gap-1 p-1.5 rounded-lg border text-[10px]"
                  style={{
                    borderColor:
                      settings.reader_theme === key ? themePalette.accent : themePalette.border,
                  }}
                >
                  <span
                    className="w-6 h-6 rounded-full border"
                    style={{ backgroundColor: t.bg, borderColor: t.fg }}
                  />
                  {t.label}
                </button>
              ))}
              <button
                type="button"
                onClick={() => updateReaderSetting("reader_theme", "custom")}
                className="flex flex-col items-center gap-1 p-1.5 rounded-lg border text-[10px]"
                style={{
                  borderColor:
                    settings.reader_theme === "custom" ? themePalette.accent : themePalette.border,
                }}
              >
                <span className="w-6 h-6 rounded-full border flex items-center justify-center">
                  <Sparkles className="w-3 h-3" />
                </span>
                Custom
              </button>
            </div>

            <div className="grid grid-cols-3 gap-2 pt-2">
              <label className="text-[11px] flex flex-col gap-1">
                Page Color
                <input
                  type="color"
                  aria-label="Page Background Color"
                  value={themePalette.bg}
                  onChange={(e) => updateCustomColors({ reader_custom_bg: e.target.value })}
                  className="w-full h-7 rounded cursor-pointer bg-transparent"
                />
              </label>
              <label className="text-[11px] flex flex-col gap-1">
                Font Color
                <input
                  type="color"
                  aria-label="Font Color"
                  value={themePalette.fg}
                  onChange={(e) => updateCustomColors({ reader_custom_fg: e.target.value })}
                  className="w-full h-7 rounded cursor-pointer bg-transparent"
                />
              </label>
              <label className="text-[11px] flex flex-col gap-1">
                Accent
                <input
                  type="color"
                  aria-label="Accent Color"
                  value={themePalette.accent}
                  onChange={(e) => updateCustomColors({ reader_custom_accent: e.target.value })}
                  className="w-full h-7 rounded cursor-pointer bg-transparent"
                />
              </label>
            </div>
          </div>

          {details && details.translations.length > 1 && onSwitchNovel && (
            <div className="space-y-1.5">
              <label className="text-[11px] uppercase tracking-wider opacity-60">
                Translation Language
              </label>
              <select
                aria-label="Reader translation language menu"
                value={novelId}
                onChange={(e) => {
                  const target = e.target.value;
                  if (target && target !== novelId) {
                    onSwitchNovel(target);
                  }
                }}
                className="w-full px-2.5 py-2 rounded-lg border bg-transparent text-xs"
                style={{ borderColor: themePalette.border }}
              >
                {details.translations.map((tr) => (
                  <option
                    key={tr.novel_id}
                    value={tr.novel_id}
                    className="bg-zinc-900 text-zinc-100"
                  >
                    {languageLabel(tr.language_code, tr.language_label)}
                  </option>
                ))}
              </select>
            </div>
          )}

          {/* Font Family */}
          <div className="space-y-1.5">
            <label className="text-[11px] uppercase tracking-wider opacity-60">Typeface</label>
            <select
              value={FONT_OPTIONS.includes(settings.reader_font) ? settings.reader_font : "custom"}
              onChange={(e) => {
                if (e.target.value !== "custom") {
                  setCustomFontInput(e.target.value);
                  updateReaderSetting("reader_font", e.target.value);
                }
              }}
              className="w-full px-2.5 py-1.5 rounded-lg border bg-transparent text-xs"
              style={{ borderColor: themePalette.border }}
            >
              {FONT_OPTIONS.map((f) => (
                <option key={f} value={f} className="bg-zinc-900 text-zinc-100">
                  {f}
                </option>
              ))}
              <option value="custom" className="bg-zinc-900 text-zinc-100">
                Custom System Font...
              </option>
            </select>
            <div className="flex gap-1.5">
              <input
                value={customFontInput}
                onChange={(e) => setCustomFontInput(e.target.value)}
                placeholder="Any installed system font..."
                className="flex-1 px-2.5 py-1 rounded-lg border bg-transparent text-xs"
                style={{ borderColor: themePalette.border }}
              />
              <button
                type="button"
                onClick={() => {
                  if (customFontInput.trim()) {
                    updateReaderSetting("reader_font", customFontInput.trim());
                  }
                }}
                className="px-2.5 py-1 rounded-lg text-xs border hover:bg-white/10"
                style={{ borderColor: themePalette.border }}
              >
                Apply
              </button>
            </div>
          </div>

          {/* Sliders */}
          <div className="space-y-3">
            <div className="space-y-1">
              <div className="flex justify-between text-xs">
                <span>Font Size</span>
                <span className="tabular-nums opacity-75">{settings.reader_font_size}px</span>
              </div>
              <input
                type="range"
                min={14}
                max={30}
                step={1}
                value={settings.reader_font_size}
                onChange={(e) => updateReaderSetting("reader_font_size", Number(e.target.value))}
                className="w-full accent-[var(--reader-accent)]"
              />
            </div>

            <div className="space-y-1">
              <div className="flex justify-between text-xs">
                <span>Line Height</span>
                <span className="tabular-nums opacity-75">{settings.reader_line_height.toFixed(2)}</span>
              </div>
              <input
                type="range"
                min={1.3}
                max={2.2}
                step={0.05}
                value={settings.reader_line_height}
                onChange={(e) => updateReaderSetting("reader_line_height", Number(e.target.value))}
                className="w-full accent-[var(--reader-accent)]"
              />
            </div>

            <div className="space-y-1">
              <div className="flex justify-between text-xs">
                <span>Paragraph Spacing</span>
                <span className="tabular-nums opacity-75">
                  {settings.reader_paragraph_spacing.toFixed(2)}em
                </span>
              </div>
              <input
                type="range"
                min={0.5}
                max={2.2}
                step={0.05}
                value={settings.reader_paragraph_spacing}
                onChange={(e) =>
                  updateReaderSetting("reader_paragraph_spacing", Number(e.target.value))
                }
                className="w-full accent-[var(--reader-accent)]"
              />
            </div>

            <div className="space-y-1">
              <div className="flex justify-between text-xs">
                <span>Reading Width</span>
                <span className="tabular-nums opacity-75">{settings.reader_width}px</span>
              </div>
              <input
                type="range"
                min={520}
                max={960}
                step={20}
                value={settings.reader_width}
                onChange={(e) => updateReaderSetting("reader_width", Number(e.target.value))}
                className="w-full accent-[var(--reader-accent)]"
              />
            </div>
          </div>

          <div className="flex items-center justify-between text-xs">
            <span>Alignment</span>
            <div className="flex gap-1.5">
              {(["left", "justify"] as const).map((a) => (
                <button
                  key={a}
                  type="button"
                  onClick={() => updateReaderSetting("reader_text_align", a)}
                  className="px-2.5 py-1 rounded border capitalize"
                  style={{
                    borderColor:
                      settings.reader_text_align === a
                        ? themePalette.accent
                        : themePalette.border,
                  }}
                >
                  {a}
                </button>
              ))}
            </div>
          </div>

          <label className="flex items-center justify-between text-xs cursor-pointer">
            <span>Auto-load next chapter</span>
            <input
              type="checkbox"
              checked={settings.reader_auto_load_next}
              onChange={(e) => updateReaderSetting("reader_auto_load_next", e.target.checked)}
              className="accent-[var(--reader-accent)]"
            />
          </label>
        </aside>
      )}

      {/* Add Passage Bookmark Modal */}
      {bookmarkModal && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm p-4"
          onClick={() => setBookmarkModal(null)}
        >
          <form
            onSubmit={handleSaveBookmark}
            onClick={(e) => e.stopPropagation()}
            className="w-full max-w-md rounded-2xl border p-5 space-y-4 shadow-2xl"
            style={{
              backgroundColor: themePalette.bg,
              borderColor: themePalette.border,
            }}
          >
            <div className="flex items-center justify-between">
              <h3 className="text-sm font-semibold">Save Passage Bookmark</h3>
              <button type="button" onClick={() => setBookmarkModal(null)}>
                <X className="w-4 h-4" />
              </button>
            </div>
            <blockquote
              className="text-xs italic border-l-2 pl-3 py-1 opacity-80 line-clamp-3"
              style={{ borderColor: swatchForColor(bookmarkColor).hex }}
            >
              “{bookmarkModal.excerpt}”
            </blockquote>
            <div className="flex items-center justify-between">
              <span className="text-xs opacity-75">Bookmark Color</span>
              <div className="flex items-center gap-2">
                {ANNOTATION_COLORS.map((c) => (
                  <button
                    key={c.id}
                    type="button"
                    aria-label={`Bookmark color ${c.label}`}
                    onClick={() => setBookmarkColor(c.id)}
                    className={`w-6 h-6 rounded-full border-2 transition-transform ${
                      bookmarkColor === c.id ? "scale-110" : "opacity-65 hover:opacity-100"
                    }`}
                    style={{
                      backgroundColor: c.hex,
                      borderColor: bookmarkColor === c.id ? "#ffffff" : "transparent",
                    }}
                  />
                ))}
              </div>
            </div>
            <textarea
              autoFocus
              rows={3}
              value={bookmarkNote}
              onChange={(e) => setBookmarkNote(e.target.value)}
              placeholder="Optional personal note..."
              className="w-full rounded-lg border bg-transparent p-2.5 text-xs focus:outline-none"
              style={{ borderColor: themePalette.border }}
            />
            <div className="flex justify-end gap-2">
              <button
                type="button"
                onClick={() => setBookmarkModal(null)}
                className="px-3 py-1.5 rounded-lg text-xs border"
                style={{ borderColor: themePalette.border }}
              >
                Cancel
              </button>
              <button
                type="submit"
                className="px-3.5 py-1.5 rounded-lg text-xs font-medium text-white"
                style={{ backgroundColor: themePalette.accent }}
              >
                Save Bookmark
              </button>
            </div>
          </form>
        </div>
      )}
    </div>
  );
};
