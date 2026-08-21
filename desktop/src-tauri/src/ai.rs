use base64::Engine;
use reqwest::blocking::{Client, Response};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::metadata::MediaMetadata;

const DEFAULT_OPENAI_VISION_MODEL: &str = "gpt-4.1-mini";
const DEFAULT_OPENAI_EMBEDDING_MODEL: &str = "text-embedding-3-small";
const DEFAULT_OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
const DEFAULT_GEMINI_VISION_MODEL: &str = "gemini-3.6-flash";
const DEFAULT_GEMINI_EMBEDDING_MODEL: &str = "gemini-embedding-001";
const DEFAULT_GEMINI_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";
const DEFAULT_LOCAL_VISION_MODEL: &str = "gemma4";
const DEFAULT_LOCAL_EMBEDDING_MODEL: &str = "embeddinggemma";
const DEFAULT_LOCAL_BASE_URL: &str = "http://127.0.0.1:11434";

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
    pub annotation_count: u64,
    pub warnings: Vec<AiWarning>,
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
        let api_key = api_key.unwrap_or_default();
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

        let vision_model = non_empty(request.vision_model)
            .or_else(|| env_non_empty("MEDIAINDEX_AI_MODEL"))
            .unwrap_or_else(|| default_vision_model.to_owned());
        let embedding_model = non_empty(request.embedding_model)
            .or_else(|| env_non_empty("MEDIAINDEX_AI_EMBEDDING_MODEL"))
            .unwrap_or_else(|| default_embedding_model.to_owned());
        let base_url = non_empty(request.base_url)
            .or_else(|| env_non_empty("MEDIAINDEX_AI_BASE_URL"))
            .or_else(|| match provider {
                AiProvider::OpenAI => env_non_empty("MEDIAINDEX_OPENAI_BASE_URL"),
                AiProvider::Gemini => env_non_empty("MEDIAINDEX_GEMINI_BASE_URL"),
                AiProvider::Local => env_non_empty("MEDIAINDEX_LOCAL_BASE_URL"),
            })
            .unwrap_or_else(|| default_base_url.to_owned())
            .trim_end_matches('/')
            .to_owned();

        let sample_interval_seconds = match request.sample_interval_seconds {
            Some(value) => value,
            None => environment_u64("MEDIAINDEX_AI_SAMPLE_SECONDS", 5)?,
        };
        let max_frames_per_file = match request.max_frames {
            Some(value) => value,
            None => environment_u64("MEDIAINDEX_AI_MAX_FRAMES", 120)?,
        } as usize;

        let ffmpeg_executable = non_empty(request.ffmpeg_path)
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("MEDIAINDEX_FFMPEG_PATH").map(PathBuf::from))
            .filter(|path| path.is_file())
            .unwrap_or_else(|| PathBuf::from("ffmpeg"));

        Ok(Self {
            provider,
            api_key,
            vision_model,
            embedding_model,
            base_url,
            ffmpeg_executable,
            sample_interval_ms: sample_interval_seconds.saturating_mul(1_000),
            max_frames_per_file,
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
}

pub fn analyze_file(
    path: &Path,
    metadata: Option<&MediaMetadata>,
    settings: &AiSettings,
) -> Result<Vec<AiAnnotation>, String> {
    let duration_ms = metadata
        .and_then(|value| value.duration_ms)
        .unwrap_or(settings.sample_interval_ms)
        .max(1);
    let mut timestamps = Vec::new();
    let mut timestamp_ms = 0;
    while timestamps.len() < settings.max_frames_per_file && timestamp_ms < duration_ms {
        timestamps.push(timestamp_ms);
        timestamp_ms = timestamp_ms.saturating_add(settings.sample_interval_ms);
    }
    if timestamps.is_empty() {
        timestamps.push(0);
    }

    let client = Client::builder()
        .build()
        .map_err(|error| format!("cannot create AI HTTP client: {error}"))?;
    let mut annotations = Vec::with_capacity(timestamps.len());
    for timestamp_ms in timestamps {
        let frame = extract_frame(path, timestamp_ms, settings)?;
        let analysis = describe_frame(&client, &frame, timestamp_ms, settings)?;
        let embedding_text = format!("{}\n{}", analysis.description, analysis.labels.join(", "));
        let embedding =
            create_embedding(&client, &embedding_text, settings, EmbeddingKind::Document)?;
        annotations.push(AiAnnotation {
            timestamp_ms,
            description: analysis.description,
            labels: analysis.labels,
            embedding,
            confidence: analysis.confidence,
            model: settings.model_namespace(),
        });
    }

    Ok(annotations)
}

pub fn embed_query(query: &str, settings: &AiSettings) -> Result<Vec<f32>, String> {
    if query.trim().is_empty() {
        return Err("AI search query cannot be empty".to_owned());
    }
    let client = Client::builder()
        .build()
        .map_err(|error| format!("cannot create AI HTTP client: {error}"))?;
    create_embedding(&client, query.trim(), settings, EmbeddingKind::Query)
}

pub fn test_connection(settings: &AiSettings) -> Result<AiConnectionReport, String> {
    let client = Client::builder()
        .build()
        .map_err(|error| format!("cannot create AI HTTP client: {error}"))?;
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

fn extract_frame(path: &Path, timestamp_ms: u64, settings: &AiSettings) -> Result<Vec<u8>, String> {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or_default();
    let output_path =
        std::env::temp_dir().join(format!("mediaindex-ai-{}-{unique}.jpg", std::process::id()));
    let timestamp = format_seconds(timestamp_ms);
    let mut command = Command::new(&settings.ffmpeg_executable);
    configure_hidden_process(&mut command);
    let output = command
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-ss",
            &timestamp,
            "-i",
        ])
        .arg(path)
        .args(["-frames:v", "1", "-q:v", "4", "-f", "image2"])
        .arg(&output_path)
        .output()
        .map_err(|error| {
            format!(
                "FFmpeg could not be started for {}: {error}",
                path.display()
            )
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let _ = fs::remove_file(&output_path);
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

    let frame = fs::read(&output_path)
        .map_err(|error| format!("cannot read extracted AI frame: {error}"))?;
    let _ = fs::remove_file(&output_path);
    Ok(frame)
}

fn configure_hidden_process(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
}

fn format_seconds(timestamp_ms: u64) -> String {
    format!("{:.3}", timestamp_ms as f64 / 1_000.0)
}

#[derive(Debug, Deserialize)]
struct FrameAnalysis {
    description: String,
    #[serde(default)]
    labels: Vec<String>,
    confidence: Option<f32>,
}

#[derive(Clone, Copy)]
enum EmbeddingKind {
    Document,
    Query,
}

fn describe_frame(
    client: &Client,
    frame: &[u8],
    timestamp_ms: u64,
    settings: &AiSettings,
) -> Result<FrameAnalysis, String> {
    let encoded = base64::engine::general_purpose::STANDARD.encode(frame);
    let prompt = format!(
        "Analyze this video frame at timestamp {timestamp_ms} ms. This is for a searchable local video library. Return only JSON with keys description (short factual sentence), labels (lowercase array of useful visual/event labels), and confidence (number from 0 to 1). Include gameplay context and visible events such as elimination, enemy defeated, player knocked, fight, building, item pickup, or victory only when supported by the frame. Add synonyms that make natural-language search useful, but do not invent events that are not visible."
    );
    let text = match settings.provider {
        AiProvider::OpenAI => describe_openai(client, &encoded, &prompt, settings)?,
        AiProvider::Gemini => describe_gemini(client, &encoded, &prompt, settings)?,
        AiProvider::Local => describe_local(client, &encoded, &prompt, settings)?,
    };
    parse_frame_analysis(&text)
}

fn describe_openai(
    client: &Client,
    encoded: &str,
    prompt: &str,
    settings: &AiSettings,
) -> Result<String, String> {
    let response = client
        .post(format!("{}/responses", settings.base_url))
        .bearer_auth(&settings.api_key)
        .json(&json!({
            "model": settings.vision_model,
            "input": [{
                "role": "user",
                "content": [
                    {"type": "input_text", "text": prompt},
                    {"type": "input_image", "image_url": format!("data:image/jpeg;base64,{encoded}"), "detail": "low"}
                ]
            }]
        }))
        .send()
        .map_err(|error| format!("OpenAI vision request failed: {error}"))?;
    let body = read_json_response(response, "OpenAI vision")?;
    response_text(&body).ok_or_else(|| "OpenAI vision returned no output text".to_owned())
}

fn describe_gemini(
    client: &Client,
    encoded: &str,
    prompt: &str,
    settings: &AiSettings,
) -> Result<String, String> {
    let response = client
        .post(format!(
            "{}/models/{}:generateContent",
            settings.base_url, settings.vision_model
        ))
        .header("x-goog-api-key", &settings.api_key)
        .json(&json!({
            "contents": [{
                "role": "user",
                "parts": [
                    {"text": prompt},
                    {"inline_data": {"mime_type": "image/jpeg", "data": encoded}}
                ]
            }],
            "generationConfig": {"responseMimeType": "application/json"}
        }))
        .send()
        .map_err(|error| format!("Gemini vision request failed: {error}"))?;
    let body = read_json_response(response, "Gemini vision")?;
    response_text(&body).ok_or_else(|| "Gemini vision returned no candidate text".to_owned())
}

fn describe_local(
    client: &Client,
    encoded: &str,
    prompt: &str,
    settings: &AiSettings,
) -> Result<String, String> {
    let response = client
        .post(format!("{}/api/chat", settings.base_url))
        .json(&json!({
            "model": settings.vision_model,
            "messages": [{"role": "user", "content": prompt, "images": [encoded]}],
            "format": "json",
            "stream": false
        }))
        .send()
        .map_err(|error| format!("Local AI request failed: {error}"))?;
    let body = read_json_response(response, "Local AI vision")?;
    response_text(&body).ok_or_else(|| "Local AI returned no message content".to_owned())
}

fn parse_frame_analysis(text: &str) -> Result<FrameAnalysis, String> {
    let json_text = strip_json_fence(text);
    let parsed: FrameAnalysis = serde_json::from_str(&json_text)
        .or_else(|_| serde_json::from_str(extract_json_object(&json_text).unwrap_or(&json_text)))
        .map_err(|error| format!("AI returned an invalid annotation: {error}"))?;
    if parsed.description.trim().is_empty() {
        return Err("AI returned an empty description".to_owned());
    }
    Ok(FrameAnalysis {
        description: parsed.description.trim().to_owned(),
        labels: normalize_labels(parsed.labels),
        confidence: parsed.confidence.map(|value| value.clamp(0.0, 1.0)),
    })
}

fn create_embedding(
    client: &Client,
    text: &str,
    settings: &AiSettings,
    kind: EmbeddingKind,
) -> Result<Vec<f32>, String> {
    let embedding = match settings.provider {
        AiProvider::OpenAI => {
            let response = client
                .post(format!("{}/embeddings", settings.base_url))
                .bearer_auth(&settings.api_key)
                .json(&json!({
                    "model": settings.embedding_model,
                    "input": text
                }))
                .send()
                .map_err(|error| format!("OpenAI embedding request failed: {error}"))?;
            let body = read_json_response(response, "OpenAI embedding")?;
            body.get("data")
                .and_then(Value::as_array)
                .and_then(|items| items.first())
                .and_then(|item| item.get("embedding"))
                .and_then(Value::as_array)
                .map(values_to_embedding)
        }
        AiProvider::Gemini => {
            let task_type = match kind {
                EmbeddingKind::Document => "RETRIEVAL_DOCUMENT",
                EmbeddingKind::Query => "RETRIEVAL_QUERY",
            };
            let response = client
                .post(format!(
                    "{}/models/{}:embedContent",
                    settings.base_url, settings.embedding_model
                ))
                .header("x-goog-api-key", &settings.api_key)
                .json(&json!({
                    "content": {"parts": [{"text": text}]},
                    "taskType": task_type
                }))
                .send()
                .map_err(|error| format!("Gemini embedding request failed: {error}"))?;
            let body = read_json_response(response, "Gemini embedding")?;
            body.get("embedding")
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
                })
        }
        AiProvider::Local => {
            let response = client
                .post(format!("{}/api/embed", settings.base_url))
                .json(&json!({
                    "model": settings.embedding_model,
                    "input": text
                }))
                .send()
                .map_err(|error| format!("Local AI embedding request failed: {error}"))?;
            let body = read_json_response(response, "Local AI embedding")?;
            body.get("embeddings")
                .and_then(Value::as_array)
                .and_then(|items| items.first())
                .and_then(Value::as_array)
                .map(values_to_embedding)
        }
    };

    embedding
        .filter(|embedding| !embedding.is_empty())
        .ok_or_else(|| "AI embedding returned no vector".to_owned())
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
        return Err(format!(
            "{operation} failed (HTTP {status}): {}",
            api_error_detail(&body)
        ));
    }
    if body.get("error").is_some() || body.get("status").and_then(Value::as_str) == Some("failed") {
        return Err(format!(
            "{operation} failed (HTTP {status}): {}",
            api_error_detail(&body)
        ));
    }
    Ok(body)
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn builds_local_settings_without_an_api_key() {
        let settings = AiSettings::from_request(Some(AiRequestConfig {
            provider: Some(AiProvider::Local),
            ..Default::default()
        }))
        .expect("local settings should not need an API key");

        assert_eq!(settings.provider, AiProvider::Local);
        assert_eq!(settings.vision_model, DEFAULT_LOCAL_VISION_MODEL);
        assert_eq!(settings.embedding_model, DEFAULT_LOCAL_EMBEDDING_MODEL);
        assert_eq!(settings.model_namespace(), "local:gemma4:embeddinggemma");
    }

    #[test]
    fn calculates_timestamp_format_without_float_drift() {
        assert_eq!(format_seconds(12_345), "12.345");
    }
}
