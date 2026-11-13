import React, { useEffect, useState } from "react";
import { BookOpen, Clock, Trash2 } from "lucide-react";
import { api, extractErrorMessage } from "../api/tauri";
import { CoverImage } from "../components/CoverImage";
import type { ReadingHistoryItem } from "../types/novel";
import { formatDuration, formatRelativeDate } from "../utils/format";

interface HistoryPageProps {
  onOpenNovel: (novelId: string) => void;
  onStartReading: (novelId: string, chapterId: string) => void;
  onNotify: (msg: string, isError?: boolean) => void;
}

export const HistoryPage: React.FC<HistoryPageProps> = ({
  onOpenNovel,
  onStartReading,
  onNotify,
}) => {
  const [items, setItems] = useState<ReadingHistoryItem[]>([]);
  const [loading, setLoading] = useState(true);

  const loadHistory = async () => {
    setLoading(true);
    try {
      setItems(await api.listReadingHistory(150));
    } catch (e) {
      onNotify(extractErrorMessage(e), true);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    loadHistory();
  }, []);

  const handleClear = async () => {
    try {
      await api.clearReadingHistory();
      setItems([]);
      onNotify("Reading history cleared");
    } catch (e) {
      onNotify(extractErrorMessage(e), true);
    }
  };

  return (
    <div className="p-6 max-w-5xl mx-auto space-y-6">
      <div className="flex items-center justify-between gap-4">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight flex items-center gap-2">
            <Clock className="w-6 h-6 text-[var(--accent-hover)]" />
            Reading History
          </h1>
          <p className="text-sm text-[var(--text-secondary)] mt-0.5">
            Your recent reading sessions across all novels.
          </p>
        </div>

        {items.length > 0 && (
          <button
            type="button"
            onClick={handleClear}
            className="inline-flex items-center gap-1.5 px-3 py-2 rounded-lg text-xs border border-rose-500/30 bg-rose-500/10 text-rose-300 hover:bg-rose-500/20"
          >
            <Trash2 className="w-3.5 h-3.5" />
            Clear History
          </button>
        )}
      </div>

      {loading ? (
        <div className="space-y-2">
          {[1, 2, 3, 4].map((n) => (
            <div
              key={n}
              className="h-16 rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-surface)] animate-pulse"
            />
          ))}
        </div>
      ) : items.length === 0 ? (
        <div className="py-16 text-center rounded-xl border border-dashed border-[var(--border-strong)] text-xs text-[var(--text-muted)]">
          No reading history recorded yet. Start reading any chapter to track your sessions.
        </div>
      ) : (
        <div className="rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-surface)] divide-y divide-[var(--border-subtle)] overflow-hidden">
          {items.map((item) => (
            <div
              key={item.id}
              className="px-4 py-3 flex items-center justify-between gap-4 hover:bg-[var(--bg-hover)] transition-colors"
            >
              <div className="flex items-center gap-3.5 min-w-0">
                <button
                  type="button"
                  onClick={() => onOpenNovel(item.novel_id)}
                  className="w-10 h-14 rounded overflow-hidden shrink-0"
                >
                  <CoverImage
                    title={item.novel_title}
                    coverUrl={item.cover_url}
                    localCoverPath={item.local_cover_path}
                    size="sm"
                    className="w-full h-full"
                  />
                </button>
                <div className="min-w-0">
                  <button
                    type="button"
                    onClick={() => onOpenNovel(item.novel_id)}
                    className="text-sm font-medium truncate hover:text-[var(--accent-hover)] block text-left"
                  >
                    {item.novel_title}
                  </button>
                  <div className="text-xs text-[var(--text-secondary)] truncate">
                    {item.chapter_title}
                  </div>
                  <div className="text-[11px] text-[var(--text-muted)] flex items-center gap-2 mt-0.5">
                    <span>{formatRelativeDate(item.read_at)}</span>
                    <span>·</span>
                    <span>{formatDuration(item.duration_seconds)}</span>
                    {item.words_read > 0 && (
                      <>
                        <span>·</span>
                        <span>{item.words_read.toLocaleString()} words</span>
                      </>
                    )}
                  </div>
                </div>
              </div>

              <button
                type="button"
                onClick={() => onStartReading(item.novel_id, item.chapter_id)}
                className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium bg-[var(--accent-soft)] text-[var(--accent-hover)] hover:bg-[var(--accent)] hover:text-white transition-colors shrink-0"
              >
                <BookOpen className="w-3.5 h-3.5" />
                Resume
              </button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
};
