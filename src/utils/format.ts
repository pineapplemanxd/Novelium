export function formatBytes(bytes: number): string {
  if (!bytes || bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  let idx = 0;
  let val = bytes;
  while (val >= 1024 && idx < units.length - 1) {
    val /= 1024;
    idx++;
  }
  return `${val.toFixed(idx === 0 ? 0 : 1)} ${units[idx]}`;
}

export function formatSpeed(bps: number): string {
  if (!bps || bps <= 0) return "—";
  return `${formatBytes(bps)}/s`;
}

export function formatDuration(seconds: number): string {
  if (!seconds || seconds <= 0) return "0m";
  const hrs = Math.floor(seconds / 3600);
  const mins = Math.floor((seconds % 3600) / 60);
  if (hrs > 0) return `${hrs}h ${mins}m`;
  if (mins > 0) return `${mins}m`;
  return `${seconds}s`;
}

export function formatRelativeDate(iso?: string | null): string {
  if (!iso) return "—";
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  const diffMs = Date.now() - date.getTime();
  const mins = Math.floor(diffMs / 60000);
  if (mins < 1) return "Just now";
  if (mins < 60) return `${mins}m ago`;
  const hrs = Math.floor(mins / 60);
  if (hrs < 24) return `${hrs}h ago`;
  const days = Math.floor(hrs / 24);
  if (days < 7) return `${days}d ago`;
  return date.toLocaleDateString(undefined, { month: "short", day: "numeric", year: "numeric" });
}

export function formatNumber(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(n);
}

export function estimateReadingMinutes(words: number): number {
  return Math.max(1, Math.round(words / 240));
}

export const IS_MAC =
  typeof navigator !== "undefined" &&
  /Mac|iPhone|iPad|iPod/i.test(navigator.platform || navigator.userAgent);

export const MOD_KEY = IS_MAC ? "⌘" : "Ctrl+";

export interface AnnotationSwatch {
  id: string;
  label: string;
  hex: string;
  markBg: string;
  markBorder: string;
}

export const ANNOTATION_COLORS: AnnotationSwatch[] = [
  { id: "amber", label: "Gold", hex: "#F59E0B", markBg: "rgba(245, 158, 11, 0.28)", markBorder: "rgba(245, 158, 11, 0.7)" },
  { id: "rose", label: "Rose", hex: "#F43F5E", markBg: "rgba(244, 63, 94, 0.25)", markBorder: "rgba(244, 63, 94, 0.65)" },
  { id: "emerald", label: "Emerald", hex: "#10B981", markBg: "rgba(16, 185, 129, 0.25)", markBorder: "rgba(16, 185, 129, 0.65)" },
  { id: "sky", label: "Sky", hex: "#38BDF8", markBg: "rgba(56, 189, 248, 0.25)", markBorder: "rgba(56, 189, 248, 0.65)" },
  { id: "violet", label: "Violet", hex: "#A78BFA", markBg: "rgba(167, 139, 250, 0.25)", markBorder: "rgba(167, 139, 250, 0.65)" },
];

export function swatchForColor(colorId?: string | null): AnnotationSwatch {
  return (
    ANNOTATION_COLORS.find((c) => c.id === colorId) ?? ANNOTATION_COLORS[0]
  );
}

export const APP_LANGUAGES: { code: string; label: string }[] = [
  { code: "en", label: "English" },
  { code: "pl", label: "Polish (Polski)" },
  { code: "fr", label: "French (Français)" },
  { code: "de", label: "German (Deutsch)" },
  { code: "es", label: "Spanish (Español)" },
  { code: "it", label: "Italian (Italiano)" },
  { code: "pt", label: "Portuguese (Português)" },
  { code: "ru", label: "Russian (Русский)" },
  { code: "ja", label: "Japanese (日本語)" },
  { code: "zh", label: "Chinese (中文)" },
];

export function languageLabel(code?: string | null, fallback?: string | null): string {
  if (!code) return fallback || "English";
  const norm = code.trim().toLowerCase();
  const hit = APP_LANGUAGES.find((l) => l.code === norm);
  return hit ? hit.label : fallback || norm.toUpperCase();
}

export const OPEN_BOOK_SOURCE_IDS = [
  "gutenberg",
  "standardebooks",
  "archive_org",
  "wikisource",
] as const;

export function sourceLabel(sourceId: string): string {
  switch (sourceId) {
    case "gutenberg":
      return "Project Gutenberg";
    case "standardebooks":
      return "Standard Ebooks";
    case "wikisource":
      return "Wikisource";
    case "archive_org":
      return "Internet Archive";
    case "fanmtl":
      return "FanMTL";
    case "wuxiabox":
      return "WuxiaBox";
    case "nobadnovel":
      return "NoBadNovel";
    case "wuxia_click":
      return "Wuxia.Click";
    case "hiraeth":
      return "Hiraeth Translation";
    case "readnovelfull":
      return "ReadNovelFull";
    case "local":
      return "Local Import";
    default:
      return sourceId;
  }
}

export function libraryStatusLabel(status: string): string {
  switch (status) {
    case "reading":
      return "Currently Reading";
    case "plan_to_read":
      return "Plan to Read";
    case "finished":
      return "Finished";
    case "on_hold":
      return "On Hold";
    case "dropped":
      return "Dropped";
    default:
      return status;
  }
}
