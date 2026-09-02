use crate::ai::AiAnnotation;
use crate::metadata::MediaMetadata;
use crate::scanner::{DiscoveredFile, ScanReport, ScanWarning};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::fmt::{Display, Formatter};
use std::path::Path;

const AI_RESULT_MERGE_WINDOW_MS: u64 = 3_000;
const AI_FOCUSED_SCORE_WINDOW: f32 = 0.08;
const AI_BALANCED_SCORE_WINDOW: f32 = 0.14;
const AI_FOCUSED_SCORE_FLOOR: f32 = 0.46;
const AI_BALANCED_SCORE_FLOOR: f32 = 0.36;

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

const MIGRATION_2: &str = r#"
CREATE TABLE ai_annotations (
    content_hash TEXT NOT NULL REFERENCES media_assets(content_hash),
    timestamp_ms INTEGER NOT NULL,
    description TEXT NOT NULL,
    labels_json TEXT NOT NULL,
    embedding_json TEXT NOT NULL,
    confidence REAL,
    model TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (content_hash, timestamp_ms, model)
);

CREATE INDEX ai_annotations_content_hash_idx ON ai_annotations(content_hash);
"#;

const MIGRATION_3: &str = r#"
CREATE VIRTUAL TABLE IF NOT EXISTS search_fts USING fts5(
    path,
    searchable_text
);

INSERT OR IGNORE INTO search_fts(path, searchable_text)
SELECT local_files.path, local_files.path || ' ' || COALESCE(media_assets.metadata_json, '')
FROM local_files
JOIN media_assets ON media_assets.content_hash = local_files.content_hash
WHERE local_files.status = 'ACTIVE';
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

#[derive(Clone, Debug, Default, Deserialize)]
pub struct SearchQuery {
    pub keyword: Option<String>,
    pub folder: Option<String>,
    pub date_from_unix_ms: Option<u64>,
    pub date_to_unix_ms: Option<u64>,
    pub resolution: Option<String>,
    pub frame_rate: Option<String>,
    pub min_duration_ms: Option<u64>,
    pub max_duration_ms: Option<u64>,
    pub codec: Option<String>,
    #[serde(default)]
    pub sort_by: SearchSortField,
    #[serde(default)]
    pub sort_direction: SearchSortDirection,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchSortField {
    #[default]
    Name,
    Duration,
    Size,
    Modified,
    Resolution,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SearchSortDirection {
    #[default]
    Asc,
    Desc,
}

#[derive(Clone, Debug, Serialize)]
pub struct SearchResult {
    pub path: String,
    pub content_hash: String,
    pub size_bytes: u64,
    pub modified_unix_ms: Option<u64>,
    pub status: LocalFileStatus,
    pub available: bool,
    pub metadata: Option<MediaMetadata>,
    pub ai_annotation_count: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct AiSearchResult {
    pub path: String,
    pub content_hash: String,
    pub timestamp_ms: u64,
    pub score: f32,
    pub description: String,
    pub labels: Vec<String>,
    pub available: bool,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AiSearchFocus {
    #[default]
    Focused,
    Balanced,
    Broad,
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
                let _ = transaction
                    .execute("DELETE FROM search_fts WHERE path = ?1", params![file.path]);
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

    pub fn known_files(&self) -> Result<Vec<IndexedFile>, IndexError> {
        let mut statement = self.connection.prepare(
            "SELECT path, content_hash, size_bytes, modified_unix_ms, status
             FROM local_files
             WHERE status = 'ACTIVE'
             ORDER BY path",
        )?;
        let rows = statement.query_map([], |row| {
            let status: String = row.get(4)?;
            Ok(IndexedFile {
                path: row.get(0)?,
                content_hash: row.get(1)?,
                size_bytes: row.get(2)?,
                modified_unix_ms: row.get(3)?,
                status: parse_status(&status),
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
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

    pub fn replace_ai_annotations(
        &mut self,
        content_hash: &str,
        annotations: &[AiAnnotation],
    ) -> Result<(), IndexError> {
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "DELETE FROM ai_annotations WHERE content_hash = ?1",
            params![content_hash],
        )?;
        for annotation in annotations {
            transaction.execute(
                "INSERT INTO ai_annotations(
                    content_hash, timestamp_ms, description, labels_json,
                    embedding_json, confidence, model
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    content_hash,
                    annotation.timestamp_ms,
                    annotation.description,
                    serde_json::to_string(&annotation.labels)?,
                    serde_json::to_string(&annotation.embedding)?,
                    annotation.confidence,
                    annotation.model,
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn ai_annotation_count(&self) -> Result<u64, IndexError> {
        Ok(self
            .connection
            .query_row("SELECT COUNT(*) FROM ai_annotations", [], |row| row.get(0))?)
    }

    pub fn ai_annotation_count_for_model(&self, model_namespace: &str) -> Result<u64, IndexError> {
        Ok(self.connection.query_row(
            "SELECT COUNT(*) FROM ai_annotations WHERE model = ?1",
            params![model_namespace],
            |row| row.get(0),
        )?)
    }

    pub fn has_ai_annotations_for_content_model(
        &self,
        content_hash: &str,
        model_namespace: &str,
    ) -> Result<bool, IndexError> {
        Ok(self.connection.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM ai_annotations
                WHERE content_hash = ?1 AND model = ?2
            )",
            params![content_hash, model_namespace],
            |row| row.get(0),
        )?)
    }

    pub fn search_ai(
        &self,
        query_text: &str,
        query_embedding: &[f32],
        limit: usize,
        model_namespace: Option<&str>,
    ) -> Result<Vec<AiSearchResult>, IndexError> {
        self.search_ai_with_focus(
            query_text,
            query_embedding,
            limit,
            model_namespace,
            AiSearchFocus::Broad,
        )
    }

    pub fn search_ai_with_focus(
        &self,
        query_text: &str,
        query_embedding: &[f32],
        limit: usize,
        model_namespace: Option<&str>,
        focus: AiSearchFocus,
    ) -> Result<Vec<AiSearchResult>, IndexError> {
        let mut statement = self.connection.prepare(
            "SELECT ai_annotations.timestamp_ms, ai_annotations.description,
                    ai_annotations.labels_json, ai_annotations.embedding_json,
                    local_files.path, local_files.content_hash, local_files.status
             FROM ai_annotations
             JOIN local_files ON local_files.content_hash = ai_annotations.content_hash
             WHERE local_files.status = 'ACTIVE'
               AND (?1 IS NULL OR ai_annotations.model = ?1)",
        )?;
        let rows = statement.query_map(params![model_namespace], |row| {
            let timestamp_ms: u64 = row.get(0)?;
            let description: String = row.get(1)?;
            let labels: Vec<String> =
                serde_json::from_str(&row.get::<_, String>(2)?).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        2,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })?;
            let embedding: Vec<f32> =
                serde_json::from_str(&row.get::<_, String>(3)?).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        3,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })?;
            let path: String = row.get(4)?;
            let content_hash: String = row.get(5)?;
            let status: String = row.get(6)?;
            Ok((
                timestamp_ms,
                description,
                labels,
                embedding,
                path,
                content_hash,
                status,
            ))
        })?;

        let query_norm_sq: f32 = query_embedding.iter().map(|v| v * v).sum();
        let query_norm = query_norm_sq.sqrt();

        let mut ranked = rows
            .filter_map(Result::ok)
            .filter_map(
                |(
                    timestamp_ms,
                    description,
                    labels,
                    stored_embedding,
                    path,
                    content_hash,
                    status,
                )| {
                    let semantic_score =
                        cosine_similarity_fast(query_embedding, query_norm, &stored_embedding)?;
                    let keyword_score = lexical_relevance(query_text, &description, &labels);
                    Some(AiSearchResult {
                        timestamp_ms,
                        score: (semantic_score * 0.70 + keyword_score * 0.30).max(0.0),
                        description,
                        labels,
                        available: status == "ACTIVE" && Path::new(&path).is_file(),
                        path,
                        content_hash,
                    })
                },
            )
            .collect::<Vec<_>>();
        ranked.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| path_key(&left.path).cmp(&path_key(&right.path)))
                .then_with(|| left.timestamp_ms.cmp(&right.timestamp_ms))
        });

        let Some(top_score) = ranked.first().map(|result| result.score) else {
            return Ok(Vec::new());
        };
        let (score_floor, max_videos, max_moments_per_video, result_limit) = match focus {
            AiSearchFocus::Focused => (
                (top_score - AI_FOCUSED_SCORE_WINDOW).max(AI_FOCUSED_SCORE_FLOOR),
                8,
                2,
                limit.min(16),
            ),
            AiSearchFocus::Balanced => (
                (top_score - AI_BALANCED_SCORE_WINDOW).max(AI_BALANCED_SCORE_FLOOR),
                16,
                3,
                limit.min(48),
            ),
            AiSearchFocus::Broad => (0.0, usize::MAX, usize::MAX, limit),
        };

        let mut results: Vec<AiSearchResult> = Vec::with_capacity(result_limit.min(ranked.len()));
        let mut video_counts = HashMap::<String, usize>::new();
        for candidate in ranked {
            if candidate.score < score_floor {
                continue;
            }
            let repeats_existing_moment = results.iter().any(|existing| {
                existing.content_hash == candidate.content_hash
                    && existing.timestamp_ms.abs_diff(candidate.timestamp_ms)
                        <= AI_RESULT_MERGE_WINDOW_MS
            });
            if repeats_existing_moment {
                continue;
            }
            let is_new_video = !video_counts.contains_key(&candidate.content_hash);
            if is_new_video && video_counts.len() == max_videos {
                continue;
            }
            let video_count = video_counts
                .entry(candidate.content_hash.clone())
                .or_default();
            if *video_count == max_moments_per_video {
                continue;
            }
            *video_count += 1;
            results.push(candidate);
            if results.len() == result_limit {
                break;
            }
        }
        Ok(results)
    }

    pub fn search(&self, query: &SearchQuery) -> Result<Vec<SearchResult>, IndexError> {
        let mut statement = self.connection.prepare(
            "SELECT local_files.path, local_files.content_hash, local_files.size_bytes,
                    local_files.modified_unix_ms, local_files.status, media_assets.metadata_json,
                    (SELECT COUNT(*) FROM ai_annotations WHERE ai_annotations.content_hash = local_files.content_hash) AS ai_count
             FROM local_files
             JOIN media_assets ON media_assets.content_hash = local_files.content_hash
             ORDER BY local_files.path",
        )?;
        let mut rows = statement.query([])?;
        let mut results = Vec::new();

        while let Some(row) = rows.next()? {
            let path: String = row.get(0)?;
            let content_hash: String = row.get(1)?;
            let size_bytes: u64 = row.get(2)?;
            let modified_unix_ms: Option<u64> = row.get(3)?;
            let status: LocalFileStatus = parse_status(&row.get::<_, String>(4)?);
            let metadata_json: Option<String> = row.get(5)?;
            let ai_annotation_count: u64 = row.get(6)?;
            let metadata = metadata_json
                .map(|value| serde_json::from_str(&value))
                .transpose()?;
            let result = SearchResult {
                available: status == LocalFileStatus::Active && Path::new(&path).is_file(),
                path,
                content_hash,
                size_bytes,
                modified_unix_ms,
                status,
                metadata,
                ai_annotation_count,
            };

            if matches_query(&result, query) {
                results.push(result);
            }
        }

        results.sort_by(|left, right| compare_results(left, right, query));

        Ok(results)
    }

    fn from_connection(connection: Connection) -> Result<Self, IndexError> {
        connection.execute_batch("PRAGMA foreign_keys = ON;")?;
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY,
                applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );",
        )?;

        for (version, migration) in [
            (1_i64, MIGRATION_1),
            (2_i64, MIGRATION_2),
            (3_i64, MIGRATION_3),
        ] {
            let applied: Option<i64> = connection
                .query_row(
                    "SELECT version FROM schema_migrations WHERE version = ?1",
                    params![version],
                    |row| row.get(0),
                )
                .optional()?;

            if applied.is_none() {
                let transaction = connection.unchecked_transaction()?;
                transaction.execute_batch(migration)?;
                transaction.execute(
                    "INSERT INTO schema_migrations(version) VALUES (?1)",
                    params![version],
                )?;
                transaction.commit()?;
            }
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
    let searchable_text = format!("{} {}", file.path, metadata_search_text(metadata));
    let _ = transaction.execute("DELETE FROM search_fts WHERE path = ?1", params![file.path]);
    let _ = transaction.execute(
        "INSERT INTO search_fts(path, searchable_text) VALUES (?1, ?2)",
        params![file.path, searchable_text],
    );
    Ok(())
}

fn parse_status(status: &str) -> LocalFileStatus {
    match status {
        "MISSING" => LocalFileStatus::Missing,
        _ => LocalFileStatus::Active,
    }
}

fn cosine_similarity_fast(left: &[f32], left_norm: f32, right: &[f32]) -> Option<f32> {
    if left.is_empty() || left.len() != right.len() || left_norm <= 0.0 {
        return None;
    }
    let mut dot = 0.0;
    let mut right_norm_sq = 0.0;
    for (left_value, right_value) in left.iter().zip(right) {
        dot += left_value * right_value;
        right_norm_sq += right_value * right_value;
    }
    let denominator = left_norm * right_norm_sq.sqrt();
    (denominator > 0.0).then_some(dot / denominator)
}

#[allow(dead_code)]
fn cosine_similarity(left: &[f32], right: &[f32]) -> Option<f32> {
    if left.is_empty() || left.len() != right.len() {
        return None;
    }
    let left_norm_sq: f32 = left.iter().map(|value| value * value).sum();
    let left_norm = left_norm_sq.sqrt();
    cosine_similarity_fast(left, left_norm, right)
}

fn lexical_relevance(query: &str, description: &str, labels: &[String]) -> f32 {
    let query_terms = search_terms(query);
    if query_terms.is_empty() {
        return 0.0;
    }
    let searchable = format!(
        "{} {}",
        description.to_ascii_lowercase(),
        labels.join(" ").to_ascii_lowercase()
    );
    let matched = query_terms
        .iter()
        .filter(|term| {
            let stem = search_stem(term);
            searchable.contains(term.as_str()) || (!stem.is_empty() && searchable.contains(&stem))
        })
        .count();
    matched as f32 / query_terms.len() as f32
}

fn search_terms(value: &str) -> Vec<String> {
    let mut terms = Vec::new();
    for term in value
        .to_ascii_lowercase()
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|term| term.len() >= 3)
    {
        if !terms.iter().any(|existing| existing == term) {
            terms.push(term.to_owned());
        }
    }
    terms
}

fn search_stem(term: &str) -> String {
    if term.ends_with("ation") && term.len() > 6 {
        return term[..term.len() - 3].to_owned();
    }
    if term.ends_with("ing") && term.len() > 5 {
        return term[..term.len() - 3].to_owned();
    }
    if term.ends_with("ed") && term.len() > 4 {
        return term[..term.len() - 2].to_owned();
    }
    if term.ends_with('e') && term.len() > 4 {
        return term[..term.len() - 1].to_owned();
    }
    if term.ends_with('s') && term.len() > 4 {
        return term[..term.len() - 1].to_owned();
    }
    term.to_owned()
}

fn matches_query(result: &SearchResult, query: &SearchQuery) -> bool {
    if let Some(keyword) = non_empty_lowercase(query.keyword.as_deref()) {
        let searchable = format!(
            "{} {}",
            result.path.to_ascii_lowercase(),
            metadata_search_text(result.metadata.as_ref()).to_ascii_lowercase()
        );
        if !searchable.contains(&keyword) {
            return false;
        }
    }

    if let Some(folder) = non_empty_lowercase(query.folder.as_deref()) {
        if !path_key(&result.path).contains(&folder) {
            return false;
        }
    }

    if let Some(date_from) = query.date_from_unix_ms {
        if result
            .modified_unix_ms
            .map(|date| date < date_from)
            .unwrap_or(true)
        {
            return false;
        }
    }
    if let Some(date_to) = query.date_to_unix_ms {
        if result
            .modified_unix_ms
            .map(|date| date > date_to)
            .unwrap_or(true)
        {
            return false;
        }
    }

    if let Some(resolution) = non_empty_lowercase(query.resolution.as_deref()) {
        let actual = result.metadata.as_ref().and_then(|metadata| {
            Some(format!("{}x{}", metadata.width?, metadata.height?).to_ascii_lowercase())
        });
        if actual.as_deref() != Some(resolution.as_str()) {
            return false;
        }
    }

    if let Some(frame_rate) = non_empty_lowercase(query.frame_rate.as_deref()) {
        let actual = result
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.frame_rate.as_deref());
        if !actual.is_some_and(|actual| frame_rate_matches(actual, &frame_rate)) {
            return false;
        }
    }

    if let Some(min_duration) = query.min_duration_ms {
        if result
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.duration_ms)
            .map(|duration| duration < min_duration)
            .unwrap_or(true)
        {
            return false;
        }
    }
    if let Some(max_duration) = query.max_duration_ms {
        if result
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.duration_ms)
            .map(|duration| duration > max_duration)
            .unwrap_or(true)
        {
            return false;
        }
    }

    if let Some(codec) = non_empty_lowercase(query.codec.as_deref()) {
        let matches_video = result
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.video_codec.as_deref())
            .is_some_and(|value| value.to_ascii_lowercase().contains(&codec));
        let matches_audio = result
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.audio_codec.as_deref())
            .is_some_and(|value| value.to_ascii_lowercase().contains(&codec));
        if !matches_video && !matches_audio {
            return false;
        }
    }

    true
}

fn compare_results(left: &SearchResult, right: &SearchResult, query: &SearchQuery) -> Ordering {
    let primary = match query.sort_by {
        SearchSortField::Name => path_key(&left.path).cmp(&path_key(&right.path)),
        SearchSortField::Duration => compare_optional(
            left.metadata
                .as_ref()
                .and_then(|metadata| metadata.duration_ms),
            right
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.duration_ms),
        ),
        SearchSortField::Size => left.size_bytes.cmp(&right.size_bytes),
        SearchSortField::Modified => {
            compare_optional(left.modified_unix_ms, right.modified_unix_ms)
        }
        SearchSortField::Resolution => compare_optional(
            left.metadata.as_ref().and_then(resolution_key),
            right.metadata.as_ref().and_then(resolution_key),
        ),
    };

    let primary = if query.sort_direction == SearchSortDirection::Desc {
        primary.reverse()
    } else {
        primary
    };

    if primary == Ordering::Equal {
        path_key(&left.path).cmp(&path_key(&right.path))
    } else {
        primary
    }
}

fn compare_optional<T: Ord>(left: Option<T>, right: Option<T>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left.cmp(&right),
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
    }
}

fn resolution_key(metadata: &MediaMetadata) -> Option<(u64, u32, u32)> {
    let width = metadata.width?;
    let height = metadata.height?;
    Some((u64::from(width) * u64::from(height), width, height))
}

fn metadata_search_text(metadata: Option<&MediaMetadata>) -> String {
    metadata
        .map(|metadata| {
            format!(
                "{} {} {} {} {} {} {} {} {}",
                metadata.container.as_deref().unwrap_or_default(),
                metadata.video_codec.as_deref().unwrap_or_default(),
                metadata.audio_codec.as_deref().unwrap_or_default(),
                metadata
                    .width
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                metadata
                    .height
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                metadata.frame_rate.as_deref().unwrap_or_default(),
                metadata.start_time.as_deref().unwrap_or_default(),
                metadata.creation_time.as_deref().unwrap_or_default(),
                metadata
                    .duration_ms
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
            )
        })
        .unwrap_or_default()
}

fn non_empty_lowercase(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    (!value.is_empty()).then(|| value.to_ascii_lowercase())
}

fn frame_rate_matches(actual: &str, expected: &str) -> bool {
    if actual.eq_ignore_ascii_case(expected) {
        return true;
    }

    let actual = rational_to_f64(actual);
    let expected = expected.parse::<f64>().ok();
    actual
        .zip(expected)
        .is_some_and(|(actual, expected)| (actual - expected).abs() < 0.01)
}

fn rational_to_f64(value: &str) -> Option<f64> {
    let (numerator, denominator) = value.split_once('/')?;
    let numerator = numerator.parse::<f64>().ok()?;
    let denominator = denominator.parse::<f64>().ok()?;
    (denominator != 0.0).then_some(numerator / denominator)
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
            3
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
    fn searches_ai_annotations_by_cosine_similarity() {
        let mut index = SqliteIndex::open_in_memory().expect("index should open");
        index
            .reconcile(
                &report(vec![file("/library/fortnite.mp4", "hash-fortnite")]),
                &HashMap::new(),
            )
            .expect("fixture should be indexed");
        index
            .replace_ai_annotations(
                "hash-fortnite",
                &[AiAnnotation {
                    timestamp_ms: 12_000,
                    description: "A player eliminates an enemy in a Fortnite fight".to_owned(),
                    labels: vec!["fortnite".to_owned(), "kill".to_owned()],
                    embedding: vec![1.0, 0.0],
                    confidence: Some(0.9),
                    model: "fixture".to_owned(),
                }],
            )
            .expect("annotation should persist");

        let results = index
            .search_ai("fortnite kill", &[0.9, 0.1], 10, None)
            .expect("AI search should work");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].timestamp_ms, 12_000);
        assert!(results[0].score > 0.9);
        assert_eq!(
            index
                .search_ai("elimination", &[0.9, 0.1], 10, Some("fixture"))
                .expect("matching model namespace should work")
                .len(),
            1
        );
        assert!(index
            .search_ai("elimination", &[0.9, 0.1], 10, Some("other-model"))
            .expect("different model namespace should be empty")
            .is_empty());
    }

    #[test]
    fn boosts_inflected_event_keywords_in_ai_search() {
        let mut index = SqliteIndex::open_in_memory().expect("index should open");
        index
            .reconcile(
                &report(vec![file("/library/elimination.mp4", "hash-elimination")]),
                &HashMap::new(),
            )
            .expect("fixture should be indexed");
        index
            .replace_ai_annotations(
                "hash-elimination",
                &[AiAnnotation {
                    timestamp_ms: 5_000,
                    description: "The kill feed shows an enemy eliminated".to_owned(),
                    labels: vec!["on-screen text: ELIMINATED".to_owned()],
                    embedding: vec![0.1, 0.9],
                    confidence: Some(0.9),
                    model: "fixture".to_owned(),
                }],
            )
            .expect("annotation should persist");

        let results = index
            .search_ai("elimination", &[0.0, 1.0], 10, Some("fixture"))
            .expect("AI search should work");

        assert_eq!(results.len(), 1);
        assert!(results[0].score > 0.99);
    }

    #[test]
    fn coalesces_adjacent_ai_timestamps_into_one_search_moment() {
        let mut index = SqliteIndex::open_in_memory().expect("index should open");
        index
            .reconcile(
                &report(vec![file("/library/scene.mp4", "hash-scene")]),
                &HashMap::new(),
            )
            .expect("fixture should be indexed");
        let annotation = |timestamp_ms| AiAnnotation {
            timestamp_ms,
            description: "A character opens the same door".to_owned(),
            labels: vec!["character".to_owned(), "opening door".to_owned()],
            embedding: vec![1.0, 0.0],
            confidence: Some(0.9),
            model: "fixture".to_owned(),
        };
        index
            .replace_ai_annotations(
                "hash-scene",
                &[
                    annotation(4_000),
                    annotation(5_000),
                    annotation(6_000),
                    annotation(10_000),
                ],
            )
            .expect("annotations should persist");

        let results = index
            .search_ai("opening door", &[1.0, 0.0], 10, Some("fixture"))
            .expect("AI search should work");

        assert_eq!(
            results
                .iter()
                .map(|result| result.timestamp_ms)
                .collect::<Vec<_>>(),
            vec![4_000, 10_000]
        );
    }

    #[test]
    fn focused_ai_search_filters_weak_matches_and_caps_video_count() {
        let mut index = SqliteIndex::open_in_memory().expect("index should open");
        let files = (0..10)
            .map(|index| {
                file(
                    &format!("/library/clip-{index}.mp4"),
                    &format!("hash-{index}"),
                )
            })
            .collect();
        index
            .reconcile(&report(files), &HashMap::new())
            .expect("fixtures should be indexed");
        for index_number in 0..10 {
            index
                .replace_ai_annotations(
                    &format!("hash-{index_number}"),
                    &[AiAnnotation {
                        timestamp_ms: 1_000,
                        description: "A person performs a visually similar action".to_owned(),
                        labels: Vec::new(),
                        embedding: vec![1.0, 0.0],
                        confidence: Some(0.8),
                        model: "fixture".to_owned(),
                    }],
                )
                .expect("annotations should persist");
        }
        index
            .replace_ai_annotations(
                "hash-9",
                &[AiAnnotation {
                    timestamp_ms: 1_000,
                    description: "An unrelated static title card".to_owned(),
                    labels: Vec::new(),
                    embedding: vec![0.0, 1.0],
                    confidence: Some(0.8),
                    model: "fixture".to_owned(),
                }],
            )
            .expect("weak annotation should persist");

        let results = index
            .search_ai_with_focus(
                "specific action",
                &[1.0, 0.0],
                100,
                Some("fixture"),
                AiSearchFocus::Focused,
            )
            .expect("focused AI search should work");

        assert_eq!(results.len(), 8);
        assert!(results.iter().all(|result| result.content_hash != "hash-9"));
    }

    #[test]
    fn focused_ai_search_keeps_keyword_evidence_and_rejects_generic_similarity() {
        let mut index = SqliteIndex::open_in_memory().expect("index should open");
        index
            .reconcile(
                &report(vec![
                    file("/library/exact.mp4", "hash-exact"),
                    file("/library/noise.mp4", "hash-noise"),
                ]),
                &HashMap::new(),
            )
            .expect("fixtures should be indexed");
        index
            .replace_ai_annotations(
                "hash-exact",
                &[AiAnnotation {
                    timestamp_ms: 2_000,
                    description: "The HUD reads eliminated after a player fires".to_owned(),
                    labels: vec!["on-screen text: eliminated".to_owned()],
                    embedding: vec![0.3, 0.953_939],
                    confidence: Some(0.9),
                    model: "fixture".to_owned(),
                }],
            )
            .expect("exact annotation should persist");
        index
            .replace_ai_annotations(
                "hash-noise",
                &[AiAnnotation {
                    timestamp_ms: 3_000,
                    description: "A generic scene with no matching event".to_owned(),
                    labels: Vec::new(),
                    embedding: vec![0.6, 0.8],
                    confidence: Some(0.8),
                    model: "fixture".to_owned(),
                }],
            )
            .expect("noise annotation should persist");

        let focused = index
            .search_ai_with_focus(
                "eliminated",
                &[1.0, 0.0],
                100,
                Some("fixture"),
                AiSearchFocus::Focused,
            )
            .expect("focused search should work");
        let balanced = index
            .search_ai_with_focus(
                "eliminated",
                &[1.0, 0.0],
                100,
                Some("fixture"),
                AiSearchFocus::Balanced,
            )
            .expect("balanced search should work");

        assert_eq!(focused.len(), 1);
        assert_eq!(focused[0].content_hash, "hash-exact");
        assert_eq!(balanced.len(), 2);
    }

    #[test]
    fn detects_existing_ai_analysis_for_a_content_and_model() {
        let mut index = SqliteIndex::open_in_memory().expect("index should open");
        index
            .reconcile(
                &report(vec![file("/library/ready.mp4", "hash-ready")]),
                &HashMap::new(),
            )
            .expect("fixture should be indexed");
        index
            .replace_ai_annotations(
                "hash-ready",
                &[AiAnnotation {
                    timestamp_ms: 0,
                    description: "Ready".to_owned(),
                    labels: Vec::new(),
                    embedding: vec![1.0],
                    confidence: None,
                    model: "model-a".to_owned(),
                }],
            )
            .expect("annotation should persist");

        assert!(index
            .has_ai_annotations_for_content_model("hash-ready", "model-a")
            .expect("existing analysis should be detected"));
        assert!(!index
            .has_ai_annotations_for_content_model("hash-ready", "model-b")
            .expect("other model should remain absent"));
    }

    #[test]
    fn searches_paths_and_technical_metadata_without_network_access() {
        let mut index = SqliteIndex::open_in_memory().expect("index should open");
        let metadata = MediaMetadata {
            duration_ms: Some(12_345),
            size_bytes: Some(1_048_576),
            container: Some("mp4".to_owned()),
            video_codec: Some("h264".to_owned()),
            audio_codec: Some("aac".to_owned()),
            width: Some(1920),
            height: Some(1080),
            frame_rate: Some("30000/1001".to_owned()),
            start_time: Some("0.000000".to_owned()),
            creation_time: Some("2026-08-21T10:15:00Z".to_owned()),
        };
        let mut metadata_by_path = HashMap::new();
        metadata_by_path.insert("/library/day-one/scene-a.mp4".to_owned(), metadata);
        index
            .reconcile(
                &ScanReport {
                    files: vec![DiscoveredFile {
                        path: "/library/day-one/scene-a.mp4".to_owned(),
                        size_bytes: 9,
                        modified_unix_ms: Some(1_000),
                        content_hash: "hash-a".to_owned(),
                    }],
                    warnings: Vec::new(),
                },
                &metadata_by_path,
            )
            .expect("fixture should be indexed");

        let results = index
            .search(&SearchQuery {
                keyword: Some("h264".to_owned()),
                folder: Some("day-one".to_owned()),
                date_from_unix_ms: Some(900),
                date_to_unix_ms: Some(1_100),
                resolution: Some("1920x1080".to_owned()),
                frame_rate: Some("29.97".to_owned()),
                min_duration_ms: Some(10_000),
                max_duration_ms: Some(20_000),
                codec: Some("aac".to_owned()),
                sort_by: SearchSortField::Name,
                sort_direction: SearchSortDirection::Asc,
            })
            .expect("search should work locally");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "/library/day-one/scene-a.mp4");
        assert!(!results[0].available);
    }

    #[test]
    fn sorts_results_by_duration_and_keeps_path_order_for_ties() {
        let mut index = SqliteIndex::open_in_memory().expect("index should open");
        let files = vec![
            DiscoveredFile {
                path: "/library/long.mp4".to_owned(),
                size_bytes: 20,
                modified_unix_ms: Some(2),
                content_hash: "hash-long".to_owned(),
            },
            DiscoveredFile {
                path: "/library/short.mp4".to_owned(),
                size_bytes: 10,
                modified_unix_ms: Some(1),
                content_hash: "hash-short".to_owned(),
            },
        ];
        let mut metadata_by_path = HashMap::new();
        metadata_by_path.insert(
            "/library/long.mp4".to_owned(),
            MediaMetadata {
                duration_ms: Some(20_000),
                size_bytes: Some(20),
                container: Some("mp4".to_owned()),
                video_codec: None,
                audio_codec: None,
                width: Some(1920),
                height: Some(1080),
                frame_rate: None,
                start_time: None,
                creation_time: None,
            },
        );
        metadata_by_path.insert(
            "/library/short.mp4".to_owned(),
            MediaMetadata {
                duration_ms: Some(5_000),
                size_bytes: Some(10),
                container: Some("mp4".to_owned()),
                video_codec: None,
                audio_codec: None,
                width: Some(1280),
                height: Some(720),
                frame_rate: None,
                start_time: None,
                creation_time: None,
            },
        );
        index
            .reconcile(&report(files), &metadata_by_path)
            .expect("fixtures should be indexed");

        let results = index
            .search(&SearchQuery {
                sort_by: SearchSortField::Duration,
                sort_direction: SearchSortDirection::Asc,
                ..SearchQuery::default()
            })
            .expect("sorted search should work");

        assert_eq!(
            results
                .iter()
                .map(|result| result.path.as_str())
                .collect::<Vec<_>>(),
            vec!["/library/short.mp4", "/library/long.mp4"]
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
