import React, { useEffect, useMemo, useRef, useState } from "react";
import {
  ArrowDownUp,
  ArrowLeft,
  Bell,
  Bookmark,
  BookOpen,
  Check,
  CheckSquare,
  Download,
  FolderPlus,
  Languages,
  Loader2,
  RefreshCw,
  Search,
  Square,
  Star,
  Trash2,
} from "lucide-react";
import { api, extractErrorMessage } from "../api/tauri";
import { CoverImage } from "../components/CoverImage";
import type { Collection, LibraryStatus, NovelDetailsResponse } from "../types/novel";
import {
  estimateReadingMinutes,
  formatBytes,
  formatRelativeDate,
  languageLabel,
  sourceLabel,
} from "../utils/format";

interface NovelDetailsPageProps {
  novelId: string;
  defaultLanguage?: string;
  onBack: () => void;
  onSelectNovel?: (novelId: string) => void;
  onStartReading: (novelId: string, chapterId: string) => void;
  onSearchAuthor: (author: string) => void;
  onNotify: (msg: string, isError?: boolean) => void;
}

export const NovelDetailsPage: React.FC<NovelDetailsPageProps> = ({
  novelId,
  defaultLanguage = "en",
  onBack,
  onSelectNovel,
  onStartReading,
  onSearchAuthor,
  onNotify,
}) => {
  const [details, setDetails] = useState<NovelDetailsResponse | null>(null);
  const [collections, setCollections] = useState<Collection[]>([]);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const manualLangSwitchRef = useRef(false);
  const autoRedirectedRef = useRef<Set<string>>(new Set());

  const [sortAsc, setSortAsc] = useState(true);
  const [chapterFilter, setChapterFilter] = useState<"all" | "downloaded" | "unread">("all");
  const [chapterSearch, setChapterSearch] = useState("");
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [showShelfPicker, setShowShelfPicker] = useState(false);
  const [descExpanded, setDescExpanded] = useState(false);

  const latestNovelIdRef = useRef(novelId);
  latestNovelIdRef.current = novelId;

  const loadDetails = async (refreshRemote = false, silent = false) => {
    const requestedNovelId = novelId;
    if (refreshRemote) setRefreshing(true);
    else if (!silent) setLoading(true);
    if (!silent) setErrorMsg(null);
    try {
      const [det, cols] = await Promise.all([
        api.getNovelDetails(requestedNovelId, refreshRemote),
        api.listCollections(),
      ]);
      if (latestNovelIdRef.current !== requestedNovelId) return;
      setDetails(det);
      setCollections(cols);
    } catch (e) {
      if (latestNovelIdRef.current === requestedNovelId && !silent) {
        setErrorMsg(extractErrorMessage(e));
      }
    } finally {
      if (refreshRemote) setRefreshing(false);
      else if (!silent) setLoading(false);
    }
  };

  useEffect(() => {
    setSelectedIds(new Set());
    loadDetails(false);
  }, [novelId]);

  useEffect(() => {
    if (!details || !onSelectNovel || manualLangSwitchRef.current) return;
    if (autoRedirectedRef.current.has(details.novel.id)) return;
    const prefLang = defaultLanguage.trim().toLowerCase();
    if (!prefLang) return;
    const currentLang = String(
      (details.novel.metadata as Record<string, unknown> | undefined)?.language ?? "en",
    )
      .trim()
      .toLowerCase();
    if (currentLang === prefLang) return;
    const preferred = details.translations.find(
      (t) =>
        t.language_code.toLowerCase() === prefLang && t.novel_id !== details.novel.id,
    );
    if (preferred) {
      autoRedirectedRef.current.add(details.novel.id);
      autoRedirectedRef.current.add(preferred.novel_id);
      onSelectNovel(preferred.novel_id);
    }
  }, [details, defaultLanguage, onSelectNovel]);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    api
      .onDownloadUpdated((task) => {
        if (
          task.novel_id === novelId &&
          (task.status === "completed" ||
            task.status === "failed" ||
            task.status === "cancelled" ||
            task.completed_chapters > 0)
        ) {
          loadDetails(false, true);
        }
      })
      .then((fn) => {
        unlisten = fn;
      })
      .catch(() => {});

    return () => {
      if (unlisten) unlisten();
    };
  }, [novelId]);

  const filteredChapters = useMemo(() => {
    if (!details) return [];
    const q = chapterSearch.trim().toLowerCase();
    const curChNum = details.reading_progress?.chapter_number ?? 0;
    const list = details.chapters.filter((ch) => {
      if (chapterFilter === "downloaded" && !ch.is_downloaded) return false;
      if (chapterFilter === "unread" && ch.chapter_number <= curChNum) return false;
      if (q) {
        const hit =
          ch.title.toLowerCase().includes(q) ||
          String(Math.round(ch.chapter_number)).includes(q);
        if (!hit) return false;
      }
      return true;
    });
    list.sort((a, b) =>
      sortAsc ? a.chapter_number - b.chapter_number : b.chapter_number - a.chapter_number,
    );
    return list;
  }, [details, sortAsc, chapterFilter, chapterSearch]);

  if (loading) {
    return (
      <div className="p-8 max-w-6xl mx-auto flex items-center justify-center gap-2 text-sm text-[var(--text-secondary)]">
        <Loader2 className="w-4 h-4 animate-spin" />
        Loading novel metadata and chapters...
      </div>
    );
  }

  if (errorMsg || !details) {
    return (
      <div className="p-8 max-w-3xl mx-auto space-y-4">
        <button
          type="button"
          onClick={onBack}
          className="inline-flex items-center gap-1.5 text-xs text-[var(--text-secondary)] hover:text-[var(--text-primary)]"
        >
          <ArrowLeft className="w-4 h-4" /> Back
        </button>
        <div className="p-5 rounded-xl border border-rose-500/30 bg-rose-500/10 text-sm text-rose-200">
          {errorMsg || "Novel could not be loaded."}
        </div>
      </div>
    );
  }

  const { novel, chapters, library_entry, reading_progress, bookmarks } = details;
  const resumeChapterId = reading_progress?.chapter_id || chapters[0]?.id;

  const handleSetStatus = async (status: LibraryStatus) => {
    try {
      await api.updateLibraryEntry(novel.id, status);
      onNotify(`Saved to library (${status.replace("_", " ")})`);
      await loadDetails(false);
    } catch (e) {
      onNotify(extractErrorMessage(e), true);
    }
  };

  const handleToggleBookmark = async () => {
    try {
      const next = !(library_entry?.is_bookmarked ?? false);
      await api.updateLibraryEntry(
        novel.id,
        library_entry?.status || "reading",
        next,
        undefined,
      );
      if (next) {
        const hasNovelBm = (bookmarks || []).some((b) => b.is_novel_bookmark);
        if (!hasNovelBm) {
          await api.addBookmark({ novelId: novel.id, isNovelBookmark: true });
        }
      } else {
        const freshBookmarks =
          typeof api.listBookmarks === "function"
            ? (await api.listBookmarks(novel.id)) ?? bookmarks ?? []
            : bookmarks || [];
        const novelBookmarks = freshBookmarks.filter((b) => b.is_novel_bookmark);
        await Promise.all(novelBookmarks.map((b) => api.deleteBookmark(b.id)));
      }
      onNotify(next ? "Novel bookmarked" : "Bookmark removed");
      await loadDetails(false, true);
    } catch (e) {
      onNotify(extractErrorMessage(e), true);
    }
  };

  const handleToggleFollow = async () => {
    try {
      const next = !(library_entry?.follow_updates ?? false);
      await api.updateLibraryEntry(
        novel.id,
        library_entry?.status || "reading",
        undefined,
        next,
      );
      onNotify(next ? "Following for chapter updates" : "Stopped following updates");
      await loadDetails(false);
    } catch (e) {
      onNotify(extractErrorMessage(e), true);
    }
  };

  const handleToggleCollection = async (colId: string) => {
    try {
      const present = !details.collection_ids.includes(colId);
      if (!library_entry) {
        await api.updateLibraryEntry(novel.id, "reading");
      }
      await api.setNovelInCollection(colId, novel.id, present);
      await loadDetails(false);
    } catch (e) {
      onNotify(extractErrorMessage(e), true);
    }
  };

  const handleDownload = async (ids: string[]) => {
    try {
      const task = await api.enqueueDownload(novel.id, ids);
      onNotify(`Queued ${task.total_chapters} chapter(s) for offline download`);
    } catch (e) {
      onNotify(extractErrorMessage(e), true);
    }
  };

  const handleDeleteAllDownloads = async () => {
    try {
      const count = await api.deleteDownloadedNovel(novel.id);
      onNotify(`Removed ${count} downloaded chapter file(s) — reading progress preserved`);
      await loadDetails(false);
    } catch (e) {
      onNotify(extractErrorMessage(e), true);
    }
  };

  const handleDeleteSelectedDownloads = async () => {
    try {
      const ids = Array.from(selectedIds);
      const count = await api.deleteDownloadedChapters(ids);
      setSelectedIds(new Set());
      onNotify(`Removed ${count} downloaded chapter file(s)`);
      await loadDetails(false);
    } catch (e) {
      onNotify(extractErrorMessage(e), true);
    }
  };

  const toggleSelect = (id: string) => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const selectAllVisible = () => {
    if (selectedIds.size === filteredChapters.length) {
      setSelectedIds(new Set());
    } else {
      setSelectedIds(new Set(filteredChapters.map((c) => c.id)));
    }
  };

  return (
    <div className="p-6 max-w-6xl mx-auto space-y-6">
      {/* Top Bar */}
      <div className="flex items-center justify-between">
        <button
          type="button"
          onClick={onBack}
          className="inline-flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg text-xs text-[var(--text-secondary)] hover:text-[var(--text-primary)] hover:bg-[var(--bg-surface)] transition-colors"
        >
          <ArrowLeft className="w-4 h-4" />
          Back
        </button>

        {!novel.is_local && (
          <button
            type="button"
            onClick={() => loadDetails(true)}
            disabled={refreshing}
            className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs border border-[var(--border-subtle)] bg-[var(--bg-surface)] hover:bg-[var(--bg-elevated)] text-[var(--text-secondary)]"
          >
            <RefreshCw className={`w-3.5 h-3.5 ${refreshing ? "animate-spin" : ""}`} />
            Refresh Metadata & Chapters
          </button>
        )}
      </div>

      {/* Hero Section */}
      <div className="flex flex-col md:flex-row gap-6 p-6 rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-surface)]">
        <div className="w-44 sm:w-52 aspect-[2/3] rounded-xl overflow-hidden shadow-lg shrink-0 mx-auto md:mx-0">
          <CoverImage
            title={novel.title}
            author={novel.author}
            novelId={novel.id}
            coverUrl={novel.cover_url}
            localCoverPath={novel.local_cover_path}
            size="lg"
            className="w-full h-full"
          />
        </div>

        <div className="flex-1 min-w-0 space-y-4">
          <div>
            <div className="flex flex-wrap items-center gap-2 text-xs mb-2">
              <span className="px-2 py-0.5 rounded-md bg-[var(--accent-soft)] text-[var(--accent-hover)] font-medium border border-[var(--accent-border)]">
                {sourceLabel(novel.source_id)}
              </span>
              <span className="px-2 py-0.5 rounded-md bg-[var(--bg-elevated)] text-[var(--text-secondary)] capitalize border border-[var(--border-subtle)]">
                {novel.status}
              </span>
              {novel.rating !== null && (
                <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-md bg-amber-500/10 text-amber-300 border border-amber-500/20">
                  <Star className="w-3 h-3 fill-current" />
                  {novel.rating.toFixed(1)}
                </span>
              )}
              {details.downloaded_chapters > 0 && (
                <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-md bg-emerald-500/10 text-emerald-300 border border-emerald-500/20">
                  <Download className="w-3 h-3" />
                  {details.downloaded_chapters}/{chapters.length} offline (
                  {formatBytes(details.total_size_bytes)})
                </span>
              )}
            </div>

            <h1 className="text-2xl sm:text-3xl font-semibold tracking-tight text-[var(--text-primary)]">
              {novel.title}
            </h1>
            <button
              type="button"
              onClick={() => onSearchAuthor(novel.author)}
              className="mt-1 text-sm text-[var(--accent-hover)] hover:underline"
            >
              {novel.author}
            </button>
          </div>

          <div className="flex flex-wrap items-center gap-4 text-xs text-[var(--text-secondary)]">
            <span>
              <strong className="text-[var(--text-primary)]">{chapters.length}</strong> Chapters
            </span>
            <span>
              Updated{" "}
              <strong className="text-[var(--text-primary)]">
                {formatRelativeDate(novel.last_updated_remote || novel.updated_at)}
              </strong>
            </span>
            {reading_progress && (
              <span>
                Progress:{" "}
                <strong className="text-[var(--accent-hover)]">
                  Ch. {Math.round(reading_progress.chapter_number)} (
                  {Math.round(reading_progress.progress_percent)}%)
                </strong>
              </span>
            )}
          </div>

          {/* Genres & Tags */}
          {(novel.genres.length > 0 || novel.tags.length > 0) && (
            <div className="flex flex-wrap gap-1.5">
              {novel.genres.map((g) => (
                <span
                  key={g}
                  className="px-2.5 py-0.5 rounded-full text-[11px] bg-[var(--bg-elevated)] text-[var(--text-secondary)] border border-[var(--border-subtle)]"
                >
                  {g}
                </span>
              ))}
              {novel.tags.slice(0, 4).map((t) => (
                <span
                  key={t}
                  className="px-2 py-0.5 rounded-full text-[11px] text-[var(--text-muted)] border border-[var(--border-subtle)]"
                >
                  {t}
                </span>
              ))}
            </div>
          )}

          {/* Description */}
          <div className="text-sm text-[var(--text-secondary)] leading-relaxed">
            <p className={descExpanded ? "whitespace-pre-line" : "line-clamp-4 whitespace-pre-line"}>
              {novel.description || "No description provided by source."}
            </p>
            {novel.description && novel.description.length > 260 && (
              <button
                type="button"
                onClick={() => setDescExpanded(!descExpanded)}
                className="mt-1 text-xs text-[var(--accent-hover)] hover:underline"
              >
                {descExpanded ? "Show less" : "Read more"}
              </button>
            )}
          </div>

          {/* Primary Actions */}
          <div className="flex flex-wrap items-center gap-2.5 pt-2">
            {resumeChapterId && (
              <button
                type="button"
                onClick={() => onStartReading(novel.id, resumeChapterId)}
                className="inline-flex items-center gap-2 px-4 py-2.5 rounded-xl text-sm font-medium text-white shadow-sm transition-transform active:scale-[0.99]"
                style={{ backgroundColor: "var(--accent)" }}
              >
                <BookOpen className="w-4 h-4" />
                {reading_progress
                  ? `Continue Reading (Ch. ${Math.round(reading_progress.chapter_number)})`
                  : "Start Reading"}
              </button>
            )}

            <select
              aria-label="Library shelf"
              value={library_entry?.status || ""}
              onChange={(e) => {
                if (e.target.value) handleSetStatus(e.target.value as LibraryStatus);
              }}
              className="px-3 py-2.5 rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-elevated)] text-xs font-medium text-[var(--text-primary)] focus:outline-none"
            >
              <option value="" disabled>
                + Add to Library
              </option>
              <option value="reading">Currently Reading</option>
              <option value="plan_to_read">Plan to Read</option>
              <option value="finished">Finished</option>
              <option value="on_hold">On Hold</option>
              <option value="dropped">Dropped</option>
            </select>

            <button
              type="button"
              onClick={handleToggleBookmark}
              className={`inline-flex items-center gap-1.5 px-3 py-2.5 rounded-xl text-xs font-medium border transition-colors ${
                library_entry?.is_bookmarked
                  ? "bg-[var(--accent-soft)] border-[var(--accent-border)] text-[var(--accent-hover)]"
                  : "border-[var(--border-subtle)] bg-[var(--bg-elevated)] text-[var(--text-secondary)] hover:text-[var(--text-primary)]"
              }`}
            >
              <Bookmark className="w-3.5 h-3.5" />
              {library_entry?.is_bookmarked ? "Bookmarked" : "Bookmark"}
            </button>

            {details.translations.length > 0 && (
              <label className="inline-flex items-center gap-1.5 px-3 py-2 rounded-xl text-xs font-medium border border-[var(--border-subtle)] bg-[var(--bg-elevated)] text-[var(--text-primary)]">
                <Languages className="w-3.5 h-3.5 text-[var(--accent-hover)] shrink-0" />
                <select
                  aria-label="Translation language"
                  value={novel.id}
                  onChange={(e) => {
                    const targetId = e.target.value;
                    if (targetId && targetId !== novel.id && onSelectNovel) {
                      manualLangSwitchRef.current = true;
                      onSelectNovel(targetId);
                    }
                  }}
                  className="bg-transparent focus:outline-none cursor-pointer"
                >
                  {details.translations.map((tr) => (
                    <option key={tr.novel_id} value={tr.novel_id}>
                      {languageLabel(tr.language_code, tr.language_label)} — {tr.title}
                    </option>
                  ))}
                </select>
              </label>
            )}

            {!novel.is_local && (
              <button
                type="button"
                onClick={handleToggleFollow}
                className={`inline-flex items-center gap-1.5 px-3 py-2.5 rounded-xl text-xs font-medium border transition-colors ${
                  library_entry?.follow_updates
                    ? "bg-[var(--accent-soft)] border-[var(--accent-border)] text-[var(--accent-hover)]"
                    : "border-[var(--border-subtle)] bg-[var(--bg-elevated)] text-[var(--text-secondary)] hover:text-[var(--text-primary)]"
                }`}
              >
                <Bell className="w-3.5 h-3.5" />
                {library_entry?.follow_updates ? "Following" : "Follow Updates"}
              </button>
            )}

            <div className="relative">
              <button
                type="button"
                onClick={() => setShowShelfPicker(!showShelfPicker)}
                className="inline-flex items-center gap-1.5 px-3 py-2.5 rounded-xl text-xs font-medium border border-[var(--border-subtle)] bg-[var(--bg-elevated)] text-[var(--text-secondary)] hover:text-[var(--text-primary)]"
              >
                <FolderPlus className="w-3.5 h-3.5" />
                Shelves ({details.collection_ids.length})
              </button>
              {showShelfPicker && (
                <div className="absolute left-0 mt-1.5 w-52 p-2 rounded-xl border border-[var(--border-strong)] bg-[var(--bg-surface)] shadow-xl z-20 space-y-1">
                  {collections.length === 0 ? (
                    <div className="text-xs text-[var(--text-muted)] p-2">
                      No custom shelves yet. Create one in Library.
                    </div>
                  ) : (
                    collections.map((c) => {
                      const active = details.collection_ids.includes(c.id);
                      return (
                        <button
                          key={c.id}
                          type="button"
                          onClick={() => handleToggleCollection(c.id)}
                          className="w-full flex items-center justify-between px-2.5 py-1.5 rounded-lg text-xs hover:bg-[var(--bg-hover)]"
                        >
                          <span className="truncate">{c.name}</span>
                          {active && <Check className="w-3.5 h-3.5 text-[var(--accent-hover)]" />}
                        </button>
                      );
                    })
                  )}
                </div>
              )}
            </div>

            {!novel.is_local && chapters.some((c) => !c.is_downloaded) && (
              <button
                type="button"
                onClick={() => handleDownload([])}
                className="inline-flex items-center gap-1.5 px-3 py-2.5 rounded-xl text-xs font-medium border border-[var(--border-subtle)] bg-[var(--bg-elevated)] hover:border-[var(--accent)] text-[var(--text-primary)]"
              >
                <Download className="w-3.5 h-3.5" />
                Download All ({chapters.filter((c) => !c.is_downloaded).length})
              </button>
            )}

            {details.downloaded_chapters > 0 && !novel.is_local && (
              <button
                type="button"
                onClick={handleDeleteAllDownloads}
                className="inline-flex items-center gap-1.5 px-3 py-2.5 rounded-xl text-xs font-medium border border-rose-500/30 bg-rose-500/10 text-rose-300 hover:bg-rose-500/20"
              >
                <Trash2 className="w-3.5 h-3.5" />
                Delete Downloads
              </button>
            )}
          </div>
        </div>
      </div>

      {/* Chapters Section */}
      <div className="rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-surface)] overflow-hidden">
        <div className="p-4 border-b border-[var(--border-subtle)] flex flex-col sm:flex-row sm:items-center justify-between gap-3">
          <div className="flex items-center gap-3">
            <h2 className="text-base font-semibold">
              Chapters ({filteredChapters.length})
            </h2>
            {selectedIds.size > 0 && (
              <div className="flex items-center gap-2">
                <button
                  type="button"
                  onClick={() => handleDownload(Array.from(selectedIds))}
                  className="inline-flex items-center gap-1 px-2.5 py-1 rounded-md text-xs font-medium text-white"
                  style={{ backgroundColor: "var(--accent)" }}
                >
                  <Download className="w-3 h-3" />
                  Download ({selectedIds.size})
                </button>
                <button
                  type="button"
                  onClick={handleDeleteSelectedDownloads}
                  className="inline-flex items-center gap-1 px-2.5 py-1 rounded-md text-xs font-medium bg-rose-500/15 text-rose-300"
                >
                  <Trash2 className="w-3 h-3" />
                  Delete Files
                </button>
              </div>
            )}
          </div>

          <div className="flex flex-wrap items-center gap-2">
            <div className="relative">
              <Search className="w-3.5 h-3.5 text-[var(--text-muted)] absolute left-2.5 top-1/2 -translate-y-1/2" />
              <input
                value={chapterSearch}
                onChange={(e) => setChapterSearch(e.target.value)}
                placeholder="Search chapter..."
                className="pl-8 pr-3 py-1.5 w-44 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-elevated)] text-xs focus:outline-none focus:border-[var(--accent)]"
              />
            </div>

            <select
              aria-label="Filter chapters"
              value={chapterFilter}
              onChange={(e) => setChapterFilter(e.target.value as typeof chapterFilter)}
              className="px-2.5 py-1.5 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-elevated)] text-xs focus:outline-none"
            >
              <option value="all">All Chapters</option>
              <option value="unread">Unread Only</option>
              <option value="downloaded">Downloaded Only</option>
            </select>

            <button
              type="button"
              onClick={() => setSortAsc(!sortAsc)}
              className="inline-flex items-center gap-1 px-2.5 py-1.5 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-elevated)] text-xs"
            >
              <ArrowDownUp className="w-3.5 h-3.5" />
              {sortAsc ? "Oldest first" : "Newest first"}
            </button>

            <button
              type="button"
              onClick={selectAllVisible}
              className="inline-flex items-center gap-1 px-2.5 py-1.5 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-elevated)] text-xs"
            >
              {selectedIds.size === filteredChapters.length && filteredChapters.length > 0
                ? "Unselect all"
                : "Select all"}
            </button>
          </div>
        </div>

        <div className="divide-y divide-[var(--border-subtle)] max-h-[540px] overflow-y-auto">
          {filteredChapters.length === 0 ? (
            <div className="p-8 text-center text-xs text-[var(--text-muted)]">
              No chapters match the current filter.
            </div>
          ) : (
            filteredChapters.map((ch) => {
              const isCurrent = reading_progress?.chapter_id === ch.id;
              const isRead =
                reading_progress && ch.chapter_number < reading_progress.chapter_number;
              const isSelected = selectedIds.has(ch.id);

              return (
                <div
                  key={ch.id}
                  className={`flex items-center gap-3 px-4 py-2.5 hover:bg-[var(--bg-hover)] transition-colors ${
                    isCurrent ? "bg-[var(--accent-soft)]/40" : ""
                  }`}
                >
                  <button
                    type="button"
                    aria-label={`Select ${ch.title}`}
                    onClick={() => toggleSelect(ch.id)}
                    className="text-[var(--text-muted)] hover:text-[var(--text-primary)]"
                  >
                    {isSelected ? (
                      <CheckSquare className="w-4 h-4 text-[var(--accent-hover)]" />
                    ) : (
                      <Square className="w-4 h-4" />
                    )}
                  </button>

                  <button
                    type="button"
                    onClick={() => onStartReading(novel.id, ch.id)}
                    className="flex-1 min-w-0 text-left flex items-center justify-between gap-3"
                  >
                    <div className="min-w-0">
                      <div
                        className={`text-sm truncate ${
                          isRead
                            ? "text-[var(--text-muted)]"
                            : isCurrent
                              ? "text-[var(--accent-hover)] font-medium"
                              : "text-[var(--text-primary)]"
                        }`}
                      >
                        {ch.title}
                      </div>
                      <div className="text-[11px] text-[var(--text-muted)] flex items-center gap-2">
                        <span>Chapter {Math.round(ch.chapter_number)}</span>
                        {ch.word_count > 0 && (
                          <>
                            <span>·</span>
                            <span>{ch.word_count.toLocaleString()} words</span>
                            <span>·</span>
                            <span>~{estimateReadingMinutes(ch.word_count)} min</span>
                          </>
                        )}
                      </div>
                    </div>

                    <div className="flex items-center gap-2 shrink-0">
                      {isCurrent && (
                        <span className="px-2 py-0.5 rounded text-[10px] font-medium bg-[var(--accent-soft)] text-[var(--accent-hover)]">
                          Reading
                        </span>
                      )}
                      {ch.is_downloaded && (
                        <span
                          title={`Downloaded (${formatBytes(ch.size_bytes)})`}
                          className="p-1 rounded-md bg-emerald-500/10 text-emerald-400"
                        >
                          <Download className="w-3.5 h-3.5" />
                        </span>
                      )}
                    </div>
                  </button>

                  {!ch.is_downloaded && !novel.is_local && (
                    <button
                      type="button"
                      title="Download chapter"
                      onClick={() => handleDownload([ch.id])}
                      className="p-1.5 rounded-lg text-[var(--text-muted)] hover:text-[var(--text-primary)] hover:bg-[var(--bg-elevated)]"
                    >
                      <Download className="w-3.5 h-3.5" />
                    </button>
                  )}
                </div>
              );
            })
          )}
        </div>
      </div>
    </div>
  );
};
