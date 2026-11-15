use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AlbumMode {
    Off,
    Move,
    Link,
    Copy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AlbumLocation {
    Top,
    Year,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DateFolderMode {
    Off,
    Year,
    Month,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FavoritesMode {
    Off,
    Move,
    Link,
    Copy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EditedMode {
    Off,
    Move,
    Link,
    Copy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TrashedMode {
    Skip,
    Include,
    OwnFolder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EditedVersions {
    Both,
    Originals,
    Edited,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MultiAlbumMode {
    First,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SameNameMode {
    Number,
    DateTime,
    Skip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NoJsonMode {
    Leave,
    Unsorted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TimeZoneMode {
    Local,
    Computer,
    Utc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NoDateMode {
    FileName,
    FileDate,
    Blank,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SpeedMode {
    Balanced,
    Gentle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PlacementKind {
    Original,
    Link,
    Copy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SplitByType {
    pub photos: bool,
    pub videos: bool,
    pub live: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannerSettings {
    #[serde(rename = "outputFolder")]
    pub output_root: Option<String>,
    pub album_mode: AlbumMode,
    pub album_loc: AlbumLocation,
    pub multi_album: MultiAlbumMode,
    pub date_folders: DateFolderMode,
    pub favorites_mode: FavoritesMode,
    pub edited_mode: EditedMode,
    pub archive_folder: bool,
    pub trashed: TrashedMode,
    pub edited_versions: EditedVersions,
    pub merge_live: bool,
    pub live_style: String,
    pub split_by_type: SplitByType,
    pub file_names: String,
    pub same_name: SameNameMode,
    pub ignore_duplicates: bool,
    pub no_json: NoJsonMode,
    pub time_zone: TimeZoneMode,
    pub no_date: NoDateMode,
    pub write_gps: bool,
    pub write_descriptions: bool,
    pub rate_favorites: bool,
    pub delete_archives_after: bool,
    pub speed: SpeedMode,
    pub dry_run: bool,
}

impl Default for PlannerSettings {
    fn default() -> Self {
        Self {
            output_root: None,
            album_mode: AlbumMode::Move,
            album_loc: AlbumLocation::Top,
            multi_album: MultiAlbumMode::First,
            date_folders: DateFolderMode::Month,
            favorites_mode: FavoritesMode::Link,
            edited_mode: EditedMode::Move,
            archive_folder: true,
            trashed: TrashedMode::Skip,
            edited_versions: EditedVersions::Both,
            merge_live: true,
            live_style: "separate".to_string(),
            split_by_type: SplitByType { photos: false, videos: false, live: false },
            file_names: "keep".to_string(),
            same_name: SameNameMode::Number,
            ignore_duplicates: true,
            no_json: NoJsonMode::Unsorted,
            time_zone: TimeZoneMode::Local,
            no_date: NoDateMode::FileName,
            write_gps: true,
            write_descriptions: true,
            rate_favorites: true,
            delete_archives_after: false,
            speed: SpeedMode::Balanced,
            dry_run: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaItem {
    pub source: String,
    pub filename: String,
    pub album: Option<String>,
    pub date: Option<String>,
    pub is_photo: bool,
    pub is_video: bool,
    pub is_live: bool,
    pub favorite: bool,
    pub archived: bool,
    pub trashed: bool,
    pub edited: bool,
    pub has_json: bool,
    pub duplicate_of: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Placement {
    pub src: String,
    pub dest: String,
    pub kind: PlacementKind,
    pub tags: Vec<String>,
}

pub fn preview(settings: &PlannerSettings) -> Vec<String> {
    let sample = vec![
        MediaItem {
            source: "sample/photos/IMG_0134.jpg".to_string(),
            filename: "IMG_0134.jpg".to_string(),
            album: Some("Paris trip".to_string()),
            date: Some("2024-05-16".to_string()),
            is_photo: true,
            is_video: false,
            is_live: false,
            favorite: true,
            archived: false,
            trashed: false,
            edited: false,
            has_json: true,
            duplicate_of: None,
        },
        MediaItem {
            source: "sample/photos/IMG_0135.mp4".to_string(),
            filename: "IMG_0135.mp4".to_string(),
            album: None,
            date: Some("2024-05-16".to_string()),
            is_photo: false,
            is_video: true,
            is_live: false,
            favorite: false,
            archived: false,
            trashed: false,
            edited: false,
            has_json: true,
            duplicate_of: None,
        },
    ];

    plan(&sample, settings)
        .into_iter()
        .map(|placement| placement.dest)
        .collect()
}

pub fn plan(items: &[MediaItem], settings: &PlannerSettings) -> Vec<Placement> {
    let output_root = effective_output_root(settings.output_root.as_deref());
    let mut placements = Vec::new();
    let mut seen_names: HashMap<String, usize> = HashMap::new();

    for item in items {
        if item.trashed && matches!(settings.trashed, TrashedMode::Skip) {
            continue;
        }

        if settings.ignore_duplicates && item.duplicate_of.is_some() {
            continue;
        }

        if matches!(settings.edited_versions, EditedVersions::Originals) && item.edited {
            continue;
        }
        if matches!(settings.edited_versions, EditedVersions::Edited) && !item.edited {
            continue;
        }

        if item.trashed && matches!(settings.trashed, TrashedMode::OwnFolder) {
            placements.push(original_placement(item, &output_root, &["Trash".to_string()], settings, &mut seen_names));
            continue;
        }
        if !item.has_json {
            if matches!(settings.no_json, NoJsonMode::Leave) {
                continue;
            }
            placements.push(original_placement(item, &output_root, &["Unsorted".to_string()], settings, &mut seen_names));
            continue;
        }

        let mut main_folders = if item.archived && settings.archive_folder {
            vec!["Archive".to_string()]
        } else {
            normal_folders(item, settings)
        };
        let mut main_is_flat = item.archived && settings.archive_folder;

        if item.favorite && matches!(settings.favorites_mode, FavoritesMode::Move) {
            main_folders = vec!["Favorites".to_string()];
            main_is_flat = true;
        }
        if item.edited && matches!(settings.edited_mode, EditedMode::Move) {
            main_folders = vec!["Edited".to_string()];
            main_is_flat = true;
        }
        if !main_is_flat {
            append_type_folders(&mut main_folders, item, settings);
        }

        placements.push(original_placement(item, &output_root, &main_folders, settings, &mut seen_names));

        if matches!(settings.album_mode, AlbumMode::Link | AlbumMode::Copy) {
            if let Some(album_name) = item.album.as_ref() {
                let mut album_folders = album_folders(album_name, item.date.as_deref(), settings.album_loc);
                append_type_folders(&mut album_folders, item, settings);
                let kind = if matches!(settings.album_mode, AlbumMode::Link) {
                    PlacementKind::Link
                } else {
                    PlacementKind::Copy
                };
                placements.push(extra_placement(
                    item,
                    &output_root,
                    &album_folders,
                    kind,
                    settings,
                    &mut seen_names,
                ));
            }
        }

        if matches!(settings.favorites_mode, FavoritesMode::Link | FavoritesMode::Copy) && item.favorite {
            let kind = if matches!(settings.favorites_mode, FavoritesMode::Link) {
                PlacementKind::Link
            } else {
                PlacementKind::Copy
            };
            placements.push(extra_placement(
                item,
                &output_root,
                &["Favorites".to_string()],
                kind,
                settings,
                &mut seen_names,
            ));
        }

        if matches!(settings.edited_mode, EditedMode::Link | EditedMode::Copy) && item.edited {
            let kind = if matches!(settings.edited_mode, EditedMode::Link) {
                PlacementKind::Link
            } else {
                PlacementKind::Copy
            };
            placements.push(extra_placement(
                item,
                &output_root,
                &["Edited".to_string()],
                kind,
                settings,
                &mut seen_names,
            ));
        }
    }

    placements
}

fn normal_folders(item: &MediaItem, settings: &PlannerSettings) -> Vec<String> {
    match (item.album.as_deref(), settings.album_mode) {
        (Some(album), AlbumMode::Move) => album_folders(album, item.date.as_deref(), settings.album_loc),
        _ => build_date_path(item.date.as_deref(), settings.date_folders)
            .split('/')
            .filter(|part| !part.is_empty())
            .map(str::to_string)
            .collect(),
    }
}

fn album_folders(album: &str, date: Option<&str>, location: AlbumLocation) -> Vec<String> {
    let album = sanitize_file_part(album);
    match location {
        AlbumLocation::Top => vec![album],
        AlbumLocation::Year => vec![parent_year(date), album],
    }
}

fn append_type_folders(folders: &mut Vec<String>, item: &MediaItem, settings: &PlannerSettings) {
    if settings.split_by_type.photos && item.is_photo {
        folders.push("Photos".to_string());
    }
    if settings.split_by_type.videos && item.is_video {
        folders.push("Videos".to_string());
    }
    if settings.split_by_type.live && item.is_live {
        folders.push("Live photos".to_string());
    }
}

fn original_placement(
    item: &MediaItem,
    output_root: &str,
    folders: &[String],
    settings: &PlannerSettings,
    seen_names: &mut HashMap<String, usize>,
) -> Placement {
    let directory = folder_path(output_root, folders);
    let filename = unique_path_name(
        &directory,
        &item.filename,
        seen_names,
        settings.same_name,
        settings.file_names.as_str(),
    );
    Placement {
        src: item.source.clone(),
        dest: format!("{}/{}", directory, filename),
        kind: PlacementKind::Original,
        tags: vec![],
    }
}

fn extra_placement(
    item: &MediaItem,
    output_root: &str,
    folders: &[String],
    kind: PlacementKind,
    settings: &PlannerSettings,
    seen_names: &mut HashMap<String, usize>,
) -> Placement {
    let directory = folder_path(output_root, folders);
    let filename = unique_path_name(
        &directory,
        &item.filename,
        seen_names,
        settings.same_name,
        settings.file_names.as_str(),
    );
    let label = match kind {
        PlacementKind::Link => "Link",
        PlacementKind::Copy => "Copy",
        PlacementKind::Original => "",
    };
    Placement {
        src: item.source.clone(),
        dest: format!("{}/{}", directory, filename),
        kind,
        tags: vec![label.to_string()],
    }
}

fn folder_path(output_root: &str, folders: &[String]) -> String {
    if folders.is_empty() {
        output_root.to_string()
    } else {
        format!("{}/{}", output_root, folders.join("/"))
    }
}

fn effective_output_root(output_root: Option<&str>) -> String {
    match output_root {
        Some(path) if !path.trim().is_empty() => {
            let trimmed = path.trim();
            if trimmed.ends_with("Photo Library") {
                trimmed.to_string()
            } else {
                format!("{}/Photo Library", trimmed.trim_end_matches(['/', '\\']))
            }
        }
        _ => "Pictures/Photo Library".to_string(),
    }
}

fn build_date_path(date: Option<&str>, mode: DateFolderMode) -> String {
    match mode {
        DateFolderMode::Off => "".to_string(),
        DateFolderMode::Year => format!("{}", parent_year(date)),
        DateFolderMode::Month => match date {
            Some(value) => {
                let parts: Vec<&str> = value.split('-').collect();
                if parts.len() >= 2 {
                    format!("{}/{}", parts[0], parts[1])
                } else {
                    format!("{}", parent_year(Some(value)))
                }
            }
            None => "Unknown".to_string(),
        },
    }
}

fn parent_year(date: Option<&str>) -> String {
    match date {
        Some(value) if value.len() >= 4 => value[..4].to_string(),
        _ => "Unknown".to_string(),
    }
}

fn unique_path_name(
    directory: &str,
    filename: &str,
    seen_names: &mut HashMap<String, usize>,
    same_name: SameNameMode,
    file_names: &str,
) -> String {
    let clean_name = sanitize_file_part(&rename_from_setting(filename, file_names));
    let key = format!("{}::{}", directory.to_ascii_lowercase(), clean_name.to_ascii_lowercase());
    let count = seen_names.entry(key).or_insert(0);
    match same_name {
        SameNameMode::Skip => {
            *count += 1;
            if *count == 1 { clean_name } else { format!("{}_{}", clean_name, *count) }
        }
        SameNameMode::Number => {
            if *count == 0 {
                *count += 1;
                clean_name
            } else {
                let stem = clean_name.rsplit_once('.').map_or(clean_name.as_str(), |(base, ext)| base);
                let ext = clean_name.rsplit_once('.').map_or("", |(_, ext)| ext);
                *count += 1;
                if ext.is_empty() {
                    format!("{}-{}", stem, *count)
                } else {
                    format!("{}-{}.{}", stem, *count, ext)
                }
            }
        }
        SameNameMode::DateTime => {
            let timestamp = if file_names == "dateTime" { "2024-05-16_120000" } else { "2024-05-16" };
            *count += 1;
            if clean_name.contains('.') {
                let (stem, ext) = clean_name.rsplit_once('.').unwrap();
                format!("{}-{}_{}.{}", stem, timestamp, *count, ext)
            } else {
                format!("{}-{}_{}", clean_name, timestamp, *count)
            }
        }
    }
}

fn rename_from_setting(filename: &str, file_names: &str) -> String {
    match file_names {
        "dateTime" => format!("2024-05-16_120000_{}", filename),
        "dateOriginal" => format!("2024-05-16_{}", filename),
        "custom" => filename.to_string(),
        _ => filename.to_string(),
    }
}

fn sanitize_file_part(part: &str) -> String {
    let mut cleaned: String = part
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            _ => c,
        })
        .collect();

    cleaned = cleaned.trim_matches(|c: char| c == '.' || c == ' ' || c == '\0').to_string();
    if cleaned.is_empty() {
        cleaned = "photo".to_string();
    }

    let upper = cleaned.to_ascii_uppercase();
    let invalid_device = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
        "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if invalid_device.contains(&upper.as_str()) {
        cleaned = format!("_{}", cleaned);
    }

    if cleaned.len() > 255 {
        cleaned.truncate(255);
    }

    cleaned
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn sample_item(filename: &str) -> MediaItem {
        MediaItem {
            source: format!("sample/{filename}"),
            filename: filename.to_string(),
            album: Some("Paris trip".to_string()),
            date: Some("2024-05-16".to_string()),
            is_photo: true,
            is_video: false,
            is_live: false,
            favorite: false,
            archived: false,
            trashed: false,
            edited: false,
            has_json: true,
            duplicate_of: None,
        }
    }

    #[test]
    fn output_root_uses_photo_library_when_none() {
        let settings = PlannerSettings::default();
        let root = effective_output_root(settings.output_root.as_deref());
        assert_eq!(root, "Pictures/Photo Library");
    }

    #[test]
    fn settings_serialize_using_frontend_names_and_values() {
        let value = serde_json::to_value(PlannerSettings::default()).unwrap();
        assert_eq!(value["outputFolder"], serde_json::Value::Null);
        assert_eq!(value["albumMode"], "move");
        assert_eq!(value["multiAlbum"], "first");
        assert_eq!(value["dateFolders"], "month");
    }

    #[test]
    fn output_root_uses_library_child_for_chosen_folder() {
        let settings = PlannerSettings { output_root: Some("/Volumes/Archive".to_string()), ..PlannerSettings::default() };
        assert_eq!(effective_output_root(settings.output_root.as_deref()), "/Volumes/Archive/Photo Library");
    }

    #[test]
    fn sanitizes_windows_illegal_chars_and_reserved_names() {
        assert_eq!(sanitize_file_part("<>:\\|?*"), "_______");
        assert_eq!(sanitize_file_part("NUL"), "_NUL");
        assert_eq!(sanitize_file_part("  final.jpg  "), "final.jpg");
    }

    #[test]
    fn case_insensitive_duplicate_names_are_resolved() {
        let mut seen: HashMap<String, usize> = HashMap::new();
        let first = unique_path_name("/tmp", "IMG_0001.jpg", &mut seen, SameNameMode::Number, "keep");
        let second = unique_path_name("/tmp", "img_0001.jpg", &mut seen, SameNameMode::Number, "keep");
        assert_ne!(first, second);
        assert!(second.contains("-2"));
    }

    #[test]
    fn album_link_with_favorites_copy_and_video_split() {
        let settings = PlannerSettings {
            album_mode: AlbumMode::Link,
            favorites_mode: FavoritesMode::Copy,
            split_by_type: SplitByType { photos: false, videos: true, live: false },
            ..PlannerSettings::default()
        };
        let items = vec![
            MediaItem {
                source: "a.mov".to_string(),
                filename: "a.mov".to_string(),
                album: Some("Paris trip".to_string()),
                date: Some("2024-05-16".to_string()),
                is_photo: false,
                is_video: true,
                is_live: false,
                favorite: true,
                archived: false,
                trashed: false,
                edited: false,
                has_json: true,
                duplicate_of: None,
            }
        ];

        let placements = plan(&items, &settings);
        assert!(placements.iter().any(|p| p.dest.ends_with("/2024/05/Videos/a.mov")));
        assert!(placements.iter().any(|p| p.dest.ends_with("/Paris trip/Videos/a.mov") && p.kind == PlacementKind::Link));
        assert!(placements.iter().any(|p| {
            p.dest.ends_with("/Favorites/a.mov")
                && p.kind == PlacementKind::Copy
                && p.tags.contains(&"Copy".to_string())
        }));
    }

    #[test]
    fn special_folders_are_flat_even_when_type_splitting_is_enabled() {
        let settings = PlannerSettings {
            favorites_mode: FavoritesMode::Move,
            edited_mode: EditedMode::Move,
            trashed: TrashedMode::OwnFolder,
            split_by_type: SplitByType { photos: true, videos: true, live: true },
            ..PlannerSettings::default()
        };
        let mut archived = sample_item("archived.jpg");
        archived.archived = true;
        let mut favorite = sample_item("favorite.jpg");
        favorite.favorite = true;
        let mut edited = sample_item("edited.jpg");
        edited.edited = true;
        let mut trashed = sample_item("trashed.jpg");
        trashed.trashed = true;
        let mut unsorted = sample_item("unsorted.jpg");
        unsorted.has_json = false;

        let placements = plan(&[archived, favorite, edited, trashed, unsorted], &settings);
        let destinations: HashSet<String> = placements.into_iter().map(|p| p.dest).collect();
        let root = "Pictures/Photo Library";
        for (folder, filename) in [
            ("Archive", "archived.jpg"),
            ("Favorites", "favorite.jpg"),
            ("Edited", "edited.jpg"),
            ("Trash", "trashed.jpg"),
            ("Unsorted", "unsorted.jpg"),
        ] {
            assert!(destinations.contains(&format!("{root}/{folder}/{filename}")));
        }
    }

    #[test]
    fn favorite_link_stays_in_date_path_and_gets_a_flat_link_placement() {
        let settings = PlannerSettings {
            album_mode: AlbumMode::Off,
            favorites_mode: FavoritesMode::Link,
            split_by_type: SplitByType { photos: true, videos: false, live: false },
            ..PlannerSettings::default()
        };
        let mut item = sample_item("favorite.jpg");
        item.favorite = true;

        let placements = plan(&[item], &settings);
        assert!(placements.iter().any(|p| {
            p.dest.ends_with("/2024/05/Photos/favorite.jpg")
                && p.kind == PlacementKind::Original
        }));
        assert!(placements.iter().any(|p| {
            p.dest.ends_with("/Favorites/favorite.jpg")
                && p.kind == PlacementKind::Link
                && p.tags == ["Link"]
        }));
    }

    #[test]
    fn moved_albums_are_top_level_by_default_or_nested_only_by_year() {
        let item = sample_item("photo.jpg");
        let top_level = plan(&[item.clone()], &PlannerSettings::default());
        assert!(top_level.iter().any(|p| p.dest.ends_with("/Paris trip/photo.jpg")));

        let year_level = plan(
            &[item],
            &PlannerSettings {
                album_loc: AlbumLocation::Year,
                ..PlannerSettings::default()
            },
        );
        assert!(year_level.iter().any(|p| p.dest.ends_with("/2024/Paris trip/photo.jpg")));
    }

    #[test]
    fn linked_albums_use_year_without_nesting_under_months() {
        let placements = plan(
            &[sample_item("photo.jpg")],
            &PlannerSettings {
                album_mode: AlbumMode::Link,
                album_loc: AlbumLocation::Year,
                ..PlannerSettings::default()
            },
        );
        assert!(placements.iter().any(|p| {
            p.dest.ends_with("/2024/Paris trip/photo.jpg")
                && p.kind == PlacementKind::Link
        }));
        assert!(placements.iter().any(|p| {
            p.dest.ends_with("/2024/05/photo.jpg")
                && p.kind == PlacementKind::Original
        }));
    }

    #[test]
    fn edited_link_stays_flat_and_preserves_normal_placement() {
        let settings = PlannerSettings {
            album_mode: AlbumMode::Off,
            edited_mode: EditedMode::Link,
            split_by_type: SplitByType { photos: true, videos: false, live: false },
            ..PlannerSettings::default()
        };
        let mut item = sample_item("edited.jpg");
        item.edited = true;

        let placements = plan(&[item], &settings);
        assert!(placements.iter().any(|p| {
            p.dest.ends_with("/2024/05/Photos/edited.jpg")
                && p.kind == PlacementKind::Original
        }));
        assert!(placements.iter().any(|p| {
            p.dest.ends_with("/Edited/edited.jpg")
                && p.kind == PlacementKind::Link
                && p.tags.contains(&"Link".to_string())
        }));
    }
}
