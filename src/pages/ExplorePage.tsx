import React, { useEffect, useState } from "react";
import { ArrowRight, Compass } from "lucide-react";
import { api } from "../api/tauri";
import { NovelSummaryCard } from "../components/NovelCard";
import type { NovelSummary } from "../types/novel";

interface ExplorePageProps {
  onOpenGenreInDiscover: (genre: string) => void;
  onOpenNovel: (summary: NovelSummary) => void;
}

const EXPLORE_GENRES = [
  { name: "Science Fiction", tagline: "Futures, spacefaring odysseys & speculative worlds", color: "#38BDF8" },
  { name: "Fantasy", tagline: "Mythic realms, magic systems & heroic epics", color: "#A78BFA" },
  { name: "Mystery", tagline: "Detectives, locked rooms & intricate puzzles", color: "#F59E0B" },
  { name: "Romance", tagline: "Regency parlors, yearning & classic love stories", color: "#FB7185" },
  { name: "Adventure", tagline: "High seas, uncharted islands & daring quests", color: "#34D399" },
  { name: "Horror", tagline: "Gothic dread, supernatural tales & uncanny nights", color: "#F87171" },
  { name: "Historical", tagline: "Sweeping eras, empires & human drama", color: "#D97706" },
  { name: "Philosophy", tagline: "Essays, ethics & timeless human inquiry", color: "#818CF8" },
];

export const ExplorePage: React.FC<ExplorePageProps> = ({
  onOpenGenreInDiscover,
  onOpenNovel,
}) => {
  const [activeGenre, setActiveGenre] = useState("Science Fiction");
  const [samples, setSamples] = useState<NovelSummary[]>([]);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    api
      .discoverNovels({
        source_id: null,
        query: null,
        genre: activeGenre,
        sort: "popular",
        status: null,
        page: 1,
        limit: 12,
      })
      .then((res) => {
        if (!cancelled) setSamples(res.items.slice(0, 12));
      })
      .catch(() => {
        if (!cancelled) setSamples([]);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [activeGenre]);

  return (
    <div className="p-4 sm:p-6 max-w-[1440px] mx-auto space-y-6 sm:space-y-8">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight flex items-center gap-2">
          <Compass className="w-6 h-6 text-[var(--accent-hover)]" />
          Explore Genres
        </h1>
        <p className="text-sm text-[var(--text-secondary)] mt-0.5">
          Curated thematic shelves across all enabled online libraries.
        </p>
      </div>

      {/* Genre Cards */}
      <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-3">
        {EXPLORE_GENRES.map((g) => {
          const isSelected = activeGenre === g.name;
          return (
            <button
              key={g.name}
              type="button"
              onClick={() => setActiveGenre(g.name)}
              className={`text-left p-4 rounded-xl border transition-all ${
                isSelected
                  ? "border-[var(--accent-border)] bg-[var(--bg-elevated)]"
                  : "border-[var(--border-subtle)] bg-[var(--bg-surface)] hover:border-[var(--border-strong)]"
              }`}
            >
              <div className="flex items-center justify-between">
                <span className="text-sm font-semibold">{g.name}</span>
                <span
                  className="w-2.5 h-2.5 rounded-full"
                  style={{ backgroundColor: g.color }}
                />
              </div>
              <p className="mt-1.5 text-xs text-[var(--text-secondary)] line-clamp-2">
                {g.tagline}
              </p>
            </button>
          );
        })}
      </div>

      {/* Selected Genre Preview */}
      <section className="space-y-4">
        <div className="flex items-center justify-between">
          <h2 className="text-lg font-semibold">Top {activeGenre}</h2>
          <button
            type="button"
            onClick={() => onOpenGenreInDiscover(activeGenre)}
            className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium bg-[var(--accent-soft)] text-[var(--accent-hover)] hover:bg-[var(--accent)] hover:text-white transition-colors"
          >
            Browse full {activeGenre} catalog <ArrowRight className="w-3.5 h-3.5" />
          </button>
        </div>

        {loading ? (
          <div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-6 gap-3.5">
            {Array.from({ length: 6 }).map((_, i) => (
              <div
                key={i}
                className="aspect-[2/3] rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-surface)] animate-pulse"
              />
            ))}
          </div>
        ) : samples.length === 0 ? (
          <div className="py-12 text-center text-sm text-[var(--text-secondary)] rounded-xl border border-dashed border-[var(--border-strong)]">
            No titles found for {activeGenre} right now. Try checking your active sources in Settings.
          </div>
        ) : (
          <div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-6 gap-3.5">
            {samples.map((novel) => (
              <NovelSummaryCard
                key={novel.id}
                novel={novel}
                onOpen={onOpenNovel}
                compact
              />
            ))}
          </div>
        )}
      </section>
    </div>
  );
};
