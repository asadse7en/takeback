use crate::archive::AddSourcesResult;
use crate::engine;
use crate::error::{AppError, Result};
use crate::organize::{self, PreviewResult};
use crate::pipeline::ProcessingStartResult;
use crate::planner::{self, MediaItem, PlannerSettings};
use crate::report::{ReportDocument, ReportExport, ReportFormat, ReportItem};
use crate::settings::Settings;
use chrono::TimeZone;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::{Manager, State};

#[derive(Clone, Default)]
pub struct PreviewIndexState(Arc<Mutex<PreviewIndexData>>);

#[derive(Default)]
struct PreviewIndexData {
    index: Option<crate::takeout::TempIndex>,
    source_paths: Vec<String>,
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
) -> Result<bool> {
    let data = preview_state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let paths = {
            let mut state = data.lock().map_err(|error| {
                AppError::Other(format!("Source index is unavailable: {error}"))
            })?;
            state.source_paths.retain(|path| path != &source_id);
            state.source_paths.clone()
        };
        let rebuilt = if paths.is_empty() {
            None
        } else {
            Some(crate::takeout::index_explicit_sources(&paths).map_err(AppError::Other)?)
        };
        let mut state = data
            .lock()
            .map_err(|error| AppError::Other(format!("Source index is unavailable: {error}")))?;
        state.index = rebuilt;
        Ok(true)
    })
    .await
    .map_err(|error| AppError::Other(format!("Could not remove source: {error}")))?
}

#[tauri::command]
pub fn get_preview(
    settings: Settings,
    app: tauri::AppHandle,
    preview_state: State<'_, PreviewIndexState>,
) -> Result<PreviewResult> {
    let (items, sample_items) = {
        let state = preview_state
            .0
            .lock()
            .map_err(|error| AppError::Other(format!("Preview index is unavailable: {error}")))?;
        let Some(index) = state.index.as_ref() else {
            return Ok(PreviewResult {
                warnings: vec!["Add and scan Takeout files to see a real preview.".to_string()],
                ..PreviewResult::default()
            });
        };
        let all_items = items_from_entries(index.media_entries().map_err(AppError::Other)?);
        let sample_sources = index
            .preview_entries(8)
            .map_err(AppError::Other)?
            .into_iter()
            .map(|entry| format!("{}::{}", entry.archive_path, entry.path))
            .collect::<HashSet<_>>();
        let sample_items = all_items
            .iter()
            .filter(|item| sample_sources.contains(&item.source))
            .cloned()
            .collect::<Vec<_>>();
        (all_items, sample_items)
    };

    let pictures = app.path().picture_dir().map_err(|error| {
        AppError::Other(format!("Could not locate the Pictures folder: {error}"))
    })?;
    let output_root = resolve_output_root(&settings, &pictures)?;
    let planner_settings =
        PlannerSettings::from_preferences(&settings, Some(output_root.as_path()));
    let placements = plan_with_existing_output(&items, &planner_settings, &output_root)?;
    let sample_placements =
        plan_with_existing_output(&sample_items, &planner_settings, &output_root)?;
    let files = sample_placements
        .iter()
        .map(|placement| placement.dest.clone())
        .collect();
    let tree = organize::tree_from_placements(&output_root, &sample_placements);
    let required_bytes =
        crate::pipeline::estimate_required_bytes(&placements, &items).map_err(AppError::Other)?;
    let free_bytes = crate::platform::disk_free_bytes(&output_root).map_err(AppError::Other)?;

    Ok(PreviewResult {
        files,
        tree,
        warnings: Vec::new(),
        required_bytes,
        free_bytes,
    })
}

#[tauri::command]
pub async fn start_processing(
    settings: Settings,
    app: tauri::AppHandle,
    preview_state: State<'_, PreviewIndexState>,
) -> Result<ProcessingStartResult> {
    let index = preview_state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (entries, all_entries, source_paths) = {
            let guard = index
                .lock()
                .map_err(|error| AppError::Other(format!("Source index is unavailable: {error}")))?;
            let Some(index) = guard.index.as_ref() else {
                return Err(AppError::Other(
                    "Scan Takeout files before starting processing.".to_string(),
                ));
            };
            (
                index.media_entries().map_err(AppError::Other)?,
                index.entries().map_err(AppError::Other)?,
                guard.source_paths.clone(),
            )
        };
        let input_locators = build_input_locators(&entries).map_err(AppError::Other)?;
        let items = items_from_entries(entries);
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
        let planner_settings =
            PlannerSettings::from_preferences(&settings, Some(output_root.as_path()));
        let placements = plan_with_existing_output(&items, &planner_settings, &output_root)?;
        let archives_to_delete = archives_with_complete_coverage(
            &items,
            &placements,
            &all_entries,
            &source_paths,
            &settings,
        );
        let required_bytes = if settings.dry_run {
            0
        } else {
            crate::pipeline::estimate_required_bytes(&placements, &items)
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
                    "The output drive is short by {:.2} GB. Choose another drive, switch Copy to Link, delete processed archives, or reduce output options.",
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
        let summary = engine::start_indexed_run(
            &app,
            &planner_settings,
            &items,
            &placements,
            input_locators,
            archives_to_delete.clone(),
            &exiftool_path,
        )
        .map_err(AppError::Other)?;

        let mut warnings = summary.warnings.clone();
        let archive_count = source_paths
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
pub fn load_settings(app: tauri::AppHandle) -> Result<Settings> {
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
    let index_state = preview_state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (result, index) =
            crate::takeout::scan_paths_with_index(&paths).map_err(AppError::Other)?;
        let mut state = index_state
            .lock()
            .map_err(|error| AppError::Other(format!("Source index is unavailable: {error}")))?;
        state.index = Some(index);
        state.source_paths = paths;
        Ok(result)
    })
    .await
    .map_err(|error| AppError::Other(format!("Could not scan sources: {error}")))?
}

fn resolve_output_root(settings: &Settings, pictures: &Path) -> Result<PathBuf> {
    crate::platform::resolve_output_root(settings.output_folder.as_deref().map(Path::new), pictures)
        .map_err(AppError::Other)
}

fn plan_with_existing_output(
    items: &[MediaItem],
    settings: &PlannerSettings,
    output_root: &Path,
) -> Result<Vec<crate::planner::Placement>> {
    let initial = planner::plan_with_root(items, settings, output_root, &HashSet::new());
    let directories = initial
        .iter()
        .filter_map(|placement| Path::new(&placement.dest).parent().map(Path::to_path_buf))
        .collect::<HashSet<_>>();
    let existing =
        crate::platform::existing_paths_in_directories(&directories).map_err(AppError::Other)?;
    Ok(planner::plan_with_root(
        items,
        settings,
        output_root,
        &existing,
    ))
}

fn items_from_entries(entries: Vec<crate::archive::IndexedEntry>) -> Vec<MediaItem> {
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
                embedded_video_verified: entry.embedded_motion_video,
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
    entries
        .into_iter()
        .filter(|entry| {
            matches!(
                entry.entry_type,
                crate::archive::EntryType::Photo | crate::archive::EntryType::Video
            )
        })
        .map(|entry| {
            let albums = crate::takeout::matcher::album_membership(&entry.path);
            let date = entry
                .metadata
                .as_ref()
                .and_then(crate::metadata::TakeoutMetadata::taken_timestamp)
                .and_then(|timestamp| chrono::DateTime::<chrono::Utc>::from_timestamp(timestamp, 0))
                .map(|datetime| datetime.format("%Y-%m-%d").to_string());
            let is_video = entry.entry_type == crate::archive::EntryType::Video;
            let edited = crate::takeout::matcher::is_edited_path(&entry.path);
            let source = format!("{}::{}", entry.archive_path, entry.path);
            let live_pair = live_pairs.get(&source);
            let live_warning = live_warnings.get(&source).cloned();
            let metadata = entry.metadata;
            MediaItem {
                source,
                filename: Path::new(&entry.path)
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| entry.path.clone()),
                album: albums.first().cloned(),
                additional_albums: albums.into_iter().skip(1).collect(),
                date,
                size_bytes: entry.size,
                is_photo: !is_video,
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
            }
        })
        .collect()
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
    source_paths
        .iter()
        .filter(|archive| is_archive_path(archive))
        .filter(|archive| {
            let prefix = format!("{archive}::");
            let archive_items = items
                .iter()
                .filter(|item| item.source.starts_with(&prefix))
                .collect::<Vec<_>>();
            let archive_entries = entries
                .iter()
                .filter(|entry| entry.archive_path == **archive)
                .collect::<Vec<_>>();
            !archive_items.is_empty()
                && archive_items.iter().all(|item| {
                    placed.contains(item.source.as_str())
                        || (settings.merge_live
                            && settings.live_style
                                == crate::livephoto::LivePhotoStyle::AndroidMotionPhoto
                            && item.live_pair_video)
                })
                && !archive_entries.is_empty()
                && archive_entries.iter().all(|entry| match entry.entry_type {
                    crate::archive::EntryType::Photo | crate::archive::EntryType::Video => true,
                    crate::archive::EntryType::JsonSidecar => items.iter().any(|item| {
                        let media_entry = entries.iter().find(|media_entry| {
                            format!("{}::{}", media_entry.archive_path, media_entry.path)
                                == item.source
                        });
                        media_entry.is_some_and(|media_entry| {
                            media_entry.sidecar_archive_path.as_deref() == Some(archive.as_str())
                                && media_entry.sidecar_path.as_deref() == Some(entry.path.as_str())
                                && item.metadata.is_some()
                                && (placed.contains(item.source.as_str())
                                    || (settings.merge_live
                                        && settings.live_style
                                            == crate::livephoto::LivePhotoStyle::AndroidMotionPhoto
                                        && item.live_pair_video))
                        })
                    }),
                    crate::archive::EntryType::Other => false,
                })
        })
        .cloned()
        .collect()
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
pub fn resume_run() -> Result<bool> {
    engine::resume_run().map_err(AppError::Other)?;
    Ok(true)
}

#[tauri::command]
pub fn get_report(app: tauri::AppHandle) -> Result<Option<ReportDocument>> {
    let path = app_config_file(&app, "last-report.json")?;
    ReportDocument::load(&path).map_err(AppError::Other)
}

#[tauri::command]
pub fn get_review_items(app: tauri::AppHandle) -> Result<Vec<ReportItem>> {
    Ok(get_report(app)?
        .map(|report| report.items)
        .unwrap_or_default())
}

#[tauri::command]
pub fn apply_review_fix(
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
    fn sample_takeout_archive_runs_through_index_plan_and_output() {
        let directory = tempdir().expect("create end-to-end test folder");
        let archive_path = directory.path().join("takeout-sample.zip");
        let archive_bytes = b"synthetic photo bytes";
        let archive_file = fs::File::create(&archive_path).expect("create sample Takeout");
        let mut archive = zip::ZipWriter::new(archive_file);
        let options = FileOptions::default().compression_method(zip::CompressionMethod::Stored);
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
            .start_file("Takeout/Google Photos/unmatched-metadata.json", options)
            .expect("start JSON entry");
        archive.write_all(b"{}").expect("write JSON sidecar");
        archive.finish().expect("finish sample Takeout");
        let input_before = fs::read(&archive_path).expect("read input archive");

        let source_path = archive_path.to_string_lossy().into_owned();
        let (_, index) = crate::takeout::scan_paths_with_index(std::slice::from_ref(&source_path))
            .expect("index sample Takeout");
        let entries = index.media_entries().expect("read indexed media");
        let items = items_from_entries(entries.clone());
        assert_eq!(items.len(), 1);
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

        assert_eq!(summary.processed, 1);
        assert_eq!(summary.failed, 0);
        assert_eq!(
            fs::read(&placements[0].dest).expect("read processed photo"),
            archive_bytes
        );
        assert_eq!(
            fs::read(&archive_path).expect("confirm input archive remains unchanged"),
            input_before
        );
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
        let items = items_from_entries(entries.clone());
        let locators = build_input_locators(&entries).expect("build archive locators");
        let mut settings = Settings::default();
        settings.delete_archives_after = true;
        let output_root = directory.path().join("Photo Library");
        let planner_settings =
            PlannerSettings::from_preferences(&settings, Some(output_root.as_path()));
        let placements = plan_with_existing_output(&items, &planner_settings, &output_root)
            .expect("plan sample output");
        let archives_to_delete = std::iter::once(source_path.clone()).collect();

        let summary = engine::start_indexed_run_with_progress(
            &planner_settings,
            &items,
            &placements,
            locators,
            archives_to_delete,
            Path::new(""),
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
