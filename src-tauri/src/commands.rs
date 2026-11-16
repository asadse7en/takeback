use crate::archive::AddSourcesResult;
use crate::engine;
use crate::error::{AppError, Result};
use crate::organize::PreviewResult;
use crate::pipeline::ProcessingStartResult;
use crate::planner::{MediaItem, PlannerSettings};
use crate::report::{ReportExport, ReportFormat};
use crate::settings::Settings;
use tauri::Manager;

#[tauri::command]
pub async fn add_sources(paths: Vec<String>) -> Result<AddSourcesResult> {
    tauri::async_runtime::spawn_blocking(move || crate::takeout::inspect_sources(&paths))
        .await
        .map_err(|error| AppError::Other(format!("Could not inspect sources: {error}")))?
        .map_err(AppError::Other)
}

#[tauri::command]
pub fn remove_source(_source_id: String) -> Result<bool> {
    Err(AppError::NotImplemented(
        "Removing sources is not available yet.".to_string(),
    ))
}

#[tauri::command]
pub fn get_preview(_settings: Settings) -> Result<PreviewResult> {
    Err(AppError::NotImplemented(
        "Preview generation is not available yet.".to_string(),
    ))
}

#[tauri::command]
pub fn start_processing(_settings: Settings) -> Result<ProcessingStartResult> {
    Err(AppError::NotImplemented(
        "Processing is not available yet.".to_string(),
    ))
}

#[tauri::command]
pub fn cancel_processing() -> Result<bool> {
    Err(AppError::NotImplemented(
        "Processing cancellation is not available yet.".to_string(),
    ))
}

#[tauri::command]
pub fn load_settings() -> Result<Settings> {
    Err(AppError::NotImplemented(
        "Loading settings is not available yet.".to_string(),
    ))
}

#[tauri::command]
pub fn save_settings(_settings: Settings) -> Result<()> {
    Err(AppError::NotImplemented(
        "Saving settings is not available yet.".to_string(),
    ))
}

#[tauri::command]
pub fn export_report(_format: ReportFormat) -> Result<ReportExport> {
    Err(AppError::NotImplemented(
        "Report export is not available yet.".to_string(),
    ))
}

#[tauri::command]
pub async fn scan_sources(paths: Vec<String>) -> Result<crate::takeout::ScanSourcesResult> {
    tauri::async_runtime::spawn_blocking(move || crate::takeout::scan_paths(&paths))
        .await
        .map_err(|error| AppError::Other(format!("Could not scan sources: {error}")))?
        .map_err(AppError::Other)
}

#[tauri::command]
pub fn preview_tree() -> Vec<String> {
    vec!["2024/Paris trip/IMG_0134.jpg".to_string()]
}

#[tauri::command]
pub async fn start_run(app: tauri::AppHandle) -> Result<bool> {
    let settings = PlannerSettings::default();
    let items = vec![MediaItem {
        source: "sample/photos/IMG_0134.jpg".to_string(),
        filename: "IMG_0134.jpg".to_string(),
        album: Some("Paris trip".to_string()),
        date: Some("2024-05-16".to_string()),
        is_photo: true,
        is_video: false,
        is_live: false,
        favorite: true,
        archived: false,
        trashed: false,
        edited: false,
        has_json: true,
        duplicate_of: None,
        metadata: None,
    }];
    let exiftool_path = if items.iter().any(|item| {
        item.metadata.is_some()
            && crate::metadata::supports_embedded_metadata(std::path::Path::new(&item.source))
    }) {
        let resource_dir = app.path().resource_dir().map_err(|error| {
            AppError::Other(format!("Could not locate application resources: {error}"))
        })?;
        crate::platform::exiftool_path(&resource_dir).map_err(AppError::Other)?
    } else {
        std::path::PathBuf::new()
    };
    tauri::async_runtime::spawn_blocking(move || {
        engine::start_run(&settings, &items, &exiftool_path)
    })
    .await
    .map_err(|error| AppError::Other(format!("Could not start processing: {error}")))?
    .map(|_| true)
    .map_err(AppError::Other)
}

#[tauri::command]
pub fn pause_run() -> Result<bool> {
    engine::pause_run().map_err(AppError::Other)?;
    Ok(true)
}

#[tauri::command]
pub fn resume_run() -> Result<bool> {
    engine::resume_run().map_err(AppError::Other)?;
    Ok(true)
}

#[tauri::command]
pub fn cancel_run() -> Result<bool> {
    engine::cancel_run().map_err(AppError::Other)?;
    Ok(true)
}

#[tauri::command]
pub fn get_report() -> String {
    "Report ready".to_string()
}

#[tauri::command]
pub fn get_review_items() -> Vec<String> {
    vec!["IMG_0134.jpg".to_string()]
}

#[tauri::command]
pub fn apply_review_fix(_item: String) -> bool {
    true
}

#[tauri::command]
pub fn disk_space() -> (u64, u64) {
    (128, 512)
}
