import React, { useCallback, useEffect, useRef, useState } from "react";
import { AlertCircle, Globe, Loader2, Search, SlidersHorizontal } from "lucide-react";
import { api, extractErrorMessage } from "../api/tauri";
import { NovelSummaryCard } from "../components/NovelCard";
import type { NovelSummary, SourceErrorBanner, SourceInfo } from "../types/novel";
import { APP_LANGUAGES } from "../utils/format";

interface DiscoverPageProps {
  initialQuery?: string;
  initialGenre?: string;
  initialSort?: string;
  onOpenNovel: (summary: NovelSummary) => void;
}

const GENRES = [
  "All",
  "Fiction",
  "Science Fiction",
  "Fantasy",
  "Mystery",
  "Romance",
  "Adventure",
  "Horror",
  "Historical",
  "Philosophy",
  "Poetry",
  "Short Stories",
];

export const DiscoverPage: React.FC<DiscoverPageProps> = ({
  initialQuery = "",
  initialGenre = "All",
  initialSort = "popular",
  onOpenNovel,
}) => {
  const [sources, setSources] = useState<SourceInfo[]>([]);
  const [selectedSource, setSelectedSource] = useState<string>("all");
  const [searchInput, setSearchInput] = useState(initialQuery);
  const [appliedQuery, setAppliedQuery] = useState(initialQuery);
  const [genre, setGenre] = useState(initialGenre);
  const [sort, setSort] = useState(initialSort);
  const [status, setStatus] = useState("all");
  const [language, setLanguage] = useState("all");

  const [items, setItems] = useState<NovelSummary[]>([]);
  const [page, setPage] = useState(1);
  const [hasMore, setHasMore] = useState(false);
  const [totalEstimate, setTotalEstimate] = useState<number | null>(null);
  const [sourceErrors, setSourceErrors] = useState<SourceErrorBanner[]>([]);
  const [loading, setLoading] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);

  const sentinelRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    setSearchInput(initialQuery);
    setAppliedQuery(initialQuery);
  }, [initialQuery]);

  useEffect(() => {
    if (initialGenre) setGenre(initialGenre);
  }, [initialGenre]);

  useEffect(() => {
    if (initialSort) setSort(initialSort);
  }, [initialSort]);

  useEffect(() => {
    api.listSources().then(setSources).catch(() => {});
  }, []);

  const fetchPage = useCallback(
    async (pageNum: number, append: boolean) => {
      if (append) setLoadingMore(true);
      else setLoading(true);
      setErrorMsg(null);
      try {
        const res = await api.discoverNovels({
          source_id: selectedSource === "all" ? null : selectedSource,
          query: appliedQuery.trim() || null,
          genre: genre === "All" ? null : genre,
          sort,
          status: status === "all" ? null : status,
          language: language === "all" ? null : language,
          page: pageNum,
          limit: 20,
        });
        setItems((prev) => {
          if (!append) return res.items;
          const seen = new Set(prev.map((x) => x.id));
          const merged = [...prev];
          for (const item of res.items) {
            if (!seen.has(item.id)) {
              seen.add(item.id);
              merged.push(item);
            }
          }
          return merged;
        });
        setPage(res.page);
        setHasMore(res.has_more);
        setTotalEstimate(res.total_estimate);
        setSourceErrors(res.source_errors);
      } catch (e) {
        setErrorMsg(extractErrorMessage(e));
      } finally {
        setLoading(false);
        setLoadingMore(false);
      }
    },
    [selectedSource, appliedQuery, genre, sort, status, language],
  );

  useEffect(() => {
    fetchPage(1, false);
  }, [fetchPage]);

  // Infinite scroll intersection observer
  useEffect(() => {
    const el = sentinelRef.current;
    if (!el || !hasMore || loading || loadingMore) return;
    const obs = new IntersectionObserver(
      (entries) => {
        if (entries[0]?.isIntersecting && hasMore && !loadingMore) {
          fetchPage(page + 1, true);
        }
      },
      { rootMargin: "320px" },
    );
    obs.observe(el);
    return () => obs.disconnect();
  }, [hasMore, loading, loadingMore, page, fetchPage]);

  const enabledSources = sources.filter((s) => s.enabled);

  return (
    <div className="p-6 max-w-[1440px] mx-auto space-y-5">
      <div className="flex flex-col md:flex-row md:items-end justify-between gap-4">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">Discover Novels</h1>
          <p className="text-sm text-[var(--text-secondary)] mt-0.5">
            Search and browse across {enabledSources.length} enabled source
            {enabledSources.length === 1 ? "" : "s"}
            {totalEstimate ? ` · ~${totalEstimate.toLocaleString()} titles` : ""}
          </p>
        </div>

        {/* Search Input */}
        <form
          onSubmit={(e) => {
            e.preventDefault();
            setAppliedQuery(searchInput);
          }}
          className="flex items-center gap-2 w-full md:w-[420px]"
        >
          <div className="relative flex-1">
            <Search className="w-4 h-4 text-[var(--text-muted)] absolute left-3 top-1/2 -translate-y-1/2" />
            <input
              value={searchInput}
              onChange={(e) => setSearchInput(e.target.value)}
              placeholder="Search by title or author..."
              className="w-full pl-9 pr-3 py-2 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-surface)] text-sm text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:outline-none focus:border-[var(--accent)]"
            />
          </div>
          <button
            type="submit"
            className="px-3.5 py-2 rounded-lg text-sm font-medium text-white shrink-0"
            style={{ backgroundColor: "var(--accent)" }}
          >
            Search
          </button>
        </form>
      </div>

      {/* Source Tabs + Sort + Status Filters */}
      <div className="flex flex-wrap items-center justify-between gap-3 pb-3 border-b border-[var(--border-subtle)]">
        <div className="flex flex-wrap items-center gap-1.5">
          <button
            type="button"
            onClick={() => setSelectedSource("all")}
            className={`inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium transition-colors ${
              selectedSource === "all"
                ? "text-white"
                : "bg-[var(--bg-surface)] text-[var(--text-secondary)] border border-[var(--border-subtle)] hover:text-[var(--text-primary)]"
            }`}
            style={selectedSource === "all" ? { backgroundColor: "var(--accent)" } : undefined}
          >
            <Globe className="w-3.5 h-3.5" />
            All Sources
          </button>
          {enabledSources.map((src) => (
            <button
              key={src.id}
              type="button"
              onClick={() => setSelectedSource(src.id)}
              className={`px-3 py-1.5 rounded-lg text-xs font-medium transition-colors ${
                selectedSource === src.id
                  ? "text-white"
                  : "bg-[var(--bg-surface)] text-[var(--text-secondary)] border border-[var(--border-subtle)] hover:text-[var(--text-primary)]"
              }`}
              style={selectedSource === src.id ? { backgroundColor: "var(--accent)" } : undefined}
            >
              {src.name}
            </button>
          ))}
        </div>

        <div className="flex items-center gap-2">
          <SlidersHorizontal className="w-3.5 h-3.5 text-[var(--text-muted)]" />
          <select
            aria-label="Sort novels"
            value={sort}
            onChange={(e) => setSort(e.target.value)}
            className="px-2.5 py-1.5 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-surface)] text-xs text-[var(--text-primary)] focus:outline-none"
          >
            <option value="popular">Popular</option>
            <option value="trending">Trending</option>
            <option value="latest">Latest Updates</option>
            <option value="title">Title (A–Z)</option>
          </select>

          <select
            aria-label="Filter completion status"
            value={status}
            onChange={(e) => setStatus(e.target.value)}
            className="px-2.5 py-1.5 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-surface)] text-xs text-[var(--text-primary)] focus:outline-none"
          >
            <option value="all">All Statuses</option>
            <option value="completed">Completed</option>
            <option value="ongoing">Ongoing</option>
          </select>

          <select
            aria-label="Filter language"
            value={language}
            onChange={(e) => setLanguage(e.target.value)}
            className="px-2.5 py-1.5 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-surface)] text-xs text-[var(--text-primary)] focus:outline-none"
          >
            <option value="all">All Languages</option>
            {APP_LANGUAGES.map((l) => (
              <option key={l.code} value={l.code}>
                {l.label}
              </option>
            ))}
          </select>
        </div>
      </div>

      {/* Genre Chips */}
      <div className="flex items-center gap-1.5 overflow-x-auto pb-1">
        {GENRES.map((g) => (
          <button
            key={g}
            type="button"
            onClick={() => setGenre(g)}
            className={`px-3 py-1 rounded-full text-xs whitespace-nowrap transition-colors ${
              genre === g
                ? "bg-[var(--accent-soft)] text-[var(--accent-hover)] border border-[var(--accent-border)] font-medium"
                : "bg-[var(--bg-surface)] text-[var(--text-secondary)] border border-[var(--border-subtle)] hover:text-[var(--text-primary)]"
            }`}
          >
            {g}
          </button>
        ))}
      </div>

      {/* Source Errors */}
      {(errorMsg || sourceErrors.length > 0) && (
        <div className="rounded-xl border border-amber-500/30 bg-amber-500/10 p-3 space-y-1 text-xs text-amber-200">
          {errorMsg && (
            <div className="flex items-center gap-2">
              <AlertCircle className="w-4 h-4 shrink-0" />
              <span>{errorMsg}</span>
            </div>
          )}
          {sourceErrors.map((se) => (
            <div key={se.source_id} className="flex items-center gap-2">
              <AlertCircle className="w-4 h-4 shrink-0" />
              <span>
                <strong>{se.source_name}:</strong> {se.message}
              </span>
            </div>
          ))}
        </div>
      )}

      {/* Results Grid */}
      {loading ? (
        <div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6 gap-4">
          {Array.from({ length: 12 }).map((_, i) => (
            <div
              key={i}
              className="aspect-[2/3] rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-surface)] animate-pulse"
            />
          ))}
        </div>
      ) : items.length === 0 ? (
        <div className="py-16 text-center rounded-xl border border-dashed border-[var(--border-strong)] bg-[var(--bg-surface)]/50">
          <p className="text-sm font-medium text-[var(--text-primary)]">No novels matched your filters</p>
          <p className="text-xs text-[var(--text-secondary)] mt-1">
            Try switching sources, clearing your search query, or choosing "All Statuses".
          </p>
        </div>
      ) : (
        <>
          <div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6 gap-4">
            {items.map((novel) => (
              <NovelSummaryCard
                key={novel.id}
                novel={novel}
                onOpen={onOpenNovel}
              />
            ))}
          </div>

          <div ref={sentinelRef} className="py-6 flex justify-center">
            {loadingMore ? (
              <div className="inline-flex items-center gap-2 text-xs text-[var(--text-secondary)]">
                <Loader2 className="w-4 h-4 animate-spin" />
                Loading more novels...
              </div>
            ) : hasMore ? (
              <button
                type="button"
                onClick={() => fetchPage(page + 1, true)}
                className="px-4 py-2 rounded-lg text-xs font-medium border border-[var(--border-subtle)] bg-[var(--bg-surface)] hover:bg-[var(--bg-elevated)] text-[var(--text-primary)]"
              >
                Load More (Page {page + 1})
              </button>
            ) : (
              <span className="text-xs text-[var(--text-muted)]">End of catalog</span>
            )}
          </div>
        </>
      )}
    </div>
  );
};
