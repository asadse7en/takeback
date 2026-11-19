pub mod live;

pub use live::apply_live_style;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use zip::ZipArchive;

use crate::livephoto::LivePhotoStyle;
use crate::manifest::{self, RunManifest};
use crate::metadata::{
    is_large_video, supports_embedded_metadata, ExifTool, MetadataWriteRequest, TakeoutMetadata,
};
use crate::planner::{self, LiveAction, MediaItem, Placement, PlannerSettings};
use crate::platform::disk_free_bytes;
use crate::report::ReportItem;
use crate::takeout::Source;

pub fn status() -> &'static str {
    "idle"
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RunSummary {
    pub planned: u32,
    pub processed: u32,
    pub skipped: u32,
    pub failed: u32,
    pub files_without_json: u32,
    #[serde(default)]
    pub other_files: u32,
    pub collisions: u32,
    pub dates_restored: u32,
    pub locations_added: u32,
    pub live_photos_merged: u32,
    pub archives_deleted: u32,
    pub dry_run: bool,
    pub paused: bool,
    pub cancelled: bool,
    pub warnings: Vec<String>,
    pub problems: Vec<String>,
    pub attention: Vec<ReportItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RunStatus {
    pub running: bool,
    pub paused: bool,
    pub completed: u32,
    pub total: u32,
    pub summary: Option<RunSummary>,
}

#[derive(Clone)]
pub struct InputLocator {
    pub source: Arc<dyn Source>,
    pub archive_path: String,
    pub entry_path: String,
    pub video_duration_seconds: Option<f64>,
}

#[derive(Debug, Default)]
pub struct RunControl {
    paused: AtomicBool,
    cancelled: AtomicBool,
    space_paused: AtomicBool,
}

impl RunControl {
    fn reset(&self) {
        self.paused.store(false, Ordering::SeqCst);
        self.cancelled.store(false, Ordering::SeqCst);
        self.space_paused.store(false, Ordering::SeqCst);
    }

    pub fn pause(&self) {
        self.space_paused.store(false, Ordering::SeqCst);
        self.paused.store(true, Ordering::SeqCst);
    }

    fn pause_for_space(&self) {
        self.space_paused.store(true, Ordering::SeqCst);
        self.paused.store(true, Ordering::SeqCst);
    }

    pub fn resume(&self) {
        self.space_paused.store(false, Ordering::SeqCst);
        self.paused.store(false, Ordering::SeqCst);
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.paused.store(false, Ordering::SeqCst);
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    fn is_space_paused(&self) -> bool {
        self.space_paused.load(Ordering::SeqCst)
    }
}

fn run_control() -> &'static Mutex<RunControl> {
    static CONTROL: OnceLock<Mutex<RunControl>> = OnceLock::new();
    CONTROL.get_or_init(|| Mutex::new(RunControl::default()))
}

#[derive(Debug)]
pub struct RunStateStore {
    conn: Connection,
    path: PathBuf,
}

impl RunStateStore {
    pub fn new_temp() -> Result<Self, String> {
        let dir = std::env::temp_dir().join("takeback-run-state");
        fs::create_dir_all(&dir).map_err(|e| format!("create temp run dir: {e}"))?;
        let path = run_state_path();
        let conn = Connection::open(&path).map_err(|e| format!("open run state sqlite: {e}"))?;
        conn.busy_timeout(Duration::from_secs(2))
            .map_err(|error| format!("configure run state sqlite: {error}"))?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS run_state (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
            (),
        )
        .map_err(|e| format!("create run state table: {e}"))?;
        Ok(Self { conn, path })
    }

    pub fn set(&self, key: &str, value: &str) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO run_state (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                params![key, value],
            )
            .map_err(|e| format!("write run state {key}: {e}"))?;
        Ok(())
    }

    pub fn get(&self, key: &str) -> Result<Option<String>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT value FROM run_state WHERE key = ?1")
            .map_err(|e| format!("read run state {key}: {e}"))?;
        let value = stmt
            .query_row(params![key], |row| row.get::<_, String>(0))
            .optional()
            .map_err(|e| format!("read run state {key}: {e}"))?;
        Ok(value)
    }

    pub fn mark_progress(&self, completed: u32, total: u32, status: &str) -> Result<(), String> {
        self.set("progress.completed", &completed.to_string())?;
        self.set("progress.total", &total.to_string())?;
        self.set("progress.status", status)?;
        Ok(())
    }

    pub fn mark_finished(&self, summary: &RunSummary) -> Result<(), String> {
        self.set(
            "finished",
            if summary.cancelled {
                "cancelled"
            } else if summary.paused {
                "paused"
            } else {
                "complete"
            },
        )?;
        let serialized = serde_json::to_string(summary)
            .map_err(|error| format!("serialize run summary: {error}"))?;
        self.set("summary", &serialized)?;
        Ok(())
    }

    pub fn clear_summary(&self) -> Result<(), String> {
        self.conn
            .execute("DELETE FROM run_state WHERE key = 'summary'", ())
            .map_err(|error| format!("clear prior run summary: {error}"))?;
        Ok(())
    }

    pub fn path(&self) -> &Path {
        self.path.as_path()
    }
}

fn run_state_path() -> PathBuf {
    std::env::temp_dir()
        .join("takeback-run-state")
        .join(format!("run-{}.sqlite", std::process::id()))
}

pub fn run_status(running: bool) -> Result<RunStatus, String> {
    let path = run_state_path();
    if !path.exists() {
        return Ok(RunStatus {
            running,
            ..RunStatus::default()
        });
    }
    let connection =
        Connection::open(&path).map_err(|error| format!("open run status database: {error}"))?;
    connection
        .busy_timeout(Duration::from_secs(2))
        .map_err(|error| format!("configure run status database: {error}"))?;
    let state = RunStateStore {
        conn: connection,
        path,
    };
    let completed = state
        .get("progress.completed")?
        .map(|value| {
            value
                .parse::<u32>()
                .map_err(|error| format!("invalid completed run count: {error}"))
        })
        .transpose()?
        .unwrap_or_default();
    let total = state
        .get("progress.total")?
        .map(|value| {
            value
                .parse::<u32>()
                .map_err(|error| format!("invalid total run count: {error}"))
        })
        .transpose()?
        .unwrap_or_default();
    let summary = state
        .get("summary")?
        .map(|value| {
            serde_json::from_str(&value).map_err(|error| format!("parse run summary: {error}"))
        })
        .transpose()?;
    let paused = if running {
        run_control()
            .lock()
            .map_err(|error| format!("run control is unavailable: {error}"))?
            .is_paused()
    } else {
        false
    };
    Ok(RunStatus {
        running,
        paused,
        completed,
        total,
        summary,
    })
}

pub fn start_run(
    settings: &PlannerSettings,
    items: &[MediaItem],
    exiftool_path: &Path,
) -> Result<RunSummary, String> {
    let control = run_control();
    let state = RunStateStore::new_temp()?;
    let placements = planner::plan(items, settings);
    let has_metadata =
        !settings.dry_run && !placements.is_empty() && items.iter().any(requires_exiftool_metadata);
    let has_live_photo_writes = !settings.dry_run
        && placements
            .iter()
            .any(|placement| placement.live_action.is_some());
    let mut exiftool = if has_metadata || has_live_photo_writes {
        Some(ExifTool::new(exiftool_path)?)
    } else {
        None
    };
    let metadata_by_source: HashMap<&str, (&TakeoutMetadata, Option<&str>)> = items
        .iter()
        .filter_map(|item| {
            item.metadata
                .as_ref()
                .map(|metadata| (item.source.as_str(), (metadata, item.tz.as_deref())))
        })
        .collect();
    let mut processed_outputs = HashMap::<String, PathBuf>::new();

    let mut summary = RunSummary {
        other_files: count_other_files(items, &placements),
        dry_run: settings.dry_run,
        ..Default::default()
    };

    state.set("status", "running")?;
    state.mark_progress(0, placements.len() as u32, "planning")?;

    let start = Instant::now();
    for (idx, placement) in placements.iter().enumerate() {
        summary.warnings.extend(
            placement
                .tags
                .iter()
                .filter_map(|tag| tag.strip_prefix("LivePhotoWarning:"))
                .map(str::to_string),
        );
        if control
            .lock()
            .map_err(|error| format!("run control is unavailable: {error}"))?
            .is_cancelled()
        {
            summary.cancelled = true;
            break;
        }

        while control
            .lock()
            .map_err(|error| format!("run control is unavailable: {error}"))?
            .is_paused()
        {
            thread::sleep(Duration::from_millis(200));
        }

        if settings.dry_run {
            summary.planned += 1;
            continue;
        }

        let (metadata, timezone_name) = metadata_by_source
            .get(placement.src.as_str())
            .map(|(metadata, timezone_name)| (Some(*metadata), *timezone_name))
            .unwrap_or((None, None));
        match write_placement(
            settings,
            placement,
            metadata,
            timezone_name,
            &mut exiftool,
            &mut processed_outputs,
        ) {
            Ok(result) => {
                if result.skipped {
                    summary.skipped += 1;
                } else {
                    summary.processed += 1;
                }
                summary.warnings.extend(result.warnings);
                summary.problems.extend(result.problems);
            }
            Err(err) => {
                summary.problems.push(err);
            }
        }

        let completed = idx as u32 + 1;
        state.mark_progress(completed, placements.len() as u32, "writing")?;

        if let Ok(free_bytes) = disk_free_bytes(
            Path::new(&placement.dest)
                .parent()
                .unwrap_or(Path::new(".")),
        ) {
            if free_bytes < 1_000_000_000 {
                control
                    .lock()
                    .map_err(|error| format!("run control is unavailable: {error}"))?
                    .pause();
                summary.paused = true;
                summary
                    .warnings
                    .push("Low disk space: paused. Remove space and resume.".to_string());
                state.set("status", "paused")?;
                break;
            }
        }

        if start.elapsed().as_millis() > 0 && idx as u32 % 2 == 0 {
            let _ = state.set(
                "last-progress",
                &format!("{} of {}", idx + 1, placements.len()),
            );
        }
    }

    if summary.cancelled {
        state.set("status", "cancelled")?;
    } else if summary.paused {
        state.set("status", "paused")?;
    } else {
        state.set("status", "complete")?;
    }

    summary.warnings.sort();
    summary.warnings.dedup();
    state.mark_finished(&summary)?;
    Ok(summary)
}

pub fn start_indexed_run(
    app: &tauri::AppHandle,
    settings: &PlannerSettings,
    items: &[MediaItem],
    placements: &[Placement],
    input_locators: HashMap<String, InputLocator>,
    archives_to_delete: HashSet<String>,
    exiftool_path: &Path,
) -> Result<RunSummary, String> {
    start_indexed_run_with_progress(
        settings,
        items,
        placements,
        input_locators,
        archives_to_delete,
        exiftool_path,
        |progress| {
            crate::events::emit_processing_progress(app, progress)
                .map_err(|error| error.to_string())
        },
    )
}

pub fn start_indexed_run_with_manifest(
    app: &tauri::AppHandle,
    settings: &PlannerSettings,
    items: &[MediaItem],
    placements: &[Placement],
    input_locators: HashMap<String, InputLocator>,
    archives_to_delete: HashSet<String>,
    exiftool_path: &Path,
    manifest: RunManifest,
    preparation_cancel: &AtomicBool,
) -> Result<RunSummary, String> {
    start_indexed_run_inner(
        settings,
        items,
        placements,
        input_locators,
        archives_to_delete,
        exiftool_path,
        Some(manifest),
        Some(preparation_cancel),
        &send_archive_to_trash,
        |progress| {
            crate::events::emit_processing_progress(app, progress)
                .map_err(|error| error.to_string())
        },
    )
}

pub fn start_indexed_run_with_progress(
    settings: &PlannerSettings,
    items: &[MediaItem],
    placements: &[Placement],
    input_locators: HashMap<String, InputLocator>,
    archives_to_delete: HashSet<String>,
    exiftool_path: &Path,
    emit_progress: impl FnMut(&crate::events::ProcessingProgressEvent) -> Result<(), String>,
) -> Result<RunSummary, String> {
    start_indexed_run_inner(
        settings,
        items,
        placements,
        input_locators,
        archives_to_delete,
        exiftool_path,
        None,
        None,
        &send_archive_to_trash,
        emit_progress,
    )
}

#[cfg(test)]
pub(crate) fn start_indexed_run_with_progress_and_deleter(
    settings: &PlannerSettings,
    items: &[MediaItem],
    placements: &[Placement],
    input_locators: HashMap<String, InputLocator>,
    archives_to_delete: HashSet<String>,
    exiftool_path: &Path,
    send_to_trash: impl Fn(&Path) -> Result<(), String>,
    emit_progress: impl FnMut(&crate::events::ProcessingProgressEvent) -> Result<(), String>,
) -> Result<RunSummary, String> {
    start_indexed_run_inner(
        settings,
        items,
        placements,
        input_locators,
        archives_to_delete,
        exiftool_path,
        None,
        None,
        &send_to_trash,
        emit_progress,
    )
}

fn start_indexed_run_inner(
    settings: &PlannerSettings,
    items: &[MediaItem],
    placements: &[Placement],
    input_locators: HashMap<String, InputLocator>,
    archives_to_delete: HashSet<String>,
    exiftool_path: &Path,
    mut manifest: Option<RunManifest>,
    preparation_cancel: Option<&AtomicBool>,
    send_to_trash: &dyn Fn(&Path) -> Result<(), String>,
    mut emit_progress: impl FnMut(&crate::events::ProcessingProgressEvent) -> Result<(), String>,
) -> Result<RunSummary, String> {
    let control = run_control();
    let initial_control = control
        .lock()
        .map_err(|error| format!("run control is unavailable: {error}"))?;
    initial_control.reset();
    if preparation_cancel.is_some_and(|cancelled| cancelled.load(Ordering::Acquire)) {
        initial_control.cancel();
    }
    drop(initial_control);

    let mut tasks_by_source = HashMap::<String, usize>::new();
    let mut tasks = Vec::<ProcessingTask>::new();
    let mut cached_placements = HashSet::new();
    let mut cached_completed = 0_u32;
    let mut completed_bytes = 0_u64;
    let items_by_source = items
        .iter()
        .map(|item| (item.source.as_str(), item))
        .collect::<HashMap<_, _>>();
    for (placement_index, placement) in placements.iter().enumerate() {
        let Some(item) = items_by_source.get(placement.src.as_str()) else {
            return Err(format!(
                "The planner returned an unknown source: {}",
                placement.src
            ));
        };
        let manifest_identity = manifest
            .as_ref()
            .filter(|_| !settings.dry_run)
            .map(|_| placement_manifest_identity(item, placement, settings))
            .transpose()?;
        if let (Some(run_manifest), Some((source_key, placement_key))) =
            (manifest.as_ref(), manifest_identity.as_ref())
        {
            if run_manifest.is_done_at_destination(
                source_key,
                placement_key,
                Path::new(&placement.dest),
            )? {
                cached_placements.insert(placement_index);
                cached_completed = cached_completed.saturating_add(1);
                completed_bytes = completed_bytes.saturating_add(item.size_bytes);
                continue;
            }
        }
        if let (Some(run_manifest), Some((source_key, placement_key))) =
            (manifest.as_mut(), manifest_identity.as_ref())
        {
            run_manifest.mark_pending(source_key, placement_key, &placement.dest)?;
        }
        let task_index = if let Some(task_index) = tasks_by_source.get(&placement.src) {
            *task_index
        } else {
            let locator = input_locators
                .get(&placement.src)
                .ok_or_else(|| format!("No archive source was indexed for {}", placement.src))?;
            let task_index = tasks.len();
            tasks.push(ProcessingTask {
                item: (*item).clone(),
                archive_path: locator.archive_path.clone(),
                placements: Vec::new(),
            });
            tasks_by_source.insert(placement.src.clone(), task_index);
            task_index
        };
        tasks[task_index].placements.push(placement.clone());
    }
    tasks.sort_by(|left, right| left.archive_path.cmp(&right.archive_path));
    let mut archive_remaining = HashMap::<String, usize>::new();
    let mut archive_success = HashMap::<String, bool>::new();
    for (placement_index, placement) in placements.iter().enumerate() {
        let archive_path = input_locators
            .get(&placement.src)
            .ok_or_else(|| format!("No archive source was indexed for {}", placement.src))?
            .archive_path
            .clone();
        if archives_to_delete.contains(&archive_path) {
            let remaining = archive_remaining.entry(archive_path.clone()).or_default();
            *remaining += 1;
            archive_success.entry(archive_path).or_insert(true);
            if cached_placements.contains(&placement_index) {
                *remaining = remaining.saturating_sub(1);
            }
        }
    }

    let mut summary = RunSummary {
        planned: placements.len() as u32,
        skipped: cached_completed,
        dry_run: settings.dry_run,
        files_without_json: items
            .iter()
            .filter(|item| (item.is_photo || item.is_video) && !item.has_json)
            .count() as u32,
        other_files: count_other_files(items, placements),
        live_photos_merged: if settings.merge_live
            && settings.live_style != LivePhotoStyle::Separate
        {
            items
                .iter()
                .filter(|item| item.live_pair_partner.is_some() && !item.live_pair_video)
                .count() as u32
        } else {
            0
        },
        ..Default::default()
    };
    let state = RunStateStore::new_temp()?;
    state.clear_summary()?;
    state.set("status", "running")?;
    state.mark_progress(cached_completed, summary.planned, "processing")?;

    let total_bytes = placements.iter().try_fold(0_u64, |total, placement| {
        let item = items_by_source
            .get(placement.src.as_str())
            .ok_or_else(|| format!("missing size for {}", placement.src))?;
        total
            .checked_add(item.size_bytes)
            .ok_or_else(|| "total processing size is too large to calculate".to_string())
    })?;
    let mut current_file = None;
    let mut completed = cached_completed;
    let mut last_emit = Instant::now() - Duration::from_secs(1);
    emit_indexed_progress(
        &mut emit_progress,
        &summary,
        current_file.as_deref(),
        completed,
        summary.planned,
        completed_bytes,
        total_bytes,
        Duration::ZERO,
    )?;

    if settings.dry_run || placements.is_empty() {
        state.mark_progress(summary.planned, summary.planned, "complete")?;
        state.mark_finished(&summary)?;
        return Ok(summary);
    }

    let needs_exiftool = tasks.iter().any(|task| {
        requires_exiftool_metadata(&task.item)
            || task
                .placements
                .iter()
                .any(|placement| placement.live_action.is_some())
    });
    let cpu_count = thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1);
    let worker_count =
        if settings.delete_archives_after || settings.speed == crate::planner::SpeedMode::Gentle {
            1
        } else {
            cpu_count.min(2).max(1)
        };
    let shared_locators = Arc::new(input_locators);
    let (task_sender, task_receiver) = mpsc::sync_channel::<ProcessingTask>(worker_count * 2);
    let task_receiver = Arc::new(Mutex::new(task_receiver));
    let (result_sender, result_receiver) = mpsc::channel::<PlacementCompletion>();
    let mut workers = Vec::with_capacity(worker_count);
    for _ in 0..worker_count {
        let receiver = task_receiver.clone();
        let sender = result_sender.clone();
        let worker_settings = settings.clone();
        let locators = shared_locators.clone();
        let executable = exiftool_path.to_path_buf();
        workers.push(thread::spawn(move || {
            process_worker(
                receiver,
                sender,
                worker_settings,
                locators,
                &executable,
                needs_exiftool,
            )
        }));
    }
    drop(result_sender);
    let started = Instant::now();
    let task_feeder = thread::spawn(move || {
        for task in tasks {
            task_sender
                .send(task)
                .map_err(|error| format!("queue processing work: {error}"))?;
        }
        Ok::<(), String>(())
    });
    let mut event_error = None;
    let mut manifest_error = None;
    let mut run_error = None;
    let output_root = settings
        .output_root
        .as_deref()
        .map(PathBuf::from)
        .or_else(|| {
            placements
                .first()
                .and_then(|placement| Path::new(&placement.dest).parent())
                .map(Path::to_path_buf)
        });
    let mut last_space_check = Instant::now() - Duration::from_secs(10);
    loop {
        let completion = match result_receiver.recv_timeout(Duration::from_millis(250)) {
            Ok(completion) => Some(completion),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        if let Some(completion) = completion {
            completed = completed.saturating_add(1);
            current_file = Some(completion.item.filename.clone());
            completed_bytes = completed_bytes.saturating_add(completion.item.size_bytes);
            let archive_path = completion.archive_path.clone();
            let output_succeeded = matches!(&completion.result, Ok(result) if !result.skipped);
            let manifest_identity = if manifest.is_some() {
                match placement_manifest_identity(&completion.item, &completion.placement, settings)
                {
                    Ok(identity) => Some(identity),
                    Err(error) => {
                        manifest_error.get_or_insert(error);
                        None
                    }
                }
            } else {
                None
            };

            if !completion.item.has_json && (completion.item.is_photo || completion.item.is_video) {
                add_attention(
                    &mut summary,
                    &completion.item,
                    Some(&completion.placement.dest),
                    "No JSON sidecar",
                    "no-json",
                );
            }
            if let Some(warning) = completion.item.live_warning.as_ref() {
                push_bounded(&mut summary.warnings, warning.clone());
            }

            match completion.result {
                Ok(result) => {
                    if result.skipped {
                        summary.skipped = summary.skipped.saturating_add(1);
                        if result
                            .warnings
                            .iter()
                            .any(|warning| warning.contains("already exists"))
                        {
                            summary.collisions = summary.collisions.saturating_add(1);
                        }
                        add_attention(
                            &mut summary,
                            &completion.item,
                            Some(&completion.placement.dest),
                            "Output was skipped",
                            "skipped",
                        );
                    } else {
                        summary.processed = summary.processed.saturating_add(1);
                        if let (Some(run_manifest), Some((source_key, placement_key))) =
                            (manifest.as_mut(), manifest_identity.as_ref())
                        {
                            if let Err(error) = run_manifest.mark_done(
                                source_key,
                                placement_key,
                                Path::new(&completion.placement.dest),
                            ) {
                                manifest_error.get_or_insert(error);
                            }
                        }
                        let metadata_failed = result.warnings.iter().any(|warning| {
                            warning.starts_with("Could not write metadata")
                                || is_metadata_error_warning(warning)
                        });
                        if completion
                            .item
                            .metadata
                            .as_ref()
                            .and_then(TakeoutMetadata::taken_timestamp)
                            .is_some()
                        {
                            summary.dates_restored = summary.dates_restored.saturating_add(1);
                        }
                        if !metadata_failed
                            && settings.write_gps
                            && crate::metadata::supports_embedded_metadata(Path::new(
                                &completion.item.filename,
                            ))
                            && completion.item.metadata.as_ref().is_some_and(|metadata| {
                                metadata.latitude.is_some() && metadata.longitude.is_some()
                            })
                        {
                            summary.locations_added = summary.locations_added.saturating_add(1);
                        }
                    }
                    for warning in result.warnings {
                        push_bounded(&mut summary.warnings, warning.clone());
                        add_attention(
                            &mut summary,
                            &completion.item,
                            Some(&completion.placement.dest),
                            &warning,
                            "warnings",
                        );
                    }
                }
                Err(error) => {
                    let is_cancelled = match control.lock() {
                        Ok(control) => control.is_cancelled(),
                        Err(lock_error) => {
                            run_error
                                .get_or_insert(format!("run control is unavailable: {lock_error}"));
                            false
                        }
                    };
                    if is_cancelled {
                        summary.cancelled = true;
                    } else {
                        summary.failed = summary.failed.saturating_add(1);
                        push_bounded(&mut summary.problems, error.clone());
                        add_attention(
                            &mut summary,
                            &completion.item,
                            Some(&completion.placement.dest),
                            &error,
                            "failed",
                        );
                    }
                }
            }

            if let Some(remaining) = archive_remaining.get_mut(&archive_path) {
                if !output_succeeded {
                    archive_success.insert(archive_path.clone(), false);
                }
                *remaining = remaining.saturating_sub(1);
            }

            if last_emit.elapsed() >= Duration::from_millis(500) || completed == summary.planned {
                if let Err(error) = emit_indexed_progress(
                    &mut emit_progress,
                    &summary,
                    current_file.as_deref(),
                    completed,
                    summary.planned,
                    completed_bytes,
                    total_bytes,
                    started.elapsed(),
                ) {
                    event_error.get_or_insert(error);
                } else {
                    last_emit = Instant::now();
                }
                if let Err(error) = state.mark_progress(completed, summary.planned, "processing") {
                    run_error.get_or_insert(error);
                    if let Ok(control) = control.lock() {
                        control.cancel();
                    }
                }
            }
        }

        if last_space_check.elapsed() >= Duration::from_secs(10) && run_error.is_none() {
            last_space_check = Instant::now();
            if let Some(path) = output_root.as_deref() {
                match crate::platform::disk_space_bytes(path) {
                    Ok((free_bytes, total_bytes)) => {
                        let threshold = minimum_free_space(total_bytes);
                        match control.lock() {
                            Ok(control) if free_bytes < threshold && !control.is_paused() => {
                                control.pause_for_space();
                                push_bounded(
                                    &mut summary.warnings,
                                    format!(
                                        "Processing paused because free space fell below {:.2} GB.",
                                        threshold as f64 / 1_000_000_000.0
                                    ),
                                );
                                if let Err(error) =
                                    state.mark_progress(completed, summary.planned, "paused")
                                {
                                    run_error.get_or_insert(error);
                                    control.cancel();
                                }
                            }
                            Ok(control) if free_bytes >= threshold && control.is_space_paused() => {
                                control.resume();
                                push_bounded(
                                    &mut summary.warnings,
                                    "Processing resumed after free space recovered.".to_string(),
                                );
                                if let Err(error) =
                                    state.mark_progress(completed, summary.planned, "processing")
                                {
                                    run_error.get_or_insert(error);
                                    control.cancel();
                                }
                            }
                            Ok(_) => {}
                            Err(error) => {
                                run_error
                                    .get_or_insert(format!("run control is unavailable: {error}"));
                            }
                        }
                    }
                    Err(error) => {
                        run_error.get_or_insert(error);
                        if let Ok(control) = control.lock() {
                            control.cancel();
                        }
                    }
                }
            }
        }
    }
    let feeder_result = match task_feeder.join() {
        Ok(result) => result,
        Err(_) => Err("the processing task feeder stopped unexpectedly".to_string()),
    };
    let mut worker_error = None;
    let mut worker_panicked = false;
    for worker in workers {
        match worker.join() {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                worker_error.get_or_insert(error);
            }
            Err(_) => worker_panicked = true,
        }
    }
    if let Some(error) = worker_error {
        return Err(error);
    }
    if worker_panicked {
        return Err("a processing worker stopped unexpectedly".to_string());
    }
    feeder_result?;

    if let Some(error) = run_error {
        return Err(error);
    }
    if let Some(error) = manifest_error {
        return Err(error);
    }

    if control
        .lock()
        .map_err(|error| format!("run control is unavailable: {error}"))?
        .is_paused()
    {
        summary.paused = true;
    }
    let can_delete_archives = settings.delete_archives_after
        && summary.failed == 0
        && !summary.cancelled
        && !summary.paused
        && event_error.is_none();
    if can_delete_archives {
        let archives_to_trash = archive_remaining
            .iter()
            .filter(|(archive, remaining)| {
                **remaining == 0 && archive_success.get(*archive) == Some(&true)
            })
            .map(|(archive, _)| archive.clone())
            .collect::<Vec<_>>();
        for archive_path in &archives_to_trash {
            match delete_archives_after_with(settings, std::slice::from_ref(archive_path), |path| {
                send_to_trash(path)
            }) {
                Ok(deleted) => {
                    summary.archives_deleted = summary
                        .archives_deleted
                        .saturating_add(deleted.len() as u32);
                }
                Err(error) => push_bounded(&mut summary.warnings, error),
            }
        }
    }
    if archives_to_delete.iter().any(|archive| {
        archive_remaining.get(archive).copied().unwrap_or(0) > 0
            || archive_success.get(archive) == Some(&false)
    }) {
        push_bounded(
            &mut summary.warnings,
            "One or more input archives were kept because not every output completed successfully."
                .to_string(),
        );
    }
    if settings.delete_archives_after && summary.failed > 0 && !archives_to_delete.is_empty() {
        push_bounded(
            &mut summary.warnings,
            "Input archives were kept because the run finished with failed outputs.".to_string(),
        );
    }
    if summary.cancelled {
        state.set("status", "cancelled")?;
    } else if summary.paused {
        state.set("status", "paused")?;
    } else {
        state.set("status", "complete")?;
    }
    summary.warnings.sort();
    summary.warnings.dedup();
    state.mark_finished(&summary)?;
    if let Some(error) = event_error {
        return Err(error);
    }
    emit_indexed_progress(
        &mut emit_progress,
        &summary,
        current_file.as_deref(),
        completed,
        summary.planned,
        completed_bytes,
        total_bytes,
        started.elapsed(),
    )?;
    Ok(summary)
}

#[derive(Clone)]
struct ProcessingTask {
    item: MediaItem,
    archive_path: String,
    placements: Vec<Placement>,
}

struct PlacementCompletion {
    item: MediaItem,
    archive_path: String,
    placement: Placement,
    result: Result<PlacementResult, String>,
    metadata_request: Option<MetadataWriteRequest>,
    deferred_temp: Option<tempfile::TempPath>,
}

const PAUSED_WRITE_MARKER: &str = "Takeback paused before output could be finalized.";

fn process_worker(
    receiver: Arc<Mutex<mpsc::Receiver<ProcessingTask>>>,
    sender: mpsc::Sender<PlacementCompletion>,
    settings: PlannerSettings,
    input_locators: Arc<HashMap<String, InputLocator>>,
    exiftool_path: &Path,
    needs_exiftool: bool,
) -> Result<(), String> {
    let mut exiftool = if needs_exiftool {
        Some(ExifTool::new(exiftool_path)?)
    } else {
        None
    };
    let mut zip_archives = HashMap::new();
    let mut pending_metadata = Vec::<PlacementCompletion>::new();
    let mut processed_outputs = HashMap::new();
    loop {
        let task = receiver
            .lock()
            .map_err(|error| format!("processing queue is unavailable: {error}"))?
            .recv();
        let Ok(task) = task else {
            flush_metadata_batch(
                &mut exiftool,
                &sender,
                &mut pending_metadata,
                &mut processed_outputs,
                &mut zip_archives,
                &input_locators,
            )?;
            return Ok(());
        };
        processed_outputs.clear();
        for (placement_index, placement) in task.placements.iter().cloned().enumerate() {
            if placement_index > 0 && !pending_metadata.is_empty() {
                flush_metadata_batch(
                    &mut exiftool,
                    &sender,
                    &mut pending_metadata,
                    &mut processed_outputs,
                    &mut zip_archives,
                    &input_locators,
                )?;
            }
            let mut deferred_metadata = None;
            let mut deferred_temp = None;
            let result = loop {
                if run_control()
                    .lock()
                    .map_err(|error| format!("run control is unavailable: {error}"))?
                    .is_cancelled()
                {
                    break Err("Processing was cancelled.".to_string());
                }
                let result = write_placement_with_sources(
                    &settings,
                    &placement,
                    task.item.metadata.as_ref(),
                    task.item.tz.as_deref(),
                    &mut exiftool,
                    &mut zip_archives,
                    &mut processed_outputs,
                    &input_locators,
                    task.item.embedded_motion_video,
                    true,
                    &mut deferred_metadata,
                    &mut deferred_temp,
                );
                match result {
                    Err(error) if error.contains(PAUSED_WRITE_MARKER) => {
                        deferred_metadata = None;
                        deferred_temp = None;
                        if let Err(error) = wait_for_run_resume() {
                            break Err(error);
                        }
                    }
                    result => break result,
                }
            };
            let metadata_request = if result.is_ok() {
                deferred_metadata
            } else {
                None
            };
            let deferred_temp = if metadata_request.is_some() {
                deferred_temp
            } else {
                None
            };
            let completion = PlacementCompletion {
                item: task.item.clone(),
                archive_path: task.archive_path.clone(),
                placement,
                result,
                metadata_request,
                deferred_temp,
            };
            if completion.metadata_request.is_some() {
                pending_metadata.push(completion);
                if pending_metadata.len() >= crate::metadata::EXIFTOOL_BATCH_SIZE {
                    flush_metadata_batch(
                        &mut exiftool,
                        &sender,
                        &mut pending_metadata,
                        &mut processed_outputs,
                        &mut zip_archives,
                        &input_locators,
                    )?;
                }
            } else {
                sender
                    .send(completion)
                    .map_err(|error| format!("return processing result: {error}"))?;
            }
        }
    }
}

fn flush_metadata_batch(
    exiftool: &mut Option<ExifTool>,
    sender: &mpsc::Sender<PlacementCompletion>,
    pending: &mut Vec<PlacementCompletion>,
    processed_outputs: &mut HashMap<String, PathBuf>,
    zip_archives: &mut WorkerZipCache,
    input_locators: &HashMap<String, InputLocator>,
) -> Result<(), String> {
    if pending.is_empty() {
        return Ok(());
    }
    if let Err(error) = wait_for_run_resume() {
        if error == "Processing was cancelled." {
            for mut completion in pending.drain(..) {
                completion.result = Err(error.clone());
                completion.metadata_request = None;
                completion.deferred_temp = None;
                sender.send(completion).map_err(|send_error| {
                    format!("return cancelled metadata result: {send_error}")
                })?;
            }
            return Ok(());
        }
        return Err(error);
    }

    let requests = pending
        .iter()
        .map(|completion| {
            completion
                .metadata_request
                .clone()
                .ok_or_else(|| "metadata batch is missing a write request".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let outcomes = match exiftool.as_mut() {
        Some(exiftool) => exiftool.write_batch(&requests).unwrap_or_else(|error| {
            vec![vec![format!("Error: could not write metadata batch: {error}")]; requests.len()]
        }),
        None => {
            vec![
                vec!["Error: ExifTool was not available for a metadata batch.".to_string(),];
                requests.len()
            ]
        }
    };

    for (mut completion, warnings) in pending.drain(..).zip(outcomes) {
        let metadata_errors = warnings
            .iter()
            .filter(|warning| is_metadata_error_warning(warning))
            .cloned()
            .collect::<Vec<_>>();
        if !metadata_errors.is_empty() {
            let temp = completion
                .deferred_temp
                .as_ref()
                .ok_or_else(|| "metadata batch is missing its temporary output".to_string())?;
            let temp_path: &Path = temp.as_ref();
            if let Err(error) = copy_source_into_temp(
                &completion.item.source,
                temp_path,
                input_locators,
                zip_archives,
            ) {
                completion.result = Err(format!(
                    "metadata write failed: {}; restore output failed: {error}",
                    metadata_errors.join("; ")
                ));
            } else if let Some(metadata) = completion.item.metadata.as_ref() {
                if let Err(error) = set_taken_file_time(temp_path, metadata) {
                    completion.result = Err(format!(
                        "metadata write failed: {}; restore output time failed: {error}",
                        metadata_errors.join("; ")
                    ));
                }
            }
            if let Ok(result) = &mut completion.result {
                result.warnings.extend(warnings);
                result.warnings.push(format!(
                    "Could not write metadata to {}; the original file was preserved: {}",
                    completion.placement.dest,
                    metadata_errors.join("; ")
                ));
            }
        } else if let Ok(result) = &mut completion.result {
            result.warnings.extend(warnings);
        }

        if completion.result.is_ok() {
            let control = run_control()
                .lock()
                .map_err(|error| format!("run control is unavailable: {error}"))?;
            if control.is_cancelled() {
                completion.result = Err("Processing was cancelled.".to_string());
            }
            let paused = control.is_paused();
            drop(control);
            if paused && completion.result.is_ok() {
                if let Err(error) = wait_for_run_resume() {
                    completion.result = Err(error);
                }
            }
        }

        if completion.result.is_ok() {
            let temp = completion
                .deferred_temp
                .take()
                .ok_or_else(|| "metadata batch is missing its temporary output".to_string())?;
            let destination = Path::new(&completion.placement.dest);
            let temp_display = temp.display().to_string();
            if let Err(error) = temp.persist_noclobber(destination) {
                completion.result = Err(format!(
                    "Could not safely finalize temporary output {} as {} without replacing an existing file: {}",
                    temp_display,
                    destination.display(),
                    error.error
                ));
            } else {
                processed_outputs
                    .insert(completion.placement.src.clone(), destination.to_path_buf());
            }
        }
        completion.metadata_request = None;
        completion.deferred_temp = None;
        sender
            .send(completion)
            .map_err(|error| format!("return metadata batch results: {error}"))?;
    }
    Ok(())
}

fn is_metadata_error_warning(warning: &str) -> bool {
    let lowercase = warning.to_ascii_lowercase();
    warning.trim_start().starts_with("Error:")
        || (lowercase.contains("files weren") && lowercase.contains("updated"))
}

fn wait_for_run_resume() -> Result<(), String> {
    loop {
        let control = run_control()
            .lock()
            .map_err(|error| format!("run control is unavailable: {error}"))?;
        if control.is_cancelled() {
            return Err("Processing was cancelled.".to_string());
        }
        if !control.is_paused() {
            return Ok(());
        }
        drop(control);
        thread::sleep(Duration::from_millis(100));
    }
}

fn emit_indexed_progress(
    emit_progress: &mut impl FnMut(&crate::events::ProcessingProgressEvent) -> Result<(), String>,
    summary: &RunSummary,
    current_file: Option<&str>,
    completed_files: u32,
    total_files: u32,
    processed_bytes: u64,
    total_bytes: u64,
    elapsed: Duration,
) -> Result<(), String> {
    let elapsed_seconds = elapsed.as_secs_f64().max(0.001);
    let bytes_per_second = processed_bytes as f64 / elapsed_seconds;
    let eta_seconds = if bytes_per_second > 0.0 {
        Some(
            ((total_bytes.saturating_sub(processed_bytes)) as f64 / bytes_per_second).ceil() as u64,
        )
    } else {
        None
    };
    emit_progress(&crate::events::ProcessingProgressEvent {
        stage: None,
        items_done: None,
        items_total: None,
        current_file: current_file.map(str::to_string),
        completed_files: completed_files.into(),
        total_files: total_files.into(),
        processed_bytes,
        total_bytes,
        bytes_per_second,
        eta_seconds,
        warnings_count: summary.warnings.len() as u64,
        warnings: summary.warnings.iter().rev().take(20).cloned().collect(),
    })
}

fn add_attention(
    summary: &mut RunSummary,
    item: &MediaItem,
    destination: Option<&str>,
    reason: &str,
    group: &str,
) {
    const ATTENTION_LIMIT: usize = 10_000;
    if summary.attention.len() >= ATTENTION_LIMIT {
        return;
    }
    let id = format!("{}::{}", item.source, destination.unwrap_or(""));
    if summary.attention.iter().any(|record| record.id == id) {
        return;
    }
    summary.attention.push(ReportItem {
        id,
        file: item.filename.clone(),
        destination: destination.map(str::to_string),
        reason: reason.to_string(),
        group: group.to_string(),
        is_video: item.is_video,
    });
}

fn push_bounded(values: &mut Vec<String>, value: String) {
    const DETAIL_LIMIT: usize = 10_000;
    if values.len() < DETAIL_LIMIT && !values.contains(&value) {
        values.push(value);
    }
}

fn count_other_files(items: &[MediaItem], placements: &[Placement]) -> u32 {
    let placed_sources = placements
        .iter()
        .map(|placement| placement.src.as_str())
        .collect::<HashSet<_>>();
    items
        .iter()
        .filter(|item| {
            !item.is_photo && !item.is_video && placed_sources.contains(item.source.as_str())
        })
        .count() as u32
}

fn requires_exiftool_metadata(item: &MediaItem) -> bool {
    item.metadata.is_some()
        && supports_embedded_metadata(Path::new(&item.filename))
        && !is_large_video(Path::new(&item.filename), item.size_bytes)
}

fn placement_manifest_identity(
    item: &MediaItem,
    placement: &Placement,
    settings: &PlannerSettings,
) -> Result<(String, String), String> {
    let output_root = settings
        .output_root
        .as_deref()
        .map(Path::new)
        .ok_or_else(|| "The output root is unavailable for the run manifest.".to_string())?;
    let destination_directory = Path::new(&placement.dest)
        .parent()
        .ok_or_else(|| format!("output path has no parent: {}", placement.dest))?;
    let kind = match placement.kind {
        crate::planner::PlacementKind::Original => "original",
        crate::planner::PlacementKind::Link => "link",
        crate::planner::PlacementKind::Copy => "copy",
    };
    Ok((
        manifest::source_key(&item.source, item.size_bytes),
        manifest::placement_key(kind, &placement.tags, destination_directory, output_root),
    ))
}

fn minimum_free_space(total_bytes: u64) -> u64 {
    1_000_000_000_u64.max(total_bytes.saturating_mul(2) / 100)
}

#[derive(Debug, Default)]
pub struct PlacementResult {
    pub skipped: bool,
    pub warnings: Vec<String>,
    pub problems: Vec<String>,
}

fn write_placement(
    settings: &PlannerSettings,
    placement: &Placement,
    metadata: Option<&TakeoutMetadata>,
    timezone_name: Option<&str>,
    exiftool: &mut Option<ExifTool>,
    processed_outputs: &mut HashMap<String, PathBuf>,
) -> Result<PlacementResult, String> {
    let mut zip_archives = HashMap::new();
    write_placement_with_sources(
        settings,
        placement,
        metadata,
        timezone_name,
        exiftool,
        &mut zip_archives,
        processed_outputs,
        &HashMap::new(),
        false,
        false,
        &mut None,
        &mut None,
    )
}

fn write_placement_with_sources(
    settings: &PlannerSettings,
    placement: &Placement,
    metadata: Option<&TakeoutMetadata>,
    timezone_name: Option<&str>,
    exiftool: &mut Option<ExifTool>,
    zip_archives: &mut WorkerZipCache,
    processed_outputs: &mut HashMap<String, PathBuf>,
    input_locators: &HashMap<String, InputLocator>,
    embedded_motion_marker: bool,
    batch_metadata: bool,
    deferred_metadata: &mut Option<MetadataWriteRequest>,
    deferred_temp: &mut Option<tempfile::TempPath>,
) -> Result<PlacementResult, String> {
    let src = Path::new(&placement.src);
    let dest = Path::new(&placement.dest);
    let mut result = PlacementResult::default();

    let indexed_source = input_locators.contains_key(&placement.src);
    if !indexed_source && !src.exists() {
        return Err(format!("source missing: {}", src.display()));
    }

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("create directory {}: {e}", parent.display()))?;
    }

    if let Ok(destination_metadata) = fs::symlink_metadata(dest) {
        let same =
            !indexed_source && destination_metadata.file_type().is_file() && same_file(src, dest)?;
        if same {
            result.skipped = true;
            if metadata.is_some() {
                result.warnings.push(format!(
                    "Identical output already exists; metadata was not refreshed for {}.",
                    dest.display()
                ));
            }
            return Ok(result);
        }
        result.skipped = true;
        result.warnings.push(format!(
            "A different file already exists at {}; it was not overwritten.",
            dest.display()
        ));
        return Ok(result);
    }

    let tmp = temp_path_for(dest)?;
    let tmp_path: &Path = tmp.as_ref();
    let metadata_output = metadata.is_some();
    let linked_output = if metadata_output && placement.kind == crate::planner::PlacementKind::Link
    {
        processed_outputs.get(&placement.src)
    } else {
        None
    };

    if let Some(previous_output) = linked_output {
        fs::remove_file(tmp_path)
            .map_err(|error| format!("prepare temporary link {}: {error}", tmp_path.display()))?;
        if let Err(error) = fs::hard_link(previous_output, tmp_path) {
            fs::copy(previous_output, tmp_path).map_err(|copy_error| {
                format!(
                    "link processed output {} -> {} failed: {error}; fallback copy failed: {copy_error}",
                    previous_output.display(),
                    tmp_path.display()
                )
            })?;
            result.warnings.push(format!(
                "Could not hard-link processed output {}; copied it instead.",
                previous_output.display()
            ));
        }
    } else if placement.kind == crate::planner::PlacementKind::Link
        && !metadata_output
        && !indexed_source
    {
        fs::remove_file(tmp_path)
            .map_err(|error| format!("prepare temporary link {}: {error}", tmp_path.display()))?;
        if crate::platform::hard_link_or_copy(src, tmp_path)? == crate::platform::LinkMethod::Copy {
            result.warnings.push(format!(
                "A hard link could not be created for {}; copied it instead.",
                dest.display()
            ));
        }
    } else {
        copy_source_into_temp(&placement.src, tmp_path, input_locators, zip_archives)?;
    }

    if embedded_motion_marker {
        let mut file = fs::File::open(tmp_path).map_err(|error| {
            format!("open possible Motion Photo {}: {error}", tmp_path.display())
        })?;
        if !crate::takeout::matcher::verify_android_motion_photo(&mut file)? {
            result.warnings.push(format!(
                "Motion Photo markers were found in {}, but no embedded MP4 was verified.",
                placement.src
            ));
        }
    }

    if metadata_output && linked_output.is_none() {
        let metadata = metadata.ok_or_else(|| "missing Takeout metadata".to_string())?;
        let temp_size = fs::metadata(tmp_path)
            .map_err(|error| format!("inspect temporary output {}: {error}", tmp_path.display()))?
            .len();
        if is_large_video(dest, temp_size) {
            set_taken_file_time(tmp_path, metadata)?;
            result.warnings.push(
                "ExifTool metadata rewrite was skipped for a video over 500 MB; only the file modified time was set when available."
                    .to_string(),
            );
        } else if supports_embedded_metadata(dest) {
            if batch_metadata {
                *deferred_metadata = Some(MetadataWriteRequest {
                    path: tmp_path.to_path_buf(),
                    metadata: metadata.clone(),
                    time_zone: settings.time_zone,
                    timezone_name: timezone_name.map(str::to_string),
                    write_gps: settings.write_gps,
                    write_descriptions: settings.write_descriptions,
                    rate_favorites: settings.rate_favorites,
                });
            } else {
                let writer = exiftool.as_mut().ok_or_else(|| {
                    "bundled ExifTool is not available for metadata writes".to_string()
                })?;
                match writer.write(
                    tmp_path,
                    metadata,
                    settings.time_zone,
                    timezone_name,
                    settings.write_gps,
                    settings.write_descriptions,
                    settings.rate_favorites,
                ) {
                    Ok(warnings) => result.warnings.extend(warnings),
                    Err(error) => {
                        copy_source_into_temp(&placement.src, tmp_path, input_locators, zip_archives).map_err(|restore_error| {
                            format!("metadata write failed: {error}; restore output failed: {restore_error}")
                        })?;
                        set_taken_file_time(tmp_path, metadata)?;
                        result.warnings.push(format!(
                            "Could not write metadata to {}; the original file was preserved: {error}",
                            dest.display()
                        ));
                    }
                }
            }
        } else {
            set_taken_file_time(tmp_path, metadata)?;
            result.warnings.push(format!(
                "Metadata is not safely supported for {}; the file was copied without metadata.",
                dest.display()
            ));
        }
    }

    if let Some(action) = &placement.live_action {
        let writer = exiftool
            .as_mut()
            .ok_or_else(|| "bundled ExifTool is not available for live photo writes".to_string())?;
        match action {
            LiveAction::IphonePair { content_identifier } => apply_live_style(
                writer,
                LivePhotoStyle::IphonePair,
                tmp_path,
                None,
                content_identifier,
            )?,
            LiveAction::AndroidMotionPhoto {
                content_identifier,
                video_source,
            } => {
                let video_temp = if input_locators.contains_key(video_source) {
                    let temp = temp_path_for(dest)?;
                    let video_temp_path: &Path = temp.as_ref();
                    copy_source_into_temp(
                        video_source,
                        video_temp_path,
                        input_locators,
                        zip_archives,
                    )?;
                    let duration_is_unknown = input_locators
                        .get(video_source)
                        .is_some_and(|locator| locator.video_duration_seconds.is_none());
                    if duration_is_unknown {
                        let metadata = fs::metadata(video_temp_path).map_err(|error| {
                            format!(
                                "read extracted video metadata {}: {error}",
                                video_temp_path.display()
                            )
                        })?;
                        let mut file = fs::File::open(video_temp_path).map_err(|error| {
                            format!(
                                "open extracted video {}: {error}",
                                video_temp_path.display()
                            )
                        })?;
                        if crate::takeout::matcher::read_video_duration_from_seekable(
                            &mut file,
                            metadata.len(),
                        )?
                        .is_none()
                        {
                            result.warnings.push(format!(
                                "Could not determine the duration of {}; the live pair was kept by matching name and folder.",
                                video_source
                            ));
                        }
                    }
                    Some(temp)
                } else {
                    None
                };
                let video_path = video_temp
                    .as_ref()
                    .map(|temp| temp.as_ref())
                    .unwrap_or_else(|| Path::new(video_source));
                apply_live_style(
                    writer,
                    LivePhotoStyle::AndroidMotionPhoto,
                    tmp_path,
                    Some(video_path),
                    content_identifier,
                )?;
            }
        }
    }
    if let Some(metadata) = metadata {
        set_taken_file_time(tmp_path, metadata)?;
    }

    let control = run_control()
        .lock()
        .map_err(|error| format!("run control is unavailable: {error}"))?;
    if control.is_cancelled() {
        return Err("Processing was cancelled.".to_string());
    }
    if control.is_paused() {
        return Err(PAUSED_WRITE_MARKER.to_string());
    }
    drop(control);

    if deferred_metadata.is_some() {
        *deferred_temp = Some(tmp);
    } else {
        let temp_display = tmp_path.display().to_string();
        tmp.persist_noclobber(dest).map_err(|error| {
            format!(
                "Could not safely finalize temporary output {} as {} without replacing an existing file: {}",
                temp_display,
                dest.display(),
                error.error
            )
        })?;

        if metadata_output {
            processed_outputs.insert(placement.src.clone(), dest.to_path_buf());
        }
    }

    Ok(result)
}

type WorkerZipCache = HashMap<String, ZipArchive<fs::File>>;

fn copy_source_into_temp(
    source_key: &str,
    destination: &Path,
    input_locators: &HashMap<String, InputLocator>,
    zip_archives: &mut WorkerZipCache,
) -> Result<(), String> {
    let output = fs::File::create(destination)
        .map_err(|error| format!("create temporary output {}: {error}", destination.display()))?;
    let mut output = CancelWriter { file: output };
    if let Some(locator) = input_locators.get(source_key) {
        if !Path::new(&locator.archive_path).is_dir()
            && Path::new(&locator.archive_path)
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
        {
            if !zip_archives.contains_key(&locator.archive_path) {
                let file = fs::File::open(&locator.archive_path)
                    .map_err(|error| format!("open zip {}: {error}", locator.archive_path))?;
                let archive = ZipArchive::new(file)
                    .map_err(|error| format!("read zip {}: {error}", locator.archive_path))?;
                zip_archives.insert(locator.archive_path.clone(), archive);
            }
            let archive = zip_archives
                .get_mut(&locator.archive_path)
                .ok_or_else(|| format!("zip cache unavailable: {}", locator.archive_path))?;
            let mut entry = archive
                .by_name(&locator.entry_path)
                .map_err(|_| format!("zip entry not found: {}", locator.entry_path))?;
            io::copy(&mut entry, &mut output)
                .map_err(|error| format!("stream zip entry {}: {error}", locator.entry_path))?;
        } else {
            locator
                .source
                .copy_entry_to(&locator.entry_path, &mut output)?;
        }
    } else {
        let mut source = fs::File::open(source_key)
            .map_err(|error| format!("open source file {source_key}: {error}"))?;
        io::copy(&mut source, &mut output)
            .map_err(|error| format!("copy {source_key} -> {}: {error}", destination.display()))?;
    }
    output
        .flush()
        .map_err(|error| format!("flush temporary output {}: {error}", destination.display()))?;
    Ok(())
}

struct CancelWriter {
    file: fs::File,
}

impl Write for CancelWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let control = run_control()
            .lock()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "run control is unavailable"))?;
        if control.is_cancelled() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "processing was cancelled",
            ));
        }
        if control.is_paused() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                PAUSED_WRITE_MARKER,
            ));
        }
        self.file.write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

fn same_file(left: &Path, right: &Path) -> Result<bool, String> {
    let left_meta =
        fs::metadata(left).map_err(|e| format!("read source metadata {}: {e}", left.display()))?;
    let right_meta = fs::metadata(right)
        .map_err(|e| format!("read target metadata {}: {e}", right.display()))?;
    if left_meta.len() != right_meta.len() {
        return Ok(false);
    }

    Ok(hash_file(left)? == hash_file(right)?)
}

fn hash_file(path: &Path) -> Result<blake3::Hash, String> {
    let mut file = fs::File::open(path)
        .map_err(|error| format!("open file for comparison {}: {error}", path.display()))?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| format!("read file for comparison {}: {error}", path.display()))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hasher.finalize())
}

fn temp_path_for(dest: &Path) -> Result<tempfile::TempPath, String> {
    let parent = dest
        .parent()
        .ok_or_else(|| format!("output path has no parent: {}", dest.display()))?;
    let suffix = dest
        .extension()
        .map(|extension| format!(".{}", extension.to_string_lossy()))
        .unwrap_or_default();
    let file = tempfile::Builder::new()
        .prefix(".takeback-")
        .suffix(&suffix)
        .tempfile_in(parent)
        .map_err(|error| format!("create temporary output in {}: {error}", parent.display()))?;
    Ok(file.into_temp_path())
}

fn set_taken_file_time(path: &Path, metadata: &TakeoutMetadata) -> Result<(), String> {
    let Some(timestamp) = metadata.taken_timestamp() else {
        return Ok(());
    };
    let datetime = chrono::DateTime::<chrono::Utc>::from_timestamp(timestamp, 0)
        .ok_or_else(|| format!("invalid Takeout timestamp: {timestamp}"))?;
    let file = fs::OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|error| format!("open output to set its modified time: {error}"))?;
    file.set_modified(std::time::SystemTime::from(datetime))
        .map_err(|error| format!("set output modified time: {error}"))
}

pub fn pause_run() -> Result<(), String> {
    run_control()
        .lock()
        .map_err(|error| format!("run control is unavailable: {error}"))?
        .pause();
    Ok(())
}

pub fn resume_run() -> Result<(), String> {
    run_control()
        .lock()
        .map_err(|error| format!("run control is unavailable: {error}"))?
        .resume();
    Ok(())
}

pub fn cancel_run() -> Result<(), String> {
    run_control()
        .lock()
        .map_err(|error| format!("run control is unavailable: {error}"))?
        .cancel();
    Ok(())
}

pub fn dry_run_preview(settings: &PlannerSettings, items: &[MediaItem]) -> Vec<String> {
    planner::plan(items, settings)
        .into_iter()
        .map(|p| p.dest)
        .collect()
}

pub fn delete_archives_after(
    settings: &PlannerSettings,
    paths: &[String],
) -> Result<Vec<String>, String> {
    delete_archives_after_with(settings, paths, send_archive_to_trash)
}

fn send_archive_to_trash(path: &Path) -> Result<(), String> {
    trash::delete(path).map_err(|error| {
        format!(
            "send archive to Recycle Bin/Trash {}: {error}",
            path.display()
        )
    })
}

fn delete_archives_after_with(
    settings: &PlannerSettings,
    paths: &[String],
    mut send_to_trash: impl FnMut(&Path) -> Result<(), String>,
) -> Result<Vec<String>, String> {
    if !settings.delete_archives_after {
        return Ok(Vec::new());
    }

    let mut deleted = Vec::new();
    for path in paths {
        let p = Path::new(path);
        let lower = p
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let is_tar_gz = p
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.to_ascii_lowercase().ends_with(".tar.gz"));
        if matches!(lower.as_str(), "zip" | "tgz") || is_tar_gz {
            if p.exists() {
                send_to_trash(p)?;
                deleted.push(path.clone());
            }
        }
    }
    Ok(deleted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::TakeoutMetadata;
    use crate::planner::{PlacementKind, TimeZoneMode};
    use tempfile::tempdir;

    #[test]
    fn deferred_metadata_keeps_outputs_temporary_until_batch_completion() {
        let directory = tempdir().expect("create output directory");
        let source = directory.path().join("source.jpg");
        let destination = directory.path().join("output.jpg");
        fs::write(&source, b"original photo").expect("write source");
        let temporary = tempfile::Builder::new()
            .prefix(".takeback-")
            .suffix(".jpg")
            .tempfile_in(directory.path())
            .expect("create temporary output");
        fs::write(temporary.path(), b"rewritten photo").expect("write temporary output");
        let temporary_path = temporary.into_temp_path();
        let source_key = source.to_string_lossy().to_string();
        let item = MediaItem {
            source: source_key.clone(),
            filename: "source.jpg".to_string(),
            album: None,
            additional_albums: Vec::new(),
            date: None,
            time: None,
            tz: None,
            size_bytes: 14,
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
            has_json: true,
            duplicate_of: None,
            metadata: Some(TakeoutMetadata::default()),
        };
        let placement = Placement {
            src: source_key.clone(),
            dest: destination.to_string_lossy().to_string(),
            kind: PlacementKind::Original,
            tags: Vec::new(),
            live_action: None,
        };
        let request = MetadataWriteRequest {
            path: temporary_path.to_path_buf(),
            metadata: TakeoutMetadata::default(),
            time_zone: TimeZoneMode::Utc,
            timezone_name: None,
            write_gps: false,
            write_descriptions: false,
            rate_favorites: false,
        };
        let mut pending = vec![PlacementCompletion {
            item,
            archive_path: String::new(),
            placement,
            result: Ok(PlacementResult::default()),
            metadata_request: Some(request),
            deferred_temp: Some(temporary_path),
        }];
        let (sender, receiver) = mpsc::channel();
        let mut exiftool = None;
        let mut processed_outputs = HashMap::new();
        let mut zip_archives = WorkerZipCache::new();
        run_control().lock().expect("lock run control").reset();

        flush_metadata_batch(
            &mut exiftool,
            &sender,
            &mut pending,
            &mut processed_outputs,
            &mut zip_archives,
            &HashMap::new(),
        )
        .expect("flush metadata batch");
        let completion = receiver.recv().expect("receive completed output");

        assert_eq!(
            fs::read(&destination).expect("read restored destination"),
            b"original photo"
        );
        assert!(completion
            .result
            .expect("metadata warning is non-fatal")
            .warnings
            .iter()
            .any(|warning| warning.contains("original file was preserved")));
        assert_eq!(processed_outputs.get(&source_key), Some(&destination));
    }

    #[test]
    fn metadata_write_keeps_unsupported_input_unchanged() {
        let directory = tempdir().expect("create output directory");
        let source = directory.path().join("source.bin");
        let destination = directory.path().join("output.bin");
        let original = b"synthetic unsupported media";
        fs::write(&source, original).expect("write source");
        let metadata = TakeoutMetadata {
            photo_taken_time: Some(1_700_000_000),
            ..Default::default()
        };
        let placement = Placement {
            src: source.to_string_lossy().to_string(),
            dest: destination.to_string_lossy().to_string(),
            kind: PlacementKind::Link,
            tags: vec!["Link".to_string()],
            live_action: None,
        };
        let settings = PlannerSettings {
            time_zone: TimeZoneMode::Utc,
            ..Default::default()
        };
        let mut exiftool = None;
        let mut processed_outputs = HashMap::new();

        let result = write_placement(
            &settings,
            &placement,
            Some(&metadata),
            None,
            &mut exiftool,
            &mut processed_outputs,
        )
        .expect("write output");

        assert_eq!(fs::read(&source).expect("read source"), original);
        assert_eq!(fs::read(&destination).expect("read destination"), original);
        assert!(result
            .warnings
            .iter()
            .any(|warning| warning.contains("not safely supported")));
        assert_eq!(
            fs::metadata(&destination)
                .expect("destination metadata")
                .modified()
                .expect("destination modified time")
                .duration_since(std::time::UNIX_EPOCH)
                .expect("valid modified time")
                .as_secs(),
            1_700_000_000
        );
        assert_eq!(
            fs::read_dir(directory.path())
                .expect("read output directory")
                .count(),
            2,
            "only source and output files should remain"
        );
    }

    #[test]
    fn existing_different_output_is_never_overwritten() {
        let directory = tempdir().expect("create output directory");
        let source = directory.path().join("source.jpg");
        let destination = directory.path().join("output.jpg");
        fs::write(&source, b"source contents").expect("write source");
        fs::write(&destination, b"existing contents").expect("write existing output");
        let placement = Placement {
            src: source.to_string_lossy().to_string(),
            dest: destination.to_string_lossy().to_string(),
            kind: PlacementKind::Original,
            tags: Vec::new(),
            live_action: None,
        };
        let mut exiftool = None;
        let mut processed_outputs = HashMap::new();

        let result = write_placement(
            &PlannerSettings::default(),
            &placement,
            None,
            None,
            &mut exiftool,
            &mut processed_outputs,
        )
        .expect("skip existing output");

        assert!(result.skipped);
        assert_eq!(
            fs::read(destination).expect("read existing output"),
            b"existing contents"
        );
    }

    #[test]
    fn zip_entries_reuse_a_worker_archive_cache() {
        let directory = tempdir().expect("create test directory");
        let archive_path = directory.path().join("takeout.zip");
        let archive_file = fs::File::create(&archive_path).expect("create zip");
        let mut archive = zip::ZipWriter::new(archive_file);
        archive
            .start_file("first.jpg", zip::write::FileOptions::default())
            .expect("start first zip entry");
        archive
            .write_all(b"first image")
            .expect("write first entry");
        archive
            .start_file("second.jpg", zip::write::FileOptions::default())
            .expect("start second zip entry");
        archive
            .write_all(b"second image")
            .expect("write second entry");
        archive.finish().expect("finish zip");

        let source: Arc<dyn Source> = Arc::new(crate::takeout::ZipSource {
            path: archive_path.clone(),
        });
        let mut input_locators = HashMap::new();
        for entry_path in ["first.jpg", "second.jpg"] {
            input_locators.insert(
                format!("takeout.zip::{entry_path}"),
                InputLocator {
                    source: source.clone(),
                    archive_path: archive_path.to_string_lossy().to_string(),
                    entry_path: entry_path.to_string(),
                    video_duration_seconds: None,
                },
            );
        }

        let mut zip_archives = WorkerZipCache::default();
        for entry_path in ["first.jpg", "second.jpg"] {
            let source_key = format!("takeout.zip::{entry_path}");
            let output_path = directory.path().join(format!("{entry_path}.out"));
            copy_source_into_temp(
                &source_key,
                &output_path,
                &input_locators,
                &mut zip_archives,
            )
            .expect("copy zip entry");
            assert_eq!(zip_archives.len(), 1);
        }

        assert_eq!(
            fs::read(directory.path().join("first.jpg.out")).expect("read first output"),
            b"first image"
        );
        assert_eq!(
            fs::read(directory.path().join("second.jpg.out")).expect("read second output"),
            b"second image"
        );
    }

    #[test]
    fn indexed_run_returns_a_worker_startup_error() {
        let directory = tempdir().expect("create test directory");
        let source_root = directory.path().join("takeout");
        fs::create_dir(&source_root).expect("create source folder");
        fs::write(source_root.join("photo.jpg"), b"synthetic photo").expect("write source");
        let source_key = "takeout::photo.jpg".to_string();
        let item = MediaItem {
            source: source_key.clone(),
            filename: "photo.jpg".to_string(),
            album: None,
            additional_albums: Vec::new(),
            date: None,
            time: None,
            tz: None,
            size_bytes: 15,
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
            has_json: true,
            duplicate_of: None,
            metadata: Some(TakeoutMetadata::default()),
        };
        let placement = Placement {
            src: source_key.clone(),
            dest: directory
                .path()
                .join("output.jpg")
                .to_string_lossy()
                .to_string(),
            kind: PlacementKind::Original,
            tags: Vec::new(),
            live_action: None,
        };
        let source: Arc<dyn Source> = crate::takeout::create_source(&source_root)
            .expect("create folder source")
            .into();
        let mut input_locators = HashMap::new();
        input_locators.insert(
            source_key,
            InputLocator {
                source,
                archive_path: source_root.to_string_lossy().to_string(),
                entry_path: "photo.jpg".to_string(),
                video_duration_seconds: None,
            },
        );
        let missing_exiftool = directory.path().join("missing-exiftool.exe");
        let expected_error = match ExifTool::new(&missing_exiftool) {
            Ok(_) => panic!("missing ExifTool unexpectedly started"),
            Err(error) => error,
        };

        let result = start_indexed_run_with_progress(
            &PlannerSettings::default(),
            &[item],
            &[placement],
            input_locators,
            HashSet::new(),
            &missing_exiftool,
            |_| Ok(()),
        );

        assert_eq!(
            result.expect_err("worker startup should fail"),
            expected_error
        );
    }

    #[test]
    fn indexed_run_skips_a_manifest_output_with_matching_size() {
        let directory = tempdir().expect("create test directory");
        let source_root = directory.path().join("takeout");
        let output_root = directory.path().join("library");
        fs::create_dir(&source_root).expect("create source folder");
        fs::create_dir(&output_root).expect("create output folder");
        fs::write(source_root.join("photo.jpg"), b"photo").expect("write source");
        let destination = output_root.join("photo.jpg");
        fs::write(&destination, b"saved").expect("write completed output");
        let source_key = "takeout::photo.jpg".to_string();
        let item = MediaItem {
            source: source_key.clone(),
            filename: "photo.jpg".to_string(),
            album: None,
            additional_albums: Vec::new(),
            date: None,
            time: None,
            tz: None,
            size_bytes: 5,
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
            has_json: true,
            duplicate_of: None,
            metadata: None,
        };
        let placement = Placement {
            src: source_key.clone(),
            dest: destination.to_string_lossy().to_string(),
            kind: PlacementKind::Original,
            tags: Vec::new(),
            live_action: None,
        };
        let mut settings = PlannerSettings::default();
        settings.output_root = Some(output_root.to_string_lossy().to_string());
        let mut manifest =
            RunManifest::open(&directory.path().join("manifest.sqlite"), &output_root)
                .expect("open manifest");
        let (manifest_source, manifest_placement) =
            placement_manifest_identity(&item, &placement, &settings)
                .expect("create manifest identity");
        manifest
            .mark_done(&manifest_source, &manifest_placement, &destination)
            .expect("record completed output");
        let source: Arc<dyn Source> = crate::takeout::create_source(&source_root)
            .expect("create folder source")
            .into();
        let input_locators = HashMap::from([(
            source_key,
            InputLocator {
                source,
                archive_path: source_root.to_string_lossy().to_string(),
                entry_path: "photo.jpg".to_string(),
                video_duration_seconds: None,
            },
        )]);

        let summary = start_indexed_run_inner(
            &settings,
            &[item],
            &[placement],
            input_locators,
            HashSet::new(),
            Path::new(""),
            Some(manifest),
            None,
            &send_archive_to_trash,
            |_| Ok(()),
        )
        .expect("resume completed run");

        assert_eq!(summary.planned, 1);
        assert_eq!(summary.processed, 0);
        assert_eq!(summary.skipped, 1);
        assert_eq!(fs::read(destination).expect("read output"), b"saved");
    }

    #[test]
    fn free_space_threshold_is_the_larger_of_one_gb_and_two_percent() {
        assert_eq!(minimum_free_space(10_000_000_000), 1_000_000_000);
        assert_eq!(minimum_free_space(200_000_000_000), 4_000_000_000);
    }
}
