pub mod live;

pub use live::{apply_live_style, LiveStyle};

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use chrono::{Local, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::planner::{self, MediaItem, Placement, PlannerSettings};
use crate::platform::{disk_free_bytes, hard_link_or_copy};

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
        self.set("finished", if summary.cancelled { "cancelled" } else if summary.paused { "paused" } else { "complete" })?;
        self.set("summary", &serde_json::to_string(summary).unwrap_or_default())?;
        Ok(())
    }

    pub fn path(&self) -> &Path {
        self.path.as_path()
    }
}

pub fn start_run(settings: &PlannerSettings, items: &[MediaItem]) -> Result<RunSummary, String> {
    let control = run_control();
    let state = RunStateStore::new_temp()?;
    let placements = planner::plan(items, settings);

    let mut summary = RunSummary {
        dry_run: settings.dry_run,
        ..Default::default()
    };

    state.set("status", "running")?;
    state.mark_progress(0, placements.len() as u32, "planning")?;

    let start = Instant::now();
    for (idx, placement) in placements.iter().enumerate() {
        if control.lock().unwrap().is_cancelled() {
            summary.cancelled = true;
            break;
        }

        while control.lock().unwrap().is_paused() {
            thread::sleep(Duration::from_millis(200));
        }

        if settings.dry_run {
            summary.planned += 1;
            continue;
        }

        match write_placement(settings, placement) {
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

        if let Ok(free_bytes) = disk_free_bytes(Path::new(&placement.dest).parent().unwrap_or(Path::new("."))) {
            if free_bytes < 1_000_000_000 {
                control.lock().unwrap().pause();
                summary.paused = true;
                summary.warnings.push("Low disk space: paused. Remove space and resume.".to_string());
                state.set("status", "paused")?;
                break;
            }
        }

        if start.elapsed().as_millis() > 0 && idx as u32 % 2 == 0 {
            let _ = state.set("last-progress", &format!("{} of {}", idx + 1, placements.len()));
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

fn write_placement(settings: &PlannerSettings, placement: &Placement) -> Result<PlacementResult, String> {
    let src = Path::new(&placement.src);
    let dest = Path::new(&placement.dest);
    let mut result = PlacementResult::default();

    if !src.exists() {
        return Err(format!("source missing: {}", src.display()));
    }

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create directory {}: {e}", parent.display()))?;
    }

    if dest.exists() {
        let same = same_file(src, dest).unwrap_or(false);
        if same {
            result.skipped = true;
            return Ok(result);
        }
    }

    let tmp = temp_path_for(dest);
    let write_ok = match placement.kind {
        crate::planner::PlacementKind::Link => hard_link_or_copy(src, &tmp),
        _ => copy_into_temp(src, &tmp),
    };

    if let Err(err) = write_ok {
        return Err(err);
    }

    if let Err(err) = fs::rename(&tmp, dest) {
        let _ = fs::remove_file(&tmp);
        return Err(format!("rename temp file {} -> {}: {err}", tmp.display(), dest.display()));
    }

    if settings.write_descriptions {
        if let Err(err) = write_metadata(dest, settings) {
            result.warnings.push(format!("metadata update failed for {}: {err}", dest.display()));
        }
    }

    if settings.rate_favorites && placement.tags.iter().any(|tag| tag == "favorite") {
        if let Err(err) = set_rating(dest, 5) {
            result.warnings.push(format!("favorite rating failed for {}: {err}", dest.display()));
        }
    }

    if !settings.delete_archives_after {
        let _ = update_file_times(dest);
    }

    Ok(result)
}

fn same_file(left: &Path, right: &Path) -> Result<bool, String> {
    let left_meta = fs::metadata(left).map_err(|e| format!("read source metadata {}: {e}", left.display()))?;
    let right_meta = fs::metadata(right).map_err(|e| format!("read target metadata {}: {e}", right.display()))?;
    if left_meta.len() != right_meta.len() {
        return Ok(false);
    }

    let left_hash = blake3::hash(&fs::read(left).map_err(|e| format!("read source {}: {e}", left.display()))?);
    let right_hash = blake3::hash(&fs::read(right).map_err(|e| format!("read target {}: {e}", right.display()))?);
    Ok(left_hash == right_hash)
}

fn temp_path_for(dest: &Path) -> PathBuf {
    let mut temp = dest.as_os_str().to_os_string();
    temp.push(".tmp");
    PathBuf::from(temp)
}

fn copy_into_temp(src: &Path, temp: &Path) -> Result<(), String> {
    fs::copy(src, temp).map_err(|e| format!("copy {} -> {}: {e}", src.display(), temp.display()))?;
    Ok(())
}

fn write_metadata(path: &Path, settings: &PlannerSettings) -> Result<(), String> {
    let exiftool = std::process::Command::new("exiftool").arg("-ver").output();
    if exiftool.is_ok() {
        let now = Local::now();
        let iso = now.format("%Y:%m:%d %H:%M:%S").to_string();
        let status = std::process::Command::new("exiftool")
            .arg("-overwrite_original")
            .arg(format!("-DateTimeOriginal={iso}"))
            .arg(path)
            .status()
            .map_err(|e| format!("run exiftool for {}: {e}", path.display()))?;
        if !status.success() {
            return Err(format!("exiftool failed for {}", path.display()));
        }
    }

    if settings.write_gps {
        let _ = std::process::Command::new("attrib").arg("+R").arg(path).status();
    }

    update_file_times(path)?;
    Ok(())
}

fn set_rating(path: &Path, stars: u8) -> Result<(), String> {
    let exiftool = std::process::Command::new("exiftool").arg("-ver").output();
    if exiftool.is_ok() {
        let status = std::process::Command::new("exiftool")
            .arg("-overwrite_original")
            .arg(format!("-Rating={stars}"))
            .arg(path)
            .status()
            .map_err(|e| format!("set rating for {}: {e}", path.display()))?;
        if !status.success() {
            return Err(format!("set rating failed for {}", path.display()));
        }
    }
    Ok(())
}

fn update_file_times(path: &Path) -> Result<(), String> {
    let now = Utc::now();
    let system_time = std::time::SystemTime::from(now);
    let _ = fs::File::open(path).and_then(|mut file| file.set_modified(system_time));
    Ok(())
}

pub fn pause_run() {
    run_control().lock().unwrap().pause();
}

pub fn resume_run() {
    run_control().lock().unwrap().resume();
}

pub fn cancel_run() {
    run_control().lock().unwrap().cancel();
}

pub fn dry_run_preview(settings: &PlannerSettings, items: &[MediaItem]) -> Vec<String> {
    planner::plan(items, settings).into_iter().map(|p| p.dest).collect()
}

pub fn delete_archives_after(settings: &PlannerSettings, paths: &[String]) -> Result<Vec<String>, String> {
    if !settings.delete_archives_after {
        return Ok(Vec::new());
    }

    let mut deleted = Vec::new();
    for path in paths {
        let p = Path::new(path);
        let lower = p.extension().and_then(|ext| ext.to_str()).unwrap_or("").to_ascii_lowercase();
        if matches!(lower.as_str(), "zip" | "tgz") {
            if p.exists() {
                fs::remove_file(p).map_err(|e| format!("delete archive {}: {e}", p.display()))?;
                deleted.push(path.clone());
            }
        }
    }
    Ok(deleted)
}
