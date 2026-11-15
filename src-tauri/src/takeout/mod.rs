use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use flate2::read::GzDecoder;
use rayon::prelude::*;
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
    fn list_entries(&self) -> Result<Vec<String>, String>;
    fn stream_entry(&self, entry_name: &str) -> Result<Vec<u8>, String>;
}

#[derive(Debug, Clone)]
pub struct ZipSource {
    pub path: PathBuf,
}

impl Source for ZipSource {
    fn list_entries(&self) -> Result<Vec<String>, String> {
        let file = fs::File::open(&self.path).map_err(|e| format!("open zip {}: {e}", self.path.display()))?;
        let mut archive = ZipArchive::new(file).map_err(|e| format!("read zip {}: {e}", self.path.display()))?;

        let mut names = Vec::new();
        for idx in 0..archive.len() {
            let entry = archive.by_index(idx).map_err(|e| format!("read zip entry {}: {e}", self.path.display()))?;
            let name = entry.name().trim_end_matches('/').to_string();
            if !name.is_empty() && !entry.is_dir() {
                names.push(name);
            }
        }
        Ok(names)
    }

    fn stream_entry(&self, entry_name: &str) -> Result<Vec<u8>, String> {
        let file = fs::File::open(&self.path).map_err(|e| format!("open zip {}: {e}", self.path.display()))?;
        let mut archive = ZipArchive::new(file).map_err(|e| format!("read zip {}: {e}", self.path.display()))?;

        let mut entry = archive
            .by_name(entry_name)
            .map_err(|_| format!("zip entry not found: {entry_name}"))?;
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).map_err(|e| format!("read zip entry {entry_name}: {e}"))?;
        Ok(bytes)
    }
}

#[derive(Debug, Clone)]
pub struct TgzSource {
    pub path: PathBuf,
}

impl Source for TgzSource {
    fn list_entries(&self) -> Result<Vec<String>, String> {
        let file = fs::File::open(&self.path).map_err(|e| format!("open tgz {}: {e}", self.path.display()))?;
        let decoder = GzDecoder::new(file);
        let mut archive = Archive::new(decoder);
        let mut names = Vec::new();

        for entry in archive.entries().map_err(|e| format!("read tgz {}: {e}", self.path.display()))? {
            let file = entry.map_err(|e| format!("read tgz entry {}: {e}", self.path.display()))?;
            let path = file.path().map_err(|e| format!("tgz path {}: {e}", self.path.display()))?
                .to_string_lossy()
                .trim_end_matches('/')
                .to_string();
            if !path.is_empty() {
                names.push(path);
            }
        }
        Ok(names)
    }

    fn stream_entry(&self, entry_name: &str) -> Result<Vec<u8>, String> {
        let file = fs::File::open(&self.path).map_err(|e| format!("open tgz {}: {e}", self.path.display()))?;
        let decoder = GzDecoder::new(file);
        let mut archive = Archive::new(decoder);

        for entry in archive.entries().map_err(|e| format!("read tgz {}: {e}", self.path.display()))? {
            let mut file = entry.map_err(|e| format!("read tgz entry {}: {e}", self.path.display()))?;
            let name = file.path().map_err(|e| format!("tgz path {}: {e}", self.path.display()))?
                .to_string_lossy()
                .to_string();
            if name == entry_name {
                let mut bytes = Vec::new();
                file.read_to_end(&mut bytes)
                    .map_err(|e| format!("read tgz entry {entry_name}: {e}"))?;
                return Ok(bytes);
            }
        }
        Err(format!("tgz entry not found: {entry_name}"))
    }
}

#[derive(Debug, Clone)]
pub struct FolderSource {
    pub root: PathBuf,
}

impl Source for FolderSource {
    fn list_entries(&self) -> Result<Vec<String>, String> {
        let mut names = Vec::new();
        for entry in WalkDir::new(&self.root).into_iter().filter_map(Result::ok) {
            let path = entry.path();
            if path.is_file() {
                let rel = path
                    .strip_prefix(&self.root)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .replace('\\', "/");
                if !rel.is_empty() {
                    names.push(rel);
                }
            }
        }
        Ok(names)
    }

    fn stream_entry(&self, entry_name: &str) -> Result<Vec<u8>, String> {
        let path = self.root.join(entry_name.replace('/', "/"));
        let mut file = fs::File::open(&path).map_err(|e| format!("open file {}: {e}", path.display()))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|e| format!("read file {}: {e}", path.display()))?;
        Ok(bytes)
    }
}

#[derive(Debug)]
pub struct TempIndex {
    conn: Connection,
}

impl TempIndex {
    pub fn new() -> Result<Self, String> {
        let conn = Connection::open_in_memory().map_err(|e| format!("open temp sqlite index: {e}"))?;
        conn.execute(
            "CREATE TABLE media (path TEXT PRIMARY KEY, kind TEXT, size INTEGER)",
            (),
        )
        .map_err(|e| format!("create media table: {e}"))?;
        conn.execute(
            "CREATE TABLE sidecars (path TEXT PRIMARY KEY, media_path TEXT, kind TEXT)",
            (),
        )
        .map_err(|e| format!("create sidecars table: {e}"))?;
        conn.execute(
            "CREATE TABLE album_membership (album TEXT, media_path TEXT)",
            (),
        )
        .map_err(|e| format!("create album-membership table: {e}"))?;
        Ok(Self { conn })
    }

    pub fn add_media(&self, path: &str, kind: &str, size: u64) -> Result<(), String> {
        self.conn.execute(
            "INSERT OR REPLACE INTO media (path, kind, size) VALUES (?1, ?2, ?3)",
            (path, kind, size as i64),
        )
        .map_err(|e| format!("insert media {path}: {e}"))?;
        Ok(())
    }

    pub fn add_sidecar(&self, path: &str, media_path: &str, kind: &str) -> Result<(), String> {
        self.conn.execute(
            "INSERT OR REPLACE INTO sidecars (path, media_path, kind) VALUES (?1, ?2, ?3)",
            (path, media_path, kind),
        )
        .map_err(|e| format!("insert sidecar {path}: {e}"))?;
        Ok(())
    }

    pub fn add_album_membership(&self, album: &str, media_path: &str) -> Result<(), String> {
        self.conn.execute(
            "INSERT INTO album_membership (album, media_path) VALUES (?1, ?2)",
            (album, media_path),
        )
        .map_err(|e| format!("insert album membership {album}: {e}"))?;
        Ok(())
    }

    pub fn counts(&self) -> Result<Counts, String> {
        let stats: (u32, u32, u32, f64) = self
            .conn
            .query_row(
                "SELECT COALESCE(SUM(CASE WHEN kind='photo' THEN 1 ELSE 0 END),0), COALESCE(SUM(CASE WHEN kind='video' THEN 1 ELSE 0 END),0), COALESCE(SUM(CASE WHEN kind='live' THEN 1 ELSE 0 END),0), COALESCE(SUM(size),0) / 1024.0 / 1024.0 / 1024.0 FROM media",
                (),
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .map_err(|e| format!("count media rows: {e}"))?;
        Ok(Counts { photos: stats.0, videos: stats.1, live: stats.2, total_gb: stats.3 })
    }
}

pub fn detect_numbering_gaps(numbers: &[u32]) -> Vec<u32> {
    let mut set = numbers.iter().copied().collect::<std::collections::HashSet<_>>();
    let start = numbers.iter().copied().min().unwrap_or(0);
    let end = numbers.iter().copied().max().unwrap_or(0);
    let mut missing = Vec::new();
    for n in start..=end {
        if !set.contains(&n) {
            missing.push(n);
        }
    }
    missing
}

pub fn detect_numbering_gap_from_names(names: &[String]) -> Vec<u32> {
    let mut numbers = Vec::new();
    for name in names {
        let mut candidate = None;
        let cleaned = name.rsplit('/').next().unwrap_or(name).to_string();
        for part in cleaned.split(|c: char| !c.is_ascii_digit()) {
            if part.is_empty() {
                continue;
            }
            if let Ok(v) = part.parse::<u32>() {
                candidate = Some(v);
            }
        }
        if let Some(number) = candidate {
            numbers.push(number);
        }
    }
    detect_numbering_gaps(&numbers)
}

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

pub fn create_source(path: &Path) -> Result<Box<dyn Source>, String> {
    let path_str = path.to_string_lossy().to_ascii_lowercase();
    if path_str.ends_with(".zip") {
        Ok(Box::new(ZipSource { path: path.to_path_buf() }))
    } else if path_str.ends_with(".tgz") || path_str.ends_with(".tar.gz") {
        Ok(Box::new(TgzSource { path: path.to_path_buf() }))
    } else if path.is_dir() {
        Ok(Box::new(FolderSource { root: path.to_path_buf() }))
    } else {
        Err(format!("unsupported Takeout source: {}", path.display()))
    }
}

pub fn scan_paths(paths: &[String]) -> Result<ScanSourcesResult, String> {
    if paths.is_empty() {
        return Ok(ScanSourcesResult {
            files: Vec::new(),
            counts: Counts { photos: 0, videos: 0, live: 0, total_gb: 0.0 },
            warnings: Vec::new(),
        });
    }

    let flattened: Vec<String> = paths
        .iter()
        .filter(|path| !path.trim().is_empty())
        .cloned()
        .collect();

    let discovered: Vec<ScanSourcesResult> = flattened
        .par_iter()
        .map(|path| {
            let source = create_source(Path::new(path)).map_err(|e| e.to_string())?;
            let entries = source.list_entries().map_err(|e| e.to_string())?;
            let mut index = TempIndex::new().map_err(|e| e.to_string())?;
            for entry in entries {
                let normalized = entry.replace('\\', "/");
                let lower = normalized.to_ascii_lowercase();
                let kind = if lower.ends_with(".jpg") || lower.ends_with(".jpeg") || lower.ends_with(".png") || lower.ends_with(".heic") || lower.ends_with(".webp") {
                    "photo"
                } else if lower.ends_with(".mp4") || lower.ends_with(".mov") || lower.ends_with(".m4v") {
                    "video"
                } else {
                    "other"
                };
                if kind != "other" {
                    let size = source.stream_entry(&normalized).map_err(|e| e.to_string())?.len() as u64;
                    index.add_media(&normalized, kind, size).map_err(|e| e.to_string())?;
                }
                if lower.ends_with(".json") {
                    index.add_sidecar(&normalized, &normalized, "json").map_err(|e| e.to_string())?;
                }
                if normalized.contains("album") {
                    index.add_album_membership("album", &normalized).map_err(|e| e.to_string())?;
                }
            }
            let counts = index.counts().map_err(|e| e.to_string())?;
            Ok(ScanSourcesResult {
                files: vec![path.clone()],
                counts,
                warnings: Vec::new(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    let mut files = flattened.clone();
    let mut counts = Counts { photos: 0, videos: 0, live: 0, total_gb: 0.0 };
    let mut warnings = Vec::new();
    for result in discovered {
        counts.photos += result.counts.photos;
        counts.videos += result.counts.videos;
        counts.live += result.counts.live;
        counts.total_gb += result.counts.total_gb;
        warnings.extend(result.warnings);
    }

    let numbering = detect_numbering_gap_from_names(&files);
    if !numbering.is_empty() {
        warnings.push(format!("Numbering gap detected in part files: {:?}", numbering));
    }

    if let Some(root) = detect_google_photos_root(&files) {
        warnings.push(format!("Detected Google Photos folder: {root}"));
    }

    Ok(ScanSourcesResult {
        files,
        counts,
        warnings,
    })
}

pub mod matcher;

pub fn scan_sources() -> Vec<String> {
    vec!["takeout-2024.zip".to_string()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use tempfile::tempdir;

    fn create_zip(path: &Path, name: &str, entries: &[(&str, &[u8])]) {
        let file = fs::File::create(path).unwrap();
        let mut archive = zip::ZipWriter::new(file);
        let options = zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
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
            archive.append_data(&mut header, *entry_name, *data).unwrap();
        }
        archive.finish().unwrap();
    }

    #[test]
    fn source_implementations_list_and_stream_entries() {
        let dir = tempdir().unwrap();
        let zip_path = dir.path().join("part-001.zip");
        let tgz_path = dir.path().join("part-002.tgz");
        let folder = dir.path().join("folder-root");
        fs::create_dir_all(&folder).unwrap();

        let zip_data = b"zip-test";
        let tgz_data = b"tgz-test";
        fs::write(folder.join("media.jpg"), b"jpg").unwrap();
        fs::write(folder.join("sidecar.json"), b"{}") .unwrap();

        create_zip(&zip_path, "test", &[("Takeout/Google Fotos/Photos from 2024/media.jpg", zip_data)]);
        create_tgz(&tgz_path, &[("Takeout/Google Fotos/Photos from 2024/sidecar.json", tgz_data)]);

        let zip_source = ZipSource { path: zip_path.clone() };
        assert!(zip_source.list_entries().unwrap().contains(&"Takeout/Google Fotos/Photos from 2024/media.jpg".to_string()));
        assert_eq!(zip_source.stream_entry("Takeout/Google Fotos/Photos from 2024/media.jpg").unwrap(), zip_data);

        let tgz_source = TgzSource { path: tgz_path.clone() };
        assert!(tgz_source.list_entries().unwrap().contains(&"Takeout/Google Fotos/Photos from 2024/sidecar.json".to_string()));
        assert_eq!(tgz_source.stream_entry("Takeout/Google Fotos/Photos from 2024/sidecar.json").unwrap(), tgz_data);

        let folder_source = FolderSource { root: folder.clone() };
        let entries = folder_source.list_entries().unwrap();
        assert!(entries.iter().any(|entry| entry.ends_with("media.jpg")));
        assert_eq!(folder_source.stream_entry("media.jpg").unwrap(), b"jpg");
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
        assert_eq!(detect_google_photos_root(&files).as_deref(), Some("Google Fotos"));
    }
}
