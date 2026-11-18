use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek, SeekFrom};

pub use crate::metadata::TakeoutMetadata as JsonFields;

const EDITED_SUFFIXES: &[&str] = &[
    "-edited",
    "_edited",
    " edited",
    ".edited",
    "-bearbeitet",
    "_bearbeitet",
    " bearbeitet",
    ".bearbeitet",
    "-modifié",
    "_modifié",
    " modifié",
    ".modifié",
    "-editado",
    "_editado",
    " editado",
    ".editado",
    "-edytowane",
    "_edytowane",
    " edytowane",
    ".edytowane",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MatchReason {
    Exact,
    Supplemental,
    Truncated,
    NumberedDuplicate,
    EditedSuffixStripped,
    Title,
    NoJson,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaMatch {
    pub media_path: String,
    pub json_path: Option<String>,
    pub json: Option<JsonFields>,
    pub match_reason: MatchReason,
    pub is_edited: bool,
    pub is_live_pair: bool,
    pub album_membership: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidecarCandidate {
    pub archive_path: String,
    pub path: String,
    pub title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaCandidate {
    pub archive_path: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidecarMatch {
    pub archive_path: String,
    pub path: String,
    pub reason: MatchReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LiveMediaKind {
    Photo,
    Video,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LivePhotoCandidate {
    pub path: String,
    pub kind: LiveMediaKind,
    pub taken_timestamp: Option<i64>,
    pub duration_seconds: Option<f64>,
    pub embedded_motion_marker: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LivePhotoPair {
    pub photo_path: String,
    pub video_path: String,
    pub content_identifier: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LivePairWarning {
    pub paths: Vec<String>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LivePhotoDetection {
    pub pairs: Vec<LivePhotoPair>,
    pub embedded_motion_photos: Vec<String>,
    pub warnings: Vec<LivePairWarning>,
}

#[derive(Debug, Default)]
struct SidecarIndex {
    by_name: HashMap<String, Vec<usize>>,
    truncated: HashMap<String, Vec<usize>>,
    title: HashMap<String, Vec<usize>>,
    names: Vec<String>,
    supplemental: Vec<bool>,
}

pub fn match_sidecars(
    media_paths: &[String],
    sidecars: &[SidecarCandidate],
) -> Vec<Option<SidecarMatch>> {
    let media: Vec<MediaCandidate> = media_paths
        .iter()
        .map(|path| MediaCandidate {
            archive_path: String::new(),
            path: path.clone(),
        })
        .collect();
    match_sidecars_for_media(&media, sidecars)
}

pub fn match_sidecars_for_media(
    media: &[MediaCandidate],
    sidecars: &[SidecarCandidate],
) -> Vec<Option<SidecarMatch>> {
    let mut index = SidecarIndex::default();
    for (candidate_index, candidate) in sidecars.iter().enumerate() {
        let (key, supplemental) = sidecar_media_name(&candidate.path);
        index.names.push(key.clone());
        index.supplemental.push(supplemental);
        index
            .by_name
            .entry(key.clone())
            .or_default()
            .push(candidate_index);
        if char_count(&key) == 46 {
            index
                .truncated
                .entry(key)
                .or_default()
                .push(candidate_index);
        }
        if let Some(title) = candidate.title.as_deref() {
            index
                .title
                .entry(title.to_lowercase())
                .or_default()
                .push(candidate_index);
        }
    }

    media
        .iter()
        .map(|media_entry| {
            let media_name = normalize_media_name(&media_entry.path);
            let match_candidate = choose_candidate(
                index.by_name.get(&media_name),
                media_entry,
                sidecars,
                &index.names,
            )
            .map(|candidate| {
                let reason = if index.supplemental[candidate] {
                    MatchReason::Supplemental
                } else {
                    MatchReason::Exact
                };
                (candidate, reason)
            })
            .or_else(|| {
                let edited_name = strip_edited_suffix(&media_name);
                if edited_name == media_name {
                    return None;
                }
                choose_candidate(
                    index.by_name.get(&edited_name),
                    media_entry,
                    sidecars,
                    &index.names,
                )
                .map(|candidate| (candidate, MatchReason::EditedSuffixStripped))
            })
            .or_else(|| {
                let mut candidates = Vec::new();
                for prefix in truncated_candidate_indices(&media_name) {
                    if let Some(indices) = index.truncated.get(&prefix) {
                        candidates.extend(indices.iter().copied());
                    }
                }
                choose_candidate(
                    (!candidates.is_empty()).then_some(&candidates),
                    media_entry,
                    sidecars,
                    &index.names,
                )
                .map(|candidate| (candidate, MatchReason::Truncated))
            })
            .or_else(|| {
                choose_candidate(
                    index.title.get(&media_name),
                    media_entry,
                    sidecars,
                    &index.names,
                )
                .map(|candidate| (candidate, MatchReason::Title))
            });

            match_candidate.map(|(candidate_index, reason)| SidecarMatch {
                archive_path: sidecars[candidate_index].archive_path.clone(),
                path: sidecars[candidate_index].path.clone(),
                reason,
            })
        })
        .collect()
}

pub fn match_media_to_json(media_paths: &[String], sidecar_paths: &[String]) -> Vec<MediaMatch> {
    let sidecars: Vec<SidecarCandidate> = sidecar_paths
        .iter()
        .map(|path| SidecarCandidate {
            archive_path: String::new(),
            path: path.clone(),
            title: read_sidecar_json(path).and_then(|(_, fields)| fields.title),
        })
        .collect();
    let matches = match_sidecars(media_paths, &sidecars);
    let live_pairs = detect_live_pairs(media_paths);

    media_paths
        .iter()
        .zip(matches)
        .map(|(media_path, matched)| {
            let json_path = matched.as_ref().map(|item| item.path.clone());
            let json = json_path
                .as_deref()
                .and_then(read_sidecar_json)
                .map(|(_, fields)| fields);

            MediaMatch {
                media_path: media_path.clone(),
                json_path: json_path,
                json,
                match_reason: matched.map_or(MatchReason::NoJson, |item| item.reason),
                is_edited: is_edited_path(media_path),
                is_live_pair: live_pairs.contains(media_path),
                album_membership: album_membership(media_path),
            }
        })
        .collect()
}

fn file_name(path: &str) -> &str {
    path.rsplit(|character| character == '/' || character == '\\')
        .next()
        .unwrap_or(path)
}

fn sidecar_media_name(path: &str) -> (String, bool) {
    let mut name = file_name(path).to_lowercase();
    if let Some(stripped) = name.strip_suffix(".json") {
        name = stripped.to_string();
    }
    let (name, supplemental) = strip_supplemental_suffix(&name);
    (normalize_duplicate_position(&name), supplemental)
}

fn normalize_media_name(path: &str) -> String {
    normalize_duplicate_position(&file_name(path).to_lowercase())
}

pub fn duplicate_group_name(name: &str) -> String {
    let normalized = normalize_duplicate_position(&file_name(name).to_lowercase());
    strip_duplicate_suffix_from_name(&strip_edited_suffix(&normalized))
}

fn strip_duplicate_suffix_from_name(name: &str) -> String {
    let Some((stem, extension)) = name.rsplit_once('.') else {
        return name.to_string();
    };
    let (base, duplicate) = split_duplicate_suffix(stem);
    if duplicate.is_empty() {
        name.to_string()
    } else {
        format!("{base}.{extension}")
    }
}

fn strip_supplemental_suffix(name: &str) -> (String, bool) {
    const SUPPLEMENTAL_SUFFIX: &str = ".supplemental-metadata";
    for length in (2..=SUPPLEMENTAL_SUFFIX.len()).rev() {
        let suffix = &SUPPLEMENTAL_SUFFIX[..length];
        if let Some(stem) = name.strip_suffix(suffix) {
            if !stem.is_empty() {
                return (stem.to_string(), true);
            }
        }
    }
    (name.to_string(), false)
}

fn normalize_duplicate_position(name: &str) -> String {
    let Some(dot) = name.rfind('.') else {
        return name.to_string();
    };
    let (stem, extension_and_duplicate) = name.split_at(dot);
    let Some(open) = extension_and_duplicate.rfind('(') else {
        return name.to_string();
    };
    let (extension, duplicate) = extension_and_duplicate.split_at(open);
    if extension.len() <= 1 || !is_duplicate_suffix(duplicate) {
        return name.to_string();
    }
    format!("{stem}{duplicate}.{}", &extension[1..])
}

fn is_duplicate_suffix(value: &str) -> bool {
    value.starts_with('(')
        && value.ends_with(')')
        && value.len() > 2
        && value[1..value.len() - 1]
            .chars()
            .all(|character| character.is_ascii_digit())
}

fn split_duplicate_suffix(stem: &str) -> (&str, &str) {
    if let Some(open) = stem.rfind('(') {
        let duplicate = &stem[open..];
        if is_duplicate_suffix(duplicate) {
            return (&stem[..open], duplicate);
        }
    }
    (stem, "")
}

fn char_count(value: &str) -> usize {
    value.chars().count()
}

fn take_chars(value: &str, count: usize) -> String {
    value.chars().take(count).collect()
}

fn strip_edited_suffix(name: &str) -> String {
    let Some((stem, extension)) = name.rsplit_once('.') else {
        return strip_edited_suffix_from_stem(name);
    };
    let (stem, duplicate) = split_duplicate_suffix(stem);
    let stripped = strip_edited_suffix_from_stem(stem);
    if stripped == stem {
        name.to_string()
    } else {
        format!("{stripped}{duplicate}.{extension}")
    }
}

fn strip_edited_suffix_from_stem(stem: &str) -> String {
    let mut cleaned = stem.to_string();
    for suffix in EDITED_SUFFIXES {
        if cleaned.ends_with(suffix) {
            cleaned = cleaned
                .strip_suffix(suffix)
                .unwrap_or(&cleaned)
                .trim()
                .to_string();
            break;
        }
    }
    cleaned
}

fn truncated_candidate_indices(media_name: &str) -> Vec<String> {
    if char_count(media_name) <= 46 {
        return Vec::new();
    }
    let Some((stem, _)) = media_name.rsplit_once('.') else {
        return Vec::new();
    };
    let mut prefixes = vec![take_chars(media_name, 46), take_chars(stem, 46)];
    prefixes.dedup();
    prefixes
}

fn choose_candidate(
    matches: Option<&Vec<usize>>,
    media: &MediaCandidate,
    candidates: &[SidecarCandidate],
    candidate_names: &[String],
) -> Option<usize> {
    let indices = matches?;
    let all_candidates = unique_indices(indices);
    let media_parent = parent_path(&media.path);
    let same_folder = indices
        .iter()
        .copied()
        .filter(|index| parent_path(&candidates[*index].path) == media_parent)
        .collect::<Vec<_>>();
    if let Some(candidate) = choose_compatible_candidate(
        &same_folder,
        &media.path,
        candidate_names,
        all_candidates.len() == 1,
    ) {
        return Some(candidate);
    }

    let album = album_membership(&media.path).into_iter().next()?;
    let album_key = album.to_lowercase();
    let media_archive = normalize_archive_key(&media.archive_path);
    let same_album_in_other_archives = indices
        .iter()
        .copied()
        .filter(|index| {
            normalize_archive_key(&candidates[*index].archive_path) != media_archive
                && album_membership(&candidates[*index].path)
                    .first()
                    .is_some_and(|candidate_album| candidate_album.to_lowercase() == album_key)
        })
        .collect::<Vec<_>>();
    choose_compatible_candidate(
        &same_album_in_other_archives,
        &media.path,
        candidate_names,
        all_candidates.len() == 1,
    )
}

fn choose_compatible_candidate(
    indices: &[usize],
    media_path: &str,
    candidate_names: &[String],
    allow_single_mismatch: bool,
) -> Option<usize> {
    let unique = unique_indices(indices);
    if unique.len() == 1 {
        let candidate = unique[0];
        let candidate_family = extension_family(&candidate_names[candidate]);
        return (allow_single_mismatch
            || candidate_family == extension_family(media_path)
            || candidate_family == ExtensionFamily::Unknown)
            .then_some(candidate);
    }

    let media_family = extension_family(media_path);
    let same_family = unique
        .iter()
        .copied()
        .filter(|index| extension_family(&candidate_names[*index]) == media_family)
        .collect::<Vec<_>>();
    if let Some(candidate) = same_family.first() {
        return Some(*candidate);
    }

    let unknown_family = unique
        .iter()
        .copied()
        .filter(|index| extension_family(&candidate_names[*index]) == ExtensionFamily::Unknown)
        .collect::<Vec<_>>();
    (unknown_family.len() == 1).then(|| unknown_family[0])
}

fn unique_indices(indices: &[usize]) -> Vec<usize> {
    let mut seen = HashSet::with_capacity(indices.len());
    indices
        .iter()
        .copied()
        .filter(|index| seen.insert(*index))
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ExtensionFamily {
    Photo,
    Video,
    Unknown,
}

fn extension_family(path: &str) -> ExtensionFamily {
    let name = file_name(path).to_lowercase();
    let Some((_, extension)) = name.rsplit_once('.') else {
        return ExtensionFamily::Unknown;
    };
    match extension {
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "heic" | "heif" | "tif" | "tiff" | "bmp"
        | "dng" | "cr2" | "cr3" | "nef" | "arw" | "rw2" | "orf" | "raf" | "pef" => {
            ExtensionFamily::Photo
        }
        "mp4" | "mov" | "m4v" | "3gp" | "avi" | "mkv" | "wmv" | "mpg" | "mpeg" | "mts" | "m2ts"
        | "webm" => ExtensionFamily::Video,
        _ => ExtensionFamily::Unknown,
    }
}

fn parent_path(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    normalized
        .rsplit_once('/')
        .map_or_else(String::new, |(parent, _)| parent.to_lowercase())
}

fn normalize_archive_key(path: &str) -> String {
    path.replace('\\', "/").to_lowercase()
}

fn read_sidecar_json(path: &str) -> Option<(String, JsonFields)> {
    let file = File::open(path).ok()?;
    let fields = JsonFields::from_reader(BufReader::new(file)).ok()?;
    Some((path.to_string(), fields))
}

pub fn is_edited_path(path: &str) -> bool {
    let name = path.to_lowercase();
    for suffix in EDITED_SUFFIXES {
        if name.contains(suffix) {
            return true;
        }
    }
    false
}

pub fn is_android_motion_photo_path(path: &str) -> bool {
    let filename = path.replace('\\', "/");
    let filename = filename.rsplit('/').next().unwrap_or(&filename);
    let filename = filename.to_ascii_lowercase();
    filename.ends_with(".mp.jpg")
        || filename.ends_with(".mp.jpeg")
        || filename.ends_with(".mp.heic")
}

pub fn has_motion_photo_xmp_markers(reader: impl Read) -> Result<bool, String> {
    let mut buffer = Vec::new();
    reader
        .take(256 * 1024)
        .read_to_end(&mut buffer)
        .map_err(|error| format!("read Motion Photo XMP markers: {error}"))?;
    let lowered = buffer
        .iter()
        .map(u8::to_ascii_lowercase)
        .collect::<Vec<_>>();
    Ok(contains_bytes(&lowered, b"microvideo") || contains_bytes(&lowered, b"motionphoto"))
}

pub fn verify_android_motion_photo(mut reader: impl Read) -> Result<bool, String> {
    let mut buffer = [0_u8; 64 * 1024];
    let mut carry = Vec::new();
    let mut has_motion_xmp = false;
    let mut has_mp4 = false;
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|error| format!("read possible Android Motion Photo: {error}"))?;
        if count == 0 {
            break;
        }
        carry.extend_from_slice(&buffer[..count]);
        let lowered = carry.iter().map(u8::to_ascii_lowercase).collect::<Vec<_>>();
        has_motion_xmp |=
            contains_bytes(&lowered, b"microvideo") || contains_bytes(&lowered, b"motionphoto");
        has_mp4 |= contains_bytes(&lowered, b"ftyp");
        if has_motion_xmp && has_mp4 {
            return Ok(true);
        }
        let retain_from = carry.len().saturating_sub(32);
        carry.drain(..retain_from);
    }
    Ok(false)
}

pub fn read_video_duration_seconds(mut reader: impl Read) -> Result<Option<f64>, String> {
    loop {
        let Some((kind, atom_size, header_size)) = read_atom_header(&mut reader)? else {
            return Ok(None);
        };
        if atom_size < header_size {
            return Ok(None);
        }
        let payload_size = atom_size - header_size;
        if &kind == b"moov" {
            return read_movie_header(&mut reader, payload_size);
        }
        if !skip_bytes(&mut reader, payload_size)? {
            return Ok(None);
        }
    }
}

pub const VIDEO_DURATION_PROBE_BYTES: usize = 2 * 1024 * 1024;

pub fn read_video_duration_from_seekable(
    reader: &mut (impl Read + Seek),
    size: u64,
) -> Result<Option<f64>, String> {
    let head_len = size.min(VIDEO_DURATION_PROBE_BYTES as u64) as usize;
    reader
        .seek(SeekFrom::Start(0))
        .map_err(|error| format!("seek to video header: {error}"))?;
    let mut head = vec![0; head_len];
    reader
        .read_exact(&mut head)
        .map_err(|error| format!("read video header: {error}"))?;
    if let Some(duration) = read_video_duration_seconds(&head[..])? {
        return Ok(Some(duration));
    }
    if size <= VIDEO_DURATION_PROBE_BYTES as u64 {
        return Ok(None);
    }

    let tail_len = size.min(VIDEO_DURATION_PROBE_BYTES as u64) as usize;
    reader
        .seek(SeekFrom::Start(size - tail_len as u64))
        .map_err(|error| format!("seek to video tail: {error}"))?;
    let mut tail = vec![0; tail_len];
    reader
        .read_exact(&mut tail)
        .map_err(|error| format!("read video tail: {error}"))?;
    read_video_duration_from_tail(&tail)
}

pub fn read_video_duration_from_tail(tail: &[u8]) -> Result<Option<f64>, String> {
    for (marker_index, marker) in tail.windows(4).enumerate() {
        if marker != b"moov" || marker_index < 4 {
            continue;
        }
        let standard_start = marker_index - 4;
        let size = u32::from_be_bytes([
            tail[standard_start],
            tail[standard_start + 1],
            tail[standard_start + 2],
            tail[standard_start + 3],
        ]) as u64;
        let (atom_start, atom_size, header_size) = if size == 1 {
            let Some(size_end) = marker_index.checked_add(12) else {
                continue;
            };
            if size_end > tail.len() {
                continue;
            }
            let mut size_bytes = [0; 8];
            size_bytes.copy_from_slice(&tail[marker_index + 4..size_end]);
            (standard_start, u64::from_be_bytes(size_bytes), 16u64)
        } else {
            (standard_start, size, 8u64)
        };
        let available = (tail.len() - atom_start) as u64;
        let atom_size = if atom_size == 0 { available } else { atom_size };
        if atom_size < header_size || atom_size > available {
            continue;
        }
        let payload_start = atom_start + header_size as usize;
        let payload_end = atom_start + atom_size as usize;
        let payload = &tail[payload_start..payload_end];
        if let Some(duration) = read_movie_header(&mut Cursor::new(payload), payload.len() as u64)?
        {
            return Ok(Some(duration));
        }
    }
    Ok(None)
}

fn read_movie_header(reader: &mut impl Read, mut remaining: u64) -> Result<Option<f64>, String> {
    while remaining >= 8 {
        let Some((kind, atom_size, header_size)) = read_atom_header(reader)? else {
            return Ok(None);
        };
        if atom_size < header_size || atom_size > remaining {
            return Ok(None);
        }
        let payload_size = atom_size - header_size;
        if &kind == b"mvhd" {
            return read_mvhd_duration(reader, payload_size);
        }
        if !skip_bytes(reader, payload_size)? {
            return Ok(None);
        }
        remaining -= atom_size;
    }
    Ok(None)
}

fn read_atom_header(reader: &mut impl Read) -> Result<Option<([u8; 4], u64, u64)>, String> {
    let mut header = [0_u8; 8];
    match reader.read(&mut header[..1]) {
        Ok(0) => return Ok(None),
        Ok(_) => {}
        Err(error) => return Err(format!("read video atom header: {error}")),
    }
    if !read_exact_or_incomplete(reader, &mut header[1..], "read video atom header")? {
        return Ok(None);
    }
    let size = u32::from_be_bytes([header[0], header[1], header[2], header[3]]) as u64;
    let mut kind = [0_u8; 4];
    kind.copy_from_slice(&header[4..8]);
    if size == 1 {
        let mut extended = [0_u8; 8];
        if !read_exact_or_incomplete(reader, &mut extended, "read extended video atom header")? {
            return Ok(None);
        }
        Ok(Some((kind, u64::from_be_bytes(extended), 16)))
    } else if size == 0 {
        Ok(Some((kind, u64::MAX, 8)))
    } else {
        Ok(Some((kind, size, 8)))
    }
}

fn read_mvhd_duration(reader: &mut impl Read, payload_size: u64) -> Result<Option<f64>, String> {
    let mut fixed = [0_u8; 4];
    if payload_size < fixed.len() as u64 {
        return Ok(None);
    }
    if !read_exact_or_incomplete(reader, &mut fixed, "read video movie header")? {
        return Ok(None);
    }
    let mut remaining = payload_size - 4;
    let (creation_size, duration_size) = match fixed[0] {
        0 => (4_u64, 4_u64),
        1 => (8_u64, 8_u64),
        _ => return Ok(None),
    };
    let required = creation_size * 2 + 4 + duration_size;
    if remaining < required {
        return Ok(None);
    }
    if !skip_bytes(reader, creation_size * 2)? {
        return Ok(None);
    }
    remaining -= creation_size * 2;
    let mut timescale = [0_u8; 4];
    if !read_exact_or_incomplete(reader, &mut timescale, "read video timescale")? {
        return Ok(None);
    }
    let timescale = u32::from_be_bytes(timescale) as f64;
    remaining -= 4;
    let mut duration = [0_u8; 8];
    let duration = if duration_size == 4 {
        if !read_exact_or_incomplete(reader, &mut duration[4..], "read video duration")? {
            return Ok(None);
        }
        u32::from_be_bytes([duration[4], duration[5], duration[6], duration[7]]) as f64
    } else {
        if !read_exact_or_incomplete(reader, &mut duration, "read video duration")? {
            return Ok(None);
        }
        u64::from_be_bytes(duration) as f64
    };
    remaining -= duration_size;
    if remaining > 0 && !skip_bytes(reader, remaining)? {
        return Ok(None);
    }
    if timescale == 0.0 {
        Ok(None)
    } else {
        Ok(Some(duration / timescale))
    }
}

fn read_exact_or_incomplete(
    reader: &mut impl Read,
    buffer: &mut [u8],
    operation: &str,
) -> Result<bool, String> {
    match reader.read_exact(buffer) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => Ok(false),
        Err(error) => Err(format!("{operation}: {error}")),
    }
}

fn skip_bytes(reader: &mut impl Read, mut count: u64) -> Result<bool, String> {
    let mut buffer = [0_u8; 16 * 1024];
    while count > 0 {
        let requested = buffer.len().min(count.min(buffer.len() as u64) as usize);
        let read = reader
            .read(&mut buffer[..requested])
            .map_err(|error| format!("skip video data: {error}"))?;
        if read == 0 {
            return Ok(false);
        }
        count -= read as u64;
    }
    Ok(true)
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

pub fn is_year_folder_name(name: &str) -> bool {
    let Some((prefix, year)) = name.rsplit_once(' ') else {
        return false;
    };
    !prefix.is_empty()
        && prefix.chars().all(|character| !character.is_ascii_digit())
        && year.len() == 4
        && year.chars().all(|character| character.is_ascii_digit())
}

pub fn is_photos_year_folder_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/");
    let parts = normalized
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    photos_root_index(&parts)
        .and_then(|root| parts.get(root + 1))
        .is_some_and(|folder| is_year_folder_name(folder))
}

fn photos_root_index(parts: &[&str]) -> Option<usize> {
    parts
        .iter()
        .position(|part| {
            ["google photos", "google fotos", "google フォト"]
                .iter()
                .any(|root| part.to_lowercase() == *root)
        })
        .or_else(|| {
            parts
                .iter()
                .position(|part| part.eq_ignore_ascii_case("takeout"))
                .and_then(|takeout| (takeout + 1 < parts.len()).then_some(takeout + 1))
        })
}

pub fn album_membership(path: &str) -> Vec<String> {
    let normalized = path.replace('\\', "/");
    let parts: Vec<&str> = normalized
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    let Some(root_index) = photos_root_index(&parts) else {
        return Vec::new();
    };
    let Some(album) = parts.get(root_index + 1) else {
        return Vec::new();
    };
    if is_year_folder_name(album)
        || matches!(
            album.to_ascii_lowercase().as_str(),
            "trash" | "archive" | "metadata.json"
        )
    {
        Vec::new()
    } else {
        vec![(*album).to_string()]
    }
}

pub fn detect_live_pairs(media_paths: &[String]) -> std::collections::HashSet<String> {
    let mut by_location: HashMap<String, Vec<(String, bool)>> = HashMap::new();
    for item in media_paths {
        let path = item.replace('\\', "/");
        let file_name = path.rsplit('/').next().unwrap_or(&path);
        let lower_name = file_name.to_ascii_lowercase();
        let Some((base, extension)) = lower_name.rsplit_once('.') else {
            continue;
        };
        let is_photo = matches!(extension, "jpg" | "jpeg" | "heic" | "heif" | "png");
        let is_video = matches!(extension, "mp4" | "mov" | "m4v");
        if !is_photo && !is_video {
            continue;
        }
        let base = base.strip_suffix(".mp").unwrap_or(base);
        let parent = path
            .rsplit_once('/')
            .map_or_else(String::new, |(parent, _)| parent.to_ascii_lowercase());
        by_location
            .entry(format!("{parent}/{base}"))
            .or_default()
            .push((item.clone(), is_video));
    }

    let mut live = std::collections::HashSet::new();
    for items in by_location.values() {
        let photos = items.iter().filter(|(_, is_video)| !is_video).count();
        let videos = items.iter().filter(|(_, is_video)| *is_video).count();
        if photos == 1 && videos == 1 {
            for (path, _) in items {
                live.insert(path.clone());
            }
        }
    }
    live
}

pub fn match_live_photos(candidates: &[LivePhotoCandidate]) -> LivePhotoDetection {
    let mut detection = LivePhotoDetection::default();
    let mut grouped: HashMap<String, Vec<usize>> = HashMap::new();
    let mut by_base: HashMap<String, Vec<usize>> = HashMap::new();

    for (index, candidate) in candidates.iter().enumerate() {
        if is_android_motion_photo_path(&candidate.path) {
            if candidate.embedded_motion_marker {
                detection
                    .embedded_motion_photos
                    .push(candidate.path.clone());
            } else {
                detection.warnings.push(LivePairWarning {
                    paths: vec![candidate.path.clone()],
                    message: format!(
                        "No Motion Photo XMP marker was found in {}; kept the file separate.",
                        candidate.path
                    ),
                });
            }
            continue;
        }

        let (parent, base) = live_path_key(&candidate.path);
        grouped
            .entry(format!("{parent}/{base}"))
            .or_default()
            .push(index);
        by_base.entry(base).or_default().push(index);
    }

    let mut paired_paths = std::collections::HashSet::new();
    for indices in grouped.values() {
        let photos = indices
            .iter()
            .copied()
            .filter(|index| candidates[*index].kind == LiveMediaKind::Photo)
            .collect::<Vec<_>>();
        let videos = indices
            .iter()
            .copied()
            .filter(|index| candidates[*index].kind == LiveMediaKind::Video)
            .collect::<Vec<_>>();
        if photos.is_empty() || videos.is_empty() {
            continue;
        }
        if photos.len() != 1 || videos.len() != 1 {
            let paths = indices
                .iter()
                .map(|index| candidates[*index].path.clone())
                .collect();
            detection.warnings.push(LivePairWarning {
                paths,
                message: "More than one image or video matched this live photo name; kept all files separate.".to_string(),
            });
            continue;
        }

        let photo = &candidates[photos[0]];
        let video = &candidates[videos[0]];
        if let Some(duration) = video.duration_seconds {
            let (Some(photo_time), Some(video_time)) =
                (photo.taken_timestamp, video.taken_timestamp)
            else {
                detection.warnings.push(LivePairWarning {
                    paths: vec![photo.path.clone(), video.path.clone()],
                    message: "A live photo name matched, but capture times were unavailable; kept both files separate.".to_string(),
                });
                continue;
            };
            if photo_time.abs_diff(video_time) > 2 {
                detection.warnings.push(LivePairWarning {
                    paths: vec![photo.path.clone(), video.path.clone()],
                    message: "A live photo name matched, but capture times differ by more than two seconds; kept both files separate.".to_string(),
                });
                continue;
            }
            if duration > 10.0 {
                detection.warnings.push(LivePairWarning {
                    paths: vec![photo.path.clone(), video.path.clone()],
                    message: "A matching video is longer than a typical live photo; kept both files separate.".to_string(),
                });
                continue;
            }
        }

        let content_identifier = content_identifier(&photo.path, &video.path);
        detection.pairs.push(LivePhotoPair {
            photo_path: photo.path.clone(),
            video_path: video.path.clone(),
            content_identifier,
        });
        paired_paths.insert(photo.path.as_str());
        paired_paths.insert(video.path.as_str());
    }

    for indices in by_base.values() {
        if indices
            .iter()
            .any(|index| paired_paths.contains(candidates[*index].path.as_str()))
        {
            continue;
        }
        let has_photo = indices
            .iter()
            .any(|index| candidates[*index].kind == LiveMediaKind::Photo);
        let has_video = indices
            .iter()
            .any(|index| candidates[*index].kind == LiveMediaKind::Video);
        if has_photo && has_video {
            let paths = indices
                .iter()
                .map(|index| candidates[*index].path.clone())
                .collect();
            if !detection
                .warnings
                .iter()
                .any(|warning| warning.paths == paths)
            {
                detection.warnings.push(LivePairWarning {
                    paths,
                    message: "Matching live photo names were found in different folders; kept all files separate.".to_string(),
                });
            }
        }
    }

    detection
}

pub fn live_path_key(path: &str) -> (String, String) {
    let normalized = path.replace('\\', "/");
    let filename = normalized.rsplit('/').next().unwrap_or(&normalized);
    let lower = filename.to_ascii_lowercase();
    let (stem, _) = lower.rsplit_once('.').unwrap_or((&lower, ""));
    let stem = stem.strip_suffix(".mp").unwrap_or(stem);
    let parent = normalized
        .rsplit_once('/')
        .map_or_else(String::new, |(parent, _)| parent.to_ascii_lowercase());
    (parent, stem.to_string())
}

fn content_identifier(photo_path: &str, video_path: &str) -> String {
    let digest = blake3::hash(format!("{photo_path}\0{video_path}").as_bytes()).to_hex();
    let hex = digest.as_str();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use tempfile::tempdir;

    fn write_json(path: &Path, title: &str) {
        fs::write(path, format!(r#"{{"title":"{title}"}}"#)).unwrap();
    }

    #[test]
    fn matcher_cover_ordered_sidecar_rules() {
        let temp = tempdir().unwrap();
        let asset_dir = temp.path();
        let exact_path = asset_dir.join("IMG_1.jpg.json");
        let supplemental_path = asset_dir.join("IMG_2.jpg.supplemental-metadata.json");
        let media_base = format!("IMG_{}", "1".repeat(50));
        let truncated_name = media_base.chars().take(46).collect::<String>();
        let truncated_path = asset_dir.join(format!("{truncated_name}.json"));
        let duplicate_media = asset_dir.join("IMG_3(1).jpg");
        let duplicate_sidecar = asset_dir.join("IMG_3.jpg(1).json");
        let edited_media = asset_dir.join("IMG_4-edited.jpg");
        let title_media = asset_dir.join("IMG_5.jpg");
        let title_sidecar = asset_dir.join("IMG_5-title.json");

        write_json(&exact_path, "IMG_1");
        write_json(&supplemental_path, "IMG_2");
        write_json(&truncated_path, &truncated_name);
        write_json(&duplicate_sidecar, "IMG_3");
        write_json(&edited_media.with_file_name("IMG_4.jpg.json"), "IMG_4");
        write_json(&title_sidecar, "IMG_5.jpg");

        let cases = [
            (
                asset_dir.join("IMG_1.jpg").to_string_lossy().to_string(),
                vec![exact_path.to_string_lossy().to_string()],
                MatchReason::Exact,
            ),
            (
                asset_dir.join("IMG_2.jpg").to_string_lossy().to_string(),
                vec![supplemental_path.to_string_lossy().to_string()],
                MatchReason::Supplemental,
            ),
            (
                asset_dir
                    .join(format!("{media_base}.jpg"))
                    .to_string_lossy()
                    .to_string(),
                vec![truncated_path.to_string_lossy().to_string()],
                MatchReason::Truncated,
            ),
            (
                duplicate_media.to_string_lossy().to_string(),
                vec![duplicate_sidecar.to_string_lossy().to_string()],
                MatchReason::Exact,
            ),
            (
                edited_media.to_string_lossy().to_string(),
                vec![edited_media
                    .with_file_name("IMG_4.jpg.json")
                    .to_string_lossy()
                    .to_string()],
                MatchReason::EditedSuffixStripped,
            ),
            (
                title_media.to_string_lossy().to_string(),
                vec![title_sidecar.to_string_lossy().to_string()],
                MatchReason::Title,
            ),
            ("IMG_999.jpg".to_string(), vec![], MatchReason::NoJson),
        ];

        for (media, sidecars, expected) in cases {
            let matched = match_media_to_json(&[media.clone()], &sidecars);
            assert_eq!(matched.len(), 1, "case for {media}");
            assert_eq!(
                matched[0].match_reason, expected,
                "unexpected reason for {media}"
            );
        }
    }

    #[test]
    fn truncated_match_uses_46_char_prefix() {
        let base = "IMG_abcdefghijklmnopqrstuvwxyz12345678901234567890";
        let media = format!("{base}.jpg");
        let truncated = base.chars().take(46).collect::<String>();
        let sidecar = format!("{truncated}.json");
        let matched = match_media_to_json(&[media], &[sidecar]);
        assert_eq!(matched[0].match_reason, MatchReason::Truncated);
    }

    #[test]
    fn numbered_duplicate_match_uses_parenthesized_suffix() {
        let matched = match_media_to_json(
            &["IMG_1(1).jpg".to_string()],
            &["IMG_1.jpg(1).json".to_string()],
        );
        assert_eq!(matched[0].match_reason, MatchReason::Exact);
    }

    #[test]
    fn duplicate_group_name_removes_numbered_and_edited_suffixes() {
        for name in [
            "IMG_1.JPG",
            "IMG_1(2).jpg",
            "IMG_1.jpg(3)",
            "IMG_1-edited.jpg",
            "IMG_1-edited(4).jpg",
        ] {
            assert_eq!(duplicate_group_name(name), "img_1.jpg");
        }
    }

    #[test]
    fn localized_year_folders_are_not_album_names() {
        let path = "Takeout/Google Fotos/Fotos de 2024/IMG_1.jpg";

        assert!(album_membership(path).is_empty());
        assert!(is_photos_year_folder_path(path));
        assert!(is_year_folder_name("旅行 2024"));
        assert!(!is_year_folder_name("Album 2024 photos"));
        assert_eq!(
            album_membership("Takeout/Google Fotos/Family/IMG_1.jpg"),
            vec!["Family"]
        );
    }

    #[test]
    fn sidecar_keys_keep_extensions_and_partial_supplemental_suffixes() {
        for suffix in [".suppl", ".supplemental-metad"] {
            let sidecar = format!("Photos/IMG_1234.jpg{suffix}.json");
            let matched = match_sidecars(
                &["Photos/IMG_1234.jpg".to_string()],
                &[SidecarCandidate {
                    archive_path: "takeout.zip".to_string(),
                    path: sidecar.clone(),
                    title: None,
                }],
            );
            assert_eq!(
                matched[0].as_ref().map(|item| item.reason),
                Some(MatchReason::Supplemental)
            );
            assert_eq!(sidecar_media_name(&sidecar).0, "img_1234.jpg");
        }

        assert_eq!(
            sidecar_media_name("Photos/IMG_1234.jpg(1).json").0,
            "img_1234(1).jpg"
        );
    }

    #[test]
    fn sidecar_lookup_uses_exact_edited_truncated_then_title_order() {
        let media = ["Photos/IMG_1-edited.jpg".to_string()];
        let sidecars = [
            SidecarCandidate {
                archive_path: String::new(),
                path: "Photos/IMG_1-edited.jpg.json".to_string(),
                title: None,
            },
            SidecarCandidate {
                archive_path: String::new(),
                path: "Photos/IMG_1.jpg.json".to_string(),
                title: None,
            },
            SidecarCandidate {
                archive_path: String::new(),
                path: "Photos/other.json".to_string(),
                title: Some("IMG_1-edited.jpg".to_string()),
            },
        ];
        let exact = match_sidecars(&media, &sidecars);
        assert_eq!(
            exact[0].as_ref().map(|item| item.path.as_str()),
            Some("Photos/IMG_1-edited.jpg.json")
        );

        let edited = match_sidecars(&media, &sidecars[1..]);
        assert_eq!(
            edited[0].as_ref().map(|item| item.reason),
            Some(MatchReason::EditedSuffixStripped)
        );

        let long_name = format!("IMG_{}.jpg", "a".repeat(55));
        let truncated = take_chars(&long_name[..long_name.len() - 4], 46);
        let truncated_sidecars = [
            SidecarCandidate {
                archive_path: String::new(),
                path: format!("Photos/{truncated}.json"),
                title: None,
            },
            SidecarCandidate {
                archive_path: String::new(),
                path: "Photos/other.jpg.json".to_string(),
                title: Some(long_name.clone()),
            },
        ];
        let matched = match_sidecars(&[format!("Photos/{long_name}")], &truncated_sidecars);
        assert_eq!(
            matched[0].as_ref().map(|item| item.reason),
            Some(MatchReason::Truncated)
        );
    }

    #[test]
    fn title_matching_requires_the_full_filename_without_underscore_replacement() {
        let sidecars = [SidecarCandidate {
            archive_path: String::new(),
            path: "Photos/title.json".to_string(),
            title: Some("img_5.JPG".to_string()),
        }];
        let matches = match_sidecars(&["Photos/IMG_5.jpg".to_string()], &sidecars);
        assert_eq!(
            matches[0].as_ref().map(|item| item.path.as_str()),
            Some("Photos/title.json")
        );
        assert_eq!(
            matches[0].as_ref().map(|item| item.reason),
            Some(MatchReason::Title)
        );

        let space_title = [SidecarCandidate {
            archive_path: String::new(),
            path: "Photos/space-title.json".to_string(),
            title: Some("IMG 5.jpg".to_string()),
        }];
        assert!(match_sidecars(&["Photos/IMG_5.jpg".to_string()], &space_title)[0].is_none());
        let missing_extension = [SidecarCandidate {
            archive_path: String::new(),
            path: "Photos/extensionless.json".to_string(),
            title: Some("IMG_5".to_string()),
        }];
        assert!(match_sidecars(&["Photos/IMG_5.jpg".to_string()], &missing_extension)[0].is_none());
    }

    #[test]
    fn extension_family_prefers_compatible_title_candidates() {
        let media = ["Album/clip.mp4".to_string()];
        let candidates = [
            SidecarCandidate {
                archive_path: String::new(),
                path: "Album/photo.jpg.json".to_string(),
                title: Some("clip.mp4".to_string()),
            },
            SidecarCandidate {
                archive_path: String::new(),
                path: "Album/video.mov.json".to_string(),
                title: Some("CLIP.MP4".to_string()),
            },
        ];
        let matched = match_sidecars(&media, &candidates);
        assert_eq!(
            matched[0].as_ref().map(|item| item.path.as_str()),
            Some("Album/video.mov.json")
        );

        let only_mismatch = match_sidecars(&media, &candidates[..1]);
        assert_eq!(
            only_mismatch[0].as_ref().map(|item| item.path.as_str()),
            Some("Album/photo.jpg.json")
        );

        let multiple_mismatches = [
            candidates[0].clone(),
            SidecarCandidate {
                archive_path: String::new(),
                path: "Album/another-photo.png.json".to_string(),
                title: Some("clip.mp4".to_string()),
            },
        ];
        assert!(match_sidecars(&media, &multiple_mismatches)[0].is_none());
    }

    #[test]
    fn extension_mismatch_in_same_folder_does_not_block_compatible_album_fallback() {
        let media = [MediaCandidate {
            archive_path: "media.zip".to_string(),
            path: "Takeout/Google Photos/Album A/Photos/clip.mp4".to_string(),
        }];
        let sidecars = [
            SidecarCandidate {
                archive_path: "same-folder.zip".to_string(),
                path: "Takeout/Google Photos/Album A/Photos/photo.jpg.json".to_string(),
                title: Some("clip.mp4".to_string()),
            },
            SidecarCandidate {
                archive_path: "other-folder.zip".to_string(),
                path: "Takeout/Google Photos/Album A/Other/video.mov.json".to_string(),
                title: Some("clip.mp4".to_string()),
            },
        ];
        let matched = match_sidecars_for_media(&media, &sidecars);
        assert_eq!(
            matched[0].as_ref().map(|item| item.archive_path.as_str()),
            Some("other-folder.zip")
        );
    }

    #[test]
    fn sidecar_matching_prefers_folder_then_same_album_across_archives() {
        let media = MediaCandidate {
            archive_path: "media.zip".to_string(),
            path: "Takeout/Google Photos/Album A/Photos/IMG_1.jpg".to_string(),
        };
        let sidecars = [
            SidecarCandidate {
                archive_path: "same-folder.zip".to_string(),
                path: "Takeout/Google Photos/Album A/Photos/IMG_1.jpg.json".to_string(),
                title: None,
            },
            SidecarCandidate {
                archive_path: "other-folder.zip".to_string(),
                path: "Takeout/Google Photos/Album A/Other/IMG_1.jpg.json".to_string(),
                title: None,
            },
        ];
        let matched = match_sidecars_for_media(&[media.clone()], &sidecars);
        assert_eq!(
            matched[0].as_ref().map(|item| item.archive_path.as_str()),
            Some("same-folder.zip")
        );

        let fallback = match_sidecars_for_media(&[media], &sidecars[1..]);
        assert_eq!(
            fallback[0].as_ref().map(|item| item.archive_path.as_str()),
            Some("other-folder.zip")
        );

        let unrelated = [SidecarCandidate {
            archive_path: "other-folder.zip".to_string(),
            path: "Takeout/Google Photos/Album B/Other/IMG_1.jpg.json".to_string(),
            title: None,
        }];
        assert!(match_sidecars_for_media(
            &[MediaCandidate {
                archive_path: "media.zip".to_string(),
                path: "Takeout/Google Photos/Album A/Photos/IMG_1.jpg".to_string(),
            }],
            &unrelated
        )[0]
        .is_none());
    }

    #[test]
    fn edited_suffixes_are_marked_and_matched() {
        let matched = match_media_to_json(
            &["IMG_1-edited.jpg".to_string()],
            &["IMG_1.jpg.json".to_string()],
        );
        assert_eq!(matched[0].match_reason, MatchReason::EditedSuffixStripped);
        assert!(matched[0].is_edited);
    }

    #[test]
    fn live_pair_detects_shared_basename() {
        let matched = match_media_to_json(
            &[
                "Takeout/Google Fotos/Photos from 2024/IMG_1.jpg".to_string(),
                "Takeout/Google Fotos/Photos from 2024/IMG_1.mov".to_string(),
            ],
            &[],
        );
        assert!(matched.iter().all(|item| item.is_live_pair));
    }
}
