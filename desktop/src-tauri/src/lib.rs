pub mod ai;
pub mod cost;
pub mod gemini_oauth;
pub mod local_index;
pub mod metadata;
pub mod pricing;
pub mod scanner;
pub mod usage;

use base64::Engine;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
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
    resumable_checkpoint_file_count: u64,
    available_vision_frame_count: u64,
    available_vision_request_count: u64,
    reused_vision_frame_count: u64,
    reused_vision_request_count: u64,
    remaining_vision_frame_count: u64,
    remaining_vision_request_count: u64,
    partial_file_count: u64,
    coverage_unknown_file_count: u64,
    requires_explicit_coverage_confirmation: bool,
    max_frames_per_file: u64,
    max_sampled_frames: u64,
    max_vision_requests: u64,
    estimated_sampled_frames: u64,
    estimated_vision_requests: u64,
    estimated_audio_seconds: u64,
    estimated_cost: cost::AiCostEstimate,
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
                    identity_verified: file.identity_verified,
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
    resume_checkpoints: Option<bool>,
) -> Result<ai::AiIndexReport, String> {
    let control = app.state::<AiAnalysisControl>().inner().clone();
    let run_guard = control.begin()?;
    let worker_control = control.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _run_guard = run_guard;
        analyze_media_folder_blocking(
            app,
            path,
            config,
            force.unwrap_or(false),
            resume_checkpoints.unwrap_or(false),
            &worker_control,
        )
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
    resume_checkpoints: Option<bool>,
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
    ensure_ai_identity_verified(&indexed_files)?;
    let model = settings.model_namespace();
    let settings_fingerprint = settings.analysis_settings_fingerprint();
    let force_reanalysis = force.unwrap_or(false);
    let resume_checkpoints = resume_checkpoints.unwrap_or(false) && !force_reanalysis;
    let total_file_count = indexed_files.len() as u64;
    let already_analyzed_file_count = count_already_analyzed(&index, &indexed_files, &model)?;
    let checkpoint_summaries =
        ai_vision_checkpoint_summaries(&index, &indexed_files, &settings, &settings_fingerprint)?;
    let resumable_checkpoint_file_count = checkpoint_summaries
        .values()
        .filter(|summary| summary.reusable_batch_count > 0)
        .count() as u64;
    let resumable_content_hashes = checkpoint_summaries
        .iter()
        .filter(|(_, summary)| summary.reusable_batch_count > 0)
        .map(|(content_hash, _)| content_hash.clone())
        .collect::<HashSet<_>>();
    let selection = select_ai_files_with_options(
        &index,
        indexed_files,
        &model,
        &settings_fingerprint,
        force_reanalysis,
        resume_checkpoints,
        &resumable_content_hashes,
    )?;
    let files = selection.files;
    let skipped_file_count = selection.skipped_file_count;
    let analyze_file_count = files.len() as u64;
    let max_frames_per_file = settings.max_frames_per_file() as u64;
    let vision_batch_size = settings.vision_batch_size() as u64;
    let requests_per_file = max_frames_per_file.div_ceil(vision_batch_size);
    let mut estimated_sampled_frames = 0u64;
    let mut estimated_vision_requests = 0u64;
    let mut estimated_audio_seconds = 0u64;
    let available_vision_frame_count = checkpoint_summaries
        .values()
        .map(|summary| summary.reusable_frame_count)
        .sum::<u64>();
    let available_vision_request_count = checkpoint_summaries
        .values()
        .map(|summary| summary.reusable_batch_count)
        .sum::<u64>();
    let mut reused_vision_frame_count = 0u64;
    let mut reused_vision_request_count = 0u64;
    let mut remaining_vision_frame_count = 0u64;
    let mut remaining_vision_request_count = 0u64;
    let mut cost_files = Vec::with_capacity(files.len());
    for file in &files {
        let metadata = index
            .get_asset_metadata(&file.content_hash)
            .map_err(|error| error.to_string())?;
        let duration_ms = metadata.as_ref().and_then(|metadata| metadata.duration_ms);
        let available_checkpoint_summary = checkpoint_summaries
            .get(&file.content_hash)
            .cloned()
            .unwrap_or_default();
        let sampled_frames = if resume_checkpoints && available_checkpoint_summary.frame_count > 0 {
            available_checkpoint_summary.frame_count
        } else {
            estimated_sampled_frame_count(duration_ms, &settings)
        };
        estimated_sampled_frames = estimated_sampled_frames.saturating_add(sampled_frames);
        let vision_requests = sampled_frames.div_ceil(vision_batch_size);
        estimated_vision_requests = estimated_vision_requests.saturating_add(vision_requests);
        let checkpoint_summary = if resume_checkpoints {
            available_checkpoint_summary
        } else {
            local_index::AiVisionCheckpointSummary::default()
        };
        let reused_frames = checkpoint_summary.reusable_frame_count.min(sampled_frames);
        let reused_requests = checkpoint_summary.reusable_batch_count.min(vision_requests);
        let remaining_frames = sampled_frames.saturating_sub(reused_frames);
        let remaining_requests = vision_requests.saturating_sub(reused_requests);
        reused_vision_frame_count = reused_vision_frame_count.saturating_add(reused_frames);
        reused_vision_request_count = reused_vision_request_count.saturating_add(reused_requests);
        remaining_vision_frame_count =
            remaining_vision_frame_count.saturating_add(remaining_frames);
        remaining_vision_request_count =
            remaining_vision_request_count.saturating_add(remaining_requests);
        cost_files.push(cost::CostFile {
            sampled_frames,
            vision_sampled_frames: remaining_frames,
            vision_requests: remaining_requests,
            width: metadata.as_ref().and_then(|metadata| metadata.width),
            height: metadata.as_ref().and_then(|metadata| metadata.height),
        });
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
    let estimated_cost = cost::estimate(cost::CostInput {
        provider: settings.provider_name().to_owned(),
        vision_model: settings.vision_model().to_owned(),
        embedding_model: settings.embedding_model().to_owned(),
        transcription_model: settings.transcription_model().to_owned(),
        transcribes_audio: settings.transcribes_audio(),
        audio_seconds: estimated_audio_seconds,
        budget_limit_usd: settings.budget_usd(),
        files: cost_files,
    });

    Ok(AiAnalysisPlan {
        total_file_count,
        analyze_file_count,
        skipped_file_count,
        already_analyzed_file_count,
        resumable_checkpoint_file_count,
        available_vision_frame_count,
        available_vision_request_count,
        reused_vision_frame_count,
        reused_vision_request_count,
        remaining_vision_frame_count,
        remaining_vision_request_count,
        partial_file_count: selection.partial_file_count,
        coverage_unknown_file_count: selection.coverage_unknown_file_count,
        requires_explicit_coverage_confirmation: requires_explicit_coverage_confirmation(
            force_reanalysis,
            selection.partial_file_count,
            selection.coverage_unknown_file_count,
        ),
        max_frames_per_file,
        max_sampled_frames: analyze_file_count.saturating_mul(max_frames_per_file),
        max_vision_requests: analyze_file_count.saturating_mul(requests_per_file),
        estimated_sampled_frames,
        estimated_vision_requests,
        estimated_audio_seconds,
        estimated_cost,
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
    resume_checkpoints: bool,
    control: &AiAnalysisControl,
) -> Result<ai::AiIndexReport, String> {
    let settings = ai_settings(&app, config.clone())?;
    let mut index = open_local_index(&app)?;
    let plan = plan_ai_analysis(
        app.clone(),
        path.clone(),
        config.clone(),
        Some(force),
        Some(resume_checkpoints),
    )?;
    validate_ai_budget(&plan)?;
    let root = Path::new(&path);
    let indexed_files = unique_indexed_files_under_root(
        index.known_files().map_err(|error| error.to_string())?,
        root,
    );
    if indexed_files.is_empty() {
        return Err("No indexed active clips were found in the selected folder".to_owned());
    }
    ensure_ai_identity_verified(&indexed_files)?;

    let provider = settings.model_namespace();
    let settings_fingerprint = settings.analysis_settings_fingerprint();
    let checkpoint_summaries =
        ai_vision_checkpoint_summaries(&index, &indexed_files, &settings, &settings_fingerprint)?;
    let resumable_content_hashes = checkpoint_summaries
        .iter()
        .filter(|(_, summary)| summary.reusable_batch_count > 0)
        .map(|(content_hash, _)| content_hash.clone())
        .collect::<HashSet<_>>();
    let selection = select_ai_files_with_options(
        &index,
        indexed_files,
        &provider,
        &settings_fingerprint,
        force,
        resume_checkpoints && !force,
        &resumable_content_hashes,
    )?;
    let files = selection.files;
    let skipped_file_count = selection.skipped_file_count;
    let run_id = new_ai_run_id();
    let recorder = usage::AiUsageRecorder::new(
        run_id.clone(),
        settings.provider_name(),
        plan.estimated_cost.pricing_status,
        plan.estimated_cost.pricing_checked_at,
    );
    let usage_recorder = match (
        plan.estimated_cost.budget_limit_usd,
        plan.estimated_cost.pricing_status,
    ) {
        (Some(limit), "known") => usage::AiBudgetGate::new(limit)
            .map(|gate| recorder.clone().with_budget_gate(gate))
            .unwrap_or(recorder),
        _ => recorder,
    };
    index
        .start_ai_analysis_run(&usage::AiRunSpec {
            run_id: run_id.clone(),
            operation: "analyze_media_folder".to_owned(),
            provider: settings.provider_name().to_owned(),
            vision_model: settings.vision_model().to_owned(),
            embedding_model: settings.embedding_model().to_owned(),
            transcription_model: settings
                .transcribes_audio()
                .then(|| settings.transcription_model().to_owned()),
            model_namespace: provider.clone(),
            pricing_status: plan.estimated_cost.pricing_status.to_owned(),
            pricing_checked_at: plan.estimated_cost.pricing_checked_at.to_owned(),
            estimated_cost_usd: plan.estimated_cost.estimated_likely_usd,
            budget_limit_usd: plan.estimated_cost.budget_limit_usd,
        })
        .map_err(|error| error.to_string())?;

    let mut report = ai::AiIndexReport {
        analyzed_file_count: 0,
        skipped_file_count,
        annotation_count: 0,
        partial_file_count: 0,
        failed_file_count: 0,
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
    let persisted_files = Arc::new(AtomicU64::new(0));
    let (sender, receiver) = mpsc::channel::<AiWorkerMessage>();

    let worker_result = std::thread::scope(|scope| {
        for _ in 0..worker_count {
            let worker_tasks = Arc::clone(&tasks);
            let worker_progress = Arc::clone(&file_progress);
            let worker_persisted = Arc::clone(&persisted_files);
            let worker_sender = sender.clone();
            let worker_settings = settings.clone().with_usage_recorder(usage_recorder.clone());
            let worker_provider = provider.clone();
            let worker_app = app.clone();
            let worker_control = control.clone();
            let checkpoint_load_sender = worker_sender.clone();
            let checkpoint_store_sender = worker_sender.clone();
            let worker_settings_fingerprint = settings_fingerprint.clone();
            let allow_checkpoint_resume = resume_checkpoints && !force;
            let worker_resumable_content_hashes = resumable_content_hashes.clone();

            scope.spawn(move || loop {
                if worker_control.is_cancelled() {
                    break;
                }
                let task = lock_unpoisoned(&worker_tasks).pop_front();
                let Some((file_index, file, metadata)) = task else {
                    break;
                };
                let current_file = file.path.clone();
                let checkpoint_load_content_hash = file.content_hash.clone();
                let checkpoint_load_settings_fingerprint = worker_settings_fingerprint.clone();
                let checkpoint_store_content_hash = file.content_hash.clone();
                let checkpoint_store_settings_fingerprint = worker_settings_fingerprint.clone();
                let task_checkpoint_load_sender = checkpoint_load_sender.clone();
                let task_checkpoint_store_sender = checkpoint_store_sender.clone();
                let require_checkpoint_match = allow_checkpoint_resume
                    && worker_resumable_content_hashes.contains(&file.content_hash);
                let result = ai::analyze_file_with_progress_and_cancel_with_checkpoints(
                    Path::new(&current_file),
                    metadata.as_ref(),
                    &worker_settings,
                    |progress| {
                        let percent =
                            update_overall_progress(&worker_progress, file_index, progress.percent);
                        emit_ai_progress(
                            &worker_app,
                            worker_persisted.load(Ordering::Relaxed),
                            total_files,
                            current_file.clone(),
                            worker_provider.clone(),
                            percent,
                            progress.phase,
                        );
                    },
                    || worker_control.is_cancelled(),
                    move |frame_timestamps, batch_size| {
                        if !allow_checkpoint_resume {
                            return Ok(Vec::new());
                        }
                        let (acknowledgement_sender, acknowledgement_receiver) = mpsc::channel();
                        task_checkpoint_load_sender
                            .send(AiWorkerMessage::LoadVisionCheckpoints {
                                content_hash: checkpoint_load_content_hash.clone(),
                                settings_fingerprint: checkpoint_load_settings_fingerprint.clone(),
                                frame_timestamps: frame_timestamps.to_vec(),
                                batch_size,
                                acknowledgement: acknowledgement_sender,
                            })
                            .map_err(|_| "AI analysis worker channel closed".to_owned())?;
                        let checkpoints = acknowledgement_receiver
                            .recv()
                            .map_err(|_| "AI checkpoint reader stopped responding".to_owned())?;
                        let checkpoints = checkpoints?;
                        if require_checkpoint_match && checkpoints.is_empty() {
                            return Err(
                                "saved vision checkpoint plan no longer matches the frames extracted from this clip; review the analysis estimate before continuing"
                                    .to_owned(),
                            );
                        }
                        Ok(checkpoints)
                    },
                    move |checkpoint| {
                        let (acknowledgement_sender, acknowledgement_receiver) = mpsc::channel();
                        task_checkpoint_store_sender
                            .send(AiWorkerMessage::StoreVisionCheckpoint {
                                content_hash: checkpoint_store_content_hash.clone(),
                                settings_fingerprint: checkpoint_store_settings_fingerprint.clone(),
                                checkpoint,
                                acknowledgement: acknowledgement_sender,
                            })
                            .map_err(|_| "AI analysis worker channel closed".to_owned())?;
                        acknowledgement_receiver
                            .recv()
                            .map_err(|_| "AI checkpoint writer stopped responding".to_owned())?
                    },
                );
                if matches!(&result, Err(error) if error == ai::AI_ANALYSIS_CANCELLED_MESSAGE) {
                    let _ = worker_sender.send(AiWorkerMessage::Result {
                        file_index,
                        file,
                        result,
                    });
                    break;
                }
                if worker_sender
                    .send(AiWorkerMessage::Result {
                        file_index,
                        file,
                        result,
                    })
                    .is_err()
                {
                    break;
                }
            });
        }
        drop(sender);

        let mut fatal_error = None;
        while let Ok(message) = receiver.recv() {
            let Some(message) =
                handle_ai_checkpoint_message(message, &mut index, control, &mut fatal_error)
            else {
                continue;
            };
            let AiWorkerMessage::Result {
                file_index,
                file,
                result,
            } = message
            else {
                unreachable!("checkpoint handler must consume checkpoint messages");
            };
            if fatal_error.is_some() {
                continue;
            }
            if let Err(error) = persist_ai_usage_events(&mut index, &usage_recorder) {
                fatal_error.get_or_insert(error);
                control.request_cancel();
                continue;
            }
            let outcome = match persist_ai_result(
                &mut index,
                &mut report,
                &file,
                &provider,
                &run_id,
                &settings_fingerprint,
                result,
                &persisted_files,
            ) {
                Ok(outcome) => outcome,
                Err(error) => {
                    fatal_error.get_or_insert(error);
                    control.request_cancel();
                    continue;
                }
            };
            let percent = update_overall_progress(&file_progress, file_index, 100);
            match outcome {
                AiResultOutcome::Committed => emit_ai_progress(
                    &app,
                    persisted_files.load(Ordering::Relaxed),
                    total_files,
                    file.path,
                    provider.clone(),
                    percent,
                    "Clip saved",
                ),
                AiResultOutcome::Warning => emit_ai_progress(
                    &app,
                    persisted_files.load(Ordering::Relaxed),
                    total_files,
                    file.path,
                    provider.clone(),
                    percent,
                    "Clip finished with a warning",
                ),
                AiResultOutcome::Partial => emit_ai_progress(
                    &app,
                    persisted_files.load(Ordering::Relaxed),
                    total_files,
                    file.path,
                    provider.clone(),
                    percent,
                    "Clip saved with partial coverage",
                ),
                AiResultOutcome::Cancelled => {}
            }
        }
        if let Some(error) = fatal_error {
            Err(error)
        } else {
            Ok(())
        }
    });

    persist_ai_usage_events(&mut index, &usage_recorder)?;

    report.cancelled |= control.is_cancelled();
    let run_status = match &worker_result {
        Ok(()) if report.cancelled => "cancelled",
        Ok(()) if report.warnings.is_empty() && report.partial_file_count == 0 => "completed",
        Ok(()) => "partial",
        Err(_) => "failed",
    };
    index
        .finish_ai_analysis_run(
            &run_id,
            run_status,
            report.analyzed_file_count,
            report.annotation_count,
            usage_recorder.reserved_budget_usd(plan.estimated_cost.budget_limit_usd),
        )
        .map_err(|error| error.to_string())?;
    worker_result?;

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

enum AiWorkerMessage {
    LoadVisionCheckpoints {
        content_hash: String,
        settings_fingerprint: String,
        frame_timestamps: Vec<u64>,
        batch_size: usize,
        acknowledgement: mpsc::Sender<Result<Vec<ai::AiVisionCheckpointBatch>, String>>,
    },
    StoreVisionCheckpoint {
        content_hash: String,
        settings_fingerprint: String,
        checkpoint: ai::AiVisionCheckpointBatch,
        acknowledgement: mpsc::Sender<Result<(), String>>,
    },
    Result {
        file_index: usize,
        file: local_index::IndexedFile,
        result: Result<ai::AiFileAnalysisResult, String>,
    },
}

fn handle_ai_checkpoint_message(
    message: AiWorkerMessage,
    index: &mut local_index::SqliteIndex,
    control: &AiAnalysisControl,
    fatal_error: &mut Option<String>,
) -> Option<AiWorkerMessage> {
    match message {
        AiWorkerMessage::LoadVisionCheckpoints {
            content_hash,
            settings_fingerprint,
            frame_timestamps,
            batch_size,
            acknowledgement,
        } => {
            let result = index
                .load_ai_vision_checkpoints(
                    &content_hash,
                    &settings_fingerprint,
                    ai::AI_VISION_CHECKPOINT_VERSION,
                    &frame_timestamps,
                    batch_size,
                )
                .map_err(|error| error.to_string());
            if let Err(error) = &result {
                fatal_error.get_or_insert_with(|| error.clone());
                control.request_cancel();
            }
            let _ = acknowledgement.send(result);
            None
        }
        AiWorkerMessage::StoreVisionCheckpoint {
            content_hash,
            settings_fingerprint,
            checkpoint,
            acknowledgement,
        } => {
            let result = index
                .store_ai_vision_checkpoint(
                    &content_hash,
                    &settings_fingerprint,
                    ai::AI_VISION_CHECKPOINT_VERSION,
                    &checkpoint,
                )
                .map_err(|error| error.to_string());
            if let Err(error) = &result {
                fatal_error.get_or_insert_with(|| error.clone());
                control.request_cancel();
            }
            let _ = acknowledgement.send(result);
            None
        }
        message @ AiWorkerMessage::Result { .. } => Some(message),
    }
}

fn new_ai_run_id() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("run-{}-{timestamp}", std::process::id())
}

fn persist_ai_usage_events(
    index: &mut local_index::SqliteIndex,
    recorder: &usage::AiUsageRecorder,
) -> Result<(), String> {
    for event in recorder.drain() {
        index
            .record_ai_usage_event(&event)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn validate_ai_budget(plan: &AiAnalysisPlan) -> Result<(), String> {
    let cost = &plan.estimated_cost;
    if cost.budget_limit_usd.is_none() || cost.pricing_status == "local" {
        return Ok(());
    }
    match cost.budget_status {
        "exceeds_limit" => Err(
            "The conservative API cost estimate exceeds the configured budget; refresh the plan before starting analysis."
                .to_owned(),
        ),
        "unknown" => Err(
            "The configured budget cannot be enforced because at least one selected model has unknown pricing."
                .to_owned(),
        ),
        _ => Ok(()),
    }
}

#[derive(Debug, PartialEq, Eq)]
enum AiResultOutcome {
    Committed,
    Partial,
    Warning,
    Cancelled,
}

fn persist_ai_result(
    index: &mut local_index::SqliteIndex,
    report: &mut ai::AiIndexReport,
    file: &local_index::IndexedFile,
    model: &str,
    run_id: &str,
    settings_fingerprint: &str,
    result: Result<ai::AiFileAnalysisResult, String>,
    persisted_files: &AtomicU64,
) -> Result<AiResultOutcome, String> {
    match result {
        Ok(analysis) => {
            let stored_annotation_count = index
                .record_ai_analysis_result(
                    &file.content_hash,
                    model,
                    settings_fingerprint,
                    Some(run_id),
                    &analysis,
                )
                .map_err(|error| error.to_string())?;
            report.annotation_count += stored_annotation_count;
            persisted_files.fetch_add(1, Ordering::Relaxed);
            match analysis.status {
                ai::AiFileAnalysisStatus::Complete => {
                    report.analyzed_file_count += 1;
                    Ok(AiResultOutcome::Committed)
                }
                ai::AiFileAnalysisStatus::Partial => {
                    report.partial_file_count += 1;
                    report.warnings.push(ai::AiWarning {
                        path: file.path.clone(),
                        message: analysis.warning.unwrap_or_else(|| {
                            format!(
                                "Partial AI analysis for {}: {}/{} frames succeeded. Rerun explicitly to complete coverage.",
                                file.path,
                                analysis.successful_frame_count,
                                analysis.planned_frame_count
                            )
                        }),
                    });
                    Ok(AiResultOutcome::Partial)
                }
                ai::AiFileAnalysisStatus::Failed => {
                    report.failed_file_count += 1;
                    report.warnings.push(ai::AiWarning {
                        path: file.path.clone(),
                        message: analysis.warning.unwrap_or_else(|| {
                            "AI analysis failed before a complete coverage result was produced."
                                .to_owned()
                        }),
                    });
                    Ok(AiResultOutcome::Warning)
                }
            }
        }
        Err(error) if error == ai::AI_ANALYSIS_CANCELLED_MESSAGE => {
            report.cancelled = true;
            Ok(AiResultOutcome::Cancelled)
        }
        Err(error) => {
            report.failed_file_count += 1;
            report.warnings.push(ai::AiWarning {
                path: file.path.clone(),
                message: error,
            });
            Ok(AiResultOutcome::Warning)
        }
    }
}

fn unique_indexed_files_under_root(
    files: Vec<local_index::IndexedFile>,
    root: &Path,
) -> Vec<local_index::IndexedFile> {
    let mut content_hashes = HashSet::new();
    files
        .into_iter()
        .filter(|file| Path::new(&file.path).starts_with(root))
        .filter(|file| !file.identity_verified || content_hashes.insert(file.content_hash.clone()))
        .collect()
}

fn ensure_ai_identity_verified(indexed_files: &[local_index::IndexedFile]) -> Result<(), String> {
    let unverified_count = indexed_files
        .iter()
        .filter(|file| !file.identity_verified)
        .count();
    if unverified_count > 0 {
        return Err(format!(
            "AI analysis requires a fresh scan before paid requests: {unverified_count} indexed clip identity(ies) are unverified"
        ));
    }
    Ok(())
}

#[cfg(test)]
fn select_ai_files(
    index: &local_index::SqliteIndex,
    indexed_files: Vec<local_index::IndexedFile>,
    model: &str,
    settings_fingerprint: &str,
    force: bool,
) -> Result<AiFileSelection, String> {
    select_ai_files_with_options(
        index,
        indexed_files,
        model,
        settings_fingerprint,
        force,
        false,
        &HashSet::new(),
    )
}

fn select_ai_files_with_options(
    index: &local_index::SqliteIndex,
    indexed_files: Vec<local_index::IndexedFile>,
    model: &str,
    settings_fingerprint: &str,
    force: bool,
    resume_checkpoints: bool,
    resumable_content_hashes: &HashSet<String>,
) -> Result<AiFileSelection, String> {
    let mut files = Vec::with_capacity(indexed_files.len());
    let mut skipped_file_count = 0u64;
    let mut partial_file_count = 0u64;
    let mut coverage_unknown_file_count = 0u64;
    for file in indexed_files {
        if !file.identity_verified {
            return Err(format!(
                "AI analysis requires a fresh scan before paid requests: '{}' has an unverified identity",
                file.path
            ));
        }
        let coverage_state =
            ai_coverage_state(index, &file.content_hash, model, settings_fingerprint)?;
        let has_resumable_checkpoint = resumable_content_hashes.contains(&file.content_hash);
        let checkpoint_requires_explicit_resume = has_resumable_checkpoint && !resume_checkpoints;
        let covered_file_requires_explicit_resume = coverage_state != AiCoverageState::New
            && !(resume_checkpoints && has_resumable_checkpoint);
        if !force && (checkpoint_requires_explicit_resume || covered_file_requires_explicit_resume)
        {
            skipped_file_count += 1;
            match coverage_state {
                AiCoverageState::Partial => partial_file_count += 1,
                AiCoverageState::CoverageUnknown => coverage_unknown_file_count += 1,
                AiCoverageState::Complete | AiCoverageState::New => {}
            }
        } else {
            files.push(file);
        }
    }
    Ok(AiFileSelection {
        files,
        skipped_file_count,
        partial_file_count,
        coverage_unknown_file_count,
    })
}

fn ai_vision_checkpoint_summaries(
    index: &local_index::SqliteIndex,
    indexed_files: &[local_index::IndexedFile],
    settings: &ai::AiSettings,
    settings_fingerprint: &str,
) -> Result<HashMap<String, local_index::AiVisionCheckpointSummary>, String> {
    let mut summaries = HashMap::new();
    for file in indexed_files {
        let summary = index
            .ai_vision_checkpoint_summary_for_any_plan(
                &file.content_hash,
                settings_fingerprint,
                ai::AI_VISION_CHECKPOINT_VERSION,
                settings.vision_batch_size(),
            )
            .map_err(|error| error.to_string())?;
        summaries.insert(file.content_hash.clone(), summary);
    }
    Ok(summaries)
}

fn estimated_sampled_frame_count(duration_ms: Option<u64>, settings: &ai::AiSettings) -> u64 {
    let max_frames = settings.max_frames_per_file().max(1) as u64;
    duration_ms
        .map(|duration| {
            duration
                .max(1)
                .div_ceil(settings.sample_interval_ms().max(1))
                .max(1)
                .min(max_frames)
        })
        .unwrap_or(max_frames)
}

struct AiFileSelection {
    files: Vec<local_index::IndexedFile>,
    skipped_file_count: u64,
    partial_file_count: u64,
    coverage_unknown_file_count: u64,
}

fn requires_explicit_coverage_confirmation(
    force: bool,
    partial_file_count: u64,
    coverage_unknown_file_count: u64,
) -> bool {
    !force && partial_file_count.saturating_add(coverage_unknown_file_count) > 0
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AiCoverageState {
    New,
    Complete,
    Partial,
    CoverageUnknown,
}

fn ai_coverage_state(
    index: &local_index::SqliteIndex,
    content_hash: &str,
    model: &str,
    settings_fingerprint: &str,
) -> Result<AiCoverageState, String> {
    let active_coverage = index
        .active_ai_analysis_coverage_or_unknown(content_hash, model)
        .map_err(|error| error.to_string())?;
    let latest_exact = index
        .latest_ai_analysis_coverage(content_hash, model, settings_fingerprint)
        .map_err(|error| error.to_string())?;

    if let Some(active_coverage) = active_coverage {
        if active_coverage.settings_fingerprint == settings_fingerprint {
            return Ok(match active_coverage.status.as_str() {
                "complete" => AiCoverageState::Complete,
                "partial" | "failed" => AiCoverageState::Partial,
                _ => AiCoverageState::CoverageUnknown,
            });
        }
        if latest_exact
            .as_ref()
            .is_some_and(|coverage| matches!(coverage.status.as_str(), "partial" | "failed"))
        {
            return Ok(AiCoverageState::Partial);
        }
        return Ok(AiCoverageState::CoverageUnknown);
    }

    if latest_exact
        .as_ref()
        .is_some_and(|coverage| matches!(coverage.status.as_str(), "partial" | "failed"))
    {
        return Ok(AiCoverageState::Partial);
    }
    if latest_exact.is_some()
        || index
            .latest_ai_analysis_coverage_for_content_model(content_hash, model)
            .map_err(|error| error.to_string())?
            .is_some()
    {
        return Ok(AiCoverageState::CoverageUnknown);
    }
    Ok(AiCoverageState::New)
}

fn count_already_analyzed(
    index: &local_index::SqliteIndex,
    indexed_files: &[local_index::IndexedFile],
    model: &str,
) -> Result<u64, String> {
    let mut count = 0u64;
    for file in indexed_files {
        if file.identity_verified
            && index
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
    let mut index = open_local_index(&app)?;
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
    let run_id = new_ai_run_id();
    let usage_recorder = usage::AiUsageRecorder::new(
        run_id.clone(),
        settings.provider_name(),
        "unknown",
        pricing::PRICING_CHECKED_AT,
    );
    index
        .start_ai_analysis_run(&usage::AiRunSpec {
            run_id: run_id.clone(),
            operation: "search_ai".to_owned(),
            provider: settings.provider_name().to_owned(),
            vision_model: settings.vision_model().to_owned(),
            embedding_model: settings.embedding_model().to_owned(),
            transcription_model: None,
            model_namespace: model_namespace.clone(),
            pricing_status: "unknown".to_owned(),
            pricing_checked_at: pricing::PRICING_CHECKED_AT.to_owned(),
            estimated_cost_usd: None,
            budget_limit_usd: None,
        })
        .map_err(|error| error.to_string())?;
    let embedding = ai::embed_query(
        &query,
        &settings.with_usage_recorder(usage_recorder.clone()),
    );
    persist_ai_usage_events(&mut index, &usage_recorder)?;
    let results = embedding.and_then(|embedding| {
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
    });
    index
        .finish_ai_analysis_run(
            &run_id,
            if results.is_ok() {
                "completed"
            } else {
                "failed"
            },
            0,
            0,
            None,
        )
        .map_err(|error| error.to_string())?;
    results
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
    let mut index = open_local_index(&app)?;
    let run_id = new_ai_run_id();
    let usage_recorder = usage::AiUsageRecorder::new(
        run_id.clone(),
        settings.provider_name(),
        "unknown",
        pricing::PRICING_CHECKED_AT,
    );
    let model_namespace = settings.model_namespace();
    index
        .start_ai_analysis_run(&usage::AiRunSpec {
            run_id: run_id.clone(),
            operation: "test_ai_connection".to_owned(),
            provider: settings.provider_name().to_owned(),
            vision_model: settings.vision_model().to_owned(),
            embedding_model: settings.embedding_model().to_owned(),
            transcription_model: None,
            model_namespace,
            pricing_status: "unknown".to_owned(),
            pricing_checked_at: pricing::PRICING_CHECKED_AT.to_owned(),
            estimated_cost_usd: None,
            budget_limit_usd: None,
        })
        .map_err(|error| error.to_string())?;
    let result = ai::test_connection(&settings.with_usage_recorder(usage_recorder.clone()));
    persist_ai_usage_events(&mut index, &usage_recorder)?;
    index
        .finish_ai_analysis_run(
            &run_id,
            if result.is_ok() {
                "completed"
            } else {
                "failed"
            },
            0,
            0,
            None,
        )
        .map_err(|error| error.to_string())?;
    result
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
    use std::time::Duration;

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
    fn persisted_file_counter_changes_only_after_successful_sqlite_commit() {
        let mut index = local_index::SqliteIndex::open_in_memory().expect("index should open");
        let file = local_index::IndexedFile {
            path: "/library/clip.mp4".to_owned(),
            content_hash: "hash-clip".to_owned(),
            size_bytes: 10,
            modified_unix_ms: None,
            status: local_index::LocalFileStatus::Active,
            identity_verified: true,
        };
        index
            .reconcile(
                &scanner::ScanReport {
                    files: vec![scanner::DiscoveredFile {
                        path: file.path.clone(),
                        size_bytes: file.size_bytes,
                        modified_unix_ms: file.modified_unix_ms,
                        content_hash: file.content_hash.clone(),
                    }],
                    warnings: Vec::new(),
                },
                &HashMap::new(),
            )
            .expect("file should be indexed");

        let mut report = ai::AiIndexReport {
            analyzed_file_count: 0,
            skipped_file_count: 0,
            annotation_count: 0,
            partial_file_count: 0,
            failed_file_count: 0,
            cancelled: false,
            warnings: Vec::new(),
        };
        let persisted_files = AtomicU64::new(0);
        let annotation = ai::AiAnnotation {
            timestamp_ms: 1_000,
            description: "A saved scene".to_owned(),
            labels: vec!["scene".to_owned()],
            embedding: vec![0.1, 0.2],
            confidence: Some(0.9),
            model: "openai:test".to_owned(),
        };

        assert_eq!(
            persist_ai_result(
                &mut index,
                &mut report,
                &file,
                "openai:test",
                "test-run",
                "test-fingerprint",
                Ok(ai::AiFileAnalysisResult {
                    annotations: vec![annotation.clone()],
                    planned_frame_count: 1,
                    successful_frame_count: 1,
                    failed_batches: Vec::new(),
                    status: ai::AiFileAnalysisStatus::Complete,
                    warning: None,
                }),
                &persisted_files,
            )
            .expect("result should be committed"),
            AiResultOutcome::Committed
        );
        assert_eq!(persisted_files.load(Ordering::Relaxed), 1);
        assert_eq!(index.ai_annotation_count().expect("count should load"), 1);

        let missing_file = local_index::IndexedFile {
            content_hash: "missing-hash".to_owned(),
            ..file
        };
        assert!(persist_ai_result(
            &mut index,
            &mut report,
            &missing_file,
            "openai:test",
            "test-run",
            "test-fingerprint",
            Ok(ai::AiFileAnalysisResult {
                annotations: vec![annotation],
                planned_frame_count: 1,
                successful_frame_count: 1,
                failed_batches: Vec::new(),
                status: ai::AiFileAnalysisStatus::Complete,
                warning: None,
            }),
            &persisted_files,
        )
        .is_err());
        assert_eq!(persisted_files.load(Ordering::Relaxed), 1);
        assert_eq!(report.analyzed_file_count, 1);
    }

    #[test]
    fn checkpoint_dispatcher_acks_two_workers_and_stops_after_a_write_error() {
        use std::sync::atomic::AtomicUsize;

        let mut index = local_index::SqliteIndex::open_in_memory().expect("index should open");
        let file = local_index::IndexedFile {
            path: "/library/dispatcher.mp4".to_owned(),
            content_hash: "hash-dispatcher".to_owned(),
            size_bytes: 10,
            modified_unix_ms: None,
            status: local_index::LocalFileStatus::Active,
            identity_verified: true,
        };
        index
            .reconcile(
                &scanner::ScanReport {
                    files: vec![scanner::DiscoveredFile {
                        path: file.path.clone(),
                        size_bytes: file.size_bytes,
                        modified_unix_ms: file.modified_unix_ms,
                        content_hash: file.content_hash.clone(),
                    }],
                    warnings: Vec::new(),
                },
                &HashMap::new(),
            )
            .expect("dispatcher fixture should be indexed");

        let control = AiAnalysisControl::default();
        let _guard = control.begin().expect("dispatcher run should start");
        let (sender, receiver) = mpsc::channel();
        let (invalid_store_sent, invalid_store_ready) = mpsc::channel();
        let (worker_one_done, worker_one_done_rx) = mpsc::channel();
        let (worker_two_done, worker_two_done_rx) = mpsc::channel();
        let next_paid_attempts = Arc::new(AtomicUsize::new(0));

        let worker_one_sender = sender.clone();
        let worker_one = std::thread::spawn(move || {
            let (acknowledgement_sender, acknowledgement_receiver) = mpsc::channel();
            worker_one_sender
                .send(AiWorkerMessage::StoreVisionCheckpoint {
                    content_hash: "hash-dispatcher".to_owned(),
                    settings_fingerprint: "fingerprint-dispatcher".to_owned(),
                    checkpoint: ai::AiVisionCheckpointBatch {
                        batch_index: 0,
                        frame_timestamps: Vec::new(),
                        batch_timestamps: Vec::new(),
                        analyses: Vec::new(),
                    },
                    acknowledgement: acknowledgement_sender,
                })
                .expect("worker one should enqueue its store");
            invalid_store_sent
                .send(())
                .expect("test should observe worker one");
            let result = acknowledgement_receiver
                .recv_timeout(Duration::from_secs(2))
                .expect("worker one should receive a write ACK");
            assert!(result.is_err(), "invalid checkpoint write must fail");
            worker_one_done_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("worker one should be released");
        });

        invalid_store_ready
            .recv_timeout(Duration::from_secs(2))
            .expect("worker one should enqueue before worker two starts");
        let worker_two_sender = sender.clone();
        let worker_two_control = control.clone();
        let worker_two_attempts = next_paid_attempts.clone();
        let worker_two = std::thread::spawn(move || {
            let (load_ack_sender, load_ack_receiver) = mpsc::channel();
            worker_two_sender
                .send(AiWorkerMessage::LoadVisionCheckpoints {
                    content_hash: "hash-dispatcher".to_owned(),
                    settings_fingerprint: "fingerprint-dispatcher".to_owned(),
                    frame_timestamps: vec![0],
                    batch_size: 1,
                    acknowledgement: load_ack_sender,
                })
                .expect("worker two should enqueue its load");
            let (store_ack_sender, store_ack_receiver) = mpsc::channel();
            worker_two_sender
                .send(AiWorkerMessage::StoreVisionCheckpoint {
                    content_hash: "hash-dispatcher".to_owned(),
                    settings_fingerprint: "fingerprint-dispatcher".to_owned(),
                    checkpoint: ai::AiVisionCheckpointBatch {
                        batch_index: 0,
                        frame_timestamps: vec![0],
                        batch_timestamps: vec![0],
                        analyses: vec![ai::FrameAnalysis {
                            description: "Dispatcher fixture".to_owned(),
                            labels: Vec::new(),
                            visible_text: Vec::new(),
                            entities: Vec::new(),
                            actions: Vec::new(),
                            dialogue: Vec::new(),
                            setting: None,
                            situation: None,
                            confidence: Some(0.9),
                        }],
                    },
                    acknowledgement: store_ack_sender,
                })
                .expect("worker two should enqueue its store");
            assert!(load_ack_receiver
                .recv_timeout(Duration::from_secs(2))
                .expect("worker two should receive a load ACK")
                .is_ok());
            assert!(store_ack_receiver
                .recv_timeout(Duration::from_secs(2))
                .expect("worker two should receive a store ACK")
                .is_ok());
            worker_two_done_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("worker two should be released");
            if !worker_two_control.is_cancelled() {
                worker_two_attempts.fetch_add(1, Ordering::SeqCst);
            }
        });

        let mut fatal_error = None;
        for _ in 0..3 {
            let message = receiver
                .recv_timeout(Duration::from_secs(2))
                .expect("dispatcher should receive every bounded worker message");
            assert!(
                handle_ai_checkpoint_message(message, &mut index, &control, &mut fatal_error)
                    .is_none()
            );
        }
        worker_one_done
            .send(())
            .expect("worker one should be released after dispatch");
        worker_two_done
            .send(())
            .expect("worker two should be released after dispatch");
        worker_one
            .join()
            .expect("worker one should finish without deadlock");
        worker_two
            .join()
            .expect("worker two should finish without deadlock");

        assert!(fatal_error.is_some());
        assert!(control.is_cancelled());
        assert_eq!(next_paid_attempts.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn cancellation_keeps_a_completed_clip_and_stops_the_next_worker_result() {
        let mut index = local_index::SqliteIndex::open_in_memory().expect("index should open");
        let file = local_index::IndexedFile {
            path: "/library/clip.mp4".to_owned(),
            content_hash: "hash-cancelled-clip".to_owned(),
            size_bytes: 10,
            modified_unix_ms: None,
            status: local_index::LocalFileStatus::Active,
            identity_verified: true,
        };
        index
            .reconcile(
                &scanner::ScanReport {
                    files: vec![scanner::DiscoveredFile {
                        path: file.path.clone(),
                        size_bytes: file.size_bytes,
                        modified_unix_ms: file.modified_unix_ms,
                        content_hash: file.content_hash.clone(),
                    }],
                    warnings: Vec::new(),
                },
                &HashMap::new(),
            )
            .expect("file should be indexed");

        let annotation = ai::AiAnnotation {
            timestamp_ms: 1_000,
            description: "A completed scene".to_owned(),
            labels: vec!["scene".to_owned()],
            embedding: vec![0.1, 0.2],
            confidence: Some(0.9),
            model: "openai:test".to_owned(),
        };
        let control = AiAnalysisControl::default();
        let _guard = control.begin().expect("analysis should start");
        let (ready_tx, ready_rx) = mpsc::channel();
        let (stop_tx, stop_rx) = mpsc::channel();
        let completed_annotation = annotation.clone();
        let completed_worker = std::thread::spawn(move || {
            ready_tx
                .send(())
                .expect("test should observe completed work");
            Ok::<ai::AiFileAnalysisResult, String>(ai::AiFileAnalysisResult {
                annotations: vec![completed_annotation],
                planned_frame_count: 1,
                successful_frame_count: 1,
                failed_batches: Vec::new(),
                status: ai::AiFileAnalysisStatus::Complete,
                warning: None,
            })
        });
        let next_worker_control = control.clone();
        let next_worker = std::thread::spawn(move || {
            stop_rx.recv().expect("test should release the next worker");
            if next_worker_control.is_cancelled() {
                Err(ai::AI_ANALYSIS_CANCELLED_MESSAGE.to_owned())
            } else {
                Ok(ai::AiFileAnalysisResult {
                    annotations: Vec::new(),
                    planned_frame_count: 0,
                    successful_frame_count: 0,
                    failed_batches: Vec::new(),
                    status: ai::AiFileAnalysisStatus::Complete,
                    warning: None,
                })
            }
        });

        ready_rx
            .recv()
            .expect("first worker should finish before cancellation");
        assert!(control.request_cancel());
        stop_tx.send(()).expect("next worker should be released");
        let completed_result = completed_worker
            .join()
            .expect("completed worker should finish");
        let cancelled_result = next_worker.join().expect("cancelled worker should finish");

        let mut report = ai::AiIndexReport {
            analyzed_file_count: 0,
            skipped_file_count: 0,
            annotation_count: 0,
            partial_file_count: 0,
            failed_file_count: 0,
            cancelled: false,
            warnings: Vec::new(),
        };
        let persisted_files = AtomicU64::new(0);
        assert_eq!(
            persist_ai_result(
                &mut index,
                &mut report,
                &file,
                "openai:test",
                "test-run",
                "test-fingerprint",
                completed_result,
                &persisted_files,
            )
            .expect("completed clip should still be committed"),
            AiResultOutcome::Committed
        );
        assert_eq!(
            persist_ai_result(
                &mut index,
                &mut report,
                &file,
                "openai:test",
                "test-run",
                "test-fingerprint",
                cancelled_result,
                &persisted_files,
            )
            .expect("cancellation should be handled"),
            AiResultOutcome::Cancelled
        );
        report.cancelled = true;

        assert_eq!(persisted_files.load(Ordering::Relaxed), 1);
        assert_eq!(index.ai_annotation_count().expect("count should load"), 1);
        assert!(report.cancelled);
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
                identity_verified: true,
            },
            local_index::IndexedFile {
                path: "/library/b/copy.mp4".to_owned(),
                content_hash: "same-content".to_owned(),
                size_bytes: 10,
                modified_unix_ms: None,
                status: local_index::LocalFileStatus::Active,
                identity_verified: true,
            },
            local_index::IndexedFile {
                path: "/library/b/unique.mp4".to_owned(),
                content_hash: "unique-content".to_owned(),
                size_bytes: 20,
                modified_unix_ms: None,
                status: local_index::LocalFileStatus::Active,
                identity_verified: true,
            },
            local_index::IndexedFile {
                path: "/outside/other.mp4".to_owned(),
                content_hash: "outside-content".to_owned(),
                size_bytes: 30,
                modified_unix_ms: None,
                status: local_index::LocalFileStatus::Active,
                identity_verified: true,
            },
        ];

        let selected = unique_indexed_files_under_root(files, Path::new("/library"));

        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].path, "/library/a/clip.mp4");
        assert_eq!(selected[1].path, "/library/b/unique.mp4");
    }

    #[test]
    fn rejects_unverified_content_before_ai_analysis() {
        let files = vec![local_index::IndexedFile {
            path: "/library/legacy/clip.mp4".to_owned(),
            content_hash: "legacy-hash".to_owned(),
            size_bytes: 10,
            modified_unix_ms: None,
            status: local_index::LocalFileStatus::Active,
            identity_verified: false,
        }];

        let error = ensure_ai_identity_verified(&files).expect_err("legacy identity must block");

        assert!(error.contains("fresh scan"));
        assert!(error.contains("unverified"));
    }

    #[test]
    fn partial_retry_is_skipped_until_explicitly_confirmed() {
        let mut index = local_index::SqliteIndex::open_in_memory().expect("index should open");
        let file = local_index::IndexedFile {
            path: "/library/partial-retry.mp4".to_owned(),
            content_hash: "hash-partial-retry".to_owned(),
            size_bytes: 10,
            modified_unix_ms: None,
            status: local_index::LocalFileStatus::Active,
            identity_verified: true,
        };
        index
            .reconcile(
                &scanner::ScanReport {
                    files: vec![scanner::DiscoveredFile {
                        path: file.path.clone(),
                        size_bytes: file.size_bytes,
                        modified_unix_ms: file.modified_unix_ms,
                        content_hash: file.content_hash.clone(),
                    }],
                    warnings: Vec::new(),
                },
                &HashMap::new(),
            )
            .expect("fixture should be indexed");
        index
            .record_ai_analysis_result(
                &file.content_hash,
                "model-a",
                "fingerprint-a",
                None,
                &ai::AiFileAnalysisResult {
                    annotations: vec![ai::AiAnnotation {
                        timestamp_ms: 1_000,
                        description: "Partial result".to_owned(),
                        labels: Vec::new(),
                        embedding: vec![1.0, 0.0],
                        confidence: None,
                        model: "model-a".to_owned(),
                    }],
                    planned_frame_count: 2,
                    successful_frame_count: 1,
                    failed_batches: Vec::new(),
                    status: ai::AiFileAnalysisStatus::Partial,
                    warning: Some("Partial coverage".to_owned()),
                },
            )
            .expect("partial fixture should persist");

        let selection = select_ai_files(
            &index,
            vec![file.clone()],
            "model-a",
            "fingerprint-a",
            false,
        )
        .expect("selection should work");
        assert!(selection.files.is_empty());
        assert_eq!(selection.partial_file_count, 1);
        assert!(requires_explicit_coverage_confirmation(
            false,
            selection.partial_file_count,
            selection.coverage_unknown_file_count,
        ));

        let forced = select_ai_files(&index, vec![file], "model-a", "fingerprint-a", true)
            .expect("forced selection should work");
        assert_eq!(forced.files.len(), 1);
        assert!(!requires_explicit_coverage_confirmation(true, 1, 0));
    }

    #[test]
    fn checkpoint_resume_requires_explicit_selection_and_reports_available_work() {
        let mut index = local_index::SqliteIndex::open_in_memory().expect("index should open");
        let file = local_index::IndexedFile {
            path: "/library/checkpoint-resume.mp4".to_owned(),
            content_hash: "hash-checkpoint-resume".to_owned(),
            size_bytes: 10,
            modified_unix_ms: None,
            status: local_index::LocalFileStatus::Active,
            identity_verified: true,
        };
        let metadata = metadata::MediaMetadata {
            duration_ms: Some(1),
            size_bytes: Some(file.size_bytes),
            container: None,
            video_codec: None,
            audio_codec: None,
            width: None,
            height: None,
            frame_rate: None,
            start_time: None,
            creation_time: None,
        };
        let metadata_by_path = HashMap::from([(file.path.clone(), metadata)]);
        index
            .reconcile(
                &scanner::ScanReport {
                    files: vec![scanner::DiscoveredFile {
                        path: file.path.clone(),
                        size_bytes: file.size_bytes,
                        modified_unix_ms: file.modified_unix_ms,
                        content_hash: file.content_hash.clone(),
                    }],
                    warnings: Vec::new(),
                },
                &metadata_by_path,
            )
            .expect("fixture should be indexed");
        let settings = ai::AiSettings::from_request(Some(ai::AiRequestConfig {
            provider: Some(ai::AiProvider::OpenAI),
            api_key: Some("test-key".to_owned()),
            sample_interval_seconds: Some(1),
            max_frames: Some(60),
            ..Default::default()
        }))
        .expect("settings should be valid");
        let settings_fingerprint = settings.analysis_settings_fingerprint();
        // Metadata-only preflight must use this persisted two-frame plan, not the
        // short metadata estimate that would predict one frame.
        let frame_timestamps = vec![0, 1_000];
        assert_eq!(estimated_sampled_frame_count(Some(1), &settings), 1);
        index
            .store_ai_vision_checkpoint(
                &file.content_hash,
                &settings_fingerprint,
                ai::AI_VISION_CHECKPOINT_VERSION,
                &ai::AiVisionCheckpointBatch {
                    batch_index: 0,
                    frame_timestamps: frame_timestamps.clone(),
                    batch_timestamps: frame_timestamps.clone(),
                    analyses: frame_timestamps
                        .iter()
                        .map(|timestamp| ai::FrameAnalysis {
                            description: format!("Frame at {timestamp} ms"),
                            labels: Vec::new(),
                            visible_text: Vec::new(),
                            entities: Vec::new(),
                            actions: Vec::new(),
                            dialogue: Vec::new(),
                            setting: None,
                            situation: None,
                            confidence: Some(0.9),
                        })
                        .collect(),
                },
            )
            .expect("checkpoint should persist");
        index
            .record_ai_analysis_result(
                &file.content_hash,
                &settings.model_namespace(),
                &settings_fingerprint,
                None,
                &ai::AiFileAnalysisResult {
                    annotations: Vec::new(),
                    planned_frame_count: 2,
                    successful_frame_count: 2,
                    failed_batches: Vec::new(),
                    status: ai::AiFileAnalysisStatus::Failed,
                    warning: Some("embedding failed after vision".to_owned()),
                },
            )
            .expect("failed downstream result should persist");

        let summaries = ai_vision_checkpoint_summaries(
            &index,
            std::slice::from_ref(&file),
            &settings,
            &settings_fingerprint,
        )
        .expect("checkpoint summary should load");
        assert_eq!(summaries[&file.content_hash].reusable_frame_count, 2);
        assert_eq!(summaries[&file.content_hash].reusable_batch_count, 1);
        assert_eq!(summaries[&file.content_hash].frame_count, 2);
        assert_eq!(
            summaries[&file.content_hash].frame_timestamps.as_deref(),
            Some(frame_timestamps.as_slice())
        );
        let no_resume = select_ai_files_with_options(
            &index,
            vec![file.clone()],
            &settings.model_namespace(),
            &settings_fingerprint,
            false,
            false,
            &HashSet::new(),
        )
        .expect("non-resume selection should work");
        assert!(no_resume.files.is_empty());
        assert_eq!(no_resume.partial_file_count, 1);
        let resume_hashes = HashSet::from([file.content_hash.clone()]);
        let resume = select_ai_files_with_options(
            &index,
            vec![file],
            &settings.model_namespace(),
            &settings_fingerprint,
            false,
            true,
            &resume_hashes,
        )
        .expect("resume selection should work");
        assert_eq!(resume.files.len(), 1);
        assert_eq!(resume.partial_file_count, 0);
    }

    #[test]
    fn selection_uses_the_active_coverage_after_settings_switches() {
        let mut index = local_index::SqliteIndex::open_in_memory().expect("index should open");
        let file = local_index::IndexedFile {
            path: "/library/settings-switch.mp4".to_owned(),
            content_hash: "hash-settings-switch".to_owned(),
            size_bytes: 10,
            modified_unix_ms: None,
            status: local_index::LocalFileStatus::Active,
            identity_verified: true,
        };
        index
            .reconcile(
                &scanner::ScanReport {
                    files: vec![scanner::DiscoveredFile {
                        path: file.path.clone(),
                        size_bytes: file.size_bytes,
                        modified_unix_ms: file.modified_unix_ms,
                        content_hash: file.content_hash.clone(),
                    }],
                    warnings: Vec::new(),
                },
                &HashMap::new(),
            )
            .expect("fixture should be indexed");

        for (fingerprint, timestamp_ms, description) in [
            ("fingerprint-a", 1_000, "Settings A"),
            ("fingerprint-b", 2_000, "Settings B"),
        ] {
            index
                .record_ai_analysis_result(
                    &file.content_hash,
                    "model-a",
                    fingerprint,
                    None,
                    &ai::AiFileAnalysisResult {
                        annotations: vec![ai::AiAnnotation {
                            timestamp_ms,
                            description: description.to_owned(),
                            labels: Vec::new(),
                            embedding: vec![1.0, 0.0],
                            confidence: None,
                            model: "model-a".to_owned(),
                        }],
                        planned_frame_count: 1,
                        successful_frame_count: 1,
                        failed_batches: Vec::new(),
                        status: ai::AiFileAnalysisStatus::Complete,
                        warning: None,
                    },
                )
                .expect("complete result should persist");
        }

        let old_settings = select_ai_files(
            &index,
            vec![file.clone()],
            "model-a",
            "fingerprint-a",
            false,
        )
        .expect("selection should work");
        assert!(old_settings.files.is_empty());
        assert_eq!(old_settings.skipped_file_count, 1);
        assert_eq!(old_settings.coverage_unknown_file_count, 1);

        let active_settings = select_ai_files(
            &index,
            vec![file.clone()],
            "model-a",
            "fingerprint-b",
            false,
        )
        .expect("selection should work");
        assert!(active_settings.files.is_empty());
        assert_eq!(active_settings.skipped_file_count, 1);
        assert_eq!(active_settings.coverage_unknown_file_count, 0);

        let restored_settings =
            select_ai_files(&index, vec![file], "model-a", "fingerprint-a", true)
                .expect("forced selection should work");
        assert_eq!(restored_settings.files.len(), 1);
    }

    #[test]
    fn failed_latest_attempt_keeps_prior_complete_result_selectable() {
        let mut index = local_index::SqliteIndex::open_in_memory().expect("index should open");
        let file = local_index::IndexedFile {
            path: "/library/failed-retry.mp4".to_owned(),
            content_hash: "hash-failed-retry".to_owned(),
            size_bytes: 10,
            modified_unix_ms: None,
            status: local_index::LocalFileStatus::Active,
            identity_verified: true,
        };
        index
            .reconcile(
                &scanner::ScanReport {
                    files: vec![scanner::DiscoveredFile {
                        path: file.path.clone(),
                        size_bytes: file.size_bytes,
                        modified_unix_ms: file.modified_unix_ms,
                        content_hash: file.content_hash.clone(),
                    }],
                    warnings: Vec::new(),
                },
                &HashMap::new(),
            )
            .expect("fixture should be indexed");
        index
            .record_ai_analysis_result(
                &file.content_hash,
                "model-a",
                "fingerprint-a",
                None,
                &ai::AiFileAnalysisResult {
                    annotations: vec![ai::AiAnnotation {
                        timestamp_ms: 1_000,
                        description: "Complete A".to_owned(),
                        labels: Vec::new(),
                        embedding: vec![1.0, 0.0],
                        confidence: None,
                        model: "model-a".to_owned(),
                    }],
                    planned_frame_count: 1,
                    successful_frame_count: 1,
                    failed_batches: Vec::new(),
                    status: ai::AiFileAnalysisStatus::Complete,
                    warning: None,
                },
            )
            .expect("complete result should persist");
        index
            .record_ai_analysis_result(
                &file.content_hash,
                "model-a",
                "fingerprint-b",
                None,
                &ai::AiFileAnalysisResult {
                    annotations: Vec::new(),
                    planned_frame_count: 1,
                    successful_frame_count: 0,
                    failed_batches: Vec::new(),
                    status: ai::AiFileAnalysisStatus::Failed,
                    warning: Some("Retry failed".to_owned()),
                },
            )
            .expect("failed retry should persist diagnostics");

        assert_eq!(
            ai_coverage_state(&index, &file.content_hash, "model-a", "fingerprint-a")
                .expect("active settings should be readable"),
            AiCoverageState::Complete
        );
        assert_eq!(
            ai_coverage_state(&index, &file.content_hash, "model-a", "fingerprint-b")
                .expect("failed settings should be readable"),
            AiCoverageState::Partial
        );
    }

    #[test]
    fn legacy_annotations_are_not_auto_queued_for_paid_analysis() {
        let mut index = local_index::SqliteIndex::open_in_memory().expect("index should open");
        let file = local_index::IndexedFile {
            path: "/library/legacy-paid.mp4".to_owned(),
            content_hash: "hash-legacy-paid".to_owned(),
            size_bytes: 10,
            modified_unix_ms: None,
            status: local_index::LocalFileStatus::Active,
            identity_verified: true,
        };
        index
            .reconcile(
                &scanner::ScanReport {
                    files: vec![scanner::DiscoveredFile {
                        path: file.path.clone(),
                        size_bytes: file.size_bytes,
                        modified_unix_ms: file.modified_unix_ms,
                        content_hash: file.content_hash.clone(),
                    }],
                    warnings: Vec::new(),
                },
                &HashMap::new(),
            )
            .expect("fixture should be indexed");
        index
            .replace_ai_annotations(
                &file.content_hash,
                &[ai::AiAnnotation {
                    timestamp_ms: 1_000,
                    description: "Legacy result".to_owned(),
                    labels: Vec::new(),
                    embedding: vec![1.0, 0.0],
                    confidence: None,
                    model: "model-a".to_owned(),
                }],
            )
            .expect("legacy annotation should persist");

        let selection = select_ai_files(
            &index,
            vec![file.clone()],
            "model-a",
            "new-settings-fingerprint",
            false,
        )
        .expect("selection should work");
        assert!(selection.files.is_empty());
        assert_eq!(selection.coverage_unknown_file_count, 1);
        assert!(requires_explicit_coverage_confirmation(
            false,
            selection.partial_file_count,
            selection.coverage_unknown_file_count,
        ));
        assert_eq!(
            select_ai_files(
                &index,
                vec![file],
                "model-a",
                "new-settings-fingerprint",
                true,
            )
            .expect("forced selection should work")
            .files
            .len(),
            1
        );
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
                identity_verified: true,
            },
            local_index::IndexedFile {
                path: "/library/fortnite/day-two/clip-b.mp4".to_owned(),
                content_hash: "hash-b".to_owned(),
                size_bytes: 20,
                modified_unix_ms: None,
                status: local_index::LocalFileStatus::Active,
                identity_verified: true,
            },
        ];

        assert_eq!(
            common_library_root(&files),
            Some(PathBuf::from("/library/fortnite"))
        );
        assert_eq!(common_library_root(&[]), None);
    }
}
