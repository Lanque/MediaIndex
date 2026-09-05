use crate::ai::AiAnnotation;
use crate::metadata::MediaMetadata;
use crate::scanner::{DiscoveredFile, ScanReport, ScanWarning};
use crate::usage::{AiRunSpec, AiUsageEvent};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::fmt::{Display, Formatter};
use std::path::Path;

const AI_MIN_CONTEXT_MERGE_GAP_MS: u64 = 3_000;
const AI_MAX_CONTEXT_MERGE_GAP_MS: u64 = 60_000;
const AI_CONTEXT_SEMANTIC_SIMILARITY: f32 = 0.90;
const AI_CONTEXT_LABEL_SIMILARITY: f32 = 0.50;
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

const MIGRATION_4: &str = r#"
ALTER TABLE local_files ADD COLUMN identity_verified INTEGER NOT NULL DEFAULT 0
    CHECK (identity_verified IN (0, 1));
"#;

const MIGRATION_5: &str = r#"
CREATE TABLE ai_analysis_runs (
    run_id TEXT PRIMARY KEY,
    operation TEXT NOT NULL,
    provider TEXT NOT NULL,
    vision_model TEXT NOT NULL,
    embedding_model TEXT NOT NULL,
    transcription_model TEXT,
    model_namespace TEXT NOT NULL,
    pricing_status TEXT NOT NULL,
    pricing_checked_at TEXT NOT NULL,
    estimated_cost_usd REAL,
    budget_limit_usd REAL,
    reserved_budget_usd REAL NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'running'
        CHECK (status IN ('running', 'partial', 'completed', 'failed', 'cancelled')),
    completed_file_count INTEGER NOT NULL DEFAULT 0,
    annotation_count INTEGER NOT NULL DEFAULT 0,
    started_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    finished_at TEXT
);

CREATE TABLE ai_usage_events (
    event_id INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id TEXT NOT NULL REFERENCES ai_analysis_runs(run_id),
    operation TEXT NOT NULL,
    model TEXT NOT NULL,
    attempt INTEGER NOT NULL,
    duration_ms INTEGER NOT NULL,
    outcome TEXT NOT NULL,
    status_code INTEGER,
    request_id TEXT,
    usage_status TEXT NOT NULL,
    pricing_status TEXT NOT NULL,
    pricing_checked_at TEXT NOT NULL,
    reported_input_tokens INTEGER,
    reported_output_tokens INTEGER,
    reported_audio_seconds REAL,
    calculated_cost_usd REAL,
    possible_cost INTEGER NOT NULL CHECK (possible_cost IN (0, 1)),
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX ai_analysis_runs_status_idx ON ai_analysis_runs(status);
CREATE INDEX ai_usage_events_run_id_idx ON ai_usage_events(run_id);
CREATE INDEX ai_usage_events_operation_idx ON ai_usage_events(operation);
"#;

const MIGRATION_6: &str = r#"
ALTER TABLE ai_usage_events ADD COLUMN local_event_id TEXT;
CREATE UNIQUE INDEX ai_usage_events_local_event_id_idx
    ON ai_usage_events(local_event_id)
    WHERE local_event_id IS NOT NULL;
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
    pub identity_verified: bool,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct SearchQuery {
    pub keyword: Option<String>,
    pub folder: Option<String>,
    pub root: Option<String>,
    pub ai_only: Option<bool>,
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
    pub end_timestamp_ms: u64,
    pub score: f32,
    pub description: String,
    pub labels: Vec<String>,
    pub available: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct SavedAiMoment {
    pub timestamp_ms: u64,
    pub end_timestamp_ms: u64,
    pub description: String,
    pub labels: Vec<String>,
    pub confidence: Option<f32>,
    pub model: String,
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

#[derive(Clone, Debug)]
struct RankedAiAnnotation {
    result: AiSearchResult,
    embedding: Vec<f32>,
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
        self.reconcile_with_root(report, metadata_by_path, None)
    }

    pub fn reconcile_under_root(
        &mut self,
        report: &ScanReport,
        metadata_by_path: &HashMap<String, MediaMetadata>,
        root: &Path,
    ) -> Result<IndexReport, IndexError> {
        self.reconcile_with_root(report, metadata_by_path, Some(root))
    }

    fn reconcile_with_root(
        &mut self,
        report: &ScanReport,
        metadata_by_path: &HashMap<String, MediaMetadata>,
        root: Option<&Path>,
    ) -> Result<IndexReport, IndexError> {
        let existing = self
            .active_files()?
            .into_iter()
            .filter(|file| root.map_or(true, |root| Path::new(&file.path).starts_with(root)))
            .collect::<Vec<_>>();
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
        let active_file_count = match root {
            Some(root) => self
                .active_files()?
                .into_iter()
                .filter(|file| Path::new(&file.path).starts_with(root))
                .count() as u64,
            None => self.active_file_count()?,
        };
        Ok(IndexReport {
            changes,
            warnings: report.warnings.clone(),
            active_file_count,
        })
    }

    pub fn get_file(&self, path: &str) -> Result<Option<IndexedFile>, IndexError> {
        Ok(self
            .connection
            .query_row(
                "SELECT path, content_hash, size_bytes, modified_unix_ms, status,
                        identity_verified
                 FROM local_files WHERE path = ?1",
                params![path],
                |row| {
                    let status: String = row.get(4)?;
                    Ok(IndexedFile {
                        path: row.get(0)?,
                        content_hash: row.get(1)?,
                        size_bytes: row.get(2)?,
                        modified_unix_ms: row.get(3)?,
                        status: parse_status(&status),
                        identity_verified: row.get::<_, i64>(5)? != 0,
                    })
                },
            )
            .optional()?)
    }

    pub fn known_files(&self) -> Result<Vec<IndexedFile>, IndexError> {
        let mut statement = self.connection.prepare(
            "SELECT path, content_hash, size_bytes, modified_unix_ms, status,
                    identity_verified
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
                identity_verified: row.get::<_, i64>(5)? != 0,
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

    pub fn start_ai_analysis_run(&mut self, run: &AiRunSpec) -> Result<(), IndexError> {
        self.connection.execute(
            "INSERT INTO ai_analysis_runs(
                 run_id, operation, provider, vision_model, embedding_model,
                 transcription_model, model_namespace, pricing_status,
                 pricing_checked_at, estimated_cost_usd, budget_limit_usd
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                run.run_id,
                run.operation,
                run.provider,
                run.vision_model,
                run.embedding_model,
                run.transcription_model,
                run.model_namespace,
                run.pricing_status,
                run.pricing_checked_at,
                run.estimated_cost_usd,
                run.budget_limit_usd,
            ],
        )?;
        Ok(())
    }

    pub fn record_ai_usage_event(&mut self, event: &AiUsageEvent) -> Result<(), IndexError> {
        self.connection.execute(
            "INSERT INTO ai_usage_events(
                 local_event_id, run_id, operation, model, attempt, duration_ms, outcome,
                 status_code, request_id, usage_status, pricing_status,
                 pricing_checked_at, reported_input_tokens, reported_output_tokens,
                 reported_audio_seconds, calculated_cost_usd, possible_cost
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
            params![
                event.local_event_id,
                event.run_id,
                event.operation,
                event.model,
                event.attempt,
                event.duration_ms,
                event.outcome,
                event.status_code,
                event.request_id,
                event.usage_status,
                event.pricing_status,
                event.pricing_checked_at,
                event.reported_input_tokens,
                event.reported_output_tokens,
                event.reported_audio_seconds,
                event.calculated_cost_usd,
                event.possible_cost,
            ],
        )?;
        Ok(())
    }

    pub fn finish_ai_analysis_run(
        &mut self,
        run_id: &str,
        status: &str,
        completed_file_count: u64,
        annotation_count: u64,
        reserved_budget_usd: Option<f64>,
    ) -> Result<(), IndexError> {
        self.connection.execute(
            "UPDATE ai_analysis_runs
             SET status = ?2, completed_file_count = ?3,
                 annotation_count = ?4, reserved_budget_usd = COALESCE(?5, reserved_budget_usd),
                 finished_at = CURRENT_TIMESTAMP
             WHERE run_id = ?1",
            params![
                run_id,
                status,
                completed_file_count,
                annotation_count,
                reserved_budget_usd,
            ],
        )?;
        Ok(())
    }

    pub fn replace_ai_annotations(
        &mut self,
        content_hash: &str,
        annotations: &[AiAnnotation],
    ) -> Result<(), IndexError> {
        let transaction = self.connection.transaction()?;
        let models = annotations
            .iter()
            .map(|annotation| annotation.model.as_str())
            .collect::<HashSet<_>>();
        for model in models {
            transaction.execute(
                "DELETE FROM ai_annotations WHERE content_hash = ?1 AND model = ?2",
                params![content_hash, model],
            )?;
        }
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
            "SELECT COUNT(*)
             FROM ai_annotations
             JOIN local_files ON local_files.content_hash = ai_annotations.content_hash
             WHERE local_files.identity_verified = 1
               AND ai_annotations.model = ?1",
            params![model_namespace],
            |row| row.get(0),
        )?)
    }

    pub fn ai_annotation_count_for_model_under_root(
        &self,
        model_namespace: &str,
        root: Option<&Path>,
    ) -> Result<u64, IndexError> {
        if root.is_none() {
            return self.ai_annotation_count_for_model(model_namespace);
        }
        let mut statement = self.connection.prepare(
            "SELECT local_files.path, local_files.content_hash, ai_annotations.timestamp_ms
             FROM ai_annotations
             JOIN local_files ON local_files.content_hash = ai_annotations.content_hash
             WHERE local_files.identity_verified = 1
               AND ai_annotations.model = ?1",
        )?;
        let rows = statement.query_map(params![model_namespace], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, u64>(2)?,
            ))
        })?;
        let root = root.expect("root was checked above");
        let mut unique_annotations = HashSet::new();
        for row in rows {
            let (path, content_hash, timestamp_ms) = row?;
            if Path::new(&path).starts_with(root) {
                unique_annotations.insert((content_hash, timestamp_ms));
            }
        }
        Ok(unique_annotations.len() as u64)
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
        self.search_ai_with_focus_under_root(
            query_text,
            query_embedding,
            limit,
            model_namespace,
            focus,
            None,
        )
    }

    pub fn search_ai_with_focus_under_root(
        &self,
        query_text: &str,
        query_embedding: &[f32],
        limit: usize,
        model_namespace: Option<&str>,
        focus: AiSearchFocus,
        root: Option<&Path>,
    ) -> Result<Vec<AiSearchResult>, IndexError> {
        let mut statement = self.connection.prepare(
            "SELECT ai_annotations.timestamp_ms, ai_annotations.description,
                    ai_annotations.labels_json, ai_annotations.embedding_json,
                    local_files.path, local_files.content_hash, local_files.status
             FROM ai_annotations
             JOIN local_files ON local_files.content_hash = ai_annotations.content_hash
             WHERE local_files.identity_verified = 1
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

        let mut unique_annotations = HashMap::<(String, u64), RankedAiAnnotation>::new();
        for row in rows.filter_map(Result::ok) {
            let (timestamp_ms, description, labels, stored_embedding, path, content_hash, status) =
                row;
            if root.is_some_and(|root| !Path::new(&path).starts_with(root)) {
                continue;
            }
            let Some(semantic_score) =
                cosine_similarity_fast(query_embedding, query_norm, &stored_embedding)
            else {
                continue;
            };
            let keyword_score = lexical_relevance(query_text, &description, &labels);
            let candidate = RankedAiAnnotation {
                result: AiSearchResult {
                    timestamp_ms,
                    end_timestamp_ms: timestamp_ms,
                    score: (semantic_score * 0.70 + keyword_score * 0.30).max(0.0),
                    description,
                    labels,
                    available: status == "ACTIVE" && Path::new(&path).is_file(),
                    path,
                    content_hash: content_hash.clone(),
                },
                embedding: stored_embedding,
            };
            let key = (content_hash, timestamp_ms);
            match unique_annotations.entry(key) {
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(candidate);
                }
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    let existing = entry.get();
                    if (candidate.result.available && !existing.result.available)
                        || (candidate.result.available == existing.result.available
                            && path_key(&candidate.result.path) < path_key(&existing.result.path))
                    {
                        entry.insert(candidate);
                    }
                }
            }
        }

        let mut annotations_by_video = HashMap::<String, Vec<RankedAiAnnotation>>::new();
        for annotation in unique_annotations.into_values() {
            annotations_by_video
                .entry(annotation.result.content_hash.clone())
                .or_default()
                .push(annotation);
        }
        let mut ranked = annotations_by_video
            .into_values()
            .flat_map(coalesce_contextual_ai_moments)
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

    pub fn saved_ai_moments_for_path(&self, path: &str) -> Result<Vec<SavedAiMoment>, IndexError> {
        let Some(file) = self.get_file(path)? else {
            return Ok(Vec::new());
        };
        if !file.identity_verified {
            return Ok(Vec::new());
        }
        let mut statement = self.connection.prepare(
            "SELECT timestamp_ms, description, labels_json, embedding_json,
                    confidence, model
             FROM ai_annotations
             WHERE content_hash = ?1
             ORDER BY model, timestamp_ms",
        )?;
        let rows = statement.query_map(params![file.content_hash], |row| {
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
            Ok((
                row.get::<_, u64>(0)?,
                row.get::<_, String>(1)?,
                labels,
                embedding,
                row.get::<_, Option<f32>>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?;

        let mut by_model = HashMap::<String, Vec<(RankedAiAnnotation, Option<f32>)>>::new();
        for row in rows {
            let (timestamp_ms, description, labels, embedding, confidence, model) = row?;
            by_model.entry(model).or_default().push((
                RankedAiAnnotation {
                    result: AiSearchResult {
                        path: file.path.clone(),
                        content_hash: file.content_hash.clone(),
                        timestamp_ms,
                        end_timestamp_ms: timestamp_ms,
                        score: confidence.unwrap_or_default(),
                        description,
                        labels,
                        available: file.status == LocalFileStatus::Active
                            && Path::new(&file.path).is_file(),
                    },
                    embedding,
                },
                confidence,
            ));
        }

        let mut moments = Vec::new();
        for (model, annotations) in by_model {
            let confidence_by_timestamp = annotations
                .iter()
                .map(|(annotation, confidence)| (annotation.result.timestamp_ms, *confidence))
                .collect::<HashMap<_, _>>();
            for result in coalesce_contextual_ai_moments(
                annotations
                    .into_iter()
                    .map(|(annotation, _)| annotation)
                    .collect(),
            ) {
                moments.push(SavedAiMoment {
                    timestamp_ms: result.timestamp_ms,
                    end_timestamp_ms: result.end_timestamp_ms,
                    confidence: confidence_by_timestamp
                        .iter()
                        .filter(|(timestamp_ms, _)| {
                            **timestamp_ms >= result.timestamp_ms
                                && **timestamp_ms <= result.end_timestamp_ms
                        })
                        .filter_map(|(_, confidence)| *confidence)
                        .max_by(f32::total_cmp),
                    description: result.description,
                    labels: result.labels,
                    model: model.clone(),
                });
            }
        }
        moments.sort_by(|left, right| {
            left.model
                .cmp(&right.model)
                .then_with(|| left.timestamp_ms.cmp(&right.timestamp_ms))
        });
        Ok(moments)
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
            (4_i64, MIGRATION_4),
            (5_i64, MIGRATION_5),
            (6_i64, MIGRATION_6),
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
        "INSERT INTO local_files(
             path, content_hash, size_bytes, modified_unix_ms, status, identity_verified
         ) VALUES (?1, ?2, ?3, ?4, 'ACTIVE', 1)
         ON CONFLICT(path) DO UPDATE SET
             content_hash = excluded.content_hash,
             size_bytes = excluded.size_bytes,
             modified_unix_ms = excluded.modified_unix_ms,
             status = 'ACTIVE',
             identity_verified = 1,
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

fn coalesce_contextual_ai_moments(mut annotations: Vec<RankedAiAnnotation>) -> Vec<AiSearchResult> {
    annotations.sort_by_key(|annotation| annotation.result.timestamp_ms);
    let mut gaps = annotations
        .windows(2)
        .filter_map(|window| {
            let gap = window[1]
                .result
                .timestamp_ms
                .saturating_sub(window[0].result.timestamp_ms);
            (gap > 0).then_some(gap)
        })
        .collect::<Vec<_>>();
    gaps.sort_unstable();
    let typical_gap = gaps.get(gaps.len() / 2).copied().unwrap_or(0);
    let merge_gap = typical_gap
        .saturating_mul(2)
        .clamp(AI_MIN_CONTEXT_MERGE_GAP_MS, AI_MAX_CONTEXT_MERGE_GAP_MS);

    let mut annotations = annotations.into_iter();
    let Some(mut current) = annotations.next() else {
        return Vec::new();
    };
    let mut previous_embedding = current.embedding.clone();
    let mut previous_labels = current.result.labels.clone();
    let mut moments = Vec::new();

    for candidate in annotations {
        let gap = candidate
            .result
            .timestamp_ms
            .saturating_sub(current.result.end_timestamp_ms);
        if gap <= merge_gap
            && ai_context_is_similar(
                &previous_embedding,
                &candidate.embedding,
                &previous_labels,
                &candidate.result.labels,
            )
        {
            current.result.end_timestamp_ms = candidate.result.timestamp_ms;
            if candidate.result.score > current.result.score {
                current.result.score = candidate.result.score;
                current.result.description = candidate.result.description.clone();
            }
            for label in &candidate.result.labels {
                if !current.result.labels.contains(label) {
                    current.result.labels.push(label.clone());
                }
            }
            current.result.labels.sort();
            previous_embedding = candidate.embedding;
            previous_labels = candidate.result.labels;
        } else {
            moments.push(current.result);
            current = candidate;
            previous_embedding = current.embedding.clone();
            previous_labels = current.result.labels.clone();
        }
    }
    moments.push(current.result);
    moments
}

fn ai_context_is_similar(
    left_embedding: &[f32],
    right_embedding: &[f32],
    left_labels: &[String],
    right_labels: &[String],
) -> bool {
    for prefix in ["action: ", "setting: ", "situation: "] {
        let left = labels_with_prefix(left_labels, prefix);
        let right = labels_with_prefix(right_labels, prefix);
        if !left.is_empty() && !right.is_empty() && left.is_disjoint(&right) {
            return false;
        }
    }

    let semantic_similarity =
        cosine_similarity(left_embedding, right_embedding).unwrap_or_default();
    if semantic_similarity >= AI_CONTEXT_SEMANTIC_SIMILARITY {
        return true;
    }

    let left = left_labels.iter().collect::<HashSet<_>>();
    let right = right_labels.iter().collect::<HashSet<_>>();
    let union = left.union(&right).count();
    if union == 0 {
        return false;
    }
    let similarity = left.intersection(&right).count() as f32 / union as f32;
    similarity >= AI_CONTEXT_LABEL_SIMILARITY
}

fn labels_with_prefix<'a>(labels: &'a [String], prefix: &str) -> HashSet<&'a str> {
    labels
        .iter()
        .filter_map(|label| label.strip_prefix(prefix))
        .collect()
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
    if let Some(root) = query.root.as_deref().filter(|root| !root.trim().is_empty()) {
        if !Path::new(&result.path).starts_with(Path::new(root)) {
            return false;
        }
    }

    if query.ai_only.unwrap_or(false) && result.ai_annotation_count == 0 {
        return false;
    }

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
            6
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
        assert!(
            index
                .get_file("/library/a.mp4")
                .expect("file should load")
                .expect("file should exist")
                .identity_verified
        );
        assert_eq!(
            index.active_file_count().expect("active count should work"),
            1
        );
    }

    #[test]
    fn reconciling_one_folder_keeps_other_folder_entries_active() {
        let mut index = SqliteIndex::open_in_memory().expect("index should open");
        index
            .reconcile_under_root(
                &report(vec![file("/library/alpha/a.mp4", "hash-a")]),
                &HashMap::new(),
                Path::new("/library/alpha"),
            )
            .expect("first folder should persist");
        index
            .reconcile_under_root(
                &report(vec![file("/library/beta/b.mp4", "hash-b")]),
                &HashMap::new(),
                Path::new("/library/beta"),
            )
            .expect("second folder should persist");

        let files = index.known_files().expect("known files should load");
        assert_eq!(files.len(), 2);
        assert!(files
            .iter()
            .all(|file| file.status == LocalFileStatus::Active));
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
                .map(|result| (result.timestamp_ms, result.end_timestamp_ms))
                .collect::<Vec<_>>(),
            vec![(4_000, 6_000), (10_000, 10_000)]
        );
    }

    #[test]
    fn coalesces_a_long_unchanged_scene_into_one_time_range() {
        let mut index = SqliteIndex::open_in_memory().expect("index should open");
        index
            .reconcile(
                &report(vec![file("/library/forest.mp4", "hash-forest")]),
                &HashMap::new(),
            )
            .expect("fixture should be indexed");
        let annotations = (0..=12)
            .map(|step| AiAnnotation {
                timestamp_ms: step * 10_000,
                description: "A man walks through a forest".to_owned(),
                labels: vec!["action: walking".to_owned(), "setting: forest".to_owned()],
                embedding: vec![1.0, 0.0],
                confidence: Some(0.9),
                model: "fixture".to_owned(),
            })
            .collect::<Vec<_>>();
        index
            .replace_ai_annotations("hash-forest", &annotations)
            .expect("annotations should persist");

        let results = index
            .search_ai("walking in forest", &[1.0, 0.0], 20, Some("fixture"))
            .expect("AI search should work");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].timestamp_ms, 0);
        assert_eq!(results[0].end_timestamp_ms, 120_000);
    }

    #[test]
    fn keeps_changing_dialogue_searchable_inside_one_continuous_scene() {
        let mut index = SqliteIndex::open_in_memory().expect("index should open");
        index
            .reconcile(
                &report(vec![file("/library/forest-talk.mp4", "hash-forest-talk")]),
                &HashMap::new(),
            )
            .expect("fixture should be indexed");
        let annotation = |timestamp_ms, dialogue: &str| AiAnnotation {
            timestamp_ms,
            description: format!("A man walks through a forest. Dialogue: {dialogue}"),
            labels: vec![
                "action: walking".to_owned(),
                "setting: forest".to_owned(),
                format!("dialogue: {dialogue}"),
            ],
            embedding: vec![1.0, 0.0],
            confidence: Some(0.9),
            model: "fixture".to_owned(),
        };
        index
            .replace_ai_annotations(
                "hash-forest-talk",
                &[
                    annotation(0, "follow the trail"),
                    annotation(10_000, "watch the river"),
                    annotation(20_000, "we are almost there"),
                ],
            )
            .expect("annotations should persist");

        let results = index
            .search_ai("watch the river", &[1.0, 0.0], 20, Some("fixture"))
            .expect("AI search should work");

        assert_eq!(results.len(), 1);
        assert_eq!(
            (results[0].timestamp_ms, results[0].end_timestamp_ms),
            (0, 20_000)
        );
        assert!(results[0]
            .labels
            .iter()
            .any(|label| label == "dialogue: watch the river"));
    }

    #[test]
    fn exposes_saved_analysis_as_contextual_moments_without_an_ai_query() {
        let mut index = SqliteIndex::open_in_memory().expect("index should open");
        index
            .reconcile(
                &report(vec![file("/library/ceremony.mp4", "hash-ceremony")]),
                &HashMap::new(),
            )
            .expect("fixture should be indexed");
        let annotation = |timestamp_ms, dialogue: &str, model: &str| AiAnnotation {
            timestamp_ms,
            description: format!("Guests listen during a ceremony. Dialogue: {dialogue}"),
            labels: vec![
                "action: listening".to_owned(),
                "setting: outdoor ceremony".to_owned(),
                "situation: wedding".to_owned(),
                format!("dialogue: {dialogue}"),
            ],
            embedding: vec![1.0, 0.0],
            confidence: Some(if timestamp_ms == 10_000 { 0.95 } else { 0.8 }),
            model: model.to_owned(),
        };
        index
            .replace_ai_annotations(
                "hash-ceremony",
                &[
                    annotation(0, "welcome everyone", "openai:model-a:embed-a"),
                    annotation(10_000, "please take your seats", "openai:model-a:embed-a"),
                ],
            )
            .expect("first model annotations should persist");
        index
            .replace_ai_annotations(
                "hash-ceremony",
                &[annotation(0, "welcome everyone", "gemini:model-b:embed-b")],
            )
            .expect("second model should not delete the first model");

        let moments = index
            .saved_ai_moments_for_path("/library/ceremony.mp4")
            .expect("saved moments should load");

        assert_eq!(moments.len(), 2);
        let openai = moments
            .iter()
            .find(|moment| moment.model.starts_with("openai:"))
            .expect("OpenAI analysis should be present");
        assert_eq!((openai.timestamp_ms, openai.end_timestamp_ms), (0, 10_000));
        assert_eq!(openai.confidence, Some(0.95));
        assert!(openai
            .labels
            .contains(&"dialogue: please take your seats".to_owned()));
    }

    #[test]
    fn reanalysis_replaces_only_the_selected_model_history() {
        let mut index = SqliteIndex::open_in_memory().expect("index should open");
        index
            .reconcile(
                &report(vec![file("/library/scene.mp4", "hash-scene")]),
                &HashMap::new(),
            )
            .expect("fixture should be indexed");
        let annotation = |timestamp_ms, description: &str, model: &str| AiAnnotation {
            timestamp_ms,
            description: description.to_owned(),
            labels: vec![format!("description: {description}")],
            embedding: vec![1.0, 0.0],
            confidence: Some(0.9),
            model: model.to_owned(),
        };
        index
            .replace_ai_annotations(
                "hash-scene",
                &[
                    annotation(0, "old first scene", "openai:model-a:embed-a"),
                    annotation(10_000, "old second scene", "openai:model-a:embed-a"),
                ],
            )
            .expect("first model history should persist");
        index
            .replace_ai_annotations(
                "hash-scene",
                &[annotation(
                    5_000,
                    "other provider scene",
                    "gemini:model-b:embed-b",
                )],
            )
            .expect("second model history should persist");
        index
            .replace_ai_annotations(
                "hash-scene",
                &[annotation(20_000, "fresh scene", "openai:model-a:embed-a")],
            )
            .expect("reanalysis should replace the selected model");

        let moments = index
            .saved_ai_moments_for_path("/library/scene.mp4")
            .expect("saved moments should load");

        assert_eq!(moments.len(), 2);
        assert!(moments.iter().any(|moment| {
            moment.model.starts_with("openai:")
                && moment.timestamp_ms == 20_000
                && moment.description == "fresh scene"
        }));
        assert!(moments.iter().any(|moment| {
            moment.model.starts_with("gemini:") && moment.description == "other provider scene"
        }));
        assert!(!moments
            .iter()
            .any(|moment| moment.description.starts_with("old")));
    }

    #[test]
    fn starts_a_new_moment_when_the_action_changes() {
        let mut index = SqliteIndex::open_in_memory().expect("index should open");
        index
            .reconcile(
                &report(vec![file("/library/forest.mp4", "hash-forest")]),
                &HashMap::new(),
            )
            .expect("fixture should be indexed");
        let annotation = |timestamp_ms, action: &str| AiAnnotation {
            timestamp_ms,
            description: format!("A person is {action} in a forest"),
            labels: vec![format!("action: {action}"), "setting: forest".to_owned()],
            embedding: vec![1.0, 0.0],
            confidence: Some(0.9),
            model: "fixture".to_owned(),
        };
        index
            .replace_ai_annotations(
                "hash-forest",
                &[
                    annotation(0, "walking"),
                    annotation(10_000, "walking"),
                    annotation(20_000, "running"),
                    annotation(30_000, "running"),
                ],
            )
            .expect("annotations should persist");

        let results = index
            .search_ai("person in forest", &[1.0, 0.0], 20, Some("fixture"))
            .expect("AI search should work");

        assert_eq!(
            results
                .iter()
                .map(|result| (result.timestamp_ms, result.end_timestamp_ms))
                .collect::<Vec<_>>(),
            vec![(0, 10_000), (20_000, 30_000)]
        );
    }

    #[test]
    fn scopes_ai_search_and_annotation_counts_to_the_selected_root() {
        let mut index = SqliteIndex::open_in_memory().expect("index should open");
        index
            .reconcile(
                &report(vec![
                    file("/library/alpha/one.mp4", "hash-one"),
                    file("/library/beta/two.mp4", "hash-two"),
                ]),
                &HashMap::new(),
            )
            .expect("fixtures should be indexed");
        for content_hash in ["hash-one", "hash-two"] {
            index
                .replace_ai_annotations(
                    content_hash,
                    &[AiAnnotation {
                        timestamp_ms: 1_000,
                        description: "A person waves".to_owned(),
                        labels: vec!["action: waving".to_owned()],
                        embedding: vec![1.0, 0.0],
                        confidence: Some(0.9),
                        model: "fixture".to_owned(),
                    }],
                )
                .expect("annotation should persist");
        }

        let results = index
            .search_ai_with_focus_under_root(
                "waving",
                &[1.0, 0.0],
                20,
                Some("fixture"),
                AiSearchFocus::Broad,
                Some(Path::new("/library/alpha")),
            )
            .expect("scoped AI search should work");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content_hash, "hash-one");
        assert_eq!(
            index
                .ai_annotation_count_for_model_under_root(
                    "fixture",
                    Some(Path::new("/library/alpha")),
                )
                .expect("scoped count should work"),
            1
        );
    }

    #[test]
    fn duplicate_file_locations_do_not_duplicate_ai_moments() {
        let mut index = SqliteIndex::open_in_memory().expect("index should open");
        index
            .reconcile(
                &report(vec![
                    file("/library/copy-b.mp4", "same-hash"),
                    file("/library/copy-a.mp4", "same-hash"),
                ]),
                &HashMap::new(),
            )
            .expect("fixtures should be indexed");
        index
            .replace_ai_annotations(
                "same-hash",
                &[AiAnnotation {
                    timestamp_ms: 1_000,
                    description: "A car drives past".to_owned(),
                    labels: vec!["action: driving".to_owned()],
                    embedding: vec![1.0, 0.0],
                    confidence: Some(0.9),
                    model: "fixture".to_owned(),
                }],
            )
            .expect("annotation should persist");

        let results = index
            .search_ai("driving", &[1.0, 0.0], 20, Some("fixture"))
            .expect("AI search should work");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "/library/copy-a.mp4");
    }

    #[test]
    fn normal_search_supports_current_folder_and_analyzed_archive_views() {
        let mut index = SqliteIndex::open_in_memory().expect("index should open");
        index
            .reconcile(
                &report(vec![
                    file("/library/alpha/one.mp4", "hash-one"),
                    file("/library/beta/two.mp4", "hash-two"),
                ]),
                &HashMap::new(),
            )
            .expect("fixtures should be indexed");
        index
            .replace_ai_annotations(
                "hash-two",
                &[AiAnnotation {
                    timestamp_ms: 1_000,
                    description: "A saved analyzed scene".to_owned(),
                    labels: Vec::new(),
                    embedding: vec![1.0, 0.0],
                    confidence: Some(0.9),
                    model: "fixture".to_owned(),
                }],
            )
            .expect("annotation should persist");

        let current = index
            .search(&SearchQuery {
                root: Some("/library/alpha".to_owned()),
                ..SearchQuery::default()
            })
            .expect("current-folder search should work");
        let archive = index
            .search(&SearchQuery {
                ai_only: Some(true),
                ..SearchQuery::default()
            })
            .expect("archive search should work");

        assert_eq!(current.len(), 1);
        assert_eq!(current[0].content_hash, "hash-one");
        assert_eq!(archive.len(), 1);
        assert_eq!(archive[0].content_hash, "hash-two");
    }

    #[test]
    fn analyzed_archive_search_keeps_missing_clips_discoverable() {
        let mut index = SqliteIndex::open_in_memory().expect("index should open");
        index
            .reconcile(
                &report(vec![file("/library/old.mp4", "hash-old")]),
                &HashMap::new(),
            )
            .expect("fixture should be indexed");
        index
            .replace_ai_annotations(
                "hash-old",
                &[AiAnnotation {
                    timestamp_ms: 2_000,
                    description: "A person rides a bicycle".to_owned(),
                    labels: vec!["action: cycling".to_owned()],
                    embedding: vec![1.0, 0.0],
                    confidence: Some(0.9),
                    model: "fixture".to_owned(),
                }],
            )
            .expect("annotation should persist");
        index
            .reconcile(&report(Vec::new()), &HashMap::new())
            .expect("empty complete scan should mark the path missing");

        let results = index
            .search_ai("cycling", &[1.0, 0.0], 20, Some("fixture"))
            .expect("saved AI search should work");

        assert_eq!(results.len(), 1);
        assert!(!results[0].available);
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
                root: None,
                ai_only: None,
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

    #[test]
    fn records_request_attempts_with_unknown_usage_explicitly() {
        let mut index = SqliteIndex::open_in_memory().expect("index should open");
        index
            .start_ai_analysis_run(&AiRunSpec {
                run_id: "run-test".to_owned(),
                operation: "search_ai".to_owned(),
                provider: "openai".to_owned(),
                vision_model: "gpt-5.6-luna".to_owned(),
                embedding_model: "text-embedding-3-small".to_owned(),
                transcription_model: None,
                model_namespace: "openai:gpt-5.6-luna:text-embedding-3-small".to_owned(),
                pricing_status: "unknown".to_owned(),
                pricing_checked_at: "2026-09-05".to_owned(),
                estimated_cost_usd: None,
                budget_limit_usd: None,
            })
            .expect("run should be inserted");
        index
            .record_ai_usage_event(&AiUsageEvent {
                local_event_id: "run-test:1".to_owned(),
                run_id: "run-test".to_owned(),
                operation: "OpenAI embedding".to_owned(),
                model: "text-embedding-3-small".to_owned(),
                attempt: 2,
                duration_ms: 180,
                outcome: "response_received".to_owned(),
                status_code: Some(200),
                request_id: Some("req_test".to_owned()),
                usage_status: "not_reported".to_owned(),
                pricing_status: "unknown".to_owned(),
                pricing_checked_at: "2026-09-05".to_owned(),
                reported_input_tokens: None,
                reported_output_tokens: None,
                reported_audio_seconds: None,
                calculated_cost_usd: None,
                possible_cost: true,
            })
            .expect("event should be inserted");
        index
            .finish_ai_analysis_run("run-test", "completed", 0, 0, None)
            .expect("run should finish");

        let event_count: u64 = index
            .connection
            .query_row(
                "SELECT COUNT(*) FROM ai_usage_events WHERE run_id = 'run-test'",
                [],
                |row| row.get(0),
            )
            .expect("event count should load");
        let status: String = index
            .connection
            .query_row(
                "SELECT status FROM ai_analysis_runs WHERE run_id = 'run-test'",
                [],
                |row| row.get(0),
            )
            .expect("run status should load");
        assert_eq!(event_count, 1);
        assert_eq!(status, "completed");
    }
}
