use serde::{Deserialize, Serialize};

use crate::engine;
use crate::planner::{MediaItem, PlannerSettings};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanSourcesResult {
    pub files: Vec<String>,
    pub counts: Counts,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    pub photos: u32,
    pub videos: u32,
    pub live: u32,
    pub total_gb: f64,
}

#[tauri::command]
pub fn scan_sources() -> ScanSourcesResult {
    ScanSourcesResult {
        files: vec!["sample-1.zip".into(), "sample-2.zip".into()],
        counts: Counts { photos: 128, videos: 36, live: 12, total_gb: 18.7 },
    }
}

#[tauri::command]
pub fn preview_tree() -> Vec<String> {
    vec!["2024/Paris trip/IMG_0134.jpg".to_string()]
}

#[tauri::command]
pub fn start_run() -> bool {
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
    }];
    engine::start_run(&settings, &items).is_ok()
}

#[tauri::command]
pub fn pause_run() -> bool {
    engine::pause_run();
    true
}

#[tauri::command]
pub fn resume_run() -> bool {
    engine::resume_run();
    true
}

#[tauri::command]
pub fn cancel_run() -> bool {
    engine::cancel_run();
    true
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
