import React, { useEffect, useState } from "react";
import {
  ArrowRight,
  BookOpen,
  Clock,
  Flame,
  Library,
  RefreshCw,
  Sparkles,
  TrendingUp,
} from "lucide-react";
import { api, extractErrorMessage } from "../api/tauri";
import { CoverImage } from "../components/CoverImage";
import { LibraryNovelCard, NovelSummaryCard } from "../components/NovelCard";
import type { HomeDashboardData, NovelSummary } from "../types/novel";
import { formatDuration, formatNumber, formatRelativeDate } from "../utils/format";

export interface HomeFeedCache {
  trending: NovelSummary[];
  popular: NovelSummary[];
  recommended: NovelSummary[];
  recommendedGenre: string;
  fetchedAt: number;
}

interface HomePageProps {
  feedCache: HomeFeedCache | null;
  onUpdateFeedCache: (cache: HomeFeedCache) => void;
  onOpenNovel: (novelId: string) => void;
  onOpenRemoteNovel: (summary: NovelSummary) => void;
  onStartReading: (novelId: string, chapterId?: string) => void;
  onNavigate: (tab: string, opts?: { genre?: string; sort?: string }) => void;
}

export const HomePage: React.FC<HomePageProps> = ({
  feedCache,
  onUpdateFeedCache,
  onOpenNovel,
  onOpenRemoteNovel,
  onStartReading,
  onNavigate,
}) => {
  const [dashboard, setDashboard] = useState<HomeDashboardData | null>(null);
  const [loadingDash, setLoadingDash] = useState(true);
  const [loadingFeeds, setLoadingFeeds] = useState(false);
  const [feedError, setFeedError] = useState<string | null>(null);

  const loadDashboard = async (): Promise<{ ok: boolean; data: HomeDashboardData | null }> => {
    setLoadingDash(true);
    try {
      const data = await api.getHomeDashboard();
      setDashboard(data);
      return { ok: true, data };
    } catch (e) {
      setFeedError(extractErrorMessage(e));
      return { ok: false, data: null };
    } finally {
      setLoadingDash(false);
    }
  };

  const loadOnlineFeeds = async (dashData: HomeDashboardData | null, force = false) => {
    if (!force && feedCache && Date.now() - feedCache.fetchedAt < 10 * 60 * 1000) {
      return;
    }
    setLoadingFeeds(true);
    setFeedError(null);
    try {
      // Pick top genre from user's library for personalized recommendations
      const genreCounts = new Map<string, number>();
      for (const item of dashData?.your_library || []) {
        for (const g of item.novel.genres) {
          genreCounts.set(g, (genreCounts.get(g) || 0) + 1);
        }
      }
      const topGenre =
        Array.from(genreCounts.entries()).sort((a, b) => b[1] - a[1])[0]?.[0] ||
        "Science Fiction";

      const [trendRes, popRes, recRes] = await Promise.all([
        api.discoverNovels({
          source_id: null,
          query: null,
          genre: null,
          sort: "trending",
          status: null,
          page: 1,
          limit: 10,
        }),
        api.discoverNovels({
          source_id: null,
          query: null,
          genre: null,
          sort: "latest",
          status: null,
          page: 1,
          limit: 10,
        }),
        api.discoverNovels({
          source_id: null,
          query: null,
          genre: topGenre,
          sort: "popular",
          status: null,
          page: 1,
          limit: 10,
        }),
      ]);

      onUpdateFeedCache({
        trending: trendRes.items.slice(0, 10),
        popular: popRes.items.slice(0, 10),
        recommended: recRes.items.slice(0, 10),
        recommendedGenre: topGenre,
        fetchedAt: Date.now(),
      });

      if (
        trendRes.items.length === 0 &&
        trendRes.source_errors.length > 0
      ) {
        setFeedError(trendRes.source_errors.map((s) => `${s.source_name}: ${s.message}`).join(" · "));
      }
    } catch (e) {
      setFeedError(extractErrorMessage(e));
    } finally {
      setLoadingFeeds(false);
    }
  };

  useEffect(() => {
    loadDashboard().then((res) => {
      if (res.ok) loadOnlineFeeds(res.data, false);
    });
  }, []);

  const stats = dashboard?.stats;
  const maxDailyMins = Math.max(
    1,
    ...(stats?.daily_7d.map((d) => d.minutes) || [1]),
  );

  return (
    <div className="p-4 sm:p-6 max-w-[1440px] mx-auto space-y-8">
      {/* Header + Stats Strip */}
      <div className="flex flex-col lg:flex-row lg:items-center lg:justify-between gap-4">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight text-[var(--text-primary)]">
            Welcome back to Novelium
          </h1>
          <p className="text-sm text-[var(--text-secondary)] mt-0.5">
            Your personal reading studio across online public catalogs and offline books.
          </p>
        </div>

        <div className="flex flex-wrap items-center gap-3">
          {stats && (
            <div className="flex items-center gap-4 px-4 py-2.5 rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-surface)]">
              <div className="flex items-center gap-2">
                <Flame className="w-4 h-4 text-amber-400" />
                <div>
                  <div className="text-xs font-semibold">{stats.current_streak_days}d</div>
                  <div className="text-[10px] text-[var(--text-muted)]">Streak</div>
                </div>
              </div>
              <div className="h-6 w-px bg-[var(--border-subtle)]" />
              <div className="flex items-center gap-2">
                <Clock className="w-4 h-4 text-[var(--accent-hover)]" />
                <div>
                  <div className="text-xs font-semibold">
                    {formatDuration(stats.total_reading_seconds)}
                  </div>
                  <div className="text-[10px] text-[var(--text-muted)]">Read Time</div>
                </div>
              </div>
              <div className="h-6 w-px bg-[var(--border-subtle)]" />
              <div>
                <div className="text-xs font-semibold">{formatNumber(stats.total_words_read)}</div>
                <div className="text-[10px] text-[var(--text-muted)]">Words</div>
              </div>
              <div className="h-6 w-px bg-[var(--border-subtle)] hidden sm:block" />
              <div className="hidden sm:flex items-end gap-1 h-6" title="Last 7 days reading minutes">
                {stats.daily_7d.map((pt) => (
                  <div
                    key={pt.date}
                    title={`${pt.date}: ${pt.minutes}m`}
                    className="w-1.5 rounded-t bg-[var(--accent)] transition-all"
                    style={{
                      height: `${Math.max(15, Math.round((pt.minutes / maxDailyMins) * 100))}%`,
                      opacity: pt.minutes > 0 ? 0.9 : 0.2,
                    }}
                  />
                ))}
              </div>
            </div>
          )}

          <button
            type="button"
            onClick={() =>
              loadDashboard().then((res) => {
                if (res.ok) loadOnlineFeeds(res.data, true);
              })
            }
            disabled={loadingFeeds}
            className="inline-flex items-center gap-1.5 px-3 py-2 rounded-lg text-xs font-medium border border-[var(--border-subtle)] bg-[var(--bg-surface)] hover:bg-[var(--bg-elevated)] text-[var(--text-secondary)] hover:text-[var(--text-primary)] transition-colors"
          >
            <RefreshCw className={`w-3.5 h-3.5 ${loadingFeeds ? "animate-spin" : ""}`} />
            Refresh
          </button>
        </div>
      </div>

      {feedError && (
        <div className="px-4 py-3 rounded-xl border border-amber-500/30 bg-amber-500/10 text-xs text-amber-200 flex items-center justify-between gap-4">
          <span>Offline or source notice: {feedError}</span>
          <button
            type="button"
            onClick={() => loadOnlineFeeds(dashboard, true)}
            className="px-2.5 py-1 rounded-md bg-amber-500/20 hover:bg-amber-500/30 font-medium shrink-0"
          >
            Retry
          </button>
        </div>
      )}

      {/* Continue Reading */}
      <section className="space-y-3">
        <div className="flex items-center justify-between">
          <h2 className="text-base font-semibold flex items-center gap-2">
            <BookOpen className="w-4 h-4 text-[var(--accent-hover)]" />
            Continue Reading
          </h2>
          {dashboard && dashboard.continue_reading.length > 0 && (
            <button
              type="button"
              onClick={() => onNavigate("library")}
              className="inline-flex items-center gap-1 text-xs text-[var(--text-secondary)] hover:text-[var(--text-primary)]"
            >
              View Library <ArrowRight className="w-3.5 h-3.5" />
            </button>
          )}
        </div>

        {loadingDash ? (
          <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-3">
            {[1, 2, 3].map((n) => (
              <div
                key={n}
                className="h-28 rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-surface)] animate-pulse"
              />
            ))}
          </div>
        ) : dashboard && dashboard.continue_reading.length > 0 ? (
          <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-3">
            {dashboard.continue_reading.map((item) => {
              const pct = Math.round(item.progress?.progress_percent ?? 0);
              return (
                <div
                  key={item.novel.id}
                  className="flex gap-3.5 p-3 rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-surface)] hover:border-[var(--accent-border)] transition-all"
                >
                  <button
                    type="button"
                    onClick={() => onOpenNovel(item.novel.id)}
                    className="w-16 h-24 rounded-lg overflow-hidden shrink-0"
                  >
                    <CoverImage
                      title={item.novel.title}
                      author={item.novel.author}
                      coverUrl={item.novel.cover_url}
                      localCoverPath={item.novel.local_cover_path}
                      size="sm"
                      className="w-full h-full"
                    />
                  </button>
                  <div className="min-w-0 flex-1 flex flex-col">
                    <button
                      type="button"
                      onClick={() => onOpenNovel(item.novel.id)}
                      className="text-left"
                    >
                      <h3 className="text-sm font-medium text-[var(--text-primary)] truncate hover:text-[var(--accent-hover)]">
                        {item.novel.title}
                      </h3>
                      <p className="text-xs text-[var(--text-secondary)] truncate">
                        {item.novel.author}
                      </p>
                    </button>

                    <div className="mt-2 text-xs text-[var(--text-muted)] truncate">
                      {item.progress?.chapter_title ||
                        `Chapter ${Math.round(item.progress?.chapter_number ?? 1)}`}
                    </div>

                    <div className="mt-auto pt-2 flex items-center gap-3">
                      <div className="flex-1 h-1.5 rounded-full bg-[var(--bg-elevated)] overflow-hidden">
                        <div
                          className="h-full rounded-full"
                          style={{ width: `${pct}%`, backgroundColor: "var(--accent)" }}
                        />
                      </div>
                      <span className="text-[11px] text-[var(--text-muted)] tabular-nums">
                        {pct}%
                      </span>
                      <button
                        type="button"
                        onClick={() =>
                          onStartReading(item.novel.id, item.progress?.chapter_id)
                        }
                        className="px-2.5 py-1 rounded-md text-xs font-medium text-white shrink-0"
                        style={{ backgroundColor: "var(--accent)" }}
                      >
                        Resume
                      </button>
                    </div>
                  </div>
                </div>
              );
            })}
          </div>
        ) : (
          <div className="rounded-xl border border-dashed border-[var(--border-strong)] bg-[var(--bg-surface)]/60 p-6 flex flex-col sm:flex-row items-start sm:items-center justify-between gap-4">
            <div>
              <h3 className="text-sm font-medium">No active novels yet</h3>
              <p className="text-xs text-[var(--text-secondary)] mt-1">
                Pick a classic from Trending below, search Discover, or import an EPUB / TXT / HTML file.
              </p>
            </div>
            <button
              type="button"
              onClick={() => onNavigate("discover")}
              className="px-3.5 py-2 rounded-lg text-xs font-medium text-white shrink-0"
              style={{ backgroundColor: "var(--accent)" }}
            >
              Browse Discover
            </button>
          </div>
        )}
      </section>

      {/* Recently Updated & Recently Added (Library) */}
      {dashboard && dashboard.recently_updated.length > 0 && (
        <section className="space-y-3">
          <div className="flex items-center justify-between">
            <h2 className="text-base font-semibold">Recently Updated in Library</h2>
            <span className="text-xs text-[var(--text-muted)]">
              Latest activity {formatRelativeDate(dashboard.recently_updated[0]?.novel.updated_at)}
            </span>
          </div>
          <div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6 gap-3.5">
            {dashboard.recently_updated.slice(0, 6).map((item) => (
              <LibraryNovelCard
                key={item.novel.id}
                item={item}
                onOpen={onOpenNovel}
                onContinue={onStartReading}
              />
            ))}
          </div>
        </section>
      )}

      {/* Trending Novels */}
      <section className="space-y-3">
        <div className="flex items-center justify-between">
          <h2 className="text-base font-semibold flex items-center gap-2">
            <TrendingUp className="w-4 h-4 text-[var(--accent-hover)]" />
            Trending Novels
          </h2>
          <button
            type="button"
            onClick={() => onNavigate("discover", { sort: "trending" })}
            className="inline-flex items-center gap-1 text-xs text-[var(--text-secondary)] hover:text-[var(--text-primary)]"
          >
            See all <ArrowRight className="w-3.5 h-3.5" />
          </button>
        </div>

        {loadingFeeds && !feedCache ? (
          <div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 gap-3.5">
            {[1, 2, 3, 4, 5].map((i) => (
              <div
                key={i}
                className="aspect-[2/3] rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-surface)] animate-pulse"
              />
            ))}
          </div>
        ) : (
          <div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 gap-3.5">
            {(feedCache?.trending || []).slice(0, 10).map((novel) => (
              <NovelSummaryCard
                key={novel.id}
                novel={novel}
                onOpen={onOpenRemoteNovel}
              />
            ))}
          </div>
        )}
      </section>

      {/* Recommended for You */}
      {(feedCache?.recommended.length ?? 0) > 0 && (
        <section className="space-y-3">
          <div className="flex items-center justify-between">
            <h2 className="text-base font-semibold flex items-center gap-2">
              <Sparkles className="w-4 h-4 text-[var(--accent-hover)]" />
              Recommended in {feedCache?.recommendedGenre}
            </h2>
            <button
              type="button"
              onClick={() =>
                onNavigate("discover", { genre: feedCache?.recommendedGenre })
              }
              className="inline-flex items-center gap-1 text-xs text-[var(--text-secondary)] hover:text-[var(--text-primary)]"
            >
              Explore {feedCache?.recommendedGenre} <ArrowRight className="w-3.5 h-3.5" />
            </button>
          </div>
          <div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 gap-3.5">
            {feedCache!.recommended.slice(0, 5).map((novel) => (
              <NovelSummaryCard
                key={`rec-${novel.id}`}
                novel={novel}
                onOpen={onOpenRemoteNovel}
                compact
              />
            ))}
          </div>
        </section>
      )}

      {/* Popular This Week / New Additions */}
      {(feedCache?.popular.length ?? 0) > 0 && (
        <section className="space-y-3">
          <div className="flex items-center justify-between">
            <h2 className="text-base font-semibold">Popular & Fresh Additions</h2>
            <button
              type="button"
              onClick={() => onNavigate("discover", { sort: "latest" })}
              className="inline-flex items-center gap-1 text-xs text-[var(--text-secondary)] hover:text-[var(--text-primary)]"
            >
              Browse Latest <ArrowRight className="w-3.5 h-3.5" />
            </button>
          </div>
          <div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 gap-3.5">
            {feedCache!.popular.slice(0, 5).map((novel) => (
              <NovelSummaryCard
                key={`pop-${novel.id}`}
                novel={novel}
                onOpen={onOpenRemoteNovel}
                compact
              />
            ))}
          </div>
        </section>
      )}

      {/* Recently Added & Your Library */}
      {dashboard && dashboard.recently_added.length > 0 && (
        <section className="space-y-3">
          <div className="flex items-center justify-between">
            <h2 className="text-base font-semibold flex items-center gap-2">
              <Library className="w-4 h-4 text-[var(--accent-hover)]" />
              Recently Added to Your Library
            </h2>
            <button
              type="button"
              onClick={() => onNavigate("library")}
              className="inline-flex items-center gap-1 text-xs text-[var(--text-secondary)] hover:text-[var(--text-primary)]"
            >
              Open Library ({dashboard.your_library.length}) <ArrowRight className="w-3.5 h-3.5" />
            </button>
          </div>
          <div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-6 gap-3.5">
            {dashboard.recently_added.slice(0, 6).map((item) => (
              <LibraryNovelCard
                key={`add-${item.novel.id}`}
                item={item}
                onOpen={onOpenNovel}
                onContinue={onStartReading}
              />
            ))}
          </div>
        </section>
      )}
    </div>
  );
};
