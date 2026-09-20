//! Read-only SQLite access (`Start.md` §13.1, §16.4).
//!
//! The investigation docs (codex.md, workbuddy.md) record that agent
//! databases are SQLite in **WAL mode**, actively written while the
//! agent runs. The open strategy therefore has two tiers:
//!
//! 1. `mode=ro` URI open — never writes, reads through the WAL.
//! 2. If SQLite refuses a read-only open because WAL *recovery* would
//!    be needed (no live `-shm`, e.g. the app crashed or we race a
//!    restart), fall back to a **private copy**: `db` + `-wal` + `-shm`
//!    are copied to a temp dir and the copy is opened read-write.
//!    Writing our own copy is safe — the agent's files are never
//!    touched — and it is the only way SQLite can perform recovery
//!    (red line: never open the agent's database read-write).
//!
//! The caller can distinguish the tiers via [`DbOrigin`] so provider
//! inspections can report a degraded read when it happened through a
//! snapshot (the copy is a point-in-time image; §3.5 revalidation at
//! execution time covers the drift).

use std::path::{Path, PathBuf};

use rusqlite::OpenFlags;

/// Error opening a database read-only.
#[derive(Debug, thiserror::Error)]
pub enum SqliteOpenError {
    #[error("database not found: {0}")]
    NotFound(PathBuf),
    /// The read-only open and the private-copy fallback both failed.
    #[error("database at {path} could not be opened read-only: {reason}")]
    Unreadable { path: PathBuf, reason: String },
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
}

/// How the connection came to exist — let inspections distinguish a
/// live read from a snapshot read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DbOrigin {
    /// Opened `mode=ro` directly on the agent's file.
    Live,
    /// Opened on a private temp copy because the direct read-only open
    /// required WAL recovery (or hit a busy lock).
    PrivateCopy,
}

/// A read-only (or snapshot) connection to an agent database.
pub struct ReadOnlyDb {
    conn: rusqlite::Connection,
    origin: DbOrigin,
    /// Set for `PrivateCopy` origins; removed on drop.
    temp_dir: Option<PathBuf>,
}

impl ReadOnlyDb {
    /// Open `path` read-only with the copy fallback (see module docs).
    pub fn open(path: &Path) -> Result<Self, SqliteOpenError> {
        if !path.is_file() {
            return Err(SqliteOpenError::NotFound(path.to_path_buf()));
        }
        match open_readonly(path) {
            Ok(conn) => Ok(Self {
                conn,
                origin: DbOrigin::Live,
                temp_dir: None,
            }),
            Err(first_error) => {
                // WAL-recovery / busy situations are the expected
                // reasons to be here; any other error also gets the
                // fallback since a snapshot copy is strictly safer.
                match open_via_private_copy(path) {
                    Ok((conn, temp_dir)) => Ok(Self {
                        conn,
                        origin: DbOrigin::PrivateCopy,
                        temp_dir: Some(temp_dir),
                    }),
                    Err(copy_error) => Err(SqliteOpenError::Unreadable {
                        path: path.to_path_buf(),
                        reason: format!("direct: {first_error}; copy fallback: {copy_error}"),
                    }),
                }
            }
        }
    }

    /// The connection for provider queries. Callers must not flip it
    /// into a writing state — providers only read (§10 contract).
    pub fn connection(&self) -> &rusqlite::Connection {
        &self.conn
    }

    /// How this connection was opened (live vs snapshot).
    pub fn origin(&self) -> &DbOrigin {
        &self.origin
    }

    /// Journal mode as SQLite reports it (`wal`, `delete`, …) — a §10.1
    /// pre-scan fact the provider inspection surfaces.
    pub fn journal_mode(&self) -> rusqlite::Result<String> {
        self.conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
    }

    /// `PRAGMA quick_check` — integrity gate before any data from this
    /// connection is trusted (§3.5: schema/structure mismatch ⇒ do not
    /// guess).
    pub fn quick_check_ok(&self) -> rusqlite::Result<bool> {
        let result: String = self
            .conn
            .query_row("PRAGMA quick_check", [], |row| row.get(0))?;
        Ok(result.eq_ignore_ascii_case("ok"))
    }
}

impl Drop for ReadOnlyDb {
    fn drop(&mut self) {
        // Close the connection *before* removing the copy dir: drop
        // order of struct fields is declaration order, so do it
        // explicitly here instead.
        if let Some(dir) = self.temp_dir.take() {
            self.conn = match rusqlite::Connection::open_in_memory() {
                Ok(fresh) => fresh,
                // In-memory open cannot realistically fail; if it
                // does, leave the temp dir behind rather than panic
                // in Drop (a stray temp dir is recoverable, a panic
                // in drop is not).
                Err(_) => return,
            };
            std::fs::remove_dir_all(&dir).ok();
        }
    }
}

/// Direct `mode=ro` open. The URI must percent-encode anything outside
/// the unreserved set — provider cwds contain spaces and CJK (see
/// workbuddy.md), and SQLite URI parsing would otherwise stop at the
/// first `?`.
fn open_readonly(path: &Path) -> rusqlite::Result<rusqlite::Connection> {
    let uri = to_sqlite_uri(path);
    rusqlite::Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
}

/// `file:<encoded-path>?mode=ro`. Windows paths get forward slashes
/// (SQLite's URI parser accepts them and rejects backslashes); UNC
/// paths map onto the `file://server/share/...` authority form.
fn to_sqlite_uri(path: &Path) -> String {
    let Some(s) = path.as_os_str().to_str() else {
        // Non-UTF-8 paths cannot become URIs; produce something that
        // will fail the open loudly rather than guess.
        return format!("file:{}", path.display());
    };
    let slashified = s.replace('\\', "/");
    let mut uri = String::from("file:");
    for byte in slashified.bytes() {
        // Unreserved set + '/' (path separator). Everything else —
        // spaces, CJK, '?', '#', '%' — is percent-encoded.
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'-' | b'_' | b'~') {
            uri.push(byte as char);
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    uri.push_str("?mode=ro");
    uri
}

/// Copy `db` + `-wal` + `-shm` into a private temp dir and open the
/// copy read-write so SQLite can perform WAL recovery on *our* file
/// (never the agent's — that is the whole point of the fallback).
/// Returns the connection plus the temp dir (the caller owns cleanup).
fn open_via_private_copy(path: &Path) -> Result<(rusqlite::Connection, PathBuf), String> {
    let dir = std::env::temp_dir().join(format!(
        "agenttidy-ro-sqlite-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).map_err(|e| format!("tempdir: {e}"))?;

    let db_copy = dir.join("db.sqlite");
    std::fs::copy(path, &db_copy).map_err(|e| {
        std::fs::remove_dir_all(&dir).ok();
        format!("copy db: {e}")
    })?;
    // Sidecars are copied only when present; a wal that was already
    // checkpointed away is fine (the copy then reads the checkpointed
    // content). A sidecar copy that fails mid-read leaves a stale — but
    // self-consistent — snapshot, which the caller reports as
    // `PrivateCopy` origin.
    for suffix in ["-wal", "-shm"] {
        let mut candidate = path.as_os_str().to_os_string();
        candidate.push(suffix);
        let dst = dir.join(format!("db.sqlite{suffix}"));
        let _ = std::fs::copy(Path::new(&candidate), dst);
    }

    let conn = rusqlite::Connection::open(&db_copy).map_err(|e| e.to_string())?;
    Ok((conn, dir))
}

/// Standalone helper for provider code that just needs "is this a WAL
/// database" without keeping a connection.
#[must_use]
pub fn has_wal_sidecars(path: &Path) -> bool {
    let mut wal = path.as_os_str().to_os_string();
    wal.push("-wal");
    Path::new(&wal).exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "agenttidy-sqlite-{}-{tag}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("agent.db")
    }

    fn cleanup(path: &Path) {
        let mut p = path.as_os_str().to_os_string();
        p.push("-wal");
        std::fs::remove_file(Path::new(&p)).ok();
        let mut p = path.as_os_str().to_os_string();
        p.push("-shm");
        std::fs::remove_file(Path::new(&p)).ok();
        std::fs::remove_file(path).ok();
        // parent is Option<&Path>; .map(remove_dir_all) → Option<Result>.
        if let Some(parent) = path.parent() {
            std::fs::remove_dir_all(parent).ok();
        }
    }

    #[test]
    fn opens_readonly_and_reports_journal_mode() {
        let path = temp_db("ro");
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute("CREATE TABLE t (x)", []).unwrap();
            conn.execute("INSERT INTO t VALUES (1)", []).unwrap();
        }
        let db = ReadOnlyDb::open(&path).unwrap();
        assert_eq!(db.origin(), &DbOrigin::Live);
        // Default journal mode (delete) or "wal" — the point of the
        // assertion is that the pragma answers.
        let mode = db.journal_mode().unwrap();
        assert!(!mode.is_empty());
        assert!(db.quick_check_ok().unwrap());
        cleanup(&path);
    }

    #[test]
    fn readonly_connection_rejects_writes() {
        let path = temp_db("ro-write");
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute("CREATE TABLE t (x)", []).unwrap();
        }
        let db = ReadOnlyDb::open(&path).unwrap();
        // The read-only guarantee itself is the security property
        // (§13.1 "SQLite 只读访问"): writes must fail.
        assert!(db
            .connection()
            .execute("CREATE TABLE evil (y)", [])
            .is_err());
        cleanup(&path);
    }

    #[test]
    fn wal_database_opens_and_reads_through_the_wal() {
        let path = temp_db("wal");
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.pragma_update(None, "journal_mode", "wal").unwrap();
            conn.execute("CREATE TABLE t (x)", []).unwrap();
            conn.execute("INSERT INTO t VALUES (42)", []).unwrap();
            // Drop without explicit checkpoint — the goal is to have
            // *live* WAL traffic when the read-only opener arrives.
        }

        let db = ReadOnlyDb::open(&path).unwrap();
        // The journal mode observed by the read-only opener must
        // report WAL (proving the read went through the WAL/rollback
        // machinery, not a plain file read).
        let mode = db.journal_mode().unwrap().to_ascii_lowercase();
        assert_eq!(mode, "wal", "read-only opener should observe WAL");
        // And the row inserted into the WAL must be visible.
        let count: i64 = db
            .connection()
            .query_row("SELECT COUNT(*) FROM t", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
        assert!(db.quick_check_ok().unwrap());
        cleanup(&path);
    }

    #[test]
    fn missing_database_fails_closed() {
        let path = temp_db("missing");
        assert!(matches!(
            ReadOnlyDb::open(&path),
            Err(SqliteOpenError::NotFound(_))
        ));
    }

    #[test]
    fn uri_encoding_handles_spaces_and_unicode() {
        // The encode helper must survive provider-realistic paths
        // (workbuddy.md: spaces and CJK in cwds) — verify round shape.
        let p = if cfg!(windows) {
            PathBuf::from(r"C:\tmp dir\数据 库.db")
        } else {
            PathBuf::from("/tmp dir/数据 库.db")
        };
        let uri = to_sqlite_uri(&p);
        assert!(uri.starts_with("file:"), "{uri}");
        assert!(uri.ends_with("?mode=ro"), "{uri}");
        assert!(!uri.contains(' '), "{uri}");
        assert!(uri.contains("%20"), "{uri}");
    }
}
