use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MediaMetadata {
    pub duration_ms: Option<u64>,
    pub size_bytes: Option<u64>,
    pub container: Option<String>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub frame_rate: Option<String>,
    pub start_time: Option<String>,
    pub creation_time: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "code", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MetadataError {
    ExecutableNotFound {
        executable: String,
        message: String,
    },
    ProcessFailed {
        status: Option<i32>,
        message: String,
    },
    InvalidOutput {
        message: String,
    },
    Io {
        message: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MetadataExtraction {
    pub metadata: Option<MediaMetadata>,
    pub error: Option<MetadataError>,
}

impl MetadataExtraction {
    fn success(metadata: MediaMetadata) -> Self {
        Self {
            metadata: Some(metadata),
            error: None,
        }
    }

    fn failure(error: MetadataError) -> Self {
        Self {
            metadata: None,
            error: Some(error),
        }
    }
}

pub trait MetadataProbe {
    fn extract(&self, path: &Path) -> MetadataExtraction;
}

#[derive(Clone, Debug)]
pub struct FfprobeMetadataProbe {
    executable: PathBuf,
}

impl Default for FfprobeMetadataProbe {
    fn default() -> Self {
        let executable = std::env::var_os("MEDIAINDEX_FFPROBE_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("ffprobe"));
        Self { executable }
    }
}

impl FfprobeMetadataProbe {
    pub fn with_executable(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
        }
    }

    pub fn executable(&self) -> &Path {
        &self.executable
    }
}

impl MetadataProbe for FfprobeMetadataProbe {
    fn extract(&self, path: &Path) -> MetadataExtraction {
        let output = match Command::new(&self.executable)
            .args([
                "-v",
                "error",
                "-print_format",
                "json",
                "-show_format",
                "-show_streams",
            ])
            .arg(path)
            .output()
        {
            Ok(output) => output,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return MetadataExtraction::failure(MetadataError::ExecutableNotFound {
                    executable: self.executable.display().to_string(),
                    message: error.to_string(),
                });
            }
            Err(error) => {
                return MetadataExtraction::failure(MetadataError::Io {
                    message: format!(
                        "cannot run FFprobe {} for {}: {error}",
                        self.executable.display(),
                        path.display()
                    ),
                });
            }
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            let message = if stderr.is_empty() {
                format!("FFprobe exited with status {}", output.status)
            } else {
                format!("FFprobe exited with status {}: {stderr}", output.status)
            };
            return MetadataExtraction::failure(MetadataError::ProcessFailed {
                status: output.status.code(),
                message,
            });
        }

        match parse_output(&output.stdout) {
            Ok(metadata) => MetadataExtraction::success(metadata),
            Err(message) => MetadataExtraction::failure(MetadataError::InvalidOutput { message }),
        }
    }
}

pub fn extract_media_metadata(path: &Path) -> MetadataExtraction {
    FfprobeMetadataProbe::default().extract(path)
}

#[derive(Debug, Deserialize)]
struct RawProbeOutput {
    #[serde(default)]
    format: Option<RawFormat>,
    #[serde(default)]
    streams: Vec<RawStream>,
}

#[derive(Debug, Deserialize)]
struct RawFormat {
    #[serde(default)]
    format_name: Option<Value>,
    #[serde(default)]
    duration: Option<Value>,
    #[serde(default)]
    size: Option<Value>,
    #[serde(default)]
    start_time: Option<Value>,
    #[serde(default)]
    tags: HashMap<String, Value>,
}

#[derive(Debug, Deserialize)]
struct RawStream {
    #[serde(default)]
    codec_type: Option<Value>,
    #[serde(default)]
    codec_name: Option<Value>,
    #[serde(default)]
    width: Option<Value>,
    #[serde(default)]
    height: Option<Value>,
    #[serde(default)]
    avg_frame_rate: Option<Value>,
    #[serde(default)]
    r_frame_rate: Option<Value>,
    #[serde(default)]
    start_time: Option<Value>,
    #[serde(default)]
    tags: HashMap<String, Value>,
}

fn parse_output(bytes: &[u8]) -> Result<MediaMetadata, String> {
    let output: RawProbeOutput = serde_json::from_slice(bytes)
        .map_err(|error| format!("FFprobe returned invalid JSON: {error}"))?;
    let format = output.format.as_ref();
    let video = output
        .streams
        .iter()
        .find(|stream| value_string(stream.codec_type.as_ref()).as_deref() == Some("video"));
    let audio = output
        .streams
        .iter()
        .find(|stream| value_string(stream.codec_type.as_ref()).as_deref() == Some("audio"));

    if format.is_none() && video.is_none() && audio.is_none() {
        return Err("FFprobe output did not contain a format or media stream".to_owned());
    }

    let start_time = format
        .and_then(|value| value_string(value.start_time.as_ref()))
        .or_else(|| video.and_then(|stream| value_string(stream.start_time.as_ref())));
    let creation_time = format
        .and_then(|value| tag_string(&value.tags, "creation_time"))
        .or_else(|| video.and_then(|stream| tag_string(&stream.tags, "creation_time")))
        .or_else(|| audio.and_then(|stream| tag_string(&stream.tags, "creation_time")));

    Ok(MediaMetadata {
        duration_ms: format.and_then(|value| value_duration_ms(value.duration.as_ref())),
        size_bytes: format.and_then(|value| value_u64(value.size.as_ref())),
        container: format.and_then(|value| value_string(value.format_name.as_ref())),
        video_codec: video.and_then(|stream| value_string(stream.codec_name.as_ref())),
        audio_codec: audio.and_then(|stream| value_string(stream.codec_name.as_ref())),
        width: video.and_then(|stream| value_u32(stream.width.as_ref())),
        height: video.and_then(|stream| value_u32(stream.height.as_ref())),
        frame_rate: video.and_then(|stream| {
            value_string(stream.avg_frame_rate.as_ref())
                .or_else(|| value_string(stream.r_frame_rate.as_ref()))
        }),
        start_time,
        creation_time,
    })
}

fn value_string(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(value) if !value.trim().is_empty() => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn tag_string(tags: &HashMap<String, Value>, key: &str) -> Option<String> {
    tags.get(key).and_then(|value| value_string(Some(value)))
}

fn value_u64(value: Option<&Value>) -> Option<u64> {
    let value = value?;
    value
        .as_u64()
        .or_else(|| value.as_str()?.parse::<u64>().ok())
}

fn value_u32(value: Option<&Value>) -> Option<u32> {
    value_u64(value).and_then(|value| value.try_into().ok())
}

fn value_duration_ms(value: Option<&Value>) -> Option<u64> {
    let seconds = value_string(value)?.parse::<f64>().ok()?;
    if !seconds.is_finite() || seconds < 0.0 {
        return None;
    }
    Some((seconds * 1000.0).round() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/ffprobe-sample.json"
    ));

    #[test]
    fn parses_the_documented_ffprobe_fixture() {
        let extraction = FfprobeMetadataProbe::with_executable("ffprobe");
        let parsed =
            parse_output(FIXTURE.as_bytes()).expect("fixture should be valid FFprobe output");

        assert_eq!(extraction.executable(), Path::new("ffprobe"));
        assert_eq!(parsed.duration_ms, Some(12_345));
        assert_eq!(parsed.size_bytes, Some(1_048_576));
        assert_eq!(parsed.container.as_deref(), Some("mov,mp4,m4a,3gp,3g2,mj2"));
        assert_eq!(parsed.video_codec.as_deref(), Some("h264"));
        assert_eq!(parsed.audio_codec.as_deref(), Some("aac"));
        assert_eq!(parsed.width, Some(1920));
        assert_eq!(parsed.height, Some(1080));
        assert_eq!(parsed.frame_rate.as_deref(), Some("30000/1001"));
        assert_eq!(parsed.start_time.as_deref(), Some("0.000000"));
        assert_eq!(
            parsed.creation_time.as_deref(),
            Some("2026-08-21T10:15:00.000000Z")
        );
    }

    #[test]
    fn invalid_output_is_actionable() {
        let error = parse_output(br#"{"format": {"duration": "not-a-number"}}"#)
            .expect("a format object should still produce a metadata record");

        assert_eq!(error.duration_ms, None);
        assert_eq!(error.container, None);
    }

    #[test]
    fn missing_executable_is_stored_as_error_state() {
        let extraction = FfprobeMetadataProbe::with_executable(
            "mediaindex-ffprobe-executable-that-does-not-exist",
        )
        .extract(Path::new("clip.mp4"));

        assert!(extraction.metadata.is_none());
        assert!(matches!(
            extraction.error,
            Some(MetadataError::ExecutableNotFound { .. })
        ));
    }

    #[test]
    fn metadata_probe_is_replaceable_for_callers_and_tests() {
        struct StubProbe;

        impl MetadataProbe for StubProbe {
            fn extract(&self, _path: &Path) -> MetadataExtraction {
                MetadataExtraction::success(MediaMetadata {
                    duration_ms: Some(1_000),
                    size_bytes: Some(42),
                    container: Some("fixture".to_owned()),
                    video_codec: None,
                    audio_codec: None,
                    width: None,
                    height: None,
                    frame_rate: None,
                    start_time: None,
                    creation_time: None,
                })
            }
        }

        let extraction = StubProbe.extract(Path::new("fixture.mp4"));
        assert_eq!(
            extraction.metadata.expect("stub should succeed").size_bytes,
            Some(42)
        );
    }
}
