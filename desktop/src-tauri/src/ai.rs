use base64::Engine;
use reqwest::blocking::{Client, Response};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::error::Error as StdError;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::metadata::MediaMetadata;

const DEFAULT_OPENAI_VISION_MODEL: &str = "gpt-5.6-luna";
const DEFAULT_OPENAI_EMBEDDING_MODEL: &str = "text-embedding-3-small";
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
    pub cancelled: bool,
    pub warnings: Vec<AiWarning>,
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
                    "Gemini needs an API key. Add it under AI connection or set MEDIAINDEX_GEMINI_API_KEY."
                        .to_owned()
                }
                AiProvider::Local => unreachable!(),
            });
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
        })
    }

    pub fn model_namespace(&self) -> String {
        format!(
            "{}:{}:{}",
            self.provider.name(),
            self.vision_model,
            self.embedding_model
        )
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
}

pub fn resolve_ffmpeg_executable(configured_path: Option<String>) -> PathBuf {
    non_empty(configured_path)
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("MEDIAINDEX_FFMPEG_PATH").map(PathBuf::from))
        .filter(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from("ffmpeg"))
}

pub fn analyze_file(
    path: &Path,
    _metadata: Option<&MediaMetadata>,
    settings: &AiSettings,
) -> Result<Vec<AiAnnotation>, String> {
    analyze_file_with_progress(path, _metadata, settings, |_| {})
}

pub fn analyze_file_with_progress<F>(
    path: &Path,
    _metadata: Option<&MediaMetadata>,
    settings: &AiSettings,
    progress: F,
) -> Result<Vec<AiAnnotation>, String>
where
    F: Fn(AiFileProgress),
{
    analyze_file_with_progress_and_cancel(path, _metadata, settings, progress, || false)
}

pub fn analyze_file_with_progress_and_cancel<F, C>(
    path: &Path,
    _metadata: Option<&MediaMetadata>,
    settings: &AiSettings,
    progress: F,
    is_cancelled: C,
) -> Result<Vec<AiAnnotation>, String>
where
    F: Fn(AiFileProgress),
    C: Fn() -> bool,
{
    ensure_analysis_not_cancelled(&is_cancelled)?;
    let client = build_http_client()?;
    progress(AiFileProgress {
        percent: 1,
        phase: "Extracting frames",
    });
    let frames = extract_frames(path, settings)?;
    ensure_analysis_not_cancelled(&is_cancelled)?;
    progress(AiFileProgress {
        percent: 10,
        phase: "Analyzing frames",
    });
    let mut analyses = Vec::with_capacity(frames.len());
    let mut frame_errors = Vec::new();
    let mut processed_frames = 0usize;
    for frame_batch in frames.chunks(settings.vision_batch_size()) {
        ensure_analysis_not_cancelled(&is_cancelled)?;
        match describe_frames(&client, frame_batch, settings) {
            Ok(batch) if batch.len() == frame_batch.len() => {
                analyses.extend(
                    frame_batch
                        .iter()
                        .map(|(timestamp_ms, _)| *timestamp_ms)
                        .zip(batch),
                );
            }
            Ok(batch) => frame_errors.push(format!(
                "vision returned {} analyses for {} frames",
                batch.len(),
                frame_batch.len()
            )),
            Err(error) => frame_errors.push(format!(
                "{}-{} ms: {error}",
                frame_batch.first().map(|frame| frame.0).unwrap_or_default(),
                frame_batch.last().map(|frame| frame.0).unwrap_or_default()
            )),
        }
        ensure_analysis_not_cancelled(&is_cancelled)?;
        processed_frames += frame_batch.len();
        let vision_percent = 10 + ((processed_frames * 80) / frames.len()) as u8;
        progress(AiFileProgress {
            percent: vision_percent.min(90),
            phase: "Analyzing frames",
        });
    }
    if analyses.is_empty() {
        return Err(format!(
            "AI vision produced no usable frames for {}: {}",
            path.display(),
            frame_errors
                .first()
                .cloned()
                .unwrap_or_else(|| "no frame analysis was returned".to_owned())
        ));
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
    let embeddings =
        create_embeddings(&client, &embedding_texts, settings, EmbeddingKind::Document)?;
    ensure_analysis_not_cancelled(&is_cancelled)?;
    if embeddings.len() != analyses.len() {
        return Err(format!(
            "AI returned {} embeddings for {} analyzed frames",
            embeddings.len(),
            analyses.len()
        ));
    }

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
        phase: "Finished",
    });
    Ok(annotations)
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
        AiProvider::OpenAI => execute_with_retry("OpenAI vision model check", || {
            client
                .get(format!("{}/models/{clean_model}", settings.base_url))
                .bearer_auth(&settings.api_key)
                .send()
        })?,
        AiProvider::Gemini => execute_with_retry("Gemini vision model check", || {
            client
                .get(format!("{}/models/{clean_model}", settings.base_url))
                .header("x-goog-api-key", &settings.api_key)
                .send()
        })?,
        AiProvider::Local => execute_with_retry("Local AI vision model check", || {
            client
                .post(format!("{}/api/show", settings.base_url))
                .json(&json!({"model": clean_model}))
                .send()
        })?,
    };
    read_json_response(
        response,
        match settings.provider {
            AiProvider::OpenAI => "OpenAI vision model check",
            AiProvider::Gemini => "Gemini vision model check",
            AiProvider::Local => "Local AI vision model check",
        },
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

fn execute_with_retry<F>(operation_name: &str, mut make_request: F) -> Result<Response, String>
where
    F: FnMut() -> Result<Response, reqwest::Error>,
{
    const MAX_ATTEMPTS: usize = 4;
    let mut attempt = 0;
    loop {
        attempt += 1;
        match make_request() {
            Ok(response) => {
                let status = response.status();
                if is_transient_status(status) && attempt < MAX_ATTEMPTS {
                    let delay_ms = match attempt {
                        1 => 1_500,
                        2 => 3_500,
                        _ => 6_000,
                    };
                    std::thread::sleep(Duration::from_millis(delay_ms));
                    continue;
                }
                return Ok(response);
            }
            Err(error) => {
                if (error.is_timeout() || error.is_connect()) && attempt < MAX_ATTEMPTS {
                    std::thread::sleep(Duration::from_millis(1_500 * attempt as u64));
                    continue;
                }
                return Err(request_failure(operation_name, &error));
            }
        }
    }
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

#[derive(Debug, Deserialize)]
struct FrameAnalysis {
    description: String,
    #[serde(default)]
    labels: Vec<String>,
    #[serde(default)]
    visible_text: Vec<String>,
    #[serde(default)]
    entities: Vec<String>,
    #[serde(default)]
    actions: Vec<String>,
    #[serde(default)]
    dialogue: Vec<String>,
    #[serde(default)]
    setting: Option<String>,
    #[serde(default)]
    situation: Option<String>,
    confidence: Option<f32>,
}

#[derive(Clone, Copy)]
enum EmbeddingKind {
    Document,
    Query,
}

fn describe_frames(
    client: &Client,
    frames: &[(u64, Vec<u8>)],
    settings: &AiSettings,
) -> Result<Vec<FrameAnalysis>, String> {
    let encoded_frames = frames
        .iter()
        .map(|(timestamp_ms, frame)| {
            (
                *timestamp_ms,
                base64::engine::general_purpose::STANDARD.encode(frame),
            )
        })
        .collect::<Vec<_>>();
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
        AiProvider::OpenAI => describe_openai(client, &encoded_frames, &prompt, settings)?,
        AiProvider::Gemini => describe_gemini(client, &encoded_frames, &prompt, settings)?,
        AiProvider::Local => describe_local(client, &encoded_frames, &prompt, settings)?,
    };
    parse_frame_analyses(&text)
}

fn describe_openai(
    client: &Client,
    encoded_frames: &[(u64, String)],
    prompt: &str,
    settings: &AiSettings,
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
    let response = execute_with_retry("OpenAI vision", || {
        client
            .post(format!("{}/responses", settings.base_url))
            .bearer_auth(&settings.api_key)
            .json(&request)
            .send()
    })?;
    let body = read_json_response(response, "OpenAI vision")?;
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

fn describe_gemini(
    client: &Client,
    encoded_frames: &[(u64, String)],
    prompt: &str,
    settings: &AiSettings,
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
    let response = execute_with_retry("Gemini vision", || {
        client
            .post(&url)
            .header("x-goog-api-key", &settings.api_key)
            .json(&payload)
            .send()
    })?;
    let body = read_json_response(response, "Gemini vision")?;
    response_text(&body).ok_or_else(|| "Gemini vision returned no candidate text".to_owned())
}

fn describe_local(
    client: &Client,
    encoded_frames: &[(u64, String)],
    prompt: &str,
    settings: &AiSettings,
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
    let response = execute_with_retry("Local AI vision", || {
        client
            .post(format!("{}/api/chat", settings.base_url))
            .json(&payload)
            .send()
    })?;
    let body = read_json_response(response, "Local AI vision")?;
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
    create_embeddings(client, &[text.to_owned()], settings, kind)?
        .into_iter()
        .next()
        .ok_or_else(|| "AI embedding returned no vector".to_owned())
}

fn create_embeddings(
    client: &Client,
    texts: &[String],
    settings: &AiSettings,
    kind: EmbeddingKind,
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
            let response = execute_with_retry("OpenAI embedding", || {
                client
                    .post(format!("{}/embeddings", settings.base_url))
                    .bearer_auth(&settings.api_key)
                    .json(&payload)
                    .send()
            })?;
            let body = read_json_response(response, "OpenAI embedding")?;
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
        AiProvider::Gemini => texts
            .iter()
            .map(|text| create_gemini_embedding(client, text, settings, kind))
            .collect::<Result<Vec<_>, _>>()?,
        AiProvider::Local => {
            let payload = json!({
                "model": settings.embedding_model,
                "input": texts
            });
            let response = execute_with_retry("Local AI embedding", || {
                client
                    .post(format!("{}/api/embed", settings.base_url))
                    .json(&payload)
                    .send()
            })?;
            let body = read_json_response(response, "Local AI embedding")?;
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

fn request_single_gemini_embedding(
    client: &Client,
    model: &str,
    text: &str,
    settings: &AiSettings,
    task_type: &str,
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
    let response = execute_with_retry("Gemini embedding", || {
        client
            .post(&url)
            .header("x-goog-api-key", &settings.api_key)
            .json(&payload)
            .send()
    })
    .map_err(|err| (false, err))?;
    let is_not_found = response.status() == reqwest::StatusCode::NOT_FOUND;
    let body =
        read_json_response(response, "Gemini embedding").map_err(|err| (is_not_found, err))?;
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

fn create_gemini_embedding(
    client: &Client,
    text: &str,
    settings: &AiSettings,
    kind: EmbeddingKind,
) -> Result<Vec<f32>, String> {
    let task_type = match kind {
        EmbeddingKind::Document => "RETRIEVAL_DOCUMENT",
        EmbeddingKind::Query => "RETRIEVAL_QUERY",
    };
    request_single_gemini_embedding(client, &settings.embedding_model, text, settings, task_type)
        .map_err(|(_, err)| err)
}

fn values_to_embedding(values: &Vec<Value>) -> Vec<f32> {
    values
        .iter()
        .filter_map(Value::as_f64)
        .map(|value| value as f32)
        .collect()
}

fn read_json_response(response: Response, operation: &str) -> Result<Value, String> {
    let status = response.status();
    let raw = response
        .text()
        .map_err(|error| format!("{operation} returned an unreadable response: {error}"))?;
    let body: Value = serde_json::from_str(&raw).map_err(|error| {
        format!(
            "{operation} failed (HTTP {status}) with non-JSON response: {} ({error})",
            truncate(&raw, 500)
        )
    })?;
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

    fn stub_header<'a>(headers: &'a [String], name: &str) -> Option<&'a str> {
        headers.iter().find_map(|line| {
            let (header_name, value) = line.split_once(':')?;
            header_name
                .eq_ignore_ascii_case(name)
                .then_some(value.trim())
        })
    }

    fn write_stub_response(stream: &mut std::net::TcpStream, body: &Value) {
        use std::io::Write;

        let body = serde_json::to_string(body).expect("stub response should serialize");
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream
            .write_all(response.as_bytes())
            .expect("stub response should write");
    }

    fn spawn_openai_stub() -> (String, std::thread::JoinHandle<()>) {
        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").expect("stub should bind locally");
        let address = listener.local_addr().expect("stub should have an address");
        let handle = std::thread::spawn(move || loop {
            let (mut stream, _) = listener.accept().expect("stub should accept a request");
            let (request_line, _headers, request) = read_stub_request(&mut stream);
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
            ..Default::default()
        }))
        .expect("stub settings should be valid");
        let progress = std::cell::RefCell::new(Vec::new());

        let annotations = analyze_file_with_progress(&video, None, &settings, |event| {
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
