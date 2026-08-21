pub mod metadata;
pub mod scanner;

use std::path::Path;

#[tauri::command]
fn scan_media_folder(path: String) -> Result<scanner::ScanReport, String> {
    scanner::scan_folder(Path::new(&path), &scanner::ScanOptions::default())
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn extract_media_metadata(path: String) -> metadata::MetadataExtraction {
    metadata::extract_media_metadata(Path::new(&path))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            scan_media_folder,
            extract_media_metadata
        ])
        .run(tauri::generate_context!())
        .expect("error while running MediaIndex");
}
