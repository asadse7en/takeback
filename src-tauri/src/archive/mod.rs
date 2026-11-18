use serde::{Deserialize, Serialize};

use crate::metadata::TakeoutMetadata;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ArchiveType {
    Zip,
    Tgz,
    Folder,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EntryType {
    Photo,
    Video,
    JsonSidecar,
    AlbumMetadata,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SourceRecord {
    pub id: String,
    pub path: String,
    pub archive_type: ArchiveType,
    pub size_bytes: u64,
    pub entry_count: u64,
    pub entry_count_known: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct AddSourcesResult {
    pub archives: Vec<SourceRecord>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IndexedEntry {
    pub archive_path: String,
    pub path: String,
    #[serde(default)]
    pub entry_path: String,
    pub size: u64,
    pub entry_type: EntryType,
    pub album: Option<String>,
    pub trashed: bool,
    pub archived: bool,
    #[serde(default)]
    pub embedded_motion_video: bool,
    #[serde(default)]
    pub video_duration_seconds: Option<f64>,
    pub sidecar_archive_path: Option<String>,
    pub sidecar_path: Option<String>,
    pub sidecar_match: Option<String>,
    pub metadata: Option<TakeoutMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceEntry {
    pub entry_path: String,
    pub path: String,
    pub size: u64,
    pub entry_type: EntryType,
}
