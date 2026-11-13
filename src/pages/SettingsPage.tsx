import React, { useEffect, useState } from "react";
import {
  Activity,
  BookOpen,
  CheckCircle2,
  Cloud,
  Database,
  Download,
  FolderOpen,
  Globe,
  Keyboard,
  Palette,
  Plus,
  RotateCcw,
  ShieldCheck,
  Sliders,
  Trash2,
  Upload,
  Wifi,
  XCircle,
} from "lucide-react";
import { api, extractErrorMessage } from "../api/tauri";
import { GoogleDriveSyncSection } from "../components/GoogleDriveSyncSection";
import { APP_LANGUAGES, IS_IOS, MOD_KEY } from "../utils/format";
import type {
  AppSettings,
  DeclarativeSourceManifest,
  DiagnosticsInfo,
  SourceHealthResult,
  SourceInfo,
} from "../types/novel";

interface SettingsPageProps {
  settings: AppSettings;
  onUpdateSettings: (next: AppSettings) => void;
  onNotify: (msg: string, isError?: boolean) => void;
}

type SettingsTab = "appearance" | "reader" | "downloads" | "sources" | "sync" | "general";

const SIDEBAR_LABELS: Record<string, string> = {
  home: "Home",
  discover: "Discover",
  explore: "Explore",
  library: "Library",
  downloads: "Downloads",
  history: "Reading History",
  bookmarks: "Bookmarks",
};

export const SettingsPage: React.FC<SettingsPageProps> = ({
  settings,
  onUpdateSettings,
  onNotify,
}) => {
  const [tab, setTab] = useState<SettingsTab>("appearance");
  const [sources, setSources] = useState<SourceInfo[]>([]);
  const [healthResults, setHealthResults] = useState<Record<string, SourceHealthResult>>({});
  const [testingId, setTestingId] = useState<string | null>(null);
  const [diagnostics, setDiagnostics] = useState<DiagnosticsInfo | null>(null);

  // Custom Source Creator modal/drawer
  const emptyManifest = (
    kind: DeclarativeSourceManifest["kind"] = "opds",
  ): DeclarativeSourceManifest => ({
    id: "",
    name: "",
    kind,
    base_url: "",
    description: "",
    genres: ["Fiction"],
    feed_url: "",
    search_url_template: "",
    browse_url_template: "",
    novel_list_selector: "",
    title_selector: "",
    author_selector: "",
    cover_selector: "",
    link_selector: "",
    description_selector: "",
    chapter_list_selector: "",
    chapter_title_selector: "",
    chapter_link_selector: "",
    content_selector: kind === "auto" ? "" : "article",
    remove_selectors: [],
    language: "",
  });

  const [showAddSource, setShowAddSource] = useState(false);
  const [customManifest, setCustomManifest] = useState<DeclarativeSourceManifest>(() =>
    emptyManifest("opds"),
  );

  const loadSources = async () => {
    try {
      setSources(await api.listSources());
    } catch (e) {
      onNotify(extractErrorMessage(e), true);
    }
  };

  useEffect(() => {
    loadSources();
    api.getDiagnostics().then(setDiagnostics).catch(() => {});
  }, []);

  const updateField = <K extends keyof AppSettings>(key: K, value: AppSettings[K]) => {
    onUpdateSettings({ ...settings, [key]: value });
  };

  const toggleSidebarSection = (id: string) => {
    const current = settings.sidebar_sections;
    const next = current.includes(id)
      ? current.filter((x) => x !== id)
      : [...current, id];
    updateField("sidebar_sections", next);
  };

  const handleToggleSource = async (src: SourceInfo) => {
    try {
      await api.setSourceEnabled(src.id, !src.enabled);
      await loadSources();
      onNotify(`${src.name} ${!src.enabled ? "enabled" : "disabled"}`);
    } catch (e) {
      onNotify(extractErrorMessage(e), true);
    }
  };

  const handleUpdateSourceConfig = async (
    src: SourceInfo,
    key: string,
    val: unknown,
  ) => {
    const nextCfg = { ...src.config, [key]: val };
    try {
      await api.updateSourceConfig(src.id, nextCfg);
      await loadSources();
      onNotify(`Saved ${src.name} settings`);
    } catch (e) {
      onNotify(extractErrorMessage(e), true);
    }
  };

  const handleTestSource = async (id: string) => {
    setTestingId(id);
    try {
      const res = await api.testSourceConnection(id);
      setHealthResults((prev) => ({ ...prev, [id]: res }));
      await loadSources();
    } catch (e) {
      onNotify(extractErrorMessage(e), true);
    } finally {
      setTestingId(null);
    }
  };

  const handleInstallCustomSource = async (e: React.FormEvent) => {
    e.preventDefault();
    try {
      const opt = (v?: string) => {
        const t = v?.trim();
        return t ? t : undefined;
      };
      const payload: DeclarativeSourceManifest = {
        ...customManifest,
        browse_url_template: opt(customManifest.browse_url_template),
        search_url_template: opt(customManifest.search_url_template),
        feed_url: opt(customManifest.feed_url),
        novel_list_selector: opt(customManifest.novel_list_selector),
        title_selector: opt(customManifest.title_selector),
        author_selector: opt(customManifest.author_selector),
        cover_selector: opt(customManifest.cover_selector),
        link_selector: opt(customManifest.link_selector),
        description_selector: opt(customManifest.description_selector),
        chapter_list_selector: opt(customManifest.chapter_list_selector),
        chapter_title_selector: opt(customManifest.chapter_title_selector),
        chapter_link_selector: opt(customManifest.chapter_link_selector),
        content_selector: opt(customManifest.content_selector),
        language: opt(customManifest.language),
      };
      const installed = await api.installCustomSource(payload);
      setShowAddSource(false);
      setCustomManifest(emptyManifest("opds"));
      await loadSources();
      onNotify(
        `Installed custom source "${installed?.name || customManifest.name || customManifest.base_url}"`,
      );
    } catch (err) {
      onNotify(extractErrorMessage(err), true);
    }
  };

  const handleRemoveCustomSource = async (id: string) => {
    try {
      await api.removeCustomSource(id);
      await loadSources();
      onNotify(`Removed custom source "${id}"`);
    } catch (err) {
      onNotify(extractErrorMessage(err), true);
    }
  };

  const handlePickDownloadDir = async () => {
    try {
      const dir = await api.pickDirectory("Choose Novelium Offline Download Location");
      if (dir) {
        updateField("download_dir", dir);
        onNotify("Updated download directory");
      }
    } catch (e) {
      onNotify(extractErrorMessage(e), true);
    }
  };

  const handleBackupDb = async () => {
    try {
      const dest = await api.pickSavePath("novelium-backup.db", "db", "SQLite Database");
      if (!dest) return;
      const bytes = await api.backupDatabaseFile(dest);
      onNotify(`Database backed up (${bytes.toLocaleString()} bytes)`);
    } catch (e) {
      onNotify(extractErrorMessage(e), true);
    }
  };

  const handleExportJson = async () => {
    try {
      const dest = await api.pickSavePath("novelium-library.json", "json", "JSON Library Backup");
      if (!dest) return;
      await api.exportLibraryJson(dest);
      onNotify("Exported library metadata, bookmarks, collections & reading progress");
    } catch (e) {
      onNotify(extractErrorMessage(e), true);
    }
  };

  const handleImportJson = async () => {
    try {
      const path = await api.pickOpenJsonFile();
      if (!path) return;
      const res = await api.importLibraryJson(path);
      if (typeof api.getSettings === "function") {
        const restored = await api.getSettings().catch(() => null);
        if (restored) {
          onUpdateSettings(restored);
        }
      }
      onNotify(`Imported ${res.novels_imported} novel(s) and ${res.entries_imported} library entries`);
    } catch (e) {
      onNotify(extractErrorMessage(e), true);
    }
  };

  const handleResetSettings = async () => {
    try {
      const defaults = await api.resetSettings();
      onUpdateSettings(defaults);
      onNotify("Reset settings to defaults");
    } catch (e) {
      onNotify(extractErrorMessage(e), true);
    }
  };

  return (
    <div className="p-4 sm:p-6 max-w-5xl mx-auto space-y-6">
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">Settings</h1>
          <p className="text-sm text-[var(--text-secondary)] mt-0.5">
            Customize Novelium’s interface, reader typography, downloads, sources, and backups.
          </p>
        </div>
        <button
          type="button"
          onClick={handleResetSettings}
          className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs border border-[var(--border-subtle)] bg-[var(--bg-surface)] hover:bg-[var(--bg-elevated)] text-[var(--text-secondary)]"
        >
          <RotateCcw className="w-3.5 h-3.5" />
          Reset Defaults
        </button>
      </div>

      {/* Category Tabs */}
      <div className="flex flex-wrap gap-1.5 border-b border-[var(--border-subtle)] pb-3">
        {(
          [
            ["appearance", "Appearance", Palette],
            ["reader", "Reader", BookOpen],
            ["downloads", "Downloads", Download],
            ["sources", "Sources", Globe],
            ["sync", "Cloud Sync", Cloud],
            ["general", "General & Backups", Sliders],
          ] as [SettingsTab, string, React.ElementType][]
        ).map(([id, label, Icon]) => (
          <button
            key={id}
            type="button"
            onClick={() => setTab(id)}
            className={`inline-flex items-center gap-2 px-3.5 py-2 rounded-lg text-xs font-medium transition-colors ${
              tab === id
                ? "text-white"
                : "bg-[var(--bg-surface)] text-[var(--text-secondary)] border border-[var(--border-subtle)] hover:text-[var(--text-primary)]"
            }`}
            style={tab === id ? { backgroundColor: "var(--accent)" } : undefined}
          >
            <Icon className="w-3.5 h-3.5" />
            {label}
          </button>
        ))}
      </div>

      {/* Appearance */}
      {tab === "appearance" && (
        <div className="space-y-6 rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-surface)] p-6">
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-6">
            <div className="space-y-2">
              <label className="text-xs font-semibold uppercase tracking-wider text-[var(--text-muted)]">
                Interface Theme
              </label>
              <div className="grid grid-cols-3 gap-2">
                {(["dark", "light", "system"] as const).map((t) => (
                  <button
                    key={t}
                    type="button"
                    onClick={() => updateField("theme", t)}
                    className={`py-2 px-3 rounded-lg text-xs font-medium border ${
                      settings.theme === t
                        ? "border-[var(--accent)] bg-[var(--accent-soft)] text-[var(--accent-hover)]"
                        : "border-[var(--border-subtle)] bg-[var(--bg-elevated)]"
                    }`}
                  >
                    {t === "light" ? "White" : t === "dark" ? "Dark" : "System"}
                  </button>
                ))}
              </div>
            </div>

            <div className="space-y-2">
              <label className="text-xs font-semibold uppercase tracking-wider text-[var(--text-muted)]">
                Accent Color
              </label>
              <div className="flex items-center gap-2.5 pt-1">
                {(
                  [
                    ["indigo", "#6366F1"],
                    ["amber", "#D97706"],
                    ["emerald", "#10B981"],
                    ["rose", "#E11D48"],
                    ["cyan", "#0EA5E9"],
                  ] as const
                ).map(([name, hex]) => (
                  <button
                    key={name}
                    type="button"
                    aria-label={`Accent ${name}`}
                    onClick={() => updateField("accent_color", name)}
                    className={`w-8 h-8 rounded-full border-2 transition-transform ${
                      settings.accent_color === name
                        ? "scale-110 border-white"
                        : "border-transparent opacity-75 hover:opacity-100"
                    }`}
                    style={{ backgroundColor: hex }}
                  />
                ))}
              </div>
            </div>

            <div className="space-y-2">
              <label className="text-xs font-semibold uppercase tracking-wider text-[var(--text-muted)]">
                Interface Density
              </label>
              <div className="grid grid-cols-2 gap-2">
                {(["comfortable", "compact"] as const).map((d) => (
                  <button
                    key={d}
                    type="button"
                    onClick={() => updateField("density", d)}
                    className={`py-2 px-3 rounded-lg text-xs font-medium border capitalize ${
                      settings.density === d
                        ? "border-[var(--accent)] bg-[var(--accent-soft)] text-[var(--accent-hover)]"
                        : "border-[var(--border-subtle)] bg-[var(--bg-elevated)]"
                    }`}
                  >
                    {d}
                  </button>
                ))}
              </div>
            </div>
          </div>

          <div className="pt-4 border-t border-[var(--border-subtle)] space-y-2">
            <label className="text-xs font-semibold uppercase tracking-wider text-[var(--text-muted)]">
              Visible Sidebar Sections
            </label>
            <div className="grid grid-cols-2 sm:grid-cols-4 gap-2">
              {Object.entries(SIDEBAR_LABELS).map(([id, label]) => (
                <label
                  key={id}
                  className="flex items-center gap-2 px-3 py-2 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-elevated)] text-xs cursor-pointer"
                >
                  <input
                    type="checkbox"
                    checked={settings.sidebar_sections.includes(id)}
                    onChange={() => toggleSidebarSection(id)}
                  />
                  {label}
                </label>
              ))}
            </div>
          </div>
        </div>
      )}

      {/* Reader Defaults */}
      {tab === "reader" && (
        <div className="space-y-5 rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-surface)] p-6">
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-5">
            <label className="space-y-1.5 text-xs">
              <span className="font-medium text-[var(--text-secondary)]">Default Font</span>
              <select
                value={settings.reader_font}
                onChange={(e) => updateField("reader_font", e.target.value)}
                className="w-full px-3 py-2 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-elevated)]"
              >
                {[
                  "Literata",
                  "Newsreader",
                  "Instrument Serif",
                  "Charter",
                  "Iowan Old Style",
                  "Georgia",
                  "Palatino Linotype",
                  "Palatino",
                  "Baskerville",
                  "Times New Roman",
                  "Segoe UI",
                  "-apple-system",
                  "Menlo",
                ].map((f) => (
                  <option key={f} value={f}>
                    {f}
                  </option>
                ))}
              </select>
            </label>

            <label className="space-y-1.5 text-xs">
              <span className="font-medium text-[var(--text-secondary)]">Default Reading Theme</span>
              <select
                value={settings.reader_theme}
                onChange={(e) =>
                  updateField("reader_theme", e.target.value as AppSettings["reader_theme"])
                }
                className="w-full px-3 py-2 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-elevated)]"
              >
                <option value="obsidian">Obsidian Dark</option>
                <option value="amoled">AMOLED Pure Black</option>
                <option value="paper">Paper Light</option>
                <option value="sepia">Warm Sepia</option>
                <option value="forest">Forest Night</option>
                <option value="custom">Custom Colors</option>
              </select>
            </label>

            <label className="space-y-1.5 text-xs">
              <span className="font-medium text-[var(--text-secondary)]">
                Reading Mode
              </span>
              <select
                value={settings.reader_mode}
                onChange={(e) =>
                  updateField("reader_mode", e.target.value as AppSettings["reader_mode"])
                }
                className="w-full px-3 py-2 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-elevated)]"
              >
                <option value="scroll">Continuous Scrolling</option>
                <option value="paginated">Book Flip (Paginated)</option>
              </select>
            </label>

            <label className="space-y-1.5 text-xs">
              <span className="font-medium text-[var(--text-secondary)]">
                Default Novel Language
              </span>
              <select
                aria-label="Default Novel Language"
                value={settings.default_language || "en"}
                onChange={(e) => updateField("default_language", e.target.value)}
                className="w-full px-3 py-2 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-elevated)]"
              >
                {APP_LANGUAGES.map((l) => (
                  <option key={l.code} value={l.code}>
                    {l.label}
                  </option>
                ))}
              </select>
            </label>

            <label className="space-y-1.5 text-xs">
              <span className="font-medium text-[var(--text-secondary)]">
                Font Size ({settings.reader_font_size}px)
              </span>
              <input
                type="range"
                min={14}
                max={30}
                value={settings.reader_font_size}
                onChange={(e) => updateField("reader_font_size", Number(e.target.value))}
                className="w-full"
              />
            </label>

            <label className="space-y-1.5 text-xs">
              <span className="font-medium text-[var(--text-secondary)]">
                Line Height ({settings.reader_line_height.toFixed(2)})
              </span>
              <input
                type="range"
                min={1.3}
                max={2.2}
                step={0.05}
                value={settings.reader_line_height}
                onChange={(e) => updateField("reader_line_height", Number(e.target.value))}
                className="w-full"
              />
            </label>

            <label className="space-y-1.5 text-xs">
              <span className="font-medium text-[var(--text-secondary)]">
                Reading Width ({settings.reader_width}px)
              </span>
              <input
                type="range"
                min={520}
                max={960}
                step={20}
                value={settings.reader_width}
                onChange={(e) => updateField("reader_width", Number(e.target.value))}
                className="w-full"
              />
            </label>

            <label className="space-y-1.5 text-xs">
              <span className="font-medium text-[var(--text-secondary)]">
                Paragraph Spacing ({settings.reader_paragraph_spacing.toFixed(2)}em)
              </span>
              <input
                type="range"
                min={0.5}
                max={2.2}
                step={0.05}
                value={settings.reader_paragraph_spacing}
                onChange={(e) =>
                  updateField("reader_paragraph_spacing", Number(e.target.value))
                }
                className="w-full"
              />
            </label>

            <label className="space-y-1.5 text-xs">
              <span className="font-medium text-[var(--text-secondary)]">Text Alignment</span>
              <select
                value={settings.reader_text_align}
                onChange={(e) =>
                  updateField(
                    "reader_text_align",
                    e.target.value as AppSettings["reader_text_align"],
                  )
                }
                className="w-full px-3 py-2 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-elevated)]"
              >
                <option value="left">Left Aligned</option>
                <option value="justify">Justified</option>
              </select>
            </label>
          </div>

          <div className="grid grid-cols-1 sm:grid-cols-3 gap-4 pt-2 border-t border-[var(--border-subtle)]">
            <label className="flex items-center justify-between gap-2 text-xs">
              <span className="font-medium text-[var(--text-secondary)]">
                Page / Background Color
              </span>
              <input
                type="color"
                aria-label="Page Background Color"
                value={settings.reader_custom_bg || "#121318"}
                onChange={(e) =>
                  onUpdateSettings({
                    ...settings,
                    reader_theme: "custom",
                    reader_custom_bg: e.target.value,
                  })
                }
                className="h-8 w-14 rounded border border-[var(--border-subtle)] bg-transparent cursor-pointer"
              />
            </label>

            <label className="flex items-center justify-between gap-2 text-xs">
              <span className="font-medium text-[var(--text-secondary)]">Font Color</span>
              <input
                type="color"
                aria-label="Font Color"
                value={settings.reader_custom_fg || "#E2E4EA"}
                onChange={(e) =>
                  onUpdateSettings({
                    ...settings,
                    reader_theme: "custom",
                    reader_custom_fg: e.target.value,
                  })
                }
                className="h-8 w-14 rounded border border-[var(--border-subtle)] bg-transparent cursor-pointer"
              />
            </label>

            <label className="flex items-center justify-between gap-2 text-xs">
              <span className="font-medium text-[var(--text-secondary)]">Accent Color</span>
              <input
                type="color"
                aria-label="Custom Accent Color"
                value={settings.reader_custom_accent || "#818CF8"}
                onChange={(e) =>
                  onUpdateSettings({
                    ...settings,
                    reader_theme: "custom",
                    reader_custom_accent: e.target.value,
                  })
                }
                className="h-8 w-14 rounded border border-[var(--border-subtle)] bg-transparent cursor-pointer"
              />
            </label>
          </div>

          <div className="space-y-2 pt-2">
            <label className="flex items-center gap-2.5 text-xs cursor-pointer">
              <input
                type="checkbox"
                checked={settings.reader_flip_animation}
                onChange={(e) =>
                  updateField("reader_flip_animation", e.target.checked)
                }
              />
              Enable page-flip animation in Book Flip mode
            </label>
            <label className="flex items-center gap-2.5 text-xs cursor-pointer">
              <input
                type="checkbox"
                checked={settings.reader_auto_load_next}
                onChange={(e) => updateField("reader_auto_load_next", e.target.checked)}
              />
              Automatically load and append next chapter in continuous scroll mode
            </label>
          </div>
        </div>
      )}

      {/* Downloads */}
      {tab === "downloads" && (
        <div className="space-y-5 rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-surface)] p-6">
          <div className="space-y-2">
            <label className="text-xs font-medium text-[var(--text-secondary)]">
              Offline Download Location
            </label>
            <div className="flex flex-col sm:flex-row gap-2">
              <input
                readOnly
                value={settings.download_dir}
                className="flex-1 min-w-0 px-3 py-2 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-elevated)] text-xs text-[var(--text-secondary)]"
              />
              {!IS_IOS && (
                <button
                  type="button"
                  onClick={handlePickDownloadDir}
                  className="inline-flex items-center justify-center gap-1.5 px-3.5 py-2 rounded-lg text-xs font-medium border border-[var(--border-subtle)] bg-[var(--bg-elevated)] hover:border-[var(--accent)] shrink-0"
                >
                  <FolderOpen className="w-3.5 h-3.5" />
                  Change...
                </button>
              )}
            </div>
            {IS_IOS && (
              <p className="text-[11px] text-[var(--text-muted)]">
                On iOS & iPadOS, downloaded novels and imported books are stored persistently inside Novelium&apos;s sandboxed Application Support container.
              </p>
            )}
          </div>

          <div className="space-y-1.5 max-w-sm text-xs">
            <span className="font-medium text-[var(--text-secondary)]">
              Maximum Concurrent Chapter Downloads ({settings.max_concurrent_downloads})
            </span>
            <input
              type="range"
              min={1}
              max={8}
              value={settings.max_concurrent_downloads}
              onChange={(e) => updateField("max_concurrent_downloads", Number(e.target.value))}
              className="w-full"
            />
          </div>

          <label className="flex items-center gap-2.5 text-xs cursor-pointer">
            <input
              type="checkbox"
              checked={settings.auto_download_followed}
              onChange={(e) => updateField("auto_download_followed", e.target.checked)}
            />
            Automatically download newly released chapters for novels marked “Follow Updates”
          </label>
        </div>
      )}

      {/* Sources Manager */}
      {tab === "sources" && (
        <div className="space-y-4">
          <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3 rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-surface)] p-4">
            <div className="flex items-start gap-3">
              <ShieldCheck className="w-5 h-5 text-emerald-400 shrink-0 mt-0.5" />
              <div className="text-xs text-[var(--text-secondary)]">
                <strong className="text-[var(--text-primary)]">Respectful Source Engine:</strong>{" "}
                All sources apply per-domain rate limits and never bypass authentication, CAPTCHAs, or DRM. Add any URL for automatic parsing, or configure an OPDS 1.2 / CSS-selector source.
              </div>
            </div>
            <div className="flex flex-wrap items-center gap-2 shrink-0">
              <button
                type="button"
                onClick={() => {
                  setCustomManifest(emptyManifest("auto"));
                  setShowAddSource(true);
                }}
                className="inline-flex items-center gap-1.5 px-3.5 py-2 rounded-lg text-xs font-medium border border-[var(--accent-border)] bg-[var(--accent-soft)] text-[var(--accent-bright)]"
              >
                <Plus className="w-3.5 h-3.5" />
                Add Source by URL
              </button>
              <button
                type="button"
                onClick={() => {
                  if (!showAddSource) {
                    setCustomManifest(emptyManifest("opds"));
                  }
                  setShowAddSource(!showAddSource);
                }}
                className="inline-flex items-center gap-1.5 px-3.5 py-2 rounded-lg text-xs font-medium text-white shrink-0"
                style={{ backgroundColor: "var(--accent)" }}
              >
                <Plus className="w-3.5 h-3.5" />
                Add Custom Source
              </button>
            </div>
          </div>

          {showAddSource && (
            <form
              onSubmit={handleInstallCustomSource}
              className="rounded-2xl border border-[var(--accent-border)] bg-[var(--bg-surface)] p-5 space-y-4"
            >
              <h3 className="text-sm font-semibold">
                {customManifest.kind === "auto"
                  ? "Add Website or Feed URL (Auto-Parsed)"
                  : "Install Custom Declarative Source (OPDS / HTML)"}
              </h3>
              <div className="grid grid-cols-1 sm:grid-cols-3 gap-3 text-xs">
                <input
                  required={customManifest.kind !== "auto"}
                  value={customManifest.id}
                  onChange={(e) => setCustomManifest({ ...customManifest, id: e.target.value })}
                  placeholder={
                    customManifest.kind === "auto"
                      ? "ID (optional, auto from URL)"
                      : "ID (e.g. my_calibre)"
                  }
                  className="px-3 py-2 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-elevated)]"
                />
                <input
                  required={customManifest.kind !== "auto"}
                  value={customManifest.name}
                  onChange={(e) => setCustomManifest({ ...customManifest, name: e.target.value })}
                  placeholder={
                    customManifest.kind === "auto"
                      ? "Display Name (optional)"
                      : "Display Name"
                  }
                  className="px-3 py-2 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-elevated)]"
                />
                <select
                  value={customManifest.kind}
                  onChange={(e) =>
                    setCustomManifest({
                      ...customManifest,
                      kind: e.target.value as DeclarativeSourceManifest["kind"],
                    })
                  }
                  className="px-3 py-2 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-elevated)]"
                >
                  <option value="auto">Auto-Parse URL (Smart Detector)</option>
                  <option value="opds">OPDS 1.2 Atom Feed</option>
                  <option value="html">HTML CSS-Selector Source</option>
                </select>
                <input
                  required
                  value={customManifest.base_url}
                  onChange={(e) =>
                    setCustomManifest({ ...customManifest, base_url: e.target.value })
                  }
                  placeholder="Base URL (https://...)"
                  className="px-3 py-2 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-elevated)] sm:col-span-2"
                />
                <input
                  value={customManifest.description}
                  onChange={(e) =>
                    setCustomManifest({ ...customManifest, description: e.target.value })
                  }
                  placeholder="Short description (optional)"
                  className="px-3 py-2 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-elevated)]"
                />
              </div>

              {customManifest.kind === "auto" ? (
                <div className="space-y-2">
                  <p className="text-[11px] text-[var(--text-secondary)]">
                    Novelium will automatically detect OPDS/RSS feeds or web novel lists, metadata, chapters, and readable content from the URL.
                  </p>
                  <div className="grid grid-cols-1 sm:grid-cols-3 gap-3 text-xs">
                    <input
                      value={customManifest.browse_url_template || ""}
                      onChange={(e) =>
                        setCustomManifest({
                          ...customManifest,
                          browse_url_template: e.target.value,
                        })
                      }
                      placeholder="Browse URL override ({page}, optional)"
                      className="px-3 py-2 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-elevated)]"
                    />
                    <input
                      value={customManifest.search_url_template || ""}
                      onChange={(e) =>
                        setCustomManifest({
                          ...customManifest,
                          search_url_template: e.target.value,
                        })
                      }
                      placeholder="Search URL override ({query}, optional)"
                      className="px-3 py-2 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-elevated)]"
                    />
                    <input
                      value={customManifest.language || ""}
                      onChange={(e) =>
                        setCustomManifest({
                          ...customManifest,
                          language: e.target.value,
                        })
                      }
                      placeholder="Language code (e.g. en, optional)"
                      className="px-3 py-2 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-elevated)]"
                    />
                  </div>
                </div>
              ) : customManifest.kind === "opds" ? (
                <div className="grid grid-cols-1 sm:grid-cols-2 gap-3 text-xs">
                  <input
                    required
                    value={customManifest.feed_url || ""}
                    onChange={(e) =>
                      setCustomManifest({ ...customManifest, feed_url: e.target.value })
                    }
                    placeholder="OPDS Atom Feed URL (supports {page})"
                    className="px-3 py-2 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-elevated)]"
                  />
                  <input
                    value={customManifest.search_url_template || ""}
                    onChange={(e) =>
                      setCustomManifest({
                        ...customManifest,
                        search_url_template: e.target.value,
                      })
                    }
                    placeholder="Search URL Template (optional, uses {query})"
                    className="px-3 py-2 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-elevated)]"
                  />
                </div>
              ) : (
                <div className="grid grid-cols-1 sm:grid-cols-3 gap-3 text-xs">
                  <input
                    value={customManifest.browse_url_template || ""}
                    onChange={(e) =>
                      setCustomManifest({
                        ...customManifest,
                        browse_url_template: e.target.value,
                      })
                    }
                    placeholder="Browse URL ({page})"
                    className="px-3 py-2 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-elevated)]"
                  />
                  <input
                    value={customManifest.search_url_template || ""}
                    onChange={(e) =>
                      setCustomManifest({
                        ...customManifest,
                        search_url_template: e.target.value,
                      })
                    }
                    placeholder="Search URL ({query})"
                    className="px-3 py-2 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-elevated)]"
                  />
                  <input
                    value={customManifest.novel_list_selector || ""}
                    onChange={(e) =>
                      setCustomManifest({
                        ...customManifest,
                        novel_list_selector: e.target.value,
                      })
                    }
                    placeholder="Novel card CSS selector"
                    className="px-3 py-2 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-elevated)]"
                  />
                  <input
                    value={customManifest.title_selector || ""}
                    onChange={(e) =>
                      setCustomManifest({
                        ...customManifest,
                        title_selector: e.target.value,
                      })
                    }
                    placeholder="Title selector"
                    className="px-3 py-2 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-elevated)]"
                  />
                  <input
                    value={customManifest.link_selector || ""}
                    onChange={(e) =>
                      setCustomManifest({
                        ...customManifest,
                        link_selector: e.target.value,
                      })
                    }
                    placeholder="Novel link selector"
                    className="px-3 py-2 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-elevated)]"
                  />
                  <input
                    value={customManifest.chapter_list_selector || ""}
                    onChange={(e) =>
                      setCustomManifest({
                        ...customManifest,
                        chapter_list_selector: e.target.value,
                      })
                    }
                    placeholder="Chapter list item selector"
                    className="px-3 py-2 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-elevated)]"
                  />
                  <input
                    value={customManifest.content_selector || ""}
                    onChange={(e) =>
                      setCustomManifest({
                        ...customManifest,
                        content_selector: e.target.value,
                      })
                    }
                    placeholder="Chapter content selector"
                    className="px-3 py-2 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-elevated)]"
                  />
                </div>
              )}

              <div className="flex justify-end gap-2">
                <button
                  type="button"
                  onClick={() => setShowAddSource(false)}
                  className="px-3 py-1.5 rounded-lg text-xs border border-[var(--border-subtle)]"
                >
                  Cancel
                </button>
                <button
                  type="submit"
                  className="px-4 py-1.5 rounded-lg text-xs font-medium text-white"
                  style={{ backgroundColor: "var(--accent)" }}
                >
                  Save & Activate Source
                </button>
              </div>
            </form>
          )}

          <div className="space-y-3">
            {sources.map((src) => {
              const health = healthResults[src.id];
              return (
                <div
                  key={src.id}
                  className="rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-surface)] p-5 space-y-3"
                >
                  <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3">
                    <div>
                      <div className="flex items-center gap-2">
                        <h3 className="text-sm font-semibold">{src.name}</h3>
                        <span className="px-2 py-0.5 rounded text-[10px] bg-[var(--bg-elevated)] text-[var(--text-muted)] uppercase">
                          {src.kind}
                        </span>
                        <span
                          className={`inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-[10px] font-medium ${
                            src.health_status === "online"
                              ? "bg-emerald-500/15 text-emerald-300"
                              : src.health_status === "offline" || src.health_status === "degraded"
                                ? "bg-rose-500/15 text-rose-300"
                                : "bg-zinc-500/15 text-zinc-300"
                          }`}
                        >
                          {src.health_status}
                        </span>
                      </div>
                      <p className="text-xs text-[var(--text-secondary)] mt-1">
                        {src.description}
                      </p>
                      <div className="text-[11px] text-[var(--text-muted)] mt-0.5">
                        {src.base_url}
                      </div>
                    </div>

                    <div className="flex items-center gap-2 shrink-0">
                      <button
                        type="button"
                        onClick={() => handleTestSource(src.id)}
                        disabled={testingId === src.id}
                        className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs border border-[var(--border-subtle)] bg-[var(--bg-elevated)] hover:border-[var(--accent)]"
                      >
                        <Wifi className="w-3.5 h-3.5" />
                        {testingId === src.id ? "Testing..." : "Test Connection"}
                      </button>

                      <button
                        type="button"
                        onClick={() => handleToggleSource(src)}
                        className={`px-3 py-1.5 rounded-lg text-xs font-medium ${
                          src.enabled
                            ? "bg-emerald-500/15 text-emerald-300 border border-emerald-500/30"
                            : "bg-[var(--bg-elevated)] text-[var(--text-muted)] border border-[var(--border-subtle)]"
                        }`}
                      >
                        {src.enabled ? "Enabled" : "Disabled"}
                      </button>

                      {src.is_custom && (
                        <button
                          type="button"
                          title="Delete Custom Source"
                          onClick={() => handleRemoveCustomSource(src.id)}
                          className="p-1.5 rounded-lg text-rose-300 hover:bg-rose-500/15"
                        >
                          <Trash2 className="w-4 h-4" />
                        </button>
                      )}
                    </div>
                  </div>

                  {health && (
                    <div
                      className={`text-xs flex items-center gap-2 px-3 py-1.5 rounded-lg ${
                        health.ok
                          ? "bg-emerald-500/10 text-emerald-300"
                          : "bg-rose-500/10 text-rose-300"
                      }`}
                    >
                      {health.ok ? (
                        <CheckCircle2 className="w-3.5 h-3.5" />
                      ) : (
                        <XCircle className="w-3.5 h-3.5" />
                      )}
                      {health.message}
                    </div>
                  )}

                  {src.config_schema.length > 0 && (
                    <div className="pt-2 border-t border-[var(--border-subtle)] flex flex-wrap gap-4">
                      {src.config_schema.map((field) => (
                        <label key={field.key} className="text-xs flex items-center gap-2">
                          <span className="text-[var(--text-secondary)]">{field.label}:</span>
                          {field.field_type === "select" && field.options ? (
                            <select
                              value={String(src.config[field.key] ?? field.default_value)}
                              onChange={(e) =>
                                handleUpdateSourceConfig(src, field.key, e.target.value)
                              }
                              className="px-2 py-1 rounded border border-[var(--border-subtle)] bg-[var(--bg-elevated)]"
                            >
                              {field.options.map((opt) => (
                                <option key={opt} value={opt}>
                                  {opt}
                                </option>
                              ))}
                            </select>
                          ) : (
                            <input
                              value={String(src.config[field.key] ?? field.default_value)}
                              onChange={(e) =>
                                handleUpdateSourceConfig(src, field.key, e.target.value)
                              }
                              className="px-2 py-1 rounded border border-[var(--border-subtle)] bg-[var(--bg-elevated)]"
                            />
                          )}
                        </label>
                      ))}
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        </div>
      )}

      {/* Cloud Sync (Google Drive OAuth) */}
      {tab === "sync" && (
        <GoogleDriveSyncSection
          onSettingsSynced={onUpdateSettings}
          onNotify={onNotify}
        />
      )}

      {/* General, Backups, Shortcuts & Diagnostics */}
      {tab === "general" && (
        <div className="space-y-6">
          <div className="rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-surface)] p-6 space-y-4">
            <h3 className="text-sm font-semibold">Startup Behavior</h3>
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-4 text-xs">
              <label className="space-y-1.5">
                <span className="text-[var(--text-secondary)]">Landing Page on Launch</span>
                <select
                  value={settings.startup_page}
                  onChange={(e) => updateField("startup_page", e.target.value)}
                  className="w-full px-3 py-2 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-elevated)]"
                >
                  <option value="home">Home Dashboard</option>
                  <option value="library">Personal Library</option>
                  <option value="discover">Discover</option>
                </select>
              </label>

              <label className="flex items-center gap-2.5 pt-5 cursor-pointer">
                <input
                  type="checkbox"
                  checked={settings.check_updates_on_launch}
                  onChange={(e) => updateField("check_updates_on_launch", e.target.checked)}
                />
                Automatically check followed novels on launch
              </label>
            </div>
          </div>

          <div className="rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-surface)] p-6 space-y-4">
            <h3 className="text-sm font-semibold flex items-center gap-2">
              <Database className="w-4 h-4 text-[var(--accent-hover)]" />
              Database Backup & Library Portability
            </h3>
            <div className="flex flex-wrap gap-3">
              <button
                type="button"
                onClick={handleBackupDb}
                className="inline-flex items-center gap-1.5 px-3.5 py-2 rounded-lg text-xs font-medium border border-[var(--border-subtle)] bg-[var(--bg-elevated)] hover:border-[var(--accent)]"
              >
                <Database className="w-3.5 h-3.5" />
                Backup SQLite Database (.db)
              </button>
              <button
                type="button"
                onClick={handleExportJson}
                className="inline-flex items-center gap-1.5 px-3.5 py-2 rounded-lg text-xs font-medium border border-[var(--border-subtle)] bg-[var(--bg-elevated)] hover:border-[var(--accent)]"
              >
                <Download className="w-3.5 h-3.5" />
                Export Library & Progress (.json)
              </button>
              <button
                type="button"
                onClick={handleImportJson}
                className="inline-flex items-center gap-1.5 px-3.5 py-2 rounded-lg text-xs font-medium border border-[var(--border-subtle)] bg-[var(--bg-elevated)] hover:border-[var(--accent)]"
              >
                <Upload className="w-3.5 h-3.5" />
                Import / Restore Backup (.json)
              </button>
              <button
                type="button"
                onClick={() => setTab("sync")}
                className="inline-flex items-center gap-1.5 px-3.5 py-2 rounded-lg text-xs font-medium border border-[var(--accent-border)] bg-[var(--accent-soft)] hover:border-[var(--accent)] text-[var(--accent-hover)]"
              >
                <Cloud className="w-3.5 h-3.5" />
                Google Drive Sync Settings…
              </button>
            </div>
          </div>

          <div className="rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-surface)] p-6 space-y-3">
            <h3 className="text-sm font-semibold flex items-center gap-2">
              <Keyboard className="w-4 h-4 text-[var(--accent-hover)]" />
              Keyboard Shortcuts
            </h3>
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-2 text-xs">
              {[
                [`${MOD_KEY}K`, "Global Search Palette"],
                ["← / → or J / K", "Previous / Next Page (Book Flip Mode)"],
                ["[ / ]", "Previous / Next Chapter"],
                ["T", "Toggle Table of Contents in Reader"],
                ["A", "Toggle Typography & Theme Panel"],
                ["B", "Bookmark Current Passage"],
                ["H", "Toggle Zen Distraction-Free Mode"],
                ["F", "Toggle Fullscreen in Reader"],
              ].map(([key, desc]) => (
                <div
                  key={key}
                  className="flex items-center justify-between px-3 py-2 rounded-lg bg-[var(--bg-elevated)]"
                >
                  <span className="text-[var(--text-secondary)]">{desc}</span>
                  <kbd className="px-2 py-0.5 rounded border border-[var(--border-strong)] font-mono text-[11px]">
                    {key}
                  </kbd>
                </div>
              ))}
            </div>
          </div>

          {diagnostics && (
            <div className="rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-surface)] p-6 space-y-3">
              <h3 className="text-sm font-semibold flex items-center gap-2">
                <Activity className="w-4 h-4 text-[var(--accent-hover)]" />
                Diagnostics & Logs
              </h3>
              <div className="text-xs text-[var(--text-secondary)] space-y-1">
                <div>Database: <code>{diagnostics.db_path}</code></div>
                <div>Custom Sources Dir: <code>{diagnostics.custom_sources_dir}</code></div>
              </div>
              <div className="max-h-40 overflow-y-auto rounded-lg bg-black/30 p-3 font-mono text-[11px] text-zinc-300 space-y-1">
                {diagnostics.logs.length === 0 ? (
                  <div>No events logged yet.</div>
                ) : (
                  diagnostics.logs.map((line, i) => <div key={i}>{line}</div>)
                )}
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
};
