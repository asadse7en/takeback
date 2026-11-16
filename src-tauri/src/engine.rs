pub mod live;

pub use live::{apply_live_style, LiveStyle};

use std::collections::HashMap;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::metadata::{supports_embedded_metadata, ExifTool, TakeoutMetadata};
use crate::planner::{self, MediaItem, Placement, PlannerSettings};
use crate::platform::disk_free_bytes;

pub fn status() -> &'static str {
    "idle"
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RunProgressEvent {
    pub file: String,
    pub completed: u32,
    pub total: u32,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RunSummary {
    pub planned: u32,
    pub processed: u32,
    pub skipped: u32,
    pub dry_run: bool,
    pub paused: bool,
    pub cancelled: bool,
    pub warnings: Vec<String>,
    pub problems: Vec<String>,
}

#[derive(Debug, Default)]
pub struct RunControl {
    paused: AtomicBool,
    cancelled: AtomicBool,
}

impl RunControl {
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
    let mut exiftool = if has_metadata {
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

    state.mark_finished(&summary)?;
    Ok(summary)
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
    let src = Path::new(&placement.src);
    let dest = Path::new(&placement.dest);
    let mut result = PlacementResult::default();

    if !src.exists() {
        return Err(format!("source missing: {}", src.display()));
    }

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("create directory {}: {e}", parent.display()))?;
    }

    if dest.exists() {
        let same = same_file(src, dest)?;
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
    } else if placement.kind == crate::planner::PlacementKind::Link && !metadata_output {
        fs::remove_file(tmp_path)
            .map_err(|error| format!("prepare temporary link {}: {error}", tmp_path.display()))?;
        crate::platform::hard_link_or_copy(src, tmp_path)?;
    } else {
        copy_into_temp(src, tmp_path)?;
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
                    copy_into_temp(src, tmp_path).map_err(|restore_error| {
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

    if let Err(err) = fs::rename(tmp_path, dest) {
        let _ = fs::remove_file(tmp_path);
        return Err(format!(
            "rename temp file {} -> {}: {err}",
            tmp_path.display(),
            dest.display()
        ));
    }

    if metadata_output {
        processed_outputs.insert(placement.src.clone(), dest.to_path_buf());
    }

    Ok(result)
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

fn copy_into_temp(src: &Path, temp: &Path) -> Result<(), String> {
    let mut source = fs::File::open(src)
        .map_err(|error| format!("open source file {}: {error}", src.display()))?;
    let mut destination = fs::File::create(temp)
        .map_err(|error| format!("create temporary output {}: {error}", temp.display()))?;
    io::copy(&mut source, &mut destination)
        .map_err(|error| format!("copy {} -> {}: {error}", src.display(), temp.display()))?;
    Ok(())
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
        if matches!(lower.as_str(), "zip" | "tgz") {
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
}
