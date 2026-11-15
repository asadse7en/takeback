use std::collections::HashMap;
use std::fs;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
pub enum MatchReason {
    Exact,
    Supplemental,
    Truncated,
    NumberedDuplicate,
    EditedSuffixStripped,
    Title,
    NoJson,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct JsonFields {
    pub title: Option<String>,
    pub description: Option<String>,
    pub photo_taken_time: Option<i64>,
    pub creation_time: Option<i64>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub altitude: Option<f64>,
    pub people: Vec<String>,
    pub favorited: bool,
    pub archived: bool,
    pub trashed: bool,
    pub url: Option<String>,
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

pub fn match_media_to_json(media_paths: &[String], sidecar_paths: &[String]) -> Vec<MediaMatch> {
    let mut sidecar_entries: HashMap<String, Vec<String>> = HashMap::new();
    for sidecar_path in sidecar_paths {
        let key = normalize_sidecar_name(sidecar_path);
        sidecar_entries.entry(key).or_default().push(sidecar_path.clone());
    }

    let live_pairs = detect_live_pairs(media_paths);

    media_paths
        .iter()
        .map(|media_path| {
            let media_name = file_name_key(media_path);
            let mut reason = MatchReason::NoJson;
            let mut matched_json: Option<(String, JsonFields)> = None;

            for candidate in sidecar_paths.iter() {
                let candidate_name = normalize_sidecar_name(candidate);
                let is_supplemental = candidate.to_ascii_lowercase().contains(".supplemental-metadata");
                if is_exact_match(&media_name, candidate_name.as_str(), is_supplemental) {
                    reason = MatchReason::Exact;
                    matched_json = read_sidecar_json(candidate);
                    break;
                }
                if is_supplemental_match(&media_name, candidate_name.as_str(), candidate) {
                    reason = MatchReason::Supplemental;
                    matched_json = read_sidecar_json(candidate);
                    break;
                }
            }

            if matched_json.is_none() {
                for candidate in sidecar_paths.iter() {
                    if is_truncated_match(&media_name, normalize_sidecar_name(candidate).as_str()) {
                        reason = MatchReason::Truncated;
                        matched_json = read_sidecar_json(candidate);
                        break;
                    }
                }
            }

            if matched_json.is_none() {
                for candidate in sidecar_paths.iter() {
                    if is_numbered_duplicate_match(&media_name, normalize_sidecar_name(candidate).as_str()) {
                        reason = MatchReason::NumberedDuplicate;
                        matched_json = read_sidecar_json(candidate);
                        break;
                    }
                }
            }

            if matched_json.is_none() {
                for candidate in sidecar_paths.iter() {
                    if is_edited_suffix_match(&media_name, normalize_sidecar_name(candidate).as_str()) {
                        reason = MatchReason::EditedSuffixStripped;
                        matched_json = read_sidecar_json(candidate);
                        break;
                    }
                }
            }

            if matched_json.is_none() {
                for candidate in sidecar_paths.iter() {
                    if matches_title(&media_name, candidate) {
                        reason = MatchReason::Title;
                        matched_json = read_sidecar_json(candidate);
                        break;
                    }
                }
            }

            let (json_path, json) = matched_json
                .map(|(path, fields)| (Some(path), Some(fields)))
                .unwrap_or((None, None));

            MediaMatch {
                media_path: media_path.clone(),
                json_path: json_path,
                json,
                match_reason: reason,
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
    let mut candidate = name.to_ascii_lowercase();
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

fn is_exact_match(media_name: &str, sidecar_name: &str, supplemental: bool) -> bool {
    !supplemental && media_name == sidecar_name
}

fn is_supplemental_match(media_name: &str, sidecar_name: &str, sidecar_path: &str) -> bool {
    sidecar_path.to_ascii_lowercase().contains("supplemental-metadata")
        && media_name == strip_supplemental_prefix(sidecar_name)
}

fn strip_supplemental_prefix(name: &str) -> String {
    let normalized = name.to_ascii_lowercase();
    normalized
        .strip_suffix(".supplemental-metadata")
        .map(str::to_string)
        .unwrap_or_else(|| normalized)
}

fn is_truncated_match(media_name: &str, sidecar_name: &str) -> bool {
    let media = media_name.trim();
    let sidecar = sidecar_name.trim();
    if media.len() <= 46 || sidecar.len() >= media.len() {
        return false;
    }
    let expected = &media[..46];
    sidecar == expected
}

fn strip_duplicate_suffix(name: &str) -> String {
    let maybe = name.to_ascii_lowercase();
    if let Some(open) = maybe.rfind('(') {
        let tail = &maybe[open + 1..];
        if tail.ends_with(')') && tail[..tail.len() - 1].chars().all(|ch| ch.is_ascii_digit()) {
            return maybe[..open].to_string();
        }
    }
    maybe
}

fn is_numbered_duplicate_match(media_name: &str, sidecar_name: &str) -> bool {
    strip_duplicate_suffix(media_name) == strip_duplicate_suffix(sidecar_name)
}

fn strip_edited_suffix(name: &str) -> String {
    let candidate = name.to_ascii_lowercase();
    let mut cleaned = candidate.clone();
    for suffix in EDITED_SUFFIXES {
        if cleaned.ends_with(suffix) {
            cleaned = cleaned.strip_suffix(suffix).unwrap_or(&cleaned).trim().to_string();
            break;
        }
        if let Some(index) = cleaned.rfind(suffix) {
            let before = &cleaned[..index];
            cleaned = before.trim().to_string();
            break;
        }
    }
    cleaned
}

fn is_edited_suffix_match(media_name: &str, sidecar_name: &str) -> bool {
    strip_edited_suffix(media_name) == strip_edited_suffix(sidecar_name)
}

fn matches_title(media_name: &str, sidecar_path: &str) -> bool {
    let Some((_, fields)) = read_sidecar_json(sidecar_path) else {
        return false;
    };
    let title = fields.title.as_deref().unwrap_or_default();
    let alt_title = media_name.replace("_", " ");
    let title_key = title.to_ascii_lowercase();
    let match_key = alt_title.to_ascii_lowercase();
    title_key == match_key || title_key == match_key.replace(".jpg", "").replace(".jpeg", "")
}

fn read_sidecar_json(path: &str) -> Option<(String, JsonFields)> {
    let raw = fs::read(path).ok()?;
    let json: Value = serde_json::from_slice(&raw).ok()?;
    let title = json.get("title").and_then(Value::as_str).map(str::to_string);
    let description = json.get("description").and_then(Value::as_str).map(str::to_string);
    let photo_taken_time = json
        .get("photoTakenTime")
        .and_then(|v| v.get("timestamp"))
        .and_then(Value::as_i64);
    let creation_time = json
        .get("creationTime")
        .and_then(|v| v.get("timestamp"))
        .and_then(Value::as_i64);
    let geo = json.get("geoData").or_else(|| json.get("geoDataExif"));
    let latitude = geo.and_then(|v| v.get("latitude")).and_then(Value::as_f64);
    let longitude = geo.and_then(|v| v.get("longitude")).and_then(Value::as_f64);
    let altitude = geo.and_then(|v| v.get("altitude")).and_then(Value::as_f64);
    let people = json
        .get("people")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("name").and_then(Value::as_str).map(str::to_string))
        .collect();
    let favorited = json.get("favorited").and_then(Value::as_bool).unwrap_or(false);
    let archived = json.get("archived").and_then(Value::as_bool).unwrap_or(false);
    let trashed = json.get("trashed").and_then(Value::as_bool).unwrap_or(false);
    let url = json.get("url").and_then(Value::as_str).map(str::to_string);

    Some((
        path.to_string(),
        JsonFields {
            title,
            description,
            photo_taken_time,
            creation_time,
            latitude,
            longitude,
            altitude,
            people,
            favorited,
            archived,
            trashed,
            url,
        },
    ))
}

fn is_edited_path(path: &str) -> bool {
    let name = path.to_ascii_lowercase();
    for suffix in EDITED_SUFFIXES {
        if name.contains(suffix) {
            return true;
        }
    }
    false
}

fn album_membership(path: &str) -> Vec<String> {
    let normalized = path.replace('\\', "/");
    normalized
        .split('/')
        .filter(|part| !part.is_empty() && part.to_ascii_lowercase().contains("album"))
        .map(str::to_string)
        .collect()
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
        by_basename.entry(base.to_ascii_lowercase()).or_default().push(item.clone());
    }

    let mut live = std::collections::HashSet::new();
    for items in by_basename.values() {
        let has_video = items.iter().any(|item| {
            let lower = item.to_ascii_lowercase();
            lower.ends_with(".mp4") || lower.ends_with(".mov") || lower.ends_with(".m4v")
        });
        let has_photo = items.iter().any(|item| {
            let lower = item.to_ascii_lowercase();
            lower.ends_with(".jpg") || lower.ends_with(".jpeg") || lower.ends_with(".heic") || lower.ends_with(".png")
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
                vec![edited_media.with_file_name("IMG_4.jpg.json").to_string_lossy().to_string()],
                MatchReason::EditedSuffixStripped,
            ),
            (
                title_media.to_string_lossy().to_string(),
                vec![title_sidecar.to_string_lossy().to_string()],
                MatchReason::Title,
            ),
            (
                "IMG_999.jpg".to_string(),
                vec![],
                MatchReason::NoJson,
            ),
        ];

        for (media, sidecars, expected) in cases {
            let matched = match_media_to_json(&[media.clone()], &sidecars);
            assert_eq!(matched.len(), 1, "case for {media}");
            assert_eq!(matched[0].match_reason, expected, "unexpected reason for {media}");
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
