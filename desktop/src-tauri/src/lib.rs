pub mod ai;
pub mod gemini_oauth;
pub mod local_index;
pub mod metadata;
pub mod scanner;

use base64::Engine;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use tauri::{Emitter, Manager};
use tauri_plugin_opener::OpenerExt;

#[derive(Default)]
struct AiAnalysisFlags {
    running: bool,
    cancel_requested: bool,
}

#[derive(Clone, Default)]
struct AiAnalysisControl {
    flags: Arc<Mutex<AiAnalysisFlags>>,
}

impl AiAnalysisControl {
    fn begin(&self) -> Result<AiAnalysisRunGuard, String> {
        let mut flags = lock_unpoisoned(&self.flags);
        if flags.running {
            return Err("AI analysis is already running".to_owned());
        }
        flags.running = true;
        flags.cancel_requested = false;
        Ok(AiAnalysisRunGuard {
            control: self.clone(),
        })
    }

    fn request_cancel(&self) -> bool {
        let mut flags = lock_unpoisoned(&self.flags);
        if !flags.running {
            return false;
        }
        flags.cancel_requested = true;
        true
    }

    fn is_cancelled(&self) -> bool {
        lock_unpoisoned(&self.flags).cancel_requested
    }
}

struct AiAnalysisRunGuard {
    control: AiAnalysisControl,
}

#[derive(Debug, serde::Serialize)]
struct AiAnalysisPlan {
    total_file_count: u64,
    analyze_file_count: u64,
    skipped_file_count: u64,
    already_analyzed_file_count: u64,
    max_frames_per_file: u64,
    max_sampled_frames: u64,
    max_vision_requests: u64,
    estimated_sampled_frames: u64,
    estimated_vision_requests: u64,
    estimated_audio_seconds: u64,
    model: String,
}

impl Drop for AiAnalysisRunGuard {
    fn drop(&mut self) {
        let mut flags = lock_unpoisoned(&self.control.flags);
        flags.running = false;
        flags.cancel_requested = false;
    }
}

#[tauri::command]
async fn scan_media_folder(path: String) -> Result<scanner::ScanReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        scanner::scan_folder(Path::new(&path), &scanner::ScanOptions::default())
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("folder scan worker failed: {error}"))?
}

#[tauri::command]
async fn extract_media_metadata(path: String) -> Result<metadata::MetadataExtraction, String> {
    tauri::async_runtime::spawn_blocking(move || metadata::extract_media_metadata(Path::new(&path)))
        .await
        .map_err(|error| format!("metadata worker failed: {error}"))
}

#[tauri::command]
async fn index_media_folder(
    app: tauri::AppHandle,
    path: String,
) -> Result<local_index::IndexReport, String> {
    tauri::async_runtime::spawn_blocking(move || index_media_folder_blocking(app, path))
        .await
        .map_err(|error| format!("folder indexing worker failed: {error}"))?
}

fn index_media_folder_blocking(
    app: tauri::AppHandle,
    path: String,
) -> Result<local_index::IndexReport, String> {
    let mut index = open_local_index(&app)?;
    let known_files = index
        .known_files()
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|file| {
            let path = file.path.clone();
            (
                path,
                scanner::LocalFileRecord {
                    path: file.path,
                    size_bytes: file.size_bytes,
                    modified_unix_ms: file.modified_unix_ms,
                    content_hash: file.content_hash,
                },
            )
        })
        .collect::<HashMap<_, _>>();

    let mut scan = scanner::scan_folder_with_known_files(
        Path::new(&path),
        &scanner::ScanOptions::default(),
        &known_files,
    )
    .map_err(|error| error.to_string())?;
    let mut cached_metadata = HashMap::new();
    for file in &scan.files {
        if cached_metadata.contains_key(&file.content_hash) {
            continue;
        }
        if let Some(metadata) = index
            .get_asset_metadata(&file.content_hash)
            .map_err(|error| error.to_string())?
        {
            cached_metadata.insert(file.content_hash.clone(), metadata);
        }
    }
    let (metadata_by_path, metadata_warnings) = metadata::collect_metadata_with_cache(
        &scan.files,
        &metadata::FfprobeMetadataProbe::default(),
        &cached_metadata,
    );
    scan.warnings.extend(metadata_warnings);
    index
        .reconcile_under_root(&scan, &metadata_by_path, Path::new(&path))
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn search_media(
    app: tauri::AppHandle,
    query: local_index::SearchQuery,
) -> Result<Vec<local_index::SearchResult>, String> {
    tauri::async_runtime::spawn_blocking(move || search_media_blocking(app, query))
        .await
        .map_err(|error| format!("local search worker failed: {error}"))?
}

fn search_media_blocking(
    app: tauri::AppHandle,
    query: local_index::SearchQuery,
) -> Result<Vec<local_index::SearchResult>, String> {
    open_local_index(&app)?
        .search(&query)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn get_indexed_library_path(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let files = open_local_index(&app)?
        .known_files()
        .map_err(|error| error.to_string())?;
    Ok(common_library_root(&files).map(|path| path.to_string_lossy().into_owned()))
}

fn common_library_root(files: &[local_index::IndexedFile]) -> Option<PathBuf> {
    let mut root = Path::new(&files.first()?.path).parent()?.to_path_buf();
    while !files
        .iter()
        .all(|file| Path::new(&file.path).starts_with(&root))
    {
        if !root.pop() {
            return None;
        }
    }
    Some(root)
}

#[tauri::command]
async fn analyze_media_folder(
    app: tauri::AppHandle,
    path: String,
    config: Option<ai::AiRequestConfig>,
    force: Option<bool>,
) -> Result<ai::AiIndexReport, String> {
    let control = app.state::<AiAnalysisControl>().inner().clone();
    let run_guard = control.begin()?;
    let worker_control = control.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _run_guard = run_guard;
        analyze_media_folder_blocking(app, path, config, force.unwrap_or(false), &worker_control)
    })
    .await
    .map_err(|error| format!("AI analysis worker failed: {error}"))?
}

#[tauri::command]
fn plan_ai_analysis(
    app: tauri::AppHandle,
    path: String,
    config: Option<ai::AiRequestConfig>,
    force: Option<bool>,
) -> Result<AiAnalysisPlan, String> {
    let settings = ai_settings(&app, config)?;
    let index = open_local_index(&app)?;
    let indexed_files = unique_indexed_files_under_root(
        index.known_files().map_err(|error| error.to_string())?,
        Path::new(&path),
    );
    if indexed_files.is_empty() {
        return Err("No indexed active clips were found in the selected folder".to_owned());
    }
    let model = settings.model_namespace();
    let total_file_count = indexed_files.len() as u64;
    let already_analyzed_file_count = count_already_analyzed(&index, &indexed_files, &model)?;
    let (files, skipped_file_count) =
        select_ai_files(&index, indexed_files, &model, force.unwrap_or(false))?;
    let analyze_file_count = files.len() as u64;
    let max_frames_per_file = settings.max_frames_per_file() as u64;
    let vision_batch_size = settings.vision_batch_size() as u64;
    let requests_per_file = max_frames_per_file.div_ceil(vision_batch_size);
    let mut estimated_sampled_frames = 0u64;
    let mut estimated_vision_requests = 0u64;
    let mut estimated_audio_seconds = 0u64;
    for file in &files {
        let metadata = index
            .get_asset_metadata(&file.content_hash)
            .map_err(|error| error.to_string())?;
        let duration_ms = metadata.as_ref().and_then(|metadata| metadata.duration_ms);
        let sampled_frames = duration_ms
            .map(|duration| {
                duration
                    .max(1)
                    .div_ceil(settings.sample_interval_ms().max(1))
                    .max(1)
                    .min(max_frames_per_file)
            })
            .unwrap_or(max_frames_per_file);
        estimated_sampled_frames = estimated_sampled_frames.saturating_add(sampled_frames);
        estimated_vision_requests =
            estimated_vision_requests.saturating_add(sampled_frames.div_ceil(vision_batch_size));
        if settings.transcribes_audio()
            && metadata
                .as_ref()
                .and_then(|metadata| metadata.audio_codec.as_deref())
                .is_some()
        {
            let configured_span_ms = settings
                .sample_interval_ms()
                .saturating_mul(max_frames_per_file);
            estimated_audio_seconds = estimated_audio_seconds.saturating_add(
                duration_ms
                    .unwrap_or(configured_span_ms)
                    .min(configured_span_ms)
                    .div_ceil(1_000),
            );
        }
    }

    Ok(AiAnalysisPlan {
        total_file_count,
        analyze_file_count,
        skipped_file_count,
        already_analyzed_file_count,
        max_frames_per_file,
        max_sampled_frames: analyze_file_count.saturating_mul(max_frames_per_file),
        max_vision_requests: analyze_file_count.saturating_mul(requests_per_file),
        estimated_sampled_frames,
        estimated_vision_requests,
        estimated_audio_seconds,
        model,
    })
}

#[tauri::command]
fn cancel_ai_analysis(control: tauri::State<'_, AiAnalysisControl>) -> bool {
    control.request_cancel()
}

fn analyze_media_folder_blocking(
    app: tauri::AppHandle,
    path: String,
    config: Option<ai::AiRequestConfig>,
    force: bool,
    control: &AiAnalysisControl,
) -> Result<ai::AiIndexReport, String> {
    let settings = ai_settings(&app, config)?;
    let mut index = open_local_index(&app)?;
    let root = Path::new(&path);
    let indexed_files = unique_indexed_files_under_root(
        index.known_files().map_err(|error| error.to_string())?,
        root,
    );
    if indexed_files.is_empty() {
        return Err("No indexed active clips were found in the selected folder".to_owned());
    }

    let provider = settings.model_namespace();
    let (files, skipped_file_count) = select_ai_files(&index, indexed_files, &provider, force)?;

    let mut report = ai::AiIndexReport {
        analyzed_file_count: 0,
        skipped_file_count,
        annotation_count: 0,
        cancelled: false,
        warnings: Vec::new(),
    };
    let total_files = files.len() as u64;
    emit_ai_progress(
        &app,
        0,
        total_files,
        path.clone(),
        provider.clone(),
        0,
        "Preparing clips",
    );

    let mut tasks = VecDeque::with_capacity(files.len());
    for (file_index, file) in files.into_iter().enumerate() {
        let metadata = index
            .get_asset_metadata(&file.content_hash)
            .map_err(|error| error.to_string())?;
        tasks.push_back((file_index, file, metadata));
    }

    let worker_count = settings.parallel_file_limit().min(tasks.len());
    let tasks = Arc::new(Mutex::new(tasks));
    let file_progress = Arc::new(Mutex::new(vec![0u8; total_files as usize]));
    let completed_files = Arc::new(AtomicU64::new(0));
    let (sender, receiver) = mpsc::channel();

    std::thread::scope(|scope| {
        for _ in 0..worker_count {
            let worker_tasks = Arc::clone(&tasks);
            let worker_progress = Arc::clone(&file_progress);
            let worker_completed = Arc::clone(&completed_files);
            let worker_sender = sender.clone();
            let worker_settings = settings.clone();
            let worker_provider = provider.clone();
            let worker_app = app.clone();
            let worker_control = control.clone();

            scope.spawn(move || loop {
                if worker_control.is_cancelled() {
                    break;
                }
                let task = lock_unpoisoned(&worker_tasks).pop_front();
                let Some((file_index, file, metadata)) = task else {
                    break;
                };
                let current_file = file.path.clone();
                let result = ai::analyze_file_with_progress_and_cancel(
                    Path::new(&current_file),
                    metadata.as_ref(),
                    &worker_settings,
                    |progress| {
                        let percent =
                            update_overall_progress(&worker_progress, file_index, progress.percent);
                        emit_ai_progress(
                            &worker_app,
                            worker_completed.load(Ordering::Relaxed),
                            total_files,
                            current_file.clone(),
                            worker_provider.clone(),
                            percent,
                            progress.phase,
                        );
                    },
                    || worker_control.is_cancelled(),
                );
                if matches!(&result, Err(error) if error == ai::AI_ANALYSIS_CANCELLED_MESSAGE) {
                    let _ = worker_sender.send((file_index, file, result));
                    break;
                }
                let percent = update_overall_progress(&worker_progress, file_index, 100);
                let completed = worker_completed.fetch_add(1, Ordering::Relaxed) + 1;
                let phase = if result.is_ok() {
                    "Clip finished"
                } else {
                    "Clip finished with a warning"
                };
                emit_ai_progress(
                    &worker_app,
                    completed,
                    total_files,
                    current_file,
                    worker_provider.clone(),
                    percent,
                    phase,
                );
                if worker_sender.send((file_index, file, result)).is_err() {
                    break;
                }
            });
        }
    });
    drop(sender);

    let mut results = receiver.into_iter().collect::<Vec<_>>();
    results.sort_by_key(|(file_index, _, _)| *file_index);
    for (_, file, result) in results {
        match result {
            Ok(annotations) => {
                report.analyzed_file_count += 1;
                report.annotation_count += annotations.len() as u64;
                index
                    .replace_ai_annotations(&file.content_hash, &annotations)
                    .map_err(|error| error.to_string())?;
            }
            Err(error) if error == ai::AI_ANALYSIS_CANCELLED_MESSAGE => {
                report.cancelled = true;
            }
            Err(error) => report.warnings.push(ai::AiWarning {
                path: file.path,
                message: error,
            }),
        }
    }

    report.cancelled |= control.is_cancelled();

    if !report.cancelled {
        emit_ai_progress(
            &app,
            total_files,
            total_files,
            path,
            provider,
            100,
            "Analysis complete",
        );
    }

    Ok(report)
}

fn unique_indexed_files_under_root(
    files: Vec<local_index::IndexedFile>,
    root: &Path,
) -> Vec<local_index::IndexedFile> {
    let mut content_hashes = HashSet::new();
    files
        .into_iter()
        .filter(|file| Path::new(&file.path).starts_with(root))
        .filter(|file| content_hashes.insert(file.content_hash.clone()))
        .collect()
}

fn select_ai_files(
    index: &local_index::SqliteIndex,
    indexed_files: Vec<local_index::IndexedFile>,
    model: &str,
    force: bool,
) -> Result<(Vec<local_index::IndexedFile>, u64), String> {
    let mut files = Vec::with_capacity(indexed_files.len());
    let mut skipped_file_count = 0u64;
    for file in indexed_files {
        let already_analyzed = !force
            && index
                .has_ai_annotations_for_content_model(&file.content_hash, model)
                .map_err(|error| error.to_string())?;
        if already_analyzed {
            skipped_file_count += 1;
        } else {
            files.push(file);
        }
    }
    Ok((files, skipped_file_count))
}

fn count_already_analyzed(
    index: &local_index::SqliteIndex,
    indexed_files: &[local_index::IndexedFile],
    model: &str,
) -> Result<u64, String> {
    let mut count = 0u64;
    for file in indexed_files {
        if index
            .has_ai_annotations_for_content_model(&file.content_hash, model)
            .map_err(|error| error.to_string())?
        {
            count += 1;
        }
    }
    Ok(count)
}

fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn update_overall_progress(progress: &Mutex<Vec<u8>>, file_index: usize, percent: u8) -> u8 {
    let mut file_progress = lock_unpoisoned(progress);
    file_progress[file_index] = file_progress[file_index].max(percent.min(100));
    let total = file_progress
        .iter()
        .map(|value| u64::from(*value))
        .sum::<u64>();
    (total / file_progress.len() as u64) as u8
}

fn emit_ai_progress(
    app: &tauri::AppHandle,
    completed_files: u64,
    total_files: u64,
    current_file: String,
    provider: String,
    percent: u8,
    phase: &str,
) {
    let _ = app.emit(
        "ai-progress",
        ai::AiProgress {
            completed_files,
            total_files,
            current_file,
            provider,
            percent: percent.min(100),
            phase: phase.to_owned(),
        },
    );
}

#[tauri::command]
async fn search_ai(
    app: tauri::AppHandle,
    query: String,
    config: Option<ai::AiRequestConfig>,
    focus: Option<local_index::AiSearchFocus>,
    root: Option<String>,
) -> Result<Vec<local_index::AiSearchResult>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        search_ai_blocking(app, query, config, focus, root)
    })
    .await
    .map_err(|error| format!("AI search worker failed: {error}"))?
}

fn search_ai_blocking(
    app: tauri::AppHandle,
    query: String,
    config: Option<ai::AiRequestConfig>,
    focus: Option<local_index::AiSearchFocus>,
    root: Option<String>,
) -> Result<Vec<local_index::AiSearchResult>, String> {
    let settings = ai_settings(&app, config)?;
    let model_namespace = settings.model_namespace();
    let index = open_local_index(&app)?;
    let root_path = root
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(Path::new);
    if index
        .ai_annotation_count_for_model_under_root(&model_namespace, root_path)
        .map_err(|error| error.to_string())?
        == 0
    {
        return Err(format!(
            "No AI moments indexed for {model_namespace}. Analyze the folder with this provider and model first."
        ));
    }
    let embedding = ai::embed_query(&query, &settings)?;
    index
        .search_ai_with_focus_under_root(
            &query,
            &embedding,
            100,
            Some(&model_namespace),
            focus.unwrap_or_default(),
            root_path,
        )
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn get_saved_ai_moments(
    app: tauri::AppHandle,
    path: String,
) -> Result<Vec<local_index::SavedAiMoment>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        open_local_index(&app)?
            .saved_ai_moments_for_path(&path)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("saved AI analysis worker failed: {error}"))?
}

#[tauri::command]
async fn get_ai_thumbnail(
    app: tauri::AppHandle,
    path: String,
    timestamp_ms: u64,
    ffmpeg_path: Option<String>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        get_ai_thumbnail_blocking(app, path, timestamp_ms, ffmpeg_path)
    })
    .await
    .map_err(|error| format!("thumbnail worker failed: {error}"))?
}

fn get_ai_thumbnail_blocking(
    app: tauri::AppHandle,
    path: String,
    timestamp_ms: u64,
    ffmpeg_path: Option<String>,
) -> Result<String, String> {
    let index = open_local_index(&app)?;
    let indexed_file = index
        .get_file(&path)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The thumbnail clip is not in the active local index".to_owned())?;
    if indexed_file.status != local_index::LocalFileStatus::Active || !Path::new(&path).is_file() {
        return Err("The thumbnail clip is no longer available".to_owned());
    }

    let cache_directory = app
        .path()
        .app_local_data_dir()
        .map_err(|error| format!("cannot determine thumbnail cache directory: {error}"))?
        .join("thumbnails");
    fs::create_dir_all(&cache_directory)
        .map_err(|error| format!("cannot create thumbnail cache directory: {error}"))?;
    let cache_path =
        cache_directory.join(format!("{}-{timestamp_ms}.jpg", indexed_file.content_hash));
    let thumbnail = match fs::read(&cache_path) {
        Ok(bytes) if !bytes.is_empty() => bytes,
        _ => {
            let executable = ai::resolve_ffmpeg_executable(ffmpeg_path);
            let bytes = ai::extract_thumbnail(Path::new(&path), timestamp_ms, &executable)?;
            let _ = fs::write(&cache_path, &bytes);
            bytes
        }
    };
    Ok(format!(
        "data:image/jpeg;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(thumbnail)
    ))
}

#[tauri::command]
async fn test_ai_connection(
    app: tauri::AppHandle,
    config: Option<ai::AiRequestConfig>,
) -> Result<ai::AiConnectionReport, String> {
    tauri::async_runtime::spawn_blocking(move || test_ai_connection_blocking(app, config))
        .await
        .map_err(|error| format!("AI connection worker failed: {error}"))?
}

fn test_ai_connection_blocking(
    app: tauri::AppHandle,
    config: Option<ai::AiRequestConfig>,
) -> Result<ai::AiConnectionReport, String> {
    let settings = ai_settings(&app, config)?;
    ai::test_connection(&settings)
}

fn ai_settings(
    app: &tauri::AppHandle,
    config: Option<ai::AiRequestConfig>,
) -> Result<ai::AiSettings, String> {
    let config = app
        .state::<gemini_oauth::GeminiOAuthSession>()
        .resolve_config(config)?;
    ai::AiSettings::from_request(config)
}

#[tauri::command]
async fn login_gemini_oauth(
    app: tauri::AppHandle,
    client_file_path: String,
) -> Result<gemini_oauth::GeminiOAuthStatus, String> {
    let session = app
        .state::<gemini_oauth::GeminiOAuthSession>()
        .inner()
        .clone();
    tauri::async_runtime::spawn_blocking(move || session.login(&app, Path::new(&client_file_path)))
        .await
        .map_err(|error| format!("Google login worker failed: {error}"))?
}

#[tauri::command]
fn get_gemini_oauth_status(
    session: tauri::State<'_, gemini_oauth::GeminiOAuthSession>,
) -> gemini_oauth::GeminiOAuthStatus {
    session.status()
}

#[tauri::command]
fn logout_gemini_oauth(
    session: tauri::State<'_, gemini_oauth::GeminiOAuthSession>,
) -> gemini_oauth::GeminiOAuthStatus {
    session.logout()
}

#[tauri::command]
fn open_indexed_media_path(app: tauri::AppHandle, path: String) -> Result<(), String> {
    let path = active_indexed_media_path(&app, &path)?;

    app.opener()
        .open_path(path.to_string_lossy().into_owned(), None::<String>)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn prepare_indexed_media_preview(app: tauri::AppHandle, path: String) -> Result<String, String> {
    let path = active_indexed_media_path(&app, &path)?;
    app.asset_protocol_scope()
        .allow_file(&path)
        .map_err(|error| format!("cannot authorize the indexed clip for preview: {error}"))?;
    Ok(path.to_string_lossy().into_owned())
}

fn active_indexed_media_path(app: &tauri::AppHandle, path: &str) -> Result<PathBuf, String> {
    let index = open_local_index(&app)?;
    let indexed_file = index
        .get_file(path)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The selected clip is not in the active local index".to_owned())?;

    if indexed_file.status != local_index::LocalFileStatus::Active {
        return Err("The selected clip is no longer active in the local index".to_owned());
    }
    let indexed_path = PathBuf::from(indexed_file.path);
    if !indexed_path.is_file() {
        return Err("The selected clip is no longer available at this path".to_owned());
    }
    Ok(indexed_path)
}

fn open_local_index(app: &tauri::AppHandle) -> Result<local_index::SqliteIndex, String> {
    let database_directory = app
        .path()
        .app_local_data_dir()
        .map_err(|error| format!("cannot determine local index directory: {error}"))?;
    fs::create_dir_all(&database_directory)
        .map_err(|error| format!("cannot create local index directory: {error}"))?;
    let database_path: PathBuf = database_directory.join("mediaindex.sqlite3");
    local_index::SqliteIndex::open(database_path).map_err(|error| error.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AiAnalysisControl::default())
        .manage(gemini_oauth::GeminiOAuthSession::default())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            scan_media_folder,
            extract_media_metadata,
            index_media_folder,
            search_media,
            get_indexed_library_path,
            plan_ai_analysis,
            analyze_media_folder,
            cancel_ai_analysis,
            search_ai,
            get_saved_ai_moments,
            get_ai_thumbnail,
            test_ai_connection,
            login_gemini_oauth,
            get_gemini_oauth_status,
            logout_gemini_oauth,
            open_indexed_media_path,
            prepare_indexed_media_preview
        ])
        .run(tauri::generate_context!())
        .expect("error while running MediaIndex");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overall_ai_progress_is_averaged_and_monotonic() {
        let progress = Mutex::new(vec![0, 0, 0, 0]);

        assert_eq!(update_overall_progress(&progress, 0, 10), 2);
        assert_eq!(update_overall_progress(&progress, 1, 50), 15);
        assert_eq!(update_overall_progress(&progress, 1, 40), 15);
        assert_eq!(update_overall_progress(&progress, 2, 100), 40);
        assert_eq!(update_overall_progress(&progress, 3, 100), 65);
        assert_eq!(update_overall_progress(&progress, 0, 100), 87);
        assert_eq!(update_overall_progress(&progress, 1, 100), 100);
    }

    #[test]
    fn analysis_control_prevents_overlap_and_resets_after_cancellation() {
        let control = AiAnalysisControl::default();
        let guard = control.begin().expect("first analysis should start");

        assert!(control.begin().is_err());
        assert!(control.request_cancel());
        assert!(control.is_cancelled());

        drop(guard);
        assert!(!control.request_cancel());
        assert!(!control.is_cancelled());
        assert!(control.begin().is_ok());
    }

    #[test]
    fn ai_analysis_deduplicates_content_inside_the_selected_root() {
        let files = vec![
            local_index::IndexedFile {
                path: "/library/a/clip.mp4".to_owned(),
                content_hash: "same-content".to_owned(),
                size_bytes: 10,
                modified_unix_ms: None,
                status: local_index::LocalFileStatus::Active,
            },
            local_index::IndexedFile {
                path: "/library/b/copy.mp4".to_owned(),
                content_hash: "same-content".to_owned(),
                size_bytes: 10,
                modified_unix_ms: None,
                status: local_index::LocalFileStatus::Active,
            },
            local_index::IndexedFile {
                path: "/library/b/unique.mp4".to_owned(),
                content_hash: "unique-content".to_owned(),
                size_bytes: 20,
                modified_unix_ms: None,
                status: local_index::LocalFileStatus::Active,
            },
            local_index::IndexedFile {
                path: "/outside/other.mp4".to_owned(),
                content_hash: "outside-content".to_owned(),
                size_bytes: 30,
                modified_unix_ms: None,
                status: local_index::LocalFileStatus::Active,
            },
        ];

        let selected = unique_indexed_files_under_root(files, Path::new("/library"));

        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].path, "/library/a/clip.mp4");
        assert_eq!(selected[1].path, "/library/b/unique.mp4");
    }

    #[test]
    fn derives_the_common_active_library_root() {
        let files = vec![
            local_index::IndexedFile {
                path: "/library/fortnite/clip-a.mp4".to_owned(),
                content_hash: "hash-a".to_owned(),
                size_bytes: 10,
                modified_unix_ms: None,
                status: local_index::LocalFileStatus::Active,
            },
            local_index::IndexedFile {
                path: "/library/fortnite/day-two/clip-b.mp4".to_owned(),
                content_hash: "hash-b".to_owned(),
                size_bytes: 20,
                modified_unix_ms: None,
                status: local_index::LocalFileStatus::Active,
            },
        ];

        assert_eq!(
            common_library_root(&files),
            Some(PathBuf::from("/library/fortnite"))
        );
        assert_eq!(common_library_root(&[]), None);
    }
}
