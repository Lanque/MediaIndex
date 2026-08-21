pub mod local_index;
pub mod metadata;
pub mod scanner;

use std::collections::HashMap;
use std::fs;
use std::path::Path;
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
    let scan = scanner::scan_folder(Path::new(&path), &scanner::ScanOptions::default())
        .map_err(|error| error.to_string())?;
    let database_directory = app
        .path()
        .app_local_data_dir()
        .map_err(|error| format!("cannot determine local index directory: {error}"))?;
    fs::create_dir_all(&database_directory)
        .map_err(|error| format!("cannot create local index directory: {error}"))?;

    let mut index = local_index::SqliteIndex::open(database_directory.join("mediaindex.sqlite3"))
        .map_err(|error| error.to_string())?;
    index
        .reconcile(&scan, &HashMap::new())
        .map_err(|error| error.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            scan_media_folder,
            extract_media_metadata,
            index_media_folder
        ])
        .run(tauri::generate_context!())
        .expect("error while running MediaIndex");
}
