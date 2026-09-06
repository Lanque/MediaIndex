use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const DIAGNOSTICS_DIRECTORY: &str = "ai-diagnostics";
const DIAGNOSTICS_RETENTION_LIMIT: usize = 12;
const DIAGNOSTICS_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug)]
pub(crate) enum StageOutcome {
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct AiStageMetrics {
    pub(crate) elapsed_ms: u64,
    pub(crate) calls: u64,
    pub(crate) succeeded: u64,
    pub(crate) failed: u64,
    pub(crate) cancelled: u64,
}

impl AiStageMetrics {
    fn record(&mut self, elapsed: Duration, outcome: StageOutcome) {
        self.elapsed_ms = self
            .elapsed_ms
            .saturating_add(elapsed.as_millis().min(u128::from(u64::MAX)) as u64);
        self.calls = self.calls.saturating_add(1);
        match outcome {
            StageOutcome::Succeeded => self.succeeded = self.succeeded.saturating_add(1),
            StageOutcome::Failed => self.failed = self.failed.saturating_add(1),
            StageOutcome::Cancelled => self.cancelled = self.cancelled.saturating_add(1),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct AiFileDiagnostics {
    pub(crate) content_id: String,
    pub(crate) status: String,
    pub(crate) wall_time_ms: u64,
    pub(crate) vision_new_frames: u64,
    pub(crate) vision_new_batches: u64,
    pub(crate) vision_reused_frames: u64,
    pub(crate) vision_reused_batches: u64,
    pub(crate) stages: BTreeMap<String, AiStageMetrics>,
}

impl AiFileDiagnostics {
    fn new(content_id: String) -> Self {
        Self {
            content_id,
            status: "running".to_owned(),
            wall_time_ms: 0,
            vision_new_frames: 0,
            vision_new_batches: 0,
            vision_reused_frames: 0,
            vision_reused_batches: 0,
            stages: BTreeMap::new(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct AiFileDiagnosticsHandle {
    state: Arc<Mutex<AiFileDiagnostics>>,
}

impl AiFileDiagnosticsHandle {
    pub(crate) fn record_stage(&self, stage: &str, elapsed: Duration, outcome: StageOutcome) {
        if let Ok(mut state) = self.state.lock() {
            state
                .stages
                .entry(stage.to_owned())
                .or_default()
                .record(elapsed, outcome);
        }
    }

    pub(crate) fn record_vision_batch(&self, reused: bool, frame_count: usize) {
        if let Ok(mut state) = self.state.lock() {
            let frame_count = frame_count as u64;
            if reused {
                state.vision_reused_frames = state.vision_reused_frames.saturating_add(frame_count);
                state.vision_reused_batches = state.vision_reused_batches.saturating_add(1);
            } else {
                state.vision_new_frames = state.vision_new_frames.saturating_add(frame_count);
                state.vision_new_batches = state.vision_new_batches.saturating_add(1);
            }
        }
    }

    pub(crate) fn finish(&self, status: &str, elapsed: Duration) {
        if let Ok(mut state) = self.state.lock() {
            state.status = status.to_owned();
            state.wall_time_ms = elapsed.as_millis().min(u128::from(u64::MAX)) as u64;
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct AiRunDiagnostics {
    pub(crate) schema_version: u32,
    pub(crate) run_id: String,
    pub(crate) operation: String,
    pub(crate) provider: String,
    pub(crate) vision_model: String,
    pub(crate) embedding_model: String,
    pub(crate) transcription_model: Option<String>,
    pub(crate) started_at_unix_ms: u64,
    pub(crate) wall_time_ms: u64,
    pub(crate) status: String,
    pub(crate) total_file_count: u64,
    pub(crate) worker_count: u64,
    pub(crate) run_stages: BTreeMap<String, AiStageMetrics>,
    pub(crate) files: Vec<AiFileDiagnostics>,
}

struct AiRunDiagnosticsState {
    run_stages: BTreeMap<String, AiStageMetrics>,
    files: Vec<Arc<Mutex<AiFileDiagnostics>>>,
}

#[derive(Clone)]
pub(crate) struct AiDiagnosticsRecorder {
    run_id: String,
    operation: String,
    provider: String,
    vision_model: String,
    embedding_model: String,
    transcription_model: Option<String>,
    started_at: Instant,
    started_at_unix_ms: u64,
    total_file_count: u64,
    worker_count: u64,
    state: Arc<Mutex<AiRunDiagnosticsState>>,
}

impl AiDiagnosticsRecorder {
    pub(crate) fn new(
        run_id: impl Into<String>,
        operation: impl Into<String>,
        provider: impl Into<String>,
        vision_model: impl Into<String>,
        embedding_model: impl Into<String>,
        transcription_model: Option<String>,
        started_at: Instant,
        started_at_unix_ms: u64,
        total_file_count: u64,
        worker_count: u64,
    ) -> Self {
        Self {
            run_id: run_id.into(),
            operation: operation.into(),
            provider: provider.into(),
            vision_model: vision_model.into(),
            embedding_model: embedding_model.into(),
            transcription_model,
            started_at,
            started_at_unix_ms,
            total_file_count,
            worker_count,
            state: Arc::new(Mutex::new(AiRunDiagnosticsState {
                run_stages: BTreeMap::new(),
                files: Vec::new(),
            })),
        }
    }

    pub(crate) fn start_file(&self, content_id: impl Into<String>) -> AiFileDiagnosticsHandle {
        let state = Arc::new(Mutex::new(AiFileDiagnostics::new(content_id.into())));
        if let Ok(mut run_state) = self.state.lock() {
            run_state.files.push(state.clone());
        }
        AiFileDiagnosticsHandle { state }
    }

    pub(crate) fn record_stage(&self, stage: &str, elapsed: Duration, outcome: StageOutcome) {
        if let Ok(mut state) = self.state.lock() {
            state
                .run_stages
                .entry(stage.to_owned())
                .or_default()
                .record(elapsed, outcome);
        }
    }

    pub(crate) fn snapshot(&self, status: &str) -> AiRunDiagnostics {
        let (run_stages, mut files) = self
            .state
            .lock()
            .map(|state| {
                let files = state
                    .files
                    .iter()
                    .filter_map(|file| file.lock().ok().map(|file| file.clone()))
                    .collect::<Vec<_>>();
                (state.run_stages.clone(), files)
            })
            .unwrap_or_default();
        files.sort_by(|left, right| left.content_id.cmp(&right.content_id));
        AiRunDiagnostics {
            schema_version: DIAGNOSTICS_SCHEMA_VERSION,
            run_id: self.run_id.clone(),
            operation: self.operation.clone(),
            provider: self.provider.clone(),
            vision_model: self.vision_model.clone(),
            embedding_model: self.embedding_model.clone(),
            transcription_model: self.transcription_model.clone(),
            started_at_unix_ms: self.started_at_unix_ms,
            wall_time_ms: self
                .started_at
                .elapsed()
                .as_millis()
                .min(u128::from(u64::MAX)) as u64,
            status: status.to_owned(),
            total_file_count: self.total_file_count,
            worker_count: self.worker_count,
            run_stages,
            files,
        }
    }

    pub(crate) fn write_json(
        &self,
        app_local_data_directory: &Path,
        status: &str,
    ) -> Result<PathBuf, String> {
        let directory = app_local_data_directory.join(DIAGNOSTICS_DIRECTORY);
        fs::create_dir_all(&directory)
            .map_err(|error| format!("cannot create AI diagnostics directory: {error}"))?;
        let output_path = directory.join(format!("{}.json", self.run_id));
        let temporary_path = directory.join(format!("{}.json.tmp", self.run_id));
        let serialized = serde_json::to_vec_pretty(&self.snapshot(status))
            .map_err(|error| format!("cannot serialize AI diagnostics: {error}"))?;
        fs::write(&temporary_path, serialized)
            .map_err(|error| format!("cannot write AI diagnostics: {error}"))?;
        fs::rename(&temporary_path, &output_path)
            .map_err(|error| format!("cannot commit AI diagnostics: {error}"))?;
        prune_old_diagnostics(&directory);
        Ok(output_path)
    }
}

pub(crate) fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or_default()
}

fn prune_old_diagnostics(directory: &Path) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let mut files = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_type()
                .map(|kind| kind.is_file())
                .unwrap_or(false)
                && entry
                    .path()
                    .extension()
                    .and_then(|extension| extension.to_str())
                    == Some("json")
                && entry.file_name().to_string_lossy().starts_with("run-")
        })
        .collect::<Vec<_>>();
    files.sort_by_key(|entry| {
        entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .unwrap_or(UNIX_EPOCH)
    });
    let remove_count = files.len().saturating_sub(DIAGNOSTICS_RETENTION_LIMIT);
    for entry in files.into_iter().take(remove_count) {
        let _ = fs::remove_file(entry.path());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_accumulate_stages_and_serialize_without_media_paths() {
        let recorder = AiDiagnosticsRecorder::new(
            "run-diagnostics-test",
            "analyze_media_folder",
            "openai",
            "vision-test",
            "embedding-test",
            Some("whisper-test".to_owned()),
            Instant::now(),
            123,
            2,
            2,
        );
        let file = recorder.start_file("content-test");
        file.record_stage(
            "vision_http",
            Duration::from_millis(12),
            StageOutcome::Succeeded,
        );
        file.record_stage(
            "retry_wait",
            Duration::from_millis(7),
            StageOutcome::Cancelled,
        );
        file.record_vision_batch(false, 3);
        file.record_vision_batch(true, 2);
        file.finish("cancelled", Duration::from_millis(25));
        recorder.record_stage(
            "run_sqlite_commit",
            Duration::from_millis(5),
            StageOutcome::Failed,
        );

        let snapshot = recorder.snapshot("cancelled");
        assert_eq!(snapshot.run_id, "run-diagnostics-test");
        assert_eq!(snapshot.files.len(), 1);
        assert_eq!(snapshot.files[0].content_id, "content-test");
        assert_eq!(snapshot.files[0].vision_new_frames, 3);
        assert_eq!(snapshot.files[0].vision_new_batches, 1);
        assert_eq!(snapshot.files[0].vision_reused_frames, 2);
        assert_eq!(snapshot.files[0].vision_reused_batches, 1);
        assert_eq!(snapshot.files[0].stages["vision_http"].calls, 1);
        assert_eq!(snapshot.files[0].stages["retry_wait"].cancelled, 1);
        assert_eq!(snapshot.run_stages["run_sqlite_commit"].failed, 1);

        let serialized = serde_json::to_string(&snapshot).expect("diagnostics should serialize");
        assert!(serialized.contains("content-test"));
        assert!(!serialized.contains("C:\\Users\\"));
        assert!(!serialized.contains("/private/media"));
    }

    #[test]
    fn diagnostics_keep_parallel_file_stage_totals_separate_from_run_wall_time() {
        let recorder = AiDiagnosticsRecorder::new(
            "run-parallel-diagnostics-test",
            "analyze_media_folder",
            "openai",
            "vision-test",
            "embedding-test",
            None,
            Instant::now(),
            123,
            2,
            2,
        );
        let first = recorder.start_file("content-first");
        let second = recorder.start_file("content-second");
        for file in [&first, &second] {
            file.record_stage(
                "vision_http",
                Duration::from_millis(40),
                StageOutcome::Succeeded,
            );
            file.finish("complete", Duration::from_millis(45));
        }
        recorder.record_stage(
            "worker_wall_time",
            Duration::from_millis(50),
            StageOutcome::Succeeded,
        );

        let snapshot = recorder.snapshot("complete");
        assert_eq!(snapshot.worker_count, 2);
        assert_eq!(snapshot.files.len(), 2);
        assert_eq!(snapshot.files[0].stages["vision_http"].elapsed_ms, 40);
        assert_eq!(snapshot.files[1].stages["vision_http"].elapsed_ms, 40);
        assert_eq!(snapshot.files[0].wall_time_ms, 45);
        assert_eq!(snapshot.files[1].wall_time_ms, 45);
        assert_eq!(snapshot.run_stages["worker_wall_time"].elapsed_ms, 50);
        assert_eq!(
            snapshot
                .run_stages
                .get("vision_http")
                .map(|stage| stage.elapsed_ms),
            None,
            "per-file stage time must not be mistaken for run wall time"
        );
    }
}
