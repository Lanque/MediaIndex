pub mod ai;
pub mod local_index;
pub mod metadata;
pub mod scanner;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tauri::Manager;
use tauri_plugin_opener::OpenerExt;

#[tauri::command]
fn scan_media_folder(path: String) -> Result<scanner::ScanReport, String> {
    scanner::scan_folder(Path::new(&path), &scanner::ScanOptions::default())
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn extract_media_metadata(path: String) -> metadata::MetadataExtraction {
    metadata::extract_media_metadata(Path::new(&path))
}

#[tauri::command]
fn index_media_folder(
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
        .reconcile(&scan, &metadata_by_path)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn search_media(
    app: tauri::AppHandle,
    query: local_index::SearchQuery,
) -> Result<Vec<local_index::SearchResult>, String> {
    open_local_index(&app)?
        .search(&query)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn analyze_media_folder(app: tauri::AppHandle, path: String) -> Result<ai::AiIndexReport, String> {
    let settings = ai::AiSettings::from_environment()?;
    let mut index = open_local_index(&app)?;
    let root = Path::new(&path);
    let files = index
        .known_files()
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|file| Path::new(&file.path).starts_with(root))
        .collect::<Vec<_>>();
    if files.is_empty() {
        return Err("No indexed active clips were found in the selected folder".to_owned());
    }

    let mut report = ai::AiIndexReport {
        analyzed_file_count: 0,
        annotation_count: 0,
        warnings: Vec::new(),
    };
    for file in files {
        let metadata = index
            .get_asset_metadata(&file.content_hash)
            .map_err(|error| error.to_string())?;
        match ai::analyze_file(Path::new(&file.path), metadata.as_ref(), &settings) {
            Ok(annotations) => {
                report.analyzed_file_count += 1;
                report.annotation_count += annotations.len() as u64;
                index
                    .replace_ai_annotations(&file.content_hash, &annotations)
                    .map_err(|error| error.to_string())?;
            }
            Err(error) => report.warnings.push(ai::AiWarning {
                path: file.path,
                message: error,
            }),
        }
    }

    Ok(report)
}

#[tauri::command]
fn search_ai(
    app: tauri::AppHandle,
    query: String,
) -> Result<Vec<local_index::AiSearchResult>, String> {
    let settings = ai::AiSettings::from_environment()?;
    let embedding = ai::embed_query(&query, &settings)?;
    open_local_index(&app)?
        .search_ai(&embedding, 100)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn open_indexed_media_path(app: tauri::AppHandle, path: String) -> Result<(), String> {
    let index = open_local_index(&app)?;
    let indexed_file = index
        .get_file(&path)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The selected clip is not in the active local index".to_owned())?;

    if indexed_file.status != local_index::LocalFileStatus::Active {
        return Err("The selected clip is no longer active in the local index".to_owned());
    }
    if !Path::new(&path).is_file() {
        return Err("The selected clip is no longer available at this path".to_owned());
    }

    app.opener()
        .open_path(path, None::<String>)
        .map_err(|error| error.to_string())
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
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            scan_media_folder,
            extract_media_metadata,
            index_media_folder,
            search_media,
            analyze_media_folder,
            search_ai,
            open_indexed_media_path
        ])
        .run(tauri::generate_context!())
        .expect("error while running MediaIndex");
}
