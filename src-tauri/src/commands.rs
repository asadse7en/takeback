use crate::archive::AddSourcesResult;
use crate::engine;
use crate::error::{AppError, Result};
use crate::manifest::{self, RunManifest};
use crate::organize::{self, PreviewResult};
use crate::pipeline::ProcessingStartResult;
use crate::planner::{self, MediaItem, PlannerSettings};
use crate::report::{ReportDocument, ReportExport, ReportFormat, ReportItem};
use crate::settings::Settings;
use chrono::TimeZone;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::{ipc::Channel, Manager, State};

#[derive(Clone, Default)]
pub struct PreviewIndexState(Arc<Mutex<PreviewIndexData>>);

#[derive(Clone, Default)]
pub struct ScanControlState(Arc<Mutex<Option<ActiveScan>>>);

struct ActiveScan {
    id: String,
    cancelled: Arc<AtomicBool>,
}

static NEXT_SCAN_ID: AtomicU64 = AtomicU64::new(1);
static PROCESSING_RUNNING: AtomicBool = AtomicBool::new(false);

struct ProcessingRunGuard;

impl ProcessingRunGuard {
    fn acquire() -> Option<Self> {
        PROCESSING_RUNNING
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| Self)
    }

    fn is_running() -> bool {
        PROCESSING_RUNNING.load(Ordering::Acquire)
    }
}

impl Drop for ProcessingRunGuard {
    fn drop(&mut self) {
        PROCESSING_RUNNING.store(false, Ordering::Release);
    }
}

#[derive(Default)]
struct PreviewIndexData {
    index: Option<crate::takeout::TempIndex>,
    source_paths: Vec<String>,
    indexed_archives: HashMap<String, bool>,
    index_version: u64,
    cached_preview: Option<CachedPreview>,
}

struct CachedPreview {
    key: [u8; 32],
    result: PreviewResult,
}

fn preview_cache_key(settings_hash: [u8; 32], index_version: u64) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&settings_hash);
    hasher.update(&index_version.to_le_bytes());
    *hasher.finalize().as_bytes()
}

fn archive_index_key(path: &str) -> String {
    path.replace('\\', "/").to_lowercase()
}

fn scan_into_preview_state(
    state: &mut PreviewIndexData,
    paths: &[String],
    cancelled: Option<&AtomicBool>,
    on_progress: &mut dyn FnMut(
        crate::takeout::ScanProgressUpdate,
    ) -> std::result::Result<(), String>,
) -> std::result::Result<
    (
        crate::takeout::ScanSourcesResult,
        crate::takeout::ScanProgressUpdate,
    ),
    String,
> {
    let expanded_paths = crate::takeout::expand_source_paths(paths)?;
    for path in &expanded_paths {
        let key = archive_index_key(path);
        if !state
            .source_paths
            .iter()
            .any(|existing| archive_index_key(existing) == key)
        {
            state.source_paths.push(path.clone());
        }
    }
    let all_paths = state.source_paths.clone();
    let new_paths = expanded_paths
        .into_iter()
        .filter(|path| state.indexed_archives.get(&archive_index_key(path)) != Some(&true))
        .collect::<Vec<_>>();
    for path in &new_paths {
        state
            .indexed_archives
            .entry(archive_index_key(path))
            .or_insert(false);
    }

    let total_bytes = crate::takeout::inspect_sources(&new_paths)?
        .archives
        .iter()
        .map(|source| source.size_bytes)
        .fold(0u64, u64::saturating_add);
    let archive_count = u32::try_from(new_paths.len()).unwrap_or(u32::MAX);
    let mut progress = crate::takeout::ScanProgressUpdate {
        archive_count,
        total_bytes,
        ..crate::takeout::ScanProgressUpdate::default()
    };
    let mut last_progress = Instant::now();

    if !new_paths.is_empty() {
        state.index_version = state.index_version.wrapping_add(1);
    }
    let PreviewIndexData {
        index: index_slot,
        indexed_archives,
        ..
    } = state;
    if index_slot.is_none() {
        *index_slot = Some(crate::takeout::TempIndex::new()?);
    }
    let index = index_slot
        .as_ref()
        .ok_or_else(|| "Source index is unavailable.".to_string())?;
    for (offset, path) in new_paths.iter().enumerate() {
        index.remove_archive(path)?;
        crate::takeout::index_archive_into(
            index,
            path,
            u32::try_from(offset + 1).unwrap_or(u32::MAX),
            archive_count,
            total_bytes,
            &mut progress,
            &mut last_progress,
            cancelled,
            on_progress,
        )?;
        indexed_archives.insert(archive_index_key(path), true);
    }
    index.rematch_sidecars()?;
    let result = crate::takeout::scan_result_from_index(&all_paths, index)?;
    Ok((result, progress))
}

fn remove_source_from_preview_state(
    state: &mut PreviewIndexData,
    source_id: &str,
) -> std::result::Result<crate::takeout::ScanSourcesResult, String> {
    let key = archive_index_key(source_id);
    let indexed_path = state
        .source_paths
        .iter()
        .find(|path| archive_index_key(path) == key)
        .cloned()
        .unwrap_or_else(|| source_id.to_string());
    if let Some(index) = state.index.as_ref() {
        index.remove_archive(&indexed_path)?;
        state.index_version = state.index_version.wrapping_add(1);
    }
    state
        .source_paths
        .retain(|path| archive_index_key(path) != key);
    state.indexed_archives.remove(&key);
    if let Some(index) = state.index.as_ref() {
        index.rematch_sidecars()?;
        crate::takeout::scan_result_from_index(&state.source_paths, index)
    } else {
        Ok(crate::takeout::ScanSourcesResult {
            files: state.source_paths.clone(),
            ..Default::default()
        })
    }
}

#[tauri::command]
pub async fn add_sources(paths: Vec<String>) -> Result<AddSourcesResult> {
    tauri::async_runtime::spawn_blocking(move || crate::takeout::inspect_sources(&paths))
        .await
        .map_err(|error| AppError::Other(format!("Could not inspect sources: {error}")))?
        .map_err(AppError::Other)
}

#[tauri::command]
pub async fn remove_source(
    source_id: String,
    preview_state: State<'_, PreviewIndexState>,
) -> Result<crate::takeout::ScanSourcesResult> {
    let data = preview_state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut state = data
            .lock()
            .map_err(|error| AppError::Other(format!("Source index is unavailable: {error}")))?;
        remove_source_from_preview_state(&mut state, &source_id).map_err(AppError::Other)
    })
    .await
    .map_err(|error| AppError::Other(format!("Could not remove source: {error}")))?
}

#[tauri::command]
pub async fn get_preview(
    settings: Settings,
    app: tauri::AppHandle,
    preview_state: State<'_, PreviewIndexState>,
) -> Result<PreviewResult> {
    let index_state = preview_state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let settings_bytes = serde_json::to_vec(&settings).map_err(|error| {
            AppError::Other(format!("Could not hash preview settings: {error}"))
        })?;
        let settings_hash = *blake3::hash(&settings_bytes).as_bytes();
        let pictures = app.path().picture_dir().map_err(|error| {
            AppError::Other(format!("Could not locate the Pictures folder: {error}"))
        })?;
        let output_root = resolve_output_root(&settings, &pictures)?;
        let cached = {
            let state = index_state.lock().map_err(|error| {
                AppError::Other(format!("Preview index is unavailable: {error}"))
            })?;
            if state.index.is_none() {
                return Ok(PreviewResult {
                    warnings: vec!["Add and scan Takeout files to see a real preview.".to_string()],
                    ..PreviewResult::default()
                });
            }
            let key = preview_cache_key(settings_hash, state.index_version);
            state
                .cached_preview
                .as_ref()
                .filter(|cached| cached.key == key)
                .map(|cached| cached.result.clone())
        };
        if let Some(mut cached) = cached {
            cached.free_bytes =
                crate::platform::disk_free_bytes(&output_root).map_err(AppError::Other)?;
            return Ok(cached);
        }

        let (index_version, mut entries, sample_sources) = {
            let state = index_state.lock().map_err(|error| {
                AppError::Other(format!("Preview index is unavailable: {error}"))
            })?;
            let Some(index) = state.index.as_ref() else {
                return Ok(PreviewResult {
                    warnings: vec!["Add and scan Takeout files to see a real preview.".to_string()],
                    ..PreviewResult::default()
                });
            };
            let sample_sources = index
                .preview_entries(8)
                .map_err(AppError::Other)?
                .into_iter()
                .map(|entry| format!("{}::{}", entry.archive_path, entry.path))
                .collect::<HashSet<_>>();
            (
                state.index_version,
                index.processing_entries().map_err(AppError::Other)?,
                sample_sources,
            )
        };
        hydrate_live_video_durations(&mut entries).map_err(AppError::Other)?;
        let items = items_from_entries(entries, settings.time_zone).map_err(AppError::Other)?;

        let planner_settings =
            PlannerSettings::from_preferences(&settings, Some(output_root.as_path()));
        let manifest_path = app_config_file(&app, "run-manifest.sqlite")?;
        let manifest = RunManifest::open(&manifest_path, &output_root).map_err(AppError::Other)?;
        let placements =
            plan_with_manifest_output(&items, &planner_settings, &output_root, &manifest)?;
        let sample_placements = placements
            .iter()
            .filter(|placement| sample_sources.contains(&placement.src))
            .cloned()
            .collect::<Vec<_>>();
        let files = sample_placements
            .iter()
            .map(|placement| placement.dest.clone())
            .collect();
        let tree = organize::tree_from_placements(&output_root, &sample_placements);
        let required_bytes = crate::pipeline::estimate_required_bytes(&placements, &items)
            .map_err(AppError::Other)?;
        let free_bytes = crate::platform::disk_free_bytes(&output_root).map_err(AppError::Other)?;

        let result = PreviewResult {
            files,
            tree,
            warnings: Vec::new(),
            required_bytes,
            free_bytes,
        };
        let mut state = index_state
            .lock()
            .map_err(|error| AppError::Other(format!("Preview index is unavailable: {error}")))?;
        if state.index_version == index_version {
            state.cached_preview = Some(CachedPreview {
                key: preview_cache_key(settings_hash, index_version),
                result: result.clone(),
            });
        }
        Ok(result)
    })
    .await
    .map_err(|error| AppError::Other(format!("Could not build preview: {error}")))?
}

#[tauri::command]
pub async fn start_processing(
    settings: Settings,
    app: tauri::AppHandle,
    preview_state: State<'_, PreviewIndexState>,
) -> Result<ProcessingStartResult> {
    let running_guard = ProcessingRunGuard::acquire()
        .ok_or_else(|| AppError::Other("A processing run is already active.".to_string()))?;
    let index = preview_state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _running_guard = running_guard;
        let (mut entries, all_entries, expanded_source_paths) = {
            let mut guard = index
                .lock()
                .map_err(|error| AppError::Other(format!("Source index is unavailable: {error}")))?;
            let Some(index) = guard.index.as_ref() else {
                return Err(AppError::Other(
                    "Scan Takeout files before starting processing.".to_string(),
                ));
            };
            let entries = index.processing_entries().map_err(AppError::Other)?;
            let all_entries = index.entries().map_err(AppError::Other)?;
            let source_paths = guard.source_paths.clone();
            guard.cached_preview = None;
            (entries, all_entries, source_paths)
        };
        hydrate_live_video_durations(&mut entries).map_err(AppError::Other)?;
        let input_locators = build_input_locators(&entries).map_err(AppError::Other)?;
        let items = items_from_entries(entries, settings.time_zone).map_err(AppError::Other)?;
        if items.is_empty() {
            return Err(AppError::Other(
                "The scanned sources contain no supported photo or video files.".to_string(),
            ));
        }

        let pictures = app
            .path()
            .picture_dir()
            .map_err(|error| AppError::Other(format!("Could not locate the Pictures folder: {error}")))?;
        let output_root = resolve_output_root(&settings, &pictures)?;
        crate::platform::cleanup_takeback_temporary_files(&output_root)
            .map_err(AppError::Other)?;
        let manifest_path = app_config_file(&app, "run-manifest.sqlite")?;
        let manifest =
            RunManifest::open(&manifest_path, &output_root).map_err(AppError::Other)?;
        let planner_settings =
            PlannerSettings::from_preferences(&settings, Some(output_root.as_path()));
        let placements = plan_with_manifest_output(
            &items,
            &planner_settings,
            &output_root,
            &manifest,
        )?;
        let archives_to_delete = archives_with_complete_coverage(
            &items,
            &placements,
            &all_entries,
            &expanded_source_paths,
            &settings,
        );
        let placements_to_process = if settings.dry_run {
            placements.clone()
        } else {
            placements_requiring_processing(&items, &placements, &output_root, &manifest)?
        };
        let required_bytes = if settings.dry_run {
            0
        } else {
            crate::pipeline::estimate_required_bytes(&placements_to_process, &items)
                .map_err(AppError::Other)?
        };
        let free_bytes = crate::platform::disk_free_bytes(&output_root)
            .map_err(AppError::Other)?;
        let shortfall_bytes = required_bytes.saturating_sub(free_bytes);

        if shortfall_bytes > 0 {
            return Ok(ProcessingStartResult {
                accepted: false,
                required_bytes,
                free_bytes,
                shortfall_bytes,
                warnings: vec![format!(
                    "The output drive is short by {:.2} GB. Choose another drive, switch Copy to Link, or reduce output options.",
                    shortfall_bytes as f64 / 1_000_000_000.0
                )],
                report: None,
            });
        }

        save_settings_file(&app, &settings)?;
        let needs_exiftool = items.iter().any(|item| {
            item.metadata.is_some()
                && crate::metadata::supports_embedded_metadata(Path::new(&item.filename))
        }) || (settings.merge_live
            && settings.live_style != crate::livephoto::LivePhotoStyle::Separate
            && items.iter().any(|item| item.live_pair_partner.is_some()));
        let exiftool_path = if needs_exiftool {
            let resource_dir = app.path().resource_dir().map_err(|error| {
                AppError::Other(format!("Could not locate application resources: {error}"))
            })?;
            crate::platform::exiftool_path(&resource_dir).map_err(AppError::Other)?
        } else {
            PathBuf::new()
        };
        let started = std::time::Instant::now();
        let summary = engine::start_indexed_run_with_manifest(
            &app,
            &planner_settings,
            &items,
            &placements,
            input_locators,
            archives_to_delete.clone(),
            &exiftool_path,
            manifest,
        )
        .map_err(AppError::Other)?;

        let mut warnings = summary.warnings.clone();
        let archive_count = expanded_source_paths
            .iter()
            .filter(|path| is_archive_path(path))
            .count();
        if settings.delete_archives_after && settings.dry_run && archive_count > 0 {
            warnings.push("Input archives were kept because Preview only is enabled.".to_string());
        } else if settings.delete_archives_after && archives_to_delete.len() < archive_count {
            warnings.push(
                "Some input archives were kept because not every source file was included in the output."
                    .to_string(),
            );
        }
        let mut report_summary = summary;
        report_summary.warnings = warnings;
        let report_items = report_summary.attention.clone();
        let report = ReportDocument {
            created_at: chrono::Utc::now().to_rfc3339(),
            elapsed_seconds: started.elapsed().as_secs_f64(),
            output_folder: output_root.to_string_lossy().into_owned(),
            settings: settings.clone(),
            summary: report_summary,
            items: report_items,
        };
        let report_path = app_config_file(&app, "last-report.json")?;
        report.save(&report_path).map_err(AppError::Other)?;

        Ok(ProcessingStartResult {
            accepted: true,
            required_bytes,
            free_bytes,
            shortfall_bytes: 0,
            warnings: report.summary.warnings.clone(),
            report: Some(report),
        })
    })
    .await
    .map_err(|error| AppError::Other(format!("Could not process Takeout files: {error}")))?
}

#[tauri::command]
pub fn cancel_processing() -> Result<bool> {
    engine::cancel_run().map_err(AppError::Other)?;
    Ok(true)
}

#[tauri::command]
pub async fn load_settings(app: tauri::AppHandle) -> Result<Settings> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = app_config_file(&app, "settings.json")?;
        let contents = match fs::read(&path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Settings::default())
            }
            Err(error) => {
                return Err(AppError::Other(format!(
                    "Could not read settings {}: {error}",
                    path.display()
                )));
            }
        };
        serde_json::from_slice(&contents)
            .map_err(|error| AppError::Other(format!("Could not parse settings: {error}")))
    })
    .await
    .map_err(|error| AppError::Other(format!("Could not load settings: {error}")))?
}

#[tauri::command]
pub fn save_settings(settings: Settings, app: tauri::AppHandle) -> Result<()> {
    save_settings_file(&app, &settings)
}

#[tauri::command]
pub fn export_report(
    format: ReportFormat,
    destination: String,
    app: tauri::AppHandle,
) -> Result<ReportExport> {
    let report_path = app_config_file(&app, "last-report.json")?;
    let report = ReportDocument::load(&report_path)
        .map_err(AppError::Other)?
        .ok_or_else(|| AppError::Other("There is no completed run to export.".to_string()))?;
    report
        .export(Path::new(&destination), format)
        .map_err(AppError::Other)
}

#[tauri::command]
pub async fn scan_sources(
    paths: Vec<String>,
    preview_state: State<'_, PreviewIndexState>,
) -> Result<crate::takeout::ScanSourcesResult> {
    let data = preview_state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut state = data
            .lock()
            .map_err(|error| AppError::Other(format!("Source index is unavailable: {error}")))?;
        scan_into_preview_state(&mut state, &paths, None, &mut |_| Ok(()))
            .map(|(result, _)| result)
            .map_err(AppError::Other)
    })
    .await
    .map_err(|error| AppError::Other(format!("Could not scan sources: {error}")))?
}

#[tauri::command]
pub fn start_scan(
    paths: Vec<String>,
    on_event: Channel<crate::events::ScanProgress>,
    preview_state: State<'_, PreviewIndexState>,
    scan_control: State<'_, ScanControlState>,
) -> Result<String> {
    let scan_id = format!(
        "scan-{}-{}",
        std::process::id(),
        NEXT_SCAN_ID.fetch_add(1, Ordering::Relaxed)
    );
    let cancelled = Arc::new(AtomicBool::new(false));
    {
        let mut active = scan_control
            .0
            .lock()
            .map_err(|error| AppError::Other(format!("Scan state is unavailable: {error}")))?;
        if active.is_some() {
            return Err(AppError::Other(
                "A Takeout scan is already in progress.".to_string(),
            ));
        }
        *active = Some(ActiveScan {
            id: scan_id.clone(),
            cancelled: cancelled.clone(),
        });
    }

    let initial_event = crate::events::ScanProgress {
        archive_index: 0,
        archive_count: u32::try_from(paths.len()).unwrap_or(u32::MAX),
        current_archive: String::new(),
        entries_seen: 0,
        bytes_read: 0,
        total_bytes: 0,
        stage: crate::events::ScanStage::Preparing,
        result: None,
        error: None,
    };
    if let Err(error) = on_event.send(initial_event) {
        clear_active_scan(&scan_control.0, &scan_id);
        return Err(AppError::Other(format!(
            "Could not start scan progress channel: {error}"
        )));
    }

    let index_state = preview_state.0.clone();
    let control_state = scan_control.0.clone();
    let task_scan_id = scan_id.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut latest_progress = None;
        let mut emit_progress = |update: crate::takeout::ScanProgressUpdate| {
            latest_progress = Some(update.clone());
            on_event
                .send(crate::events::ScanProgress {
                    archive_index: update.archive_index,
                    archive_count: update.archive_count,
                    current_archive: update.current_archive,
                    entries_seen: update.entries_seen,
                    bytes_read: update.bytes_read,
                    total_bytes: update.total_bytes,
                    stage: crate::events::ScanStage::Scanning,
                    result: None,
                    error: None,
                })
                .map_err(|error| format!("Could not send scan progress: {error}"))
        };
        let scan_result = match index_state.lock() {
            Ok(mut state) => scan_into_preview_state(
                &mut state,
                &paths,
                Some(cancelled.as_ref()),
                &mut emit_progress,
            ),
            Err(error) => Err(format!("Source index is unavailable: {error}")),
        };
        drop(emit_progress);

        let mut terminal_progress = latest_progress.unwrap_or_default();
        let (stage, result, error) = match scan_result {
            Ok((result, progress)) if !cancelled.load(Ordering::SeqCst) => {
                terminal_progress = progress;
                (crate::events::ScanStage::Done, Some(result), None)
            }
            Ok(_) => (crate::events::ScanStage::Cancelled, None, None),
            Err(error)
                if cancelled.load(Ordering::SeqCst)
                    || error == crate::takeout::SCAN_CANCELLED_ERROR =>
            {
                (crate::events::ScanStage::Cancelled, None, None)
            }
            Err(error) => (crate::events::ScanStage::Error, None, Some(error)),
        };
        let terminal = crate::events::ScanProgress {
            archive_index: terminal_progress.archive_index,
            archive_count: terminal_progress.archive_count,
            current_archive: terminal_progress.current_archive,
            entries_seen: terminal_progress.entries_seen,
            bytes_read: terminal_progress.bytes_read,
            total_bytes: terminal_progress.total_bytes,
            stage,
            result,
            error,
        };
        if let Err(error) = on_event.send(terminal) {
            eprintln!("Could not send final scan event: {error}");
        }
        clear_active_scan(&control_state, &task_scan_id);
    });

    Ok(scan_id)
}

#[tauri::command]
pub fn cancel_scan(scan_control: State<'_, ScanControlState>) -> Result<bool> {
    let active = scan_control
        .0
        .lock()
        .map_err(|error| AppError::Other(format!("Scan state is unavailable: {error}")))?;
    if let Some(scan) = active.as_ref() {
        scan.cancelled.store(true, Ordering::SeqCst);
        Ok(true)
    } else {
        Ok(false)
    }
}

fn clear_active_scan(state: &Arc<Mutex<Option<ActiveScan>>>, scan_id: &str) {
    match state.lock() {
        Ok(mut active) if active.as_ref().is_some_and(|scan| scan.id == scan_id) => {
            *active = None;
        }
        Ok(_) => {}
        Err(error) => eprintln!("Could not clear scan state: {error}"),
    }
}

fn resolve_output_root(settings: &Settings, pictures: &Path) -> Result<PathBuf> {
    crate::platform::resolve_output_root(settings.output_folder.as_deref().map(Path::new), pictures)
        .map_err(AppError::Other)
}

#[cfg(test)]
fn plan_with_existing_output(
    items: &[MediaItem],
    settings: &PlannerSettings,
    output_root: &Path,
) -> Result<Vec<crate::planner::Placement>> {
    plan_with_existing_paths(items, settings, output_root, &HashMap::new())
}

fn plan_with_manifest_output(
    items: &[MediaItem],
    settings: &PlannerSettings,
    output_root: &Path,
    manifest: &RunManifest,
) -> Result<Vec<crate::planner::Placement>> {
    let source_keys = items
        .iter()
        .map(|item| manifest::source_key(&item.source, item.size_bytes))
        .collect::<HashSet<_>>();
    let manifest_destinations = manifest
        .planner_destinations(&source_keys)
        .map_err(AppError::Other)?;
    plan_with_existing_paths(items, settings, output_root, &manifest_destinations)
}

fn placements_requiring_processing(
    items: &[MediaItem],
    placements: &[crate::planner::Placement],
    output_root: &Path,
    manifest: &RunManifest,
) -> Result<Vec<crate::planner::Placement>> {
    let items_by_source = items
        .iter()
        .map(|item| (item.source.as_str(), item))
        .collect::<HashMap<_, _>>();
    let mut pending = Vec::new();
    for placement in placements {
        let item = items_by_source.get(placement.src.as_str()).ok_or_else(|| {
            AppError::Other(format!(
                "The planner returned an unknown source: {}",
                placement.src
            ))
        })?;
        let kind = match placement.kind {
            crate::planner::PlacementKind::Original => "original",
            crate::planner::PlacementKind::Link => "link",
            crate::planner::PlacementKind::Copy => "copy",
        };
        let directory = Path::new(&placement.dest).parent().ok_or_else(|| {
            AppError::Other(format!("output path has no parent: {}", placement.dest))
        })?;
        let source_key = manifest::source_key(&item.source, item.size_bytes);
        let placement_key = manifest::placement_key(kind, &placement.tags, directory, output_root);
        if !manifest
            .is_done_at_destination(&source_key, &placement_key, Path::new(&placement.dest))
            .map_err(AppError::Other)?
        {
            pending.push(placement.clone());
        }
    }
    Ok(pending)
}

fn plan_with_existing_paths(
    items: &[MediaItem],
    settings: &PlannerSettings,
    output_root: &Path,
    manifest_destinations: &HashMap<String, String>,
) -> Result<Vec<crate::planner::Placement>> {
    let directories = planner::destination_directories(items, settings, output_root);
    let existing =
        crate::platform::existing_paths_in_directories(&directories).map_err(AppError::Other)?;
    Ok(planner::plan_with_root_and_manifest(
        items,
        settings,
        output_root,
        &existing,
        manifest_destinations,
    ))
}

fn hydrate_live_video_durations(
    entries: &mut [crate::archive::IndexedEntry],
) -> std::result::Result<(), String> {
    let photo_keys = entries
        .iter()
        .filter(|entry| entry.entry_type == crate::archive::EntryType::Photo)
        .map(|entry| {
            let (parent, base) = crate::takeout::matcher::live_path_key(&entry.path);
            (entry.archive_path.to_lowercase(), parent, base)
        })
        .collect::<HashSet<_>>();
    for entry in entries.iter_mut().filter(|entry| {
        entry.entry_type == crate::archive::EntryType::Video
            && entry.video_duration_seconds.is_none()
    }) {
        let (parent, base) = crate::takeout::matcher::live_path_key(&entry.path);
        if photo_keys.contains(&(entry.archive_path.to_lowercase(), parent, base)) {
            entry.video_duration_seconds = crate::takeout::read_video_duration_seconds(
                &entry.archive_path,
                &entry.entry_path,
            )?;
        }
    }
    Ok(())
}

fn items_from_entries(
    entries: Vec<crate::archive::IndexedEntry>,
    time_zone: crate::settings::TimeZoneMode,
) -> std::result::Result<Vec<MediaItem>, String> {
    let computer_timezone = if time_zone == crate::settings::TimeZoneMode::Utc {
        None
    } else {
        Some(crate::metadata::computer_timezone_name()?)
    };
    let candidates = entries
        .iter()
        .filter(|entry| {
            matches!(
                entry.entry_type,
                crate::archive::EntryType::Photo | crate::archive::EntryType::Video
            )
        })
        .map(|entry| {
            let source = format!("{}::{}", entry.archive_path, entry.path);
            crate::takeout::matcher::LivePhotoCandidate {
                path: source,
                kind: if entry.entry_type == crate::archive::EntryType::Video {
                    crate::takeout::matcher::LiveMediaKind::Video
                } else {
                    crate::takeout::matcher::LiveMediaKind::Photo
                },
                taken_timestamp: entry
                    .metadata
                    .as_ref()
                    .and_then(crate::metadata::TakeoutMetadata::taken_timestamp),
                duration_seconds: entry.video_duration_seconds,
                embedded_motion_marker: entry.embedded_motion_video,
            }
        })
        .collect::<Vec<_>>();
    let live_detection = crate::takeout::matcher::match_live_photos(&candidates);
    let mut live_pairs = HashMap::new();
    for pair in live_detection.pairs {
        live_pairs.insert(
            pair.photo_path.clone(),
            (
                pair.video_path.clone(),
                pair.content_identifier.clone(),
                false,
            ),
        );
        live_pairs.insert(
            pair.video_path,
            (pair.photo_path, pair.content_identifier, true),
        );
    }
    let mut live_warnings = HashMap::new();
    for warning in live_detection.warnings {
        for path in warning.paths {
            live_warnings
                .entry(path)
                .or_insert_with(|| warning.message.clone());
        }
    }
    let mut locations = HashMap::with_capacity(entries.len());
    let mut items = entries
        .into_iter()
        .filter(|entry| {
            matches!(
                entry.entry_type,
                crate::archive::EntryType::Photo
                    | crate::archive::EntryType::Video
                    | crate::archive::EntryType::Other
            )
        })
        .map(|entry| {
            let source = format!("{}::{}", entry.archive_path, entry.path);
            locations.insert(
                source.clone(),
                (
                    entry.archive_path.clone(),
                    entry.entry_path.clone(),
                    entry.path.clone(),
                ),
            );
            let albums = crate::takeout::matcher::album_membership(&entry.path);
            let metadata = entry.metadata;
            let timezone_name = crate::metadata::timezone_name_for_item(
                time_zone,
                metadata.as_ref(),
                computer_timezone.as_deref(),
            )?;
            let timezone = timezone_name.parse::<chrono_tz::Tz>().map_err(|_| {
                format!("The resolved time zone is not a recognized IANA zone: {timezone_name}")
            })?;
            let date_time = metadata
                .as_ref()
                .and_then(crate::metadata::TakeoutMetadata::taken_timestamp)
                .and_then(|timestamp| chrono::DateTime::<chrono::Utc>::from_timestamp(timestamp, 0))
                .map(|datetime| datetime.with_timezone(&timezone));
            let date = date_time
                .as_ref()
                .map(|datetime| datetime.format("%Y-%m-%d").to_string());
            let time = date_time
                .as_ref()
                .map(|datetime| datetime.format("%H%M%S").to_string());
            let is_photo = entry.entry_type == crate::archive::EntryType::Photo;
            let is_video = entry.entry_type == crate::archive::EntryType::Video;
            let edited = crate::takeout::matcher::is_edited_path(&entry.path);
            let live_pair = live_pairs.get(&source);
            let live_warning = live_warnings.get(&source).cloned();
            Ok(MediaItem {
                source,
                filename: Path::new(&entry.path)
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| entry.path.clone()),
                album: albums.first().cloned(),
                additional_albums: albums.into_iter().skip(1).collect(),
                date,
                time,
                tz: Some(timezone_name),
                size_bytes: entry.size,
                is_photo,
                is_video,
                is_live: live_pair.is_some() || entry.embedded_motion_video,
                live_pair_partner: live_pair.map(|(partner, _, _)| partner.clone()),
                live_content_identifier: live_pair.map(|(_, identifier, _)| identifier.clone()),
                live_pair_video: live_pair.is_some_and(|(_, _, is_pair_video)| *is_pair_video),
                embedded_motion_video: entry.embedded_motion_video,
                live_warning,
                favorite: metadata.as_ref().is_some_and(|fields| fields.favorited),
                archived: entry.archived,
                trashed: entry.trashed,
                edited,
                has_json: entry.sidecar_path.is_some(),
                duplicate_of: None,
                metadata,
            })
        })
        .collect::<std::result::Result<Vec<_>, String>>()?;
    mark_duplicate_media(&mut items, &locations)?;
    Ok(items)
}

fn mark_duplicate_media(
    items: &mut [MediaItem],
    locations: &HashMap<String, (String, String, String)>,
) -> std::result::Result<(), String> {
    let mut candidates = HashMap::<(String, u64, Option<i64>), Vec<usize>>::new();
    for (index, item) in items.iter().enumerate() {
        if !item.is_photo && !item.is_video {
            continue;
        }
        let photo_taken_time = item
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.photo_taken_time);
        candidates
            .entry((
                crate::takeout::matcher::duplicate_group_name(&item.filename),
                item.size_bytes,
                photo_taken_time,
            ))
            .or_default()
            .push(index);
    }

    let mut archive_sources = HashMap::<String, Box<dyn crate::takeout::Source>>::new();
    let mut duplicate_groups = Vec::<Vec<usize>>::new();
    for ((_, _, photo_taken_time), indices) in candidates {
        if indices.len() < 2 {
            continue;
        }
        if photo_taken_time.is_some() {
            duplicate_groups.push(indices);
            continue;
        }

        let mut by_prefix_hash = HashMap::<String, Vec<usize>>::new();
        for index in indices {
            let location = locations.get(&items[index].source).ok_or_else(|| {
                format!("source location is unavailable for {}", items[index].source)
            })?;
            if !archive_sources.contains_key(&location.0) {
                archive_sources.insert(
                    location.0.clone(),
                    crate::takeout::create_source(Path::new(&location.0))?,
                );
            }
            let source = archive_sources
                .get(&location.0)
                .ok_or_else(|| format!("source is unavailable for {}", location.0))?;
            let prefix_hash = source.hash_entry_prefix(&location.1)?.to_hex().to_string();
            by_prefix_hash.entry(prefix_hash).or_default().push(index);
        }
        duplicate_groups.extend(by_prefix_hash.into_values().filter(|group| group.len() > 1));
    }

    for group in duplicate_groups {
        let mut ranked = group
            .iter()
            .map(|index| {
                let location = locations.get(&items[*index].source).ok_or_else(|| {
                    format!(
                        "source location is unavailable for {}",
                        items[*index].source
                    )
                })?;
                Ok((
                    *index,
                    crate::takeout::matcher::is_photos_year_folder_path(&location.2),
                    location.2.to_lowercase(),
                    location.0.to_lowercase(),
                ))
            })
            .collect::<std::result::Result<Vec<_>, String>>()?;
        ranked.sort_by(|left, right| {
            right
                .1
                .cmp(&left.1)
                .then_with(|| left.2.cmp(&right.2))
                .then_with(|| left.3.cmp(&right.3))
        });
        let canonical_index = ranked
            .first()
            .map(|candidate| candidate.0)
            .ok_or_else(|| "duplicate group has no canonical item".to_string())?;
        let canonical_source = items[canonical_index].source.clone();

        for duplicate_index in group {
            if duplicate_index == canonical_index {
                continue;
            }
            items[duplicate_index].duplicate_of = Some(canonical_source.clone());
            let duplicate_albums = items[duplicate_index]
                .album
                .iter()
                .chain(items[duplicate_index].additional_albums.iter())
                .cloned()
                .collect::<Vec<_>>();
            for album in duplicate_albums {
                let already_present = items[canonical_index]
                    .album
                    .iter()
                    .chain(items[canonical_index].additional_albums.iter())
                    .any(|existing| existing.eq_ignore_ascii_case(&album));
                if !already_present {
                    items[canonical_index].additional_albums.push(album);
                }
            }
        }
    }
    Ok(())
}

fn build_input_locators(
    entries: &[crate::archive::IndexedEntry],
) -> std::result::Result<HashMap<String, engine::InputLocator>, String> {
    let mut sources = HashMap::<String, Arc<dyn crate::takeout::Source>>::new();
    let mut locators = HashMap::with_capacity(entries.len());
    for entry in entries {
        let source_key = format!("{}::{}", entry.archive_path, entry.path);
        let source = if let Some(source) = sources.get(&entry.archive_path) {
            source.clone()
        } else {
            let source: Arc<dyn crate::takeout::Source> = Arc::from(crate::takeout::create_source(
                Path::new(&entry.archive_path),
            )?);
            sources.insert(entry.archive_path.clone(), source.clone());
            source
        };
        locators.insert(
            source_key,
            engine::InputLocator {
                source,
                archive_path: entry.archive_path.clone(),
                entry_path: entry.entry_path.clone(),
                video_duration_seconds: entry.video_duration_seconds,
            },
        );
    }
    Ok(locators)
}

fn archives_with_complete_coverage(
    items: &[MediaItem],
    placements: &[crate::planner::Placement],
    entries: &[crate::archive::IndexedEntry],
    source_paths: &[String],
    settings: &Settings,
) -> HashSet<String> {
    if !settings.delete_archives_after || settings.dry_run {
        return HashSet::new();
    }
    let placed = placements
        .iter()
        .map(|placement| placement.src.as_str())
        .collect::<HashSet<_>>();
    let items_by_source = items
        .iter()
        .map(|item| (item.source.as_str(), item))
        .collect::<HashMap<_, _>>();
    let mut items_by_archive = HashMap::<&str, Vec<&MediaItem>>::new();
    for item in items {
        if let Some((archive, _)) = item.source.split_once("::") {
            items_by_archive.entry(archive).or_default().push(item);
        }
    }

    let mut entries_by_archive = HashMap::<&str, Vec<&crate::archive::IndexedEntry>>::new();
    let mut covered_sidecars = HashSet::<(&str, &str)>::new();
    for entry in entries {
        if entry.entry_type == crate::archive::EntryType::AlbumMetadata {
            continue;
        }
        entries_by_archive
            .entry(entry.archive_path.as_str())
            .or_default()
            .push(entry);
        if matches!(
            entry.entry_type,
            crate::archive::EntryType::Photo | crate::archive::EntryType::Video
        ) {
            let source = format!("{}::{}", entry.archive_path, entry.path);
            if let Some(item) = items_by_source.get(source.as_str()).copied() {
                if item.metadata.is_some() && item_is_covered(item, &placed, settings) {
                    if let (Some(archive), Some(path)) = (
                        entry.sidecar_archive_path.as_deref(),
                        entry.sidecar_path.as_deref(),
                    ) {
                        covered_sidecars.insert((archive, path));
                    }
                }
            }
        }
    }

    source_paths
        .iter()
        .filter(|archive| is_archive_path(archive))
        .filter(|archive| {
            let archive_items = items_by_archive.get(archive.as_str());
            let archive_entries = entries_by_archive.get(archive.as_str());
            archive_items.is_some_and(|items| !items.is_empty())
                && archive_items
                    .into_iter()
                    .flatten()
                    .all(|item| item_is_covered(item, &placed, settings))
                && archive_entries.is_some_and(|entries| !entries.is_empty())
                && archive_entries
                    .into_iter()
                    .flatten()
                    .all(|entry| match entry.entry_type {
                        crate::archive::EntryType::Photo | crate::archive::EntryType::Video => true,
                        crate::archive::EntryType::JsonSidecar => {
                            covered_sidecars.contains(&(archive.as_str(), entry.path.as_str()))
                        }
                        crate::archive::EntryType::AlbumMetadata => true,
                        crate::archive::EntryType::Other => {
                            let source = format!("{}::{}", entry.archive_path, entry.path);
                            placed.contains(source.as_str())
                        }
                    })
        })
        .cloned()
        .collect()
}

fn item_is_covered(item: &MediaItem, placed: &HashSet<&str>, settings: &Settings) -> bool {
    placed.contains(item.source.as_str())
        || (settings.merge_live
            && settings.live_style == crate::livephoto::LivePhotoStyle::AndroidMotionPhoto
            && item.live_pair_video)
}

fn is_archive_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".zip") || lower.ends_with(".tgz") || lower.ends_with(".tar.gz")
}

fn app_config_file(app: &tauri::AppHandle, file_name: &str) -> Result<PathBuf> {
    let directory = app
        .path()
        .app_config_dir()
        .map_err(|error| AppError::Other(format!("Could not locate settings folder: {error}")))?;
    fs::create_dir_all(&directory)
        .map_err(|error| AppError::Other(format!("Could not create settings folder: {error}")))?;
    Ok(directory.join(file_name))
}

fn save_settings_file(app: &tauri::AppHandle, settings: &Settings) -> Result<()> {
    let destination = app_config_file(app, "settings.json")?;
    let parent = destination
        .parent()
        .ok_or_else(|| AppError::Other("Settings folder is unavailable.".to_string()))?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".takeback-settings-")
        .tempfile_in(parent)
        .map_err(|error| AppError::Other(format!("Could not save settings: {error}")))?;
    serde_json::to_writer(temporary.as_file_mut(), settings)
        .map_err(|error| AppError::Other(format!("Could not serialize settings: {error}")))?;
    temporary
        .persist(&destination)
        .map_err(|error| AppError::Other(format!("Could not save settings: {}", error.error)))?;
    Ok(())
}

#[tauri::command]
pub fn pause_run() -> Result<bool> {
    engine::pause_run().map_err(AppError::Other)?;
    Ok(true)
}

#[tauri::command]
pub async fn run_status() -> Result<engine::RunStatus> {
    let running = ProcessingRunGuard::is_running();
    tauri::async_runtime::spawn_blocking(move || {
        engine::run_status(running).map_err(AppError::Other)
    })
    .await
    .map_err(|error| AppError::Other(format!("Could not read run status: {error}")))?
}

#[tauri::command]
pub fn resume_run() -> Result<bool> {
    engine::resume_run().map_err(AppError::Other)?;
    Ok(true)
}

#[tauri::command]
pub async fn get_report(app: tauri::AppHandle) -> Result<Option<ReportDocument>> {
    tauri::async_runtime::spawn_blocking(move || load_report(&app))
        .await
        .map_err(|error| AppError::Other(format!("Could not load report: {error}")))?
}

#[tauri::command]
pub async fn get_review_items(app: tauri::AppHandle) -> Result<Vec<ReportItem>> {
    tauri::async_runtime::spawn_blocking(move || {
        Ok(load_report(&app)?
            .map(|report| report.items)
            .unwrap_or_default())
    })
    .await
    .map_err(|error| AppError::Other(format!("Could not load review items: {error}")))?
}

#[tauri::command]
pub async fn apply_review_fix(
    item_id: String,
    choice: String,
    picked_date: Option<String>,
    app: tauri::AppHandle,
) -> Result<bool> {
    tauri::async_runtime::spawn_blocking(move || {
        apply_review_fix_blocking(item_id, choice, picked_date, app)
    })
    .await
    .map_err(|error| AppError::Other(format!("Could not apply review fix: {error}")))?
}

fn load_report(app: &tauri::AppHandle) -> Result<Option<ReportDocument>> {
    let path = app_config_file(app, "last-report.json")?;
    ReportDocument::load(&path).map_err(AppError::Other)
}

fn apply_review_fix_blocking(
    item_id: String,
    choice: String,
    picked_date: Option<String>,
    app: tauri::AppHandle,
) -> Result<bool> {
    let report_path = app_config_file(&app, "last-report.json")?;
    let mut report = ReportDocument::load(&report_path)
        .map_err(AppError::Other)?
        .ok_or_else(|| AppError::Other("There is no completed run to review.".to_string()))?;
    let item_index = report
        .items
        .iter()
        .position(|item| item.id == item_id)
        .ok_or_else(|| AppError::Other("That review item is no longer available.".to_string()))?;

    if choice != "leave" {
        let destination = report.items[item_index]
            .destination
            .as_deref()
            .map(Path::new)
            .ok_or_else(|| AppError::Other("This review item has no output file.".to_string()))?;
        if !crate::metadata::supports_embedded_metadata(destination) {
            return Err(AppError::Other(format!(
                "Metadata cannot be safely written to {}.",
                destination.display()
            )));
        }
        let resource_dir = app.path().resource_dir().map_err(|error| {
            AppError::Other(format!("Could not locate application resources: {error}"))
        })?;
        let exiftool_path =
            crate::platform::exiftool_path(&resource_dir).map_err(AppError::Other)?;
        let mut exiftool =
            crate::metadata::ExifTool::new(&exiftool_path).map_err(AppError::Other)?;
        let timestamp = match choice.as_str() {
            "fileDate" => exiftool
                .read_taken_timestamp(destination)
                .map_err(AppError::Other)?
                .ok_or_else(|| {
                    AppError::Other("No date was found in the photo metadata.".to_string())
                })?,
            "pickDate" => {
                let picked_date = picked_date.ok_or_else(|| {
                    AppError::Other("Choose a date before applying this change.".to_string())
                })?;
                let local = chrono::NaiveDateTime::parse_from_str(&picked_date, "%Y-%m-%dT%H:%M")
                    .map_err(|error| AppError::Other(format!("Invalid date: {error}")))?;
                chrono::Local
                    .from_local_datetime(&local)
                    .single()
                    .ok_or_else(|| {
                        AppError::Other(
                            "That local time is ambiguous or does not exist.".to_string(),
                        )
                    })?
                    .timestamp()
            }
            _ => {
                return Err(AppError::Other("Choose a valid review action.".to_string()));
            }
        };
        let metadata = crate::metadata::TakeoutMetadata {
            photo_taken_time: Some(timestamp),
            ..Default::default()
        };
        let parent = destination
            .parent()
            .ok_or_else(|| AppError::Other("Output file has no parent folder.".to_string()))?;
        let staged = tempfile::Builder::new()
            .prefix(".takeback-review-")
            .suffix(
                &destination
                    .extension()
                    .map(|extension| format!(".{}", extension.to_string_lossy()))
                    .unwrap_or_default(),
            )
            .tempfile_in(parent)
            .map_err(|error| AppError::Other(format!("Could not stage review update: {error}")))?;
        fs::copy(destination, staged.path())
            .map_err(|error| AppError::Other(format!("Could not stage output file: {error}")))?;
        let staged_path = staged.into_temp_path();
        exiftool
            .write(
                &staged_path,
                &metadata,
                match report.settings.time_zone {
                    crate::settings::TimeZoneMode::Local => crate::planner::TimeZoneMode::Local,
                    crate::settings::TimeZoneMode::Computer => {
                        crate::planner::TimeZoneMode::Computer
                    }
                    crate::settings::TimeZoneMode::Utc => crate::planner::TimeZoneMode::Utc,
                },
                None,
                false,
                false,
                false,
            )
            .map_err(AppError::Other)?;

        let backup = tempfile::Builder::new()
            .prefix(".takeback-review-backup-")
            .tempfile_in(parent)
            .map_err(|error| AppError::Other(format!("Could not protect output file: {error}")))?
            .into_temp_path();
        fs::remove_file(&backup).map_err(|error| {
            AppError::Other(format!("Could not prepare output replacement: {error}"))
        })?;
        fs::rename(destination, &backup).map_err(|error| {
            AppError::Other(format!("Could not stage existing output: {error}"))
        })?;
        if let Err(error) = fs::rename(&staged_path, destination) {
            return match fs::rename(&backup, destination) {
                Ok(()) => Err(AppError::Other(format!(
                    "Could not replace the output file: {error}"
                ))),
                Err(restore_error) => {
                    let backup_path = backup.to_path_buf();
                    std::mem::forget(backup);
                    Err(AppError::Other(format!(
                        "Could not replace the output file: {error}; restoring the original also failed: {restore_error}. The original output is preserved at {}.",
                        backup_path.display()
                    )))
                }
            };
        }
        drop(backup);
        report.summary.dates_restored = report.summary.dates_restored.saturating_add(1);
    }

    report.items.remove(item_index);
    report.summary.attention.retain(|item| item.id != item_id);
    report.save(&report_path).map_err(AppError::Other)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;
    use zip::write::FileOptions;

    #[test]
    fn processing_guard_allows_only_one_active_run() {
        let guard = ProcessingRunGuard::acquire().expect("acquire processing guard");
        assert!(ProcessingRunGuard::is_running());
        assert!(ProcessingRunGuard::acquire().is_none());
        drop(guard);
        assert!(!ProcessingRunGuard::is_running());
    }

    fn write_index_test_archive(path: &Path, entries: &[(&str, &[u8])]) {
        let file = fs::File::create(path).expect("create test archive");
        let mut archive = zip::ZipWriter::new(file);
        let options = FileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (entry_path, contents) in entries {
            archive
                .start_file(*entry_path, options)
                .expect("start test entry");
            archive.write_all(contents).expect("write test entry");
        }
        archive.finish().expect("finish test archive");
    }

    fn movie_with_duration() -> Vec<u8> {
        fn atom(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
            let mut bytes = Vec::new();
            bytes.extend_from_slice(&((payload.len() + 8) as u32).to_be_bytes());
            bytes.extend_from_slice(kind);
            bytes.extend_from_slice(payload);
            bytes
        }

        let mut header = vec![0, 0, 0, 0];
        header.extend_from_slice(&0_u32.to_be_bytes());
        header.extend_from_slice(&0_u32.to_be_bytes());
        header.extend_from_slice(&1_000_u32.to_be_bytes());
        header.extend_from_slice(&2_500_u32.to_be_bytes());
        atom(b"moov", &atom(b"mvhd", &header))
    }

    #[test]
    fn media_items_store_timezone_adjusted_date_and_time() {
        let entry = crate::archive::IndexedEntry {
            archive_path: "takeout.zip".to_string(),
            path: "Takeout/Google Photos/Photos from 2021/photo.jpg".to_string(),
            entry_path: "Takeout/Google Photos/Photos from 2021/photo.jpg".to_string(),
            size: 10,
            entry_type: crate::archive::EntryType::Photo,
            album: None,
            trashed: false,
            archived: false,
            embedded_motion_video: false,
            video_duration_seconds: None,
            sidecar_archive_path: Some("takeout.zip".to_string()),
            sidecar_path: Some("photo.jpg.json".to_string()),
            sidecar_match: Some("exact".to_string()),
            metadata: Some(crate::metadata::TakeoutMetadata {
                photo_taken_time: Some(1_700_000_000),
                latitude: Some(47.6),
                longitude: Some(-122.3),
                ..Default::default()
            }),
        };

        let local = items_from_entries(vec![entry.clone()], crate::settings::TimeZoneMode::Local)
            .expect("resolve local time zone");
        assert_eq!(local[0].tz.as_deref(), Some("America/Los_Angeles"));
        assert_eq!(local[0].date.as_deref(), Some("2023-11-14"));
        assert_eq!(local[0].time.as_deref(), Some("141320"));

        let utc = items_from_entries(vec![entry.clone()], crate::settings::TimeZoneMode::Utc)
            .expect("resolve UTC time zone");
        assert_eq!(utc[0].tz.as_deref(), Some("UTC"));
        assert_eq!(utc[0].date.as_deref(), Some("2023-11-14"));
        assert_eq!(utc[0].time.as_deref(), Some("221320"));

        let computer_timezone =
            crate::metadata::computer_timezone_name().expect("resolve computer time zone");
        let computer =
            items_from_entries(vec![entry.clone()], crate::settings::TimeZoneMode::Computer)
                .expect("use computer time zone");
        assert_eq!(computer[0].tz.as_deref(), Some(computer_timezone.as_str()));

        let mut no_gps_entry = entry;
        no_gps_entry.path = "Takeout/Google Photos/Photos from 2021/no-gps.jpg".to_string();
        no_gps_entry.entry_path = no_gps_entry.path.clone();
        no_gps_entry.metadata.as_mut().unwrap().latitude = None;
        no_gps_entry.metadata.as_mut().unwrap().longitude = None;
        let local_fallback =
            items_from_entries(vec![no_gps_entry], crate::settings::TimeZoneMode::Local)
                .expect("fall back to computer time zone without GPS");
        assert_eq!(
            local_fallback[0].tz.as_deref(),
            Some(computer_timezone.as_str())
        );
    }

    #[test]
    fn preview_index_incrementally_adds_and_removes_archives() {
        let directory = tempdir().expect("create preview-index test folder");
        let media_archive = directory.path().join("media.zip");
        let sidecar_archive = directory.path().join("sidecar.zip");
        let media_entry = "Takeout/Google Photos/Photos from 2021/photo.jpg";
        write_index_test_archive(&media_archive, &[(media_entry, b"photo")]);
        write_index_test_archive(
            &sidecar_archive,
            &[(
                "Takeout/Google Photos/Photos from 2021/photo.jpg.json",
                br#"{"title":"photo.jpg"}"#,
            )],
        );
        let media_path = media_archive.to_string_lossy().into_owned();
        let sidecar_path = sidecar_archive.to_string_lossy().into_owned();
        let mut state = PreviewIndexData::default();

        scan_into_preview_state(&mut state, &[media_path.clone()], None, &mut |_| Ok(()))
            .expect("index first archive");
        let settings_hash = [7; 32];
        let cached_key = preview_cache_key(settings_hash, state.index_version);
        state.cached_preview = Some(CachedPreview {
            key: cached_key,
            result: PreviewResult::default(),
        });
        assert_eq!(
            state.indexed_archives.get(&archive_index_key(&media_path)),
            Some(&true)
        );

        let index_address = state
            .index
            .as_ref()
            .map(|index| index as *const crate::takeout::TempIndex as usize);
        write_index_test_archive(
            &media_archive,
            &[
                (media_entry, b"photo"),
                (
                    "Takeout/Google Photos/Photos from 2021/new.jpg",
                    b"new photo",
                ),
            ],
        );
        scan_into_preview_state(
            &mut state,
            &[media_path.clone(), sidecar_path.clone()],
            None,
            &mut |_| Ok(()),
        )
        .expect("index only newly added archive");
        assert_ne!(
            state.cached_preview.as_ref().map(|cached| cached.key),
            Some(preview_cache_key(settings_hash, state.index_version))
        );

        let index = state.index.as_ref().expect("persistent temp index");
        assert_eq!(
            state
                .index
                .as_ref()
                .map(|index| index as *const crate::takeout::TempIndex as usize),
            index_address
        );
        let media_entries = index.media_entries().expect("read indexed media");
        assert_eq!(media_entries.len(), 1);
        assert_eq!(
            media_entries[0].sidecar_archive_path.as_deref(),
            Some(sidecar_path.as_str())
        );

        let removal = remove_source_from_preview_state(&mut state, &sidecar_path)
            .expect("remove sidecar archive");
        assert_ne!(
            state.cached_preview.as_ref().map(|cached| cached.key),
            Some(preview_cache_key(settings_hash, state.index_version))
        );
        assert_eq!(removal.files, vec![media_path.clone()]);
        assert_eq!(removal.counts.photos, 1);
        let remaining_media = state
            .index
            .as_ref()
            .expect("keep temp index after removal")
            .media_entries()
            .expect("read remaining media");
        assert_eq!(remaining_media.len(), 1);
        assert_eq!(remaining_media[0].sidecar_path, None);
        assert!(!state
            .indexed_archives
            .contains_key(&archive_index_key(&sidecar_path)));
    }

    #[test]
    fn live_duration_hydration_reads_only_same_folder_video_candidates() {
        let directory = tempdir().expect("create duration candidate folder");
        let photos = directory.path().join("Photos");
        fs::create_dir_all(&photos).expect("create Photos folder");
        fs::write(photos.join("IMG_1.jpg"), b"synthetic photo").expect("write photo");
        fs::write(photos.join("IMG_1.mov"), movie_with_duration()).expect("write candidate video");
        fs::write(photos.join("IMG_2.mov"), movie_with_duration()).expect("write unmatched video");

        let mut entries = crate::takeout::index_explicit_sources(&[directory
            .path()
            .to_string_lossy()
            .into_owned()])
        .expect("index synthetic folder")
        .media_entries()
        .expect("read indexed media");
        hydrate_live_video_durations(&mut entries).expect("read candidate duration");

        let candidate = entries
            .iter()
            .find(|entry| entry.path.ends_with("IMG_1.mov"))
            .expect("find same-name video");
        let unmatched = entries
            .iter()
            .find(|entry| entry.path.ends_with("IMG_2.mov"))
            .expect("find unmatched video");
        assert_eq!(candidate.video_duration_seconds, Some(2.5));
        assert_eq!(unmatched.video_duration_seconds, None);
    }

    #[test]
    fn sample_takeout_archive_runs_through_index_plan_and_output() {
        let directory = tempdir().expect("create end-to-end test folder");
        let archive_path = directory.path().join("takeout-sample.zip");
        let archive_bytes = b"synthetic photo bytes";
        let archive_file = fs::File::create(&archive_path).expect("create sample Takeout");
        let mut archive = zip::ZipWriter::new(archive_file);
        let options = FileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let gif_bytes = b"synthetic gif bytes";
        let other_bytes = b"unrecognized file bytes";
        archive
            .start_file(
                "Takeout/Google Photos/Photos from 2021/IMG_0001.jpg",
                options,
            )
            .expect("start photo entry");
        archive
            .write_all(archive_bytes)
            .expect("write synthetic photo");
        archive
            .start_file(
                "Takeout/Google Photos/Photos from 2021/animation.gif",
                options,
            )
            .expect("start unsupported photo format");
        archive
            .write_all(gif_bytes)
            .expect("write unsupported photo format");
        archive
            .start_file(
                "Takeout/Google Photos/Photos from 2021/animation.gif.json",
                options,
            )
            .expect("start GIF metadata");
        archive
            .write_all(br#"{"title":"animation.gif","description":"caption"}"#)
            .expect("write GIF metadata");
        archive
            .start_file(
                "Takeout/Google Photos/Photos from 2021/notes.unknown",
                options,
            )
            .expect("start unknown file");
        archive.write_all(other_bytes).expect("write unknown file");
        archive
            .start_file("Takeout/Google Photos/unmatched-metadata.json", options)
            .expect("start JSON entry");
        archive.write_all(b"{}").expect("write JSON sidecar");
        archive.finish().expect("finish sample Takeout");
        let input_before = fs::read(&archive_path).expect("read input archive");

        let source_path = archive_path.to_string_lossy().into_owned();
        let (_, index) = crate::takeout::scan_paths_with_index(std::slice::from_ref(&source_path))
            .expect("index sample Takeout");
        let entries = index
            .processing_entries()
            .expect("read processable entries");
        let items = items_from_entries(entries.clone(), crate::settings::TimeZoneMode::Utc)
            .expect("create media items");
        assert_eq!(items.len(), 3);
        let locators = build_input_locators(&entries).expect("build archive locators");
        let settings = Settings::default();
        let output_root = directory.path().join("Photo Library");
        let planner_settings =
            PlannerSettings::from_preferences(&settings, Some(output_root.as_path()));
        let placements = plan_with_existing_output(&items, &planner_settings, &output_root)
            .expect("plan sample output");
        let summary = engine::start_indexed_run_with_progress(
            &planner_settings,
            &items,
            &placements,
            locators,
            HashSet::new(),
            Path::new(""),
            |_| Ok(()),
        )
        .expect("process sample Takeout");

        assert_eq!(summary.processed, 3);
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.other_files, 1);
        assert_eq!(
            fs::read(&placements[0].dest).expect("read processed photo"),
            archive_bytes
        );
        let gif_placement = placements
            .iter()
            .find(|placement| placement.src.ends_with("animation.gif"))
            .expect("GIF output placement");
        assert_eq!(
            fs::read(&gif_placement.dest).expect("read copied GIF"),
            gif_bytes
        );
        let other_placement = placements
            .iter()
            .find(|placement| placement.src.ends_with("notes.unknown"))
            .expect("unknown output placement");
        assert!(other_placement
            .dest
            .replace('\\', "/")
            .ends_with("/Other/notes.unknown"));
        assert_eq!(
            fs::read(&other_placement.dest).expect("read copied unknown file"),
            other_bytes
        );
        assert_eq!(
            fs::read(&archive_path).expect("confirm input archive remains unchanged"),
            input_before
        );
    }

    #[test]
    fn duplicate_media_prefers_year_copy_and_collects_other_albums() {
        let directory = tempdir().expect("create duplicate test folder");
        let photos_root = directory.path().join("Takeout/Google Photos");
        let prefix = vec![b'x'; 64 * 1024];
        let make_media = |tail| {
            let mut bytes = prefix.clone();
            bytes.extend_from_slice(&[tail; 128]);
            bytes
        };
        let files = [
            ("Photos from 2021/IMG_0001.jpg", make_media(b'a')),
            ("Album A/IMG_0001(1).jpg", make_media(b'b')),
            ("Album B/IMG_0001-edited.jpg", make_media(b'c')),
            ("Album C/IMG_0001(2).jpg", vec![b'y'; 64 * 1024 + 128]),
            ("Z Album/other.jpg", b"other-file".to_vec()),
            ("A Album/other(1).jpg", b"other-file".to_vec()),
            ("A Album/timed.jpg", b"timed-file".to_vec()),
            ("B Album/timed(1).jpg", b"timed-file".to_vec()),
            ("C Album/timed(2).jpg", b"not-timed!".to_vec()),
        ];
        for (relative, bytes) in files {
            let path = photos_root.join(relative);
            fs::create_dir_all(path.parent().expect("media parent")).expect("create media folder");
            fs::write(path, bytes).expect("write media file");
        }
        fs::write(
            photos_root.join("A Album/timed.jpg.json"),
            br#"{"photoTakenTime":{"timestamp":"100"}}"#,
        )
        .expect("write first timestamp sidecar");
        fs::write(
            photos_root.join("B Album/timed(1).jpg.json"),
            br#"{"photoTakenTime":{"timestamp":"200"}}"#,
        )
        .expect("write second timestamp sidecar");
        fs::write(
            photos_root.join("C Album/timed(2).jpg.json"),
            br#"{"photoTakenTime":{"timestamp":"100"}}"#,
        )
        .expect("write third timestamp sidecar");

        let archive_path = directory.path().join("Takeout");
        let (_, index) =
            crate::takeout::scan_paths_with_index(&[archive_path.to_string_lossy().into_owned()])
                .expect("index duplicate files");
        let entries = index
            .processing_entries()
            .expect("read processable entries");
        let items = items_from_entries(entries, crate::settings::TimeZoneMode::Utc)
            .expect("detect duplicates");
        let canonical = items
            .iter()
            .find(|item| item.source.ends_with("Photos from 2021/IMG_0001.jpg"))
            .expect("year-folder canonical");

        assert_eq!(
            canonical.additional_albums,
            vec!["Album A".to_string(), "Album B".to_string()]
        );
        for path in ["Album A/IMG_0001(1).jpg", "Album B/IMG_0001-edited.jpg"] {
            let duplicate = items
                .iter()
                .find(|item| item.source.ends_with(path))
                .expect("duplicate item");
            assert_eq!(
                duplicate.duplicate_of.as_deref(),
                Some(canonical.source.as_str())
            );
        }
        assert!(items
            .iter()
            .find(|item| item.source.ends_with("Album C/IMG_0001(2).jpg"))
            .is_some_and(|item| item.duplicate_of.is_none()));

        let path_canonical = items
            .iter()
            .find(|item| item.source.ends_with("A Album/other(1).jpg"))
            .expect("first-path canonical");
        assert!(items
            .iter()
            .find(|item| item.source.ends_with("Z Album/other.jpg"))
            .is_some_and(|duplicate| {
                duplicate.duplicate_of.as_deref() == Some(path_canonical.source.as_str())
            }));
        let timed_canonical = items
            .iter()
            .find(|item| item.source.ends_with("A Album/timed.jpg"))
            .expect("timestamp canonical");
        assert!(items
            .iter()
            .find(|item| item.source.ends_with("B Album/timed(1).jpg"))
            .is_some_and(|item| item.duplicate_of.is_none()));
        assert!(items
            .iter()
            .find(|item| item.source.ends_with("C Album/timed(2).jpg"))
            .is_some_and(|item| {
                item.duplicate_of.as_deref() == Some(timed_canonical.source.as_str())
            }));

        let mut planning_items = items.clone();
        for item in &mut planning_items {
            item.has_json = true;
        }
        let duplicate_placements = planner::plan(
            &planning_items,
            &PlannerSettings {
                album_mode: crate::planner::AlbumMode::Move,
                multi_album: crate::planner::MultiAlbumMode::All,
                ignore_duplicates: true,
                ..PlannerSettings::default()
            },
        );
        assert!(planning_items
            .iter()
            .filter(|item| item.duplicate_of.is_some())
            .all(|duplicate| !duplicate_placements
                .iter()
                .any(|placement| placement.src == duplicate.source)));

        let mut canonical = canonical.clone();
        canonical.has_json = true;
        for (album_mode, expected_kind) in [
            (
                crate::planner::AlbumMode::Move,
                crate::planner::PlacementKind::Copy,
            ),
            (
                crate::planner::AlbumMode::Link,
                crate::planner::PlacementKind::Link,
            ),
            (
                crate::planner::AlbumMode::Copy,
                crate::planner::PlacementKind::Copy,
            ),
        ] {
            let settings = PlannerSettings {
                album_mode,
                multi_album: crate::planner::MultiAlbumMode::All,
                ignore_duplicates: true,
                ..PlannerSettings::default()
            };
            let placements = planner::plan(&[canonical.clone()], &settings);
            for album in ["Album A", "Album B"] {
                assert!(placements.iter().any(|placement| {
                    placement
                        .dest
                        .replace('\\', "/")
                        .contains(&format!("/{album}/"))
                        && placement.kind == expected_kind
                }));
            }
        }
    }

    #[test]
    fn archive_deletion_waits_until_its_outputs_are_written() {
        let directory = tempdir().expect("create deletion test folder");
        let archive_path = directory.path().join("takeout-delete.zip");
        let archive_file = fs::File::create(&archive_path).expect("create sample Takeout");
        let mut archive = zip::ZipWriter::new(archive_file);
        archive
            .start_file(
                "Takeout/Google Photos/Photos from 2021/IMG_0002.jpg",
                FileOptions::default().compression_method(zip::CompressionMethod::Stored),
            )
            .expect("start photo entry");
        archive
            .write_all(b"synthetic photo bytes")
            .expect("write photo");
        archive.finish().expect("finish sample Takeout");

        let source_path = archive_path.to_string_lossy().into_owned();
        let (_, index) = crate::takeout::scan_paths_with_index(std::slice::from_ref(&source_path))
            .expect("index sample Takeout");
        let entries = index.media_entries().expect("read indexed media");
        let items = items_from_entries(entries.clone(), crate::settings::TimeZoneMode::Utc)
            .expect("create media items");
        let locators = build_input_locators(&entries).expect("build archive locators");
        let mut settings = Settings::default();
        settings.delete_archives_after = true;
        let output_root = directory.path().join("Photo Library");
        let planner_settings =
            PlannerSettings::from_preferences(&settings, Some(output_root.as_path()));
        let placements = plan_with_existing_output(&items, &planner_settings, &output_root)
            .expect("plan sample output");
        let archives_to_delete = std::iter::once(source_path.clone()).collect();

        let summary = engine::start_indexed_run_with_progress_and_deleter(
            &planner_settings,
            &items,
            &placements,
            locators,
            archives_to_delete,
            Path::new(""),
            |path| {
                fs::remove_file(path)
                    .map_err(|error| format!("test removal {}: {error}", path.display()))
            },
            |_| Ok(()),
        )
        .expect("process sample Takeout");

        assert_eq!(summary.processed, 1);
        assert_eq!(summary.archives_deleted, 1);
        assert!(!archive_path.exists());
        assert!(Path::new(&placements[0].dest).exists());
    }

    #[test]
    fn archive_deletion_candidates_require_complete_media_coverage_and_no_dry_run() {
        let archive = "C:\\takeout-001.zip".to_string();
        let item = MediaItem {
            source: format!("{archive}::Photos/photo.jpg"),
            filename: "photo.jpg".to_string(),
            album: None,
            additional_albums: Vec::new(),
            date: None,
            time: None,
            tz: None,
            size_bytes: 10,
            is_photo: true,
            is_video: false,
            is_live: false,
            live_pair_partner: None,
            live_content_identifier: None,
            live_pair_video: false,
            embedded_motion_video: false,
            live_warning: None,
            favorite: false,
            archived: false,
            trashed: false,
            edited: false,
            has_json: false,
            duplicate_of: None,
            metadata: None,
        };
        let placement = crate::planner::Placement {
            src: item.source.clone(),
            dest: "C:\\output\\photo.jpg".to_string(),
            kind: crate::planner::PlacementKind::Original,
            tags: Vec::new(),
            live_action: None,
        };
        let entry = crate::archive::IndexedEntry {
            archive_path: archive.clone(),
            path: "Photos/photo.jpg".to_string(),
            entry_path: "Photos/photo.jpg".to_string(),
            size: 10,
            entry_type: crate::archive::EntryType::Photo,
            album: None,
            trashed: false,
            archived: false,
            embedded_motion_video: false,
            video_duration_seconds: None,
            sidecar_archive_path: None,
            sidecar_path: None,
            sidecar_match: None,
            metadata: None,
        };
        let mut settings = Settings::default();
        settings.delete_archives_after = true;
        let source_paths = vec![archive.clone()];

        assert_eq!(
            archives_with_complete_coverage(
                std::slice::from_ref(&item),
                std::slice::from_ref(&placement),
                std::slice::from_ref(&entry),
                &source_paths,
                &settings,
            ),
            HashSet::from([archive.clone()])
        );
        settings.dry_run = true;
        assert!(archives_with_complete_coverage(
            std::slice::from_ref(&item),
            std::slice::from_ref(&placement),
            std::slice::from_ref(&entry),
            &source_paths,
            &settings,
        )
        .is_empty());
        settings.dry_run = false;
        assert!(archives_with_complete_coverage(
            std::slice::from_ref(&item),
            &[],
            std::slice::from_ref(&entry),
            &source_paths,
            &settings,
        )
        .is_empty());
        let other_entry = crate::archive::IndexedEntry {
            entry_type: crate::archive::EntryType::Other,
            path: "other.bin".to_string(),
            entry_path: "other.bin".to_string(),
            ..entry.clone()
        };
        assert!(archives_with_complete_coverage(
            std::slice::from_ref(&item),
            std::slice::from_ref(&placement),
            &[entry.clone(), other_entry],
            &source_paths,
            &settings,
        )
        .is_empty());
        let album_metadata = crate::archive::IndexedEntry {
            entry_type: crate::archive::EntryType::AlbumMetadata,
            path: "Photos from 2021/metadata.json".to_string(),
            entry_path: "Photos from 2021/metadata.json".to_string(),
            ..entry.clone()
        };
        assert_eq!(
            archives_with_complete_coverage(
                std::slice::from_ref(&item),
                std::slice::from_ref(&placement),
                &[entry.clone(), album_metadata],
                &source_paths,
                &settings,
            ),
            HashSet::from([archive.clone()])
        );
        let orphan_sidecar = crate::archive::IndexedEntry {
            entry_type: crate::archive::EntryType::JsonSidecar,
            path: "orphan.json".to_string(),
            entry_path: "orphan.json".to_string(),
            ..entry
        };
        assert!(archives_with_complete_coverage(
            std::slice::from_ref(&item),
            std::slice::from_ref(&placement),
            &[orphan_sidecar],
            &source_paths,
            &settings,
        )
        .is_empty());
    }
}
