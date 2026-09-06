use base64::Engine;
use reqwest::blocking::{multipart, Client, RequestBuilder, Response};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::error::Error as StdError;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::cost;
use crate::metadata::MediaMetadata;
use crate::usage::{AiUsageEventHandle, AiUsageRecorder};

const DEFAULT_OPENAI_VISION_MODEL: &str = "gpt-5.6-luna";
const DEFAULT_OPENAI_EMBEDDING_MODEL: &str = "text-embedding-3-small";
const DEFAULT_OPENAI_TRANSCRIPTION_MODEL: &str = "whisper-1";
const DEFAULT_OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
const DEFAULT_GEMINI_VISION_MODEL: &str = "gemini-3.8-flash";
const DEFAULT_GEMINI_EMBEDDING_MODEL: &str = "gemini-embedding-2";
const DEFAULT_GEMINI_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";
const DEFAULT_LOCAL_VISION_MODEL: &str = "gemma4:e2b";
const DEFAULT_LOCAL_EMBEDDING_MODEL: &str = "embeddinggemma";
const DEFAULT_LOCAL_BASE_URL: &str = "http://127.0.0.1:11434";
const REMOTE_PARALLEL_FILE_LIMIT: usize = 2;
const REMOTE_VISION_BATCH_SIZE: usize = 8;
const MAX_EXTRACTED_FRAME_WIDTH: u32 = 1_280;
const MAX_THUMBNAIL_WIDTH: u32 = 640;
const AI_CONNECT_TIMEOUT_SECONDS: u64 = 20;
const AI_REQUEST_TIMEOUT_SECONDS: u64 = 180;
pub const AI_ANALYSIS_CANCELLED_MESSAGE: &str = "AI analysis cancelled by user";
pub const AI_ANALYSIS_PROMPT_VERSION: &str = "2026-09-05-v1";
pub(crate) const AI_VISION_CHECKPOINT_VERSION: &str = "2026-09-06-v1";

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub enum AiProvider {
    #[serde(rename = "local")]
    Local,
    #[serde(rename = "openai")]
    OpenAI,
    #[serde(rename = "gemini")]
    Gemini,
}

impl AiProvider {
    fn name(&self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::OpenAI => "openai",
            Self::Gemini => "gemini",
        }
    }

    fn is_remote(&self) -> bool {
        !matches!(self, Self::Local)
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiRequestConfig {
    pub provider: Option<AiProvider>,
    pub api_key: Option<String>,
    pub vision_model: Option<String>,
    pub embedding_model: Option<String>,
    pub base_url: Option<String>,
    pub ffmpeg_path: Option<String>,
    pub sample_interval_seconds: Option<u64>,
    pub max_frames: Option<u64>,
    pub context_hint: Option<String>,
    pub transcribe_audio: Option<bool>,
    pub transcription_model: Option<String>,
    pub budget_usd: Option<f64>,
    pub auth_mode: Option<String>,
    pub google_project_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AiAnnotation {
    pub timestamp_ms: u64,
    pub description: String,
    pub labels: Vec<String>,
    pub embedding: Vec<f32>,
    pub confidence: Option<f32>,
    pub model: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiFileAnalysisStatus {
    Complete,
    Partial,
    Failed,
}

impl AiFileAnalysisStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Partial => "partial",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiFrameBatchFailure {
    pub start_timestamp_ms: u64,
    pub end_timestamp_ms: u64,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AiFileAnalysisResult {
    pub annotations: Vec<AiAnnotation>,
    pub planned_frame_count: u64,
    pub successful_frame_count: u64,
    pub failed_batches: Vec<AiFrameBatchFailure>,
    pub status: AiFileAnalysisStatus,
    pub warning: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AiWarning {
    pub path: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct AiIndexReport {
    pub analyzed_file_count: u64,
    pub skipped_file_count: u64,
    pub annotation_count: u64,
    pub partial_file_count: u64,
    pub failed_file_count: u64,
    pub cancelled: bool,
    pub warnings: Vec<AiWarning>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostics_path: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AiProgress {
    pub completed_files: u64,
    pub total_files: u64,
    pub current_file: String,
    pub provider: String,
    pub percent: u8,
    pub phase: String,
}

#[derive(Clone, Debug)]
pub struct AiFileProgress {
    pub percent: u8,
    pub phase: &'static str,
}

#[derive(Debug, Serialize)]
pub struct AiConnectionReport {
    pub provider: String,
    pub vision_model: String,
    pub embedding_model: String,
    pub embedding_dimensions: usize,
}

#[derive(Clone)]
pub struct AiSettings {
    provider: AiProvider,
    api_key: String,
    vision_model: String,
    embedding_model: String,
    base_url: String,
    ffmpeg_executable: PathBuf,
    sample_interval_ms: u64,
    max_frames_per_file: usize,
    context_hint: Option<String>,
    transcribe_audio: bool,
    transcription_model: String,
    budget_usd: Option<f64>,
    usage_recorder: Option<AiUsageRecorder>,
    diagnostics: Option<crate::diagnostics::AiFileDiagnosticsHandle>,
    gemini_uses_oauth: bool,
    google_project_id: Option<String>,
}

fn sanitize_api_key(raw: &str) -> String {
    let mut key = raw.trim();
    if let Some(stripped) = key.strip_prefix("Bearer ") {
        key = stripped.trim();
    } else if let Some(stripped) = key.strip_prefix("bearer ") {
        key = stripped.trim();
    }
    if (key.starts_with('"') && key.ends_with('"'))
        || (key.starts_with('\'') && key.ends_with('\''))
    {
        if key.len() >= 2 {
            key = key[1..key.len() - 1].trim();
        }
    }
    key.chars()
        .filter(|c| !c.is_whitespace() && !c.is_control())
        .collect::<String>()
}

impl AiSettings {
    pub fn from_request(request: Option<AiRequestConfig>) -> Result<Self, String> {
        let request = request.unwrap_or_default();
        let provider = request
            .provider
            .or_else(provider_from_environment)
            .unwrap_or_else(|| {
                if env_non_empty("MEDIAINDEX_OPENAI_API_KEY")
                    .or_else(|| env_non_empty("OPENAI_API_KEY"))
                    .is_some()
                {
                    AiProvider::OpenAI
                } else {
                    AiProvider::Local
                }
            });
        let gemini_uses_oauth =
            provider == AiProvider::Gemini && request.auth_mode.as_deref() == Some("oauth");
        let google_project_id = non_empty(request.google_project_id.clone());

        let api_key = non_empty(request.api_key).or_else(|| match provider {
            AiProvider::OpenAI => env_non_empty("MEDIAINDEX_OPENAI_API_KEY")
                .or_else(|| env_non_empty("OPENAI_API_KEY")),
            AiProvider::Gemini => env_non_empty("MEDIAINDEX_GEMINI_API_KEY")
                .or_else(|| env_non_empty("GEMINI_API_KEY")),
            AiProvider::Local => None,
        });
        let api_key = sanitize_api_key(&api_key.unwrap_or_default());
        if provider.is_remote() && api_key.is_empty() {
            return Err(match provider {
                AiProvider::OpenAI => {
                    "OpenAI needs an API key. Add it under AI connection or set MEDIAINDEX_OPENAI_API_KEY."
                        .to_owned()
                }
                AiProvider::Gemini => {
                    if gemini_uses_oauth {
                        "Google login is missing or expired. Press Login with Google and try again."
                            .to_owned()
                    } else {
                        "Gemini needs an API key or Google login. Add one under AI connection or set MEDIAINDEX_GEMINI_API_KEY."
                            .to_owned()
                    }
                }
                AiProvider::Local => unreachable!(),
            });
        }
        if gemini_uses_oauth && google_project_id.is_none() {
            return Err("Google login did not provide a Cloud project ID. Log in again with a Desktop OAuth client JSON that contains project_id.".to_owned());
        }

        let (default_vision_model, default_embedding_model, default_base_url) = match provider {
            AiProvider::OpenAI => (
                DEFAULT_OPENAI_VISION_MODEL,
                DEFAULT_OPENAI_EMBEDDING_MODEL,
                DEFAULT_OPENAI_BASE_URL,
            ),
            AiProvider::Gemini => (
                DEFAULT_GEMINI_VISION_MODEL,
                DEFAULT_GEMINI_EMBEDDING_MODEL,
                DEFAULT_GEMINI_BASE_URL,
            ),
            AiProvider::Local => (
                DEFAULT_LOCAL_VISION_MODEL,
                DEFAULT_LOCAL_EMBEDDING_MODEL,
                DEFAULT_LOCAL_BASE_URL,
            ),
        };

        let mut vision_model = non_empty(request.vision_model)
            .or_else(|| env_non_empty("MEDIAINDEX_AI_MODEL"))
            .unwrap_or_else(|| default_vision_model.to_owned());
        if provider == AiProvider::OpenAI && vision_model.eq_ignore_ascii_case("04-mini") {
            vision_model = "o4-mini".to_owned();
        }
        let embedding_model = non_empty(request.embedding_model)
            .or_else(|| env_non_empty("MEDIAINDEX_AI_EMBEDDING_MODEL"))
            .unwrap_or_else(|| default_embedding_model.to_owned());
        let mut base_url = non_empty(request.base_url)
            .or_else(|| env_non_empty("MEDIAINDEX_AI_BASE_URL"))
            .or_else(|| match provider {
                AiProvider::OpenAI => env_non_empty("MEDIAINDEX_OPENAI_BASE_URL"),
                AiProvider::Gemini => env_non_empty("MEDIAINDEX_GEMINI_BASE_URL"),
                AiProvider::Local => env_non_empty("MEDIAINDEX_LOCAL_BASE_URL"),
            })
            .unwrap_or_else(|| default_base_url.to_owned())
            .trim_end_matches('/')
            .to_owned();

        match provider {
            AiProvider::OpenAI
                if base_url.contains("googleapis.com") || base_url.contains("11434") =>
            {
                base_url = DEFAULT_OPENAI_BASE_URL.to_owned();
            }
            AiProvider::Gemini
                if base_url.contains("api.openai.com") || base_url.contains("11434") =>
            {
                base_url = DEFAULT_GEMINI_BASE_URL.to_owned();
            }
            AiProvider::Local
                if base_url.contains("googleapis.com") || base_url.contains("api.openai.com") =>
            {
                base_url = DEFAULT_LOCAL_BASE_URL.to_owned();
            }
            _ => {}
        }

        let sample_interval_seconds = match request.sample_interval_seconds {
            Some(value) => value,
            None => environment_u64("MEDIAINDEX_AI_SAMPLE_SECONDS", 5)?,
        };
        let max_frames_per_file = match request.max_frames {
            Some(value) => value,
            None => environment_u64("MEDIAINDEX_AI_MAX_FRAMES", 120)?,
        } as usize;

        let ffmpeg_executable = resolve_ffmpeg_executable(request.ffmpeg_path);
        let context_hint =
            non_empty(request.context_hint).or_else(|| env_non_empty("MEDIAINDEX_AI_CONTEXT"));
        let transcribe_audio =
            provider == AiProvider::OpenAI && request.transcribe_audio.unwrap_or(false);
        let transcription_model = non_empty(request.transcription_model)
            .unwrap_or_else(|| DEFAULT_OPENAI_TRANSCRIPTION_MODEL.to_owned());
        let budget_usd = request
            .budget_usd
            .filter(|value| value.is_finite() && *value > 0.0);

        Ok(Self {
            provider,
            api_key,
            vision_model,
            embedding_model,
            base_url,
            ffmpeg_executable,
            sample_interval_ms: sample_interval_seconds.saturating_mul(1_000),
            max_frames_per_file,
            context_hint,
            transcribe_audio,
            transcription_model,
            budget_usd,
            usage_recorder: None,
            diagnostics: None,
            gemini_uses_oauth,
            google_project_id,
        })
    }

    pub fn model_namespace(&self) -> String {
        let base = format!(
            "{}:{}:{}",
            self.provider.name(),
            self.vision_model,
            self.embedding_model
        );
        if self.transcribe_audio {
            format!("{base}:speech-{}", self.transcription_model)
        } else {
            base
        }
    }

    pub fn analysis_settings_fingerprint(&self) -> String {
        let payload = format!(
            "prompt_version={AI_ANALYSIS_PROMPT_VERSION}\nprovider={}\nvision_model={}\nembedding_model={}\nsample_interval_ms={}\nmax_frames_per_file={}\nvision_batch_size={}\ncontext_hint={}\ntranscribe_audio={}\ntranscription_model={}",
            self.provider.name(),
            self.vision_model,
            self.embedding_model,
            self.sample_interval_ms,
            self.max_frames_per_file,
            self.vision_batch_size(),
            self.context_hint.as_deref().unwrap_or_default(),
            self.transcribe_audio,
            self.transcription_model,
        );
        let digest = Sha256::digest(payload.as_bytes());
        format!("sha256:{digest:x}")
    }

    pub fn parallel_file_limit(&self) -> usize {
        if self.provider.is_remote() {
            REMOTE_PARALLEL_FILE_LIMIT
        } else {
            1
        }
    }

    pub(crate) fn vision_batch_size(&self) -> usize {
        if self.provider.is_remote() {
            REMOTE_VISION_BATCH_SIZE
        } else {
            4
        }
    }

    pub(crate) fn max_frames_per_file(&self) -> usize {
        self.max_frames_per_file
    }

    pub(crate) fn sample_interval_ms(&self) -> u64 {
        self.sample_interval_ms
    }

    pub(crate) fn transcribes_audio(&self) -> bool {
        self.transcribe_audio
    }

    pub(crate) fn provider_name(&self) -> &'static str {
        self.provider.name()
    }

    pub(crate) fn vision_model(&self) -> &str {
        &self.vision_model
    }

    pub(crate) fn embedding_model(&self) -> &str {
        &self.embedding_model
    }

    pub(crate) fn transcription_model(&self) -> &str {
        &self.transcription_model
    }

    pub(crate) fn budget_usd(&self) -> Option<f64> {
        self.budget_usd
    }

    pub(crate) fn with_usage_recorder(mut self, recorder: AiUsageRecorder) -> Self {
        self.usage_recorder = Some(recorder);
        self
    }

    pub(crate) fn with_diagnostics(
        mut self,
        diagnostics: crate::diagnostics::AiFileDiagnosticsHandle,
    ) -> Self {
        self.diagnostics = Some(diagnostics);
        self
    }
}

fn authorize_gemini(request: RequestBuilder, settings: &AiSettings) -> RequestBuilder {
    if settings.gemini_uses_oauth {
        request.bearer_auth(&settings.api_key).header(
            "x-goog-user-project",
            settings.google_project_id.as_deref().unwrap_or_default(),
        )
    } else {
        request.header("x-goog-api-key", &settings.api_key)
    }
}

pub fn resolve_ffmpeg_executable(configured_path: Option<String>) -> PathBuf {
    non_empty(configured_path)
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("MEDIAINDEX_FFMPEG_PATH").map(PathBuf::from))
        .filter(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from("ffmpeg"))
}

thread_local! {
    static CURRENT_FILE_DIAGNOSTICS: RefCell<Option<crate::diagnostics::AiFileDiagnosticsHandle>> = const {
        RefCell::new(None)
    };
}

struct FileDiagnosticsScope {
    previous: Option<crate::diagnostics::AiFileDiagnosticsHandle>,
}

impl FileDiagnosticsScope {
    fn enter(diagnostics: Option<crate::diagnostics::AiFileDiagnosticsHandle>) -> Self {
        let previous = CURRENT_FILE_DIAGNOSTICS.with(|current| current.replace(diagnostics));
        Self { previous }
    }
}

impl Drop for FileDiagnosticsScope {
    fn drop(&mut self) {
        CURRENT_FILE_DIAGNOSTICS.with(|current| {
            let _ = current.replace(self.previous.take());
        });
    }
}

fn record_current_stage(stage: &str, elapsed: Duration, outcome: crate::diagnostics::StageOutcome) {
    CURRENT_FILE_DIAGNOSTICS.with(|current| {
        if let Some(diagnostics) = current.borrow().as_ref() {
            diagnostics.record_stage(stage, elapsed, outcome);
        }
    });
}

fn record_current_http_attempt(operation: &str, elapsed: Duration, succeeded: bool) {
    let stage = if operation.contains("vision") {
        "vision_http"
    } else if operation.contains("embedding") {
        "embedding_http"
    } else if operation.contains("speech transcription") {
        "transcription_http"
    } else {
        "other_http"
    };
    record_current_stage(
        stage,
        elapsed,
        if succeeded {
            crate::diagnostics::StageOutcome::Succeeded
        } else {
            crate::diagnostics::StageOutcome::Failed
        },
    );
}

fn record_current_vision_batch(reused: bool, frame_count: usize) {
    CURRENT_FILE_DIAGNOSTICS.with(|current| {
        if let Some(diagnostics) = current.borrow().as_ref() {
            diagnostics.record_vision_batch(reused, frame_count);
        }
    });
}

pub fn analyze_file(
    path: &Path,
    metadata: Option<&MediaMetadata>,
    settings: &AiSettings,
) -> Result<Vec<AiAnnotation>, String> {
    analyze_file_with_progress(path, metadata, settings, |_| {})
}

pub fn analyze_file_with_progress<F>(
    path: &Path,
    metadata: Option<&MediaMetadata>,
    settings: &AiSettings,
    progress: F,
) -> Result<Vec<AiAnnotation>, String>
where
    F: Fn(AiFileProgress),
{
    analyze_file_with_progress_and_cancel(path, metadata, settings, progress, || false)
        .map(|result| result.annotations)
}

pub fn analyze_file_with_progress_and_cancel<F, C>(
    path: &Path,
    metadata: Option<&MediaMetadata>,
    settings: &AiSettings,
    progress: F,
    is_cancelled: C,
) -> Result<AiFileAnalysisResult, String>
where
    F: Fn(AiFileProgress),
    C: Fn() -> bool,
{
    analyze_file_with_progress_and_cancel_with_checkpoints(
        path,
        metadata,
        settings,
        progress,
        is_cancelled,
        |_frame_timestamps, _batch_size| Ok(Vec::new()),
        |_checkpoint| Ok(()),
    )
}

pub(crate) fn analyze_file_with_progress_and_cancel_with_checkpoints<F, C, L, S>(
    path: &Path,
    metadata: Option<&MediaMetadata>,
    settings: &AiSettings,
    progress: F,
    is_cancelled: C,
    load_checkpoints: L,
    save_checkpoint: S,
) -> Result<AiFileAnalysisResult, String>
where
    F: Fn(AiFileProgress),
    C: Fn() -> bool,
    L: FnOnce(&[u64], usize) -> Result<Vec<AiVisionCheckpointBatch>, String>,
    S: FnMut(AiVisionCheckpointBatch) -> Result<(), String>,
{
    let _diagnostics_scope = FileDiagnosticsScope::enter(settings.diagnostics.clone());
    ensure_analysis_not_cancelled(&is_cancelled)?;
    let client = build_http_client()?;
    progress(AiFileProgress {
        percent: 1,
        phase: "Extracting frames",
    });
    let extraction_started = std::time::Instant::now();
    let extraction = extract_frames(path, settings);
    let extraction_outcome = if extraction.is_ok() {
        crate::diagnostics::StageOutcome::Succeeded
    } else {
        crate::diagnostics::StageOutcome::Failed
    };
    record_current_stage(
        "frame_extraction",
        extraction_started.elapsed(),
        extraction_outcome,
    );
    let frames = match extraction {
        Ok(frames) => frames,
        Err(error) => {
            return Ok(failed_file_analysis_result(
                0,
                0,
                Vec::new(),
                format!("frame extraction failed for {}: {error}", path.display()),
            ));
        }
    };
    ensure_analysis_not_cancelled(&is_cancelled)?;
    progress(AiFileProgress {
        percent: 10,
        phase: "Analyzing frames",
    });
    let should_transcribe_audio = settings.transcribe_audio
        && metadata
            .and_then(|metadata| metadata.audio_codec.as_deref())
            .is_some();
    let vision_percent_span = if should_transcribe_audio { 60 } else { 80 };
    let (mut analyses, frame_errors) = analyze_vision_batches(
        path,
        &client,
        &frames,
        settings,
        vision_percent_span,
        &progress,
        &is_cancelled,
        load_checkpoints,
        save_checkpoint,
    )?;
    if analyses.is_empty() {
        let detail = frame_errors
            .first()
            .map(|failure| failure.message.clone())
            .unwrap_or_else(|| "no frame analysis was returned".to_owned());
        return Ok(failed_file_analysis_result(
            frames.len(),
            0,
            frame_errors,
            format!(
                "AI vision produced no usable frames for {}: {detail}",
                path.display()
            ),
        ));
    }

    if should_transcribe_audio {
        progress(AiFileProgress {
            percent: 72,
            phase: "Extracting speech audio",
        });
        ensure_analysis_not_cancelled(&is_cancelled)?;
        let audio_started = std::time::Instant::now();
        let extracted_audio_result = extract_audio_track(path, settings);
        record_current_stage(
            "audio_extraction",
            audio_started.elapsed(),
            if extracted_audio_result.is_ok() {
                crate::diagnostics::StageOutcome::Succeeded
            } else {
                crate::diagnostics::StageOutcome::Failed
            },
        );
        let extracted_audio = match extracted_audio_result {
            Ok(audio) => audio,
            Err(error) => {
                return Ok(failed_file_analysis_result(
                    frames.len(),
                    analyses.len(),
                    frame_errors,
                    format!("audio extraction failed for {}: {error}", path.display()),
                ));
            }
        };
        progress(AiFileProgress {
            percent: 78,
            phase: "Transcribing speech",
        });
        let transcription_started = std::time::Instant::now();
        let transcription =
            transcribe_openai_audio(&client, &extracted_audio, settings, &is_cancelled);
        record_current_stage(
            "transcription",
            transcription_started.elapsed(),
            if transcription.is_ok() {
                crate::diagnostics::StageOutcome::Succeeded
            } else if transcription
                .as_ref()
                .is_err_and(|error| error == AI_ANALYSIS_CANCELLED_MESSAGE)
            {
                crate::diagnostics::StageOutcome::Cancelled
            } else {
                crate::diagnostics::StageOutcome::Failed
            },
        );
        let _ = fs::remove_file(&extracted_audio.path);
        let transcript_segments = match transcription {
            Ok(segments) => segments,
            Err(error) => {
                return transcription_failure_result(
                    error,
                    path,
                    frames.len(),
                    analyses.len(),
                    frame_errors,
                );
            }
        };
        ensure_analysis_not_cancelled(&is_cancelled)?;
        attach_transcript_segments(&mut analyses, &transcript_segments);
        progress(AiFileProgress {
            percent: 90,
            phase: "Speech indexed",
        });
    }

    let embedding_texts = analyses
        .iter()
        .map(|(_, analysis)| {
            format!(
                "{}\nEntities: {}\nActions: {}\nSetting: {}\nSituation: {}\nDialogue: {}\nLabels: {}\nOn-screen text: {}",
                analysis.description,
                analysis.entities.join(", "),
                analysis.actions.join(", "),
                analysis.setting.as_deref().unwrap_or_default(),
                analysis.situation.as_deref().unwrap_or_default(),
                analysis.dialogue.join(" | "),
                analysis.labels.join(", "),
                analysis.visible_text.join(" | ")
            )
        })
        .collect::<Vec<_>>();
    progress(AiFileProgress {
        percent: 92,
        phase: "Creating search index",
    });
    ensure_analysis_not_cancelled(&is_cancelled)?;
    let embeddings_started = std::time::Instant::now();
    let embeddings_result = create_embeddings(
        &client,
        &embedding_texts,
        settings,
        EmbeddingKind::Document,
        &is_cancelled,
    );
    record_current_stage(
        "embeddings",
        embeddings_started.elapsed(),
        if embeddings_result.is_ok() {
            crate::diagnostics::StageOutcome::Succeeded
        } else if embeddings_result
            .as_ref()
            .is_err_and(|error| error == AI_ANALYSIS_CANCELLED_MESSAGE)
        {
            crate::diagnostics::StageOutcome::Cancelled
        } else {
            crate::diagnostics::StageOutcome::Failed
        },
    );
    let embeddings = match embeddings_result {
        Ok(embeddings) => embeddings,
        Err(error) if error == AI_ANALYSIS_CANCELLED_MESSAGE => return Err(error),
        Err(error) => {
            return Ok(failed_file_analysis_result(
                frames.len(),
                analyses.len(),
                frame_errors,
                format!("embedding creation failed for {}: {error}", path.display()),
            ));
        }
    };
    ensure_analysis_not_cancelled(&is_cancelled)?;
    if embeddings.len() != analyses.len() {
        return Ok(failed_file_analysis_result(
            frames.len(),
            analyses.len(),
            frame_errors,
            format!(
                "AI returned {} embeddings for {} analyzed frames",
                embeddings.len(),
                analyses.len()
            ),
        ));
    }

    let successful_frame_count = analyses.len() as u64;
    let status = if frame_errors.is_empty() && analyses.len() == frames.len() {
        AiFileAnalysisStatus::Complete
    } else {
        AiFileAnalysisStatus::Partial
    };
    let warning = match status {
        AiFileAnalysisStatus::Complete => None,
        AiFileAnalysisStatus::Partial => Some(partial_coverage_warning(
            path,
            frames.len() as u64,
            successful_frame_count,
            &frame_errors,
        )),
        AiFileAnalysisStatus::Failed => {
            unreachable!("failed results return before annotation construction")
        }
    };

    let mut annotations = Vec::with_capacity(analyses.len());
    for ((timestamp_ms, analysis), embedding) in analyses.into_iter().zip(embeddings) {
        let visible_text = normalize_labels(analysis.visible_text);
        let entities = normalize_labels(analysis.entities);
        let actions = normalize_labels(analysis.actions);
        let dialogue = normalize_labels(analysis.dialogue);
        let setting = normalize_optional_text(analysis.setting);
        let situation = normalize_optional_text(analysis.situation);
        let mut labels = normalize_labels(analysis.labels);
        labels.extend(entities.iter().map(|entity| format!("entity: {entity}")));
        labels.extend(actions.iter().map(|action| format!("action: {action}")));
        labels.extend(dialogue.iter().map(|line| format!("dialogue: {line}")));
        if let Some(setting) = &setting {
            labels.push(format!("setting: {setting}"));
        }
        if let Some(situation) = &situation {
            labels.push(format!("situation: {situation}"));
        }
        labels.extend(
            visible_text
                .iter()
                .map(|text| format!("on-screen text: {text}")),
        );
        labels.sort();
        labels.dedup();
        let mut details = Vec::new();
        if !entities.is_empty() {
            details.push(format!("Entities: {}", entities.join(", ")));
        }
        if !actions.is_empty() {
            details.push(format!("Actions: {}", actions.join(", ")));
        }
        if !dialogue.is_empty() {
            details.push(format!("Dialogue: {}", dialogue.join(" | ")));
        }
        if let Some(setting) = setting {
            details.push(format!("Setting: {setting}"));
        }
        if let Some(situation) = situation {
            details.push(format!("Situation: {situation}"));
        }
        if !visible_text.is_empty() {
            details.push(format!("On-screen text: {}", visible_text.join(" | ")));
        }
        let description = if details.is_empty() {
            analysis.description
        } else {
            format!("{} {}", analysis.description, details.join(". "))
        };
        annotations.push(AiAnnotation {
            timestamp_ms,
            description,
            labels,
            embedding,
            confidence: analysis.confidence,
            model: settings.model_namespace(),
        });
    }

    progress(AiFileProgress {
        percent: 100,
        phase: if status == AiFileAnalysisStatus::Complete {
            "Finished"
        } else {
            "Finished with partial coverage"
        },
    });
    Ok(AiFileAnalysisResult {
        annotations,
        planned_frame_count: frames.len() as u64,
        successful_frame_count,
        failed_batches: frame_errors,
        status,
        warning,
    })
}

fn failed_file_analysis_result(
    planned_frame_count: usize,
    successful_frame_count: usize,
    failed_batches: Vec<AiFrameBatchFailure>,
    warning: String,
) -> AiFileAnalysisResult {
    AiFileAnalysisResult {
        annotations: Vec::new(),
        planned_frame_count: planned_frame_count as u64,
        successful_frame_count: successful_frame_count as u64,
        failed_batches,
        status: AiFileAnalysisStatus::Failed,
        warning: Some(warning),
    }
}

fn transcription_failure_result(
    error: String,
    path: &Path,
    planned_frame_count: usize,
    successful_frame_count: usize,
    failed_batches: Vec<AiFrameBatchFailure>,
) -> Result<AiFileAnalysisResult, String> {
    if error == AI_ANALYSIS_CANCELLED_MESSAGE {
        return Err(error);
    }
    Ok(failed_file_analysis_result(
        planned_frame_count,
        successful_frame_count,
        failed_batches,
        format!(
            "speech transcription failed for {}: {error}",
            path.display()
        ),
    ))
}

fn partial_coverage_warning(
    path: &Path,
    planned_frame_count: u64,
    successful_frame_count: u64,
    failed_batches: &[AiFrameBatchFailure],
) -> String {
    let ranges = failed_batches
        .iter()
        .map(|failure| {
            format!(
                "{}-{} ms ({})",
                failure.start_timestamp_ms, failure.end_timestamp_ms, failure.message
            )
        })
        .collect::<Vec<_>>();
    let failed_summary = if ranges.is_empty() {
        "one or more frame batches did not complete".to_owned()
    } else {
        ranges.join(", ")
    };
    format!(
        "Partial AI analysis for {}: {successful_frame_count}/{planned_frame_count} frames succeeded; failed batches: {failed_summary}. Rerun explicitly to complete coverage.",
        path.display()
    )
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
struct TranscriptSegment {
    start: f64,
    end: f64,
    text: String,
}

#[derive(Debug, Deserialize)]
struct TimestampedTranscript {
    #[serde(default)]
    segments: Vec<TranscriptSegment>,
}

struct ExtractedAudio {
    path: PathBuf,
    duration_seconds: f64,
}

fn extract_audio_track(path: &Path, settings: &AiSettings) -> Result<ExtractedAudio, String> {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or_default();
    let output_path = std::env::temp_dir().join(format!(
        "mediaindex-speech-{}-{unique}.mp3",
        std::process::id()
    ));
    let maximum_seconds = settings
        .sample_interval_ms
        .saturating_mul(settings.max_frames_per_file as u64)
        .div_ceil(1_000)
        .max(1);
    let mut command = Command::new(&settings.ffmpeg_executable);
    configure_hidden_process(&mut command);
    let output = command
        .args(["-hide_banner", "-loglevel", "error", "-i"])
        .arg(path)
        .args([
            "-map", "0:a:0", "-vn", "-ac", "1", "-ar", "16000", "-b:a", "32k", "-t",
        ])
        .arg(maximum_seconds.to_string())
        .args(["-f", "mp3", "-y"])
        .arg(&output_path)
        .output()
        .map_err(|error| {
            format!(
                "FFmpeg could not extract speech audio from {}: {error}",
                path.display()
            )
        })?;
    if !output.status.success() {
        let _ = fs::remove_file(&output_path);
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if stderr.is_empty() {
            format!(
                "FFmpeg could not extract speech audio from {}",
                path.display()
            )
        } else {
            format!(
                "FFmpeg could not extract speech audio from {}: {stderr}",
                path.display()
            )
        });
    }
    if !output_path.is_file()
        || fs::metadata(&output_path)
            .map(|value| value.len())
            .unwrap_or(0)
            == 0
    {
        let _ = fs::remove_file(&output_path);
        return Err(format!(
            "FFmpeg produced no speech audio for {}",
            path.display()
        ));
    }
    let duration_probe_started = std::time::Instant::now();
    let duration_probe = measure_audio_duration(&output_path, &settings.ffmpeg_executable);
    record_current_stage(
        "audio_duration_probe",
        duration_probe_started.elapsed(),
        if duration_probe.is_ok() {
            crate::diagnostics::StageOutcome::Succeeded
        } else {
            crate::diagnostics::StageOutcome::Failed
        },
    );
    let duration_seconds = match duration_probe {
        Ok(duration_seconds) => duration_seconds,
        Err(error) => {
            let _ = fs::remove_file(&output_path);
            return Err(error);
        }
    };
    Ok(ExtractedAudio {
        path: output_path,
        duration_seconds,
    })
}

fn measure_audio_duration(path: &Path, ffmpeg_executable: &Path) -> Result<f64, String> {
    let ffprobe_executable = resolve_ffprobe_executable(ffmpeg_executable);
    let mut command = Command::new(&ffprobe_executable);
    configure_hidden_process(&mut command);
    let output = command
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(path)
        .output()
        .map_err(|error| {
            format!(
                "FFprobe could not be started for extracted speech audio {}: {error}",
                path.display()
            )
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if stderr.is_empty() {
            format!(
                "FFprobe could not measure extracted speech audio {}",
                path.display()
            )
        } else {
            format!(
                "FFprobe could not measure extracted speech audio {}: {stderr}",
                path.display()
            )
        });
    }
    let raw_duration = String::from_utf8_lossy(&output.stdout);
    parse_audio_duration(&raw_duration).ok_or_else(|| {
        format!(
            "FFprobe returned no valid duration for extracted speech audio {}",
            path.display()
        )
    })
}

fn resolve_ffprobe_executable(ffmpeg_executable: &Path) -> PathBuf {
    let Some(file_name) = ffmpeg_executable.file_name().and_then(|name| name.to_str()) else {
        return PathBuf::from("ffprobe");
    };
    let ffprobe_name = if file_name.eq_ignore_ascii_case("ffmpeg.exe") {
        "ffprobe.exe"
    } else if file_name.eq_ignore_ascii_case("ffmpeg") {
        "ffprobe"
    } else {
        return PathBuf::from("ffprobe");
    };
    ffmpeg_executable
        .parent()
        .map(|parent| parent.join(ffprobe_name))
        .filter(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from(ffprobe_name))
}

fn parse_audio_duration(raw_duration: &str) -> Option<f64> {
    let duration_seconds = raw_duration.trim().parse::<f64>().ok()?;
    (duration_seconds.is_finite() && duration_seconds > 0.0).then_some(duration_seconds)
}

fn transcribe_openai_audio<C: Fn() -> bool>(
    client: &Client,
    audio: &ExtractedAudio,
    settings: &AiSettings,
    is_cancelled: &C,
) -> Result<Vec<TranscriptSegment>, String> {
    if settings.provider != AiProvider::OpenAI {
        return Ok(Vec::new());
    }
    let audio_bytes = fs::read(&audio.path).map_err(|error| {
        format!(
            "cannot read extracted speech audio {}: {error}",
            audio.path.display()
        )
    })?;
    let response = execute_with_retry_cancellable(
        "OpenAI speech transcription",
        &settings.transcription_model,
        settings.usage_recorder.as_ref(),
        cost::transcription_request_cost(
            settings.provider_name(),
            &settings.transcription_model,
            audio.duration_seconds,
        ),
        is_cancelled,
        || {
            let part = multipart::Part::bytes(audio_bytes.clone())
                .file_name("speech.mp3")
                .mime_str("audio/mpeg")?;
            let form = multipart::Form::new()
                .text("model", settings.transcription_model.clone())
                .text("response_format", "verbose_json")
                .text("timestamp_granularities[]", "segment")
                .part("file", part);
            client
                .post(format!("{}/audio/transcriptions", settings.base_url))
                .bearer_auth(&settings.api_key)
                .multipart(form)
                .send()
        },
    )?;
    let body = read_json_response(
        response,
        "OpenAI speech transcription",
        &settings.transcription_model,
        settings.usage_recorder.as_ref(),
        Some(audio.duration_seconds),
    )?;
    let transcript: TimestampedTranscript = serde_json::from_value(body).map_err(|error| {
        format!("OpenAI speech transcription returned invalid timestamps: {error}")
    })?;
    Ok(transcript
        .segments
        .into_iter()
        .filter_map(|segment| {
            let text = segment.text.trim().to_owned();
            (!text.is_empty() && segment.start.is_finite() && segment.end.is_finite()).then_some(
                TranscriptSegment {
                    start: segment.start.max(0.0),
                    end: segment.end.max(segment.start).max(0.0),
                    text,
                },
            )
        })
        .collect())
}

fn attach_transcript_segments(
    analyses: &mut [(u64, FrameAnalysis)],
    segments: &[TranscriptSegment],
) {
    if analyses.is_empty() {
        return;
    }
    for segment in segments {
        let midpoint_ms = (((segment.start + segment.end) / 2.0) * 1_000.0).max(0.0) as u64;
        let target = analyses
            .partition_point(|(timestamp_ms, _)| *timestamp_ms <= midpoint_ms)
            .saturating_sub(1)
            .min(analyses.len() - 1);
        analyses[target].1.dialogue.push(segment.text.clone());
    }
}

fn ensure_analysis_not_cancelled<C>(is_cancelled: &C) -> Result<(), String>
where
    C: Fn() -> bool,
{
    if is_cancelled() {
        Err(AI_ANALYSIS_CANCELLED_MESSAGE.to_owned())
    } else {
        Ok(())
    }
}

fn analyze_vision_batches<F, C, L, S>(
    path: &Path,
    client: &Client,
    frames: &[(u64, Vec<u8>)],
    settings: &AiSettings,
    vision_percent_span: u8,
    progress: &F,
    is_cancelled: &C,
    load_checkpoints: L,
    mut save_checkpoint: S,
) -> Result<(Vec<(u64, FrameAnalysis)>, Vec<AiFrameBatchFailure>), String>
where
    F: Fn(AiFileProgress),
    C: Fn() -> bool,
    L: FnOnce(&[u64], usize) -> Result<Vec<AiVisionCheckpointBatch>, String>,
    S: FnMut(AiVisionCheckpointBatch) -> Result<(), String>,
{
    let frame_timestamps = frames
        .iter()
        .map(|(timestamp_ms, _)| *timestamp_ms)
        .collect::<Vec<_>>();
    let checkpoint_lookup_started = std::time::Instant::now();
    let checkpoint_batches_result =
        load_checkpoints(&frame_timestamps, settings.vision_batch_size());
    record_current_stage(
        "checkpoint_lookup",
        checkpoint_lookup_started.elapsed(),
        if checkpoint_batches_result.is_ok() {
            crate::diagnostics::StageOutcome::Succeeded
        } else if checkpoint_batches_result
            .as_ref()
            .is_err_and(|error| error == AI_ANALYSIS_CANCELLED_MESSAGE)
        {
            crate::diagnostics::StageOutcome::Cancelled
        } else {
            crate::diagnostics::StageOutcome::Failed
        },
    );
    let checkpoint_batches = checkpoint_batches_result?;
    let mut analyses = Vec::with_capacity(frames.len());
    let mut frame_errors = Vec::new();
    let mut processed_frames = 0usize;
    for (batch_index, frame_batch) in frames.chunks(settings.vision_batch_size()).enumerate() {
        ensure_analysis_not_cancelled(is_cancelled)?;
        let batch_timestamps = frame_batch
            .iter()
            .map(|(timestamp_ms, _)| *timestamp_ms)
            .collect::<Vec<_>>();
        let checkpoint = checkpoint_batches.iter().find(|checkpoint| {
            checkpoint.batch_index == batch_index
                && checkpoint.frame_timestamps == frame_timestamps
                && checkpoint.batch_timestamps == batch_timestamps
                && checkpoint.analyses.len() == frame_batch.len()
        });
        let reused_checkpoint = checkpoint.is_some();
        record_current_vision_batch(reused_checkpoint, frame_batch.len());
        if let Some(checkpoint) = checkpoint {
            analyses.extend(
                frame_batch
                    .iter()
                    .map(|(timestamp_ms, _)| *timestamp_ms)
                    .zip(checkpoint.analyses.iter().cloned()),
            );
        } else {
            match describe_frames(client, frame_batch, settings, is_cancelled) {
                Ok(batch) if batch.len() == frame_batch.len() => {
                    let checkpoint_save_started = std::time::Instant::now();
                    let checkpoint_save_result = save_checkpoint(AiVisionCheckpointBatch {
                        batch_index,
                        frame_timestamps: frame_timestamps.clone(),
                        batch_timestamps,
                        analyses: batch.clone(),
                    });
                    record_current_stage(
                        "checkpoint_save_ack",
                        checkpoint_save_started.elapsed(),
                        if checkpoint_save_result.is_ok() {
                            crate::diagnostics::StageOutcome::Succeeded
                        } else if checkpoint_save_result
                            .as_ref()
                            .is_err_and(|error| error == AI_ANALYSIS_CANCELLED_MESSAGE)
                        {
                            crate::diagnostics::StageOutcome::Cancelled
                        } else {
                            crate::diagnostics::StageOutcome::Failed
                        },
                    );
                    checkpoint_save_result.map_err(|error| {
                        format!(
                            "cannot persist vision checkpoint for {} batch {}: {error}",
                            path.display(),
                            batch_index + 1
                        )
                    })?;
                    analyses.extend(
                        frame_batch
                            .iter()
                            .map(|(timestamp_ms, _)| *timestamp_ms)
                            .zip(batch),
                    );
                }
                Ok(batch) => frame_errors.push(AiFrameBatchFailure {
                    start_timestamp_ms: frame_batch
                        .first()
                        .map(|frame| frame.0)
                        .unwrap_or_default(),
                    end_timestamp_ms: frame_batch.last().map(|frame| frame.0).unwrap_or_default(),
                    message: format!(
                        "vision returned {} analyses for {} frames",
                        batch.len(),
                        frame_batch.len()
                    ),
                }),
                Err(error) => frame_errors.push(AiFrameBatchFailure {
                    start_timestamp_ms: frame_batch
                        .first()
                        .map(|frame| frame.0)
                        .unwrap_or_default(),
                    end_timestamp_ms: frame_batch.last().map(|frame| frame.0).unwrap_or_default(),
                    message: error,
                }),
            }
        }
        ensure_analysis_not_cancelled(is_cancelled)?;
        processed_frames += frame_batch.len();
        let vision_percent =
            10 + ((processed_frames * usize::from(vision_percent_span)) / frames.len()) as u8;
        progress(AiFileProgress {
            percent: vision_percent.min(10 + vision_percent_span),
            phase: if reused_checkpoint {
                "Reusing saved frame analysis"
            } else {
                "Analyzing frames"
            },
        });
    }
    Ok((analyses, frame_errors))
}

pub fn embed_query(query: &str, settings: &AiSettings) -> Result<Vec<f32>, String> {
    if query.trim().is_empty() {
        return Err("AI search query cannot be empty".to_owned());
    }
    let client = build_http_client()?;
    create_embedding(&client, query.trim(), settings, EmbeddingKind::Query)
}

pub fn test_connection(settings: &AiSettings) -> Result<AiConnectionReport, String> {
    let client = build_http_client()?;
    validate_vision_model(&client, settings)?;
    let embedding = create_embedding(
        &client,
        "MediaIndex connection test",
        settings,
        EmbeddingKind::Query,
    )?;
    Ok(AiConnectionReport {
        provider: settings.provider.name().to_owned(),
        vision_model: settings.vision_model.clone(),
        embedding_model: settings.embedding_model.clone(),
        embedding_dimensions: embedding.len(),
    })
}

fn validate_vision_model(client: &Client, settings: &AiSettings) -> Result<(), String> {
    let clean_model = settings.vision_model.trim_start_matches("models/").trim();
    let response = match settings.provider {
        AiProvider::OpenAI => execute_with_retry(
            "OpenAI vision model check",
            &settings.vision_model,
            settings.usage_recorder.as_ref(),
            Some(0.0),
            || {
                client
                    .get(format!("{}/models/{clean_model}", settings.base_url))
                    .bearer_auth(&settings.api_key)
                    .send()
            },
        )?,
        AiProvider::Gemini => execute_with_retry(
            "Gemini vision model check",
            &settings.vision_model,
            settings.usage_recorder.as_ref(),
            Some(0.0),
            || {
                authorize_gemini(
                    client.get(format!("{}/models/{clean_model}", settings.base_url)),
                    settings,
                )
                .send()
            },
        )?,
        AiProvider::Local => execute_with_retry(
            "Local AI vision model check",
            &settings.vision_model,
            settings.usage_recorder.as_ref(),
            Some(0.0),
            || {
                client
                    .post(format!("{}/api/show", settings.base_url))
                    .json(&json!({"model": clean_model}))
                    .send()
            },
        )?,
    };
    read_json_response(
        response,
        match settings.provider {
            AiProvider::OpenAI => "OpenAI vision model check",
            AiProvider::Gemini => "Gemini vision model check",
            AiProvider::Local => "Local AI vision model check",
        },
        &settings.vision_model,
        settings.usage_recorder.as_ref(),
        None,
    )?;
    Ok(())
}

fn environment_u64(name: &str, default: u64) -> Result<u64, String> {
    let value = std::env::var(name).unwrap_or_else(|_| default.to_string());
    value
        .parse::<u64>()
        .map_err(|_| format!("{name} must be a positive integer"))
        .and_then(|parsed| {
            if parsed == 0 {
                Err(format!("{name} must be greater than zero"))
            } else {
                Ok(parsed)
            }
        })
}

fn env_non_empty(name: &str) -> Option<String> {
    non_empty(std::env::var(name).ok())
}

fn non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn build_http_client() -> Result<Client, String> {
    Client::builder()
        .connect_timeout(Duration::from_secs(AI_CONNECT_TIMEOUT_SECONDS))
        .timeout(Duration::from_secs(AI_REQUEST_TIMEOUT_SECONDS))
        .pool_max_idle_per_host(REMOTE_PARALLEL_FILE_LIMIT)
        .build()
        .map_err(|error| format!("cannot create AI HTTP client: {error}"))
}

fn request_failure(operation: &str, error: &reqwest::Error) -> String {
    let mut chain = vec![error.to_string()];
    let mut source = StdError::source(error);
    while let Some(cause) = source {
        let detail = cause.to_string();
        if chain.last() != Some(&detail) {
            chain.push(detail);
        }
        source = cause.source();
    }

    let category = if error.is_timeout() {
        "timeout"
    } else if error.is_connect() {
        "connection/TLS"
    } else if error.is_body() {
        "upload"
    } else if error.is_decode() {
        "response decoding"
    } else {
        "transport"
    };
    let root_cause = chain.last().cloned().unwrap_or_else(|| error.to_string());
    format!(
        "{operation} request failed ({category}): {root_cause}. Details: {}",
        chain.join(" -> ")
    )
}

fn is_transient_status(status: reqwest::StatusCode) -> bool {
    status == reqwest::StatusCode::TOO_MANY_REQUESTS
        || status == reqwest::StatusCode::SERVICE_UNAVAILABLE
        || status == reqwest::StatusCode::GATEWAY_TIMEOUT
        || status == reqwest::StatusCode::BAD_GATEWAY
        || status == reqwest::StatusCode::INTERNAL_SERVER_ERROR
}

struct TrackedResponse {
    response: Response,
    usage_event: Option<AiUsageEventHandle>,
}

impl TrackedResponse {
    fn status(&self) -> reqwest::StatusCode {
        self.response.status()
    }
}

fn execute_with_retry<F>(
    operation_name: &str,
    model: &str,
    recorder: Option<&AiUsageRecorder>,
    request_reserve_usd: Option<f64>,
    make_request: F,
) -> Result<TrackedResponse, String>
where
    F: FnMut() -> Result<Response, reqwest::Error>,
{
    execute_with_retry_cancellable(
        operation_name,
        model,
        recorder,
        request_reserve_usd,
        &never_cancelled,
        make_request,
    )
}

fn execute_with_retry_cancellable<F, C>(
    operation_name: &str,
    model: &str,
    recorder: Option<&AiUsageRecorder>,
    request_reserve_usd: Option<f64>,
    is_cancelled: &C,
    mut make_request: F,
) -> Result<TrackedResponse, String>
where
    F: FnMut() -> Result<Response, reqwest::Error>,
    C: Fn() -> bool,
{
    const MAX_ATTEMPTS: usize = 4;
    let mut attempt = 0;
    loop {
        if is_cancelled() {
            return Err(AI_ANALYSIS_CANCELLED_MESSAGE.to_owned());
        }
        attempt += 1;
        let usage_event = if let Some(recorder) = recorder {
            match recorder.begin_reserved_request(
                operation_name,
                model,
                attempt as u32,
                request_reserve_usd,
            ) {
                Some(handle) => Some(handle),
                None => {
                    recorder.record_budget_blocked(operation_name, model, attempt as u32);
                    return Err(
                        "AI analysis stopped because the configured budget reserve is exhausted."
                            .to_owned(),
                    );
                }
            }
        } else {
            None
        };
        if is_cancelled() {
            if let Some(usage_event) = usage_event.as_ref() {
                usage_event.cancel_before_send();
            }
            return Err(AI_ANALYSIS_CANCELLED_MESSAGE.to_owned());
        }
        let started = std::time::Instant::now();
        match make_request() {
            Ok(response) => {
                let status = response.status();
                record_current_http_attempt(operation_name, started.elapsed(), status.is_success());
                let retryable = is_transient_status(status) && attempt < MAX_ATTEMPTS;
                if let Some(usage_event) = usage_event.as_ref() {
                    usage_event.record_response(
                        started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
                        if retryable {
                            "retryable_http_error"
                        } else if status.is_success() {
                            "response_received"
                        } else {
                            "http_error"
                        },
                        Some(status.as_u16()),
                        response_request_id(&response),
                    );
                }
                if retryable {
                    if let Some(usage_event) = usage_event.as_ref() {
                        usage_event.finish();
                    }
                    let delay_ms = match attempt {
                        1 => 1_500,
                        2 => 3_500,
                        _ => 6_000,
                    };
                    let retry_wait_started = std::time::Instant::now();
                    let retry_cancelled = wait_for_retry_or_cancel(delay_ms, is_cancelled);
                    record_current_stage(
                        "retry_wait",
                        retry_wait_started.elapsed(),
                        if retry_cancelled {
                            crate::diagnostics::StageOutcome::Cancelled
                        } else {
                            crate::diagnostics::StageOutcome::Succeeded
                        },
                    );
                    if retry_cancelled {
                        return Err(AI_ANALYSIS_CANCELLED_MESSAGE.to_owned());
                    }
                    continue;
                }
                return Ok(TrackedResponse {
                    response,
                    usage_event,
                });
            }
            Err(error) => {
                record_current_http_attempt(operation_name, started.elapsed(), false);
                let retryable =
                    (error.is_timeout() || error.is_connect()) && attempt < MAX_ATTEMPTS;
                if let Some(usage_event) = usage_event.as_ref() {
                    usage_event.record_response(
                        started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
                        if retryable {
                            "retryable_transport_error"
                        } else {
                            "transport_error"
                        },
                        None,
                        None,
                    );
                    usage_event.finish();
                }
                if retryable {
                    let retry_wait_started = std::time::Instant::now();
                    let retry_cancelled =
                        wait_for_retry_or_cancel(1_500 * attempt as u64, is_cancelled);
                    record_current_stage(
                        "retry_wait",
                        retry_wait_started.elapsed(),
                        if retry_cancelled {
                            crate::diagnostics::StageOutcome::Cancelled
                        } else {
                            crate::diagnostics::StageOutcome::Succeeded
                        },
                    );
                    if retry_cancelled {
                        return Err(AI_ANALYSIS_CANCELLED_MESSAGE.to_owned());
                    }
                    continue;
                }
                return Err(request_failure(operation_name, &error));
            }
        }
    }
}

fn wait_for_retry_or_cancel<C: Fn() -> bool>(delay_ms: u64, is_cancelled: &C) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_millis(delay_ms);
    while !is_cancelled() {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return false;
        }
        std::thread::sleep(remaining.min(Duration::from_millis(50)));
    }
    true
}

fn never_cancelled() -> bool {
    false
}

fn response_request_id(response: &Response) -> Option<String> {
    ["x-request-id", "request-id", "x-goog-request-id"]
        .iter()
        .find_map(|header| {
            response
                .headers()
                .get(*header)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned)
        })
}

fn provider_from_environment() -> Option<AiProvider> {
    match env_non_empty("MEDIAINDEX_AI_PROVIDER")?
        .to_ascii_lowercase()
        .as_str()
    {
        "local" | "ollama" => Some(AiProvider::Local),
        "openai" | "chatgpt" => Some(AiProvider::OpenAI),
        "gemini" | "google" => Some(AiProvider::Gemini),
        _ => None,
    }
}

fn extract_frames(path: &Path, settings: &AiSettings) -> Result<Vec<(u64, Vec<u8>)>, String> {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or_default();
    let output_directory =
        std::env::temp_dir().join(format!("mediaindex-ai-{}-{unique}", std::process::id()));
    fs::create_dir_all(&output_directory)
        .map_err(|error| format!("cannot create temporary AI frame directory: {error}"))?;
    let output_pattern = output_directory.join("frame-%06d.jpg");
    let sample_seconds = (settings.sample_interval_ms / 1_000).max(1);
    let filter = format!(
        "fps=1/{sample_seconds},scale=w='min({MAX_EXTRACTED_FRAME_WIDTH},iw)':h=-2,format=yuvj420p"
    );
    let mut command = Command::new(&settings.ffmpeg_executable);
    configure_hidden_process(&mut command);
    let output = command
        .args(["-hide_banner", "-loglevel", "error", "-i"])
        .arg(path)
        .args(["-vf", &filter, "-frames:v"])
        .arg(settings.max_frames_per_file.to_string())
        .args(["-q:v", "5", "-f", "image2"])
        .arg(&output_pattern)
        .output()
        .map_err(|error| {
            format!(
                "FFmpeg could not be started for {}: {error}",
                path.display()
            )
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let _ = fs::remove_dir_all(&output_directory);
        return Err(if stderr.is_empty() {
            format!(
                "FFmpeg failed for {} with status {}",
                path.display(),
                output.status
            )
        } else {
            format!("FFmpeg failed for {}: {stderr}", path.display())
        });
    }

    let mut frames = Vec::new();
    for index in 1..=settings.max_frames_per_file {
        let frame_path = output_directory.join(format!("frame-{index:06}.jpg"));
        if !frame_path.is_file() {
            break;
        }
        let frame = fs::read(&frame_path).map_err(|error| {
            format!(
                "cannot read extracted AI frame {}: {error}",
                frame_path.display()
            )
        })?;
        let timestamp_ms = (index as u64 - 1).saturating_mul(settings.sample_interval_ms);
        frames.push((timestamp_ms, frame));
    }
    let _ = fs::remove_dir_all(&output_directory);
    if frames.is_empty() {
        return Err(format!("FFmpeg produced no frames for {}", path.display()));
    }
    Ok(frames)
}

fn jpeg_dimensions(data: &[u8]) -> Option<cost::ImageDimensions> {
    if data.get(..2) != Some(&[0xff, 0xd8]) {
        return None;
    }
    let mut index = 2;
    while index + 1 < data.len() {
        if data[index] != 0xff {
            index += 1;
            continue;
        }
        while data.get(index) == Some(&0xff) {
            index += 1;
        }
        let marker = *data.get(index)?;
        index += 1;
        if matches!(marker, 0xd8 | 0xd9 | 0x01 | 0xd0..=0xd7) {
            continue;
        }
        let segment_length = u16::from_be_bytes([*data.get(index)?, *data.get(index + 1)?]);
        if segment_length < 2 || index + usize::from(segment_length) > data.len() {
            return None;
        }
        if matches!(
            marker,
            0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf
        ) {
            let height = u16::from_be_bytes([*data.get(index + 3)?, *data.get(index + 4)?]);
            let width = u16::from_be_bytes([*data.get(index + 5)?, *data.get(index + 6)?]);
            return (width > 0 && height > 0).then_some(cost::ImageDimensions {
                width: u32::from(width),
                height: u32::from(height),
            });
        }
        index += usize::from(segment_length);
    }
    None
}

pub fn extract_thumbnail(
    path: &Path,
    timestamp_ms: u64,
    ffmpeg_executable: &Path,
) -> Result<Vec<u8>, String> {
    let filter = format!("scale=w='min({MAX_THUMBNAIL_WIDTH},iw)':h=-2,format=yuvj420p");
    let mut command = Command::new(ffmpeg_executable);
    configure_hidden_process(&mut command);
    let output = command
        .args(["-hide_banner", "-loglevel", "error", "-ss"])
        .arg(format!("{:.3}", timestamp_ms as f64 / 1_000.0))
        .arg("-i")
        .arg(path)
        .args([
            "-map",
            "0:v:0",
            "-frames:v",
            "1",
            "-vf",
            &filter,
            "-q:v",
            "4",
            "-f",
            "image2pipe",
            "-vcodec",
            "mjpeg",
            "pipe:1",
        ])
        .output()
        .map_err(|error| {
            format!(
                "FFmpeg could not create a thumbnail for {}: {error}",
                path.display()
            )
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if stderr.is_empty() {
            format!(
                "FFmpeg thumbnail extraction failed for {} with status {}",
                path.display(),
                output.status
            )
        } else {
            format!(
                "FFmpeg thumbnail extraction failed for {}: {stderr}",
                path.display()
            )
        });
    }
    if output.stdout.is_empty() {
        return Err(format!(
            "FFmpeg produced no thumbnail for {} at {timestamp_ms} ms",
            path.display()
        ));
    }
    Ok(output.stdout)
}

fn configure_hidden_process(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct FrameAnalysis {
    pub(crate) description: String,
    #[serde(default)]
    pub(crate) labels: Vec<String>,
    #[serde(default)]
    pub(crate) visible_text: Vec<String>,
    #[serde(default)]
    pub(crate) entities: Vec<String>,
    #[serde(default)]
    pub(crate) actions: Vec<String>,
    #[serde(default)]
    pub(crate) dialogue: Vec<String>,
    #[serde(default)]
    pub(crate) setting: Option<String>,
    #[serde(default)]
    pub(crate) situation: Option<String>,
    pub(crate) confidence: Option<f32>,
}

#[derive(Clone, Debug)]
pub(crate) struct AiVisionCheckpointBatch {
    pub batch_index: usize,
    pub frame_timestamps: Vec<u64>,
    pub batch_timestamps: Vec<u64>,
    pub analyses: Vec<FrameAnalysis>,
}

pub(crate) fn validate_checkpoint_analyses(
    analyses: Vec<FrameAnalysis>,
    expected_len: usize,
) -> Option<Vec<FrameAnalysis>> {
    if analyses.len() != expected_len {
        return None;
    }
    analyses
        .into_iter()
        .map(normalize_frame_analysis)
        .collect::<Result<Vec<_>, _>>()
        .ok()
}

#[derive(Clone, Copy)]
enum EmbeddingKind {
    Document,
    Query,
}

fn describe_frames<C: Fn() -> bool>(
    client: &Client,
    frames: &[(u64, Vec<u8>)],
    settings: &AiSettings,
    is_cancelled: &C,
) -> Result<Vec<FrameAnalysis>, String> {
    let frame_encoding_started = std::time::Instant::now();
    let encoded_frames = frames
        .iter()
        .map(|(timestamp_ms, frame)| {
            (
                *timestamp_ms,
                base64::engine::general_purpose::STANDARD.encode(frame),
            )
        })
        .collect::<Vec<_>>();
    let dimensions = frames
        .iter()
        .map(|(_, frame)| {
            jpeg_dimensions(frame).unwrap_or(cost::ImageDimensions {
                width: MAX_EXTRACTED_FRAME_WIDTH,
                height: MAX_EXTRACTED_FRAME_WIDTH * 9 / 16,
            })
        })
        .collect::<Vec<_>>();
    record_current_stage(
        "frame_encoding",
        frame_encoding_started.elapsed(),
        crate::diagnostics::StageOutcome::Succeeded,
    );
    let timestamps = encoded_frames
        .iter()
        .map(|(timestamp_ms, _)| format!("{timestamp_ms} ms"))
        .collect::<Vec<_>>()
        .join(", ");
    let library_context = settings.context_hint.as_deref().map_or_else(
        || "No user-provided library context is available.".to_owned(),
        |context| {
            format!(
                "User-provided library context (candidate information, not proof): {context}. Apply a supplied name or circumstance only when the frame is visually consistent with it."
            )
        },
    );
    let prompt = format!(
        "Analyze these ordered video frames for a general-purpose searchable media library. The frame timestamps, in order, are: {timestamps}. {library_context} Use adjacent frames as temporal context so recurring subjects stay consistent and an ongoing action or situation is understood as a sequence. Return only a JSON object with a frames array containing exactly one object per input frame, in the same order. Each frame object must contain: description (one concise factual sentence covering who or what is visible, what is happening, and the important context); entities (lowercase array of confidently recognizable fictional characters, game characters, creatures, teams, franchises, products, vehicles, landmarks, or named objects); actions (lowercase array of concrete actions and interactions); setting (short lowercase location or environment, or an empty string); situation (short lowercase event or circumstance such as conversation, ceremony, chase, battle, tutorial, performance, sports play, accident, travel, gameplay event, or an empty string); dialogue (array of exact dialogue that is visibly shown in subtitles, captions, or speech bubbles; never infer unheard audio); labels (lowercase array covering useful subjects, objects, genre, visual style, mood, shot type, and concepts); visible_text (array of exact readable words or short phrases from subtitles, signs, titles, HUD, menus, score overlays, or logos); and confidence (number from 0 to 1). Name a well-known fictional character or franchise only when distinctive visual evidence supports it; otherwise describe appearance and role precisely. Never identify a real person from their face alone—use a real person's name only when readable on-screen text establishes it. Inspect the full frame, including background details and small UI text. Add useful search synonyms only when supported by the image. Do not invent identities, actions, relationships, locations, events, audio, or text. Use empty arrays or strings when evidence is insufficient."
    );
    let text = match settings.provider {
        AiProvider::OpenAI => describe_openai(
            client,
            &encoded_frames,
            &dimensions,
            &prompt,
            settings,
            is_cancelled,
        )?,
        AiProvider::Gemini => describe_gemini(
            client,
            &encoded_frames,
            &dimensions,
            &prompt,
            settings,
            is_cancelled,
        )?,
        AiProvider::Local => {
            describe_local(client, &encoded_frames, &prompt, settings, is_cancelled)?
        }
    };
    parse_frame_analyses(&text)
}

fn describe_openai<C: Fn() -> bool>(
    client: &Client,
    encoded_frames: &[(u64, String)],
    dimensions: &[cost::ImageDimensions],
    prompt: &str,
    settings: &AiSettings,
    is_cancelled: &C,
) -> Result<String, String> {
    let mut content = vec![json!({"type": "input_text", "text": prompt})];
    content.extend(encoded_frames.iter().map(|(_, encoded)| {
        json!({
            "type": "input_image",
            "image_url": format!("data:image/jpeg;base64,{encoded}"),
            "detail": "high"
        })
    }));
    let frame_count = encoded_frames.len();
    let output_token_limit = (frame_count * 320).clamp(1_280, 8_192);
    let mut request = json!({
        "model": settings.vision_model,
        "store": false,
        "max_output_tokens": output_token_limit,
        "text": {
            "format": {
                "type": "json_schema",
                "name": "mediaindex_frame_analyses",
                "strict": true,
                "schema": {
                    "type": "object",
                    "properties": {
                        "frames": {
                            "type": "array",
                            "minItems": frame_count,
                            "maxItems": frame_count,
                            "items": {
                                "type": "object",
                                "properties": {
                                    "description": {"type": "string"},
                                    "labels": {
                                        "type": "array",
                                        "items": {"type": "string"}
                                    },
                                    "visible_text": {
                                        "type": "array",
                                        "items": {"type": "string"}
                                    },
                                    "entities": {
                                        "type": "array",
                                        "items": {"type": "string"}
                                    },
                                    "actions": {
                                        "type": "array",
                                        "items": {"type": "string"}
                                    },
                                    "dialogue": {
                                        "type": "array",
                                        "items": {"type": "string"}
                                    },
                                    "setting": {"type": "string"},
                                    "situation": {"type": "string"},
                                    "confidence": {
                                        "type": "number",
                                        "minimum": 0,
                                        "maximum": 1
                                    }
                                },
                                "required": [
                                    "description",
                                    "labels",
                                    "visible_text",
                                    "entities",
                                    "actions",
                                    "dialogue",
                                    "setting",
                                    "situation",
                                    "confidence"
                                ],
                                "additionalProperties": false
                            }
                        }
                    },
                    "required": ["frames"],
                    "additionalProperties": false
                }
            }
        },
        "input": [{
            "role": "user",
            "content": content
        }]
    });
    if let Some(effort) = openai_reasoning_effort(&settings.vision_model) {
        request["reasoning"] = json!({"effort": effort});
    }
    let response = execute_with_retry_cancellable(
        "OpenAI vision",
        &settings.vision_model,
        settings.usage_recorder.as_ref(),
        cost::vision_request_cost(
            settings.provider_name(),
            &settings.vision_model,
            dimensions,
            cost::text_tokens_upper(prompt).saturating_add(8_192),
            output_token_limit as u64,
        ),
        is_cancelled,
        || {
            client
                .post(format!("{}/responses", settings.base_url))
                .bearer_auth(&settings.api_key)
                .json(&request)
                .send()
        },
    )?;
    let body = read_json_response(
        response,
        "OpenAI vision",
        &settings.vision_model,
        settings.usage_recorder.as_ref(),
        None,
    )?;
    response_text(&body).ok_or_else(|| "OpenAI vision returned no output text".to_owned())
}

fn openai_reasoning_effort(model: &str) -> Option<&'static str> {
    let model = model.to_ascii_lowercase();
    if model.starts_with("gpt-5.6-terra") {
        Some("low")
    } else if model.starts_with("gpt-5.6") {
        Some("none")
    } else {
        None
    }
}

fn describe_gemini<C: Fn() -> bool>(
    client: &Client,
    encoded_frames: &[(u64, String)],
    dimensions: &[cost::ImageDimensions],
    prompt: &str,
    settings: &AiSettings,
    is_cancelled: &C,
) -> Result<String, String> {
    let clean_model = settings.vision_model.trim_start_matches("models/").trim();
    let mut parts = vec![json!({"text": prompt})];
    parts.extend(
        encoded_frames.iter().map(
            |(_, encoded)| json!({"inline_data": {"mime_type": "image/jpeg", "data": encoded}}),
        ),
    );
    let url = format!(
        "{}/models/{}:generateContent",
        settings.base_url, clean_model
    );
    let payload = json!({
        "contents": [{
            "role": "user",
            "parts": parts
        }],
        "generationConfig": {"responseMimeType": "application/json"}
    });
    let response = execute_with_retry_cancellable(
        "Gemini vision",
        &settings.vision_model,
        settings.usage_recorder.as_ref(),
        cost::vision_request_cost(
            settings.provider_name(),
            &settings.vision_model,
            dimensions,
            cost::text_tokens_upper(prompt),
            encoded_frames.len() as u64 * 640,
        ),
        is_cancelled,
        || {
            authorize_gemini(client.post(&url), settings)
                .json(&payload)
                .send()
        },
    )?;
    let body = read_json_response(
        response,
        "Gemini vision",
        &settings.vision_model,
        settings.usage_recorder.as_ref(),
        None,
    )?;
    response_text(&body).ok_or_else(|| "Gemini vision returned no candidate text".to_owned())
}

fn describe_local<C: Fn() -> bool>(
    client: &Client,
    encoded_frames: &[(u64, String)],
    prompt: &str,
    settings: &AiSettings,
    is_cancelled: &C,
) -> Result<String, String> {
    let payload = json!({
        "model": settings.vision_model,
        "messages": [{
            "role": "user",
            "content": prompt,
            "images": encoded_frames.iter().map(|(_, encoded)| encoded).collect::<Vec<_>>()
        }],
        "format": "json",
        "stream": false
    });
    let response = execute_with_retry_cancellable(
        "Local AI vision",
        &settings.vision_model,
        settings.usage_recorder.as_ref(),
        Some(0.0),
        is_cancelled,
        || {
            client
                .post(format!("{}/api/chat", settings.base_url))
                .json(&payload)
                .send()
        },
    )?;
    let body = read_json_response(
        response,
        "Local AI vision",
        &settings.vision_model,
        settings.usage_recorder.as_ref(),
        None,
    )?;
    response_text(&body).ok_or_else(|| "Local AI returned no message content".to_owned())
}

fn parse_frame_analyses(text: &str) -> Result<Vec<FrameAnalysis>, String> {
    let json_text = strip_json_fence(text);
    let parsed: Vec<FrameAnalysis> = serde_json::from_str(&json_text)
        .or_else(|_| {
            serde_json::from_str::<FrameAnalysis>(
                extract_json_object(&json_text).unwrap_or(&json_text),
            )
            .map(|frame| vec![frame])
        })
        .or_else(|_| {
            serde_json::from_str::<Value>(&json_text)
                .ok()
                .and_then(|value| value.get("frames").cloned())
                .ok_or_else(|| serde_json::Error::io(std::io::Error::other("missing frames")))
                .and_then(|value| serde_json::from_value(value))
        })
        .map_err(|error| format!("AI returned invalid annotations: {error}"))?;
    parsed.into_iter().map(normalize_frame_analysis).collect()
}

fn normalize_frame_analysis(parsed: FrameAnalysis) -> Result<FrameAnalysis, String> {
    if parsed.description.trim().is_empty() {
        return Err("AI returned an empty description".to_owned());
    }
    Ok(FrameAnalysis {
        description: parsed.description.trim().to_owned(),
        labels: normalize_labels(parsed.labels),
        visible_text: normalize_labels(parsed.visible_text),
        entities: normalize_labels(parsed.entities),
        actions: normalize_labels(parsed.actions),
        dialogue: normalize_labels(parsed.dialogue),
        setting: normalize_optional_text(parsed.setting),
        situation: normalize_optional_text(parsed.situation),
        confidence: parsed.confidence.map(|value| value.clamp(0.0, 1.0)),
    })
}

fn create_embedding(
    client: &Client,
    text: &str,
    settings: &AiSettings,
    kind: EmbeddingKind,
) -> Result<Vec<f32>, String> {
    create_embeddings(client, &[text.to_owned()], settings, kind, &never_cancelled)?
        .into_iter()
        .next()
        .ok_or_else(|| "AI embedding returned no vector".to_owned())
}

fn create_embeddings<C: Fn() -> bool>(
    client: &Client,
    texts: &[String],
    settings: &AiSettings,
    kind: EmbeddingKind,
    is_cancelled: &C,
) -> Result<Vec<Vec<f32>>, String> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }

    let embeddings = match settings.provider {
        AiProvider::OpenAI => {
            let payload = json!({
                "model": settings.embedding_model,
                "input": texts
            });
            let response = execute_with_retry_cancellable(
                "OpenAI embedding",
                &settings.embedding_model,
                settings.usage_recorder.as_ref(),
                cost::embedding_request_cost(
                    settings.provider_name(),
                    &settings.embedding_model,
                    texts.iter().map(|text| cost::text_tokens_upper(text)).sum(),
                ),
                is_cancelled,
                || {
                    client
                        .post(format!("{}/embeddings", settings.base_url))
                        .bearer_auth(&settings.api_key)
                        .json(&payload)
                        .send()
                },
            )?;
            let body = read_json_response(
                response,
                "OpenAI embedding",
                &settings.embedding_model,
                settings.usage_recorder.as_ref(),
                None,
            )?;
            body.get("data")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.get("embedding").and_then(Value::as_array))
                        .map(values_to_embedding)
                        .collect::<Vec<_>>()
                })
                .ok_or_else(|| "OpenAI embedding returned no vectors".to_owned())?
        }
        AiProvider::Gemini => {
            let mut embeddings = Vec::with_capacity(texts.len());
            for text in texts {
                ensure_analysis_not_cancelled(is_cancelled)?;
                embeddings.push(create_gemini_embedding(
                    client,
                    text,
                    settings,
                    kind,
                    is_cancelled,
                )?);
            }
            embeddings
        }
        AiProvider::Local => {
            let payload = json!({
                "model": settings.embedding_model,
                "input": texts
            });
            let response = execute_with_retry_cancellable(
                "Local AI embedding",
                &settings.embedding_model,
                settings.usage_recorder.as_ref(),
                Some(0.0),
                is_cancelled,
                || {
                    client
                        .post(format!("{}/api/embed", settings.base_url))
                        .json(&payload)
                        .send()
                },
            )?;
            let body = read_json_response(
                response,
                "Local AI embedding",
                &settings.embedding_model,
                settings.usage_recorder.as_ref(),
                None,
            )?;
            body.get("embeddings")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_array)
                        .map(values_to_embedding)
                        .collect::<Vec<_>>()
                })
                .ok_or_else(|| "Local AI embedding returned no vectors".to_owned())?
        }
    };

    if embeddings.len() != texts.len() || embeddings.iter().any(Vec::is_empty) {
        return Err(format!(
            "AI embedding returned {} vectors for {} texts",
            embeddings.len(),
            texts.len()
        ));
    }
    Ok(embeddings)
}

fn request_single_gemini_embedding<C: Fn() -> bool>(
    client: &Client,
    model: &str,
    text: &str,
    settings: &AiSettings,
    task_type: &str,
    is_cancelled: &C,
) -> Result<Vec<f32>, (bool, String)> {
    let clean_model = model.trim_start_matches("models/").trim();
    let url = format!("{}/models/{}:embedContent", settings.base_url, clean_model);
    let is_embedding_2 = clean_model.eq_ignore_ascii_case("gemini-embedding-2");
    let prepared_text = if is_embedding_2 {
        match task_type {
            "RETRIEVAL_DOCUMENT" => format!("title: none | text: {text}"),
            _ => format!("task: search result | query: {text}"),
        }
    } else {
        text.to_owned()
    };
    let mut payload = json!({
        "model": format!("models/{}", clean_model),
        "content": {"parts": [{"text": prepared_text}]}
    });
    if is_embedding_2 {
        payload["output_dimensionality"] = json!(768);
    } else {
        payload["taskType"] = json!(task_type);
    }
    let response = execute_with_retry_cancellable(
        "Gemini embedding",
        &settings.embedding_model,
        settings.usage_recorder.as_ref(),
        cost::embedding_request_cost(
            settings.provider_name(),
            &settings.embedding_model,
            cost::text_tokens_upper(&prepared_text),
        ),
        is_cancelled,
        || {
            authorize_gemini(client.post(&url), settings)
                .json(&payload)
                .send()
        },
    )
    .map_err(|err| (false, err))?;
    let is_not_found = response.status() == reqwest::StatusCode::NOT_FOUND;
    let body = read_json_response(
        response,
        "Gemini embedding",
        &settings.embedding_model,
        settings.usage_recorder.as_ref(),
        None,
    )
    .map_err(|err| (is_not_found, err))?;
    let embedding = body
        .get("embedding")
        .and_then(|embedding| embedding.get("values"))
        .and_then(Value::as_array)
        .map(values_to_embedding)
        .or_else(|| {
            body.get("embeddings")
                .and_then(Value::as_array)
                .and_then(|items| items.first())
                .and_then(|embedding| embedding.get("values"))
                .and_then(Value::as_array)
                .map(values_to_embedding)
        });
    embedding
        .filter(|values| !values.is_empty())
        .ok_or_else(|| (false, "Gemini embedding returned no vector".to_owned()))
}

fn create_gemini_embedding<C: Fn() -> bool>(
    client: &Client,
    text: &str,
    settings: &AiSettings,
    kind: EmbeddingKind,
    is_cancelled: &C,
) -> Result<Vec<f32>, String> {
    let task_type = match kind {
        EmbeddingKind::Document => "RETRIEVAL_DOCUMENT",
        EmbeddingKind::Query => "RETRIEVAL_QUERY",
    };
    request_single_gemini_embedding(
        client,
        &settings.embedding_model,
        text,
        settings,
        task_type,
        is_cancelled,
    )
    .map_err(|(_, err)| err)
}

fn values_to_embedding(values: &Vec<Value>) -> Vec<f32> {
    values
        .iter()
        .filter_map(Value::as_f64)
        .map(|value| value as f32)
        .collect()
}

fn read_json_response(
    tracked_response: TrackedResponse,
    operation: &str,
    model: &str,
    recorder: Option<&AiUsageRecorder>,
    estimated_audio_seconds: Option<f64>,
) -> Result<Value, String> {
    let started = std::time::Instant::now();
    let result = read_json_response_inner(
        tracked_response,
        operation,
        model,
        recorder,
        estimated_audio_seconds,
    );
    let stage = if operation.contains("vision") {
        "vision_response_parse"
    } else if operation.contains("embedding") {
        "embedding_response_parse"
    } else if operation.contains("speech transcription") {
        "transcription_response_parse"
    } else {
        "other_response_parse"
    };
    record_current_stage(
        stage,
        started.elapsed(),
        if result.is_ok() {
            crate::diagnostics::StageOutcome::Succeeded
        } else if result
            .as_ref()
            .is_err_and(|error| error == AI_ANALYSIS_CANCELLED_MESSAGE)
        {
            crate::diagnostics::StageOutcome::Cancelled
        } else {
            crate::diagnostics::StageOutcome::Failed
        },
    );
    result
}

fn read_json_response_inner(
    tracked_response: TrackedResponse,
    operation: &str,
    _model: &str,
    _recorder: Option<&AiUsageRecorder>,
    estimated_audio_seconds: Option<f64>,
) -> Result<Value, String> {
    let TrackedResponse {
        response,
        usage_event,
    } = tracked_response;
    let status = response.status();
    let raw = match response.text() {
        Ok(raw) => raw,
        Err(error) => {
            if let Some(usage_event) = usage_event.as_ref() {
                usage_event.finish();
            }
            return Err(format!(
                "{operation} returned an unreadable response: {error}"
            ));
        }
    };
    let body: Value = match serde_json::from_str(&raw) {
        Ok(body) => body,
        Err(error) => {
            if let Some(usage_event) = usage_event.as_ref() {
                usage_event.finish();
            }
            return Err(format!(
                "{operation} failed (HTTP {status}) with non-JSON response: {} ({error})",
                truncate(&raw, 500)
            ));
        }
    };
    if let Some(usage_event) = usage_event.as_ref() {
        let (input_tokens, output_tokens) = reported_token_usage(&body);
        let reported_audio_seconds = reported_audio_seconds(&body, operation);
        usage_event.record_reported_usage(
            _recorder
                .map(|recorder| recorder.provider_name())
                .unwrap_or_default(),
            input_tokens,
            output_tokens,
            reported_audio_seconds,
            estimated_audio_seconds,
        );
        usage_event.finish();
    }
    if !status.is_success() {
        return Err(api_failure_message(
            operation,
            status,
            &api_error_detail(&body),
        ));
    }
    if response_body_reports_error(&body) {
        return Err(api_failure_message(
            operation,
            status,
            &api_error_detail(&body),
        ));
    }
    Ok(body)
}

fn reported_token_usage(body: &Value) -> (Option<u64>, Option<u64>) {
    let input_tokens = body
        .pointer("/usage/input_tokens")
        .or_else(|| body.pointer("/usage/prompt_tokens"))
        .or_else(|| body.pointer("/usageMetadata/promptTokenCount"))
        .and_then(Value::as_u64);
    let output_tokens = body
        .pointer("/usage/output_tokens")
        .or_else(|| body.pointer("/usage/completion_tokens"))
        .or_else(|| body.pointer("/usageMetadata/candidatesTokenCount"))
        .and_then(Value::as_u64);
    (input_tokens, output_tokens)
}

fn reported_audio_seconds(body: &Value, operation: &str) -> Option<f64> {
    operation
        .contains("speech transcription")
        .then(|| body.pointer("/duration").and_then(Value::as_f64))
        .flatten()
        .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
}

fn api_failure_message(operation: &str, status: reqwest::StatusCode, detail: &str) -> String {
    let mut message = format!("{operation} failed (HTTP {status}): {detail}");
    if operation.starts_with("Gemini") {
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            message.push_str(
                ". Create a current auth key in Google AI Studio. Unrestricted standard keys are rejected, and Google announced the remaining standard-key shutdown for September 2026",
            );
        } else if status == reqwest::StatusCode::NOT_FOUND {
            message.push_str(
                ". Check the exact Gemini model name; removed embedding models such as text-embedding-004 and embedding-001 no longer work",
            );
        }
    } else if operation.starts_with("OpenAI") {
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            message.push_str(
                ". Check that this is an active OpenAI API key and that its project can use the selected model",
            );
        } else if status == reqwest::StatusCode::NOT_FOUND {
            message.push_str(
                ". Check that the selected OpenAI model exists and is available to this project",
            );
        }
    } else if operation.starts_with("Local AI") && status == reqwest::StatusCode::NOT_FOUND {
        message.push_str(
            ". Pull the exact vision or embedding model shown in settings with `ollama pull <model>`",
        );
    }
    message
}

fn response_body_reports_error(body: &Value) -> bool {
    body.get("error").is_some_and(|error| !error.is_null())
        || body
            .get("status")
            .and_then(Value::as_str)
            .is_some_and(|status| status.eq_ignore_ascii_case("failed"))
}

fn api_error_detail(body: &Value) -> String {
    if let Some(error) = body.get("error") {
        if let Some(message) = error.as_str() {
            return message.to_owned();
        }
        if let Some(object) = error.as_object() {
            let message = object.get("message").and_then(Value::as_str);
            let code = object.get("code").and_then(Value::as_str);
            let error_type = object.get("type").and_then(Value::as_str);
            let detail = [message, code, error_type]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>();
            if !detail.is_empty() {
                return detail.join(" · ");
            }
        }
    }
    if let Some(message) = body.get("message").and_then(Value::as_str) {
        return message.to_owned();
    }
    truncate(
        &serde_json::to_string(body).unwrap_or_else(|_| "unknown response".to_owned()),
        800,
    )
}

fn response_text(body: &Value) -> Option<String> {
    if let Some(text) = output_text(body) {
        return Some(text);
    }
    if let Some(text) = body
        .get("message")
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
    {
        return Some(text.to_owned());
    }
    body.get("candidates")
        .and_then(Value::as_array)
        .and_then(|candidates| candidates.first())
        .and_then(|candidate| candidate.get("content"))
        .and_then(|content| content.get("parts"))
        .and_then(Value::as_array)
        .map(|parts| {
            parts
                .iter()
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .filter(|text| !text.is_empty())
}

fn output_text(body: &Value) -> Option<String> {
    fn walk(value: &Value, output: &mut Vec<String>) {
        match value {
            Value::Object(object) => {
                if object.get("type").and_then(Value::as_str) == Some("output_text") {
                    if let Some(text) = object.get("text").and_then(Value::as_str) {
                        output.push(text.to_owned());
                    }
                }
                for child in object.values() {
                    walk(child, output);
                }
            }
            Value::Array(array) => {
                for child in array {
                    walk(child, output);
                }
            }
            _ => {}
        }
    }

    let mut output = Vec::new();
    walk(body, &mut output);
    (!output.is_empty()).then(|| output.join("\n"))
}

fn truncate(value: &str, limit: usize) -> String {
    let mut result = value.chars().take(limit).collect::<String>();
    if value.chars().count() > limit {
        result.push_str("…");
    }
    result
}

fn strip_json_fence(text: &str) -> String {
    let trimmed = text.trim();
    if let Some(content) = trimmed.strip_prefix("\x60\x60\x60json") {
        return content.trim_end_matches('\x60').trim().to_owned();
    }
    if let Some(content) = trimmed.strip_prefix("\x60\x60\x60") {
        return content.trim_end_matches('\x60').trim().to_owned();
    }
    trimmed.to_owned()
}

fn extract_json_object(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    (start < end).then_some(&text[start..=end])
}

fn normalize_labels(labels: Vec<String>) -> Vec<String> {
    let mut normalized = Vec::new();
    for label in labels {
        let label = label.trim().to_ascii_lowercase();
        if !label.is_empty() && !normalized.contains(&label) {
            normalized.push(label);
        }
    }
    normalized
}

fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::AiBudgetGate;

    fn read_stub_request(stream: &mut std::net::TcpStream) -> (String, Vec<String>, Value) {
        use std::io::Read;

        let mut request = Vec::new();
        let mut buffer = [0u8; 8_192];
        let (header_end, content_length) = loop {
            let read = stream.read(&mut buffer).expect("stub request should read");
            assert!(read > 0, "stub request closed before its headers arrived");
            request.extend_from_slice(&buffer[..read]);
            let Some(header_end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") else {
                continue;
            };
            let header_text = String::from_utf8_lossy(&request[..header_end]);
            let content_length = header_text
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                })
                .unwrap_or_default();
            break (header_end + 4, content_length);
        };

        while request.len() < header_end + content_length {
            let read = stream.read(&mut buffer).expect("stub body should read");
            assert!(read > 0, "stub request closed before its body arrived");
            request.extend_from_slice(&buffer[..read]);
        }

        let header_text = String::from_utf8_lossy(&request[..header_end]);
        let mut header_lines = header_text.lines();
        let request_line = header_lines.next().unwrap_or_default().to_owned();
        let headers = header_lines.map(str::to_owned).collect::<Vec<_>>();
        let body = if content_length == 0 {
            Value::Null
        } else {
            serde_json::from_slice(&request[header_end..header_end + content_length])
                .expect("stub request body should be JSON")
        };
        (request_line, headers, body)
    }

    fn read_raw_stub_request(stream: &mut std::net::TcpStream) -> (String, Vec<String>, Vec<u8>) {
        use std::io::Read;

        let mut request = Vec::new();
        let mut buffer = [0u8; 8_192];
        let (header_end, content_length) = loop {
            let read = stream.read(&mut buffer).expect("stub request should read");
            assert!(read > 0, "stub request closed before its headers arrived");
            request.extend_from_slice(&buffer[..read]);
            let Some(header_end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") else {
                continue;
            };
            let header_text = String::from_utf8_lossy(&request[..header_end]);
            let content_length = header_text
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                })
                .unwrap_or_default();
            break (header_end + 4, content_length);
        };
        while request.len() < header_end + content_length {
            let read = stream.read(&mut buffer).expect("stub body should read");
            assert!(read > 0, "stub request closed before its body arrived");
            request.extend_from_slice(&buffer[..read]);
        }
        let header_text = String::from_utf8_lossy(&request[..header_end]);
        let mut header_lines = header_text.lines();
        let request_line = header_lines.next().unwrap_or_default().to_owned();
        let headers = header_lines.map(str::to_owned).collect::<Vec<_>>();
        (
            request_line,
            headers,
            request[header_end..header_end + content_length].to_vec(),
        )
    }

    fn stub_header<'a>(headers: &'a [String], name: &str) -> Option<&'a str> {
        headers.iter().find_map(|line| {
            let (header_name, value) = line.split_once(':')?;
            header_name
                .eq_ignore_ascii_case(name)
                .then_some(value.trim())
        })
    }

    fn write_stub_response(stream: &mut std::net::TcpStream, body: &Value) {
        let body = serde_json::to_string(body).expect("stub response should serialize");
        write_stub_response_with_status(stream, 200, "OK", body.as_bytes());
    }

    fn write_stub_response_with_status(
        stream: &mut std::net::TcpStream,
        status: u16,
        reason: &str,
        body: &[u8],
    ) {
        use std::io::Write;

        let response = format!(
            "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream
            .write_all(response.as_bytes())
            .and_then(|_| stream.write_all(body))
            .expect("stub response should write");
    }

    fn spawn_single_json_stub(body: Value) -> (String, std::thread::JoinHandle<()>) {
        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").expect("stub should bind locally");
        let address = listener.local_addr().expect("stub should have an address");
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("stub should accept a request");
            let _ = read_stub_request(&mut stream);
            write_stub_response(&mut stream, &body);
        });
        (format!("http://{address}"), handle)
    }

    fn spawn_counting_vision_stub(
        body: Value,
    ) -> (
        String,
        std::sync::Arc<std::sync::atomic::AtomicUsize>,
        std::sync::Arc<std::sync::atomic::AtomicBool>,
        std::thread::JoinHandle<()>,
    ) {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").expect("stub should bind locally");
        listener
            .set_nonblocking(true)
            .expect("stub should support nonblocking accepts");
        let address = listener.local_addr().expect("stub should have an address");
        let requests = std::sync::Arc::new(AtomicUsize::new(0));
        let stopped = std::sync::Arc::new(AtomicBool::new(false));
        let request_count = requests.clone();
        let stop_signal = stopped.clone();
        let handle = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            while !stop_signal.load(Ordering::SeqCst) && std::time::Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream
                            .set_nonblocking(false)
                            .expect("stub stream should support blocking reads");
                        request_count.fetch_add(1, Ordering::SeqCst);
                        let _ = read_stub_request(&mut stream);
                        write_stub_response(&mut stream, &body);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    Err(_) => break,
                }
            }
        });
        (format!("http://{address}"), requests, stopped, handle)
    }

    fn spawn_benchmark_pipeline_stub(
        delay: Duration,
    ) -> (
        String,
        std::sync::Arc<std::sync::atomic::AtomicUsize>,
        std::sync::Arc<std::sync::atomic::AtomicBool>,
        std::thread::JoinHandle<()>,
    ) {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").expect("benchmark stub should bind");
        listener
            .set_nonblocking(true)
            .expect("benchmark stub should support nonblocking accepts");
        let address = listener
            .local_addr()
            .expect("benchmark stub should have an address");
        let requests = std::sync::Arc::new(AtomicUsize::new(0));
        let stopped = std::sync::Arc::new(AtomicBool::new(false));
        let request_count = requests.clone();
        let stop_signal = stopped.clone();
        let handle = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(60);
            while !stop_signal.load(Ordering::SeqCst) && std::time::Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream
                            .set_nonblocking(false)
                            .expect("benchmark stub stream should support blocking reads");
                        request_count.fetch_add(1, Ordering::SeqCst);
                        let (request_line, _headers, raw_body) = read_raw_stub_request(&mut stream);
                        let body = if request_line.contains("/audio/transcriptions") {
                            Value::Null
                        } else {
                            serde_json::from_slice(&raw_body)
                                .expect("benchmark JSON request body should parse")
                        };
                        std::thread::sleep(delay);
                        if request_line.contains("/responses") {
                            let frame_count = body
                                .pointer("/input/0/content")
                                .and_then(Value::as_array)
                                .map(|items| {
                                    items
                                        .iter()
                                        .filter(|item| {
                                            item.get("type").and_then(Value::as_str)
                                                == Some("input_image")
                                        })
                                        .count()
                                })
                                .unwrap_or(1);
                            write_stub_response(
                                &mut stream,
                                &vision_stub_response(frame_count.max(1)),
                            );
                        } else if request_line.contains("/embeddings") {
                            let count = body
                                .get("input")
                                .and_then(Value::as_array)
                                .map(Vec::len)
                                .unwrap_or(1);
                            let data = (0..count)
                                .map(|index| {
                                    json!({
                                        "index": index,
                                        "embedding": [0.5, 0.25, 0.125]
                                    })
                                })
                                .collect::<Vec<_>>();
                            write_stub_response(&mut stream, &json!({"data": data}));
                        } else if request_line.contains("/audio/transcriptions") {
                            write_stub_response(
                                &mut stream,
                                &json!({"duration": 1.0, "segments": []}),
                            );
                        } else {
                            write_stub_response(&mut stream, &json!({}));
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    Err(error) => panic!("benchmark stub accept failed: {error}"),
                }
            }
        });
        (format!("http://{address}"), requests, stopped, handle)
    }

    fn create_benchmark_video(
        path: &Path,
        ffmpeg: &Path,
        duration_seconds: u64,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        let video_source = format!("color=c=black:s={width}x{height}:r=1");
        let audio_source = "sine=frequency=880:sample_rate=16000";
        let duration = duration_seconds.to_string();
        let output = Command::new(ffmpeg)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                &video_source,
                "-f",
                "lavfi",
                "-i",
                audio_source,
                "-t",
                &duration,
                "-map",
                "0:v:0",
                "-map",
                "1:a:0",
                "-c:v",
                "mpeg4",
                "-q:v",
                "5",
                "-c:a",
                "aac",
                "-ar",
                "16000",
                "-ac",
                "1",
                "-shortest",
                "-pix_fmt",
                "yuv420p",
                "-y",
            ])
            .arg(path)
            .output()
            .map_err(|error| format!("benchmark FFmpeg could not start: {error}"))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(format!(
                "benchmark FFmpeg failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    }

    fn vision_stub_response(frame_count: usize) -> Value {
        let frames = (0..frame_count)
            .map(|index| {
                json!({
                    "description": format!("Stub frame {index}"),
                    "labels": ["stub"],
                    "visible_text": [],
                    "entities": [],
                    "actions": ["testing"],
                    "dialogue": [],
                    "setting": "test scene",
                    "situation": "testing",
                    "confidence": 0.9
                })
            })
            .collect::<Vec<_>>();
        let text = serde_json::to_string(&json!({"frames": frames}))
            .expect("stub response text should serialize");
        json!({
            "output": [{
                "content": [{"type": "output_text", "text": text}]
            }]
        })
    }

    #[derive(Clone, Copy)]
    enum BenchmarkCheckpointMode {
        All,
        FirstBatchOnly,
    }

    #[derive(Clone, Copy)]
    enum BenchmarkPersistMode {
        PreserveCheckpoints,
        Complete,
    }

    fn run_benchmark_case(
        path: &Path,
        metadata: &MediaMetadata,
        content_id: &str,
        storage_content_id: &str,
        storage: &std::rc::Rc<std::cell::RefCell<crate::local_index::SqliteIndex>>,
        settings: &AiSettings,
        recorder: &crate::diagnostics::AiDiagnosticsRecorder,
        checkpoint_mode: BenchmarkCheckpointMode,
        persist_mode: BenchmarkPersistMode,
    ) -> Result<AiFileAnalysisStatus, String> {
        let file_diagnostics = recorder.start_file(content_id.to_owned());
        let started = std::time::Instant::now();
        let task_settings = settings.clone().with_diagnostics(file_diagnostics.clone());
        let fingerprint = task_settings.analysis_settings_fingerprint();
        let model_namespace = task_settings.model_namespace();

        let load_storage = storage.clone();
        let load_content_id = storage_content_id.to_owned();
        let load_fingerprint = fingerprint.clone();
        let save_storage = storage.clone();
        let save_content_id = storage_content_id.to_owned();
        let save_fingerprint = fingerprint.clone();
        let analysis = match analyze_file_with_progress_and_cancel_with_checkpoints(
            path,
            Some(metadata),
            &task_settings,
            |_| {},
            || false,
            move |frame_timestamps, batch_size| {
                let checkpoints = load_storage
                    .borrow()
                    .load_ai_vision_checkpoints(
                        &load_content_id,
                        &load_fingerprint,
                        AI_VISION_CHECKPOINT_VERSION,
                        frame_timestamps,
                        batch_size,
                    )
                    .map_err(|error| error.to_string())?;
                Ok(match checkpoint_mode {
                    BenchmarkCheckpointMode::All => checkpoints,
                    BenchmarkCheckpointMode::FirstBatchOnly => checkpoints
                        .into_iter()
                        .filter(|checkpoint| checkpoint.batch_index == 0)
                        .collect(),
                })
            },
            move |checkpoint| {
                save_storage
                    .borrow_mut()
                    .store_ai_vision_checkpoint(
                        &save_content_id,
                        &save_fingerprint,
                        AI_VISION_CHECKPOINT_VERSION,
                        &checkpoint,
                    )
                    .map_err(|error| error.to_string())
            },
        ) {
            Ok(analysis) => analysis,
            Err(error) => {
                let status = if error == AI_ANALYSIS_CANCELLED_MESSAGE {
                    "cancelled"
                } else {
                    "failed"
                };
                file_diagnostics.finish(status, started.elapsed());
                return Err(error);
            }
        };

        let commit_started = std::time::Instant::now();
        let commit_result = match persist_mode {
            BenchmarkPersistMode::PreserveCheckpoints => {
                let mut failed_result = analysis.clone();
                failed_result.annotations.clear();
                failed_result.status = AiFileAnalysisStatus::Failed;
                failed_result.warning =
                    Some("MI-05 benchmark simulated downstream failure".to_owned());
                storage
                    .borrow_mut()
                    .record_ai_analysis_result(
                        storage_content_id,
                        &model_namespace,
                        &fingerprint,
                        None,
                        &failed_result,
                    )
                    .map(|_| ())
            }
            BenchmarkPersistMode::Complete => storage
                .borrow_mut()
                .record_ai_analysis_result(
                    storage_content_id,
                    &model_namespace,
                    &fingerprint,
                    None,
                    &analysis,
                )
                .map(|_| ()),
        };
        file_diagnostics.record_stage(
            "final_sqlite_commit",
            commit_started.elapsed(),
            if commit_result.is_ok() {
                crate::diagnostics::StageOutcome::Succeeded
            } else {
                crate::diagnostics::StageOutcome::Failed
            },
        );
        if let Err(error) = commit_result {
            file_diagnostics.finish("failed", started.elapsed());
            return Err(error.to_string());
        }

        let status = match persist_mode {
            BenchmarkPersistMode::PreserveCheckpoints => "failed",
            BenchmarkPersistMode::Complete => analysis.status.as_str(),
        };
        file_diagnostics.finish(status, started.elapsed());
        Ok(analysis.status)
    }

    fn benchmark_metadata(duration_ms: u64, width: u32, height: u32) -> MediaMetadata {
        MediaMetadata {
            duration_ms: Some(duration_ms),
            size_bytes: None,
            container: Some("mp4".to_owned()),
            video_codec: Some("mpeg4".to_owned()),
            audio_codec: Some("aac".to_owned()),
            width: Some(width),
            height: Some(height),
            frame_rate: Some("1/1".to_owned()),
            start_time: None,
            creation_time: None,
        }
    }

    fn checkpoint_test_settings(base_url: String, max_frames: u64) -> AiSettings {
        AiSettings::from_request(Some(AiRequestConfig {
            provider: Some(AiProvider::OpenAI),
            api_key: Some("checkpoint-test-key".to_owned()),
            base_url: Some(base_url),
            sample_interval_seconds: Some(1),
            max_frames: Some(max_frames),
            ..Default::default()
        }))
        .expect("checkpoint test settings should be valid")
    }

    fn checkpoint_test_frames(count: usize) -> Vec<(u64, Vec<u8>)> {
        (0..count)
            .map(|index| (index as u64 * 1_000, vec![0xff, 0xd8, 0xff, 0xd9]))
            .collect()
    }

    fn checkpoint_test_database(path: &Path) -> crate::local_index::SqliteIndex {
        let mut index = crate::local_index::SqliteIndex::open(path)
            .expect("checkpoint test database should open");
        index
            .reconcile(
                &crate::scanner::ScanReport {
                    files: vec![crate::scanner::DiscoveredFile {
                        path: "/library/checkpoint.mp4".to_owned(),
                        size_bytes: 1,
                        modified_unix_ms: Some(1),
                        content_hash: "hash-checkpoint".to_owned(),
                    }],
                    warnings: Vec::new(),
                },
                &std::collections::HashMap::new(),
            )
            .expect("checkpoint test media asset should persist");
        index
    }

    fn spawn_timeout_stub() -> (
        String,
        std::sync::Arc<std::sync::atomic::AtomicUsize>,
        std::thread::JoinHandle<()>,
    ) {
        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").expect("stub should bind locally");
        let address = listener.local_addr().expect("stub should have an address");
        let requests = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let request_count = requests.clone();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("stub should accept a request");
            request_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let _ = read_stub_request(&mut stream);
            std::thread::sleep(Duration::from_millis(250));
        });
        (format!("http://{address}"), requests, handle)
    }

    fn spawn_truncated_response_stub() -> (String, std::thread::JoinHandle<()>) {
        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").expect("stub should bind locally");
        let address = listener.local_addr().expect("stub should have an address");
        let handle = std::thread::spawn(move || {
            use std::io::Write;

            let (mut stream, _) = listener.accept().expect("stub should accept a request");
            let _ = read_stub_request(&mut stream);
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 64\r\nConnection: close\r\n\r\n{}",
                )
                .expect("stub response should write");
            let _ = stream.shutdown(std::net::Shutdown::Both);
        });
        (format!("http://{address}"), handle)
    }

    fn spawn_parallel_retry_budget_stub() -> (
        String,
        std::sync::Arc<std::sync::atomic::AtomicUsize>,
        std::thread::JoinHandle<()>,
    ) {
        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").expect("stub should bind locally");
        listener
            .set_nonblocking(true)
            .expect("stub should support nonblocking accepts");
        let address = listener.local_addr().expect("stub should have an address");
        let requests = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let request_count = requests.clone();
        let handle = std::thread::spawn(move || {
            let started = std::time::Instant::now();
            let mut last_accept = started;
            let mut workers = Vec::new();
            while started.elapsed() < Duration::from_secs(4)
                && (request_count.load(std::sync::atomic::Ordering::SeqCst) < 3
                    || last_accept.elapsed() < Duration::from_secs(2))
            {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let sequence =
                            request_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                        last_accept = std::time::Instant::now();
                        workers.push(std::thread::spawn(move || {
                            let _ = read_stub_request(&mut stream);
                            if sequence == 1 {
                                write_stub_response_with_status(
                                    &mut stream,
                                    500,
                                    "Internal Server Error",
                                    br#"{"error":{"message":"temporary failure"}}"#,
                                );
                            } else {
                                write_stub_response_with_status(&mut stream, 200, "OK", b"{}");
                            }
                        }));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("stub accept failed: {error}"),
                }
            }
            for worker in workers {
                worker.join().expect("stub worker should finish");
            }
        });
        (format!("http://{address}"), requests, handle)
    }

    fn spawn_cancel_on_retry_stub(
        cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
        worker_done: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> (
        String,
        std::sync::Arc<std::sync::atomic::AtomicUsize>,
        std::sync::mpsc::Receiver<()>,
        std::thread::JoinHandle<()>,
    ) {
        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").expect("stub should bind locally");
        let address = listener.local_addr().expect("stub should have an address");
        let requests = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let request_count = requests.clone();
        let (response_sent, response_sent_rx) = std::sync::mpsc::channel();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("stub should accept a request");
            request_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let _ = read_stub_request(&mut stream);
            write_stub_response_with_status(
                &mut stream,
                500,
                "Internal Server Error",
                br#"{"error":{"message":"temporary failure"}}"#,
            );
            cancelled.store(true, std::sync::atomic::Ordering::SeqCst);
            response_sent
                .send(())
                .expect("test should receive cancellation signal");

            listener
                .set_nonblocking(true)
                .expect("stub should support nonblocking accepts");
            let started = std::time::Instant::now();
            let mut done_since = None;
            while started.elapsed() < Duration::from_secs(3) {
                if worker_done.load(std::sync::atomic::Ordering::SeqCst) {
                    done_since.get_or_insert_with(std::time::Instant::now);
                    if done_since
                        .as_ref()
                        .is_some_and(|finished| finished.elapsed() >= Duration::from_millis(300))
                    {
                        break;
                    }
                }
                match listener.accept() {
                    Ok((mut retry_stream, _)) => {
                        request_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        let _ = read_stub_request(&mut retry_stream);
                        write_stub_response(&mut retry_stream, &json!({}));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("stub accept failed: {error}"),
                }
            }
        });
        (
            format!("http://{address}"),
            requests,
            response_sent_rx,
            handle,
        )
    }

    fn spawn_cancel_on_retry_multipart_stub(
        cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
        worker_done: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> (
        String,
        std::sync::Arc<std::sync::atomic::AtomicUsize>,
        std::sync::mpsc::Receiver<()>,
        std::thread::JoinHandle<()>,
    ) {
        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").expect("stub should bind locally");
        let address = listener.local_addr().expect("stub should have an address");
        let requests = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let request_count = requests.clone();
        let (response_sent, response_sent_rx) = std::sync::mpsc::channel();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("stub should accept a request");
            request_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let _ = read_raw_stub_request(&mut stream);
            write_stub_response_with_status(
                &mut stream,
                500,
                "Internal Server Error",
                br#"{"error":{"message":"temporary failure"}}"#,
            );
            cancelled.store(true, std::sync::atomic::Ordering::SeqCst);
            response_sent
                .send(())
                .expect("test should receive cancellation signal");

            listener
                .set_nonblocking(true)
                .expect("stub should support nonblocking accepts");
            let started = std::time::Instant::now();
            while started.elapsed() < Duration::from_secs(3)
                && !worker_done.load(std::sync::atomic::Ordering::SeqCst)
            {
                match listener.accept() {
                    Ok((mut retry_stream, _)) => {
                        request_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        let _ = read_raw_stub_request(&mut retry_stream);
                        write_stub_response(&mut retry_stream, &json!({}));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("stub accept failed: {error}"),
                }
            }
        });
        (
            format!("http://{address}"),
            requests,
            response_sent_rx,
            handle,
        )
    }

    fn spawn_gemini_embedding_stop_stub(
        cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> (
        String,
        std::sync::Arc<std::sync::atomic::AtomicUsize>,
        std::thread::JoinHandle<()>,
    ) {
        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").expect("stub should bind locally");
        let address = listener.local_addr().expect("stub should have an address");
        let requests = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let request_count = requests.clone();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("stub should accept a request");
            request_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let _ = read_stub_request(&mut stream);
            write_stub_response(
                &mut stream,
                &json!({"embedding": {"values": [0.5, 0.25, 0.125]}}),
            );
            cancelled.store(true, std::sync::atomic::Ordering::SeqCst);
            listener
                .set_nonblocking(true)
                .expect("stub should support nonblocking accepts");
            let deadline = std::time::Instant::now() + Duration::from_millis(500);
            while std::time::Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut retry_stream, _)) => {
                        request_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        let _ = read_stub_request(&mut retry_stream);
                        write_stub_response(
                            &mut retry_stream,
                            &json!({"embedding": {"values": [0.5, 0.25, 0.125]}}),
                        );
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("stub accept failed: {error}"),
                }
            }
        });
        (format!("http://{address}"), requests, handle)
    }

    fn spawn_openai_stub() -> (String, std::thread::JoinHandle<()>) {
        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").expect("stub should bind locally");
        let address = listener.local_addr().expect("stub should have an address");
        let handle = std::thread::spawn(move || loop {
            let (mut stream, _) = listener.accept().expect("stub should accept a request");
            let (request_line, headers, raw_body) = read_raw_stub_request(&mut stream);
            if request_line.starts_with("POST /audio/transcriptions ") {
                assert_eq!(
                    stub_header(&headers, "authorization"),
                    Some("Bearer stub-api-key")
                );
                let body = String::from_utf8_lossy(&raw_body);
                assert!(body.contains("name=\"timestamp_granularities[]\""));
                assert!(body.contains("filename=\"speech.mp3\""));
                write_stub_response(
                    &mut stream,
                    &json!({
                        "segments": [
                            {"start": 0.2, "end": 0.8, "text": "enemy eliminated"}
                        ]
                    }),
                );
                continue;
            }
            let request: Value = if raw_body.is_empty() {
                Value::Null
            } else {
                serde_json::from_slice(&raw_body).expect("stub request body should be JSON")
            };
            if request_line.starts_with("POST /responses ") {
                let image_count = request
                    .pointer("/input/0/content")
                    .and_then(Value::as_array)
                    .expect("vision request should contain content")
                    .iter()
                    .filter(|item| item.get("type").and_then(Value::as_str) == Some("input_image"))
                    .count();
                assert!(image_count > 0, "vision request should contain images");
                assert!(
                    image_count <= REMOTE_VISION_BATCH_SIZE,
                    "cloud vision request should stay within the upload bound"
                );
                assert_eq!(request.get("store"), Some(&Value::Bool(false)));
                assert_eq!(
                    request.pointer("/text/format/type").and_then(Value::as_str),
                    Some("json_schema")
                );
                assert_eq!(
                    request
                        .pointer("/text/format/schema/properties/frames/maxItems")
                        .and_then(Value::as_u64),
                    Some(image_count as u64)
                );
                let required = request
                    .pointer("/text/format/schema/properties/frames/items/required")
                    .and_then(Value::as_array)
                    .expect("vision schema should require searchable context fields");
                for field in ["entities", "actions", "setting", "situation"] {
                    assert!(required.iter().any(|value| value.as_str() == Some(field)));
                }
                let analyses = (0..image_count)
                    .map(|_| {
                        json!({
                            "description": "A Fortnite player eliminates an opponent",
                            "labels": ["fortnite", "kill", "elimination"],
                            "visible_text": ["ELIMINATED"],
                            "entities": ["Fortnite player"],
                            "actions": ["eliminating opponent"],
                            "dialogue": [],
                            "setting": "forest battlefield",
                            "situation": "battle royale fight",
                            "confidence": 0.98
                        })
                    })
                    .collect::<Vec<_>>();
                write_stub_response(
                    &mut stream,
                    &json!({
                        "status": "completed",
                        "error": null,
                        "output": [{
                            "content": [{
                                "type": "output_text",
                                "text": serde_json::to_string(&json!({"frames": analyses})).unwrap()
                            }]
                        }]
                    }),
                );
            } else if request_line.starts_with("POST /embeddings ") {
                let input_count = request
                    .get("input")
                    .and_then(Value::as_array)
                    .expect("embedding request should contain input")
                    .len();
                let data = (0..input_count)
                    .map(|index| json!({"index": index, "embedding": [1.0, 0.0, 0.5]}))
                    .collect::<Vec<_>>();
                write_stub_response(&mut stream, &json!({"data": data}));
                break;
            } else {
                panic!("unexpected stub request: {request_line}");
            }
        });
        (format!("http://{address}"), handle)
    }

    fn spawn_openai_connection_stub() -> (String, std::thread::JoinHandle<()>) {
        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").expect("stub should bind locally");
        let address = listener.local_addr().expect("stub should have an address");
        let handle = std::thread::spawn(move || {
            let (mut model_stream, _) = listener.accept().expect("model check should connect");
            let (request_line, headers, body) = read_stub_request(&mut model_stream);
            assert!(request_line.starts_with("GET /models/gpt-5.6-luna "));
            assert!(!request_line.contains("?key="));
            assert_eq!(
                stub_header(&headers, "authorization"),
                Some("Bearer openai-test-key")
            );
            assert_eq!(body, Value::Null);
            write_stub_response(&mut model_stream, &json!({"id": "gpt-5.6-luna"}));

            let (mut embedding_stream, _) =
                listener.accept().expect("embedding check should connect");
            let (request_line, headers, body) = read_stub_request(&mut embedding_stream);
            assert!(request_line.starts_with("POST /embeddings "));
            assert_eq!(
                stub_header(&headers, "authorization"),
                Some("Bearer openai-test-key")
            );
            assert_eq!(
                body.get("model").and_then(Value::as_str),
                Some("text-embedding-3-small")
            );
            assert_eq!(
                body.get("input").and_then(Value::as_array).map(Vec::len),
                Some(1)
            );
            write_stub_response(
                &mut embedding_stream,
                &json!({"data": [{"index": 0, "embedding": [0.5, 0.25, 0.125]}]}),
            );
        });
        (format!("http://{address}"), handle)
    }

    fn spawn_gemini_connection_stub() -> (String, std::thread::JoinHandle<()>) {
        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").expect("stub should bind locally");
        let address = listener.local_addr().expect("stub should have an address");
        let handle = std::thread::spawn(move || {
            let (mut model_stream, _) = listener.accept().expect("model check should connect");
            let (request_line, headers, body) = read_stub_request(&mut model_stream);
            assert!(request_line.starts_with("GET /models/gemini-3.8-flash "));
            assert!(!request_line.contains("?key="));
            assert_eq!(
                stub_header(&headers, "x-goog-api-key"),
                Some("gemini-test-key")
            );
            assert_eq!(body, Value::Null);
            write_stub_response(
                &mut model_stream,
                &json!({"name": "models/gemini-3.8-flash"}),
            );

            let (mut embedding_stream, _) =
                listener.accept().expect("embedding check should connect");
            let (request_line, headers, body) = read_stub_request(&mut embedding_stream);
            assert!(request_line.starts_with("POST /models/gemini-embedding-2:embedContent "));
            assert!(!request_line.contains("?key="));
            assert_eq!(
                stub_header(&headers, "x-goog-api-key"),
                Some("gemini-test-key")
            );
            assert_eq!(
                body.get("model").and_then(Value::as_str),
                Some("models/gemini-embedding-2")
            );
            assert_eq!(
                body.get("output_dimensionality").and_then(Value::as_u64),
                Some(768)
            );
            assert!(body.get("taskType").is_none());
            assert_eq!(
                body.pointer("/content/parts/0/text")
                    .and_then(Value::as_str),
                Some("task: search result | query: MediaIndex connection test")
            );
            write_stub_response(
                &mut embedding_stream,
                &json!({"embedding": {"values": [0.75, 0.5, 0.25]}}),
            );
        });
        (format!("http://{address}"), handle)
    }

    fn spawn_gemini_oauth_connection_stub() -> (String, std::thread::JoinHandle<()>) {
        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").expect("stub should bind locally");
        let address = listener.local_addr().expect("stub should have an address");
        let handle = std::thread::spawn(move || {
            for expected_path in [
                "GET /models/gemini-3.8-flash ",
                "POST /models/gemini-embedding-2:embedContent ",
            ] {
                let (mut stream, _) = listener.accept().expect("OAuth check should connect");
                let (request_line, headers, _body) = read_stub_request(&mut stream);
                assert!(request_line.starts_with(expected_path));
                assert_eq!(
                    stub_header(&headers, "authorization"),
                    Some("Bearer google-oauth-token")
                );
                assert_eq!(
                    stub_header(&headers, "x-goog-user-project"),
                    Some("mediaindex-oauth-test")
                );
                assert!(stub_header(&headers, "x-goog-api-key").is_none());
                if expected_path.starts_with("GET") {
                    write_stub_response(&mut stream, &json!({"name": "models/gemini-3.8-flash"}));
                } else {
                    write_stub_response(
                        &mut stream,
                        &json!({"embedding": {"values": [0.75, 0.5, 0.25]}}),
                    );
                }
            }
        });
        (format!("http://{address}"), handle)
    }

    #[test]
    fn extracts_output_text_from_responses_payload() {
        let body = json!({
            "output": [{
                "content": [{"type": "output_text", "text": "{\"description\":\"A player wins\"}"}]
            }]
        });

        assert_eq!(
            response_text(&body).as_deref(),
            Some("{\"description\":\"A player wins\"}")
        );
    }

    #[test]
    fn accepts_successful_responses_payload_with_null_error() {
        let body = json!({
            "status": "completed",
            "error": null,
            "output": [{
                "content": [{"type": "output_text", "text": "[{\"description\":\"Eliminated opponent\"}]"}]
            }]
        });

        assert!(!response_body_reports_error(&body));
        assert!(response_text(&body).is_some());
    }

    #[test]
    fn successful_json_without_usage_keeps_the_entire_request_reserve() {
        let (base_url, server) = spawn_single_json_stub(json!({"status": "completed"}));
        let recorder = AiUsageRecorder::new("run-test", "openai", "known", "2026-09-05")
            .with_budget_gate(AiBudgetGate::new(1.0).expect("valid budget should create a gate"));
        let client = Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .expect("test client should build");
        let response = execute_with_retry(
            "OpenAI vision",
            "gpt-5.6-luna",
            Some(&recorder),
            Some(0.6),
            || client.get(&base_url).send(),
        )
        .expect("successful stub response should be returned");
        let body = read_json_response(
            response,
            "OpenAI vision",
            "gpt-5.6-luna",
            Some(&recorder),
            None,
        )
        .expect("successful JSON should parse");
        server.join().expect("stub should finish");

        assert_eq!(
            body.get("status").and_then(Value::as_str),
            Some("completed")
        );
        assert!((recorder.reserved_budget_usd(Some(1.0)).unwrap() - 0.6).abs() < 0.000_001);
        let event = recorder
            .drain()
            .pop()
            .expect("usage event should be available");
        assert_eq!(event.usage_status, "not_reported");
        assert_eq!(event.calculated_cost_usd, None);
    }

    #[test]
    fn api_error_without_usage_keeps_the_request_reserve_unknown() {
        let (base_url, server) =
            spawn_single_json_stub(json!({"error": {"message": "quota exceeded"}}));
        let recorder = AiUsageRecorder::new("run-test", "openai", "known", "2026-09-05")
            .with_budget_gate(AiBudgetGate::new(1.0).expect("valid budget should create a gate"));
        let client = Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .expect("test client should build");
        let response = execute_with_retry(
            "OpenAI vision",
            "gpt-5.6-luna",
            Some(&recorder),
            Some(0.6),
            || client.get(&base_url).send(),
        )
        .expect("HTTP response should be returned for body parsing");
        let error = read_json_response(
            response,
            "OpenAI vision",
            "gpt-5.6-luna",
            Some(&recorder),
            None,
        )
        .expect_err("API error body should fail the request");
        server.join().expect("stub should finish");

        assert!(error.contains("quota exceeded"));
        assert!((recorder.reserved_budget_usd(Some(1.0)).unwrap() - 0.6).abs() < 0.000_001);
        let event = recorder
            .drain()
            .pop()
            .expect("usage event should be available");
        assert_eq!(event.usage_status, "not_reported");
        assert_eq!(event.outcome, "response_received");
    }

    #[test]
    fn timeout_keeps_the_request_reserve_as_unknown() {
        let (base_url, requests, server) = spawn_timeout_stub();
        let recorder = AiUsageRecorder::new("run-test", "openai", "known", "2026-09-05")
            .with_budget_gate(AiBudgetGate::new(1.0).expect("valid budget should create a gate"));
        let client = Client::builder()
            .connect_timeout(Duration::from_millis(100))
            .timeout(Duration::from_millis(50))
            .build()
            .expect("test client should build");
        let result = execute_with_retry(
            "OpenAI vision",
            "gpt-5.6-luna",
            Some(&recorder),
            Some(0.6),
            || client.get(&base_url).send(),
        );
        server.join().expect("timeout stub should finish");

        assert!(result.is_err());
        assert_eq!(requests.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!((recorder.reserved_budget_usd(Some(1.0)).unwrap() - 0.6).abs() < 0.000_001);
        let event = recorder
            .drain()
            .into_iter()
            .find(|event| event.outcome != "budget_blocked")
            .expect("timeout usage event should be available");
        assert!(matches!(
            event.outcome.as_str(),
            "retryable_transport_error" | "transport_error"
        ));
        assert_eq!(event.usage_status, "not_reported");
    }

    #[test]
    fn unreadable_response_keeps_the_request_reserve_as_unknown() {
        let (base_url, server) = spawn_truncated_response_stub();
        let recorder = AiUsageRecorder::new("run-test", "openai", "known", "2026-09-05")
            .with_budget_gate(AiBudgetGate::new(1.0).expect("valid budget should create a gate"));
        let client = Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .expect("test client should build");
        let response = execute_with_retry(
            "OpenAI vision",
            "gpt-5.6-luna",
            Some(&recorder),
            Some(0.6),
            || client.get(&base_url).send(),
        )
        .expect("HTTP response should be returned for body parsing");
        let error = read_json_response(
            response,
            "OpenAI vision",
            "gpt-5.6-luna",
            Some(&recorder),
            None,
        )
        .expect_err("truncated response should be unreadable");
        server.join().expect("truncated stub should finish");

        assert!(error.contains("unreadable response"));
        assert!((recorder.reserved_budget_usd(Some(1.0)).unwrap() - 0.6).abs() < 0.000_001);
        let event = recorder
            .drain()
            .pop()
            .expect("usage event should be available");
        assert_eq!(event.usage_status, "not_reported");
        assert_eq!(event.outcome, "response_received");
    }

    #[test]
    fn parallel_requests_and_retry_do_not_send_a_request_over_the_budget() {
        let (base_url, requests, server) = spawn_parallel_retry_budget_stub();
        let recorder = AiUsageRecorder::new("run-test", "openai", "known", "2026-09-05")
            .with_budget_gate(AiBudgetGate::new(1.0).expect("valid budget should create a gate"));
        let client = Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .expect("test client should build");
        let workers = (0..2)
            .map(|_| {
                let client = client.clone();
                let base_url = base_url.clone();
                let recorder = recorder.clone();
                std::thread::spawn(move || {
                    let result = execute_with_retry(
                        "OpenAI vision",
                        "gpt-5.6-luna",
                        Some(&recorder),
                        Some(0.5),
                        || client.get(&base_url).send(),
                    );
                    match result {
                        Ok(response) => {
                            if let Some(usage_event) = response.usage_event.as_ref() {
                                usage_event.finish();
                            }
                            Ok(())
                        }
                        Err(error) => Err(error),
                    }
                })
            })
            .collect::<Vec<_>>();
        let results = workers
            .into_iter()
            .map(|worker| worker.join().expect("request worker should finish"))
            .collect::<Vec<Result<(), String>>>();
        server.join().expect("parallel stub should finish");

        assert_eq!(requests.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(results.iter().filter(|result| result.is_err()).count(), 1);
        assert!(
            (recorder.reserved_budget_usd(Some(1.0)).unwrap() - 1.0).abs() < 0.000_001,
            "both unknown requests must remain committed"
        );
    }

    #[test]
    fn cancellation_before_first_request_sends_no_request_or_usage_event() {
        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").expect("stub should bind locally");
        listener
            .set_nonblocking(true)
            .expect("stub should support nonblocking accepts");
        let address = listener.local_addr().expect("stub should have an address");
        let recorder = AiUsageRecorder::new("run-test", "openai", "known", "2026-09-05")
            .with_budget_gate(AiBudgetGate::new(1.0).expect("valid budget should create a gate"));
        let client = Client::builder()
            .connect_timeout(Duration::from_millis(100))
            .timeout(Duration::from_millis(100))
            .build()
            .expect("test client should build");
        let is_cancelled = || true;
        let result = execute_with_retry_cancellable(
            "OpenAI vision",
            "gpt-5.6-luna",
            Some(&recorder),
            Some(0.6),
            &is_cancelled,
            || client.get(format!("http://{address}")).send(),
        );

        assert!(matches!(
            result,
            Err(error) if error == AI_ANALYSIS_CANCELLED_MESSAGE
        ));
        assert_eq!(recorder.drain().len(), 0);
        assert_eq!(recorder.reserved_budget_usd(Some(1.0)), Some(0.0));
        std::thread::sleep(Duration::from_millis(25));
        assert!(matches!(
            listener.accept(),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
        ));
    }

    #[test]
    fn cancellation_after_reservation_releases_it_without_a_paid_attempt_event() {
        let recorder = AiUsageRecorder::new("run-test", "openai", "known", "2026-09-05")
            .with_budget_gate(AiBudgetGate::new(1.0).expect("valid budget should create a gate"));
        let checks = std::sync::atomic::AtomicUsize::new(0);
        let sent = std::sync::atomic::AtomicUsize::new(0);
        let is_cancelled = || checks.fetch_add(1, std::sync::atomic::Ordering::SeqCst) > 0;
        let result = execute_with_retry_cancellable(
            "OpenAI vision",
            "gpt-5.6-luna",
            Some(&recorder),
            Some(0.6),
            &is_cancelled,
            || {
                sent.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                panic!("cancelled request must not be sent");
            },
        );

        assert!(matches!(
            result,
            Err(error) if error == AI_ANALYSIS_CANCELLED_MESSAGE
        ));
        assert_eq!(sent.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(recorder.reserved_budget_usd(Some(1.0)), Some(0.0));
        assert!(recorder.drain().is_empty());
    }

    #[test]
    fn cancellation_during_retry_wait_skips_the_next_request_quickly() {
        let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker_done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (base_url, requests, response_sent, server) =
            spawn_cancel_on_retry_stub(cancelled.clone(), worker_done.clone());
        let recorder = AiUsageRecorder::new("run-test", "openai", "known", "2026-09-05")
            .with_budget_gate(AiBudgetGate::new(1.0).expect("valid budget should create a gate"));
        let client = Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .expect("test client should build");
        let worker_recorder = recorder.clone();
        let worker_cancelled = cancelled.clone();
        let worker_done_flag = worker_done.clone();
        let worker = std::thread::spawn(move || {
            let is_cancelled = || worker_cancelled.load(std::sync::atomic::Ordering::SeqCst);
            let result = execute_with_retry_cancellable(
                "OpenAI vision",
                "gpt-5.6-luna",
                Some(&worker_recorder),
                Some(0.5),
                &is_cancelled,
                || client.get(&base_url).send(),
            );
            worker_done_flag.store(true, std::sync::atomic::Ordering::SeqCst);
            result.map(|response| {
                if let Some(usage_event) = response.usage_event.as_ref() {
                    usage_event.finish();
                }
            })
        });

        response_sent
            .recv_timeout(Duration::from_secs(1))
            .expect("stub should signal after the first response");
        let cancellation_observed_at = std::time::Instant::now();
        let result = worker.join().expect("retry worker should finish");
        assert!(cancellation_observed_at.elapsed() < Duration::from_millis(500));
        server.join().expect("retry stub should finish");

        assert!(matches!(
            result,
            Err(error) if error == AI_ANALYSIS_CANCELLED_MESSAGE
        ));
        assert_eq!(requests.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!((recorder.reserved_budget_usd(Some(1.0)).unwrap() - 0.5).abs() < 0.000_001);
        let event = recorder
            .drain()
            .pop()
            .expect("sent request event should be available");
        assert_eq!(event.outcome, "retryable_http_error");
        assert_eq!(event.usage_status, "not_reported");
    }

    #[test]
    fn gemini_embedding_loop_stops_before_the_next_request_after_cancellation() {
        let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (base_url, requests, server) = spawn_gemini_embedding_stop_stub(cancelled.clone());
        let settings = AiSettings::from_request(Some(AiRequestConfig {
            provider: Some(AiProvider::Gemini),
            api_key: Some("gemini-test-key".to_owned()),
            base_url: Some(base_url),
            ..Default::default()
        }))
        .expect("Gemini settings should be valid");
        let client = Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .expect("test client should build");
        let texts = vec![
            "first annotation".to_owned(),
            "second annotation".to_owned(),
        ];
        let is_cancelled = || cancelled.load(std::sync::atomic::Ordering::SeqCst);
        let result = create_embeddings(
            &client,
            &texts,
            &settings,
            EmbeddingKind::Document,
            &is_cancelled,
        );
        server.join().expect("Gemini embedding stub should finish");

        assert!(matches!(
            result,
            Err(error) if error == AI_ANALYSIS_CANCELLED_MESSAGE
        ));
        assert_eq!(requests.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn detects_non_null_api_errors_and_failed_statuses() {
        assert!(response_body_reports_error(&json!({
            "error": {"message": "quota exceeded"}
        })));
        assert!(response_body_reports_error(&json!({
            "error": null,
            "status": "FAILED"
        })));
    }

    #[test]
    fn extracts_local_and_gemini_response_text() {
        let local = json!({"message": {"content": "local text"}});
        let gemini = json!({"candidates": [{"content": {"parts": [{"text": "gemini text"}]}}]});
        assert_eq!(response_text(&local).as_deref(), Some("local text"));
        assert_eq!(response_text(&gemini).as_deref(), Some("gemini text"));
    }

    #[test]
    fn preserves_string_and_code_api_errors() {
        assert_eq!(
            api_error_detail(&json!({"error": "model not found"})),
            "model not found"
        );
        assert_eq!(
            api_error_detail(&json!({"error": {"code": "quota", "message": "No credits"}})),
            "No credits · quota"
        );
    }

    #[test]
    fn normalizes_duplicate_labels() {
        assert_eq!(
            normalize_labels(vec![
                " Kill ".to_owned(),
                "kill".to_owned(),
                "Fortnite".to_owned()
            ]),
            vec!["kill", "fortnite"]
        );
    }

    #[test]
    fn parses_visible_screen_text_for_search() {
        let parsed = parse_frame_analyses(
            r#"{"description":"A masked hero swings between buildings","labels":["Superhero"],"visible_text":["NEW YORK"],"entities":["Spider-Man"],"actions":["Web swinging"],"setting":"New York skyline","situation":"Superhero chase","confidence":0.9}"#,
        )
        .expect("frame analysis should parse")
        .into_iter()
        .next()
        .expect("one frame should be returned");

        assert_eq!(parsed.visible_text, vec!["new york"]);
        assert_eq!(parsed.entities, vec!["spider-man"]);
        assert_eq!(parsed.actions, vec!["web swinging"]);
        assert_eq!(parsed.setting.as_deref(), Some("new york skyline"));
        assert_eq!(parsed.situation.as_deref(), Some("superhero chase"));
    }

    #[test]
    fn attaches_timestamped_spoken_segments_to_the_matching_visual_frames() {
        let frame = |description: &str| FrameAnalysis {
            description: description.to_owned(),
            labels: Vec::new(),
            visible_text: Vec::new(),
            entities: Vec::new(),
            actions: Vec::new(),
            dialogue: Vec::new(),
            setting: None,
            situation: None,
            confidence: Some(0.9),
        };
        let mut analyses = vec![
            (0, frame("Opening frame")),
            (5_000, frame("Middle frame")),
            (10_000, frame("Later frame")),
        ];
        attach_transcript_segments(
            &mut analyses,
            &[
                TranscriptSegment {
                    start: 1.0,
                    end: 2.0,
                    text: "We should follow the trail".to_owned(),
                },
                TranscriptSegment {
                    start: 6.0,
                    end: 7.0,
                    text: "There is someone ahead".to_owned(),
                },
            ],
        );

        assert_eq!(analyses[0].1.dialogue, vec!["We should follow the trail"]);
        assert_eq!(analyses[1].1.dialogue, vec!["There is someone ahead"]);
        assert!(analyses[2].1.dialogue.is_empty());
    }

    #[test]
    fn accepts_a_valid_audio_duration_and_rejects_zero_or_invalid_values() {
        assert_eq!(parse_audio_duration("60.25\n"), Some(60.25));
        assert_eq!(parse_audio_duration("0\n"), None);
        assert_eq!(parse_audio_duration("N/A\n"), None);
    }

    #[test]
    fn prefers_service_reported_audio_duration_over_local_estimate() {
        let body = json!({"duration": 42.5});

        assert_eq!(
            reported_audio_seconds(&body, "OpenAI speech transcription"),
            Some(42.5)
        );
        assert_eq!(
            reported_audio_seconds(&json!({}), "OpenAI speech transcription"),
            None
        );
    }

    #[test]
    fn sends_timestamped_openai_transcription_as_multipart_audio() {
        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").expect("stub should bind locally");
        let address = listener.local_addr().expect("stub should have an address");
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("stub should accept a request");
            let (request_line, headers, body) = read_raw_stub_request(&mut stream);
            assert!(request_line.starts_with("POST /audio/transcriptions "));
            assert_eq!(
                stub_header(&headers, "authorization"),
                Some("Bearer openai-test-key")
            );
            let content_type = stub_header(&headers, "content-type").unwrap_or_default();
            assert!(content_type.starts_with("multipart/form-data; boundary="));
            let body = String::from_utf8_lossy(&body);
            assert!(body.contains("name=\"model\""));
            assert!(body.contains("whisper-1"));
            assert!(body.contains("name=\"response_format\""));
            assert!(body.contains("verbose_json"));
            assert!(body.contains("name=\"timestamp_granularities[]\""));
            assert!(body.contains("segment"));
            assert!(body.contains("filename=\"speech.mp3\""));
            write_stub_response(
                &mut stream,
                &json!({
                    "segments": [
                        {"start": 1.25, "end": 2.75, "text": "Follow the trail"}
                    ]
                }),
            );
        });
        let audio_path = std::env::temp_dir().join("mediaindex-transcription-test-speech.mp3");
        fs::write(&audio_path, b"fake mp3 bytes").expect("fixture audio should write");
        let settings = AiSettings::from_request(Some(AiRequestConfig {
            provider: Some(AiProvider::OpenAI),
            api_key: Some("openai-test-key".to_owned()),
            base_url: Some(format!("http://{address}")),
            transcribe_audio: Some(true),
            ..Default::default()
        }))
        .expect("OpenAI settings should be valid");
        let client = build_http_client().expect("HTTP client should build");

        let transcript = transcribe_openai_audio(
            &client,
            &ExtractedAudio {
                path: audio_path.clone(),
                duration_seconds: 60.0,
            },
            &settings,
            &never_cancelled,
        )
        .expect("transcription should parse");
        let _ = fs::remove_file(&audio_path);
        server.join().expect("stub should finish cleanly");

        assert_eq!(transcript.len(), 1);
        assert_eq!(transcript[0].text, "Follow the trail");
        assert_eq!(
            settings.model_namespace(),
            "openai:gpt-5.6-luna:text-embedding-3-small:speech-whisper-1"
        );
    }

    #[test]
    fn speech_retry_cancellation_returns_cancelled_and_keeps_sent_usage() {
        let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker_done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (base_url, requests, response_sent, server) =
            spawn_cancel_on_retry_multipart_stub(cancelled.clone(), worker_done.clone());
        let audio_path = std::env::temp_dir().join(format!(
            "mediaindex-cancel-transcription-{}.mp3",
            std::process::id()
        ));
        fs::write(&audio_path, b"fake mp3 bytes").expect("fixture audio should write");
        let recorder = AiUsageRecorder::new("run-cancel", "openai", "known", "2026-09-05");
        let settings = AiSettings::from_request(Some(AiRequestConfig {
            provider: Some(AiProvider::OpenAI),
            api_key: Some("openai-test-key".to_owned()),
            base_url: Some(base_url),
            transcribe_audio: Some(true),
            ..Default::default()
        }))
        .expect("OpenAI settings should be valid")
        .with_usage_recorder(recorder.clone());
        let client = Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .expect("test client should build");

        let result = transcribe_openai_audio(
            &client,
            &ExtractedAudio {
                path: audio_path.clone(),
                duration_seconds: 12.0,
            },
            &settings,
            &|| cancelled.load(std::sync::atomic::Ordering::SeqCst),
        );
        response_sent
            .recv_timeout(Duration::from_secs(1))
            .expect("stub should observe the first request before cancellation");
        worker_done.store(true, std::sync::atomic::Ordering::SeqCst);
        server.join().expect("stub should finish cleanly");
        let _ = fs::remove_file(&audio_path);

        let error = result.expect_err("cancellation should stop the speech retry");
        assert_eq!(error, AI_ANALYSIS_CANCELLED_MESSAGE);
        assert_eq!(requests.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(
            transcription_failure_result(error, Path::new("cancelled-video.mp4"), 4, 4, Vec::new(),),
            Err(AI_ANALYSIS_CANCELLED_MESSAGE.to_owned())
        );
        let event = recorder
            .drain()
            .pop()
            .expect("sent speech request usage should be retained");
        assert_eq!(event.operation, "OpenAI speech transcription");
        assert_eq!(event.outcome, "retryable_http_error");
        assert_eq!(event.status_code, Some(500));
    }

    #[test]
    fn builds_local_settings_without_an_api_key() {
        let settings = AiSettings::from_request(Some(AiRequestConfig {
            provider: Some(AiProvider::Local),
            ..Default::default()
        }))
        .expect("local settings should not need an API key");

        assert_eq!(settings.provider, AiProvider::Local);
        assert_eq!(settings.vision_model, DEFAULT_LOCAL_VISION_MODEL);
        assert_eq!(settings.embedding_model, DEFAULT_LOCAL_EMBEDDING_MODEL);
        assert_eq!(
            settings.model_namespace(),
            "local:gemma4:e2b:embeddinggemma"
        );
    }

    #[test]
    fn uses_supported_gemini_defaults() {
        let settings = AiSettings::from_request(Some(AiRequestConfig {
            provider: Some(AiProvider::Gemini),
            api_key: Some("test-key".to_owned()),
            ..Default::default()
        }))
        .expect("Gemini settings should be valid");

        assert_eq!(settings.vision_model, "gemini-3.8-flash");
        assert_eq!(settings.embedding_model, "gemini-embedding-2");
    }

    #[test]
    fn validates_openai_vision_and_embedding_connection_contract() {
        let (base_url, server) = spawn_openai_connection_stub();
        let settings = AiSettings::from_request(Some(AiRequestConfig {
            provider: Some(AiProvider::OpenAI),
            api_key: Some("openai-test-key".to_owned()),
            base_url: Some(base_url),
            ..Default::default()
        }))
        .expect("OpenAI settings should be valid");

        let report = test_connection(&settings).expect("OpenAI connection should validate");
        server.join().expect("OpenAI stub should finish");
        assert_eq!(report.provider, "openai");
        assert_eq!(report.vision_model, "gpt-5.6-luna");
        assert_eq!(report.embedding_model, "text-embedding-3-small");
        assert_eq!(report.embedding_dimensions, 3);
    }

    #[test]
    fn validates_gemini_auth_header_and_embedding_2_contract() {
        let (base_url, server) = spawn_gemini_connection_stub();
        let settings = AiSettings::from_request(Some(AiRequestConfig {
            provider: Some(AiProvider::Gemini),
            api_key: Some("gemini-test-key".to_owned()),
            base_url: Some(base_url),
            ..Default::default()
        }))
        .expect("Gemini settings should be valid");

        let report = test_connection(&settings).expect("Gemini connection should validate");
        server.join().expect("Gemini stub should finish");
        assert_eq!(report.provider, "gemini");
        assert_eq!(report.vision_model, "gemini-3.8-flash");
        assert_eq!(report.embedding_model, "gemini-embedding-2");
        assert_eq!(report.embedding_dimensions, 3);
    }

    #[test]
    fn validates_gemini_oauth_bearer_and_quota_project_contract() {
        let (base_url, server) = spawn_gemini_oauth_connection_stub();
        let settings = AiSettings::from_request(Some(AiRequestConfig {
            provider: Some(AiProvider::Gemini),
            auth_mode: Some("oauth".to_owned()),
            api_key: Some("google-oauth-token".to_owned()),
            google_project_id: Some("mediaindex-oauth-test".to_owned()),
            base_url: Some(base_url),
            ..Default::default()
        }))
        .expect("Gemini OAuth settings should be valid");

        let report = test_connection(&settings).expect("Gemini OAuth should validate");
        server.join().expect("Gemini OAuth stub should finish");
        assert_eq!(report.provider, "gemini");
        assert_eq!(report.embedding_dimensions, 3);
    }

    #[test]
    fn api_errors_name_provider_specific_recovery() {
        let gemini = api_failure_message(
            "Gemini vision model check",
            reqwest::StatusCode::FORBIDDEN,
            "permission denied",
        );
        assert!(gemini.contains("auth key"));
        assert!(gemini.contains("September 2026"));

        let local = api_failure_message(
            "Local AI vision model check",
            reqwest::StatusCode::NOT_FOUND,
            "model not found",
        );
        assert!(local.contains("ollama pull <model>"));
    }

    #[test]
    fn cancellation_stops_before_frame_extraction_or_network_work() {
        let settings = AiSettings::from_request(Some(AiRequestConfig {
            provider: Some(AiProvider::Local),
            ..Default::default()
        }))
        .expect("local settings should be valid");

        let error = analyze_file_with_progress_and_cancel(
            Path::new("missing-video.mp4"),
            None,
            &settings,
            |_| {},
            || true,
        )
        .expect_err("cancellation should stop before the missing path is read");

        assert_eq!(error, AI_ANALYSIS_CANCELLED_MESSAGE);
    }

    #[test]
    fn uses_current_fast_openai_vision_defaults_without_hidden_reasoning() {
        let settings = AiSettings::from_request(Some(AiRequestConfig {
            provider: Some(AiProvider::OpenAI),
            api_key: Some("test-key".to_owned()),
            ..Default::default()
        }))
        .expect("OpenAI settings should be valid");

        assert_eq!(settings.vision_model, "gpt-5.6-luna");
        assert_eq!(
            openai_reasoning_effort(&settings.vision_model),
            Some("none")
        );
        assert_eq!(openai_reasoning_effort("gpt-5.6-terra"), Some("low"));
        assert_eq!(openai_reasoning_effort("gpt-4.1-mini"), None);
    }

    #[test]
    fn corrects_the_common_zero_four_mini_model_typo() {
        let settings = AiSettings::from_request(Some(AiRequestConfig {
            provider: Some(AiProvider::OpenAI),
            api_key: Some("test-key".to_owned()),
            vision_model: Some("04-mini".to_owned()),
            ..Default::default()
        }))
        .expect("OpenAI settings should be valid");

        assert_eq!(settings.vision_model, "o4-mini");
    }

    #[test]
    fn request_provider_and_models_override_environment_defaults() {
        let request: AiRequestConfig = serde_json::from_value(json!({
            "provider": "openai",
            "apiKey": "test-key",
            "visionModel": "vision-test",
            "embeddingModel": "embedding-test",
            "baseUrl": "https://example.test/v1",
            "contextHint": "Animated series; possible character: Nova"
        }))
        .expect("frontend configuration should deserialize");
        let settings = AiSettings::from_request(Some(request)).expect("request should win");

        assert_eq!(settings.provider, AiProvider::OpenAI);
        assert_eq!(settings.vision_model, "vision-test");
        assert_eq!(settings.embedding_model, "embedding-test");
        assert_eq!(settings.base_url, "https://example.test/v1");
        assert_eq!(
            settings.context_hint.as_deref(),
            Some("Animated series; possible character: Nova")
        );
        assert_eq!(settings.parallel_file_limit(), 2);
        assert_eq!(settings.vision_batch_size(), 8);
        assert_eq!(
            settings.model_namespace(),
            "openai:vision-test:embedding-test"
        );
    }

    #[test]
    fn analysis_fingerprint_changes_with_coverage_settings() {
        let settings =
            |sample_interval_seconds: u64, max_frames: u64, context_hint: Option<&str>| {
                AiSettings::from_request(Some(AiRequestConfig {
                    provider: Some(AiProvider::OpenAI),
                    api_key: Some("test-key".to_owned()),
                    sample_interval_seconds: Some(sample_interval_seconds),
                    max_frames: Some(max_frames),
                    context_hint: context_hint.map(str::to_owned),
                    ..Default::default()
                }))
                .expect("fingerprint fixture should be valid")
            };

        let base = settings(5, 120, None);
        assert_eq!(
            base.analysis_settings_fingerprint(),
            settings(5, 120, None).analysis_settings_fingerprint()
        );
        assert_ne!(
            base.analysis_settings_fingerprint(),
            settings(10, 120, None).analysis_settings_fingerprint()
        );
        assert_ne!(
            base.analysis_settings_fingerprint(),
            settings(5, 60, None).analysis_settings_fingerprint()
        );
        assert_ne!(
            base.analysis_settings_fingerprint(),
            settings(5, 120, Some("sports footage")).analysis_settings_fingerprint()
        );
    }

    #[test]
    fn committed_vision_checkpoints_resume_after_sqlite_restart_without_repeating_requests() {
        use std::cell::RefCell;
        use std::rc::Rc;
        use std::sync::atomic::{AtomicBool, Ordering};

        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("test clock should be after epoch")
            .as_nanos();
        let database_path = std::env::temp_dir().join(format!(
            "mediaindex-mi04-checkpoints-{}-{stamp}.sqlite3",
            std::process::id()
        ));
        drop(checkpoint_test_database(&database_path));

        let frames = checkpoint_test_frames(16);
        let (first_base_url, first_requests, first_stop, first_server) =
            spawn_counting_vision_stub(vision_stub_response(8));
        let first_settings = checkpoint_test_settings(first_base_url, 16);
        let first_fingerprint = first_settings.analysis_settings_fingerprint();
        let first_client = build_http_client().expect("first test client should build");
        let first_cancelled = std::sync::Arc::new(AtomicBool::new(false));
        let first_cancel_signal = first_cancelled.clone();
        let first_storage = Rc::new(RefCell::new(
            crate::local_index::SqliteIndex::open(&database_path)
                .expect("first checkpoint database should reopen"),
        ));
        let first_store = first_storage.clone();
        let first_checkpoint_fingerprint = first_fingerprint.clone();
        let first_result = analyze_vision_batches(
            Path::new("/library/checkpoint.mp4"),
            &first_client,
            &frames,
            &first_settings,
            80,
            &|_| {},
            &|| first_cancelled.load(Ordering::SeqCst),
            |_, _| Ok(Vec::new()),
            move |checkpoint| {
                let result = first_store
                    .borrow_mut()
                    .store_ai_vision_checkpoint(
                        "hash-checkpoint",
                        &first_checkpoint_fingerprint,
                        AI_VISION_CHECKPOINT_VERSION,
                        &checkpoint,
                    )
                    .map_err(|error| error.to_string());
                if result.is_ok() {
                    first_cancel_signal.store(true, Ordering::SeqCst);
                }
                result
            },
        );
        first_stop.store(true, Ordering::SeqCst);
        first_server
            .join()
            .expect("first vision stub should finish");
        assert!(matches!(
            first_result,
            Err(error) if error == AI_ANALYSIS_CANCELLED_MESSAGE
        ));
        assert_eq!(first_requests.load(Ordering::SeqCst), 1);
        let first_summary = first_storage
            .borrow()
            .ai_vision_checkpoint_summary(
                "hash-checkpoint",
                &first_fingerprint,
                AI_VISION_CHECKPOINT_VERSION,
                &frames
                    .iter()
                    .map(|(timestamp, _)| *timestamp)
                    .collect::<Vec<_>>(),
                first_settings.vision_batch_size(),
            )
            .expect("first checkpoint should be queryable");
        assert_eq!(first_summary.reusable_frame_count, 8);
        assert_eq!(first_summary.reusable_batch_count, 1);
        drop(first_storage);

        let (second_base_url, second_requests, second_stop, second_server) =
            spawn_counting_vision_stub(vision_stub_response(8));
        let second_settings = checkpoint_test_settings(second_base_url, 16);
        let second_client = build_http_client().expect("second test client should build");
        let second_storage = Rc::new(RefCell::new(
            crate::local_index::SqliteIndex::open(&database_path)
                .expect("second checkpoint database should reopen"),
        ));
        let second_load = second_storage.clone();
        let second_store = second_storage.clone();
        let second_fingerprint = second_settings.analysis_settings_fingerprint();
        let second_load_fingerprint = second_fingerprint.clone();
        let second_store_fingerprint = second_fingerprint.clone();
        let second_result = analyze_vision_batches(
            Path::new("/library/checkpoint.mp4"),
            &second_client,
            &frames,
            &second_settings,
            80,
            &|_| {},
            &|| false,
            move |frame_timestamps, batch_size| {
                second_load
                    .borrow()
                    .load_ai_vision_checkpoints(
                        "hash-checkpoint",
                        &second_load_fingerprint,
                        AI_VISION_CHECKPOINT_VERSION,
                        frame_timestamps,
                        batch_size,
                    )
                    .map_err(|error| error.to_string())
            },
            move |checkpoint| {
                second_store
                    .borrow_mut()
                    .store_ai_vision_checkpoint(
                        "hash-checkpoint",
                        &second_store_fingerprint,
                        AI_VISION_CHECKPOINT_VERSION,
                        &checkpoint,
                    )
                    .map_err(|error| error.to_string())
            },
        )
        .expect("resume should complete both vision batches");
        second_stop.store(true, Ordering::SeqCst);
        second_server
            .join()
            .expect("second vision stub should finish");
        assert_eq!(second_result.0.len(), 16);
        assert!(second_result.1.is_empty());
        assert_eq!(second_requests.load(Ordering::SeqCst), 1);
        second_storage
            .borrow_mut()
            .record_ai_analysis_result(
                "hash-checkpoint",
                &second_settings.model_namespace(),
                &second_fingerprint,
                None,
                &AiFileAnalysisResult {
                    annotations: Vec::new(),
                    planned_frame_count: 16,
                    successful_frame_count: 16,
                    failed_batches: Vec::new(),
                    status: AiFileAnalysisStatus::Failed,
                    warning: Some("simulated embedding failure".to_owned()),
                },
            )
            .expect("downstream failure should preserve the checkpoints");
        drop(second_storage);

        let (third_base_url, third_requests, third_stop, third_server) =
            spawn_counting_vision_stub(vision_stub_response(8));
        let third_settings = checkpoint_test_settings(third_base_url, 16);
        let third_client = build_http_client().expect("third test client should build");
        let third_storage = Rc::new(RefCell::new(
            crate::local_index::SqliteIndex::open(&database_path)
                .expect("third checkpoint database should reopen"),
        ));
        let third_load = third_storage.clone();
        let third_fingerprint = third_settings.analysis_settings_fingerprint();
        let third_result = analyze_vision_batches(
            Path::new("/library/checkpoint.mp4"),
            &third_client,
            &frames,
            &third_settings,
            80,
            &|_| {},
            &|| false,
            move |frame_timestamps, batch_size| {
                third_load
                    .borrow()
                    .load_ai_vision_checkpoints(
                        "hash-checkpoint",
                        &third_fingerprint,
                        AI_VISION_CHECKPOINT_VERSION,
                        frame_timestamps,
                        batch_size,
                    )
                    .map_err(|error| error.to_string())
            },
            |_checkpoint| Ok(()),
        )
        .expect("all committed vision work should be reusable");
        third_stop.store(true, Ordering::SeqCst);
        third_server
            .join()
            .expect("third vision stub should finish");
        assert_eq!(third_result.0.len(), 16);
        assert!(third_result.1.is_empty());
        assert_eq!(
            third_requests.load(Ordering::SeqCst),
            0,
            "a retry after downstream work fails must not repeat completed vision"
        );

        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn two_frame_checkpoint_with_missing_metadata_resumes_after_restart_without_new_vision_request()
    {
        use std::cell::RefCell;
        use std::rc::Rc;
        use std::sync::atomic::Ordering;

        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("test clock should be after epoch")
            .as_nanos();
        let database_path = std::env::temp_dir().join(format!(
            "mediaindex-mi04r-metadata-free-{}-{stamp}.sqlite3",
            std::process::id()
        ));
        drop(checkpoint_test_database(&database_path));

        let frames = checkpoint_test_frames(2);
        let (first_base_url, first_requests, first_stop, first_server) =
            spawn_counting_vision_stub(vision_stub_response(2));
        let first_settings = checkpoint_test_settings(first_base_url, 60);
        let first_fingerprint = first_settings.analysis_settings_fingerprint();
        let first_client = build_http_client().expect("first metadata-free client should build");
        let first_storage = Rc::new(RefCell::new(
            crate::local_index::SqliteIndex::open(&database_path)
                .expect("first metadata-free database should reopen"),
        ));
        let first_store = first_storage.clone();
        let first_checkpoint_fingerprint = first_fingerprint.clone();
        let first_result = analyze_vision_batches(
            Path::new("/library/checkpoint.mp4"),
            &first_client,
            &frames,
            &first_settings,
            80,
            &|_| {},
            &|| false,
            |_, _| Ok(Vec::new()),
            move |checkpoint| {
                first_store
                    .borrow_mut()
                    .store_ai_vision_checkpoint(
                        "hash-checkpoint",
                        &first_checkpoint_fingerprint,
                        AI_VISION_CHECKPOINT_VERSION,
                        &checkpoint,
                    )
                    .map_err(|error| error.to_string())
            },
        )
        .expect("the first two-frame vision run should complete");
        first_stop.store(true, Ordering::SeqCst);
        first_server
            .join()
            .expect("first metadata-free vision stub should finish");
        assert_eq!(first_result.0.len(), 2);
        assert!(first_result.1.is_empty());
        assert_eq!(first_requests.load(Ordering::SeqCst), 1);

        let first_summary = first_storage
            .borrow()
            .ai_vision_checkpoint_summary_for_any_plan(
                "hash-checkpoint",
                &first_fingerprint,
                AI_VISION_CHECKPOINT_VERSION,
                first_settings.vision_batch_size(),
            )
            .expect("the actual two-frame plan should be discoverable");
        assert_eq!(first_summary.frame_count, 2);
        assert_eq!(first_summary.reusable_frame_count, 2);
        assert_eq!(first_summary.reusable_batch_count, 1);
        assert_eq!(
            first_summary.frame_timestamps.as_deref(),
            Some([0, 1_000].as_slice())
        );
        first_storage
            .borrow_mut()
            .record_ai_analysis_result(
                "hash-checkpoint",
                &first_settings.model_namespace(),
                &first_fingerprint,
                None,
                &AiFileAnalysisResult {
                    annotations: Vec::new(),
                    planned_frame_count: 2,
                    successful_frame_count: 2,
                    failed_batches: Vec::new(),
                    status: AiFileAnalysisStatus::Failed,
                    warning: Some("simulated downstream failure".to_owned()),
                },
            )
            .expect("downstream failure should preserve the vision checkpoint");
        drop(first_storage);

        let (second_base_url, second_requests, second_stop, second_server) =
            spawn_counting_vision_stub(vision_stub_response(2));
        let second_settings = checkpoint_test_settings(second_base_url, 60);
        assert_eq!(
            second_settings.analysis_settings_fingerprint(),
            first_fingerprint,
            "the provider endpoint must not change checkpoint identity"
        );
        let second_client = build_http_client().expect("second metadata-free client should build");
        let second_storage = Rc::new(RefCell::new(
            crate::local_index::SqliteIndex::open(&database_path)
                .expect("second metadata-free database should reopen"),
        ));
        let second_load = second_storage.clone();
        let second_load_fingerprint = second_settings.analysis_settings_fingerprint();
        let second_result = analyze_vision_batches(
            Path::new("/library/checkpoint.mp4"),
            &second_client,
            &frames,
            &second_settings,
            80,
            &|_| {},
            &|| false,
            move |frame_timestamps, batch_size| {
                second_load
                    .borrow()
                    .load_ai_vision_checkpoints(
                        "hash-checkpoint",
                        &second_load_fingerprint,
                        AI_VISION_CHECKPOINT_VERSION,
                        frame_timestamps,
                        batch_size,
                    )
                    .map_err(|error| error.to_string())
            },
            |_checkpoint| Ok(()),
        )
        .expect("the reopened checkpoint should cover both actual frames");
        second_stop.store(true, Ordering::SeqCst);
        second_server
            .join()
            .expect("second metadata-free vision stub should finish");
        assert_eq!(second_result.0.len(), 2);
        assert!(second_result.1.is_empty());
        assert_eq!(
            second_requests.load(Ordering::SeqCst),
            0,
            "a metadata-free restart must reuse the saved actual frame plan"
        );

        let _ = fs::remove_file(database_path);
    }

    #[test]
    fn vision_checkpoint_write_error_stops_before_the_next_paid_batch() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let (base_url, requests, stop, server) =
            spawn_counting_vision_stub(vision_stub_response(8));
        let settings = checkpoint_test_settings(base_url, 16);
        let client = build_http_client().expect("checkpoint error test client should build");
        let writes = AtomicUsize::new(0);
        let result = analyze_vision_batches(
            Path::new("/library/checkpoint-error.mp4"),
            &client,
            &checkpoint_test_frames(16),
            &settings,
            80,
            &|_| {},
            &|| false,
            |_, _| Ok(Vec::new()),
            |_checkpoint| {
                writes.fetch_add(1, Ordering::SeqCst);
                Err("simulated SQLite write failure".to_owned())
            },
        );
        stop.store(true, Ordering::SeqCst);
        server.join().expect("checkpoint error stub should finish");

        assert!(matches!(
            result,
            Err(error) if error.contains("cannot persist vision checkpoint")
        ));
        assert_eq!(writes.load(Ordering::SeqCst), 1);
        assert_eq!(
            requests.load(Ordering::SeqCst),
            1,
            "the second paid vision batch must wait for the failed write ACK"
        );
    }

    #[test]
    #[ignore = "benchmark: requires FFmpeg and is intentionally manual"]
    fn mi05_benchmark_real_ffmpeg_with_delayed_local_stub() {
        let ffmpeg = resolve_ffmpeg_executable(None);
        let ffmpeg_check = Command::new(&ffmpeg).arg("-version").output();
        let Ok(ffmpeg_check) = ffmpeg_check else {
            println!("MI05 benchmark skipped: FFmpeg is unavailable");
            return;
        };
        if !ffmpeg_check.status.success() {
            println!("MI05 benchmark skipped: FFmpeg returned a failed version check");
            return;
        }
        let ffmpeg_version = String::from_utf8_lossy(&ffmpeg_check.stdout)
            .lines()
            .next()
            .unwrap_or("unknown")
            .to_owned();
        println!("MI05_BENCHMARK_ENV version={ffmpeg_version}");

        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("benchmark clock should be after epoch")
            .as_nanos();
        let benchmark_root = std::env::temp_dir().join(format!(
            "mediaindex-mi05-benchmark-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&benchmark_root).expect("benchmark directory should be creatable");
        let short_video = benchmark_root.join("short.mp4");
        let long_video = benchmark_root.join("long.mp4");
        create_benchmark_video(&short_video, &ffmpeg, 3, 320, 180)
            .expect("short benchmark video should be created");
        create_benchmark_video(&long_video, &ffmpeg, 12, 640, 360)
            .expect("long benchmark video should be created");

        let short_metadata = benchmark_metadata(3_000, 320, 180);
        let long_metadata = benchmark_metadata(12_000, 640, 360);
        let mut index = crate::local_index::SqliteIndex::open_in_memory()
            .expect("benchmark SQLite index should open");
        index
            .reconcile(
                &crate::scanner::ScanReport {
                    files: vec![
                        crate::scanner::DiscoveredFile {
                            path: short_video.to_string_lossy().into_owned(),
                            size_bytes: fs::metadata(&short_video)
                                .expect("short benchmark metadata should be readable")
                                .len(),
                            modified_unix_ms: None,
                            content_hash: "benchmark-short".to_owned(),
                        },
                        crate::scanner::DiscoveredFile {
                            path: long_video.to_string_lossy().into_owned(),
                            size_bytes: fs::metadata(&long_video)
                                .expect("long benchmark metadata should be readable")
                                .len(),
                            modified_unix_ms: None,
                            content_hash: "benchmark-long".to_owned(),
                        },
                    ],
                    warnings: Vec::new(),
                },
                &std::collections::HashMap::new(),
            )
            .expect("benchmark videos should be indexed");
        let storage = std::rc::Rc::new(std::cell::RefCell::new(index));

        let (base_url, requests, stop_stub, stub_server) =
            spawn_benchmark_pipeline_stub(Duration::from_millis(15));
        let settings = AiSettings::from_request(Some(AiRequestConfig {
            provider: Some(AiProvider::OpenAI),
            api_key: Some("mi05-benchmark-stub-key".to_owned()),
            vision_model: Some("vision-benchmark".to_owned()),
            embedding_model: Some("embedding-benchmark".to_owned()),
            base_url: Some(base_url),
            ffmpeg_path: Some(ffmpeg.to_string_lossy().into_owned()),
            sample_interval_seconds: Some(1),
            max_frames: Some(12),
            transcribe_audio: Some(true),
            transcription_model: Some("whisper-1".to_owned()),
            ..Default::default()
        }))
        .expect("benchmark settings should be valid");
        let recorder = crate::diagnostics::AiDiagnosticsRecorder::new(
            format!("run-mi05-benchmark-{stamp}"),
            "mi05_benchmark",
            settings.provider_name(),
            settings.vision_model(),
            settings.embedding_model(),
            Some(settings.transcription_model().to_owned()),
            std::time::Instant::now(),
            crate::diagnostics::unix_time_ms(),
            8,
            1,
        );

        for (path, label, metadata) in [
            (short_video.as_path(), "short", &short_metadata),
            (long_video.as_path(), "long", &long_metadata),
        ] {
            let content_id = format!("{label}:fresh");
            let result = run_benchmark_case(
                path,
                metadata,
                &content_id,
                &format!("benchmark-{label}"),
                &storage,
                &settings,
                &recorder,
                BenchmarkCheckpointMode::All,
                BenchmarkPersistMode::PreserveCheckpoints,
            )
            .expect("fresh benchmark analysis should complete");
            assert_eq!(result, AiFileAnalysisStatus::Complete);

            let ready_diagnostics = recorder.start_file(format!("{label}:ready_repeat"));
            ready_diagnostics.finish("skipped", Duration::ZERO);

            let content_id = format!("{label}:partial_resume");
            let result = run_benchmark_case(
                path,
                metadata,
                &content_id,
                &format!("benchmark-{label}"),
                &storage,
                &settings,
                &recorder,
                BenchmarkCheckpointMode::FirstBatchOnly,
                BenchmarkPersistMode::PreserveCheckpoints,
            )
            .expect("partial benchmark analysis should complete");
            assert_eq!(result, AiFileAnalysisStatus::Complete);

            let content_id = format!("{label}:downstream_retry");
            let result = run_benchmark_case(
                path,
                metadata,
                &content_id,
                &format!("benchmark-{label}"),
                &storage,
                &settings,
                &recorder,
                BenchmarkCheckpointMode::All,
                BenchmarkPersistMode::Complete,
            )
            .expect("downstream retry benchmark analysis should complete");
            assert_eq!(result, AiFileAnalysisStatus::Complete);
        }

        let snapshot = recorder.snapshot("completed");
        assert_eq!(snapshot.files.len(), 8);
        let find_file = |content_id: &str| {
            snapshot
                .files
                .iter()
                .find(|file| file.content_id == content_id)
                .unwrap_or_else(|| panic!("benchmark file {content_id} should be present"))
        };
        let long_fresh = find_file("long:fresh");
        let long_partial = find_file("long:partial_resume");
        let long_retry = find_file("long:downstream_retry");
        assert!(long_fresh.vision_new_frames > 0);
        assert!(long_partial.vision_new_frames > 0);
        assert!(long_partial.vision_reused_frames > 0);
        assert_eq!(long_retry.vision_new_frames, 0);
        assert!(long_retry.vision_reused_frames > 0);
        assert_eq!(
            long_retry
                .stages
                .get("vision_http")
                .map(|stage| stage.calls)
                .unwrap_or_default(),
            0,
            "a downstream retry should reuse all saved vision batches"
        );

        let stage_elapsed = |file: &crate::diagnostics::AiFileDiagnostics, name: &str| {
            file.stages
                .get(name)
                .map(|stage| stage.elapsed_ms)
                .unwrap_or_default()
        };
        let stage_calls = |file: &crate::diagnostics::AiFileDiagnostics, name: &str| {
            file.stages
                .get(name)
                .map(|stage| stage.calls)
                .unwrap_or_default()
        };
        for file in &snapshot.files {
            println!(
                "MI05_BENCHMARK_FILE {}",
                json!({
                    "content_id": &file.content_id,
                    "status": &file.status,
                    "wall_ms": file.wall_time_ms,
                    "vision_new_frames": file.vision_new_frames,
                    "vision_new_batches": file.vision_new_batches,
                    "vision_reused_frames": file.vision_reused_frames,
                    "vision_reused_batches": file.vision_reused_batches,
                    "frame_extraction_ms": stage_elapsed(file, "frame_extraction"),
                    "checkpoint_lookup_ms": stage_elapsed(file, "checkpoint_lookup"),
                    "vision_http_ms": stage_elapsed(file, "vision_http"),
                    "vision_http_calls": stage_calls(file, "vision_http"),
                    "audio_extraction_ms": stage_elapsed(file, "audio_extraction"),
                    "transcription_http_ms": stage_elapsed(file, "transcription_http"),
                    "transcription_http_calls": stage_calls(file, "transcription_http"),
                    "embedding_http_ms": stage_elapsed(file, "embedding_http"),
                    "embedding_http_calls": stage_calls(file, "embedding_http"),
                    "final_sqlite_commit_ms": stage_elapsed(file, "final_sqlite_commit")
                })
            );
        }
        let diagnostics_path = recorder
            .write_json(&benchmark_root, "completed")
            .expect("benchmark diagnostics should be written");
        println!("MI05_BENCHMARK_DIAGNOSTICS {}", diagnostics_path.display());
        println!(
            "MI05_BENCHMARK_REQUESTS {}",
            requests.load(std::sync::atomic::Ordering::SeqCst)
        );

        stop_stub.store(true, std::sync::atomic::Ordering::SeqCst);
        stub_server
            .join()
            .expect("benchmark stub should stop cleanly");
        let _ = fs::remove_file(short_video);
        let _ = fs::remove_file(long_video);
    }

    #[test]
    #[ignore = "requires MEDIAINDEX_SMOKE_VIDEO and FFmpeg"]
    fn analyzes_real_video_through_openai_response_pipeline() {
        let video = std::env::var_os("MEDIAINDEX_SMOKE_VIDEO")
            .map(PathBuf::from)
            .expect("MEDIAINDEX_SMOKE_VIDEO should point to a real video");
        let (base_url, server) = spawn_openai_stub();
        let settings = AiSettings::from_request(Some(AiRequestConfig {
            provider: Some(AiProvider::OpenAI),
            api_key: Some("stub-api-key".to_owned()),
            vision_model: Some("vision-stub".to_owned()),
            embedding_model: Some("embedding-stub".to_owned()),
            base_url: Some(base_url),
            sample_interval_seconds: Some(1),
            max_frames: Some(24),
            transcribe_audio: Some(true),
            ..Default::default()
        }))
        .expect("stub settings should be valid");
        let progress = std::cell::RefCell::new(Vec::new());
        let metadata = MediaMetadata {
            duration_ms: None,
            size_bytes: None,
            container: Some("mp4".to_owned()),
            video_codec: Some("h264".to_owned()),
            audio_codec: Some("aac".to_owned()),
            width: None,
            height: None,
            frame_rate: None,
            start_time: None,
            creation_time: None,
        };

        let annotations = analyze_file_with_progress(&video, Some(&metadata), &settings, |event| {
            progress.borrow_mut().push(event.percent);
        })
        .expect("real video should complete the OpenAI response pipeline");
        server.join().expect("analysis stub should finish cleanly");
        let thumbnail = extract_thumbnail(&video, 1_000, &settings.ffmpeg_executable)
            .expect("real video should produce a local thumbnail");
        assert!(thumbnail.starts_with(&[0xff, 0xd8]));
        println!(
            "analyzed {} sampled frames and extracted a {} byte local thumbnail",
            annotations.len(),
            thumbnail.len()
        );
        assert!(!annotations.is_empty());
        assert!(annotations.len() <= 24);
        assert!(annotations.iter().all(|annotation| {
            annotation
                .description
                .contains("On-screen text: eliminated")
                && annotation
                    .labels
                    .contains(&"entity: fortnite player".to_owned())
                && annotation
                    .labels
                    .contains(&"action: eliminating opponent".to_owned())
                && annotation
                    .labels
                    .contains(&"on-screen text: eliminated".to_owned())
                && annotation.embedding == vec![1.0, 0.0, 0.5]
        }));
        assert!(annotations.iter().any(|annotation| annotation
            .labels
            .contains(&"dialogue: enemy eliminated".to_owned())));
        let progress = progress.into_inner();
        assert_eq!(progress.first(), Some(&1));
        assert_eq!(progress.last(), Some(&100));
        assert!(progress.windows(2).all(|values| values[0] <= values[1]));

        let path = video.to_string_lossy().into_owned();
        let mut index =
            crate::local_index::SqliteIndex::open_in_memory().expect("smoke index should open");
        index
            .reconcile(
                &crate::scanner::ScanReport {
                    files: vec![crate::scanner::DiscoveredFile {
                        path: path.clone(),
                        size_bytes: fs::metadata(&video)
                            .expect("smoke video metadata should be readable")
                            .len(),
                        modified_unix_ms: None,
                        content_hash: "smoke-video".to_owned(),
                    }],
                    warnings: Vec::new(),
                },
                &std::collections::HashMap::new(),
            )
            .expect("smoke video should be indexed");
        index
            .replace_ai_annotations("smoke-video", &annotations)
            .expect("AI annotations should persist");
        let (query_base_url, query_server) = spawn_openai_stub();
        let mut query_settings = settings.clone();
        query_settings.base_url = query_base_url;
        let query_embedding =
            embed_query("kill", &query_settings).expect("AI query embedding should be created");
        let results = index
            .search_ai(
                "kill",
                &query_embedding,
                10,
                Some(&settings.model_namespace()),
            )
            .expect("AI search should complete");
        query_server
            .join()
            .expect("query stub should finish cleanly");

        assert!(!results.is_empty());
        assert_eq!(results[0].path, path);
        assert!(results[0].available);
        assert!(results[0].description.contains("Fortnite player"));
        assert!(results[0].labels.contains(&"kill".to_owned()));
    }
}
