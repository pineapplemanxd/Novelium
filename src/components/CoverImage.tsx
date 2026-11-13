import React, { useEffect, useState } from "react";
import { api, isTauriEnv, resolveCoverSrc } from "../api/tauri";

const coverDiskCache = new Map<string, string>();

function getCachedLocalSrc(coverUrl: string): string | null {
  const localPath = coverDiskCache.get(coverUrl);
  if (!localPath || !isTauriEnv()) return null;
  return resolveCoverSrc(coverUrl, localPath);
}

interface CoverImageProps {
  title: string;
  author?: string;
  novelId?: string;
  coverUrl?: string | null;
  localCoverPath?: string | null;
  className?: string;
  size?: "sm" | "md" | "lg";
}

const COVER_PALETTES = [
  ["#1A2238", "#9DAAF2"],
  ["#241E2B", "#E0B0D5"],
  ["#172A26", "#7FD8BE"],
  ["#2B2118", "#E6B88A"],
  ["#1F2430", "#93C5FD"],
  ["#2B1C22", "#FDA4AF"],
];

function hashTitle(title: string): number {
  let h = 0;
  for (let i = 0; i < title.length; i++) {
    h = (h * 31 + title.charCodeAt(i)) | 0;
  }
  return Math.abs(h);
}

export const CoverImage: React.FC<CoverImageProps> = ({
  title,
  author,
  novelId,
  coverUrl,
  localCoverPath,
  className = "",
  size = "md",
}) => {
  const [src, setSrc] = useState<string | null>(() => {
    if (!localCoverPath && coverUrl) {
      return getCachedLocalSrc(coverUrl) ?? resolveCoverSrc(coverUrl, localCoverPath);
    }
    return resolveCoverSrc(coverUrl, localCoverPath);
  });
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    setFailed(false);
    const localKnown =
      !localCoverPath && coverUrl ? getCachedLocalSrc(coverUrl) : null;
    setSrc(localKnown ?? resolveCoverSrc(coverUrl, localCoverPath));
  }, [coverUrl, localCoverPath, novelId]);

  const handleImgError = async () => {
    const localCached =
      localCoverPath ?? (coverUrl ? coverDiskCache.get(coverUrl) : undefined);
    // If asset:// failed for a local path, try base64 fallback once
    if (localCached && src && !src.startsWith("data:")) {
      try {
        const dataUrl = await api.readLocalImageDataUrl(localCached);
        setSrc(dataUrl);
        return;
      } catch {
        // fallback to remote or typographic cover
      }
    }
    if (coverUrl && src !== coverUrl) {
      setSrc(coverUrl);
      return;
    }
    setFailed(true);
  };

  const [bg, fg] = COVER_PALETTES[hashTitle(title) % COVER_PALETTES.length];

  if (!src || failed) {
    return (
      <div
        className={`relative flex flex-col justify-between overflow-hidden rounded-lg border border-[var(--border-subtle)] select-none ${className}`}
        style={{ backgroundColor: bg, color: fg }}
      >
        <div
          className="absolute inset-y-0 left-0 w-1.5 opacity-40"
          style={{ backgroundColor: fg }}
        />
        <div className="p-2.5 pl-3.5">
          <div
            className={`font-serif leading-snug font-medium line-clamp-4 ${
              size === "lg"
                ? "text-lg"
                : size === "sm"
                  ? "text-[11px]"
                  : "text-xs"
            }`}
            style={{ fontFamily: "'Literata', Georgia, serif" }}
          >
            {title}
          </div>
        </div>
        {author && size !== "sm" && (
          <div className="px-3.5 pb-2.5 text-[10px] opacity-75 truncate">
            {author}
          </div>
        )}
      </div>
    );
  }

  return (
    <img
      src={src}
      alt={title}
      loading="lazy"
      decoding="async"
      onError={handleImgError}
      className={`object-cover bg-[var(--bg-elevated)] ${className}`}
    />
  );
};
