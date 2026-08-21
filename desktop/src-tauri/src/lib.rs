pub mod local_index;
pub mod metadata;
pub mod scanner;

use std::fs;
use std::path::{Path, PathBuf};
use tauri::Manager;

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
    let mut scan = scanner::scan_folder(Path::new(&path), &scanner::ScanOptions::default())
        .map_err(|error| error.to_string())?;
    let (metadata_by_path, metadata_warnings) =
        metadata::collect_metadata(&scan.files, &metadata::FfprobeMetadataProbe::default());
    scan.warnings.extend(metadata_warnings);
    let mut index = open_local_index(&app)?;
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
            search_media
        ])
        .run(tauri::generate_context!())
        .expect("error while running MediaIndex");
}
