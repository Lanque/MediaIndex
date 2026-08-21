use base64::Engine;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::metadata::MediaMetadata;

const DEFAULT_VISION_MODEL: &str = "gpt-4.1-mini";
const DEFAULT_EMBEDDING_MODEL: &str = "text-embedding-3-small";
const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

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

#[derive(Clone)]
pub struct AiSettings {
    api_key: String,
    vision_model: String,
    embedding_model: String,
    base_url: String,
    ffmpeg_executable: PathBuf,
    sample_interval_ms: u64,
    max_frames_per_file: usize,
}

impl AiSettings {
    pub fn from_environment() -> Result<Self, String> {
        let api_key = std::env::var("MEDIAINDEX_OPENAI_API_KEY")
            .or_else(|_| std::env::var("OPENAI_API_KEY"))
            .map_err(|_| {
                "AI search needs MEDIAINDEX_OPENAI_API_KEY or OPENAI_API_KEY in the environment"
                    .to_owned()
            })?;
        if api_key.trim().is_empty() {
            return Err("AI search API key is empty".to_owned());
        }

        let sample_interval_seconds = environment_u64("MEDIAINDEX_AI_SAMPLE_SECONDS", 5)?;
        let max_frames_per_file = environment_u64("MEDIAINDEX_AI_MAX_FRAMES", 120)? as usize;
        if max_frames_per_file == 0 {
            return Err("MEDIAINDEX_AI_MAX_FRAMES must be greater than zero".to_owned());
        }

        Ok(Self {
            api_key,
            vision_model: std::env::var("MEDIAINDEX_AI_MODEL")
                .unwrap_or_else(|_| DEFAULT_VISION_MODEL.to_owned()),
            embedding_model: std::env::var("MEDIAINDEX_AI_EMBEDDING_MODEL")
                .unwrap_or_else(|_| DEFAULT_EMBEDDING_MODEL.to_owned()),
            base_url: std::env::var("MEDIAINDEX_OPENAI_BASE_URL")
                .unwrap_or_else(|_| DEFAULT_BASE_URL.to_owned())
                .trim_end_matches('/')
                .to_owned(),
            ffmpeg_executable: std::env::var_os("MEDIAINDEX_FFMPEG_PATH")
                .map(PathBuf::from)
                .filter(|path| path.is_file())
                .unwrap_or_else(|| PathBuf::from("ffmpeg")),
            sample_interval_ms: sample_interval_seconds.saturating_mul(1_000),
            max_frames_per_file,
        })
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
        let embedding = create_embedding(&client, &embedding_text, settings)?;
        annotations.push(AiAnnotation {
            timestamp_ms,
            description: analysis.description,
            labels: analysis.labels,
            embedding,
            confidence: analysis.confidence,
            model: settings.vision_model.clone(),
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
    create_embedding(&client, query.trim(), settings)
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

fn extract_frame(path: &Path, timestamp_ms: u64, settings: &AiSettings) -> Result<Vec<u8>, String> {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or_default();
    let output_path =
        std::env::temp_dir().join(format!("mediaindex-ai-{}-{unique}.jpg", std::process::id()));
    let timestamp = format_seconds(timestamp_ms);
    let output = Command::new(&settings.ffmpeg_executable)
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
        .map_err(|error| format!("AI vision request failed: {error}"))?;
    let body: Value = response
        .json()
        .map_err(|error| format!("AI vision returned invalid JSON: {error}"))?;
    ensure_success(&body)?;
    let text = output_text(&body).ok_or_else(|| "AI vision returned no output text".to_owned())?;
    let json_text = strip_json_fence(&text);
    let parsed: FrameAnalysis = serde_json::from_str(&json_text)
        .or_else(|_| serde_json::from_str(extract_json_object(&json_text).unwrap_or(&json_text)))
        .map_err(|error| format!("AI vision returned an invalid annotation: {error}"))?;
    if parsed.description.trim().is_empty() {
        return Err("AI vision returned an empty description".to_owned());
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
) -> Result<Vec<f32>, String> {
    let response = client
        .post(format!("{}/embeddings", settings.base_url))
        .bearer_auth(&settings.api_key)
        .json(&json!({
            "model": settings.embedding_model,
            "input": text
        }))
        .send()
        .map_err(|error| format!("AI embedding request failed: {error}"))?;
    let body: Value = response
        .json()
        .map_err(|error| format!("AI embedding returned invalid JSON: {error}"))?;
    ensure_success(&body)?;
    body.get("data")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(|item| item.get("embedding"))
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_f64)
                .map(|value| value as f32)
                .collect::<Vec<_>>()
        })
        .filter(|embedding| !embedding.is_empty())
        .ok_or_else(|| "AI embedding returned no vector".to_owned())
}

fn ensure_success(body: &Value) -> Result<(), String> {
    if body.get("error").is_some() {
        let message = body
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("unknown AI API error");
        return Err(format!("AI API error: {message}"));
    }
    Ok(())
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

fn strip_json_fence(text: &str) -> String {
    let trimmed = text.trim();
    if let Some(content) = trimmed.strip_prefix("```json") {
        return content.trim_end_matches('`').trim().to_owned();
    }
    if let Some(content) = trimmed.strip_prefix("```") {
        return content.trim_end_matches('`').trim().to_owned();
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
            output_text(&body).as_deref(),
            Some("{\"description\":\"A player wins\"}")
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
    fn calculates_timestamp_format_without_float_drift() {
        assert_eq!(format_seconds(12_345), "12.345");
    }
}
