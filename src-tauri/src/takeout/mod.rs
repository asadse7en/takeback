use std::collections::HashMap;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};

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

pub trait Source: Send + Sync {
    fn visit_entries(
        &self,
        visitor: &mut dyn FnMut(&SourceEntry, &mut dyn Read) -> Result<(), String>,
    ) -> Result<(), String>;

    fn copy_entry_to(&self, entry_path: &str, output: &mut dyn Write) -> Result<u64, String>;

    fn list_entries(&self) -> Result<Vec<SourceEntry>, String> {
        let mut entries = Vec::new();
        self.visit_entries(&mut |entry, _| {
            entries.push(entry.clone());
            Ok(())
        })?;
        Ok(entries)
    }
}

#[derive(Debug, Clone)]
pub struct ZipSource {
    pub path: PathBuf,
}

impl Source for ZipSource {
    fn visit_entries(
        &self,
        visitor: &mut dyn FnMut(&SourceEntry, &mut dyn Read) -> Result<(), String>,
    ) -> Result<(), String> {
        let file = fs::File::open(&self.path)
            .map_err(|e| format!("open zip {}: {e}", self.path.display()))?;
        let mut archive =
            ZipArchive::new(file).map_err(|e| format!("read zip {}: {e}", self.path.display()))?;

        for idx in 0..archive.len() {
            let mut entry = archive
                .by_index(idx)
                .map_err(|e| format!("read zip entry {}: {e}", self.path.display()))?;
            if entry.is_dir() {
                continue;
            }
            let entry_path = entry.name().to_string();
            let path = normalize_archive_entry_path(&entry_path)?;
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
}

#[derive(Debug, Clone)]
pub struct TgzSource {
    pub path: PathBuf,
}

impl Source for TgzSource {
    fn visit_entries(
        &self,
        visitor: &mut dyn FnMut(&SourceEntry, &mut dyn Read) -> Result<(), String>,
    ) -> Result<(), String> {
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
            if !file.header().entry_type().is_file() {
                continue;
            }
            let entry_path = std::str::from_utf8(file.path_bytes().as_ref())
                .map_err(|e| format!("tgz entry path is not valid UTF-8: {e}"))?
                .to_string();
            let path = normalize_archive_entry_path(&entry_path)?;
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
}

#[derive(Debug, Clone)]
pub struct FolderSource {
    pub root: PathBuf,
}

impl Source for FolderSource {
    fn visit_entries(
        &self,
        visitor: &mut dyn FnMut(&SourceEntry, &mut dyn Read) -> Result<(), String>,
    ) -> Result<(), String> {
        for entry in WalkDir::new(&self.root).into_iter() {
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
            let normalized = normalize_archive_entry_path(&entry_path)?;
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
}

#[derive(Debug)]
pub struct TempIndex {
    conn: Connection,
    _temp_dir: tempfile::TempDir,
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
            _temp_dir: temp_dir,
        })
    }

    fn add_entry(
        &self,
        archive_path: &str,
        entry: &SourceEntry,
        album: Option<&str>,
        trashed: bool,
        archived: bool,
        embedded_motion_video: bool,
        video_duration_seconds: Option<f64>,
        metadata: Option<&crate::metadata::TakeoutMetadata>,
    ) -> Result<(), String> {
        let size = i64::try_from(entry.size)
            .map_err(|_| format!("entry is too large to index: {}", entry.path))?;
        self.conn
            .execute(
                "INSERT OR REPLACE INTO entries (
                archive_path, path, entry_path, size, kind, album, trashed, archived,
                embedded_motion_video, video_duration_seconds, metadata_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                (
                    archive_path,
                    &entry.path,
                    &entry.entry_path,
                    size,
                    entry_type_name(entry.entry_type),
                    album,
                    trashed,
                    archived,
                    embedded_motion_video,
                    video_duration_seconds,
                    metadata
                        .map(serde_json::to_string)
                        .transpose()
                        .map_err(|error| {
                            format!("serialize JSON sidecar {}: {error}", entry.path)
                        })?,
                ),
            )
            .map_err(|e| format!("index entry {}: {e}", entry.path))?;
        Ok(())
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

    pub fn entries(&self) -> Result<Vec<IndexedEntry>, String> {
        self.entries_limited(None, false)
    }

    pub fn media_entries(&self) -> Result<Vec<IndexedEntry>, String> {
        self.entries_limited(None, true)
    }

    pub fn preview_entries(&self, limit: usize) -> Result<Vec<IndexedEntry>, String> {
        self.entries_limited(Some(limit), true)
    }

    fn entries_limited(
        &self,
        limit: Option<usize>,
        media_only: bool,
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
                 WHERE (?2 = 0 OR entry.kind IN ('photo', 'video'))
                 ORDER BY entry.archive_path, entry.path
                 LIMIT COALESCE(?1, -1)",
            )
            .map_err(|e| format!("prepare archive index read: {e}"))?;
        let rows = statement
            .query_map(
                [
                    limit.map(|value| value.min(i64::MAX as usize) as i64),
                    Some(i64::from(media_only)),
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
        Ok(Counts {
            photos: stats.0,
            videos: stats.1,
            live: 0,
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

pub fn index_sources(paths: &[String]) -> Result<TempIndex, String> {
    let expanded_paths = expand_source_paths(paths)?;
    index_expanded_sources(&expanded_paths)
}

pub fn index_explicit_sources(paths: &[String]) -> Result<TempIndex, String> {
    index_expanded_sources(paths)
}

fn index_expanded_sources(paths: &[String]) -> Result<TempIndex, String> {
    let index = TempIndex::new()?;
    let mut media_refs = Vec::new();
    let mut media_paths = Vec::new();
    let mut sidecars = Vec::new();

    for path in paths.iter().filter(|path| !path.trim().is_empty()) {
        let source_path = Path::new(path);
        let source = create_source(source_path)?;
        let archive_path = source_path.to_string_lossy().to_string();
        source.visit_entries(&mut |entry, reader| {
            let metadata = if entry.entry_type == EntryType::JsonSidecar {
                Some(crate::metadata::TakeoutMetadata::from_reader(&mut *reader)?)
            } else {
                None
            };
            let embedded_motion_video = if matcher::is_android_motion_photo_path(&entry.path) {
                matcher::verify_android_motion_photo(&mut *reader)?
            } else {
                false
            };
            let video_duration_seconds = if entry.entry_type == EntryType::Video {
                matcher::read_video_duration_seconds(&mut *reader)?
            } else {
                None
            };
            let (album, trashed, archived) = classify_entry_path(&entry.path);
            index.add_entry(
                &archive_path,
                entry,
                album.as_deref(),
                trashed,
                archived,
                embedded_motion_video,
                video_duration_seconds,
                metadata.as_ref(),
            )?;
            match entry.entry_type {
                EntryType::Photo | EntryType::Video => {
                    media_refs.push((archive_path.clone(), entry.path.clone()));
                    media_paths.push(entry.path.clone());
                }
                EntryType::JsonSidecar => {
                    sidecars.push(matcher::SidecarCandidate {
                        archive_path: archive_path.clone(),
                        path: entry.path.clone(),
                        title: metadata.and_then(|fields| fields.title),
                    });
                }
                EntryType::Other => {}
            }
            Ok(())
        })?;
    }

    let matches = matcher::match_sidecars(&media_paths, &sidecars);
    for ((archive_path, media_path), matched) in media_refs.iter().zip(matches) {
        if let Some(matched) = matched {
            index.set_sidecar(
                archive_path,
                media_path,
                &matched.archive_path,
                &matched.path,
                &format!("{:?}", matched.reason),
            )?;
        }
    }

    Ok(index)
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
    ) {
        EntryType::Photo
    } else if matches!(
        extension.as_str(),
        "mp4" | "mov" | "m4v" | "avi" | "mkv" | "3gp" | "mts" | "m2ts"
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
        EntryType::Other => "other",
    }
}

fn parse_entry_type(value: &str) -> Result<EntryType, String> {
    match value {
        "photo" => Ok(EntryType::Photo),
        "video" => Ok(EntryType::Video),
        "jsonSidecar" => Ok(EntryType::JsonSidecar),
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

fn expand_source_paths(paths: &[String]) -> Result<Vec<String>, String> {
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
        let source = create_source(path)?;
        let mut entry_count = 0u64;
        let mut folder_size = 0u64;
        source.visit_entries(&mut |entry, _| {
            entry_count += 1;
            if archive_type == ArchiveType::Folder {
                folder_size = folder_size
                    .checked_add(entry.size)
                    .ok_or_else(|| "folder size is too large to calculate".to_string())?;
            }
            Ok(())
        })?;
        let size_bytes = if archive_type == ArchiveType::Folder {
            folder_size
        } else {
            fs::metadata(path)
                .map_err(|error| format!("read source metadata {}: {error}", path.display()))?
                .len()
        };
        result.archives.push(SourceRecord {
            id: path.to_string_lossy().to_string(),
            path: path.to_string_lossy().to_string(),
            archive_type,
            size_bytes,
            entry_count,
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
    let expanded_paths = expand_source_paths(paths)?;
    let index = index_expanded_sources(&expanded_paths)?;
    let counts = index.counts()?;
    let mut warnings: Vec<String> = detect_numbering_gap_from_names(&expanded_paths)
        .into_iter()
        .map(|part| format!("Missing Takeout archive part {part:03}."))
        .collect();
    if let Some(root) = index.google_photos_root()? {
        warnings.push(format!("Detected Google Photos folder: {root}"));
    }
    Ok((
        ScanSourcesResult {
            files: expanded_paths,
            counts,
            warnings,
        },
        index,
    ))
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
        assert_eq!(result.archives[1].entry_count, 1);
        assert_eq!(result.warnings, vec!["Missing Takeout archive part 003."]);
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
                br#"{"title":"PXL 1"}"#.as_slice(),
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
            entry.path.ends_with("PXL_1.jpg") && entry.sidecar_match.as_deref() == Some("Exact")
        }));
        assert!(media.iter().any(|entry| {
            entry.path.ends_with("IMG_3(1).jpg")
                && entry.sidecar_match.as_deref() == Some("NumberedDuplicate")
        }));
        assert!(media.iter().any(|entry| {
            entry.path.ends_with("IMG_4-edited.jpg")
                && entry.sidecar_match.as_deref() == Some("EditedSuffixStripped")
        }));
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
    fn archive_index_persists_motion_photo_verification_and_video_duration() {
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
        assert_eq!(video.video_duration_seconds, Some(2.5));
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
        assert_eq!(summary.archives[0].entry_count, 2);
        assert!(entries.iter().any(|entry| {
            entry.path.ends_with("photo.jpg")
                && entry.sidecar_path.as_deref() == Some("Takeout/Google Fotos/photo.jpg.json")
        }));
    }
}
