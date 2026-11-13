import React, { useEffect, useState } from "react";
import {
  Download,
  HardDrive,
  Pause,
  Play,
  RefreshCw,
  RotateCcw,
  Trash2,
  X,
} from "lucide-react";
import { api, extractErrorMessage } from "../api/tauri";
import { CoverImage } from "../components/CoverImage";
import type { DownloadTask, StorageStats } from "../types/novel";
import { formatBytes, formatRelativeDate, formatSpeed } from "../utils/format";

interface DownloadsPageProps {
  onOpenNovel: (novelId: string) => void;
  onNotify: (msg: string, isError?: boolean) => void;
}

export const DownloadsPage: React.FC<DownloadsPageProps> = ({
  onOpenNovel,
  onNotify,
}) => {
  const [tasks, setTasks] = useState<DownloadTask[]>([]);
  const [storage, setStorage] = useState<StorageStats | null>(null);
  const [loading, setLoading] = useState(true);

  const loadAll = async () => {
    setLoading(true);
    try {
      const [t, s] = await Promise.all([api.listDownloads(), api.getStorageStats()]);
      setTasks(t);
      setStorage(s);
    } catch (e) {
      onNotify(extractErrorMessage(e), true);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    loadAll();
    let unlisten: (() => void) | undefined;
    api
      .onDownloadUpdated((updated) => {
        setTasks((prev) => {
          const idx = prev.findIndex((x) => x.id === updated.id);
          if (idx === -1) return [updated, ...prev];
          const copy = [...prev];
          copy[idx] = updated;
          return copy;
        });
        if (updated.status === "completed") {
          api.getStorageStats().then(setStorage).catch(() => {});
        }
      })
      .then((fn) => {
        unlisten = fn;
      });
    return () => {
      unlisten?.();
    };
  }, []);

  const handleAction = async (fn: () => Promise<unknown>, successMsg?: string) => {
    try {
      await fn();
      if (successMsg) onNotify(successMsg);
      await loadAll();
    } catch (e) {
      onNotify(extractErrorMessage(e), true);
    }
  };

  const statusBadge = (status: DownloadTask["status"]) => {
    switch (status) {
      case "downloading":
        return "bg-indigo-500/15 text-indigo-300 border-indigo-500/30";
      case "completed":
        return "bg-emerald-500/15 text-emerald-300 border-emerald-500/30";
      case "paused":
        return "bg-amber-500/15 text-amber-300 border-amber-500/30";
      case "failed":
        return "bg-rose-500/15 text-rose-300 border-rose-500/30";
      default:
        return "bg-zinc-500/15 text-zinc-300 border-zinc-500/30";
    }
  };

  return (
    <div className="p-6 max-w-6xl mx-auto space-y-8">
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">Downloads & Offline Storage</h1>
          <p className="text-sm text-[var(--text-secondary)] mt-0.5">
            Manage queued chapter downloads and local disk usage. Deleting offline files preserves reading progress.
          </p>
        </div>
        <button
          type="button"
          onClick={loadAll}
          className="inline-flex items-center gap-1.5 px-3 py-2 rounded-lg text-xs font-medium border border-[var(--border-subtle)] bg-[var(--bg-surface)] hover:bg-[var(--bg-elevated)] self-start"
        >
          <RefreshCw className={`w-3.5 h-3.5 ${loading ? "animate-spin" : ""}`} />
          Refresh
        </button>
      </div>

      {/* Storage Overview Card */}
      {storage && (
        <div className="p-5 rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-surface)] space-y-4">
          <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-2">
            <div className="flex items-center gap-2.5">
              <div className="p-2 rounded-lg bg-[var(--accent-soft)] text-[var(--accent-hover)]">
                <HardDrive className="w-4 h-4" />
              </div>
              <div>
                <div className="text-sm font-semibold">
                  {formatBytes(storage.total_bytes)} Used Locally
                </div>
                <div className="text-xs text-[var(--text-muted)] truncate max-w-md">
                  {storage.download_dir}
                </div>
              </div>
            </div>
            <div className="flex flex-wrap gap-4 text-xs text-[var(--text-secondary)]">
              <span>Chapters: {formatBytes(storage.chapters_bytes)}</span>
              <span>Covers: {formatBytes(storage.covers_bytes)}</span>
              <span>Database: {formatBytes(storage.database_bytes)}</span>
            </div>
          </div>

          <div className="h-2 rounded-full bg-[var(--bg-elevated)] overflow-hidden flex">
            {storage.total_bytes > 0 && (
              <>
                <div
                  className="h-full"
                  style={{
                    width: `${(storage.chapters_bytes / storage.total_bytes) * 100}%`,
                    backgroundColor: "var(--accent)",
                  }}
                />
                <div
                  className="h-full bg-emerald-500"
                  style={{
                    width: `${(storage.covers_bytes / storage.total_bytes) * 100}%`,
                  }}
                />
                <div
                  className="h-full bg-amber-500"
                  style={{
                    width: `${(storage.database_bytes / storage.total_bytes) * 100}%`,
                  }}
                />
              </>
            )}
          </div>
        </div>
      )}

      {/* Download Queue */}
      <section className="space-y-3">
        <h2 className="text-base font-semibold flex items-center gap-2">
          <Download className="w-4 h-4 text-[var(--accent-hover)]" />
          Download Queue ({tasks.length})
        </h2>

        {tasks.length === 0 ? (
          <div className="p-8 text-center rounded-xl border border-dashed border-[var(--border-strong)] text-xs text-[var(--text-muted)]">
            No downloads yet. Open any novel and click Download All or select specific chapters.
          </div>
        ) : (
          <div className="rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-surface)] divide-y divide-[var(--border-subtle)] overflow-hidden">
            {tasks.map((task) => {
              const pct =
                task.total_chapters > 0
                  ? Math.round((task.completed_chapters / task.total_chapters) * 100)
                  : 0;
              return (
                <div key={task.id} className="p-4 flex flex-col sm:flex-row sm:items-center gap-4">
                  <button
                    type="button"
                    onClick={() => onOpenNovel(task.novel_id)}
                    className="w-10 h-14 rounded overflow-hidden shrink-0"
                  >
                    <CoverImage
                      title={task.novel_title}
                      coverUrl={task.cover_url}
                      localCoverPath={task.local_cover_path}
                      size="sm"
                      className="w-full h-full"
                    />
                  </button>

                  <div className="flex-1 min-w-0 space-y-1.5">
                    <div className="flex items-center justify-between gap-2">
                      <button
                        type="button"
                        onClick={() => onOpenNovel(task.novel_id)}
                        className="text-sm font-medium truncate hover:text-[var(--accent-hover)] text-left"
                      >
                        {task.novel_title}
                      </button>
                      <span
                        className={`px-2 py-0.5 rounded-full text-[10px] font-medium border capitalize shrink-0 ${statusBadge(
                          task.status,
                        )}`}
                      >
                        {task.status}
                      </span>
                    </div>

                    <div className="h-1.5 rounded-full bg-[var(--bg-elevated)] overflow-hidden">
                      <div
                        className="h-full transition-all duration-200"
                        style={{
                          width: `${pct}%`,
                          backgroundColor:
                            task.status === "failed" ? "#FB7185" : "var(--accent)",
                        }}
                      />
                    </div>

                    <div className="flex flex-wrap items-center justify-between text-[11px] text-[var(--text-muted)]">
                      <span>
                        {task.completed_chapters} / {task.total_chapters} chapters ({pct}%) ·{" "}
                        {formatBytes(task.bytes_downloaded)}
                        {task.status === "downloading" &&
                          ` · ${formatSpeed(task.speed_bytes_per_sec)}`}
                      </span>
                      <span>{formatRelativeDate(task.updated_at)}</span>
                    </div>
                    {task.error_message && (
                      <div className="text-[11px] text-rose-300">{task.error_message}</div>
                    )}
                  </div>

                  <div className="flex items-center gap-1.5 shrink-0">
                    {task.status === "downloading" && (
                      <button
                        type="button"
                        title="Pause"
                        onClick={() => handleAction(() => api.pauseDownload(task.id))}
                        className="p-2 rounded-lg border border-[var(--border-subtle)] hover:bg-[var(--bg-elevated)]"
                      >
                        <Pause className="w-3.5 h-3.5" />
                      </button>
                    )}
                    {task.status === "paused" && (
                      <button
                        type="button"
                        title="Resume"
                        onClick={() => handleAction(() => api.resumeDownload(task.id))}
                        className="p-2 rounded-lg border border-[var(--border-subtle)] hover:bg-[var(--bg-elevated)] text-emerald-400"
                      >
                        <Play className="w-3.5 h-3.5" />
                      </button>
                    )}
                    {(task.status === "failed" || task.status === "cancelled") && (
                      <button
                        type="button"
                        title="Retry Failed"
                        onClick={() =>
                          handleAction(() => api.retryDownload(task.id), "Retrying download")
                        }
                        className="p-2 rounded-lg border border-[var(--border-subtle)] hover:bg-[var(--bg-elevated)] text-amber-300"
                      >
                        <RotateCcw className="w-3.5 h-3.5" />
                      </button>
                    )}
                    {(task.status === "downloading" ||
                      task.status === "paused" ||
                      task.status === "queued") && (
                      <button
                        type="button"
                        title="Cancel"
                        onClick={() => handleAction(() => api.cancelDownload(task.id))}
                        className="p-2 rounded-lg border border-[var(--border-subtle)] hover:bg-rose-500/15 text-rose-300"
                      >
                        <X className="w-3.5 h-3.5" />
                      </button>
                    )}
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </section>

      {/* Per-Novel Storage Management */}
      <section className="space-y-3">
        <h2 className="text-base font-semibold">Storage Used Per Novel</h2>
        {!storage || storage.novels.length === 0 ? (
          <div className="p-8 text-center rounded-xl border border-dashed border-[var(--border-strong)] text-xs text-[var(--text-muted)]">
            No novels stored offline yet.
          </div>
        ) : (
          <div className="rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-surface)] divide-y divide-[var(--border-subtle)] overflow-hidden">
            {storage.novels.map((n) => (
              <div
                key={n.novel_id}
                className="px-4 py-3 flex items-center justify-between gap-4 hover:bg-[var(--bg-hover)]"
              >
                <div className="flex items-center gap-3 min-w-0">
                  <CoverImage
                    title={n.title}
                    coverUrl={n.cover_url}
                    localCoverPath={n.local_cover_path}
                    size="sm"
                    className="w-9 h-12 rounded shrink-0"
                  />
                  <div className="min-w-0">
                    <button
                      type="button"
                      onClick={() => onOpenNovel(n.novel_id)}
                      className="text-sm font-medium truncate hover:text-[var(--accent-hover)] block text-left"
                    >
                      {n.title}
                    </button>
                    <div className="text-xs text-[var(--text-muted)]">
                      {n.author} · {n.downloaded_chapters}/{n.total_chapters} chapters offline
                    </div>
                  </div>
                </div>
                <div className="flex items-center gap-3 shrink-0">
                  <span className="text-xs font-medium tabular-nums">
                    {formatBytes(n.size_bytes)}
                  </span>
                  <button
                    type="button"
                    onClick={() =>
                      handleAction(
                        () => api.deleteDownloadedNovel(n.novel_id),
                        `Cleared offline chapters for "${n.title}" (progress preserved)`,
                      )
                    }
                    className="inline-flex items-center gap-1 px-2.5 py-1.5 rounded-lg text-xs border border-rose-500/30 bg-rose-500/10 text-rose-300 hover:bg-rose-500/20"
                  >
                    <Trash2 className="w-3.5 h-3.5" />
                    Delete
                  </button>
                </div>
              </div>
            ))}
          </div>
        )}
      </section>
    </div>
  );
};
