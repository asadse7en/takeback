pub mod commands;
pub mod engine;
pub mod error;
pub mod events;
pub mod matcher;
pub mod planner;
pub mod platform;
pub mod takeout;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::scan_sources,
            commands::preview_tree,
            commands::start_run,
            commands::pause_run,
            commands::resume_run,
            commands::cancel_run,
            commands::get_report,
            commands::get_review_items,
            commands::apply_review_fix,
            commands::disk_space,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
