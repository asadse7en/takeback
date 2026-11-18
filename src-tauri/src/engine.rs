pub mod live;

pub use live::{apply_live_style, LiveStyle};

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

use crate::metadata::{supports_embedded_metadata, ExifTool, TakeoutMetadata};
use crate::planner::{self, MediaItem, Placement, PlannerSettings};
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
}

impl RunControl {
    fn reset(&self) {
        self.paused.store(false, Ordering::SeqCst);
        self.cancelled.store(false, Ordering::SeqCst);
    }

    pub fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
    }

    pub fn resume(&self) {
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
        let path = dir.join(format!("run-{}.sqlite", std::process::id()));
        let conn = Connection::open(&path).map_err(|e| format!("open run state sqlite: {e}"))?;
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

    pub fn path(&self) -> &Path {
        self.path.as_path()
    }
}

pub fn start_run(
    settings: &PlannerSettings,
    items: &[MediaItem],
    exiftool_path: &Path,
) -> Result<RunSummary, String> {
    let control = run_control();
    let state = RunStateStore::new_temp()?;
    let placements = planner::plan(items, settings);
    let has_metadata = !settings.dry_run
        && !placements.is_empty()
        && items.iter().any(|item| {
            item.metadata.is_some() && supports_embedded_metadata(Path::new(&item.source))
        });
    let has_live_photo_writes = !settings.dry_run
        && placements.iter().any(|placement| {
            placement.tags.iter().any(|tag| {
                tag.starts_with("LivePhotoIphone:") || tag.starts_with("LivePhotoAndroid:")
            })
        });
    let mut exiftool = if has_metadata || has_live_photo_writes {
        Some(ExifTool::new(exiftool_path)?)
    } else {
        None
    };
    let metadata_by_source: HashMap<&str, &TakeoutMetadata> = items
        .iter()
        .filter_map(|item| {
            item.metadata
                .as_ref()
                .map(|metadata| (item.source.as_str(), metadata))
        })
        .collect();
    let mut processed_outputs = HashMap::<String, PathBuf>::new();

    let mut summary = RunSummary {
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

        match write_placement(
            settings,
            placement,
            metadata_by_source.get(placement.src.as_str()).copied(),
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

pub fn start_indexed_run_with_progress(
    settings: &PlannerSettings,
    items: &[MediaItem],
    placements: &[Placement],
    input_locators: HashMap<String, InputLocator>,
    archives_to_delete: HashSet<String>,
    exiftool_path: &Path,
    mut emit_progress: impl FnMut(&crate::events::ProcessingProgressEvent) -> Result<(), String>,
) -> Result<RunSummary, String> {
    let control = run_control();
    control
        .lock()
        .map_err(|error| format!("run control is unavailable: {error}"))?
        .reset();

    let mut tasks_by_source = HashMap::<String, usize>::new();
    let mut tasks = Vec::<ProcessingTask>::new();
    let items_by_source = items
        .iter()
        .map(|item| (item.source.as_str(), item))
        .collect::<HashMap<_, _>>();
    for placement in placements {
        let Some(item) = items_by_source.get(placement.src.as_str()) else {
            return Err(format!(
                "The planner returned an unknown source: {}",
                placement.src
            ));
        };
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
    for placement in placements {
        let archive_path = input_locators
            .get(&placement.src)
            .ok_or_else(|| format!("No archive source was indexed for {}", placement.src))?
            .archive_path
            .clone();
        if archives_to_delete.contains(&archive_path) {
            *archive_remaining.entry(archive_path.clone()).or_default() += 1;
            archive_success.entry(archive_path).or_insert(true);
        }
    }

    let mut summary = RunSummary {
        planned: placements.len() as u32,
        dry_run: settings.dry_run,
        files_without_json: items.iter().filter(|item| !item.has_json).count() as u32,
        live_photos_merged: if settings.merge_live && settings.live_style != "separate" {
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
    state.set("status", "running")?;
    state.mark_progress(0, summary.planned, "planning")?;

    let total_bytes = placements.iter().try_fold(0_u64, |total, placement| {
        let item = items_by_source
            .get(placement.src.as_str())
            .ok_or_else(|| format!("missing size for {}", placement.src))?;
        total
            .checked_add(item.size_bytes)
            .ok_or_else(|| "total processing size is too large to calculate".to_string())
    })?;
    let mut current_file = None;
    let mut completed_bytes = 0_u64;
    let mut last_emit = Instant::now() - Duration::from_secs(1);
    emit_indexed_progress(
        &mut emit_progress,
        &summary,
        current_file.as_deref(),
        0,
        summary.planned,
        completed_bytes,
        total_bytes,
        Duration::ZERO,
    )?;

    if settings.dry_run || tasks.is_empty() {
        state.mark_progress(summary.planned, summary.planned, "complete")?;
        state.mark_finished(&summary)?;
        return Ok(summary);
    }

    let needs_exiftool = tasks.iter().any(|task| {
        task.item.metadata.is_some() && supports_embedded_metadata(Path::new(&task.item.filename))
            || task.placements.iter().any(|placement| {
                placement.tags.iter().any(|tag| {
                    tag.starts_with("LivePhotoIphone:") || tag.starts_with("LivePhotoAndroid:")
                })
            })
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
    for task in tasks {
        task_sender
            .send(task)
            .map_err(|error| format!("queue processing work: {error}"))?;
    }
    drop(task_sender);

    let started = Instant::now();
    let mut completed = 0_u32;
    let mut event_error = None;
    for completion in result_receiver {
        completed = completed.saturating_add(1);
        current_file = Some(completion.item.filename.clone());
        completed_bytes = completed_bytes.saturating_add(completion.item.size_bytes);
        let archive_path = completion.archive_path.clone();
        let output_succeeded = matches!(&completion.result, Ok(result) if !result.skipped);

        if !completion.item.has_json {
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
                    let metadata_failed = result
                        .warnings
                        .iter()
                        .any(|warning| warning.starts_with("Could not write metadata"));
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
            Err(error)
                if control
                    .lock()
                    .map_err(|lock_error| format!("run control is unavailable: {lock_error}"))?
                    .is_cancelled() =>
            {
                let _ = error;
                summary.cancelled = true;
            }
            Err(error) => {
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

        if let Some(remaining) = archive_remaining.get_mut(&archive_path) {
            if !output_succeeded {
                archive_success.insert(archive_path.clone(), false);
            }
            *remaining = remaining.saturating_sub(1);
            if *remaining == 0 && archive_success.get(&archive_path) == Some(&true) {
                match delete_archives_after(settings, std::slice::from_ref(&archive_path)) {
                    Ok(deleted) => {
                        summary.archives_deleted = summary
                            .archives_deleted
                            .saturating_add(deleted.len() as u32);
                    }
                    Err(error) => push_bounded(&mut summary.warnings, error),
                }
            }
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
            state.mark_progress(completed, summary.planned, "processing")?;
        }
    }
    for worker in workers {
        worker
            .join()
            .map_err(|_| "a processing worker stopped unexpectedly".to_string())??;
    }

    if control
        .lock()
        .map_err(|error| format!("run control is unavailable: {error}"))?
        .is_paused()
    {
        summary.paused = true;
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
}

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
    loop {
        let task = receiver
            .lock()
            .map_err(|error| format!("processing queue is unavailable: {error}"))?
            .recv();
        let Ok(task) = task else {
            return Ok(());
        };
        let mut processed_outputs = HashMap::new();
        for placement in task.placements {
            let result = if run_control()
                .lock()
                .map_err(|error| format!("run control is unavailable: {error}"))?
                .is_cancelled()
            {
                Err("Processing was cancelled.".to_string())
            } else {
                write_placement_with_sources(
                    &settings,
                    &placement,
                    task.item.metadata.as_ref(),
                    &mut exiftool,
                    &mut processed_outputs,
                    &input_locators,
                    task.item.embedded_motion_video,
                )
            };
            sender
                .send(PlacementCompletion {
                    item: task.item.clone(),
                    archive_path: task.archive_path.clone(),
                    placement,
                    result,
                })
                .map_err(|error| format!("return processing result: {error}"))?;
        }
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
    exiftool: &mut Option<ExifTool>,
    processed_outputs: &mut HashMap<String, PathBuf>,
) -> Result<PlacementResult, String> {
    write_placement_with_sources(
        settings,
        placement,
        metadata,
        exiftool,
        processed_outputs,
        &HashMap::new(),
        false,
    )
}

fn write_placement_with_sources(
    settings: &PlannerSettings,
    placement: &Placement,
    metadata: Option<&TakeoutMetadata>,
    exiftool: &mut Option<ExifTool>,
    processed_outputs: &mut HashMap<String, PathBuf>,
    input_locators: &HashMap<String, InputLocator>,
    embedded_motion_marker: bool,
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
        copy_source_into_temp(&placement.src, tmp_path, input_locators)?;
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
        if supports_embedded_metadata(dest) {
            let writer = exiftool.as_mut().ok_or_else(|| {
                "bundled ExifTool is not available for metadata writes".to_string()
            })?;
            match writer.write(
                tmp_path,
                metadata.ok_or_else(|| "missing Takeout metadata".to_string())?,
                settings.time_zone,
                settings.write_gps,
                settings.write_descriptions,
                settings.rate_favorites,
            ) {
                Ok(warnings) => result.warnings.extend(warnings),
                Err(error) => {
                    copy_source_into_temp(&placement.src, tmp_path, input_locators).map_err(|restore_error| {
                        format!("metadata write failed: {error}; restore output failed: {restore_error}")
                    })?;
                    set_taken_file_time(
                        tmp_path,
                        metadata.ok_or_else(|| "missing Takeout metadata".to_string())?,
                    )?;
                    result.warnings.push(format!(
                        "Could not write metadata to {}; the original file was preserved: {error}",
                        dest.display()
                    ));
                }
            }
        } else if let Some(metadata) = metadata {
            set_taken_file_time(tmp_path, metadata)?;
            result.warnings.push(format!(
                "Metadata is not safely supported for {}; the file was copied without metadata.",
                dest.display()
            ));
        }
    }

    if let Some(tag) = placement
        .tags
        .iter()
        .find(|tag| tag.starts_with("LivePhotoIphone:"))
    {
        let content_identifier = tag
            .strip_prefix("LivePhotoIphone:")
            .ok_or_else(|| "invalid iPhone live photo action".to_string())?;
        let writer = exiftool
            .as_mut()
            .ok_or_else(|| "bundled ExifTool is not available for live photo writes".to_string())?;
        apply_live_style(
            writer,
            LiveStyle::Iphone,
            tmp_path,
            None,
            content_identifier,
        )?;
    } else if let Some(tag) = placement
        .tags
        .iter()
        .find(|tag| tag.starts_with("LivePhotoAndroid:"))
    {
        let action = tag
            .strip_prefix("LivePhotoAndroid:")
            .ok_or_else(|| "invalid Android Motion Photo action".to_string())?;
        let (content_identifier, video_source) = action
            .split_once(':')
            .ok_or_else(|| "Android Motion Photo action has no video source".to_string())?;
        let writer = exiftool
            .as_mut()
            .ok_or_else(|| "bundled ExifTool is not available for live photo writes".to_string())?;
        let video_temp = if input_locators.contains_key(video_source) {
            let temp = temp_path_for(dest)?;
            let video_temp_path: &Path = temp.as_ref();
            copy_source_into_temp(video_source, video_temp_path, input_locators)?;
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
            LiveStyle::Android,
            tmp_path,
            Some(video_path),
            content_identifier,
        )?;
    }
    if let Some(metadata) = metadata {
        set_taken_file_time(tmp_path, metadata)?;
    }

    if run_control()
        .lock()
        .map_err(|error| format!("run control is unavailable: {error}"))?
        .is_cancelled()
    {
        return Err("Processing was cancelled.".to_string());
    }

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

    Ok(result)
}

fn copy_source_into_temp(
    source_key: &str,
    destination: &Path,
    input_locators: &HashMap<String, InputLocator>,
) -> Result<(), String> {
    let output = fs::File::create(destination)
        .map_err(|error| format!("create temporary output {}: {error}", destination.display()))?;
    let mut output = CancelWriter { file: output };
    if let Some(locator) = input_locators.get(source_key) {
        locator
            .source
            .copy_entry_to(&locator.entry_path, &mut output)?;
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
        loop {
            let control = run_control()
                .lock()
                .map_err(|_| io::Error::new(io::ErrorKind::Other, "run control is unavailable"))?;
            if control.is_cancelled() {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "processing was cancelled",
                ));
            }
            if !control.is_paused() {
                break;
            }
            drop(control);
            thread::sleep(Duration::from_millis(100));
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
                fs::remove_file(p).map_err(|e| format!("delete archive {}: {e}", p.display()))?;
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
        };
        let mut exiftool = None;
        let mut processed_outputs = HashMap::new();

        let result = write_placement(
            &PlannerSettings::default(),
            &placement,
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
}
