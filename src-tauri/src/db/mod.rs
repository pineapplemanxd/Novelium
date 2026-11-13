pub mod migrations;
pub mod queries;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use rusqlite::Connection;
use crate::error::{AppError, AppResult};

#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
    db_path: PathBuf,
}

impl Database {
    pub fn open<P: AsRef<Path>>(path: P) -> AppResult<Self> {
        let path_buf = path.as_ref().to_path_buf();
        if let Some(parent) = path_buf.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&path_buf)?;
        migrations::run_migrations(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            db_path: path_buf,
        })
    }

    pub fn open_in_memory() -> AppResult<Self> {
        let conn = Connection::open_in_memory()?;
        migrations::run_migrations(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            db_path: PathBuf::from(":memory:"),
        })
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub fn with_conn<F, R>(&self, f: F) -> AppResult<R>
    where
        F: FnOnce(&Connection) -> AppResult<R>,
    {
        let guard = self
            .conn
            .lock()
            .map_err(|_| AppError::Other("Database mutex poisoned".into()))?;
        f(&guard)
    }

    pub fn with_conn_mut<F, R>(&self, f: F) -> AppResult<R>
    where
        F: FnOnce(&mut Connection) -> AppResult<R>,
    {
        let mut guard = self
            .conn
            .lock()
            .map_err(|_| AppError::Other("Database mutex poisoned".into()))?;
        f(&mut guard)
    }

    pub fn backup_to<P: AsRef<Path>>(&self, dest: P) -> AppResult<u64> {
        let dest_path = dest.as_ref();
        if let Some(parent) = dest_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let guard = self
            .conn
            .lock()
            .map_err(|_| AppError::Other("Database mutex poisoned".into()))?;
        let mut dst = Connection::open(dest_path)?;
        let backup = rusqlite::backup::Backup::new(&guard, &mut dst)?;
        backup.run_to_completion(64, std::time::Duration::from_millis(10), None)?;
        let bytes = std::fs::metadata(dest_path)?.len();
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_migrations_create_all_required_tables() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let tables: Vec<String> = db
            .with_conn(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name",
                )?;
                let rows = stmt
                    .query_map([], |row| row.get::<_, String>(0))?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .unwrap();

        for expected in [
            "novels",
            "authors",
            "chapters",
            "sources",
            "reading_progress",
            "bookmarks",
            "highlights",
            "library_entries",
            "collections",
            "collection_items",
            "downloads",
            "settings",
            "reading_history",
            "schema_migrations",
        ] {
            assert!(
                tables.contains(&expected.to_string()),
                "Missing table: {expected}"
            );
        }
    }
}
