import React, { useEffect, useMemo, useRef, useState } from "react";
import {
  CheckCircle2,
  Cloud,
  Copy,
  ExternalLink,
  FolderCheck,
  KeyRound,
  Link2Off,
  Loader2,
  RefreshCw,
  ShieldCheck,
} from "lucide-react";
import { api, extractErrorMessage } from "../api/tauri";
import { extractGoogleDriveFolderId, formatRelativeDate } from "../utils/format";
import type {
  AppSettings,
  GoogleDriveOAuthStartResult,
  GoogleDriveSyncReport,
  GoogleDriveSyncStatus,
} from "../types/novel";

interface GoogleDriveSyncSectionProps {
  onSettingsSynced?: (next: AppSettings) => void;
  onNotify: (msg: string, isError?: boolean) => void;
}

export const GoogleDriveSyncSection: React.FC<GoogleDriveSyncSectionProps> = ({
  onSettingsSynced,
  onNotify,
}) => {
  const [status, setStatus] = useState<GoogleDriveSyncStatus | null>(null);
  const [loading, setLoading] = useState(true);

  const [enabled, setEnabled] = useState(false);
  const [folderInput, setFolderInput] = useState("");
  const [clientId, setClientId] = useState("");
  const [clientSecretInput, setClientSecretInput] = useState("");
  const [clientSecretDirty, setClientSecretDirty] = useState(false);
  const [syncDownloadedBooks, setSyncDownloadedBooks] = useState(true);
  const [syncSettings, setSyncSettings] = useState(true);
  const [autoSyncOnLaunch, setAutoSyncOnLaunch] = useState(false);

  const [oauthSession, setOauthSession] = useState<GoogleDriveOAuthStartResult | null>(null);
  const [manualCallbackInput, setManualCallbackInput] = useState("");

  const [saving, setSaving] = useState(false);
  const [connecting, setConnecting] = useState(false);
  const [verifying, setVerifying] = useState(false);
  const [syncing, setSyncing] = useState(false);
  const [lastReport, setLastReport] = useState<GoogleDriveSyncReport | null>(null);
  const onNotifyRef = useRef(onNotify);
  onNotifyRef.current = onNotify;

  const applyStatusToForm = (st: GoogleDriveSyncStatus) => {
    setStatus(st);
    setEnabled(st.enabled);
    setFolderInput(st.folder_input);
    setClientId(st.client_id);
    setClientSecretInput("");
    setClientSecretDirty(false);
    setSyncDownloadedBooks(st.sync_downloaded_books);
    setSyncSettings(st.sync_settings);
    setAutoSyncOnLaunch(st.auto_sync_on_launch);
  };

  useEffect(() => {
    let mounted = true;
    api
      .getGdriveSyncStatus()
      .then((st) => {
        if (mounted) applyStatusToForm(st);
      })
      .catch(() => {})
      .finally(() => {
        if (mounted) setLoading(false);
      });

    let unlistenUpdate: (() => void) | undefined;
    let unlistenErr: (() => void) | undefined;

    Promise.resolve(
      api.onGdriveSyncUpdated((st) => {
        if (!mounted) return;
        applyStatusToForm(st);
        setOauthSession(null);
        setManualCallbackInput("");
        onNotifyRef.current(
          st.connected_email
            ? `Connected Google Drive (${st.connected_email})`
            : "Google Drive sync updated",
        );
      }),
    )
      .then((fn) => {
        unlistenUpdate = fn;
      })
      .catch(() => {});

    Promise.resolve(
      api.onGdriveOAuthError((msg) => {
        if (!mounted) return;
        setOauthSession(null);
        onNotifyRef.current(msg, true);
      }),
    )
      .then((fn) => {
        unlistenErr = fn;
      })
      .catch(() => {});

    return () => {
      mounted = false;
      unlistenUpdate?.();
      unlistenErr?.();
    };
  }, []);

  const parsedFolderId = useMemo(
    () => extractGoogleDriveFolderId(folderInput),
    [folderInput],
  );

  const persistConfig = async (overrideEnabled?: boolean): Promise<GoogleDriveSyncStatus> => {
    const updated = await api.saveGdriveSyncConfig({
      enabled: overrideEnabled ?? enabled,
      client_id: clientId,
      client_secret_update: clientSecretDirty ? clientSecretInput : null,
      folder_input: folderInput,
      sync_downloaded_books: syncDownloadedBooks,
      sync_settings: syncSettings,
      auto_sync_on_launch: autoSyncOnLaunch,
    });
    applyStatusToForm(updated);
    return updated;
  };

  const handleSave = async () => {
    setSaving(true);
    try {
      await persistConfig();
      onNotify("Saved Google Drive sync configuration");
    } catch (e) {
      onNotify(extractErrorMessage(e), true);
    } finally {
      setSaving(false);
    }
  };

  const handleConnect = async () => {
    setConnecting(true);
    try {
      await persistConfig();
      const started = await api.startGdriveOAuth();
      setOauthSession(started);
      onNotify(
        started.browser_opened
          ? "Complete Google authorization in your browser"
          : "Open the Google authorization link below to connect",
      );
    } catch (e) {
      onNotify(extractErrorMessage(e), true);
    } finally {
      setConnecting(false);
    }
  };

  const handleManualComplete = async () => {
    if (!manualCallbackInput.trim()) return;
    setConnecting(true);
    try {
      const updated = await api.completeGdriveOAuthManual(manualCallbackInput);
      applyStatusToForm(updated);
      setOauthSession(null);
      setManualCallbackInput("");
      onNotify(
        updated.connected_email
          ? `Connected Google Drive (${updated.connected_email})`
          : "Connected Google Drive",
      );
    } catch (e) {
      onNotify(extractErrorMessage(e), true);
    } finally {
      setConnecting(false);
    }
  };

  const handleCancelOAuth = async () => {
    try {
      await api.cancelGdriveOAuth();
    } catch {
      // ignore
    }
    setOauthSession(null);
    setManualCallbackInput("");
  };

  const handleDisconnect = async () => {
    try {
      const updated = await api.disconnectGdrive();
      applyStatusToForm(updated);
      setOauthSession(null);
      onNotify("Disconnected Google Drive account");
    } catch (e) {
      onNotify(extractErrorMessage(e), true);
    }
  };

  const handleVerifyFolder = async () => {
    setVerifying(true);
    try {
      await persistConfig();
      const verified = await api.verifyGdriveFolder();
      applyStatusToForm(verified);
      onNotify(
        `Verified Drive folder "${verified.folder_name || verified.folder_id}"`,
      );
    } catch (e) {
      onNotify(extractErrorMessage(e), true);
    } finally {
      setVerifying(false);
    }
  };

  const handleSyncNow = async () => {
    setSyncing(true);
    try {
      await persistConfig(true);
      const report = await api.syncWithGdrive();
      setLastReport(report);
      const fresh = await api.getGdriveSyncStatus();
      applyStatusToForm(fresh);
      if (report.settings_applied && onSettingsSynced) {
        const nextSettings = await api.getSettings();
        onSettingsSynced(nextSettings);
      }
      onNotify(
        `Synced: ${report.novels_merged} books merged, ${report.books_downloaded} downloaded, ${report.books_uploaded} uploaded`,
      );
    } catch (e) {
      onNotify(extractErrorMessage(e), true);
    } finally {
      setSyncing(false);
    }
  };

  const handleClearSecret = async () => {
    setSaving(true);
    try {
      const updated = await api.saveGdriveSyncConfig({
        enabled,
        client_id: clientId,
        client_secret_update: "",
        folder_input: folderInput,
        sync_downloaded_books: syncDownloadedBooks,
        sync_settings: syncSettings,
        auto_sync_on_launch: autoSyncOnLaunch,
      });
      applyStatusToForm(updated);
      onNotify("Cleared stored OAuth client secret");
    } catch (e) {
      onNotify(extractErrorMessage(e), true);
    } finally {
      setSaving(false);
    }
  };

  if (loading) {
    return (
      <div className="rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-surface)] p-6 text-xs text-[var(--text-secondary)] flex items-center gap-2">
        <Loader2 className="w-4 h-4 animate-spin" />
        Loading Google Drive sync settings…
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <div className="rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-surface)] p-6 space-y-5">
        <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3">
          <div>
            <h3 className="text-sm font-semibold flex items-center gap-2">
              <Cloud className="w-4 h-4 text-[var(--accent-hover)]" />
              Google Drive Cross-Device Sync
            </h3>
            <p className="text-xs text-[var(--text-secondary)] mt-1">
              Sync your downloaded books, covers, bookmarks, highlights, shelves, and reading
              progress via a Google Drive folder using OAuth 2.0.
            </p>
          </div>
          <span
            className={`inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full text-[11px] font-medium shrink-0 border ${
              status?.connected
                ? "border-emerald-500/30 bg-emerald-500/10 text-emerald-300"
                : "border-[var(--border-subtle)] bg-[var(--bg-elevated)] text-[var(--text-muted)]"
            }`}
          >
            <CheckCircle2 className="w-3.5 h-3.5" />
            {status?.connected
              ? `Connected${status.connected_email ? `: ${status.connected_email}` : ""}`
              : "Not Connected"}
          </span>
        </div>

        {/* Target Folder */}
        <div className="space-y-1.5">
          <label className="text-xs font-medium text-[var(--text-secondary)] block">
            Google Drive Folder URL or Folder ID
          </label>
          <div className="flex flex-col sm:flex-row gap-2">
            <input
              type="text"
              value={folderInput}
              onChange={(e) => setFolderInput(e.target.value)}
              placeholder="https://drive.google.com/drive/folders/1JoF3DeGpjMRY__lN-nY-OITpT6Dma2Yd"
              aria-label="Google Drive Folder URL or ID"
              className="flex-1 px-3 py-2 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-elevated)] text-xs font-mono"
            />
            {status?.connected && (
              <button
                type="button"
                onClick={handleVerifyFolder}
                disabled={verifying || !parsedFolderId}
                className="inline-flex items-center justify-center gap-1.5 px-3.5 py-2 rounded-lg text-xs font-medium border border-[var(--border-subtle)] bg-[var(--bg-elevated)] hover:border-[var(--accent)] disabled:opacity-50"
              >
                {verifying ? (
                  <Loader2 className="w-3.5 h-3.5 animate-spin" />
                ) : (
                  <FolderCheck className="w-3.5 h-3.5" />
                )}
                Verify Folder
              </button>
            )}
          </div>
          {folderInput.trim() && (
            <div
              className={`text-[11px] ${
                parsedFolderId ? "text-emerald-300" : "text-amber-300"
              }`}
            >
              {parsedFolderId
                ? `Detected Folder ID: ${parsedFolderId}${
                    status?.folder_name && status.folder_id === parsedFolderId
                      ? ` (${status.folder_name})`
                      : ""
                  }`
                : "Paste a valid https://drive.google.com/drive/folders/... link or raw Drive folder ID."}
            </div>
          )}
        </div>

        {/* OAuth Credentials */}
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-4 text-xs">
          <label className="space-y-1.5">
            <span className="text-[var(--text-secondary)] flex items-center gap-1.5">
              <KeyRound className="w-3.5 h-3.5" />
              Google OAuth Client ID
            </span>
            <input
              type="text"
              value={clientId}
              onChange={(e) => setClientId(e.target.value)}
              placeholder="1234567890-xxxx.apps.googleusercontent.com"
              aria-label="Google OAuth Client ID"
              className="w-full px-3 py-2 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-elevated)] font-mono"
            />
          </label>

          <label className="space-y-1.5">
            <span className="text-[var(--text-secondary)] flex items-center justify-between gap-2">
              <span className="inline-flex items-center gap-1.5">
                <ShieldCheck className="w-3.5 h-3.5" />
                Client Secret (if Desktop client requires it)
              </span>
              {status?.has_client_secret && !clientSecretDirty && (
                <button
                  type="button"
                  onClick={handleClearSecret}
                  className="text-[11px] text-rose-400 hover:underline"
                >
                  Clear Saved
                </button>
              )}
            </span>
            <input
              type="password"
              value={clientSecretInput}
              onChange={(e) => {
                setClientSecretInput(e.target.value);
                setClientSecretDirty(true);
              }}
              placeholder={
                status?.has_client_secret
                  ? "•••••••• (saved locally; leave blank to keep)"
                  : "Optional GOCSPX-... secret"
              }
              aria-label="Google OAuth Client Secret"
              className="w-full px-3 py-2 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-elevated)] font-mono"
            />
          </label>
        </div>

        {/* Setup Guide */}
        <details className="rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-elevated)]/60 p-3.5 text-[11px] text-[var(--text-secondary)] space-y-1.5">
          <summary className="cursor-pointer font-medium text-[var(--text-primary)]">
            How to create your Google OAuth Client ID (Desktop App)
          </summary>
          <ol className="list-decimal list-inside space-y-1 pt-1 leading-relaxed">
            <li>
              In Google Cloud Console, enable <strong>Google Drive API</strong>.
            </li>
            <li>
              Under <strong>Google Auth Platform / OAuth consent screen</strong>, add your Gmail
              under <strong>Test users</strong> if your app is in Testing mode.
            </li>
            <li>
              Create Credentials → <strong>OAuth client ID</strong> → Application type{" "}
              <strong>Desktop app</strong> (automatically supports localhost loopback redirects).
            </li>
            <li>
              Paste the Client ID (and Client Secret if shown) above, paste the same Drive folder URL
              on each device, and click <strong>Connect Google Drive</strong>.
            </li>
          </ol>
        </details>

        {/* Sync Preferences */}
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-3 text-xs pt-1">
          <label className="flex items-center gap-2.5 cursor-pointer">
            <input
              type="checkbox"
              checked={enabled}
              onChange={(e) => setEnabled(e.target.checked)}
            />
            Enable Google Drive sync
          </label>
          <label className="flex items-center gap-2.5 cursor-pointer">
            <input
              type="checkbox"
              checked={syncDownloadedBooks}
              onChange={(e) => setSyncDownloadedBooks(e.target.checked)}
            />
            Sync downloaded offline books & covers
          </label>
          <label className="flex items-center gap-2.5 cursor-pointer">
            <input
              type="checkbox"
              checked={syncSettings}
              onChange={(e) => setSyncSettings(e.target.checked)}
            />
            Sync reader & appearance settings across devices
          </label>
          <label className="flex items-center gap-2.5 cursor-pointer">
            <input
              type="checkbox"
              checked={autoSyncOnLaunch}
              onChange={(e) => setAutoSyncOnLaunch(e.target.checked)}
            />
            Automatically sync on app launch
          </label>
        </div>

        {/* Primary Action Buttons */}
        <div className="flex flex-wrap items-center gap-2.5 pt-2">
          <button
            type="button"
            onClick={handleSave}
            disabled={saving}
            className="inline-flex items-center gap-1.5 px-3.5 py-2 rounded-lg text-xs font-medium border border-[var(--border-subtle)] bg-[var(--bg-elevated)] hover:border-[var(--accent)] disabled:opacity-50"
          >
            {saving && <Loader2 className="w-3.5 h-3.5 animate-spin" />}
            Save Sync Config
          </button>

          <button
            type="button"
            onClick={handleConnect}
            disabled={connecting || !clientId.trim()}
            className="inline-flex items-center gap-1.5 px-4 py-2 rounded-lg text-xs font-medium text-white disabled:opacity-50"
            style={{ backgroundColor: "var(--accent)" }}
          >
            {connecting ? (
              <Loader2 className="w-3.5 h-3.5 animate-spin" />
            ) : (
              <Cloud className="w-3.5 h-3.5" />
            )}
            {status?.connected ? "Reconnect Google Drive" : "Connect Google Drive"}
          </button>

          {status?.connected && (
            <>
              <button
                type="button"
                onClick={handleSyncNow}
                disabled={syncing || !parsedFolderId}
                className="inline-flex items-center gap-1.5 px-4 py-2 rounded-lg text-xs font-medium bg-emerald-600 hover:bg-emerald-500 text-white disabled:opacity-50"
              >
                <RefreshCw className={`w-3.5 h-3.5 ${syncing ? "animate-spin" : ""}`} />
                {syncing ? "Syncing…" : "Sync Now"}
              </button>

              <button
                type="button"
                onClick={handleDisconnect}
                className="inline-flex items-center gap-1.5 px-3.5 py-2 rounded-lg text-xs font-medium border border-rose-500/30 bg-rose-500/10 text-rose-300 hover:bg-rose-500/20"
              >
                <Link2Off className="w-3.5 h-3.5" />
                Disconnect
              </button>
            </>
          )}
        </div>

        {/* Active OAuth Prompt & Fallback Box */}
        {oauthSession && (
          <div className="rounded-xl border border-[var(--accent-border)] bg-[var(--accent-soft)] p-4 space-y-3 text-xs">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <div className="font-medium text-[var(--text-primary)]">
                Waiting for Google sign-in confirmation…
              </div>
              <button
                type="button"
                onClick={handleCancelOAuth}
                className="text-[11px] text-[var(--text-secondary)] hover:underline"
              >
                Cancel
              </button>
            </div>
            <div className="flex flex-wrap items-center gap-2">
              <a
                href={oauthSession.auth_url}
                target="_blank"
                rel="noreferrer"
                className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-[var(--bg-surface)] border border-[var(--border-subtle)] hover:border-[var(--accent)]"
              >
                <ExternalLink className="w-3.5 h-3.5" />
                Open Google Sign-In Link
              </a>
              <button
                type="button"
                onClick={() => {
                  navigator.clipboard?.writeText(oauthSession.auth_url);
                  onNotify("Copied Google sign-in URL");
                }}
                className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-[var(--bg-surface)] border border-[var(--border-subtle)] hover:border-[var(--accent)]"
              >
                <Copy className="w-3.5 h-3.5" />
                Copy Link
              </button>
            </div>
            <div className="space-y-1.5 pt-1">
              <div className="text-[11px] text-[var(--text-secondary)]">
                If your device didn’t complete the redirect automatically, paste the{" "}
                <code>http://127.0.0.1:...</code> redirect URL or authorization code:
              </div>
              <div className="flex gap-2">
                <input
                  type="text"
                  value={manualCallbackInput}
                  onChange={(e) => setManualCallbackInput(e.target.value)}
                  placeholder="http://127.0.0.1:.../oauth2callback?code=...&state=..."
                  aria-label="OAuth Redirect URL or Authorization Code"
                  className="flex-1 px-3 py-1.5 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-surface)] text-xs font-mono"
                />
                <button
                  type="button"
                  onClick={handleManualComplete}
                  disabled={!manualCallbackInput.trim() || connecting}
                  className="px-3 py-1.5 rounded-lg text-xs font-medium text-white disabled:opacity-50"
                  style={{ backgroundColor: "var(--accent)" }}
                >
                  Complete Sign-In
                </button>
              </div>
            </div>
          </div>
        )}

        {/* Sync Status / Last Sync Summary */}
        {(status?.last_sync_at || lastReport) && (
          <div className="rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-elevated)] p-3.5 text-xs space-y-1">
            <div className="font-medium flex items-center justify-between gap-2">
              <span>
                Last Synced:{" "}
                {formatRelativeDate(lastReport?.synced_at || status?.last_sync_at || null)}
              </span>
              {(lastReport?.folder_name || status?.folder_name) && (
                <span className="text-[11px] text-[var(--text-muted)]">
                  Folder: {lastReport?.folder_name || status?.folder_name}
                </span>
              )}
            </div>
            <div className="text-[var(--text-secondary)]">
              {lastReport
                ? `Merged ${lastReport.novels_merged} books (${lastReport.library_entries_merged} library entries) · downloaded ${lastReport.books_downloaded} book(s) (${lastReport.chapters_downloaded} chapters) · uploaded ${lastReport.books_uploaded} book(s)`
                : status?.last_sync_summary}
            </div>
          </div>
        )}
      </div>
    </div>
  );
};
