import React from "react";
import { BookOpen, CheckCircle2, Download, Sparkles } from "lucide-react";
import type { LibraryNovelItem, NovelSummary } from "../types/novel";
import { formatNumber, sourceLabel } from "../utils/format";
import { CoverImage } from "./CoverImage";

interface SummaryCardProps {
  novel: NovelSummary;
  onOpen: (novel: NovelSummary) => void;
  compact?: boolean;
}

export const NovelSummaryCard: React.FC<SummaryCardProps> = ({
  novel,
  onOpen,
  compact = false,
}) => {
  return (
    <button
      type="button"
      onClick={() => onOpen(novel)}
      className="group text-left flex flex-col rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-surface)] hover:border-[var(--accent-border)] hover:bg-[var(--bg-elevated)] transition-all duration-150 overflow-hidden focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent)]"
    >
      <div className="relative aspect-[2/3] w-full overflow-hidden bg-[var(--bg-elevated)]">
        <CoverImage
          title={novel.title}
          author={novel.author}
          coverUrl={novel.cover_url}
          localCoverPath={novel.local_cover_path}
          className="w-full h-full transition-transform duration-300 group-hover:scale-[1.03]"
        />
        <div className="absolute top-2 left-2 flex items-center gap-1">
          <span className="px-1.5 py-0.5 rounded-md text-[10px] font-medium bg-black/70 text-zinc-200 backdrop-blur-sm border border-white/10">
            {sourceLabel(novel.source_id)}
          </span>
          {novel.language && (
            <span className="px-1.5 py-0.5 rounded-md text-[10px] font-medium uppercase bg-black/70 text-zinc-200 backdrop-blur-sm border border-white/10">
              {novel.language}
            </span>
          )}
        </div>
        {novel.status === "completed" && (
          <span className="absolute bottom-2 right-2 px-1.5 py-0.5 rounded text-[10px] font-medium bg-emerald-950/80 text-emerald-300 border border-emerald-500/30 backdrop-blur-sm">
            Completed
          </span>
        )}
      </div>

      <div className={compact ? "p-2.5 flex-1 flex flex-col" : "p-3 flex-1 flex flex-col"}>
        <h3 className="text-sm font-medium text-[var(--text-primary)] line-clamp-2 leading-snug group-hover:text-[var(--accent-hover)] transition-colors">
          {novel.title}
        </h3>
        <p className="mt-1 text-xs text-[var(--text-secondary)] truncate">{novel.author}</p>

        {!compact && novel.description && (
          <p className="mt-2 text-xs text-[var(--text-muted)] line-clamp-2 leading-relaxed">
            {novel.description}
          </p>
        )}

        <div className="mt-auto pt-2.5 flex items-center justify-between gap-2 text-[11px] text-[var(--text-muted)]">
          <span className="truncate">
            {novel.genres[0] || "Fiction"}
          </span>
          {novel.popularity > 0 && (
            <span className="shrink-0 flex items-center gap-1">
              <Sparkles className="w-3 h-3 opacity-70" />
              {formatNumber(novel.popularity)}
            </span>
          )}
        </div>
      </div>
    </button>
  );
};

interface LibraryCardProps {
  item: LibraryNovelItem;
  onOpen: (novelId: string) => void;
  onContinue: (novelId: string, chapterId?: string) => void;
}

export const LibraryNovelCard: React.FC<LibraryCardProps> = ({
  item,
  onOpen,
  onContinue,
}) => {
  const { novel, progress, downloaded_chapters } = item;
  const pct = progress ? Math.round(progress.progress_percent) : 0;

  return (
    <div className="group flex flex-col rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-surface)] hover:border-[var(--accent-border)] transition-all duration-150 overflow-hidden">
      <button
        type="button"
        onClick={() => onOpen(novel.id)}
        className="relative aspect-[2/3] w-full overflow-hidden bg-[var(--bg-elevated)] text-left focus-visible:outline-none"
      >
        <CoverImage
          title={novel.title}
          author={novel.author}
          coverUrl={novel.cover_url}
          localCoverPath={novel.local_cover_path}
          className="w-full h-full transition-transform duration-300 group-hover:scale-[1.03]"
        />
        <div className="absolute top-2 left-2 flex flex-wrap gap-1">
          {downloaded_chapters > 0 && (
            <span className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded-md text-[10px] font-medium bg-black/75 text-emerald-300 border border-emerald-500/30 backdrop-blur-sm">
              <Download className="w-2.5 h-2.5" />
              {downloaded_chapters}/{novel.chapter_count || downloaded_chapters}
            </span>
          )}
        </div>
        {item.entry.status === "finished" && (
          <span className="absolute top-2 right-2 p-1 rounded-full bg-emerald-600/90 text-white">
            <CheckCircle2 className="w-3.5 h-3.5" />
          </span>
        )}
        {pct > 0 && (
          <div className="absolute inset-x-0 bottom-0 h-1 bg-black/50">
            <div
              className="h-full transition-all"
              style={{ width: `${pct}%`, backgroundColor: "var(--accent)" }}
            />
          </div>
        )}
      </button>

      <div className="p-3 flex-1 flex flex-col">
        <button
          type="button"
          onClick={() => onOpen(novel.id)}
          className="text-left focus-visible:outline-none"
        >
          <h3 className="text-sm font-medium text-[var(--text-primary)] line-clamp-2 leading-snug group-hover:text-[var(--accent-hover)] transition-colors">
            {novel.title}
          </h3>
          <p className="mt-0.5 text-xs text-[var(--text-secondary)] truncate">{novel.author}</p>
        </button>

        <div className="mt-auto pt-2.5 flex items-center justify-between gap-2">
          <span className="text-[11px] text-[var(--text-muted)] truncate">
            {progress
              ? `Ch. ${Math.round(progress.chapter_number)} · ${pct}%`
              : `${novel.chapter_count || "—"} chapters`}
          </span>
          <button
            type="button"
            onClick={(e) => {
              e.stopPropagation();
              onContinue(novel.id, progress?.chapter_id);
            }}
            title={progress ? "Continue Reading" : "Start Reading"}
            className="inline-flex items-center gap-1 px-2 py-1 rounded-md text-[11px] font-medium bg-[var(--accent-soft)] text-[var(--accent-hover)] hover:bg-[var(--accent)] hover:text-white transition-colors"
          >
            <BookOpen className="w-3 h-3" />
            {progress ? "Read" : "Start"}
          </button>
        </div>
      </div>
    </div>
  );
};
