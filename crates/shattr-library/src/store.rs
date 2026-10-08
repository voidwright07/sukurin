use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use rusqlite::{Connection, params};

use crate::hash::PerceptualHash;
use crate::roots::Stamp;
use crate::{Entry, Item};

/// Bump when the hash layout or the meaning of a column changes; rows from
/// an older version are dropped and re-indexed.
const SCHEMA_VERSION: i64 = 1;

/// Durable copy of the index. One connection, owned by `Library` behind a
/// mutex: SQLite is the single source of truth, so one writer is the
/// invariant rather than a convenience.
pub(crate) struct Store {
    conn: Connection,
    thumbs: PathBuf,
}

pub(crate) struct Row<'a> {
    pub path: &'a str,
    pub stamp: Stamp,
    pub width: u32,
    pub height: u32,
    pub hash: &'a PerceptualHash,
    pub checksum: u64,
}

impl Store {
    pub fn open(db_path: &Path, thumbs: &Path) -> Result<Self> {
        if let Some(dir) = db_path.parent() {
            fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        fs::create_dir_all(thumbs).with_context(|| format!("creating {}", thumbs.display()))?;
        let conn = Connection::open(db_path).with_context(|| format!("opening {}", db_path.display()))?;
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        let store = Self { conn, thumbs: thumbs.to_path_buf() };
        if version != SCHEMA_VERSION {
            store.conn.execute_batch("DROP TABLE IF EXISTS entries")?;
            store.remove_all_thumbs();
        }
        store.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS entries (
                id INTEGER PRIMARY KEY,
                path TEXT NOT NULL UNIQUE,
                modified_ns INTEGER NOT NULL,
                bytes INTEGER NOT NULL,
                width INTEGER NOT NULL,
                height INTEGER NOT NULL,
                hash BLOB NOT NULL,
                checksum INTEGER NOT NULL,
                has_thumb INTEGER NOT NULL
            )",
        )?;
        store.conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(store)
    }

    pub fn load(&self) -> Result<Vec<Item>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, path, modified_ns, bytes, width, height, hash, checksum, has_thumb FROM entries")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, u32>(4)?,
                r.get::<_, u32>(5)?,
                r.get::<_, Vec<u8>>(6)?,
                r.get::<_, i64>(7)?,
                r.get::<_, bool>(8)?,
            ))
        })?;
        let mut items = Vec::new();
        for row in rows {
            let (id, path, modified_ns, bytes, width, height, hash, checksum, has_thumb) = row?;
            let Some(hash) = PerceptualHash::from_bytes(&hash) else { continue };
            let entry = Entry {
                id,
                path: PathBuf::from(path),
                modified: from_ns(modified_ns),
                bytes: bytes as u64,
                width,
                height,
                hash,
                thumb: has_thumb.then(|| self.thumb_path(id)),
            };
            items.push(Item { entry, checksum: checksum as u64 });
        }
        Ok(items)
    }

    /// Inserts or replaces the row for `row.path`, writes its thumbnail, and
    /// returns the stored entry. The id survives re-indexing of a path.
    pub fn upsert(&self, row: Row<'_>, thumb_jpeg: Option<&[u8]>) -> Result<Entry> {
        let id: i64 = self.conn.query_row(
            "INSERT INTO entries (path, modified_ns, bytes, width, height, hash, checksum, has_thumb)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0)
             ON CONFLICT(path) DO UPDATE SET modified_ns = excluded.modified_ns, bytes = excluded.bytes,
                width = excluded.width, height = excluded.height, hash = excluded.hash,
                checksum = excluded.checksum
             RETURNING id",
            params![
                row.path,
                to_ns(row.stamp.modified),
                row.stamp.bytes as i64,
                row.width,
                row.height,
                row.hash.as_bytes(),
                row.checksum as i64
            ],
            |r| r.get(0),
        )?;
        let thumb_path = self.thumb_path(id);
        let thumb = thumb_jpeg.filter(|jpeg| fs::write(&thumb_path, jpeg).is_ok()).map(|_| thumb_path);
        self.conn.execute("UPDATE entries SET has_thumb = ?1 WHERE id = ?2", params![thumb.is_some(), id])?;
        Ok(Entry {
            id,
            path: PathBuf::from(row.path),
            modified: row.stamp.modified,
            bytes: row.stamp.bytes,
            width: row.width,
            height: row.height,
            hash: row.hash.clone(),
            thumb,
        })
    }

    pub fn delete(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM entries WHERE id = ?1", [id])?;
        let _ = fs::remove_file(self.thumb_path(id));
        Ok(())
    }

    fn thumb_path(&self, id: i64) -> PathBuf {
        self.thumbs.join(format!("{id}.jpg"))
    }

    /// Only files this store named (`<id>.jpg`) are removed.
    fn remove_all_thumbs(&self) {
        let Ok(read) = fs::read_dir(&self.thumbs) else { return };
        for entry in read.flatten() {
            let path = entry.path();
            let ours = path.extension().is_some_and(|e| e == "jpg")
                && path.file_stem().and_then(|s| s.to_str()).is_some_and(|s| s.parse::<i64>().is_ok());
            if ours {
                let _ = fs::remove_file(path);
            }
        }
    }
}

fn to_ns(t: SystemTime) -> i64 {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_nanos() as i64,
        Err(e) => -(e.duration().as_nanos() as i64),
    }
}

fn from_ns(ns: i64) -> SystemTime {
    if ns >= 0 {
        UNIX_EPOCH + Duration::from_nanos(ns as u64)
    } else {
        UNIX_EPOCH - Duration::from_nanos(ns.unsigned_abs())
    }
}
