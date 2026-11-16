use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::metadata::TakeoutMetadata;
use crate::settings::{self, Settings};

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
    YearMonth,
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
    pub custom_name_pattern: String,
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
            date_folders: DateFolderMode::YearMonth,
            favorites_mode: FavoritesMode::Link,
            edited_mode: EditedMode::Move,
            archive_folder: true,
            trashed: TrashedMode::Skip,
            edited_versions: EditedVersions::Both,
            merge_live: true,
            live_style: "separate".to_string(),
            split_by_type: SplitByType {
                photos: false,
                videos: false,
                live: false,
            },
            file_names: "keep".to_string(),
            custom_name_pattern: String::new(),
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaItem {
    pub source: String,
    pub filename: String,
    pub album: Option<String>,
    #[serde(default)]
    pub additional_albums: Vec<String>,
    pub date: Option<String>,
    #[serde(default)]
    pub size_bytes: u64,
    pub is_photo: bool,
    pub is_video: bool,
    pub is_live: bool,
    #[serde(default)]
    pub live_pair_partner: Option<String>,
    #[serde(default)]
    pub live_content_identifier: Option<String>,
    #[serde(default)]
    pub live_pair_video: bool,
    #[serde(default)]
    pub embedded_motion_video: bool,
    #[serde(default)]
    pub live_warning: Option<String>,
    pub favorite: bool,
    pub archived: bool,
    pub trashed: bool,
    pub edited: bool,
    pub has_json: bool,
    pub duplicate_of: Option<String>,
    pub metadata: Option<TakeoutMetadata>,
}

impl PlannerSettings {
    pub fn from_preferences(settings: &Settings, resolved_output_root: Option<&Path>) -> Self {
        let output_root = resolved_output_root
            .map(|path| path.to_string_lossy().into_owned())
            .or_else(|| settings.output_folder.clone());
        Self {
            output_root,
            album_mode: match settings.album_mode {
                settings::FolderMode::Off => AlbumMode::Off,
                settings::FolderMode::Move => AlbumMode::Move,
                settings::FolderMode::Link => AlbumMode::Link,
                settings::FolderMode::Copy => AlbumMode::Copy,
            },
            album_loc: match settings.album_loc {
                settings::AlbumLocation::Top => AlbumLocation::Top,
                settings::AlbumLocation::Year => AlbumLocation::Year,
            },
            multi_album: match settings.multi_album {
                settings::MultiAlbumMode::First => MultiAlbumMode::First,
                settings::MultiAlbumMode::All => MultiAlbumMode::All,
            },
            date_folders: match settings.date_folders {
                settings::DateFolderMode::Off => DateFolderMode::Off,
                settings::DateFolderMode::Year => DateFolderMode::Year,
                settings::DateFolderMode::YearMonth => DateFolderMode::YearMonth,
            },
            favorites_mode: match settings.favorites_mode {
                settings::FolderMode::Off => FavoritesMode::Off,
                settings::FolderMode::Move => FavoritesMode::Move,
                settings::FolderMode::Link => FavoritesMode::Link,
                settings::FolderMode::Copy => FavoritesMode::Copy,
            },
            edited_mode: match settings.edited_mode {
                settings::FolderMode::Off => EditedMode::Off,
                settings::FolderMode::Move => EditedMode::Move,
                settings::FolderMode::Link => EditedMode::Link,
                settings::FolderMode::Copy => EditedMode::Copy,
            },
            archive_folder: settings.archive_folder,
            trashed: match settings.trashed {
                settings::TrashedMode::Skip => TrashedMode::Skip,
                settings::TrashedMode::Include => TrashedMode::Include,
                settings::TrashedMode::OwnFolder => TrashedMode::OwnFolder,
            },
            edited_versions: match settings.edited_versions {
                settings::EditedVersions::Both => EditedVersions::Both,
                settings::EditedVersions::Originals => EditedVersions::Originals,
                settings::EditedVersions::Edited => EditedVersions::Edited,
            },
            merge_live: settings.merge_live,
            live_style: match settings.live_style {
                crate::livephoto::LivePhotoStyle::Separate => "separate",
                crate::livephoto::LivePhotoStyle::IphonePair => "iphonePair",
                crate::livephoto::LivePhotoStyle::AndroidMotionPhoto => "androidMotionPhoto",
            }
            .to_string(),
            split_by_type: SplitByType {
                photos: settings.split_by_type.photos,
                videos: settings.split_by_type.videos,
                live: settings.split_by_type.live,
            },
            file_names: match settings.file_names {
                settings::FileNamesMode::Keep => "keep",
                settings::FileNamesMode::DateTime => "dateTime",
                settings::FileNamesMode::DateOriginal => "dateOriginal",
                settings::FileNamesMode::Custom => "custom",
            }
            .to_string(),
            custom_name_pattern: settings.custom_name_pattern.clone(),
            same_name: match settings.same_name {
                settings::SameNameMode::Number => SameNameMode::Number,
                settings::SameNameMode::DateTime => SameNameMode::DateTime,
                settings::SameNameMode::Skip => SameNameMode::Skip,
            },
            ignore_duplicates: settings.ignore_duplicates,
            no_json: match settings.no_json {
                settings::NoJsonMode::Leave => NoJsonMode::Leave,
                settings::NoJsonMode::Unsorted => NoJsonMode::Unsorted,
            },
            time_zone: match settings.time_zone {
                settings::TimeZoneMode::Local => TimeZoneMode::Local,
                settings::TimeZoneMode::Computer => TimeZoneMode::Computer,
                settings::TimeZoneMode::Utc => TimeZoneMode::Utc,
            },
            no_date: match settings.no_date {
                settings::NoDateMode::FileName => NoDateMode::FileName,
                settings::NoDateMode::FileDate => NoDateMode::FileDate,
                settings::NoDateMode::Blank => NoDateMode::Blank,
            },
            write_gps: settings.write_gps,
            write_descriptions: settings.write_descriptions,
            rate_favorites: settings.rate_favorites,
            delete_archives_after: settings.delete_archives_after,
            speed: match settings.speed {
                settings::SpeedMode::Balanced => SpeedMode::Balanced,
                settings::SpeedMode::Gentle => SpeedMode::Gentle,
            },
            dry_run: settings.dry_run,
        }
    }
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
            additional_albums: Vec::new(),
            date: Some("2024-05-16".to_string()),
            size_bytes: 0,
            is_photo: true,
            is_video: false,
            is_live: false,
            live_pair_partner: None,
            live_content_identifier: None,
            live_pair_video: false,
            embedded_motion_video: false,
            live_warning: None,
            favorite: true,
            archived: false,
            trashed: false,
            edited: false,
            has_json: true,
            duplicate_of: None,
            metadata: None,
        },
        MediaItem {
            source: "sample/photos/IMG_0135.mp4".to_string(),
            filename: "IMG_0135.mp4".to_string(),
            album: None,
            additional_albums: Vec::new(),
            date: Some("2024-05-16".to_string()),
            size_bytes: 0,
            is_photo: false,
            is_video: true,
            is_live: false,
            live_pair_partner: None,
            live_content_identifier: None,
            live_pair_video: false,
            embedded_motion_video: false,
            live_warning: None,
            favorite: false,
            archived: false,
            trashed: false,
            edited: false,
            has_json: true,
            duplicate_of: None,
            metadata: None,
        },
    ];

    plan(&sample, settings)
        .into_iter()
        .map(|placement| placement.dest)
        .collect()
}

pub fn plan(items: &[MediaItem], settings: &PlannerSettings) -> Vec<Placement> {
    let output_root = effective_output_root(settings.output_root.as_deref());
    plan_with_root(items, settings, Path::new(&output_root), &HashSet::new())
}

pub fn plan_with_root(
    items: &[MediaItem],
    settings: &PlannerSettings,
    output_root: &Path,
    existing_paths: &HashSet<String>,
) -> Vec<Placement> {
    let mut placements = Vec::new();
    let mut occupied = existing_paths
        .iter()
        .map(|path| normalized_path_key(path))
        .collect::<HashSet<_>>();

    for item in items {
        if item.trashed && matches!(settings.trashed, TrashedMode::Skip) {
            continue;
        }

        if settings.ignore_duplicates && item.duplicate_of.is_some() {
            continue;
        }

        if settings.merge_live
            && settings.live_style == "androidMotionPhoto"
            && item.live_pair_video
        {
            continue;
        }

        if matches!(settings.edited_versions, EditedVersions::Originals) && item.edited {
            continue;
        }
        if matches!(settings.edited_versions, EditedVersions::Edited) && !item.edited {
            continue;
        }

        if item.trashed && matches!(settings.trashed, TrashedMode::OwnFolder) {
            if let Some(placement) = original_placement(
                item,
                output_root,
                &["Trash".to_string()],
                settings,
                &mut occupied,
            ) {
                placements.push(placement);
            }
            continue;
        }
        if !item.has_json {
            if matches!(settings.no_json, NoJsonMode::Leave) {
                continue;
            }
            if let Some(placement) = original_placement(
                item,
                output_root,
                &["Unsorted".to_string()],
                settings,
                &mut occupied,
            ) {
                placements.push(placement);
            }
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

        if let Some(placement) =
            original_placement(item, output_root, &main_folders, settings, &mut occupied)
        {
            placements.push(placement);
        }

        let albums = item_albums(item);
        if matches!(settings.album_mode, AlbumMode::Link | AlbumMode::Copy) {
            for album_name in &albums {
                let mut album_folders = album_folders(
                    album_name,
                    organization_date(item, settings).as_deref(),
                    settings.album_loc,
                );
                append_type_folders(&mut album_folders, item, settings);
                let kind = if matches!(settings.album_mode, AlbumMode::Link) {
                    PlacementKind::Link
                } else {
                    PlacementKind::Copy
                };
                if let Some(placement) = extra_placement(
                    item,
                    output_root,
                    &album_folders,
                    kind,
                    Some(album_name),
                    settings,
                    &mut occupied,
                ) {
                    placements.push(placement);
                }
            }
        } else if matches!(settings.album_mode, AlbumMode::Move)
            && matches!(settings.multi_album, MultiAlbumMode::All)
        {
            for album_name in albums.iter().skip(1) {
                let mut album_folders = album_folders(
                    album_name,
                    organization_date(item, settings).as_deref(),
                    settings.album_loc,
                );
                append_type_folders(&mut album_folders, item, settings);
                if let Some(placement) = extra_placement(
                    item,
                    output_root,
                    &album_folders,
                    PlacementKind::Copy,
                    Some(album_name),
                    settings,
                    &mut occupied,
                ) {
                    placements.push(placement);
                }
            }
        }

        if matches!(
            settings.favorites_mode,
            FavoritesMode::Link | FavoritesMode::Copy
        ) && item.favorite
        {
            let kind = if matches!(settings.favorites_mode, FavoritesMode::Link) {
                PlacementKind::Link
            } else {
                PlacementKind::Copy
            };
            if let Some(placement) = extra_placement(
                item,
                output_root,
                &["Favorites".to_string()],
                kind,
                None,
                settings,
                &mut occupied,
            ) {
                placements.push(placement);
            }
        }

        if matches!(settings.edited_mode, EditedMode::Link | EditedMode::Copy) && item.edited {
            let kind = if matches!(settings.edited_mode, EditedMode::Link) {
                PlacementKind::Link
            } else {
                PlacementKind::Copy
            };
            if let Some(placement) = extra_placement(
                item,
                output_root,
                &["Edited".to_string()],
                kind,
                None,
                settings,
                &mut occupied,
            ) {
                placements.push(placement);
            }
        }
    }

    for placement in &mut placements {
        let Some(item) = items.iter().find(|item| item.source == placement.src) else {
            continue;
        };
        if settings.merge_live {
            if let Some(identifier) = item.live_content_identifier.as_deref() {
                if matches!(settings.live_style.as_str(), "iphonePair" | "iphone") {
                    placement.tags.push(format!("LivePhotoIphone:{identifier}"));
                }
            }
            if !item.live_pair_video && matches!(settings.live_style.as_str(), "androidMotionPhoto")
            {
                if let Some(video_source) = item.live_pair_partner.as_deref() {
                    if let Some(identifier) = item.live_content_identifier.as_deref() {
                        placement
                            .tags
                            .push(format!("LivePhotoAndroid:{identifier}:{video_source}"));
                    }
                }
            }
        }
        if let Some(warning) = item.live_warning.as_deref() {
            placement.tags.push(format!("LivePhotoWarning:{warning}"));
        }
    }

    placements
}

fn item_albums(item: &MediaItem) -> Vec<String> {
    item.album
        .iter()
        .chain(item.additional_albums.iter())
        .map(|album| sanitize_file_part(album))
        .filter(|album| !album.is_empty())
        .fold(Vec::new(), |mut albums, album| {
            if !albums
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(&album))
            {
                albums.push(album);
            }
            albums
        })
}

fn normal_folders(item: &MediaItem, settings: &PlannerSettings) -> Vec<String> {
    let date = organization_date(item, settings);
    let date_mode = if date.is_none() && matches!(settings.no_date, NoDateMode::Blank) {
        DateFolderMode::Off
    } else {
        settings.date_folders
    };
    match (item.album.as_deref(), settings.album_mode) {
        (Some(album), AlbumMode::Move) => album_folders(album, date.as_deref(), settings.album_loc),
        _ => build_date_path(date.as_deref(), date_mode)
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
    output_root: &Path,
    folders: &[String],
    settings: &PlannerSettings,
    occupied: &mut HashSet<String>,
) -> Option<Placement> {
    let directory = folder_path(output_root, folders);
    let filename = unique_path_name(&directory, item, None, settings, occupied)?;
    Some(Placement {
        src: item.source.clone(),
        dest: directory.join(filename).to_string_lossy().into_owned(),
        kind: PlacementKind::Original,
        tags: vec![],
    })
}

fn extra_placement(
    item: &MediaItem,
    output_root: &Path,
    folders: &[String],
    kind: PlacementKind,
    album_override: Option<&str>,
    settings: &PlannerSettings,
    occupied: &mut HashSet<String>,
) -> Option<Placement> {
    let directory = folder_path(output_root, folders);
    let filename = unique_path_name(&directory, item, album_override, settings, occupied)?;
    let label = match kind {
        PlacementKind::Link => "Link",
        PlacementKind::Copy => "Copy",
        PlacementKind::Original => "",
    };
    Some(Placement {
        src: item.source.clone(),
        dest: directory.join(filename).to_string_lossy().into_owned(),
        kind,
        tags: vec![label.to_string()],
    })
}

fn folder_path(output_root: &Path, folders: &[String]) -> PathBuf {
    folders
        .iter()
        .fold(output_root.to_path_buf(), |path, part| path.join(part))
}

fn effective_output_root(output_root: Option<&str>) -> String {
    match output_root {
        Some(path) if !path.trim().is_empty() => path.trim().to_string(),
        _ => "Pictures/Photo Library".to_string(),
    }
}

fn build_date_path(date: Option<&str>, mode: DateFolderMode) -> String {
    match mode {
        DateFolderMode::Off => "".to_string(),
        DateFolderMode::Year => parent_year(date),
        DateFolderMode::YearMonth => match date.and_then(date_year_month) {
            Some((year, month)) => format!("{year}/{month}"),
            None => "Unknown".to_string(),
        },
    }
}

fn parent_year(date: Option<&str>) -> String {
    date.and_then(date_year_month)
        .map(|(year, _)| year)
        .or_else(|| {
            date.filter(|value| {
                value.chars().take(4).count() == 4
                    && value.chars().take(4).all(|ch| ch.is_ascii_digit())
            })
            .map(|value| value.chars().take(4).collect())
        })
        .unwrap_or_else(|| "Unknown".to_string())
}

fn date_year_month(date: &str) -> Option<(String, String)> {
    let bytes = date.as_bytes();
    if bytes.len() < 7
        || !bytes[..4].iter().all(u8::is_ascii_digit)
        || bytes[4] != b'-'
        || !bytes[5..7].iter().all(u8::is_ascii_digit)
    {
        return None;
    }
    let month = u8::from_str_radix(&date[5..7], 10).ok()?;
    if !(1..=12).contains(&month) {
        return None;
    }
    Some((date[..4].to_string(), format!("{month:02}")))
}

fn unique_path_name(
    directory: &Path,
    item: &MediaItem,
    album_override: Option<&str>,
    settings: &PlannerSettings,
    occupied: &mut HashSet<String>,
) -> Option<String> {
    let base_name = sanitize_file_part(&rename_from_setting(item, album_override, settings, 1));
    let base_path = directory.join(&base_name);
    if occupy_if_available(&base_path, occupied) {
        return Some(base_name);
    }

    match settings.same_name {
        SameNameMode::Skip => None,
        SameNameMode::Number => {
            if settings.file_names == "custom" && settings.custom_name_pattern.contains("{n}") {
                for number in 2_u64.. {
                    let candidate = sanitize_file_part(&rename_from_setting(
                        item,
                        album_override,
                        settings,
                        number,
                    ));
                    if occupy_if_available(&directory.join(&candidate), occupied) {
                        return Some(candidate);
                    }
                }
                None
            } else {
                numbered_name(directory, &base_name, occupied)
            }
        }
        SameNameMode::DateTime => {
            let (stem, extension) = split_extension(&base_name);
            let (date, time) = item_date_time(item, settings);
            let dated_stem = format!("{stem}-{date}_{time}");
            let candidate = with_extension(&dated_stem, extension);
            let path = directory.join(&candidate);
            if occupy_if_available(&path, occupied) {
                Some(candidate)
            } else {
                numbered_name(directory, &candidate, occupied)
            }
        }
    }
}

fn rename_from_setting(
    item: &MediaItem,
    album_override: Option<&str>,
    settings: &PlannerSettings,
    number: u64,
) -> String {
    let (date, time) = item_date_time(item, settings);
    match settings.file_names.as_str() {
        "dateTime" => format!("{date}_{time}_{}", item.filename),
        "dateOriginal" => format!("{date}_{}", item.filename),
        "custom" if !settings.custom_name_pattern.is_empty() => settings
            .custom_name_pattern
            .replace("{date}", &date)
            .replace("{time}", &time)
            .replace("{original}", &item.filename)
            .replace("{n}", &number.to_string())
            .replace(
                "{album}",
                album_override.or(item.album.as_deref()).unwrap_or(""),
            ),
        _ => item.filename.clone(),
    }
}

fn item_date_time(item: &MediaItem, settings: &PlannerSettings) -> (String, String) {
    if let Some(timestamp) = item
        .metadata
        .as_ref()
        .and_then(TakeoutMetadata::taken_timestamp)
    {
        if let Some(datetime) = chrono::DateTime::<chrono::Utc>::from_timestamp(timestamp, 0) {
            return (
                datetime.format("%Y-%m-%d").to_string(),
                datetime.format("%H%M%S").to_string(),
            );
        }
    }
    let date = organization_date(item, settings).unwrap_or_else(|| "Unknown".to_string());
    let date = date.get(..10).unwrap_or(&date).to_string();
    (date, "000000".to_string())
}

fn organization_date(item: &MediaItem, settings: &PlannerSettings) -> Option<String> {
    item.date.clone().or_else(|| match settings.no_date {
        NoDateMode::FileName => date_from_filename(&item.filename),
        NoDateMode::FileDate | NoDateMode::Blank => None,
    })
}

fn date_from_filename(filename: &str) -> Option<String> {
    let bytes = filename.as_bytes();
    for (index, window) in bytes.windows(10).enumerate() {
        if window[4] == b'-' || window[4] == b'_' {
            if window[..4].iter().all(u8::is_ascii_digit)
                && window[5] == window[4]
                && window[5..7].iter().all(u8::is_ascii_digit)
                && matches!(window[7], b'-' | b'_')
                && window[8..10].iter().all(u8::is_ascii_digit)
            {
                let year = std::str::from_utf8(&window[..4]).ok()?;
                let month =
                    u8::from_str_radix(std::str::from_utf8(&window[5..7]).ok()?, 10).ok()?;
                let day = u8::from_str_radix(std::str::from_utf8(&window[8..10]).ok()?, 10).ok()?;
                if (1..=12).contains(&month) && (1..=31).contains(&day) {
                    return Some(format!("{year}-{month:02}-{day:02}"));
                }
            }
        } else if index + 8 <= bytes.len() && window[..8].iter().all(u8::is_ascii_digit) {
            let year = std::str::from_utf8(&window[..4]).ok()?;
            let month = u8::from_str_radix(std::str::from_utf8(&window[4..6]).ok()?, 10).ok()?;
            let day = u8::from_str_radix(std::str::from_utf8(&window[6..8]).ok()?, 10).ok()?;
            if (1..=12).contains(&month) && (1..=31).contains(&day) {
                return Some(format!("{year}-{month:02}-{day:02}"));
            }
        }
    }
    None
}

fn split_extension(filename: &str) -> (&str, &str) {
    filename
        .rsplit_once('.')
        .filter(|(stem, _)| !stem.is_empty())
        .map_or((filename, ""), |(stem, extension)| (stem, extension))
}

fn with_extension(stem: &str, extension: &str) -> String {
    if extension.is_empty() {
        stem.to_string()
    } else {
        format!("{stem}.{extension}")
    }
}

fn numbered_name(
    directory: &Path,
    filename: &str,
    occupied: &mut HashSet<String>,
) -> Option<String> {
    let (stem, extension) = split_extension(filename);
    for number in 2_u64.. {
        let candidate = with_extension(&format!("{stem}-{number}"), extension);
        if occupy_if_available(&directory.join(&candidate), occupied) {
            return Some(candidate);
        }
    }
    None
}

fn occupy_if_available(path: &Path, occupied: &mut HashSet<String>) -> bool {
    occupied.insert(normalized_path_key(&path.to_string_lossy()))
}

fn normalized_path_key(path: &str) -> String {
    path.replace('\\', "/").to_lowercase()
}

fn sanitize_file_part(part: &str) -> String {
    let mut cleaned: String = part
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            _ => c,
        })
        .collect();

    cleaned = cleaned
        .trim_matches(|c: char| c == '.' || c == ' ' || c == '\0')
        .to_string();
    if cleaned.is_empty() {
        cleaned = "photo".to_string();
    }

    let upper = cleaned
        .split('.')
        .next()
        .unwrap_or(&cleaned)
        .trim_end_matches([' ', '.'])
        .to_ascii_uppercase();
    let invalid_device = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if invalid_device.contains(&upper.as_str()) {
        cleaned = format!("_{}", cleaned);
    }

    if cleaned.len() > 255 {
        let mut end = 255;
        while !cleaned.is_char_boundary(end) {
            end -= 1;
        }
        cleaned.truncate(end);
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
            additional_albums: Vec::new(),
            date: Some("2024-05-16".to_string()),
            size_bytes: 0,
            is_photo: true,
            is_video: false,
            is_live: false,
            live_pair_partner: None,
            live_content_identifier: None,
            live_pair_video: false,
            embedded_motion_video: false,
            live_warning: None,
            favorite: false,
            archived: false,
            trashed: false,
            edited: false,
            has_json: true,
            duplicate_of: None,
            metadata: None,
        }
    }

    fn destination_ends_with(destination: &str, suffix: &str) -> bool {
        destination.replace('\\', "/").ends_with(suffix)
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
        assert_eq!(value["dateFolders"], "yearMonth");
        assert_eq!(value["customNamePattern"], "");
    }

    #[test]
    fn canonical_preferences_map_to_all_planner_fields() {
        let preferences = Settings {
            date_folders: settings::DateFolderMode::Year,
            multi_album: settings::MultiAlbumMode::All,
            file_names: settings::FileNamesMode::Custom,
            custom_name_pattern: "{date}_{n}_{original}_{album}".to_string(),
            same_name: settings::SameNameMode::Skip,
            ..Settings::default()
        };
        let mapped = PlannerSettings::from_preferences(&preferences, None);

        assert_eq!(mapped.date_folders, DateFolderMode::Year);
        assert_eq!(mapped.multi_album, MultiAlbumMode::All);
        assert_eq!(mapped.file_names, "custom");
        assert_eq!(mapped.custom_name_pattern, "{date}_{n}_{original}_{album}");
        assert_eq!(mapped.same_name, SameNameMode::Skip);
        assert_eq!(mapped.rate_favorites, preferences.rate_favorites);
        assert_eq!(mapped.live_style, "separate");
    }

    #[test]
    fn date_folder_options_build_expected_paths() {
        let item = sample_item("photo.jpg");
        let cases = [
            (DateFolderMode::Off, "photo.jpg"),
            (DateFolderMode::Year, "2024/photo.jpg"),
            (DateFolderMode::YearMonth, "2024/05/photo.jpg"),
        ];
        for (date_folders, expected) in cases {
            let placements = plan(
                &[item.clone()],
                &PlannerSettings {
                    album_mode: AlbumMode::Off,
                    date_folders,
                    ..PlannerSettings::default()
                },
            );
            assert!(placements
                .iter()
                .any(|placement| destination_ends_with(&placement.dest, expected)));
        }
    }

    #[test]
    fn edited_version_policy_keeps_the_requested_versions() {
        let original = sample_item("photo.jpg");
        let mut edited = sample_item("photo-edited.jpg");
        edited.edited = true;
        let items = [original, edited];
        let common = PlannerSettings {
            album_mode: AlbumMode::Off,
            favorites_mode: FavoritesMode::Off,
            edited_mode: EditedMode::Off,
            ..PlannerSettings::default()
        };
        let cases = [
            (EditedVersions::Both, vec!["photo.jpg", "photo-edited.jpg"]),
            (EditedVersions::Originals, vec!["photo.jpg"]),
            (EditedVersions::Edited, vec!["photo-edited.jpg"]),
        ];

        for (policy, expected) in cases {
            let placements = plan(
                &items,
                &PlannerSettings {
                    edited_versions: policy,
                    ..common.clone()
                },
            );
            let filenames = placements
                .iter()
                .map(|placement| {
                    Path::new(&placement.dest)
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned()
                })
                .collect::<HashSet<_>>();
            assert_eq!(
                filenames,
                expected
                    .into_iter()
                    .map(str::to_string)
                    .collect::<HashSet<_>>()
            );
        }
    }

    #[test]
    fn live_photo_styles_keep_or_merge_only_confident_pairs() {
        let mut photo = sample_item("live.jpg");
        photo.live_pair_partner = Some("sample/live.mov".to_string());
        photo.live_content_identifier = Some("takeback-live-id".to_string());
        photo.is_live = true;
        let mut video = sample_item("live.mov");
        video.source = "sample/live.mov".to_string();
        video.is_photo = false;
        video.is_video = true;
        video.is_live = true;
        video.live_pair_partner = Some("sample/live.jpg".to_string());
        video.live_content_identifier = Some("takeback-live-id".to_string());
        video.live_pair_video = true;
        let items = [photo, video];
        let common = PlannerSettings {
            album_mode: AlbumMode::Off,
            favorites_mode: FavoritesMode::Off,
            edited_mode: EditedMode::Off,
            date_folders: DateFolderMode::Off,
            ..PlannerSettings::default()
        };

        let separate = plan(
            &items,
            &PlannerSettings {
                live_style: "separate".to_string(),
                ..common.clone()
            },
        );
        assert_eq!(separate.len(), 2);
        assert!(separate.iter().all(|placement| !placement
            .tags
            .iter()
            .any(|tag| tag.starts_with("LivePhoto"))));

        let iphone = plan(
            &items,
            &PlannerSettings {
                live_style: "iphonePair".to_string(),
                ..common.clone()
            },
        );
        assert_eq!(iphone.len(), 2);
        assert!(iphone.iter().all(|placement| placement
            .tags
            .contains(&"LivePhotoIphone:takeback-live-id".to_string())));

        let android = plan(
            &items,
            &PlannerSettings {
                live_style: "androidMotionPhoto".to_string(),
                ..common
            },
        );
        assert_eq!(android.len(), 1);
        assert!(android[0]
            .tags
            .iter()
            .any(|tag| { tag == "LivePhotoAndroid:takeback-live-id:sample/live.mov" }));
    }

    #[test]
    fn no_date_policy_uses_filename_or_leaves_date_folders_blank() {
        let mut item = sample_item("IMG_20240516_photo.jpg");
        item.date = None;
        let file_name_date = plan(
            &[item.clone()],
            &PlannerSettings {
                album_mode: AlbumMode::Off,
                no_date: NoDateMode::FileName,
                ..PlannerSettings::default()
            },
        );
        assert!(file_name_date.iter().any(|placement| destination_ends_with(
            &placement.dest,
            "2024/05/IMG_20240516_photo.jpg"
        )));

        let blank = plan(
            &[item],
            &PlannerSettings {
                album_mode: AlbumMode::Off,
                no_date: NoDateMode::Blank,
                ..PlannerSettings::default()
            },
        );
        assert!(blank
            .iter()
            .any(|placement| destination_ends_with(&placement.dest, "IMG_20240516_photo.jpg")));
    }

    #[test]
    fn custom_name_pattern_and_multiple_album_copy_are_applied() {
        let mut item = sample_item("photo.jpg");
        item.additional_albums = vec!["Family".to_string()];
        let settings = PlannerSettings {
            multi_album: MultiAlbumMode::All,
            file_names: "custom".to_string(),
            custom_name_pattern: "{date}_{n}_{original}_{album}".to_string(),
            ..PlannerSettings::default()
        };
        let placements = plan(&[item], &settings);

        assert!(placements.iter().any(|placement| destination_ends_with(
            &placement.dest,
            "Paris trip/2024-05-16_1_photo.jpg_Paris trip"
        )));
        assert!(placements.iter().any(|placement| {
            destination_ends_with(&placement.dest, "Family/2024-05-16_1_photo.jpg_Family")
                && placement.kind == PlacementKind::Copy
        }));
    }

    #[test]
    fn existing_output_paths_participate_in_collision_rules() {
        let item = sample_item("photo.jpg");
        let settings = PlannerSettings {
            album_mode: AlbumMode::Off,
            ..PlannerSettings::default()
        };
        let root = Path::new("Pictures/Photo Library");
        let existing = plan(&[item.clone()], &settings);
        let existing_paths = existing
            .iter()
            .map(|placement| normalized_path_key(&placement.dest))
            .collect::<HashSet<_>>();
        let numbered = plan_with_root(&[item.clone()], &settings, root, &existing_paths);
        assert!(numbered
            .iter()
            .any(|placement| destination_ends_with(&placement.dest, "photo-2.jpg")));

        let skipped = plan_with_root(
            &[item],
            &PlannerSettings {
                same_name: SameNameMode::Skip,
                ..settings
            },
            root,
            &existing_paths,
        );
        assert!(skipped.is_empty());
    }

    #[test]
    fn output_root_uses_library_child_for_chosen_folder() {
        let settings = PlannerSettings {
            output_root: Some("/Volumes/Archive".to_string()),
            ..PlannerSettings::default()
        };
        assert_eq!(
            effective_output_root(settings.output_root.as_deref()),
            "/Volumes/Archive"
        );
    }

    #[test]
    fn sanitizes_windows_illegal_chars_and_reserved_names() {
        assert_eq!(sanitize_file_part("<>:\\|?*"), "_______");
        assert_eq!(sanitize_file_part("NUL"), "_NUL");
        assert_eq!(sanitize_file_part("CON.txt"), "_CON.txt");
        assert_eq!(sanitize_file_part("  final.jpg  "), "final.jpg");
        assert_eq!(sanitize_file_part("2024/01/01"), "2024_01_01");
    }

    #[test]
    fn case_insensitive_duplicate_names_are_numbered_or_skipped() {
        let items = [sample_item("IMG_0001.jpg"), sample_item("img_0001.jpg")];
        let numbered = plan(&items, &PlannerSettings::default());
        assert_eq!(numbered.len(), 2);
        assert_ne!(
            numbered[0].dest.to_lowercase(),
            numbered[1].dest.to_lowercase()
        );
        assert!(numbered[1].dest.contains("-2"));

        let skipped = plan(
            &items,
            &PlannerSettings {
                same_name: SameNameMode::Skip,
                ..PlannerSettings::default()
            },
        );
        assert_eq!(skipped.len(), 1);
    }

    #[test]
    fn album_link_with_favorites_copy_and_video_split() {
        let settings = PlannerSettings {
            album_mode: AlbumMode::Link,
            favorites_mode: FavoritesMode::Copy,
            split_by_type: SplitByType {
                photos: false,
                videos: true,
                live: false,
            },
            ..PlannerSettings::default()
        };
        let items = vec![MediaItem {
            source: "a.mov".to_string(),
            filename: "a.mov".to_string(),
            album: Some("Paris trip".to_string()),
            additional_albums: Vec::new(),
            date: Some("2024-05-16".to_string()),
            size_bytes: 0,
            is_photo: false,
            is_video: true,
            is_live: false,
            live_pair_partner: None,
            live_content_identifier: None,
            live_pair_video: false,
            embedded_motion_video: false,
            live_warning: None,
            favorite: true,
            archived: false,
            trashed: false,
            edited: false,
            has_json: true,
            duplicate_of: None,
            metadata: None,
        }];

        let placements = plan(&items, &settings);
        assert!(placements
            .iter()
            .any(|p| destination_ends_with(&p.dest, "2024/05/Videos/a.mov")));
        assert!(placements.iter().any(|p| {
            destination_ends_with(&p.dest, "Paris trip/Videos/a.mov")
                && p.kind == PlacementKind::Link
        }));
        assert!(placements.iter().any(|p| {
            destination_ends_with(&p.dest, "Favorites/a.mov")
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
            split_by_type: SplitByType {
                photos: true,
                videos: true,
                live: true,
            },
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
        let destinations: HashSet<String> = placements
            .into_iter()
            .map(|p| p.dest.replace('\\', "/"))
            .collect();
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
            split_by_type: SplitByType {
                photos: true,
                videos: false,
                live: false,
            },
            ..PlannerSettings::default()
        };
        let mut item = sample_item("favorite.jpg");
        item.favorite = true;

        let placements = plan(&[item], &settings);
        assert!(placements.iter().any(|p| {
            destination_ends_with(&p.dest, "2024/05/Photos/favorite.jpg")
                && p.kind == PlacementKind::Original
        }));
        assert!(placements.iter().any(|p| {
            destination_ends_with(&p.dest, "Favorites/favorite.jpg")
                && p.kind == PlacementKind::Link
                && p.tags == ["Link"]
        }));
    }

    #[test]
    fn moved_albums_are_top_level_by_default_or_nested_only_by_year() {
        let item = sample_item("photo.jpg");
        let top_level = plan(&[item.clone()], &PlannerSettings::default());
        assert!(top_level
            .iter()
            .any(|p| destination_ends_with(&p.dest, "Paris trip/photo.jpg")));

        let year_level = plan(
            &[item],
            &PlannerSettings {
                album_loc: AlbumLocation::Year,
                ..PlannerSettings::default()
            },
        );
        assert!(year_level
            .iter()
            .any(|p| destination_ends_with(&p.dest, "2024/Paris trip/photo.jpg")));
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
            destination_ends_with(&p.dest, "2024/Paris trip/photo.jpg")
                && p.kind == PlacementKind::Link
        }));
        assert!(placements.iter().any(|p| {
            destination_ends_with(&p.dest, "2024/05/photo.jpg") && p.kind == PlacementKind::Original
        }));
    }

    #[test]
    fn edited_link_stays_flat_and_preserves_normal_placement() {
        let settings = PlannerSettings {
            album_mode: AlbumMode::Off,
            edited_mode: EditedMode::Link,
            split_by_type: SplitByType {
                photos: true,
                videos: false,
                live: false,
            },
            ..PlannerSettings::default()
        };
        let mut item = sample_item("edited.jpg");
        item.edited = true;

        let placements = plan(&[item], &settings);
        assert!(placements.iter().any(|p| {
            destination_ends_with(&p.dest, "2024/05/Photos/edited.jpg")
                && p.kind == PlacementKind::Original
        }));
        assert!(placements.iter().any(|p| {
            destination_ends_with(&p.dest, "Edited/edited.jpg")
                && p.kind == PlacementKind::Link
                && p.tags.contains(&"Link".to_string())
        }));
    }
}
