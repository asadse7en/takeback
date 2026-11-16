use crate::livephoto::LivePhotoStyle;
use serde::{Deserialize, Serialize};

pub const FREE_SPACE_CHECK_ENABLED: bool = true;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FolderMode {
    Off,
    #[default]
    Move,
    Link,
    Copy,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AlbumLocation {
    #[default]
    Top,
    Year,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MultiAlbumMode {
    #[default]
    First,
    All,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DateFolderMode {
    Off,
    Year,
    #[default]
    YearMonth,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TrashedMode {
    #[default]
    Skip,
    Include,
    OwnFolder,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EditedVersions {
    #[default]
    Both,
    Originals,
    Edited,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FileNamesMode {
    #[default]
    Keep,
    DateTime,
    DateOriginal,
    Custom,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SameNameMode {
    #[default]
    Number,
    DateTime,
    Skip,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum NoJsonMode {
    Leave,
    #[default]
    Unsorted,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TimeZoneMode {
    #[default]
    Local,
    Computer,
    Utc,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum NoDateMode {
    #[default]
    FileName,
    FileDate,
    Blank,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SpeedMode {
    #[default]
    Balanced,
    Gentle,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SplitByType {
    pub photos: bool,
    pub videos: bool,
    pub live: bool,
}

impl Default for SplitByType {
    fn default() -> Self {
        Self {
            photos: false,
            videos: false,
            live: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct Settings {
    pub output_folder: Option<String>,
    pub album_mode: FolderMode,
    pub album_loc: AlbumLocation,
    pub multi_album: MultiAlbumMode,
    pub date_folders: DateFolderMode,
    pub favorites_mode: FolderMode,
    pub edited_mode: FolderMode,
    pub archive_folder: bool,
    pub trashed: TrashedMode,
    pub edited_versions: EditedVersions,
    pub merge_live: bool,
    pub live_style: LivePhotoStyle,
    pub split_by_type: SplitByType,
    pub file_names: FileNamesMode,
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

impl Default for Settings {
    fn default() -> Self {
        Self {
            output_folder: None,
            album_mode: FolderMode::Move,
            album_loc: AlbumLocation::Top,
            multi_album: MultiAlbumMode::First,
            date_folders: DateFolderMode::YearMonth,
            favorites_mode: FolderMode::Link,
            edited_mode: FolderMode::Move,
            archive_folder: true,
            trashed: TrashedMode::Skip,
            edited_versions: EditedVersions::Both,
            merge_live: true,
            live_style: LivePhotoStyle::Separate,
            split_by_type: SplitByType::default(),
            file_names: FileNamesMode::Keep,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_use_frontend_field_and_variant_names() {
        let value = serde_json::to_value(Settings::default()).expect("serialize settings");

        assert_eq!(value["dateFolders"], "yearMonth");
        assert_eq!(value["liveStyle"], "separate");
        assert_eq!(value["customNamePattern"], "");
        assert_eq!(value["rateFavorites"], true);
    }

    #[test]
    fn live_photo_styles_match_frontend_values() {
        assert_eq!(
            serde_json::to_string(&LivePhotoStyle::IphonePair).expect("serialize style"),
            "\"iphonePair\""
        );
        assert_eq!(
            serde_json::to_string(&LivePhotoStyle::AndroidMotionPhoto).expect("serialize style"),
            "\"androidMotionPhoto\""
        );
    }
}
