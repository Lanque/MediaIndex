use crate::metadata::MediaMetadata;
use crate::scanner::{DiscoveredFile, ScanReport, ScanWarning};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fmt::{Display, Formatter};
use std::path::Path;

const SCHEMA_VERSION: i64 = 1;

const MIGRATION_1: &str = r#"
CREATE TABLE media_assets (
    content_hash TEXT PRIMARY KEY,
    size_bytes INTEGER NOT NULL,
    metadata_json TEXT,
    first_seen_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_seen_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE local_files (
    path TEXT PRIMARY KEY,
    content_hash TEXT NOT NULL REFERENCES media_assets(content_hash),
    size_bytes INTEGER NOT NULL,
    modified_unix_ms INTEGER,
    status TEXT NOT NULL DEFAULT 'ACTIVE' CHECK (status IN ('ACTIVE', 'MISSING')),
    first_seen_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_seen_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX local_files_content_hash_idx ON local_files(content_hash);
CREATE INDEX local_files_status_idx ON local_files(status);
"#;

#[derive(Debug)]
pub enum IndexError {
    Database(rusqlite::Error),
    Json(serde_json::Error),
}

impl Display for IndexError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Database(error) => write!(formatter, "local index database error: {error}"),
            Self::Json(error) => {
                write!(formatter, "cannot encode or decode media metadata: {error}")
            }
        }
    }
}

impl std::error::Error for IndexError {}

impl From<rusqlite::Error> for IndexError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Database(error)
    }
}

impl From<serde_json::Error> for IndexError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum IndexChangeKind {
    New,
    Unchanged,
    Modified,
    Moved,
    Duplicate,
    Deleted,
}

#[derive(Clone, Debug, Serialize)]
pub struct IndexChange {
    pub kind: IndexChangeKind,
    pub path: String,
    pub previous_path: Option<String>,
    pub content_hash: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct IndexReport {
    pub changes: Vec<IndexChange>,
    pub warnings: Vec<ScanWarning>,
    pub active_file_count: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LocalFileStatus {
    Active,
    Missing,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct IndexedFile {
    pub path: String,
    pub content_hash: String,
    pub size_bytes: u64,
    pub modified_unix_ms: Option<u64>,
    pub status: LocalFileStatus,
}

#[derive(Clone, Debug)]
struct ExistingFile {
    path: String,
    content_hash: String,
}

pub struct SqliteIndex {
    connection: Connection,
}

impl SqliteIndex {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, IndexError> {
        let connection = Connection::open(path)?;
        Self::from_connection(connection)
    }

    pub fn open_in_memory() -> Result<Self, IndexError> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    pub fn schema_version(&self) -> Result<i64, IndexError> {
        Ok(self
            .connection
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get::<_, Option<i64>>(0)
            })?
            .unwrap_or(0))
    }

    pub fn reconcile(
        &mut self,
        report: &ScanReport,
        metadata_by_path: &HashMap<String, MediaMetadata>,
    ) -> Result<IndexReport, IndexError> {
        let existing = self.active_files()?;
        let mut previous_by_path: HashMap<String, ExistingFile> = existing
            .iter()
            .map(|file| (file.path.clone(), file.clone()))
            .collect();
        let mut previous_by_hash: HashMap<String, Vec<String>> = HashMap::new();
        for file in existing {
            previous_by_hash
                .entry(file.content_hash)
                .or_default()
                .push(file.path);
        }

        let transaction = self.connection.transaction()?;
        let mut matched_previous = HashSet::new();
        let mut seen_current_hashes = HashSet::new();
        let mut changes = Vec::with_capacity(report.files.len());

        for file in &report.files {
            let mut previous_path = None;
            let kind = if let Some(previous) = previous_by_path.remove(&file.path) {
                matched_previous.insert(previous.path);
                if previous.content_hash == file.content_hash {
                    IndexChangeKind::Unchanged
                } else {
                    IndexChangeKind::Modified
                }
            } else if let Some(moved_from) =
                previous_by_hash.get(&file.content_hash).and_then(|paths| {
                    paths
                        .iter()
                        .find(|path| !matched_previous.contains(*path))
                        .cloned()
                })
            {
                matched_previous.insert(moved_from.clone());
                previous_path = Some(moved_from);
                IndexChangeKind::Moved
            } else if seen_current_hashes.contains(&file.content_hash) {
                IndexChangeKind::Duplicate
            } else {
                IndexChangeKind::New
            };

            upsert_file(&transaction, file, metadata_by_path.get(&file.path))?;
            seen_current_hashes.insert(file.content_hash.clone());
            changes.push(IndexChange {
                kind,
                path: file.path.clone(),
                previous_path,
                content_hash: Some(file.content_hash.clone()),
            });
        }

        if report.warnings.is_empty() {
            let mut deleted: Vec<_> = previous_by_path.into_values().collect();
            deleted.sort_by(|left, right| path_key(&left.path).cmp(&path_key(&right.path)));
            for file in deleted {
                transaction.execute(
                    "UPDATE local_files SET status = 'MISSING', last_seen_at = CURRENT_TIMESTAMP WHERE path = ?1",
                    params![file.path],
                )?;
                changes.push(IndexChange {
                    kind: IndexChangeKind::Deleted,
                    path: file.path,
                    previous_path: None,
                    content_hash: Some(file.content_hash),
                });
            }
        }

        transaction.commit()?;
        Ok(IndexReport {
            changes,
            warnings: report.warnings.clone(),
            active_file_count: self.active_file_count()?,
        })
    }

    pub fn get_file(&self, path: &str) -> Result<Option<IndexedFile>, IndexError> {
        Ok(self
            .connection
            .query_row(
                "SELECT path, content_hash, size_bytes, modified_unix_ms, status FROM local_files WHERE path = ?1",
                params![path],
                |row| {
                    let status: String = row.get(4)?;
                    Ok(IndexedFile {
                        path: row.get(0)?,
                        content_hash: row.get(1)?,
                        size_bytes: row.get(2)?,
                        modified_unix_ms: row.get(3)?,
                        status: parse_status(&status),
                    })
                },
            )
            .optional()?)
    }

    pub fn asset_count(&self) -> Result<u64, IndexError> {
        Ok(self
            .connection
            .query_row("SELECT COUNT(*) FROM media_assets", [], |row| row.get(0))?)
    }

    pub fn local_file_count(&self) -> Result<u64, IndexError> {
        Ok(self
            .connection
            .query_row("SELECT COUNT(*) FROM local_files", [], |row| row.get(0))?)
    }

    pub fn active_file_count(&self) -> Result<u64, IndexError> {
        Ok(self.connection.query_row(
            "SELECT COUNT(*) FROM local_files WHERE status = 'ACTIVE'",
            [],
            |row| row.get(0),
        )?)
    }

    pub fn get_asset_metadata(
        &self,
        content_hash: &str,
    ) -> Result<Option<MediaMetadata>, IndexError> {
        let json: Option<String> = self
            .connection
            .query_row(
                "SELECT metadata_json FROM media_assets WHERE content_hash = ?1",
                params![content_hash],
                |row| row.get(0),
            )
            .optional()?;
        json.map(|value| serde_json::from_str(&value))
            .transpose()
            .map_err(Into::into)
    }

    fn from_connection(connection: Connection) -> Result<Self, IndexError> {
        connection.execute_batch("PRAGMA foreign_keys = ON;")?;
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY,
                applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );",
        )?;

        let applied: Option<i64> = connection
            .query_row(
                "SELECT version FROM schema_migrations WHERE version = ?1",
                params![SCHEMA_VERSION],
                |row| row.get(0),
            )
            .optional()?;

        if applied.is_none() {
            let transaction = connection.unchecked_transaction()?;
            transaction.execute_batch(MIGRATION_1)?;
            transaction.execute(
                "INSERT INTO schema_migrations(version) VALUES (?1)",
                params![SCHEMA_VERSION],
            )?;
            transaction.commit()?;
        }

        Ok(Self { connection })
    }

    fn active_files(&self) -> Result<Vec<ExistingFile>, IndexError> {
        let mut statement = self.connection.prepare(
            "SELECT path, content_hash FROM local_files WHERE status = 'ACTIVE' ORDER BY path",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(ExistingFile {
                path: row.get(0)?,
                content_hash: row.get(1)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
}

fn upsert_file(
    transaction: &Transaction<'_>,
    file: &DiscoveredFile,
    metadata: Option<&MediaMetadata>,
) -> Result<(), IndexError> {
    let metadata_json = metadata.map(serde_json::to_string).transpose()?;
    transaction.execute(
        "INSERT INTO media_assets(content_hash, size_bytes, metadata_json)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(content_hash) DO UPDATE SET
             size_bytes = excluded.size_bytes,
             metadata_json = COALESCE(excluded.metadata_json, media_assets.metadata_json),
             last_seen_at = CURRENT_TIMESTAMP",
        params![file.content_hash, file.size_bytes, metadata_json],
    )?;
    transaction.execute(
        "INSERT INTO local_files(path, content_hash, size_bytes, modified_unix_ms, status)
         VALUES (?1, ?2, ?3, ?4, 'ACTIVE')
         ON CONFLICT(path) DO UPDATE SET
             content_hash = excluded.content_hash,
             size_bytes = excluded.size_bytes,
             modified_unix_ms = excluded.modified_unix_ms,
             status = 'ACTIVE',
             last_seen_at = CURRENT_TIMESTAMP",
        params![
            file.path,
            file.content_hash,
            file.size_bytes,
            file.modified_unix_ms
        ],
    )?;
    Ok(())
}

fn parse_status(status: &str) -> LocalFileStatus {
    match status {
        "MISSING" => LocalFileStatus::Missing,
        _ => LocalFileStatus::Active,
    }
}

fn path_key(path: &str) -> String {
    let normalized = path.replace('\\', "/");

    #[cfg(windows)]
    {
        normalized.to_ascii_lowercase()
    }

    #[cfg(not(windows))]
    {
        normalized
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scanner::{DiscoveredFile, ScanReport};

    fn file(path: &str, hash: &str) -> DiscoveredFile {
        DiscoveredFile {
            path: path.to_owned(),
            size_bytes: hash.len() as u64,
            modified_unix_ms: Some(1),
            content_hash: hash.to_owned(),
        }
    }

    fn report(files: Vec<DiscoveredFile>) -> ScanReport {
        ScanReport {
            files,
            warnings: Vec::new(),
        }
    }

    #[test]
    fn creates_a_versioned_schema_and_is_idempotent() {
        let mut index = SqliteIndex::open_in_memory().expect("index should open");
        assert_eq!(
            index.schema_version().expect("version should be readable"),
            1
        );

        let first = index
            .reconcile(
                &report(vec![file("/library/a.mp4", "hash-a")]),
                &HashMap::new(),
            )
            .expect("first scan should persist");
        let second = index
            .reconcile(
                &report(vec![file("/library/a.mp4", "hash-a")]),
                &HashMap::new(),
            )
            .expect("second scan should persist");

        assert_eq!(first.changes[0].kind, IndexChangeKind::New);
        assert_eq!(second.changes[0].kind, IndexChangeKind::Unchanged);
        assert_eq!(index.asset_count().expect("asset count should work"), 1);
        assert_eq!(index.local_file_count().expect("file count should work"), 1);
        assert_eq!(
            index.active_file_count().expect("active count should work"),
            1
        );
    }

    #[test]
    fn duplicate_copies_share_one_logical_asset() {
        let mut index = SqliteIndex::open_in_memory().expect("index should open");
        index
            .reconcile(
                &report(vec![file("/library/a.mp4", "hash-a")]),
                &HashMap::new(),
            )
            .expect("first scan should persist");

        let result = index
            .reconcile(
                &report(vec![
                    file("/library/a.mp4", "hash-a"),
                    file("/backup/a.mp4", "hash-a"),
                ]),
                &HashMap::new(),
            )
            .expect("duplicate scan should persist");

        assert_eq!(result.changes[1].kind, IndexChangeKind::Duplicate);
        assert_eq!(index.asset_count().expect("asset count should work"), 1);
        assert_eq!(index.local_file_count().expect("file count should work"), 2);
    }

    #[test]
    fn stores_metadata_on_the_content_identity() {
        let mut index = SqliteIndex::open_in_memory().expect("index should open");
        let metadata = MediaMetadata {
            duration_ms: Some(12_345),
            size_bytes: Some(1_048_576),
            container: Some("mp4".to_owned()),
            video_codec: Some("h264".to_owned()),
            audio_codec: None,
            width: Some(1920),
            height: Some(1080),
            frame_rate: Some("30000/1001".to_owned()),
            start_time: Some("0.000000".to_owned()),
            creation_time: None,
        };
        let mut metadata_by_path = HashMap::new();
        metadata_by_path.insert("/library/a.mp4".to_owned(), metadata.clone());

        index
            .reconcile(
                &report(vec![file("/library/a.mp4", "hash-a")]),
                &metadata_by_path,
            )
            .expect("metadata should persist");

        assert_eq!(
            index
                .get_asset_metadata("hash-a")
                .expect("metadata should be readable"),
            Some(metadata)
        );
    }

    #[test]
    fn moved_and_modified_files_keep_content_identity_separate_from_path() {
        let mut index = SqliteIndex::open_in_memory().expect("index should open");
        index
            .reconcile(
                &report(vec![file("/library/a.mp4", "hash-a")]),
                &HashMap::new(),
            )
            .expect("first scan should persist");

        let moved = index
            .reconcile(
                &report(vec![file("/archive/a.mp4", "hash-a")]),
                &HashMap::new(),
            )
            .expect("moved scan should persist");
        assert_eq!(moved.changes[0].kind, IndexChangeKind::Moved);
        assert_eq!(
            index
                .get_file("/library/a.mp4")
                .expect("old path should be readable")
                .expect("old path should remain as history")
                .status,
            LocalFileStatus::Missing
        );

        let modified = index
            .reconcile(
                &report(vec![file("/archive/a.mp4", "hash-new")]),
                &HashMap::new(),
            )
            .expect("modified scan should persist");
        assert_eq!(modified.changes[0].kind, IndexChangeKind::Modified);
        assert_eq!(index.asset_count().expect("asset count should work"), 2);
    }

    #[test]
    fn incomplete_scan_does_not_turn_hash_failures_into_deletions() {
        let mut index = SqliteIndex::open_in_memory().expect("index should open");
        index
            .reconcile(
                &report(vec![file("/library/a.mp4", "hash-a")]),
                &HashMap::new(),
            )
            .expect("first scan should persist");

        let result = index
            .reconcile(
                &ScanReport {
                    files: Vec::new(),
                    warnings: vec![ScanWarning {
                        path: "/library/a.mp4".to_owned(),
                        message: "cannot hash file: interrupted read".to_owned(),
                    }],
                },
                &HashMap::new(),
            )
            .expect("incomplete scan should still return");

        assert_eq!(result.warnings.len(), 1);
        assert_eq!(result.changes.len(), 0);
        assert_eq!(
            index
                .get_file("/library/a.mp4")
                .expect("file should be readable")
                .expect("file should remain indexed")
                .status,
            LocalFileStatus::Active
        );
    }
}
