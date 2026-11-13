import React, { useEffect, useMemo, useState } from "react";
import {
  Bell,
  BookOpen,
  Download,
  FolderPlus,
  Grid,
  List,
  Plus,
  RefreshCw,
  Search,
  Trash2,
  Upload,
} from "lucide-react";
import { api, extractErrorMessage } from "../api/tauri";
import { CoverImage } from "../components/CoverImage";
import { LibraryNovelCard } from "../components/NovelCard";
import type { Collection, LibraryNovelItem, UpdateCheckResult } from "../types/novel";
import { APP_LANGUAGES, formatBytes, formatRelativeDate, languageLabel, libraryStatusLabel } from "../utils/format";

interface LibraryPageProps {
  onOpenNovel: (novelId: string) => void;
  onStartReading: (novelId: string, chapterId?: string) => void;
  onNotify: (msg: string, isError?: boolean) => void;
}

type ShelfFilter =
  | "all"
  | "reading"
  | "bookmarked"
  | "plan_to_read"
  | "finished"
  | "downloaded";

export const LibraryPage: React.FC<LibraryPageProps> = ({
  onOpenNovel,
  onStartReading,
  onNotify,
}) => {
  const [items, setItems] = useState<LibraryNovelItem[]>([]);
  const [collections, setCollections] = useState<Collection[]>([]);
  const [loading, setLoading] = useState(true);
  const [shelf, setShelf] = useState<ShelfFilter>("all");
  const [selectedCollection, setSelectedCollection] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  const [languageFilter, setLanguageFilter] = useState("all");
  const [sortBy, setSortBy] = useState<"last_read" | "title" | "progress" | "added" | "chapters">("last_read");
  const [viewMode, setViewMode] = useState<"grid" | "list">("grid");
  const [newCollectionName, setNewCollectionName] = useState("");
  const [creatingCol, setCreatingCol] = useState(false);
  const [checkingUpdates, setCheckingUpdates] = useState(false);
  const [importing, setImporting] = useState(false);

  const loadData = async () => {
    setLoading(true);
    try {
      const [lib, cols] = await Promise.all([api.listLibrary(), api.listCollections()]);
      setItems(lib);
      setCollections(cols);
    } catch (e) {
      onNotify(extractErrorMessage(e), true);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    loadData();
  }, []);

  const libraryLanguages = useMemo(() => {
    const codes = new Set<string>(APP_LANGUAGES.map((l) => l.code));
    for (const it of items) {
      const raw = (it.novel.metadata as Record<string, unknown> | undefined)?.language;
      if (typeof raw === "string" && raw.trim()) {
        codes.add(raw.trim().toLowerCase().split("-")[0]);
      }
    }
    return Array.from(codes);
  }, [items]);

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase();
    const list = items.filter((it) => {
      if (selectedCollection && !it.collection_ids.includes(selectedCollection)) {
        return false;
      }
      if (!selectedCollection) {
        if (shelf === "reading" && it.entry.status !== "reading") return false;
        if (shelf === "plan_to_read" && it.entry.status !== "plan_to_read") return false;
        if (shelf === "finished" && it.entry.status !== "finished") return false;
        if (shelf === "bookmarked" && !it.entry.is_bookmarked) return false;
        if (shelf === "downloaded" && it.downloaded_chapters === 0) return false;
      }
      if (languageFilter !== "all") {
        const raw = (it.novel.metadata as Record<string, unknown> | undefined)?.language;
        const lang = typeof raw === "string" ? raw.trim().toLowerCase() : "";
        if (!lang || (lang !== languageFilter && !lang.startsWith(`${languageFilter}-`))) {
          return false;
        }
      }
      if (q) {
        const hit =
          it.novel.title.toLowerCase().includes(q) ||
          it.novel.author.toLowerCase().includes(q) ||
          it.novel.genres.some((g) => g.toLowerCase().includes(q));
        if (!hit) return false;
      }
      return true;
    });

    list.sort((a, b) => {
      switch (sortBy) {
        case "title":
          return a.novel.title.localeCompare(b.novel.title);
        case "progress":
          return (b.progress?.progress_percent ?? 0) - (a.progress?.progress_percent ?? 0);
        case "added":
          return b.entry.added_at.localeCompare(a.entry.added_at);
        case "chapters":
          return b.novel.chapter_count - a.novel.chapter_count;
        default: {
          const ta = a.progress?.updated_at || a.entry.updated_at;
          const tb = b.progress?.updated_at || b.entry.updated_at;
          return tb.localeCompare(ta);
        }
      }
    });
    return list;
  }, [items, shelf, selectedCollection, languageFilter, search, sortBy]);

  const handleImportFiles = async () => {
    try {
      const files = await api.pickBookFiles();
      if (files.length === 0) return;
      setImporting(true);
      let importedCount = 0;
      for (const f of files) {
        await api.importLocalBook(f);
        importedCount++;
      }
      onNotify(`Imported ${importedCount} book${importedCount === 1 ? "" : "s"} into your library`);
      await loadData();
    } catch (e) {
      onNotify(extractErrorMessage(e), true);
    } finally {
      setImporting(false);
    }
  };

  const handleCreateCollection = async (e: React.FormEvent) => {
    e.preventDefault();
    const name = newCollectionName.trim();
    if (!name) return;
    try {
      await api.createCollection(name);
      setNewCollectionName("");
      setCreatingCol(false);
      await loadData();
      onNotify(`Created shelf "${name}"`);
    } catch (err) {
      onNotify(extractErrorMessage(err), true);
    }
  };

  const handleDeleteCollection = async (col: Collection) => {
    try {
      await api.deleteCollection(col.id);
      if (selectedCollection === col.id) setSelectedCollection(null);
      await loadData();
    } catch (err) {
      onNotify(extractErrorMessage(err), true);
    }
  };

  const handleCheckUpdates = async () => {
    setCheckingUpdates(true);
    try {
      const res: UpdateCheckResult[] = await api.checkFollowedUpdates();
      const totalNew = res.reduce((acc, r) => acc + r.new_chapters, 0);
      if (res.length === 0) {
        onNotify("No novels are marked 'Follow Updates' yet");
      } else if (totalNew > 0) {
        onNotify(`Found ${totalNew} new chapter(s) across ${res.length} followed novel(s)`);
        await loadData();
      } else {
        onNotify(`Checked ${res.length} followed novel(s) — all up to date`);
      }
    } catch (err) {
      onNotify(extractErrorMessage(err), true);
    } finally {
      setCheckingUpdates(false);
    }
  };

  return (
    <div className="p-6 max-w-[1440px] mx-auto space-y-5">
      {/* Header */}
      <div className="flex flex-col md:flex-row md:items-center justify-between gap-4">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">Personal Library</h1>
          <p className="text-sm text-[var(--text-secondary)] mt-0.5">
            {items.length} novel{items.length === 1 ? "" : "s"} organized across shelves and offline storage.
          </p>
        </div>

        <div className="flex flex-wrap items-center gap-2">
          <button
            type="button"
            onClick={handleCheckUpdates}
            disabled={checkingUpdates}
            className="inline-flex items-center gap-1.5 px-3 py-2 rounded-lg text-xs font-medium border border-[var(--border-subtle)] bg-[var(--bg-surface)] hover:bg-[var(--bg-elevated)] transition-colors"
          >
            <RefreshCw className={`w-3.5 h-3.5 ${checkingUpdates ? "animate-spin" : ""}`} />
            Check Followed Updates
          </button>

          <button
            type="button"
            onClick={handleImportFiles}
            disabled={importing}
            className="inline-flex items-center gap-1.5 px-3.5 py-2 rounded-lg text-xs font-medium text-white transition-colors"
            style={{ backgroundColor: "var(--accent)" }}
          >
            <Upload className="w-3.5 h-3.5" />
            {importing ? "Importing..." : "Import EPUB / TXT / HTML"}
          </button>
        </div>
      </div>

      {/* Shelves & Controls */}
      <div className="flex flex-col lg:flex-row lg:items-center justify-between gap-3 pb-3 border-b border-[var(--border-subtle)]">
        <div className="flex flex-wrap items-center gap-1.5">
          {(
            [
              ["all", "All"],
              ["reading", "Currently Reading"],
              ["bookmarked", "Bookmarked"],
              ["plan_to_read", "Plan to Read"],
              ["finished", "Finished"],
              ["downloaded", "Downloaded"],
            ] as [ShelfFilter, string][]
          ).map(([key, label]) => (
            <button
              key={key}
              type="button"
              onClick={() => {
                setSelectedCollection(null);
                setShelf(key);
              }}
              className={`px-3 py-1.5 rounded-lg text-xs font-medium transition-colors ${
                !selectedCollection && shelf === key
                  ? "text-white"
                  : "bg-[var(--bg-surface)] text-[var(--text-secondary)] border border-[var(--border-subtle)] hover:text-[var(--text-primary)]"
              }`}
              style={
                !selectedCollection && shelf === key
                  ? { backgroundColor: "var(--accent)" }
                  : undefined
              }
            >
              {label}
            </button>
          ))}
        </div>

        <div className="flex flex-wrap items-center gap-2">
          <div className="relative">
            <Search className="w-3.5 h-3.5 text-[var(--text-muted)] absolute left-2.5 top-1/2 -translate-y-1/2" />
            <input
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder="Filter library..."
              className="pl-8 pr-3 py-1.5 w-48 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-surface)] text-xs focus:outline-none focus:border-[var(--accent)]"
            />
          </div>

          <select
            aria-label="Filter library by language"
            value={languageFilter}
            onChange={(e) => setLanguageFilter(e.target.value)}
            className="px-2.5 py-1.5 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-surface)] text-xs focus:outline-none"
          >
            <option value="all">All Languages</option>
            {libraryLanguages.map((code) => (
              <option key={code} value={code}>
                {languageLabel(code)}
              </option>
            ))}
          </select>

          <select
            aria-label="Sort library"
            value={sortBy}
            onChange={(e) => setSortBy(e.target.value as typeof sortBy)}
            className="px-2.5 py-1.5 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-surface)] text-xs focus:outline-none"
          >
            <option value="last_read">Last Read</option>
            <option value="title">Title (A–Z)</option>
            <option value="progress">Progress</option>
            <option value="added">Date Added</option>
            <option value="chapters">Chapters</option>
          </select>

          <div className="flex rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-surface)] overflow-hidden">
            <button
              type="button"
              aria-label="Grid view"
              onClick={() => setViewMode("grid")}
              className={`p-1.5 ${viewMode === "grid" ? "bg-[var(--bg-elevated)] text-[var(--text-primary)]" : "text-[var(--text-muted)]"}`}
            >
              <Grid className="w-3.5 h-3.5" />
            </button>
            <button
              type="button"
              aria-label="List view"
              onClick={() => setViewMode("list")}
              className={`p-1.5 ${viewMode === "list" ? "bg-[var(--bg-elevated)] text-[var(--text-primary)]" : "text-[var(--text-muted)]"}`}
            >
              <List className="w-3.5 h-3.5" />
            </button>
          </div>
        </div>
      </div>

      {/* Custom Collections Bar */}
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-[11px] font-medium uppercase tracking-wider text-[var(--text-muted)] mr-1">
          Custom Shelves:
        </span>
        {collections.map((col) => (
          <div
            key={col.id}
            className={`group inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs border transition-colors ${
              selectedCollection === col.id
                ? "bg-[var(--accent-soft)] border-[var(--accent-border)] text-[var(--accent-hover)] font-medium"
                : "bg-[var(--bg-surface)] border-[var(--border-subtle)] text-[var(--text-secondary)]"
            }`}
          >
            <button
              type="button"
              onClick={() =>
                setSelectedCollection(selectedCollection === col.id ? null : col.id)
              }
              className="inline-flex items-center gap-1.5"
            >
              <span
                className="w-2 h-2 rounded-full"
                style={{ backgroundColor: col.color || "#6366F1" }}
              />
              {col.name}
              <span className="opacity-60 text-[10px]">({col.item_count})</span>
            </button>
            <button
              type="button"
              title={`Delete ${col.name}`}
              onClick={() => handleDeleteCollection(col)}
              className="opacity-0 group-hover:opacity-100 hover:text-rose-400 transition-opacity ml-0.5"
            >
              <Trash2 className="w-3 h-3" />
            </button>
          </div>
        ))}

        {creatingCol ? (
          <form onSubmit={handleCreateCollection} className="inline-flex items-center gap-1.5">
            <input
              autoFocus
              value={newCollectionName}
              onChange={(e) => setNewCollectionName(e.target.value)}
              placeholder="Shelf name..."
              className="px-2.5 py-1 rounded-full border border-[var(--accent)] bg-[var(--bg-surface)] text-xs w-36 focus:outline-none"
            />
            <button
              type="submit"
              className="px-2.5 py-1 rounded-full text-xs text-white"
              style={{ backgroundColor: "var(--accent)" }}
            >
              Add
            </button>
            <button
              type="button"
              onClick={() => setCreatingCol(false)}
              className="text-xs text-[var(--text-muted)] px-1"
            >
              Cancel
            </button>
          </form>
        ) : (
          <button
            type="button"
            onClick={() => setCreatingCol(true)}
            className="inline-flex items-center gap-1 px-2.5 py-1 rounded-full text-xs border border-dashed border-[var(--border-strong)] text-[var(--text-muted)] hover:text-[var(--text-primary)]"
          >
            <FolderPlus className="w-3 h-3" />
            New Shelf
          </button>
        )}
      </div>

      {/* Library Grid or List */}
      {loading ? (
        <div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-6 gap-4">
          {Array.from({ length: 6 }).map((_, i) => (
            <div
              key={i}
              className="aspect-[2/3] rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-surface)] animate-pulse"
            />
          ))}
        </div>
      ) : filtered.length === 0 ? (
        <div className="py-16 text-center rounded-xl border border-dashed border-[var(--border-strong)] bg-[var(--bg-surface)]/40 space-y-2">
          <p className="text-sm font-medium">No novels in this shelf</p>
          <p className="text-xs text-[var(--text-secondary)]">
            Add novels from Discover or import local EPUB, TXT, or HTML books.
          </p>
          <button
            type="button"
            onClick={handleImportFiles}
            className="mt-2 inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium bg-[var(--bg-elevated)] border border-[var(--border-subtle)] hover:border-[var(--accent)]"
          >
            <Plus className="w-3.5 h-3.5" /> Import Book
          </button>
        </div>
      ) : viewMode === "grid" ? (
        <div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6 gap-4">
          {filtered.map((item) => (
            <LibraryNovelCard
              key={item.novel.id}
              item={item}
              onOpen={onOpenNovel}
              onContinue={onStartReading}
            />
          ))}
        </div>
      ) : (
        <div className="rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-surface)] divide-y divide-[var(--border-subtle)] overflow-hidden">
          {filtered.map((item) => {
            const pct = Math.round(item.progress?.progress_percent ?? 0);
            return (
              <div
                key={item.novel.id}
                className="flex items-center gap-4 px-4 py-3 hover:bg-[var(--bg-hover)] transition-colors"
              >
                <button
                  type="button"
                  onClick={() => onOpenNovel(item.novel.id)}
                  className="w-11 h-16 rounded-md overflow-hidden shrink-0"
                >
                  <CoverImage
                    title={item.novel.title}
                    coverUrl={item.novel.cover_url}
                    localCoverPath={item.novel.local_cover_path}
                    size="sm"
                    className="w-full h-full"
                  />
                </button>
                <div className="min-w-0 flex-1">
                  <button
                    type="button"
                    onClick={() => onOpenNovel(item.novel.id)}
                    className="text-left"
                  >
                    <h3 className="text-sm font-medium hover:text-[var(--accent-hover)] truncate">
                      {item.novel.title}
                    </h3>
                  </button>
                  <div className="text-xs text-[var(--text-secondary)] flex flex-wrap items-center gap-2 mt-0.5">
                    <span>{item.novel.author}</span>
                    <span>·</span>
                    <span>{libraryStatusLabel(item.entry.status)}</span>
                    {item.entry.follow_updates && (
                      <span className="inline-flex items-center gap-0.5 text-[var(--accent-hover)]">
                        <Bell className="w-3 h-3" /> Following
                      </span>
                    )}
                    {item.downloaded_chapters > 0 && (
                      <span className="inline-flex items-center gap-0.5 text-emerald-400">
                        <Download className="w-3 h-3" /> {item.downloaded_chapters}/
                        {item.novel.chapter_count} ({formatBytes(item.total_size_bytes)})
                      </span>
                    )}
                  </div>
                </div>
                <div className="hidden sm:block text-right text-xs text-[var(--text-muted)] shrink-0">
                  <div>
                    {item.progress
                      ? `Ch. ${Math.round(item.progress.chapter_number)} (${pct}%)`
                      : item.latest_chapter
                        ? `${item.novel.chapter_count} chapters`
                        : "—"}
                  </div>
                  <div className="text-[11px]">
                    {formatRelativeDate(item.progress?.updated_at || item.entry.updated_at)}
                  </div>
                </div>
                <button
                  type="button"
                  onClick={() => onStartReading(item.novel.id, item.progress?.chapter_id)}
                  className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium text-white shrink-0"
                  style={{ backgroundColor: "var(--accent)" }}
                >
                  <BookOpen className="w-3.5 h-3.5" />
                  {item.progress ? "Continue" : "Read"}
                </button>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
};
