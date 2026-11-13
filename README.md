# Novelium

Cross-platform desktop novel reader and personal library built with **Rust**, **Tauri 2**, **SQLite**, **React**, **TypeScript**, and **Tailwind CSS** for **macOS** and **Windows**.

---

## Features

### Discovery & Modular Sources
- Multi-source catalog browsing, title/author search, genre/status/language filtering, sorting, and infinite pagination.
- **Built-in sources**: Project Gutenberg (`gutenberg`), Wikisource (`wikisource`), Internet Archive (`archive_org`), plus HTML presets (`fanmtl`, `wuxiabox`, `nobadnovel`, `wuxia_click`, `hiraeth`, `readnovelfull`).
- **Extensible source system**: Add, enable/disable, test, and configure custom **OPDS** or CSS-selector **HTML** sources directly from Settings without editing core Rust code.
- **Translations & Language Filtering**: Filter discovery catalogs and your personal library by language, switch between available language editions on Novel Details and Reader views, and set your preferred default language in Settings.

### Distraction-Free Reading Studio
- **Continuous Scroll & Book Flip Modes**: Switch between seamless vertical scrolling and book-style page flipping with an optional flip animation toggle.
- **Seamless Chapter Progression**: Auto-loads and prefetches adjacent chapters so scrolling or flipping past a chapter boundary advances smoothly without resetting scroll position.
- **Full Typography & Color Controls**: Configure font family, font size, line height, paragraph spacing, reading width, font color, and page background color directly from the Reader or Settings.
- **Keyboard Navigation**: Global search palette (`Cmd+K` on macOS, `Ctrl+K` on Windows) and fast chapter navigation (`ArrowLeft`/`k`/`[` and `ArrowRight`/`j`/`]`).

### Annotations, Themes & Personal Library
- **White & Dark UI Themes**: Crisp light theme, dark theme, system appearance sync, accent colors, and a quick theme switch in the sidebar.
- **Colored Bookmarks & Highlighters**: Annotate chapters with 5 color presets (`Amber`, `Emerald`, `Sky`, `Rose`, `Violet`), manage inline `<mark>` text highlights in the Reader drawer, and filter annotations by color on the Bookmarks & Highlights view.
- **Personal Library & Tracking**: Organize novels across Reading, Plan to Read, Completed, Bookmarked, Downloaded, Custom Collections, and Reading History.

### Caching, Offline Downloads & Portable Formats
- **Persistent Disk Caching**: Caches novel covers locally to avoid repeated image downloads and stores read chapters cleanly on disk.
- **Background Download Queue**: Download full novels or selected chapters with pause, resume, cancel, retry, and per-novel storage tracking while keeping reading progress across cleanup.
- **Local Import & Backup**: Import **EPUB**, **TXT**, and **HTML** books, export/import library metadata as JSON, and back up the SQLite database.

---

## Downloads & Releases

Pre-built binaries are produced via GitHub Actions (`.github/workflows/release.yml`) on tagged releases (`v*`):

| Platform | Target | Artifacts |
| --- | --- | --- |
| macOS (Apple Silicon & Intel) | `universal-apple-darwin` | `Novelium-macos-universal.app.zip` (`.app`), `.dmg` |
| Windows (64-bit) | `x86_64-pc-windows-msvc` | NSIS Installer (`*-setup.exe`), `.msi` |

---

## Development Setup

### Prerequisites
- **Node.js** 20+
- **Rust** (stable toolchain)
- **macOS**: Xcode Command Line Tools (`aarch64-apple-darwin` and `x86_64-apple-darwin` targets for Universal builds)
- **Windows**: Microsoft C++ Build Tools and WebView2 Runtime (preinstalled on Windows 10/11)

### Run Locally

```bash
npm install
npm run tauri dev
```

### Build Installers

```bash
# macOS (.app / .dmg)
npm run tauri build -- --bundles app,dmg

# Windows (.exe / .msi)
npm run tauri build -- --bundles nsis,msi
```

### Run Tests & Checks

```bash
npx tsc --noEmit
npm test
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```

---

## Architecture Overview

- **Frontend (`src/`)**: React 19, TypeScript, Tailwind CSS v4, communicating with Rust over **51** registered Tauri IPC commands (`src/api/tauri.ts`).
- **Backend (`src-tauri/src/`)**: Async Tokio runtime, Reqwest HTTP client with rate limiting and `robots.txt` awareness, Scraper HTML parsing, source trait abstraction (`NovelSource`), background download worker, and **14** SQLite tables (`sources`, `authors`, `novels`, `chapters`, `reading_progress`, `bookmarks`, `highlights`, `library_entries`, `collections`, `collection_items`, `downloads`, `settings`, `reading_history`, `schema_migrations`).

---

## Legal Notice

Novelium is an open-source client application for browsing and reading publicly accessible online catalogs and user-imported ebook files. Novelium does not host, store on external servers, scrape past authentication walls, or distribute copyrighted content. Users are responsible for complying with local laws, copyright terms, and the terms of service of any external or custom sources they choose to enable. Built-in sources only access public APIs, feeds, or openly readable pages in accordance with `robots.txt` and rate limits, without bypassing paywalls, accounts, or DRM protections.
