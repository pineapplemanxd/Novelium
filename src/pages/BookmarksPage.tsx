import React, { useEffect, useState } from "react";
import { Bookmark, BookOpen, Trash2 } from "lucide-react";
import { api, extractErrorMessage } from "../api/tauri";
import { CoverImage } from "../components/CoverImage";
import type { Bookmark as BookmarkItem } from "../types/novel";
import {
  ANNOTATION_COLORS,
  formatRelativeDate,
  swatchForColor,
} from "../utils/format";

interface BookmarksPageProps {
  onOpenNovel: (novelId: string) => void;
  onJumpToBookmark: (novelId: string, chapterId: string, paragraphIndex?: number | null) => void;
  onNotify: (msg: string, isError?: boolean) => void;
}

export const BookmarksPage: React.FC<BookmarksPageProps> = ({
  onOpenNovel,
  onJumpToBookmark,
  onNotify,
}) => {
  const [bookmarks, setBookmarks] = useState<BookmarkItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [colorFilter, setColorFilter] = useState<string>("all");

  const load = async () => {
    setLoading(true);
    try {
      setBookmarks(await api.listBookmarks());
    } catch (e) {
      onNotify(extractErrorMessage(e), true);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    load();
  }, []);

  const handleDelete = async (id: string) => {
    try {
      await api.deleteBookmark(id);
      setBookmarks((prev) => prev.filter((b) => b.id !== id));
      onNotify("Bookmark deleted");
    } catch (e) {
      onNotify(extractErrorMessage(e), true);
    }
  };

  return (
    <div className="p-4 sm:p-6 max-w-5xl mx-auto space-y-5 sm:space-y-6">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight flex items-center gap-2">
          <Bookmark className="w-6 h-6 text-[var(--accent-hover)]" />
          Bookmarks & Saved Passages
        </h1>
        <p className="text-sm text-[var(--text-secondary)] mt-0.5">
          Bookmarked novels, chapters, and highlighted passages with notes.
        </p>
      </div>

      {!loading && bookmarks.length > 0 && (
        <div className="flex flex-wrap items-center gap-2">
          <button
            type="button"
            onClick={() => setColorFilter("all")}
            className={`px-3 py-1 rounded-full text-xs border transition-colors ${
              colorFilter === "all"
                ? "border-[var(--accent)] bg-[var(--accent-soft)] text-[var(--accent-hover)] font-medium"
                : "border-[var(--border-subtle)] bg-[var(--bg-surface)] text-[var(--text-secondary)]"
            }`}
          >
            All Colors ({bookmarks.length})
          </button>
          {ANNOTATION_COLORS.map((c) => {
            const count = bookmarks.filter((b) => (b.color || "amber") === c.id).length;
            if (count === 0) return null;
            return (
              <button
                key={c.id}
                type="button"
                onClick={() => setColorFilter(c.id)}
                className={`inline-flex items-center gap-1.5 px-3 py-1 rounded-full text-xs border transition-colors ${
                  colorFilter === c.id
                    ? "border-[var(--accent)] bg-[var(--accent-soft)] text-[var(--text-primary)] font-medium"
                    : "border-[var(--border-subtle)] bg-[var(--bg-surface)] text-[var(--text-secondary)]"
                }`}
              >
                <span
                  className="w-2.5 h-2.5 rounded-full"
                  style={{ backgroundColor: c.hex }}
                />
                {c.label} ({count})
              </button>
            );
          })}
        </div>
      )}

      {loading ? (
        <div className="space-y-3">
          {[1, 2, 3].map((i) => (
            <div
              key={i}
              className="h-24 rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-surface)] animate-pulse"
            />
          ))}
        </div>
      ) : bookmarks.length === 0 ? (
        <div className="py-16 text-center rounded-xl border border-dashed border-[var(--border-strong)] text-xs text-[var(--text-muted)]">
          No bookmarks saved yet. Press <kbd className="px-1.5 py-0.5 rounded bg-[var(--bg-elevated)]">B</kbd> while reading or click Bookmark on any novel.
        </div>
      ) : (
        <div className="space-y-3">
          {bookmarks
            .filter((bm) => colorFilter === "all" || (bm.color || "amber") === colorFilter)
            .map((bm) => {
              const swatch = swatchForColor(bm.color);
              return (
                <div
                  key={bm.id}
                  className="p-4 rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-surface)] flex items-start gap-4 hover:border-[var(--border-strong)] transition-colors"
                  style={{ borderLeftWidth: 4, borderLeftColor: swatch.hex }}
                >
                  <button
                    type="button"
                    onClick={() => onOpenNovel(bm.novel_id)}
                    className="w-12 h-16 rounded-md overflow-hidden shrink-0"
                  >
                    <CoverImage
                      title={bm.novel_title || "Novel"}
                      novelId={bm.novel_id}
                      coverUrl={bm.cover_url}
                      localCoverPath={bm.local_cover_path}
                      size="sm"
                      className="w-full h-full"
                    />
                  </button>

                  <div className="flex-1 min-w-0 space-y-1.5">
                    <div className="flex items-start justify-between gap-2">
                      <div>
                        <div className="flex items-center gap-2">
                          <span
                            className="w-2.5 h-2.5 rounded-full shrink-0"
                            style={{ backgroundColor: swatch.hex }}
                            title={swatch.label}
                          />
                          <button
                            type="button"
                            onClick={() => onOpenNovel(bm.novel_id)}
                            className="text-sm font-semibold hover:text-[var(--accent-hover)] text-left"
                          >
                            {bm.novel_title || bm.novel_id}
                          </button>
                        </div>
                        <div className="text-xs text-[var(--text-secondary)]">
                          {bm.chapter_title
                            ? `Ch. ${Math.round(bm.chapter_number ?? 1)} — ${bm.chapter_title}`
                            : "Novel Bookmark"}
                          <span className="mx-1.5 opacity-50">·</span>
                          <span className="text-[var(--text-muted)]">
                            {formatRelativeDate(bm.created_at)}
                          </span>
                        </div>
                      </div>

                      <div className="flex items-center gap-1.5 shrink-0">
                        {bm.chapter_id && (
                          <button
                            type="button"
                            onClick={() =>
                              onJumpToBookmark(bm.novel_id, bm.chapter_id!, bm.paragraph_index)
                            }
                            className="inline-flex items-center gap-1 px-2.5 py-1.5 rounded-lg text-xs font-medium bg-[var(--accent-soft)] text-[var(--accent-hover)] hover:bg-[var(--accent)] hover:text-white transition-colors"
                          >
                            <BookOpen className="w-3.5 h-3.5" />
                            Open Passage
                          </button>
                        )}
                        <button
                          type="button"
                          title="Delete Bookmark"
                          onClick={() => handleDelete(bm.id)}
                          className="p-1.5 rounded-lg text-[var(--text-muted)] hover:text-rose-400 hover:bg-[var(--bg-elevated)]"
                        >
                          <Trash2 className="w-4 h-4" />
                        </button>
                      </div>
                    </div>

                    {bm.excerpt && (
                      <blockquote
                        className="text-xs italic text-[var(--text-secondary)] border-l-2 pl-3 py-0.5 line-clamp-3"
                        style={{ borderLeftColor: swatch.hex }}
                      >
                        “{bm.excerpt}”
                      </blockquote>
                    )}
                    {bm.note && (
                      <p className="text-xs text-[var(--text-primary)] bg-[var(--bg-elevated)] px-3 py-1.5 rounded-lg">
                        {bm.note}
                      </p>
                    )}
                  </div>
                </div>
              );
            })}
        </div>
      )}
    </div>
  );
};
