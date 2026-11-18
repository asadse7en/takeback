use std::collections::HashMap;
use std::fs;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::archive::{
    AddSourcesResult, ArchiveType, EntryType, IndexedEntry, SourceEntry, SourceRecord,
};
use flate2::read::GzDecoder;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tar::Archive;
use walkdir::WalkDir;
use zip::ZipArchive;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    pub photos: u32,
    pub videos: u32,
    pub live: u32,
    pub total_gb: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ScanSourcesResult {
    pub files: Vec<String>,
    pub counts: Counts,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanWarning {
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct ScanProgressUpdate {
    pub archive_index: u32,
    pub archive_count: u32,
    pub current_archive: String,
    pub entries_seen: u64,
    pub bytes_read: u64,
    pub total_bytes: u64,
}

pub const SCAN_CANCELLED_ERROR: &str = "Scan cancelled.";

pub trait Source: Send + Sync {
    fn visit_entries(
        &self,
        visitor: &mut dyn FnMut(&SourceEntry, &mut dyn Read) -> Result<(), String>,
        cancelled: Option<&AtomicBool>,
        on_invalid_path: &mut dyn FnMut(&str, &str) -> Result<(), String>,
    ) -> Result<(), String>;

    fn copy_entry_to(&self, entry_path: &str, output: &mut dyn Write) -> Result<u64, String>;

    fn hash_entry_prefix(&self, entry_path: &str) -> Result<blake3::Hash, String>;

    fn read_video_duration_seconds(&self, entry_path: &str) -> Result<Option<f64>, String>;

    fn list_entries(&self) -> Result<Vec<SourceEntry>, String> {
        let mut entries = Vec::new();
        self.visit_entries(
            &mut |entry, _| {
                entries.push(entry.clone());
                Ok(())
            },
            None,
            &mut |entry_path, error| {
                Err(format!("invalid source entry path {entry_path}: {error}"))
            },
        )?;
        Ok(entries)
    }
}

fn check_scan_cancelled(cancelled: Option<&AtomicBool>) -> Result<(), String> {
    if cancelled.is_some_and(|flag| flag.load(Ordering::SeqCst)) {
        Err(SCAN_CANCELLED_ERROR.to_string())
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ZipSource {
    pub path: PathBuf,
}

impl ZipSource {
    fn entry_count(&self) -> Result<u64, String> {
        let file = fs::File::open(&self.path)
            .map_err(|error| format!("open zip {}: {error}", self.path.display()))?;
        let archive = ZipArchive::new(file)
            .map_err(|error| format!("read zip {}: {error}", self.path.display()))?;
        u64::try_from(archive.len())
            .map_err(|_| format!("zip {} has too many entries", self.path.display()))
    }
}

impl Source for ZipSource {
    fn visit_entries(
        &self,
        visitor: &mut dyn FnMut(&SourceEntry, &mut dyn Read) -> Result<(), String>,
        cancelled: Option<&AtomicBool>,
        on_invalid_path: &mut dyn FnMut(&str, &str) -> Result<(), String>,
    ) -> Result<(), String> {
        let file = fs::File::open(&self.path)
            .map_err(|e| format!("open zip {}: {e}", self.path.display()))?;
        let mut archive =
            ZipArchive::new(file).map_err(|e| format!("read zip {}: {e}", self.path.display()))?;

        for idx in 0..archive.len() {
            check_scan_cancelled(cancelled)?;
            let mut entry = archive
                .by_index(idx)
                .map_err(|e| format!("read zip entry {}: {e}", self.path.display()))?;
            if entry.is_dir() {
                continue;
            }
            let entry_path = entry.name().to_string();
            let path = match normalize_archive_entry_path(&entry_path) {
                Ok(path) => path,
                Err(error) => {
                    on_invalid_path(&entry_path, &error)?;
                    continue;
                }
            };
            let metadata = SourceEntry {
                entry_path,
                entry_type: entry_type_for_path(&path),
                path,
                size: entry.size(),
            };
            visitor(&metadata, &mut entry)?;
        }
        Ok(())
    }

    fn copy_entry_to(&self, entry_path: &str, output: &mut dyn Write) -> Result<u64, String> {
        let file = fs::File::open(&self.path)
            .map_err(|e| format!("open zip {}: {e}", self.path.display()))?;
        let mut archive =
            ZipArchive::new(file).map_err(|e| format!("read zip {}: {e}", self.path.display()))?;
        let mut entry = archive
            .by_name(entry_path)
            .map_err(|_| format!("zip entry not found: {entry_path}"))?;
        io::copy(&mut entry, output).map_err(|e| format!("stream zip entry {entry_path}: {e}"))
    }

    fn hash_entry_prefix(&self, entry_path: &str) -> Result<blake3::Hash, String> {
        let file = fs::File::open(&self.path)
            .map_err(|error| format!("open zip {}: {error}", self.path.display()))?;
        let mut archive = ZipArchive::new(file)
            .map_err(|error| format!("read zip {}: {error}", self.path.display()))?;
        let mut entry = archive
            .by_name(entry_path)
            .map_err(|error| format!("open entry {entry_path} for duplicate check: {error}"))?;
        hash_prefix(&mut entry).map_err(|error| format!("hash entry prefix {entry_path}: {error}"))
    }

    fn read_video_duration_seconds(&self, entry_path: &str) -> Result<Option<f64>, String> {
        let file = fs::File::open(&self.path)
            .map_err(|error| format!("open zip {}: {error}", self.path.display()))?;
        let mut archive = ZipArchive::new(file)
            .map_err(|error| format!("read zip {}: {error}", self.path.display()))?;
        let mut entry = archive
            .by_name(entry_path)
            .map_err(|error| format!("open video entry {entry_path}: {error}"))?;
        let size = entry.size();
        let data_start = entry.data_start();
        let stored = entry.compression() == zip::CompressionMethod::Stored;
        let mut head = Vec::new();
        entry
            .by_ref()
            .take(crate::takeout::matcher::VIDEO_DURATION_PROBE_BYTES as u64)
            .read_to_end(&mut head)
            .map_err(|error| format!("read video header {entry_path}: {error}"))?;
        drop(entry);
        drop(archive);
        if let Some(duration) = matcher::read_video_duration_seconds(&head[..])? {
            return Ok(Some(duration));
        }
        if !stored || size <= matcher::VIDEO_DURATION_PROBE_BYTES as u64 {
            return Ok(None);
        }

        let tail_len = size.min(matcher::VIDEO_DURATION_PROBE_BYTES as u64);
        let tail_start = data_start
            .checked_add(size - tail_len)
            .ok_or_else(|| format!("video entry offset overflows: {entry_path}"))?;
        let mut file = fs::File::open(&self.path)
            .map_err(|error| format!("open zip {}: {error}", self.path.display()))?;
        file.seek(SeekFrom::Start(tail_start))
            .map_err(|error| format!("seek to video tail {entry_path}: {error}"))?;
        let mut tail = vec![0; tail_len as usize];
        file.read_exact(&mut tail)
            .map_err(|error| format!("read video tail {entry_path}: {error}"))?;
        matcher::read_video_duration_from_tail(&tail)
    }
}

#[derive(Debug, Clone)]
pub struct TgzSource {
    pub path: PathBuf,
}

impl Source for TgzSource {
    fn visit_entries(
        &self,
        visitor: &mut dyn FnMut(&SourceEntry, &mut dyn Read) -> Result<(), String>,
        cancelled: Option<&AtomicBool>,
        on_invalid_path: &mut dyn FnMut(&str, &str) -> Result<(), String>,
    ) -> Result<(), String> {
        let file = fs::File::open(&self.path)
            .map_err(|e| format!("open tgz {}: {e}", self.path.display()))?;
        let decoder = GzDecoder::new(file);
        let mut archive = Archive::new(decoder);

        for entry in archive
            .entries()
            .map_err(|e| format!("read tgz {}: {e}", self.path.display()))?
        {
            check_scan_cancelled(cancelled)?;
            let mut file =
                entry.map_err(|e| format!("read tgz entry {}: {e}", self.path.display()))?;
            if !file.header().entry_type().is_file() {
                continue;
            }
            let entry_path = std::str::from_utf8(file.path_bytes().as_ref())
                .map_err(|e| format!("tgz entry path is not valid UTF-8: {e}"))?
                .to_string();
            let path = match normalize_archive_entry_path(&entry_path) {
                Ok(path) => path,
                Err(error) => {
                    on_invalid_path(&entry_path, &error)?;
                    continue;
                }
            };
            let metadata = SourceEntry {
                entry_path,
                entry_type: entry_type_for_path(&path),
                path,
                size: file.size(),
            };
            visitor(&metadata, &mut file)?;
        }
        Ok(())
    }

    fn copy_entry_to(&self, entry_path: &str, output: &mut dyn Write) -> Result<u64, String> {
        let file = fs::File::open(&self.path)
            .map_err(|e| format!("open tgz {}: {e}", self.path.display()))?;
        let decoder = GzDecoder::new(file);
        let mut archive = Archive::new(decoder);

        for entry in archive
            .entries()
            .map_err(|e| format!("read tgz {}: {e}", self.path.display()))?
        {
            let mut file =
                entry.map_err(|e| format!("read tgz entry {}: {e}", self.path.display()))?;
            let path_bytes = file.path_bytes();
            let name = std::str::from_utf8(path_bytes.as_ref())
                .map_err(|e| format!("tgz entry path is not valid UTF-8: {e}"))?;
            if name == entry_path {
                return io::copy(&mut file, output)
                    .map_err(|e| format!("stream tgz entry {entry_path}: {e}"));
            }
        }
        Err(format!("tgz entry not found: {entry_path}"))
    }

    fn hash_entry_prefix(&self, entry_path: &str) -> Result<blake3::Hash, String> {
        let file = fs::File::open(&self.path)
            .map_err(|error| format!("open tgz {}: {error}", self.path.display()))?;
        let decoder = GzDecoder::new(file);
        let mut archive = Archive::new(decoder);
        for entry in archive
            .entries()
            .map_err(|error| format!("read tgz {}: {error}", self.path.display()))?
        {
            let mut entry = entry
                .map_err(|error| format!("read tgz entry {}: {error}", self.path.display()))?;
            if !entry.header().entry_type().is_file() {
                continue;
            }
            let path_bytes = entry.path_bytes();
            let name = std::str::from_utf8(path_bytes.as_ref())
                .map_err(|error| format!("tgz entry path is not valid UTF-8: {error}"))?;
            if name == entry_path {
                return hash_prefix(&mut entry)
                    .map_err(|error| format!("hash entry prefix {entry_path}: {error}"));
            }
        }
        Err(format!("tgz entry not found: {entry_path}"))
    }

    fn read_video_duration_seconds(&self, entry_path: &str) -> Result<Option<f64>, String> {
        let target_path = normalize_archive_entry_path(entry_path)?;
        let file = fs::File::open(&self.path)
            .map_err(|error| format!("open tgz {}: {error}", self.path.display()))?;
        let decoder = GzDecoder::new(file);
        let mut archive = Archive::new(decoder);
        for item in archive
            .entries()
            .map_err(|error| format!("read tgz {}: {error}", self.path.display()))?
        {
            let mut entry =
                item.map_err(|error| format!("read tgz entry {}: {error}", self.path.display()))?;
            if !entry.header().entry_type().is_file() {
                continue;
            }
            let path_bytes = entry.path_bytes();
            let raw_path = std::str::from_utf8(path_bytes.as_ref())
                .map_err(|error| format!("tgz entry path is not valid UTF-8: {error}"))?;
            if normalize_archive_entry_path(raw_path)? != target_path {
                continue;
            }
            let mut head = Vec::new();
            (&mut entry)
                .take(matcher::VIDEO_DURATION_PROBE_BYTES as u64)
                .read_to_end(&mut head)
                .map_err(|error| format!("read video header {entry_path}: {error}"))?;
            return matcher::read_video_duration_seconds(&head[..]);
        }
        Err(format!("tgz entry not found: {entry_path}"))
    }
}

#[derive(Debug, Clone)]
pub struct FolderSource {
    pub root: PathBuf,
}

impl Source for FolderSource {
    fn visit_entries(
        &self,
        visitor: &mut dyn FnMut(&SourceEntry, &mut dyn Read) -> Result<(), String>,
        cancelled: Option<&AtomicBool>,
        on_invalid_path: &mut dyn FnMut(&str, &str) -> Result<(), String>,
    ) -> Result<(), String> {
        for entry in WalkDir::new(&self.root).into_iter() {
            check_scan_cancelled(cancelled)?;
            let entry = entry.map_err(|e| format!("read folder {}: {e}", self.root.display()))?;
            let path = entry.path();
            if !entry.file_type().is_file() {
                continue;
            }
            let relative = path
                .strip_prefix(&self.root)
                .map_err(|e| format!("resolve folder entry {}: {e}", path.display()))?;
            let entry_path = relative
                .to_str()
                .ok_or_else(|| {
                    format!("folder entry path is not valid Unicode: {}", path.display())
                })?
                .replace('\\', "/");
            let normalized = match normalize_archive_entry_path(&entry_path) {
                Ok(path) => path,
                Err(error) => {
                    on_invalid_path(&entry_path, &error)?;
                    continue;
                }
            };
            let metadata = fs::metadata(path)
                .map_err(|e| format!("read file metadata {}: {e}", path.display()))?;
            let source_entry = SourceEntry {
                entry_path,
                entry_type: entry_type_for_path(&normalized),
                path: normalized,
                size: metadata.len(),
            };
            let mut file =
                fs::File::open(path).map_err(|e| format!("open file {}: {e}", path.display()))?;
            visitor(&source_entry, &mut file)?;
        }
        Ok(())
    }

    fn copy_entry_to(&self, entry_path: &str, output: &mut dyn Write) -> Result<u64, String> {
        let normalized = normalize_archive_entry_path(entry_path)?;
        let path = normalized
            .split('/')
            .fold(self.root.clone(), |current, part| current.join(part));
        let mut file =
            fs::File::open(&path).map_err(|e| format!("open file {}: {e}", path.display()))?;
        io::copy(&mut file, output).map_err(|e| format!("stream file {}: {e}", path.display()))
    }

    fn hash_entry_prefix(&self, entry_path: &str) -> Result<blake3::Hash, String> {
        let normalized = normalize_archive_entry_path(entry_path)?;
        let path = normalized
            .split('/')
            .fold(self.root.clone(), |current, part| current.join(part));
        let mut file = fs::File::open(&path)
            .map_err(|error| format!("open file {}: {error}", path.display()))?;
        hash_prefix(&mut file)
            .map_err(|error| format!("hash file prefix {}: {error}", path.display()))
    }

    fn read_video_duration_seconds(&self, entry_path: &str) -> Result<Option<f64>, String> {
        let normalized = normalize_archive_entry_path(entry_path)?;
        let path = normalized
            .split('/')
            .fold(self.root.clone(), |current, part| current.join(part));
        let metadata = fs::metadata(&path)
            .map_err(|error| format!("read video metadata {}: {error}", path.display()))?;
        let mut file = fs::File::open(&path)
            .map_err(|error| format!("open video {}: {error}", path.display()))?;
        matcher::read_video_duration_from_seekable(&mut file, metadata.len())
    }
}

fn hash_prefix(reader: &mut dyn Read) -> io::Result<blake3::Hash> {
    const PREFIX_BYTES: usize = 64 * 1024;
    let mut hasher = blake3::Hasher::new();
    let mut read_bytes = 0;
    let mut buffer = [0; 8 * 1024];
    while read_bytes < PREFIX_BYTES {
        let requested = buffer.len().min(PREFIX_BYTES - read_bytes);
        let count = reader.read(&mut buffer[..requested])?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
        read_bytes += count;
    }
    Ok(hasher.finalize())
}

#[derive(Debug)]
pub struct TempIndex {
    conn: Connection,
    skipped_entry_warnings: std::sync::Mutex<HashMap<String, SkippedEntryWarnings>>,
    _temp_dir: tempfile::TempDir,
}

#[derive(Debug, Default)]
struct SkippedEntryWarnings {
    messages: Vec<String>,
    total: u64,
}

impl TempIndex {
    pub fn new() -> Result<Self, String> {
        let temp_dir =
            tempfile::tempdir().map_err(|e| format!("create temp index directory: {e}"))?;
        let db_path = temp_dir.path().join("takeback-index.sqlite");
        let conn = Connection::open(&db_path)
            .map_err(|e| format!("open temp sqlite index {}: {e}", db_path.display()))?;
        conn.execute_batch("PRAGMA journal_mode=OFF; PRAGMA synchronous=OFF;")
            .map_err(|e| format!("configure temp sqlite index: {e}"))?;
        conn.execute(
            "CREATE TABLE entries (
                archive_path TEXT NOT NULL,
                path TEXT NOT NULL,
                entry_path TEXT NOT NULL,
                size INTEGER NOT NULL,
                kind TEXT NOT NULL,
                album TEXT,
                trashed INTEGER NOT NULL,
                archived INTEGER NOT NULL,
                embedded_motion_video INTEGER NOT NULL,
                video_duration_seconds REAL,
                metadata_json TEXT,
                sidecar_archive_path TEXT,
                sidecar_path TEXT,
                sidecar_match TEXT,
                PRIMARY KEY (archive_path, path)
            )",
            (),
        )
        .map_err(|e| format!("create archive index table: {e}"))?;
        Ok(Self {
            conn,
            skipped_entry_warnings: std::sync::Mutex::new(HashMap::new()),
            _temp_dir: temp_dir,
        })
    }

    fn record_skipped_entry(
        &self,
        archive_path: &str,
        entry_path: &str,
        reason: &str,
    ) -> Result<(), String> {
        let mut warnings = self
            .skipped_entry_warnings
            .lock()
            .map_err(|error| format!("lock skipped-entry warnings: {error}"))?;
        let summary = warnings.entry(archive_path.to_string()).or_default();
        summary.total = summary.total.saturating_add(1);
        if summary.messages.len() < 100 {
            summary
                .messages
                .push(format!("Skipped entry {entry_path}: {reason}"));
        }
        Ok(())
    }

    fn skipped_entry_warning_messages(&self) -> Result<Vec<String>, String> {
        let warnings = self
            .skipped_entry_warnings
            .lock()
            .map_err(|error| format!("lock skipped-entry warnings: {error}"))?;
        let mut archives = warnings.iter().collect::<Vec<_>>();
        archives.sort_by(|(left, _), (right, _)| left.cmp(right));
        let total = archives
            .iter()
            .map(|(_, summary)| summary.total)
            .fold(0u64, u64::saturating_add);
        let mut messages = archives
            .iter()
            .flat_map(|(_, summary)| summary.messages.iter().cloned())
            .take(100)
            .collect::<Vec<_>>();
        if total > messages.len() as u64 {
            messages.push(format!(
                "Skipped {} additional entries; individual warning details are capped at 100.",
                total - messages.len() as u64
            ));
        }
        Ok(messages)
    }

    fn set_sidecar(
        &self,
        archive_path: &str,
        path: &str,
        sidecar_archive_path: &str,
        sidecar_path: &str,
        match_reason: &str,
    ) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE entries
             SET sidecar_archive_path = ?1, sidecar_path = ?2, sidecar_match = ?3
             WHERE archive_path = ?4 AND path = ?5",
                (
                    sidecar_archive_path,
                    sidecar_path,
                    match_reason,
                    archive_path,
                    path,
                ),
            )
            .map_err(|e| format!("pair JSON sidecar with {}: {e}", path))?;
        Ok(())
    }

    pub fn remove_archive(&self, archive_path: &str) -> Result<(), String> {
        self.conn
            .execute(
                "DELETE FROM entries WHERE archive_path = ?1",
                [archive_path],
            )
            .map_err(|error| format!("remove archive from index {archive_path}: {error}"))?;
        self.skipped_entry_warnings
            .lock()
            .map_err(|error| format!("lock skipped-entry warnings: {error}"))?
            .remove(archive_path);
        Ok(())
    }

    pub fn rematch_sidecars(&self) -> Result<(), String> {
        let entries = self.entries()?;
        let media_entries = entries
            .iter()
            .filter(|entry| matches!(entry.entry_type, EntryType::Photo | EntryType::Video))
            .collect::<Vec<_>>();
        let media_candidates = media_entries
            .iter()
            .map(|entry| matcher::MediaCandidate {
                archive_path: entry.archive_path.clone(),
                path: entry.path.clone(),
            })
            .collect::<Vec<_>>();
        let mut statement = self
            .conn
            .prepare(
                "SELECT archive_path, path, metadata_json
                 FROM entries WHERE kind = 'jsonSidecar'",
            )
            .map_err(|error| format!("prepare sidecar lookup: {error}"))?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })
            .map_err(|error| format!("query indexed sidecars: {error}"))?;
        let mut sidecars = Vec::new();
        for row in rows {
            let (archive_path, path, metadata_json) =
                row.map_err(|error| format!("read indexed sidecar: {error}"))?;
            let metadata: Option<crate::metadata::TakeoutMetadata> = metadata_json
                .map(|json| {
                    serde_json::from_str(&json)
                        .map_err(|error| format!("read indexed sidecar metadata: {error}"))
                })
                .transpose()?;
            sidecars.push(matcher::SidecarCandidate {
                archive_path,
                path,
                title: metadata.and_then(|metadata| metadata.title),
            });
        }

        self.conn
            .execute(
                "UPDATE entries
                 SET sidecar_archive_path = NULL, sidecar_path = NULL, sidecar_match = NULL",
                (),
            )
            .map_err(|error| format!("clear sidecar matches: {error}"))?;

        let matches = matcher::match_sidecars_for_media(&media_candidates, &sidecars);
        for (entry, matched) in media_entries.iter().zip(matches) {
            if let Some(matched) = matched {
                self.set_sidecar(
                    &entry.archive_path,
                    &entry.path,
                    &matched.archive_path,
                    &matched.path,
                    &format!("{:?}", matched.reason),
                )?;
            }
        }
        Ok(())
    }

    pub fn entries(&self) -> Result<Vec<IndexedEntry>, String> {
        self.entries_limited(None, EntryFilter::All)
    }

    pub fn media_entries(&self) -> Result<Vec<IndexedEntry>, String> {
        self.entries_limited(None, EntryFilter::Media)
    }

    pub fn processing_entries(&self) -> Result<Vec<IndexedEntry>, String> {
        self.entries_limited(None, EntryFilter::Processable)
    }

    pub fn preview_entries(&self, limit: usize) -> Result<Vec<IndexedEntry>, String> {
        self.entries_limited(Some(limit), EntryFilter::Processable)
    }

    fn entries_limited(
        &self,
        limit: Option<usize>,
        entry_filter: EntryFilter,
    ) -> Result<Vec<IndexedEntry>, String> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT entry.archive_path, entry.path, entry.entry_path, entry.size, entry.kind, entry.album,
                        entry.trashed, entry.archived, entry.embedded_motion_video,
                        entry.video_duration_seconds,
                        entry.sidecar_archive_path, entry.sidecar_path, entry.sidecar_match,
                        sidecar.metadata_json
                 FROM entries AS entry
                 LEFT JOIN entries AS sidecar
                   ON sidecar.archive_path = entry.sidecar_archive_path
                  AND sidecar.path = entry.sidecar_path
                 WHERE (?2 = 0 OR entry.kind IN ('photo', 'video')
                        OR (?2 = 2 AND entry.kind = 'other'))
                 ORDER BY entry.archive_path, entry.path
                 LIMIT COALESCE(?1, -1)",
            )
            .map_err(|e| format!("prepare archive index read: {e}"))?;
        let rows = statement
            .query_map(
                [
                    limit.map(|value| value.min(i64::MAX as usize) as i64),
                    Some(entry_filter as i64),
                ],
                |row| {
                    let kind: String = row.get(4)?;
                    let size: i64 = row.get(3)?;
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        size,
                        kind,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, bool>(6)?,
                        row.get::<_, bool>(7)?,
                        row.get::<_, bool>(8)?,
                        row.get::<_, Option<f64>>(9)?,
                        row.get::<_, Option<String>>(10)?,
                        row.get::<_, Option<String>>(11)?,
                        row.get::<_, Option<String>>(12)?,
                        row.get::<_, Option<String>>(13)?,
                    ))
                },
            )
            .map_err(|e| format!("query archive index: {e}"))?;
        let mut entries = Vec::new();
        for row in rows {
            let (
                archive_path,
                path,
                entry_path,
                size,
                kind,
                album,
                trashed,
                archived,
                embedded_motion_video,
                video_duration_seconds,
                sidecar_archive_path,
                sidecar_path,
                sidecar_match,
                metadata_json,
            ) = row.map_err(|e| format!("read archive index entry: {e}"))?;
            let entry_type = parse_entry_type(&kind)?;
            let size = u64::try_from(size).map_err(|e| format!("invalid indexed size: {e}"))?;
            let metadata: Option<crate::metadata::TakeoutMetadata> = metadata_json
                .map(|json| {
                    serde_json::from_str(&json)
                        .map_err(|error| format!("read indexed sidecar metadata: {error}"))
                })
                .transpose()?;
            entries.push(IndexedEntry {
                archive_path,
                path,
                entry_path,
                size,
                entry_type,
                album,
                trashed: trashed || metadata.as_ref().is_some_and(|fields| fields.trashed),
                archived: archived || metadata.as_ref().is_some_and(|fields| fields.archived),
                embedded_motion_video,
                video_duration_seconds,
                sidecar_archive_path,
                sidecar_path,
                sidecar_match,
                metadata,
            });
        }
        Ok(entries)
    }

    pub fn counts(&self) -> Result<Counts, String> {
        let stats: (u32, u32, f64) = self
            .conn
            .query_row(
                "SELECT
                    COALESCE(SUM(CASE WHEN kind='photo' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN kind='video' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN kind IN ('photo', 'video') THEN size ELSE 0 END), 0)
                        / 1024.0 / 1024.0 / 1024.0
                 FROM entries",
                (),
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(|e| format!("count indexed entries: {e}"))?;
        let candidates = self
            .media_entries()?
            .into_iter()
            .filter(|entry| matches!(entry.entry_type, EntryType::Photo | EntryType::Video))
            .map(|entry| matcher::LivePhotoCandidate {
                path: format!("{}::{}", entry.archive_path, entry.path),
                kind: if entry.entry_type == EntryType::Photo {
                    matcher::LiveMediaKind::Photo
                } else {
                    matcher::LiveMediaKind::Video
                },
                taken_timestamp: entry
                    .metadata
                    .as_ref()
                    .and_then(crate::metadata::TakeoutMetadata::taken_timestamp),
                duration_seconds: entry.video_duration_seconds,
                embedded_motion_marker: entry.embedded_motion_video,
            })
            .collect::<Vec<_>>();
        let live_detection = matcher::match_live_photos(&candidates);
        let live = live_detection.pairs.len() + live_detection.embedded_motion_photos.len();
        Ok(Counts {
            photos: stats.0,
            videos: stats.1,
            live: u32::try_from(live).map_err(|e| format!("live photo count exceeds u32: {e}"))?,
            total_gb: stats.2,
        })
    }

    fn google_photos_root(&self) -> Result<Option<String>, String> {
        let mut statement = self
            .conn
            .prepare("SELECT path FROM entries")
            .map_err(|e| format!("prepare archive-root lookup: {e}"))?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| format!("query archive paths: {e}"))?;
        let mut counts = HashMap::<String, usize>::new();
        for row in rows {
            let path = row.map_err(|e| format!("read archive path: {e}"))?;
            for component in Path::new(&path).components() {
                if let Component::Normal(part) = component {
                    let name = part.to_string_lossy();
                    if ["Google Photos", "Google Fotos", "Google フォト"]
                        .iter()
                        .any(|candidate| name.eq_ignore_ascii_case(candidate))
                    {
                        *counts.entry(name.to_string()).or_default() += 1;
                    }
                }
            }
        }
        Ok(counts
            .into_iter()
            .max_by_key(|(_, count)| *count)
            .map(|(name, _)| name))
    }
}

#[derive(Clone, Copy)]
#[repr(i64)]
enum EntryFilter {
    All = 0,
    Media = 1,
    Processable = 2,
}

struct IndexInsertBatch<'a> {
    connection: &'a rusqlite::Connection,
    transaction: Option<rusqlite::Transaction<'a>>,
    rows_since_commit: usize,
}

impl<'a> IndexInsertBatch<'a> {
    fn new(connection: &'a rusqlite::Connection) -> Result<Self, String> {
        let transaction = connection
            .unchecked_transaction()
            .map_err(|error| format!("begin archive index transaction: {error}"))?;
        Ok(Self {
            connection,
            transaction: Some(transaction),
            rows_since_commit: 0,
        })
    }

    fn insert(
        &mut self,
        archive_path: &str,
        entry: &SourceEntry,
        album: Option<&str>,
        trashed: bool,
        archived: bool,
        embedded_motion_video: bool,
        metadata: Option<&crate::metadata::TakeoutMetadata>,
    ) -> Result<(), String> {
        let size = i64::try_from(entry.size)
            .map_err(|_| format!("entry is too large to index: {}", entry.path))?;
        let transaction = self
            .transaction
            .as_ref()
            .ok_or_else(|| "archive index transaction is unavailable".to_string())?;
        let mut statement = transaction
            .prepare_cached(
                "INSERT OR REPLACE INTO entries (
                    archive_path, path, entry_path, size, kind, album, trashed, archived,
                    embedded_motion_video, video_duration_seconds, metadata_json
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, ?10)",
            )
            .map_err(|error| format!("prepare archive index insert: {error}"))?;
        statement
            .execute((
                archive_path,
                &entry.path,
                &entry.entry_path,
                size,
                entry_type_name(entry.entry_type),
                album,
                trashed,
                archived,
                embedded_motion_video,
                metadata
                    .map(serde_json::to_string)
                    .transpose()
                    .map_err(|error| format!("serialize JSON sidecar {}: {error}", entry.path))?,
            ))
            .map_err(|error| format!("index entry {}: {error}", entry.path))?;
        drop(statement);
        self.rows_since_commit += 1;
        if self.rows_since_commit == 5_000 {
            self.commit_and_continue()?;
        }
        Ok(())
    }

    fn commit_and_continue(&mut self) -> Result<(), String> {
        let transaction = self
            .transaction
            .take()
            .ok_or_else(|| "archive index transaction is unavailable".to_string())?;
        transaction
            .commit()
            .map_err(|error| format!("commit archive index batch: {error}"))?;
        self.rows_since_commit = 0;
        self.transaction = Some(
            self.connection
                .unchecked_transaction()
                .map_err(|error| format!("begin archive index transaction: {error}"))?,
        );
        Ok(())
    }

    fn finish(mut self) -> Result<(), String> {
        if let Some(transaction) = self.transaction.take() {
            transaction
                .commit()
                .map_err(|error| format!("commit archive index batch: {error}"))?;
        }
        Ok(())
    }
}

pub fn index_sources(paths: &[String]) -> Result<TempIndex, String> {
    let expanded_paths = expand_source_paths(paths)?;
    index_expanded_sources(&expanded_paths)
}

pub fn index_explicit_sources(paths: &[String]) -> Result<TempIndex, String> {
    index_expanded_sources(paths)
}

fn index_expanded_sources(paths: &[String]) -> Result<TempIndex, String> {
    let mut on_progress = |_| Ok(());
    index_expanded_sources_with_progress(paths, None, 0, &mut on_progress).map(|(index, _)| index)
}

fn index_expanded_sources_with_progress(
    paths: &[String],
    cancelled: Option<&AtomicBool>,
    total_bytes: u64,
    on_progress: &mut dyn FnMut(ScanProgressUpdate) -> Result<(), String>,
) -> Result<(TempIndex, ScanProgressUpdate), String> {
    let index = TempIndex::new()?;
    let archive_count = u32::try_from(paths.len()).unwrap_or(u32::MAX);
    let mut progress = ScanProgressUpdate::default();
    let mut last_progress = Instant::now();

    for (archive_offset, path) in paths.iter().enumerate() {
        index_archive_into(
            &index,
            path,
            u32::try_from(archive_offset + 1).unwrap_or(u32::MAX),
            archive_count,
            total_bytes,
            &mut progress,
            &mut last_progress,
            cancelled,
            on_progress,
        )?;
    }

    index.rematch_sidecars()?;
    Ok((index, progress))
}

pub fn index_archive_into(
    index: &TempIndex,
    path: &str,
    archive_index: u32,
    archive_count: u32,
    total_bytes: u64,
    progress: &mut ScanProgressUpdate,
    last_progress: &mut Instant,
    cancelled: Option<&AtomicBool>,
    on_progress: &mut dyn FnMut(ScanProgressUpdate) -> Result<(), String>,
) -> Result<(), String> {
    check_scan_cancelled(cancelled)?;
    let source_path = Path::new(path);
    let source = create_source(source_path)?;
    let archive_path = source_path.to_string_lossy().to_string();
    progress.archive_index = archive_index;
    progress.archive_count = archive_count;
    progress.current_archive.clone_from(&archive_path);
    progress.total_bytes = total_bytes;
    emit_scan_progress(on_progress, last_progress, progress)?;
    let mut insert_batch = IndexInsertBatch::new(&index.conn)?;
    let invalid_paths_seen = std::cell::Cell::new(0u64);

    let visit_result = source.visit_entries(
        &mut |entry, reader| {
            check_scan_cancelled(cancelled)?;
            let mut reader = CountingReader::new(reader, cancelled);
            let metadata = if entry.entry_type == EntryType::JsonSidecar {
                match crate::metadata::TakeoutMetadata::from_reader(&mut reader) {
                    Ok(metadata) => Some(metadata),
                    Err(error) => {
                        check_scan_cancelled(cancelled)?;
                        index.record_skipped_entry(&archive_path, &entry.entry_path, &error)?;
                        progress.entries_seen = progress.entries_seen.saturating_add(1);
                        progress.bytes_read =
                            progress.bytes_read.saturating_add(reader.bytes_read());
                        emit_scan_progress(on_progress, last_progress, progress)?;
                        return Ok(());
                    }
                }
            } else {
                None
            };
            let embedded_motion_video = if matcher::is_android_motion_photo_path(&entry.path) {
                matcher::has_motion_photo_xmp_markers(&mut reader)?
            } else {
                false
            };
            let entry_bytes_read = reader.bytes_read();
            let (album, trashed, archived) = classify_entry_path(&entry.path);
            insert_batch.insert(
                &archive_path,
                entry,
                album.as_deref(),
                trashed,
                archived,
                embedded_motion_video,
                metadata.as_ref(),
            )?;
            progress.entries_seen = progress.entries_seen.saturating_add(1);
            progress.bytes_read = progress.bytes_read.saturating_add(entry_bytes_read);
            emit_scan_progress(on_progress, last_progress, progress)?;
            Ok(())
        },
        cancelled,
        &mut |entry_path, error| {
            invalid_paths_seen.set(invalid_paths_seen.get().saturating_add(1));
            index.record_skipped_entry(&archive_path, entry_path, error)
        },
    );
    progress.entries_seen = progress
        .entries_seen
        .saturating_add(invalid_paths_seen.get());
    if let Err(error) = visit_result {
        drop(insert_batch);
        return match index.remove_archive(&archive_path) {
            Ok(()) => Err(error),
            Err(cleanup_error) => Err(format!(
                "{error}; could not remove partial archive index: {cleanup_error}"
            )),
        };
    }

    match insert_batch.finish() {
        Ok(()) => Ok(()),
        Err(error) => match index.remove_archive(&archive_path) {
            Ok(()) => Err(error),
            Err(cleanup_error) => Err(format!(
                "{error}; could not remove partial archive index: {cleanup_error}"
            )),
        },
    }
}

fn emit_scan_progress(
    on_progress: &mut dyn FnMut(ScanProgressUpdate) -> Result<(), String>,
    last_progress: &mut Instant,
    progress: &ScanProgressUpdate,
) -> Result<(), String> {
    if last_progress.elapsed() >= Duration::from_millis(250) {
        on_progress(progress.clone())?;
        *last_progress = Instant::now();
    }
    Ok(())
}

struct CountingReader<'a> {
    inner: &'a mut dyn Read,
    bytes_read: u64,
    cancelled: Option<&'a AtomicBool>,
}

impl<'a> CountingReader<'a> {
    fn new(inner: &'a mut dyn Read, cancelled: Option<&'a AtomicBool>) -> Self {
        Self {
            inner,
            bytes_read: 0,
            cancelled,
        }
    }

    fn bytes_read(&self) -> u64 {
        self.bytes_read
    }
}

impl Read for CountingReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self
            .cancelled
            .is_some_and(|flag| flag.load(Ordering::SeqCst))
        {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "scan cancelled"));
        }
        let count = self.inner.read(buffer)?;
        self.bytes_read = self.bytes_read.saturating_add(count as u64);
        Ok(count)
    }
}

fn classify_entry_path(path: &str) -> (Option<String>, bool, bool) {
    let normalized = path.replace('\\', "/");
    let parts: Vec<&str> = normalized
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    let trashed = parts.iter().any(|part| part.eq_ignore_ascii_case("trash"));
    let archived = parts
        .iter()
        .any(|part| part.eq_ignore_ascii_case("archive"));
    let album = matcher::album_membership(path).into_iter().next();
    (album, trashed, archived)
}

fn entry_type_for_path(path: &str) -> EntryType {
    if Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("metadata.json"))
    {
        return EntryType::AlbumMetadata;
    }
    let extension = Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_lowercase();
    if extension == "json" {
        EntryType::JsonSidecar
    } else if matches!(
        extension.as_str(),
        "jpg"
            | "jpeg"
            | "png"
            | "gif"
            | "bmp"
            | "avif"
            | "jfif"
            | "heic"
            | "heif"
            | "webp"
            | "tif"
            | "tiff"
            | "dng"
            | "cr2"
            | "nef"
            | "arw"
            | "orf"
            | "rw2"
            | "cr3"
            | "nrw"
            | "raf"
            | "pef"
            | "sr2"
    ) {
        EntryType::Photo
    } else if matches!(
        extension.as_str(),
        "mp4"
            | "mov"
            | "m4v"
            | "avi"
            | "mkv"
            | "3gp"
            | "mts"
            | "m2ts"
            | "webm"
            | "mpg"
            | "mpeg"
            | "wmv"
            | "flv"
            | "3g2"
            | "mp"
    ) {
        EntryType::Video
    } else {
        EntryType::Other
    }
}

fn entry_type_name(entry_type: EntryType) -> &'static str {
    match entry_type {
        EntryType::Photo => "photo",
        EntryType::Video => "video",
        EntryType::JsonSidecar => "jsonSidecar",
        EntryType::AlbumMetadata => "albumMetadata",
        EntryType::Other => "other",
    }
}

fn parse_entry_type(value: &str) -> Result<EntryType, String> {
    match value {
        "photo" => Ok(EntryType::Photo),
        "video" => Ok(EntryType::Video),
        "jsonSidecar" => Ok(EntryType::JsonSidecar),
        "albumMetadata" => Ok(EntryType::AlbumMetadata),
        "other" => Ok(EntryType::Other),
        _ => Err(format!("unknown archive entry type: {value}")),
    }
}

fn normalize_archive_entry_path(entry_path: &str) -> Result<String, String> {
    let normalized = entry_path.replace('\\', "/");
    let mut components = normalized.split('/');
    let first = components.next().unwrap_or_default();
    if normalized.starts_with('/')
        || first.ends_with(':')
        || normalized.split('/').any(|component| component == "..")
    {
        return Err(format!("unsafe path in Takeout source: {entry_path}"));
    }

    let path = normalized
        .split('/')
        .filter(|component| !component.is_empty() && *component != ".")
        .collect::<Vec<_>>()
        .join("/");
    if path.is_empty() {
        return Err("empty path in Takeout source".to_string());
    }
    Ok(path)
}

fn archive_type_for_path(path: &Path) -> Result<ArchiveType, String> {
    if path.is_dir() {
        return Ok(ArchiveType::Folder);
    }
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("source path is not valid Unicode: {}", path.display()))?
        .to_lowercase();
    if file_name.ends_with(".zip") {
        Ok(ArchiveType::Zip)
    } else if file_name.ends_with(".tgz") || file_name.ends_with(".tar.gz") {
        Ok(ArchiveType::Tgz)
    } else {
        Err(format!("unsupported Takeout source: {}", path.display()))
    }
}

fn numbered_part(path: &Path) -> Option<(String, u32)> {
    let file_name = path.file_name()?.to_str()?.to_lowercase();
    let stem = file_name.strip_suffix(".zip")?;
    let (prefix, number) = stem.rsplit_once('-')?;
    if number.len() < 3 || !number.chars().all(|character| character.is_ascii_digit()) {
        return None;
    }
    let number = number.parse().ok()?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let key = format!(
        "{}|{prefix}.zip",
        parent.to_string_lossy().replace('\\', "/").to_lowercase()
    );
    Some((key, number))
}

fn push_source_path(
    path: String,
    seen: &mut std::collections::HashSet<String>,
    expanded: &mut Vec<String>,
) {
    let key = path.replace('\\', "/").to_lowercase();
    if seen.insert(key) {
        expanded.push(path);
    }
}

pub fn expand_source_paths(paths: &[String]) -> Result<Vec<String>, String> {
    let mut expanded = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for path in paths.iter().filter(|path| !path.trim().is_empty()) {
        let selected = Path::new(path);
        let Some((group_key, _)) = numbered_part(selected) else {
            push_source_path(path.clone(), &mut seen, &mut expanded);
            continue;
        };
        let parent = selected
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let entries = fs::read_dir(parent).map_err(|error| {
            format!(
                "find Takeout archive parts in {}: {error}",
                parent.display()
            )
        })?;
        for entry in entries {
            let entry = entry.map_err(|error| {
                format!("read Takeout archive list in {}: {error}", parent.display())
            })?;
            if !entry
                .file_type()
                .map_err(|error| {
                    format!(
                        "read Takeout archive part metadata in {}: {error}",
                        parent.display()
                    )
                })?
                .is_file()
            {
                continue;
            }
            let candidate_path = entry.path();
            if numbered_part(&candidate_path)
                .is_some_and(|(candidate_group, _)| candidate_group == group_key)
            {
                push_source_path(
                    candidate_path.to_string_lossy().to_string(),
                    &mut seen,
                    &mut expanded,
                );
            }
        }
        push_source_path(path.clone(), &mut seen, &mut expanded);
    }

    expanded.sort_by_key(|path| path.to_lowercase());
    Ok(expanded)
}

pub fn inspect_sources(paths: &[String]) -> Result<AddSourcesResult, String> {
    let mut result = AddSourcesResult::default();
    let mut part_names = Vec::new();

    for source_path in expand_source_paths(paths)? {
        let path = Path::new(&source_path);
        let archive_type = archive_type_for_path(path)?;
        let (entry_count, entry_count_known, folder_size) = match archive_type {
            ArchiveType::Zip => (
                ZipSource {
                    path: path.to_path_buf(),
                }
                .entry_count()?,
                true,
                0,
            ),
            ArchiveType::Tgz => (0, false, 0),
            ArchiveType::Folder => {
                let (entry_count, folder_size) = folder_entry_stats(path)?;
                (entry_count, true, folder_size)
            }
        };
        let size_bytes = match archive_type {
            ArchiveType::Folder => folder_size,
            ArchiveType::Zip | ArchiveType::Tgz => fs::metadata(path)
                .map_err(|error| format!("read source metadata {}: {error}", path.display()))?
                .len(),
        };
        result.archives.push(SourceRecord {
            id: path.to_string_lossy().to_string(),
            path: path.to_string_lossy().to_string(),
            archive_type,
            size_bytes,
            entry_count,
            entry_count_known,
        });
        part_names.push(source_path);
    }

    for missing in detect_numbering_gap_from_names(&part_names) {
        result
            .warnings
            .push(format!("Missing Takeout archive part {missing:03}."));
    }
    Ok(result)
}

fn folder_entry_stats(root: &Path) -> Result<(u64, u64), String> {
    let mut entry_count = 0u64;
    let mut folder_size = 0u64;
    for entry in WalkDir::new(root) {
        let entry = entry.map_err(|error| format!("read folder {}: {error}", root.display()))?;
        if !entry.file_type().is_file() {
            continue;
        }
        let metadata = entry
            .metadata()
            .map_err(|error| format!("read file metadata {}: {error}", entry.path().display()))?;
        entry_count = entry_count
            .checked_add(1)
            .ok_or_else(|| "folder has too many files to count".to_string())?;
        folder_size = folder_size
            .checked_add(metadata.len())
            .ok_or_else(|| "folder size is too large to calculate".to_string())?;
    }
    Ok((entry_count, folder_size))
}

pub fn detect_numbering_gaps(numbers: &[u32]) -> Vec<u32> {
    if numbers.is_empty() {
        return Vec::new();
    }
    let set = numbers
        .iter()
        .copied()
        .collect::<std::collections::HashSet<_>>();
    let start = numbers.iter().copied().min().unwrap_or(0);
    let end = numbers.iter().copied().max().unwrap_or(0);
    let mut missing = Vec::new();
    if start > 1 {
        missing.extend(1..start);
    }
    for n in start..=end {
        if !set.contains(&n) {
            missing.push(n);
        }
    }
    missing
}

pub fn detect_numbering_gap_from_names(names: &[String]) -> Vec<u32> {
    let mut groups: HashMap<String, Vec<u32>> = HashMap::new();
    for name in names {
        let file_name = name.rsplit(['/', '\\']).next().unwrap_or(name);
        let lower = file_name.to_lowercase();
        let Some(stem) = lower.strip_suffix(".zip") else {
            continue;
        };
        let Some((group, number)) = stem.rsplit_once('-') else {
            continue;
        };
        if number.len() >= 3 && number.chars().all(|character| character.is_ascii_digit()) {
            if let Ok(number) = number.parse::<u32>() {
                groups.entry(group.to_string()).or_default().push(number);
            }
        }
    }
    let mut missing = Vec::new();
    for numbers in groups.values() {
        missing.extend(detect_numbering_gaps(numbers));
    }
    missing.sort_unstable();
    missing.dedup();
    missing
}

pub fn scan_paths(paths: &[String]) -> Result<ScanSourcesResult, String> {
    scan_paths_with_index(paths).map(|(result, _)| result)
}

pub fn scan_paths_with_index(paths: &[String]) -> Result<(ScanSourcesResult, TempIndex), String> {
    scan_paths_with_progress(paths, None, &mut |_| Ok(())).map(|(result, index, _)| (result, index))
}

pub fn scan_paths_with_progress(
    paths: &[String],
    cancelled: Option<&AtomicBool>,
    on_progress: &mut dyn FnMut(ScanProgressUpdate) -> Result<(), String>,
) -> Result<(ScanSourcesResult, TempIndex, ScanProgressUpdate), String> {
    check_scan_cancelled(cancelled)?;
    let expanded_paths = expand_source_paths(paths)?;
    let inspected_sources = inspect_sources(paths)?;
    let total_bytes = inspected_sources
        .archives
        .iter()
        .map(|source| source.size_bytes)
        .fold(0u64, u64::saturating_add);
    let (index, progress) =
        index_expanded_sources_with_progress(&expanded_paths, cancelled, total_bytes, on_progress)?;
    check_scan_cancelled(cancelled)?;
    let result = scan_result_from_index(&expanded_paths, &index)?;
    Ok((result, index, progress))
}

pub fn scan_result_from_index(
    paths: &[String],
    index: &TempIndex,
) -> Result<ScanSourcesResult, String> {
    let counts = index.counts()?;
    let mut warnings: Vec<String> = detect_numbering_gap_from_names(paths)
        .into_iter()
        .map(|part| format!("Missing Takeout archive part {part:03}."))
        .collect();
    if let Some(root) = index.google_photos_root()? {
        warnings.push(format!("Detected Google Photos folder: {root}"));
    }
    warnings.extend(index.skipped_entry_warning_messages()?);
    Ok(ScanSourcesResult {
        files: paths.to_vec(),
        counts,
        warnings,
    })
}

pub fn create_source(path: &Path) -> Result<Box<dyn Source>, String> {
    if path.is_dir() {
        return Ok(Box::new(FolderSource {
            root: path.to_path_buf(),
        }));
    }
    if !path.is_file() {
        return Err(format!("Takeout source does not exist: {}", path.display()));
    }
    match archive_type_for_path(path)? {
        ArchiveType::Zip => Ok(Box::new(ZipSource {
            path: path.to_path_buf(),
        })),
        ArchiveType::Tgz => Ok(Box::new(TgzSource {
            path: path.to_path_buf(),
        })),
        ArchiveType::Folder => Ok(Box::new(FolderSource {
            root: path.to_path_buf(),
        })),
    }
}

pub fn read_video_duration_seconds(
    archive_path: &str,
    entry_path: &str,
) -> Result<Option<f64>, String> {
    create_source(Path::new(archive_path))?.read_video_duration_seconds(entry_path)
}

pub mod matcher;

pub fn detect_google_photos_root(entries: &[String]) -> Option<String> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for entry in entries {
        let normalized = entry.replace('\\', "/");
        let path = Path::new(&normalized);
        for component in path.components() {
            if let Component::Normal(part) = component {
                let s = part.to_string_lossy();
                if ["Google Photos", "Google Fotos", "Google フォト"]
                    .iter()
                    .any(|candidate| s.eq_ignore_ascii_case(candidate))
                {
                    *counts.entry(s.to_string()).or_insert(0) += 1;
                }
            }
        }
    }
    counts
        .into_iter()
        .filter(|(_, count)| *count > 0)
        .max_by_key(|(_, count)| *count)
        .map(|(name, _)| name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use tempfile::tempdir;

    fn create_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = fs::File::create(path).unwrap();
        let mut archive = zip::ZipWriter::new(file);
        let options =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (entry_name, data) in entries {
            archive.start_file(*entry_name, options).unwrap();
            archive.write_all(data).unwrap();
        }
        archive.finish().unwrap();
    }

    fn create_tgz(path: &Path, entries: &[(&str, &[u8])]) {
        let file = fs::File::create(path).unwrap();
        let enc = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut archive = tar::Builder::new(enc);
        for (entry_name, data) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            archive
                .append_data(&mut header, *entry_name, *data)
                .unwrap();
        }
        archive.finish().unwrap();
    }

    #[test]
    fn counts_detect_live_pairs_and_embedded_motion_photos() {
        let index = TempIndex::new().unwrap();
        let insert = |path: &str, kind: &str, embedded_motion_video: bool| {
            index
                .conn
                .execute(
                    "INSERT INTO entries (
                        archive_path, path, entry_path, size, kind, album, trashed, archived,
                        embedded_motion_video, video_duration_seconds, metadata_json,
                        sidecar_archive_path, sidecar_path, sidecar_match
                    ) VALUES (?1, ?2, ?2, 1, ?3, NULL, 0, 0, ?4, NULL, NULL, NULL, NULL, NULL)",
                    rusqlite::params!["takeout.zip", path, kind, embedded_motion_video],
                )
                .unwrap();
        };
        insert("Photos/Album/IMG_0001.jpg", "photo", false);
        insert("Photos/Album/IMG_0001.mov", "video", false);
        insert("Photos/Album/IMG_0002.MP.jpg", "photo", true);

        let counts = index.counts().unwrap();

        assert_eq!(counts.photos, 2);
        assert_eq!(counts.videos, 1);
        assert_eq!(counts.live, 2);
    }

    #[test]
    fn zip_tgz_and_folder_sources_stream_entries() {
        let dir = tempdir().unwrap();
        let zip_path = dir.path().join("part-001.zip");
        let tgz_path = dir.path().join("part-002.tgz");
        let folder = dir.path().join("folder-root");
        fs::create_dir_all(&folder).unwrap();

        let zip_data = b"zip-test";
        let tgz_data = b"tgz-test";
        fs::write(folder.join("media.jpg"), b"jpg").unwrap();
        fs::write(folder.join("sidecar.json"), b"{}").unwrap();

        create_zip(
            &zip_path,
            &[("Takeout/Google Fotos/Photos from 2024/media.jpg", zip_data)],
        );
        create_tgz(
            &tgz_path,
            &[(
                "Takeout/Google Fotos/Photos from 2024/sidecar.json",
                tgz_data,
            )],
        );

        let zip_source = ZipSource {
            path: zip_path.clone(),
        };
        assert!(zip_source
            .list_entries()
            .unwrap()
            .iter()
            .any(|entry| entry.path.ends_with("media.jpg")));
        let mut zip_bytes = Vec::new();
        zip_source
            .copy_entry_to(
                "Takeout/Google Fotos/Photos from 2024/media.jpg",
                &mut zip_bytes,
            )
            .unwrap();
        assert_eq!(zip_bytes, zip_data);
        assert_eq!(
            zip_source
                .hash_entry_prefix("Takeout/Google Fotos/Photos from 2024/media.jpg")
                .unwrap(),
            blake3::hash(zip_data)
        );

        let tgz_source = TgzSource {
            path: tgz_path.clone(),
        };
        assert!(tgz_source
            .list_entries()
            .unwrap()
            .iter()
            .any(|entry| entry.path.ends_with("sidecar.json")));
        let mut tgz_bytes = Vec::new();
        tgz_source
            .copy_entry_to(
                "Takeout/Google Fotos/Photos from 2024/sidecar.json",
                &mut tgz_bytes,
            )
            .unwrap();
        assert_eq!(tgz_bytes, tgz_data);
        assert_eq!(
            tgz_source
                .hash_entry_prefix("Takeout/Google Fotos/Photos from 2024/sidecar.json")
                .unwrap(),
            blake3::hash(tgz_data)
        );

        let folder_source = FolderSource {
            root: folder.clone(),
        };
        let entries = folder_source.list_entries().unwrap();
        assert!(entries
            .iter()
            .any(|entry| entry.path.ends_with("media.jpg")));
        let mut folder_bytes = Vec::new();
        folder_source
            .copy_entry_to("media.jpg", &mut folder_bytes)
            .unwrap();
        assert_eq!(folder_bytes, b"jpg");
        assert_eq!(
            folder_source.hash_entry_prefix("media.jpg").unwrap(),
            blake3::hash(b"jpg")
        );
    }

    #[test]
    fn cancelled_source_visit_stops_before_processing_entries() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("photo.jpg"), b"image").unwrap();
        let source = FolderSource {
            root: dir.path().to_path_buf(),
        };
        let cancelled = AtomicBool::new(true);
        let mut visited = 0;

        let result = source.visit_entries(
            &mut |_, _| {
                visited += 1;
                Ok(())
            },
            Some(&cancelled),
            &mut |_, _| Ok(()),
        );

        assert_eq!(result, Err(SCAN_CANCELLED_ERROR.to_string()));
        assert_eq!(visited, 0);
    }

    #[test]
    fn scan_progress_is_throttled() {
        let mut last_progress = Instant::now() - Duration::from_secs(1);
        let mut emitted = 0;
        let mut callback = |_| {
            emitted += 1;
            Ok(())
        };
        let progress = ScanProgressUpdate::default();

        emit_scan_progress(&mut callback, &mut last_progress, &progress).unwrap();
        emit_scan_progress(&mut callback, &mut last_progress, &progress).unwrap();

        assert_eq!(emitted, 1);
    }

    #[test]
    fn numbering_gaps_and_google_photos_root_detected() {
        let files = vec![
            "takeout-20240612T101500Z-001.zip".to_string(),
            "takeout-20240612T101500Z-003.zip".to_string(),
            "Takeout/Google Fotos/Photos from 2024/IMG_001.jpg".to_string(),
        ];
        let gaps = detect_numbering_gap_from_names(&files);
        assert!(gaps.contains(&2));
        assert_eq!(
            detect_google_photos_root(&files).as_deref(),
            Some("Google Fotos")
        );
    }

    #[test]
    fn classifies_additional_photo_video_and_other_extensions() {
        for path in [
            "photo.gif",
            "photo.bmp",
            "photo.avif",
            "photo.jfif",
            "photo.heif",
            "photo.cr3",
            "photo.nrw",
            "photo.raf",
            "photo.pef",
            "photo.sr2",
        ] {
            assert_eq!(entry_type_for_path(path), EntryType::Photo, "{path}");
        }
        for path in [
            "video.webm",
            "video.mpg",
            "video.mpeg",
            "video.wmv",
            "video.flv",
            "video.3g2",
            "video.mp",
        ] {
            assert_eq!(entry_type_for_path(path), EntryType::Video, "{path}");
        }
        assert_eq!(entry_type_for_path("notes.unknown"), EntryType::Other);
    }

    #[test]
    fn add_sources_reports_archive_sizes_entry_counts_and_part_gaps() {
        let dir = tempdir().unwrap();
        let first = dir.path().join("takeout-20240612T101500Z-001.zip");
        let second = dir.path().join("takeout-20240612T101500Z-002.zip");
        let fourth = dir.path().join("takeout-20240612T101500Z-004.zip");
        create_zip(&first, &[("one.jpg", b"photo"), ("readme.txt", b"readme")]);
        create_zip(&second, &[("two.mp4", b"video")]);
        create_zip(&fourth, &[("three.json", b"{}")]);

        let result = inspect_sources(&[first.to_string_lossy().to_string()]).unwrap();

        assert_eq!(result.archives.len(), 3);
        assert_eq!(result.archives[0].archive_type, ArchiveType::Zip);
        assert_eq!(
            result.archives[0].size_bytes,
            fs::metadata(&first).unwrap().len()
        );
        assert_eq!(result.archives[0].entry_count, 2);
        assert!(result.archives[0].entry_count_known);
        assert_eq!(result.archives[1].entry_count, 1);
        assert!(result.archives[1].entry_count_known);
        assert_eq!(result.warnings, vec!["Missing Takeout archive part 003."]);
    }

    #[test]
    fn zip_inspection_counts_without_opening_entries() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("takeout.zip");
        create_zip(&path, &[("../unsafe.jpg", b"photo")]);

        let result = inspect_sources(&[path.to_string_lossy().to_string()]).unwrap();

        assert_eq!(result.archives[0].entry_count, 1);
        assert!(result.archives[0].entry_count_known);
    }

    #[test]
    fn folder_inspection_counts_files_and_sums_file_sizes() {
        let dir = tempdir().unwrap();
        let folder = dir.path().join("takeout");
        fs::create_dir_all(folder.join("nested")).unwrap();
        fs::write(folder.join("photo.jpg"), b"photo").unwrap();
        fs::write(folder.join("nested").join("video.mp4"), b"video-data").unwrap();

        let result = inspect_sources(&[folder.to_string_lossy().to_string()]).unwrap();

        assert_eq!(result.archives[0].archive_type, ArchiveType::Folder);
        assert_eq!(result.archives[0].entry_count, 2);
        assert!(result.archives[0].entry_count_known);
        assert_eq!(result.archives[0].size_bytes, 15);
    }

    #[test]
    fn index_pairs_json_across_parts_and_classifies_takeout_paths() {
        let dir = tempdir().unwrap();
        let first = dir.path().join("takeout-20240612T101500Z-001.zip");
        let second = dir.path().join("takeout-20240612T101500Z-002.zip");
        let long_stem = format!("IMG_{}", "a".repeat(52));
        let truncated = long_stem.chars().take(46).collect::<String>();
        let media_entries = vec![
            (
                format!("Takeout/Google Fotos/Photos from 2024/{long_stem}.jpg"),
                b"photo".as_slice(),
            ),
            (
                "Takeout/Google Fotos/Album Ñ/猫.jpg".to_string(),
                b"cat".as_slice(),
            ),
            (
                "Takeout/Google Fotos/Archive/old.jpg".to_string(),
                b"old".as_slice(),
            ),
            (
                "Takeout/Google Fotos/Trash/deleted.jpg".to_string(),
                b"trash".as_slice(),
            ),
            (
                "Takeout/Google Fotos/Photos from 2023/PXL_1.jpg".to_string(),
                b"legacy".as_slice(),
            ),
            (
                "Takeout/Google Fotos/Photos from 2023/IMG_3(1).jpg".to_string(),
                b"duplicate".as_slice(),
            ),
            (
                "Takeout/Google Fotos/Photos from 2023/IMG_4-edited.jpg".to_string(),
                b"edited".as_slice(),
            ),
        ];
        let media_refs: Vec<(&str, &[u8])> = media_entries
            .iter()
            .map(|(path, bytes)| (path.as_str(), *bytes))
            .collect();
        create_zip(&first, &media_refs);

        let sidecars = vec![
            (
                format!("Takeout/Google Fotos/Photos from 2024/{truncated}.json"),
                br#"{"title":"long"}"#.as_slice(),
            ),
            (
                "Takeout/Google Fotos/Album Ñ/猫.jpg.supplemental-metadata.json".to_string(),
                br#"{
                    "title":"cat",
                    "description":"A Takeout caption",
                    "photoTakenTime":{"timestamp":"1700000000"},
                    "geoData":{"latitude":0,"longitude":0},
                    "geoDataExif":{"latitude":47.6,"longitude":-122.3,"altitude":12.5},
                    "people":[{"name":"Ada"}],
                    "favorited":true,
                    "archived":true,
                    "trashed":false,
                    "url":"https://photos.example/cat"
                }"#
                .as_slice(),
            ),
            (
                "Takeout/Google Fotos/Archive/old.jpg.json".to_string(),
                br#"{"title":"old"}"#.as_slice(),
            ),
            (
                "Takeout/Google Fotos/Trash/deleted.jpg.json".to_string(),
                br#"{"title":"deleted"}"#.as_slice(),
            ),
            (
                "Takeout/Google Fotos/Photos from 2023/PXL_1.json".to_string(),
                br#"{"title":"PXL_1.jpg"}"#.as_slice(),
            ),
            (
                "Takeout/Google Fotos/Photos from 2023/IMG_3.jpg(1).json".to_string(),
                br#"{"title":"IMG 3"}"#.as_slice(),
            ),
            (
                "Takeout/Google Fotos/Photos from 2023/IMG_4.jpg.json".to_string(),
                br#"{"title":"IMG 4"}"#.as_slice(),
            ),
        ];
        let sidecar_refs: Vec<(&str, &[u8])> = sidecars
            .iter()
            .map(|(path, bytes)| (path.as_str(), *bytes))
            .collect();
        create_zip(&second, &sidecar_refs);

        let index = index_sources(&[
            first.to_string_lossy().to_string(),
            second.to_string_lossy().to_string(),
        ])
        .unwrap();
        let entries = index.entries().unwrap();
        let preview_entries = index.preview_entries(2).unwrap();
        let media: Vec<_> = entries
            .iter()
            .filter(|entry| entry.entry_type == EntryType::Photo)
            .collect();

        assert_eq!(media.len(), media_entries.len());
        assert_eq!(preview_entries.len(), 2);
        assert!(preview_entries
            .iter()
            .all(|entry| matches!(entry.entry_type, EntryType::Photo | EntryType::Video)));
        assert!(media
            .iter()
            .all(|entry| entry.sidecar_archive_path.as_deref()
                == Some(second.to_string_lossy().as_ref())));
        assert!(media.iter().any(
            |entry| entry.album.as_deref() == Some("Album Ñ") && entry.path.ends_with("猫.jpg")
        ));
        let cat = media
            .iter()
            .find(|entry| entry.path.ends_with("猫.jpg"))
            .and_then(|entry| entry.metadata.as_ref())
            .expect("indexed sidecar metadata");
        assert_eq!(cat.photo_taken_time, Some(1_700_000_000));
        assert_eq!(cat.latitude, Some(47.6));
        assert_eq!(cat.description.as_deref(), Some("A Takeout caption"));
        assert_eq!(cat.people, vec!["Ada"]);
        assert!(cat.favorited && cat.archived);
        assert!(media
            .iter()
            .find(|entry| entry.path.ends_with("猫.jpg"))
            .is_some_and(|entry| entry.archived));
        assert_eq!(cat.url.as_deref(), Some("https://photos.example/cat"));
        assert!(media
            .iter()
            .any(|entry| { entry.album.is_none() && entry.path.contains("Photos from 2024") }));
        assert!(media
            .iter()
            .any(|entry| entry.archived && entry.path.ends_with("old.jpg")));
        assert!(media
            .iter()
            .any(|entry| entry.trashed && entry.path.ends_with("deleted.jpg")));
        assert!(media.iter().any(|entry| {
            entry.path.ends_with(&format!("{long_stem}.jpg"))
                && entry.sidecar_match.as_deref() == Some("Truncated")
        }));
        assert!(media.iter().any(|entry| {
            entry.path.ends_with("猫.jpg") && entry.sidecar_match.as_deref() == Some("Supplemental")
        }));
        assert!(media.iter().any(|entry| {
            entry.path.ends_with("PXL_1.jpg") && entry.sidecar_match.as_deref() == Some("Title")
        }));
        assert!(media.iter().any(|entry| {
            entry.path.ends_with("IMG_3(1).jpg") && entry.sidecar_match.as_deref() == Some("Exact")
        }));
        assert!(media.iter().any(|entry| {
            entry.path.ends_with("IMG_4-edited.jpg")
                && entry.sidecar_match.as_deref() == Some("EditedSuffixStripped")
        }));
    }

    #[test]
    fn indexing_skips_bad_json_and_unsafe_paths_with_capped_warnings() {
        let dir = tempdir().unwrap();
        let archive_path = dir.path().join("takeout.zip");
        let mut contents = vec![("../unsafe.json".to_string(), b"{}".to_vec())];
        contents.extend(
            (0..105).map(|index| (format!("Album/bad-{index:03}.json"), b"{invalid".to_vec())),
        );
        contents.extend([
            ("Album/metadata.json".to_string(), b"{invalid".to_vec()),
            ("Album/photo.jpg".to_string(), b"photo".to_vec()),
            (
                "Album/photo.jpg.json".to_string(),
                br#"{"title":"photo.jpg"}"#.to_vec(),
            ),
        ]);
        let entries = contents
            .iter()
            .map(|(path, bytes)| (path.as_str(), bytes.as_slice()))
            .collect::<Vec<_>>();
        create_zip(&archive_path, &entries);
        let paths = vec![archive_path.to_string_lossy().to_string()];

        let (result, index) = scan_paths_with_index(&paths).unwrap();
        let indexed_entries = index.entries().unwrap();
        let album_metadata = indexed_entries
            .iter()
            .find(|entry| entry.path.eq_ignore_ascii_case("Album/metadata.json"))
            .expect("album metadata entry");

        assert_eq!(result.counts.photos, 1);
        assert_eq!(result.warnings.len(), 101);
        assert!(result
            .warnings
            .iter()
            .any(|warning| { warning.contains("Skipped 6 additional entries") }));
        assert!(result
            .warnings
            .iter()
            .any(|warning| warning.contains("Album/bad-")));
        assert!(result
            .warnings
            .iter()
            .any(|warning| warning.contains("../unsafe.json")));
        assert_eq!(album_metadata.entry_type, EntryType::AlbumMetadata);
        assert!(album_metadata.metadata.is_none());

        index.remove_archive(&paths[0]).unwrap();
        let after_remove = scan_result_from_index(&[], &index).unwrap();
        assert!(after_remove.warnings.is_empty());
        assert_eq!(after_remove.counts.photos, 0);
    }

    #[test]
    fn explicit_source_index_does_not_reexpand_removed_archive_parts() {
        let dir = tempdir().unwrap();
        let first = dir.path().join("takeout-20240612T101500Z-001.zip");
        let second = dir.path().join("takeout-20240612T101500Z-002.zip");
        create_zip(&first, &[("Photos/first.jpg", b"first")]);
        create_zip(&second, &[("Photos/second.jpg", b"second")]);

        let index = index_explicit_sources(&[second.to_string_lossy().to_string()]).unwrap();
        let entries = index.media_entries().unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].archive_path,
            second.to_string_lossy().to_string()
        );
    }

    #[test]
    fn archive_index_probes_motion_photo_markers_and_defers_video_duration() {
        fn atom(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
            let mut bytes = Vec::new();
            bytes.extend_from_slice(&((payload.len() + 8) as u32).to_be_bytes());
            bytes.extend_from_slice(kind);
            bytes.extend_from_slice(payload);
            bytes
        }

        let dir = tempdir().unwrap();
        let archive_path = dir.path().join("live-photos.zip");
        let mut movie_header = vec![0, 0, 0, 0];
        movie_header.extend_from_slice(&0_u32.to_be_bytes());
        movie_header.extend_from_slice(&0_u32.to_be_bytes());
        movie_header.extend_from_slice(&1_000_u32.to_be_bytes());
        movie_header.extend_from_slice(&2_500_u32.to_be_bytes());
        let movie = atom(b"moov", &atom(b"mvhd", &movie_header));
        let motion_photo = b"<x:xmpmeta>GCamera:MicroVideo</x:xmpmeta>ftypisom";
        create_zip(
            &archive_path,
            &[
                ("Photos/IMG_1.MP.jpg", motion_photo),
                ("Photos/IMG_2.mov", &movie),
            ],
        );

        let index = index_sources(&[archive_path.to_string_lossy().to_string()]).unwrap();
        let entries = index.media_entries().unwrap();
        let motion = entries
            .iter()
            .find(|entry| entry.path.ends_with("IMG_1.MP.jpg"))
            .unwrap();
        assert!(motion.embedded_motion_video);
        let video = entries
            .iter()
            .find(|entry| entry.path.ends_with("IMG_2.mov"))
            .unwrap();
        assert_eq!(video.video_duration_seconds, None);
        assert_eq!(
            read_video_duration_seconds(&archive_path.to_string_lossy(), &video.entry_path)
                .unwrap(),
            Some(2.5)
        );
    }

    #[test]
    fn indexing_reads_json_and_only_the_motion_photo_prefix() {
        let directory = tempdir().unwrap();
        let archive_path = directory.path().join("bounded-index.zip");
        let motion_photo = {
            let mut bytes = vec![b'x'; 300 * 1024];
            let marker = b"GCamera:MicroVideo XMP marker!";
            bytes[..marker.len()].copy_from_slice(marker);
            bytes
        };
        create_zip(
            &archive_path,
            &[
                ("Photos/ordinary.jpg", &vec![b'p'; 300 * 1024]),
                ("Photos/ordinary.mov", &vec![b'v'; 300 * 1024]),
                ("Photos/IMG_1.MP.jpg", &motion_photo),
                ("metadata.json", b"{}"),
            ],
        );

        let mut progress = ScanProgressUpdate::default();
        let mut last_progress = Instant::now();
        let mut on_progress = |_| Ok(());
        let index = TempIndex::new().unwrap();
        index_archive_into(
            &index,
            &archive_path.to_string_lossy(),
            1,
            1,
            0,
            &mut progress,
            &mut last_progress,
            None,
            &mut on_progress,
        )
        .unwrap();

        assert_eq!(progress.bytes_read, 256 * 1024);
        assert!(index
            .media_entries()
            .unwrap()
            .iter()
            .any(|entry| entry.embedded_motion_video));
    }

    #[test]
    fn duration_probes_tail_only_for_stored_zip_entries() {
        fn atom(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
            let mut bytes = Vec::new();
            bytes.extend_from_slice(&((payload.len() + 8) as u32).to_be_bytes());
            bytes.extend_from_slice(kind);
            bytes.extend_from_slice(payload);
            bytes
        }

        let directory = tempdir().unwrap();
        let mut movie_header = vec![0, 0, 0, 0];
        movie_header.extend_from_slice(&0_u32.to_be_bytes());
        movie_header.extend_from_slice(&0_u32.to_be_bytes());
        movie_header.extend_from_slice(&1_000_u32.to_be_bytes());
        movie_header.extend_from_slice(&2_500_u32.to_be_bytes());
        let moov = atom(b"moov", &atom(b"mvhd", &movie_header));
        let mut video = vec![b'x'; matcher::VIDEO_DURATION_PROBE_BYTES + 128];
        video.extend_from_slice(&moov);

        let stored_path = directory.path().join("stored.zip");
        create_zip(&stored_path, &[("Photos/IMG_1.mov", &video)]);
        let deflated_path = directory.path().join("deflated.zip");
        let file = fs::File::create(&deflated_path).unwrap();
        let mut archive = zip::ZipWriter::new(file);
        archive
            .start_file(
                "Photos/IMG_1.mov",
                zip::write::FileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated),
            )
            .unwrap();
        archive.write_all(&video).unwrap();
        archive.finish().unwrap();
        let tgz_path = directory.path().join("video.tgz");
        create_tgz(&tgz_path, &[("Photos/IMG_1.mov", &video)]);

        assert_eq!(
            ZipSource { path: stored_path }
                .read_video_duration_seconds("Photos/IMG_1.mov")
                .unwrap(),
            Some(2.5)
        );
        assert_eq!(
            ZipSource {
                path: deflated_path
            }
            .read_video_duration_seconds("Photos/IMG_1.mov")
            .unwrap(),
            None
        );
        assert_eq!(
            TgzSource { path: tgz_path }
                .read_video_duration_seconds("Photos/IMG_1.mov")
                .unwrap(),
            None
        );
    }

    #[test]
    fn archive_index_inserts_commit_every_five_thousand_rows() {
        let index = TempIndex::new().unwrap();
        let db_path = index._temp_dir.path().join("takeback-index.sqlite");
        let observer = Connection::open(db_path).unwrap();
        let mut batch = IndexInsertBatch::new(&index.conn).unwrap();

        for number in 0..5_001 {
            let path = format!("Photos/{number}.jpg");
            let entry = SourceEntry {
                entry_path: path.clone(),
                path,
                size: 0,
                entry_type: EntryType::Photo,
            };
            batch
                .insert("archive.zip", &entry, None, false, false, false, None)
                .unwrap();
        }

        let committed_rows: u32 = observer
            .query_row("SELECT COUNT(*) FROM entries", [], |row| row.get(0))
            .unwrap();
        assert_eq!(committed_rows, 5_000);
        batch.finish().unwrap();
        let final_rows: u32 = observer
            .query_row("SELECT COUNT(*) FROM entries", [], |row| row.get(0))
            .unwrap();
        assert_eq!(final_rows, 5_001);
    }

    #[test]
    fn archive_entry_paths_reject_traversal() {
        assert!(normalize_archive_entry_path("../outside.jpg").is_err());
        assert!(normalize_archive_entry_path("Takeout/../../outside.jpg").is_err());
        assert_eq!(
            normalize_archive_entry_path("Takeout\\Photos\\猫.jpg").unwrap(),
            "Takeout/Photos/猫.jpg"
        );
    }

    #[test]
    fn tar_gz_extension_is_accepted() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("takeout.tar.gz");
        create_tgz(
            &path,
            &[
                ("Takeout/Google Fotos/photo.jpg", b"photo"),
                (
                    "Takeout/Google Fotos/photo.jpg.json",
                    br#"{"title":"photo"}"#,
                ),
            ],
        );

        let summary = inspect_sources(&[path.to_string_lossy().to_string()]).unwrap();
        let index = index_sources(&[path.to_string_lossy().to_string()]).unwrap();
        let entries = index.entries().unwrap();

        assert_eq!(summary.archives.len(), 1);
        assert_eq!(summary.archives[0].archive_type, ArchiveType::Tgz);
        assert_eq!(summary.archives[0].entry_count, 0);
        assert!(!summary.archives[0].entry_count_known);
        assert_eq!(
            summary.archives[0].size_bytes,
            fs::metadata(&path).unwrap().len()
        );
        assert!(entries.iter().any(|entry| {
            entry.path.ends_with("photo.jpg")
                && entry.sidecar_path.as_deref() == Some("Takeout/Google Fotos/photo.jpg.json")
        }));
    }
}
