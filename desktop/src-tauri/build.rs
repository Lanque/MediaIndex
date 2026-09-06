use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repository_dir = manifest_dir
        .parent()
        .and_then(|parent| parent.parent())
        .unwrap_or(&manifest_dir);
    let git_sha = Command::new("git")
        .args([
            "-C",
            &repository_dir.to_string_lossy(),
            "rev-parse",
            "--short=12",
            "HEAD",
        ])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown".to_owned());
    let build_time_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_owned());
    println!("cargo:rustc-env=MEDIAINDEX_BUILD_GIT_SHA={git_sha}");
    println!("cargo:rustc-env=MEDIAINDEX_BUILD_TIME_UNIX={build_time_unix}");
    tauri_build::build()
}
