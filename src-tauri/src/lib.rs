pub mod archive;
pub mod commands;
pub mod engine;
pub mod error;
pub mod events;
pub mod livephoto;
pub mod manifest;
pub mod metadata;
pub mod organize;
pub mod pipeline;
pub mod planner;
pub mod platform;
pub mod report;
pub mod settings;
pub mod takeout;

pub fn run() {
    tauri::Builder::default()
        .manage(commands::PreviewIndexState::default())
        .manage(commands::ScanControlState::default())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::add_sources,
            commands::remove_source,
            commands::get_preview,
            commands::start_processing,
            commands::run_status,
            commands::cancel_processing,
            commands::load_settings,
            commands::save_settings,
            commands::export_report,
            commands::scan_sources,
            commands::start_scan,
            commands::cancel_scan,
            commands::pause_run,
            commands::resume_run,
            commands::get_report,
            commands::get_review_items,
            commands::apply_review_fix,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
