use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;

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

fn is_edited_path(path: &str) -> bool {
    let name = path.to_lowercase();
    for suffix in EDITED_SUFFIXES {
        if name.contains(suffix) {
            return true;
        }
    }
    false
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

fn detect_live_pairs(media_paths: &[String]) -> std::collections::HashSet<String> {
    let mut by_basename: HashMap<String, Vec<String>> = HashMap::new();
    for item in media_paths {
        let path = item.replace('\\', "/");
        let file_name = path.rsplit('/').next().unwrap_or(&path);
        let base = file_name
            .rsplit_once('.')
            .map(|(stem, _)| stem)
            .unwrap_or(file_name);
        by_basename
            .entry(base.to_ascii_lowercase())
            .or_default()
            .push(item.clone());
    }

    let mut live = std::collections::HashSet::new();
    for items in by_basename.values() {
        let has_video = items.iter().any(|item| {
            let lower = item.to_ascii_lowercase();
            lower.ends_with(".mp4") || lower.ends_with(".mov") || lower.ends_with(".m4v")
        });
        let has_photo = items.iter().any(|item| {
            let lower = item.to_ascii_lowercase();
            lower.ends_with(".jpg")
                || lower.ends_with(".jpeg")
                || lower.ends_with(".heic")
                || lower.ends_with(".png")
        });
        if has_video && has_photo {
            for item in items {
                live.insert(item.clone());
            }
        }
    }
    live
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
