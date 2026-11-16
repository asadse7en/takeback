use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};

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
    pub embedded_video_verified: bool,
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
    exact: HashMap<String, Vec<usize>>,
    supplemental: HashMap<String, Vec<usize>>,
    truncated: HashMap<String, Vec<usize>>,
    numbered: HashMap<String, Vec<usize>>,
    edited: HashMap<String, Vec<usize>>,
    title: HashMap<String, Vec<usize>>,
}

pub fn match_sidecars(
    media_paths: &[String],
    sidecars: &[SidecarCandidate],
) -> Vec<Option<SidecarMatch>> {
    let mut index = SidecarIndex::default();
    for (candidate_index, candidate) in sidecars.iter().enumerate() {
        let key = normalize_sidecar_name(&candidate.path);
        let is_supplemental = candidate
            .path
            .to_lowercase()
            .contains(".supplemental-metadata");

        if is_supplemental {
            index
                .supplemental
                .entry(strip_supplemental_prefix(&key))
                .or_default()
                .push(candidate_index);
        } else {
            index
                .exact
                .entry(key.clone())
                .or_default()
                .push(candidate_index);
        }

        if char_count(&key) == 46 {
            index
                .truncated
                .entry(key.clone())
                .or_default()
                .push(candidate_index);
        }
        index
            .numbered
            .entry(strip_duplicate_suffix(&key))
            .or_default()
            .push(candidate_index);
        index
            .edited
            .entry(strip_edited_suffix(&key))
            .or_default()
            .push(candidate_index);
        if let Some(title) = candidate.title.as_deref() {
            index
                .title
                .entry(title.to_lowercase())
                .or_default()
                .push(candidate_index);
        }
    }

    media_paths
        .iter()
        .map(|media_path| {
            let media_name = file_name_key(media_path);
            let media_len = char_count(&media_name);
            let match_candidate =
                choose_candidate(index.exact.get(&media_name), media_path, sidecars)
                    .map(|candidate| (candidate, MatchReason::Exact))
                    .or_else(|| {
                        choose_candidate(index.supplemental.get(&media_name), media_path, sidecars)
                            .map(|candidate| (candidate, MatchReason::Supplemental))
                    })
                    .or_else(|| {
                        if media_len > 46 {
                            let prefix = take_chars(&media_name, 46);
                            choose_candidate(index.truncated.get(&prefix), media_path, sidecars)
                                .map(|candidate| (candidate, MatchReason::Truncated))
                        } else {
                            None
                        }
                    })
                    .or_else(|| {
                        let key = strip_duplicate_suffix(&media_name);
                        choose_candidate(index.numbered.get(&key), media_path, sidecars)
                            .map(|candidate| (candidate, MatchReason::NumberedDuplicate))
                    })
                    .or_else(|| {
                        let key = strip_edited_suffix(&media_name);
                        choose_candidate(index.edited.get(&key), media_path, sidecars)
                            .map(|candidate| (candidate, MatchReason::EditedSuffixStripped))
                    })
                    .or_else(|| {
                        let title_key = media_name.replace('_', " ").to_lowercase();
                        choose_candidate(index.title.get(&title_key), media_path, sidecars)
                            .or_else(|| {
                                let without_extension = title_key
                                    .strip_suffix(".jpg")
                                    .or_else(|| title_key.strip_suffix(".jpeg"));
                                without_extension.and_then(|key| {
                                    choose_candidate(index.title.get(key), media_path, sidecars)
                                })
                            })
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

fn file_name_key(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    let name = normalized.rsplit('/').next().unwrap_or(&normalized);
    let mut candidate = name.to_lowercase();
    if let Some(stripped) = candidate.strip_suffix(".json") {
        candidate = stripped.to_string();
    }
    if let Some(stripped) = candidate.strip_suffix(".supplemental-metadata") {
        candidate = stripped.to_string();
    }
    if let Some(index) = candidate.rfind('.') {
        candidate = candidate[..index].to_string();
    }
    candidate
}

fn normalize_sidecar_name(path: &str) -> String {
    file_name_key(path)
}

fn strip_supplemental_prefix(name: &str) -> String {
    let normalized = name.to_lowercase();
    normalized
        .strip_suffix(".supplemental-metadata")
        .map(str::to_string)
        .unwrap_or_else(|| normalized)
}

fn char_count(value: &str) -> usize {
    value.chars().count()
}

fn take_chars(value: &str, count: usize) -> String {
    value.chars().take(count).collect()
}

fn strip_duplicate_suffix(name: &str) -> String {
    let maybe = name.to_lowercase();
    if let Some(open) = maybe.rfind('(') {
        let tail = &maybe[open + 1..];
        if tail.ends_with(')') && tail[..tail.len() - 1].chars().all(|ch| ch.is_ascii_digit()) {
            return maybe[..open].to_string();
        }
    }
    maybe
}

fn strip_edited_suffix(name: &str) -> String {
    let candidate = name.to_lowercase();
    let mut cleaned = candidate.clone();
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

fn choose_candidate(
    matches: Option<&Vec<usize>>,
    media_path: &str,
    candidates: &[SidecarCandidate],
) -> Option<usize> {
    let media_parent = parent_path(media_path);
    matches.and_then(|indices| {
        indices
            .iter()
            .copied()
            .min_by_key(|index| usize::from(parent_path(&candidates[*index].path) != media_parent))
    })
}

fn parent_path(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    normalized
        .rsplit_once('/')
        .map_or_else(String::new, |(parent, _)| parent.to_lowercase())
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

pub fn album_membership(path: &str) -> Vec<String> {
    let normalized = path.replace('\\', "/");
    let parts: Vec<&str> = normalized
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    let photos_root = parts
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
        });
    let Some(root_index) = photos_root else {
        return Vec::new();
    };
    let Some(album) = parts.get(root_index + 1) else {
        return Vec::new();
    };
    let lower = album.to_lowercase();
    if lower.starts_with("photos from ")
        || matches!(lower.as_str(), "trash" | "archive" | "metadata.json")
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
            if candidate.embedded_video_verified {
                detection
                    .embedded_motion_photos
                    .push(candidate.path.clone());
            } else {
                detection.warnings.push(LivePairWarning {
                    paths: vec![candidate.path.clone()],
                    message: format!(
                        "Could not verify the embedded video in {}; kept the file separate.",
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
        let (Some(photo_time), Some(video_time)) = (photo.taken_timestamp, video.taken_timestamp)
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
        let Some(duration) = video.duration_seconds else {
            detection.warnings.push(LivePairWarning {
                paths: vec![photo.path.clone(), video.path.clone()],
                message: "A live photo name matched, but video duration could not be verified; kept both files separate.".to_string(),
            });
            continue;
        };
        if duration > 10.0 {
            detection.warnings.push(LivePairWarning {
                paths: vec![photo.path.clone(), video.path.clone()],
                message: "A matching video is longer than a typical live photo; kept both files separate.".to_string(),
            });
            continue;
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

fn live_path_key(path: &str) -> (String, String) {
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
        write_json(&title_sidecar, "IMG 5");

        let cases = [
            (
                "IMG_1.jpg".to_string(),
                vec![exact_path.to_string_lossy().to_string()],
                MatchReason::Exact,
            ),
            (
                "IMG_2.jpg".to_string(),
                vec![supplemental_path.to_string_lossy().to_string()],
                MatchReason::Supplemental,
            ),
            (
                format!("{media_base}.jpg"),
                vec![truncated_path.to_string_lossy().to_string()],
                MatchReason::Truncated,
            ),
            (
                duplicate_media.to_string_lossy().to_string(),
                vec![duplicate_sidecar.to_string_lossy().to_string()],
                MatchReason::NumberedDuplicate,
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
        assert_eq!(matched[0].match_reason, MatchReason::NumberedDuplicate);
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
