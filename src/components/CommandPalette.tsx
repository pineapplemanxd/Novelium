import React, { useEffect, useRef, useState } from "react";
import { ArrowRight, BookOpen, Globe, Search, X } from "lucide-react";
import { api } from "../api/tauri";
import type { NovelRecord, NovelSummary } from "../types/novel";
import { CoverImage } from "./CoverImage";
import { sourceLabel } from "../utils/format";

interface CommandPaletteProps {
  open: boolean;
  onClose: () => void;
  onOpenLocalNovel: (novelId: string) => void;
  onOpenRemoteNovel: (novel: NovelSummary) => void;
  onSearchAllSources: (query: string) => void;
}

export const CommandPalette: React.FC<CommandPaletteProps> = ({
  open,
  onClose,
  onOpenLocalNovel,
  onOpenRemoteNovel,
  onSearchAllSources,
}) => {
  const [query, setQuery] = useState("");
  const [localResults, setLocalResults] = useState<NovelRecord[]>([]);
  const [remoteResults, setRemoteResults] = useState<NovelSummary[]>([]);
  const [searchingRemote, setSearchingRemote] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (open) {
      setQuery("");
      setRemoteResults([]);
      setTimeout(() => inputRef.current?.focus(), 20);
      api.searchLocalNovels("").then(setLocalResults).catch(() => {});
    }
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const t = setTimeout(() => {
      api.searchLocalNovels(query).then(setLocalResults).catch(() => {});
    }, 120);
    return () => clearTimeout(t);
  }, [query, open]);

  if (!open) return null;

  const triggerRemoteSearch = async () => {
    const q = query.trim();
    if (!q) return;
    setSearchingRemote(true);
    try {
      const res = await api.discoverNovels({
        source_id: null,
        query: q,
        genre: null,
        sort: "popular",
        status: null,
        page: 1,
        limit: 8,
      });
      setRemoteResults(res.items.slice(0, 8));
    } catch {
      // handled cleanly by discover page fallback
    } finally {
      setSearchingRemote(false);
    }
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center pt-[12vh] bg-black/60 backdrop-blur-sm px-4"
      onClick={onClose}
    >
      <div
        className="w-full max-w-2xl rounded-2xl border border-[var(--border-strong)] bg-[var(--bg-surface)] shadow-2xl overflow-hidden"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center gap-3 px-4 py-3 border-b border-[var(--border-subtle)]">
          <Search className="w-4 h-4 text-[var(--text-muted)] shrink-0" />
          <input
            ref={inputRef}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Escape") onClose();
              if (e.key === "Enter" && query.trim()) triggerRemoteSearch();
            }}
            placeholder="Search your library, or press Enter to search online sources..."
            className="flex-1 bg-transparent text-sm text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:outline-none"
          />
          {query && (
            <button
              type="button"
              onClick={() => setQuery("")}
              className="p-1 rounded-md text-[var(--text-muted)] hover:text-[var(--text-primary)]"
            >
              <X className="w-4 h-4" />
            </button>
          )}
          <kbd className="px-1.5 py-0.5 text-[10px] rounded border border-[var(--border-subtle)] text-[var(--text-muted)]">
            ESC
          </kbd>
        </div>

        <div className="max-h-[60vh] overflow-y-auto p-2 space-y-3">
          {query.trim() && (
            <div className="flex items-center justify-between px-2 py-1.5 rounded-lg bg-[var(--bg-elevated)]">
              <button
                type="button"
                onClick={triggerRemoteSearch}
                disabled={searchingRemote}
                className="flex items-center gap-2 text-xs font-medium text-[var(--accent-hover)] hover:underline"
              >
                <Globe className="w-3.5 h-3.5" />
                {searchingRemote
                  ? `Searching online sources for "${query.trim()}"...`
                  : `Quick search online sources for "${query.trim()}" (Enter)`}
              </button>
              <button
                type="button"
                onClick={() => {
                  onSearchAllSources(query.trim());
                  onClose();
                }}
                className="inline-flex items-center gap-1 text-xs text-[var(--text-secondary)] hover:text-[var(--text-primary)]"
              >
                Open in Discover <ArrowRight className="w-3 h-3" />
              </button>
            </div>
          )}

          {localResults.length > 0 && (
            <div>
              <div className="px-2 py-1 text-[11px] font-medium uppercase tracking-wider text-[var(--text-muted)]">
                In Your Library & Cache
              </div>
              <div className="space-y-0.5">
                {localResults.slice(0, 6).map((novel) => (
                  <button
                    key={novel.id}
                    type="button"
                    onClick={() => {
                      onOpenLocalNovel(novel.id);
                      onClose();
                    }}
                    className="w-full flex items-center gap-3 px-2.5 py-2 rounded-lg hover:bg-[var(--bg-hover)] text-left transition-colors"
                  >
                    <CoverImage
                      title={novel.title}
                      coverUrl={novel.cover_url}
                      localCoverPath={novel.local_cover_path}
                      size="sm"
                      className="w-8 h-11 rounded shrink-0"
                    />
                    <div className="min-w-0 flex-1">
                      <div className="text-sm font-medium text-[var(--text-primary)] truncate">
                        {novel.title}
                      </div>
                      <div className="text-xs text-[var(--text-secondary)] truncate">
                        {novel.author} · {sourceLabel(novel.source_id)}
                      </div>
                    </div>
                    <BookOpen className="w-4 h-4 text-[var(--text-muted)] shrink-0" />
                  </button>
                ))}
              </div>
            </div>
          )}

          {remoteResults.length > 0 && (
            <div>
              <div className="px-2 py-1 text-[11px] font-medium uppercase tracking-wider text-[var(--text-muted)]">
                Online Results
              </div>
              <div className="space-y-0.5">
                {remoteResults.map((novel) => (
                  <button
                    key={novel.id}
                    type="button"
                    onClick={() => {
                      onOpenRemoteNovel(novel);
                      onClose();
                    }}
                    className="w-full flex items-center gap-3 px-2.5 py-2 rounded-lg hover:bg-[var(--bg-hover)] text-left transition-colors"
                  >
                    <CoverImage
                      title={novel.title}
                      coverUrl={novel.cover_url}
                      size="sm"
                      className="w-8 h-11 rounded shrink-0"
                    />
                    <div className="min-w-0 flex-1">
                      <div className="text-sm font-medium text-[var(--text-primary)] truncate">
                        {novel.title}
                      </div>
                      <div className="text-xs text-[var(--text-secondary)] truncate">
                        {novel.author} · {sourceLabel(novel.source_id)}
                      </div>
                    </div>
                    <ArrowRight className="w-4 h-4 text-[var(--text-muted)] shrink-0" />
                  </button>
                ))}
              </div>
            </div>
          )}

          {localResults.length === 0 && remoteResults.length === 0 && !searchingRemote && (
            <div className="py-8 text-center text-xs text-[var(--text-muted)]">
              Type a novel title, author, or genre — press Enter to search online sources.
            </div>
          )}
        </div>
      </div>
    </div>
  );
};
