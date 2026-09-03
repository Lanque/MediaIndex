use base64::Engine;
use rand::Rng;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri_plugin_opener::OpenerExt;
use url::Url;

use crate::ai::{AiProvider, AiRequestConfig};

const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const GOOGLE_SCOPE: &str = "openid email https://www.googleapis.com/auth/cloud-platform";
const LOGIN_TIMEOUT: Duration = Duration::from_secs(180);
const MAX_CLIENT_FILE_BYTES: u64 = 1_048_576;

#[derive(Clone, Debug)]
struct GeminiCredential {
    access_token: String,
    project_id: String,
    expires_at_unix_ms: u64,
}

#[derive(Clone, Default)]
pub struct GeminiOAuthSession {
    credential: Arc<Mutex<Option<GeminiCredential>>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct GeminiOAuthStatus {
    pub connected: bool,
    pub project_id: Option<String>,
    pub expires_at_unix_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct OAuthClientFile {
    installed: InstalledClient,
}

#[derive(Debug, Deserialize)]
struct InstalledClient {
    client_id: String,
    client_secret: String,
    project_id: String,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
}

impl GeminiOAuthSession {
    pub fn login(
        &self,
        app: &tauri::AppHandle,
        client_file_path: &Path,
    ) -> Result<GeminiOAuthStatus, String> {
        let client = read_client_file(client_file_path)?;
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|error| format!("cannot start the local Google login callback: {error}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|error| format!("cannot configure the Google login callback: {error}"))?;
        let port = listener
            .local_addr()
            .map_err(|error| format!("cannot read the Google login callback address: {error}"))?
            .port();
        let redirect_uri = format!("http://127.0.0.1:{port}");
        let state = random_urlsafe(32)?;
        let verifier = random_urlsafe(48)?;
        let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(Sha256::digest(verifier.as_bytes()));
        let mut authorization_url = Url::parse(GOOGLE_AUTH_URL)
            .map_err(|error| format!("cannot build the Google login URL: {error}"))?;
        authorization_url
            .query_pairs_mut()
            .append_pair("client_id", client.installed.client_id.trim())
            .append_pair("redirect_uri", &redirect_uri)
            .append_pair("response_type", "code")
            .append_pair("scope", GOOGLE_SCOPE)
            .append_pair("access_type", "offline")
            .append_pair("prompt", "consent")
            .append_pair("state", &state)
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256");

        app.opener()
            .open_url(authorization_url.as_str(), None::<String>)
            .map_err(|error| format!("cannot open Google login in your browser: {error}"))?;

        let callback = wait_for_callback(&listener, LOGIN_TIMEOUT)?;
        let callback_url = Url::parse(&format!("http://127.0.0.1{callback}"))
            .map_err(|error| format!("Google returned an invalid login callback: {error}"))?;
        let parameters = callback_url
            .query_pairs()
            .map(|(name, value)| (name.into_owned(), value.into_owned()))
            .collect::<std::collections::HashMap<_, _>>();
        if parameters.get("state") != Some(&state) {
            return Err("Google login state did not match; no token was accepted".to_owned());
        }
        if let Some(error) = parameters.get("error") {
            let detail = parameters
                .get("error_description")
                .map(String::as_str)
                .unwrap_or(error);
            return Err(format!("Google login was not completed: {detail}"));
        }
        let code = parameters
            .get("code")
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "Google login returned no authorization code".to_owned())?;

        let http = Client::builder()
            .connect_timeout(Duration::from_secs(20))
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|error| format!("cannot create the Google token client: {error}"))?;
        let response = http
            .post(GOOGLE_TOKEN_URL)
            .form(&[
                ("client_id", client.installed.client_id.trim()),
                ("client_secret", client.installed.client_secret.trim()),
                ("code", code.as_str()),
                ("code_verifier", verifier.as_str()),
                ("grant_type", "authorization_code"),
                ("redirect_uri", redirect_uri.as_str()),
            ])
            .send()
            .map_err(|error| format!("Google token exchange failed: {error}"))?;
        let status = response.status();
        let body = response
            .text()
            .map_err(|error| format!("cannot read the Google token response: {error}"))?;
        if !status.is_success() {
            let detail = serde_json::from_str::<serde_json::Value>(&body)
                .ok()
                .and_then(|value| {
                    value
                        .get("error_description")
                        .or_else(|| value.get("error"))
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned)
                })
                .unwrap_or_else(|| format!("HTTP {status}"));
            return Err(format!("Google token exchange was rejected: {detail}"));
        }
        let token: TokenResponse = serde_json::from_str(&body)
            .map_err(|error| format!("Google returned an invalid token response: {error}"))?;
        if token.access_token.trim().is_empty() {
            return Err("Google returned an empty access token".to_owned());
        }
        let expires_at_unix_ms = unix_time_ms()
            .saturating_add(token.expires_in.saturating_mul(1_000))
            .saturating_sub(30_000);
        let credential = GeminiCredential {
            access_token: token.access_token,
            project_id: client.installed.project_id.trim().to_owned(),
            expires_at_unix_ms,
        };
        if credential.project_id.is_empty() {
            return Err("The Google Desktop OAuth JSON has no project_id".to_owned());
        }
        *self.lock() = Some(credential);
        Ok(self.status())
    }

    pub fn logout(&self) -> GeminiOAuthStatus {
        *self.lock() = None;
        self.status()
    }

    pub fn status(&self) -> GeminiOAuthStatus {
        let now = unix_time_ms();
        let mut credential = self.lock();
        if credential
            .as_ref()
            .is_some_and(|value| value.expires_at_unix_ms <= now)
        {
            *credential = None;
        }
        GeminiOAuthStatus {
            connected: credential.is_some(),
            project_id: credential.as_ref().map(|value| value.project_id.clone()),
            expires_at_unix_ms: credential.as_ref().map(|value| value.expires_at_unix_ms),
        }
    }

    pub fn resolve_config(
        &self,
        config: Option<AiRequestConfig>,
    ) -> Result<Option<AiRequestConfig>, String> {
        let Some(mut config) = config else {
            return Ok(None);
        };
        let uses_oauth = config.provider == Some(AiProvider::Gemini)
            && config.auth_mode.as_deref() == Some("oauth");
        if !uses_oauth {
            return Ok(Some(config));
        }
        let now = unix_time_ms();
        let mut stored = self.lock();
        if stored
            .as_ref()
            .is_some_and(|value| value.expires_at_unix_ms <= now)
        {
            *stored = None;
        }
        let credential = stored.as_ref().ok_or_else(|| {
            "Google login is missing or expired. Press Login with Google and try again.".to_owned()
        })?;
        config.api_key = Some(credential.access_token.clone());
        config.google_project_id = Some(credential.project_id.clone());
        Ok(Some(config))
    }

    fn lock(&self) -> MutexGuard<'_, Option<GeminiCredential>> {
        self.credential
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn read_client_file(path: &Path) -> Result<OAuthClientFile, String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("cannot read the Google Desktop OAuth JSON: {error}"))?;
    if !metadata.is_file() {
        return Err("Select a Google Desktop OAuth client JSON file".to_owned());
    }
    if metadata.len() > MAX_CLIENT_FILE_BYTES {
        return Err("The selected OAuth client JSON is unexpectedly large".to_owned());
    }
    let body = fs::read_to_string(path)
        .map_err(|error| format!("cannot read the Google Desktop OAuth JSON: {error}"))?;
    let client: OAuthClientFile = serde_json::from_str(&body).map_err(|error| {
        format!("The selected file is not a Google Desktop OAuth client JSON: {error}")
    })?;
    if client.installed.client_id.trim().is_empty()
        || client.installed.client_secret.trim().is_empty()
    {
        return Err("The Google Desktop OAuth JSON is missing client credentials".to_owned());
    }
    Ok(client)
}

fn random_urlsafe(byte_count: usize) -> Result<String, String> {
    let mut bytes = vec![0u8; byte_count];
    rand::rng().fill_bytes(&mut bytes);
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
}

fn wait_for_callback(listener: &TcpListener, timeout: Duration) -> Result<String, String> {
    let started = Instant::now();
    loop {
        match listener.accept() {
            Ok((mut stream, _)) => return read_callback(&mut stream),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if started.elapsed() >= timeout {
                    return Err("Google login timed out after 3 minutes".to_owned());
                }
                thread::sleep(Duration::from_millis(100));
            }
            Err(error) => {
                return Err(format!("Google login callback failed: {error}"));
            }
        }
    }
}

fn read_callback(stream: &mut TcpStream) -> Result<String, String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|error| format!("cannot configure the Google callback connection: {error}"))?;
    let mut request = Vec::new();
    let mut buffer = [0u8; 2_048];
    while request.len() < 32_768 && !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
        let read = stream
            .read(&mut buffer)
            .map_err(|error| format!("cannot read the Google login callback: {error}"))?;
        if read == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..read]);
    }
    let request_line = String::from_utf8_lossy(&request)
        .lines()
        .next()
        .unwrap_or_default()
        .to_owned();
    let target = request_line
        .split_whitespace()
        .nth(1)
        .filter(|value| value.starts_with('/'))
        .ok_or_else(|| "Google returned an invalid local callback".to_owned())?
        .to_owned();
    let body = "<!doctype html><meta charset=\"utf-8\"><title>MediaIndex Google login</title><style>body{font:16px system-ui;background:#171815;color:#eee;padding:48px}main{max-width:560px;margin:auto}</style><main><h1>Return to MediaIndex</h1><p>Google sign-in was received. You can close this tab and continue in the desktop app.</p></main>";
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    Ok(target)
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis() as u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oauth_session_injects_only_gemini_oauth_requests() {
        let session = GeminiOAuthSession::default();
        *session.lock() = Some(GeminiCredential {
            access_token: "oauth-token".to_owned(),
            project_id: "mediaindex-test".to_owned(),
            expires_at_unix_ms: unix_time_ms() + 60_000,
        });
        let resolved = session
            .resolve_config(Some(AiRequestConfig {
                provider: Some(AiProvider::Gemini),
                auth_mode: Some("oauth".to_owned()),
                ..Default::default()
            }))
            .expect("valid OAuth session should resolve")
            .expect("request should remain present");
        assert_eq!(resolved.api_key.as_deref(), Some("oauth-token"));
        assert_eq!(
            resolved.google_project_id.as_deref(),
            Some("mediaindex-test")
        );

        let openai = session
            .resolve_config(Some(AiRequestConfig {
                provider: Some(AiProvider::OpenAI),
                auth_mode: Some("oauth".to_owned()),
                api_key: Some("openai-key".to_owned()),
                ..Default::default()
            }))
            .expect("OpenAI request should pass through")
            .expect("request should remain present");
        assert_eq!(openai.api_key.as_deref(), Some("openai-key"));
    }

    #[test]
    fn expired_oauth_session_is_rejected_and_cleared() {
        let session = GeminiOAuthSession::default();
        *session.lock() = Some(GeminiCredential {
            access_token: "expired".to_owned(),
            project_id: "mediaindex-test".to_owned(),
            expires_at_unix_ms: unix_time_ms().saturating_sub(1),
        });
        let error = session
            .resolve_config(Some(AiRequestConfig {
                provider: Some(AiProvider::Gemini),
                auth_mode: Some("oauth".to_owned()),
                ..Default::default()
            }))
            .expect_err("expired OAuth session should fail");
        assert!(error.contains("expired"));
        assert!(!session.status().connected);
    }
}
