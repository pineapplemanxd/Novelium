use rusqlite::Connection;
use crate::error::AppResult;

struct Migration {
    version: i64,
    name: &'static str,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "initial_schema",
        sql: r#"
CREATE TABLE IF NOT EXISTS sources (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    base_url TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    enabled INTEGER NOT NULL DEFAULT 1,
    priority INTEGER NOT NULL DEFAULT 100,
    config_json TEXT NOT NULL DEFAULT '{}',
    health_status TEXT NOT NULL DEFAULT 'unknown',
    last_checked_at TEXT
);

CREATE TABLE IF NOT EXISTS authors (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    bio TEXT,
    url TEXT
);

CREATE TABLE IF NOT EXISTS novels (
    id TEXT PRIMARY KEY,
    source_id TEXT NOT NULL,
    external_id TEXT NOT NULL,
    title TEXT NOT NULL,
    author_id TEXT REFERENCES authors(id) ON DELETE SET NULL,
    author_name TEXT NOT NULL DEFAULT 'Unknown Author',
    description TEXT NOT NULL DEFAULT '',
    cover_url TEXT,
    local_cover_path TEXT,
    status TEXT NOT NULL DEFAULT 'unknown',
    genres_json TEXT NOT NULL DEFAULT '[]',
    tags_json TEXT NOT NULL DEFAULT '[]',
    rating REAL,
    popularity INTEGER NOT NULL DEFAULT 0,
    chapter_count INTEGER NOT NULL DEFAULT 0,
    last_updated_remote TEXT,
    metadata_json TEXT NOT NULL DEFAULT '{}',
    is_local INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(source_id, external_id)
);

CREATE INDEX IF NOT EXISTS idx_novels_source ON novels(source_id, external_id);
CREATE INDEX IF NOT EXISTS idx_novels_title ON novels(title);

CREATE TABLE IF NOT EXISTS chapters (
    id TEXT PRIMARY KEY,
    novel_id TEXT NOT NULL REFERENCES novels(id) ON DELETE CASCADE,
    external_id TEXT NOT NULL,
    chapter_number REAL NOT NULL,
    volume TEXT,
    title TEXT NOT NULL,
    url TEXT,
    word_count INTEGER NOT NULL DEFAULT 0,
    published_at TEXT,
    is_downloaded INTEGER NOT NULL DEFAULT 0,
    local_path TEXT,
    size_bytes INTEGER NOT NULL DEFAULT 0,
    content_hash TEXT,
    created_at TEXT NOT NULL,
    UNIQUE(novel_id, external_id)
);

CREATE INDEX IF NOT EXISTS idx_chapters_novel_num ON chapters(novel_id, chapter_number);
CREATE INDEX IF NOT EXISTS idx_chapters_downloaded ON chapters(novel_id, is_downloaded);

CREATE TABLE IF NOT EXISTS reading_progress (
    novel_id TEXT PRIMARY KEY REFERENCES novels(id) ON DELETE CASCADE,
    chapter_id TEXT NOT NULL REFERENCES chapters(id) ON DELETE CASCADE,
    chapter_number REAL NOT NULL,
    scroll_offset REAL NOT NULL DEFAULT 0.0,
    paragraph_index INTEGER NOT NULL DEFAULT 0,
    page_index INTEGER NOT NULL DEFAULT 0,
    total_pages INTEGER NOT NULL DEFAULT 1,
    progress_percent REAL NOT NULL DEFAULT 0.0,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS bookmarks (
    id TEXT PRIMARY KEY,
    novel_id TEXT NOT NULL REFERENCES novels(id) ON DELETE CASCADE,
    chapter_id TEXT REFERENCES chapters(id) ON DELETE SET NULL,
    paragraph_index INTEGER,
    excerpt TEXT,
    note TEXT,
    is_novel_bookmark INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_bookmarks_novel ON bookmarks(novel_id, created_at DESC);

CREATE TABLE IF NOT EXISTS library_entries (
    novel_id TEXT PRIMARY KEY REFERENCES novels(id) ON DELETE CASCADE,
    status TEXT NOT NULL DEFAULT 'reading',
    is_bookmarked INTEGER NOT NULL DEFAULT 0,
    follow_updates INTEGER NOT NULL DEFAULT 0,
    last_checked_chapters INTEGER NOT NULL DEFAULT 0,
    added_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_library_status ON library_entries(status, updated_at DESC);

CREATE TABLE IF NOT EXISTS collections (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    description TEXT,
    color TEXT,
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS collection_items (
    collection_id TEXT NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
    novel_id TEXT NOT NULL REFERENCES novels(id) ON DELETE CASCADE,
    added_at TEXT NOT NULL,
    PRIMARY KEY (collection_id, novel_id)
);

CREATE TABLE IF NOT EXISTS downloads (
    id TEXT PRIMARY KEY,
    novel_id TEXT NOT NULL REFERENCES novels(id) ON DELETE CASCADE,
    chapter_ids_json TEXT NOT NULL,
    total_chapters INTEGER NOT NULL,
    completed_chapters INTEGER NOT NULL DEFAULT 0,
    failed_chapters INTEGER NOT NULL DEFAULT 0,
    bytes_downloaded INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'queued',
    error_message TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_downloads_status ON downloads(status, created_at DESC);

CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value_json TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS reading_history (
    id TEXT PRIMARY KEY,
    novel_id TEXT NOT NULL REFERENCES novels(id) ON DELETE CASCADE,
    chapter_id TEXT NOT NULL REFERENCES chapters(id) ON DELETE CASCADE,
    duration_seconds INTEGER NOT NULL DEFAULT 0,
    words_read INTEGER NOT NULL DEFAULT 0,
    read_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_history_read_at ON reading_history(read_at DESC);
CREATE INDEX IF NOT EXISTS idx_history_novel ON reading_history(novel_id, read_at DESC);
"#,
    },
    Migration {
        version: 2,
        name: "seed_default_collections",
        sql: r#"
INSERT OR IGNORE INTO collections (id, name, description, color, sort_order, created_at)
VALUES
  ('favorites', 'Favorites', 'All-time favorite stories', '#6366F1', 0, datetime('now')),
  ('weekend-reads', 'Weekend Reads', 'Short and bingeable novels', '#10B981', 1, datetime('now'));
"#,
    },
    Migration {
        version: 3,
        name: "colored_bookmarks_and_highlights",
        sql: r#"
ALTER TABLE bookmarks ADD COLUMN color TEXT NOT NULL DEFAULT '#818CF8';

CREATE TABLE IF NOT EXISTS highlights (
    id TEXT PRIMARY KEY,
    novel_id TEXT NOT NULL REFERENCES novels(id) ON DELETE CASCADE,
    chapter_id TEXT NOT NULL REFERENCES chapters(id) ON DELETE CASCADE,
    paragraph_index INTEGER NOT NULL,
    start_offset INTEGER NOT NULL DEFAULT 0,
    end_offset INTEGER NOT NULL DEFAULT 0,
    text TEXT NOT NULL,
    color TEXT NOT NULL DEFAULT '#FDE047',
    note TEXT,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_highlights_chapter ON highlights(novel_id, chapter_id, paragraph_index);
"#,
    },
];

pub fn run_migrations(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        r#"
        PRAGMA journal_mode = WAL;
        PRAGMA foreign_keys = ON;
        PRAGMA busy_timeout = 5000;
        CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            applied_at TEXT NOT NULL
        );
        "#,
    )?;

    let current_version: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    for m in MIGRATIONS {
        if m.version > current_version {
            let tx = conn.unchecked_transaction()?;
            tx.execute_batch(m.sql)?;
            tx.execute(
                "INSERT INTO schema_migrations (version, name, applied_at) VALUES (?1, ?2, datetime('now'))",
                rusqlite::params![m.version, m.name],
            )?;
            tx.commit()?;
            tracing::info!("Applied SQLite migration v{}: {}", m.version, m.name);
        }
    }

    Ok(())
}
