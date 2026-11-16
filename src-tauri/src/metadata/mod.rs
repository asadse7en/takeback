use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::SystemTime;

use chrono::{DateTime, FixedOffset, Local, Offset, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::sync::OnceLock;

use crate::planner::TimeZoneMode;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TakeoutMetadata {
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

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawTakeoutMetadata {
    title: Option<String>,
    description: Option<String>,
    photo_taken_time: Option<RawTakeoutTime>,
    creation_time: Option<RawTakeoutTime>,
    geo_data: Option<RawGeoData>,
    geo_data_exif: Option<RawGeoData>,
    #[serde(default)]
    people: Vec<RawPerson>,
    #[serde(default)]
    favorited: bool,
    #[serde(default)]
    archived: bool,
    #[serde(default)]
    trashed: bool,
    url: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RawTakeoutTime {
    timestamp: Option<FlexibleTimestamp>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum FlexibleTimestamp {
    Integer(i64),
    String(String),
}

impl FlexibleTimestamp {
    fn seconds(&self) -> Option<i64> {
        match self {
            Self::Integer(value) => Some(*value),
            Self::String(value) => value.parse().ok(),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawGeoData {
    latitude: Option<f64>,
    longitude: Option<f64>,
    altitude: Option<f64>,
}

impl RawGeoData {
    fn coordinates(&self) -> Option<(f64, f64)> {
        let latitude = self.latitude?;
        let longitude = self.longitude?;
        if !latitude.is_finite() || !longitude.is_finite() || (latitude == 0.0 && longitude == 0.0)
        {
            return None;
        }
        Some((latitude, longitude))
    }
}

#[derive(Debug, Deserialize)]
struct RawPerson {
    name: Option<String>,
}

impl TakeoutMetadata {
    pub fn from_reader(reader: impl Read) -> Result<Self, String> {
        let raw: RawTakeoutMetadata = serde_json::from_reader(reader)
            .map_err(|error| format!("invalid Takeout JSON sidecar: {error}"))?;

        let geo = raw
            .geo_data
            .as_ref()
            .filter(|data| data.coordinates().is_some())
            .or(raw.geo_data_exif.as_ref());
        let coordinates = geo.and_then(RawGeoData::coordinates);

        Ok(Self {
            title: raw.title,
            description: raw.description,
            photo_taken_time: raw
                .photo_taken_time
                .and_then(|time| time.timestamp)
                .and_then(|timestamp| timestamp.seconds()),
            creation_time: raw
                .creation_time
                .and_then(|time| time.timestamp)
                .and_then(|timestamp| timestamp.seconds()),
            latitude: coordinates.map(|(latitude, _)| latitude),
            longitude: coordinates.map(|(_, longitude)| longitude),
            altitude: geo.and_then(|data| data.altitude),
            people: raw
                .people
                .into_iter()
                .filter_map(|person| person.name)
                .collect(),
            favorited: raw.favorited,
            archived: raw.archived,
            trashed: raw.trashed,
            url: raw.url,
        })
    }

    pub fn taken_timestamp(&self) -> Option<i64> {
        self.photo_taken_time.or(self.creation_time)
    }
}

pub fn supports_embedded_metadata(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some(
            "jpg"
                | "jpeg"
                | "png"
                | "webp"
                | "heic"
                | "heif"
                | "gif"
                | "tif"
                | "tiff"
                | "dng"
                | "cr2"
                | "cr3"
                | "nef"
                | "nrw"
                | "arw"
                | "orf"
                | "rw2"
                | "raf"
                | "pef"
                | "sr2"
                | "iiq"
                | "mp4"
                | "mov"
                | "m4v"
        )
    )
}

pub struct ExifTool {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl ExifTool {
    pub fn new(executable: &Path) -> Result<Self, String> {
        let mut child = Command::new(executable)
            .args(["-stay_open", "True", "-@", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("start bundled ExifTool: {error}"))?;
        let input = child
            .stdin
            .take()
            .ok_or_else(|| "could not open ExifTool input".to_string())?;
        let output = child
            .stdout
            .take()
            .ok_or_else(|| "could not open ExifTool output".to_string())?;
        let mut tool = Self {
            child,
            input,
            output: BufReader::new(output),
        };
        tool.execute(&["-charset".to_string(), "filename=UTF8".to_string()])?;
        let version = tool.execute(&["-ver".to_string()])?;
        if version.trim().is_empty() {
            return Err("bundled ExifTool did not report its version".to_string());
        }
        Ok(tool)
    }

    pub fn write(
        &mut self,
        path: &Path,
        metadata: &TakeoutMetadata,
        time_zone: TimeZoneMode,
        write_gps: bool,
        write_descriptions: bool,
        rate_favorites: bool,
    ) -> Result<Vec<String>, String> {
        let source_file = path
            .to_str()
            .ok_or_else(|| format!("file path is not valid Unicode: {}", path.display()))?;
        let offset = if time_zone == TimeZoneMode::Local && metadata.taken_timestamp().is_some() {
            self.read_original_offset(source_file)?
        } else {
            None
        };
        let tags = metadata_tags(
            source_file,
            metadata,
            time_zone,
            offset.as_deref(),
            write_gps,
            write_descriptions,
            rate_favorites,
        )?;
        if tags.len() <= 1 {
            return self.set_modified_time(path, metadata);
        }

        let parent = path
            .parent()
            .ok_or_else(|| format!("output path has no parent: {}", path.display()))?;
        let mut json_file = tempfile::Builder::new()
            .prefix(".takeback-metadata-")
            .suffix(".json")
            .tempfile_in(parent)
            .map_err(|error| format!("create temporary metadata file: {error}"))?;
        serde_json::to_writer(&mut json_file, &Value::Array(vec![Value::Object(tags)]))
            .map_err(|error| format!("serialize metadata for {}: {error}", path.display()))?;
        json_file
            .flush()
            .map_err(|error| format!("flush temporary metadata file: {error}"))?;
        let json_file_path = json_file.into_temp_path();
        let json_path = json_file_path
            .to_str()
            .ok_or_else(|| "temporary metadata path is not valid Unicode".to_string())?;
        let output = self.execute(&[
            "-overwrite_original".to_string(),
            format!("-json={json_path}"),
        ])?;
        drop(json_file_path);
        let mut warnings = tool_warnings(&output);
        warnings.extend(self.set_modified_time(path, metadata)?);
        Ok(warnings)
    }

    pub fn write_live_content_identifier(
        &mut self,
        path: &Path,
        content_identifier: &str,
    ) -> Result<(), String> {
        let path = path
            .to_str()
            .ok_or_else(|| format!("file path is not valid Unicode: {}", path.display()))?;
        if content_identifier.contains('\n') || content_identifier.contains('\r') {
            return Err("live photo content identifier contains a line break".to_string());
        }
        self.execute(&[
            "-overwrite_original".to_string(),
            format!("-XMP:ContentIdentifier={content_identifier}"),
            format!("-ContentIdentifier={content_identifier}"),
            path.to_string(),
        ])?;
        Ok(())
    }

    pub fn read_taken_timestamp(&mut self, path: &Path) -> Result<Option<i64>, String> {
        let path = path
            .to_str()
            .ok_or_else(|| format!("file path is not valid Unicode: {}", path.display()))?;
        let output = self.execute(&[
            "-s".to_string(),
            "-s".to_string(),
            "-s".to_string(),
            "-DateTimeOriginal".to_string(),
            "-CreateDate".to_string(),
            "-MediaCreateDate".to_string(),
            "-TrackCreateDate".to_string(),
            path.to_string(),
        ])?;
        for value in output
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
        {
            if let Ok(datetime) = DateTime::parse_from_str(value, "%Y:%m:%d %H:%M:%S%:z") {
                return Ok(Some(datetime.timestamp()));
            }
            if let Ok(timestamp) = chrono::NaiveDateTime::parse_from_str(value, "%Y:%m:%d %H:%M:%S")
            {
                if let Some(datetime) = Local.from_local_datetime(&timestamp).single() {
                    return Ok(Some(datetime.timestamp()));
                }
            }
        }
        Ok(None)
    }

    pub fn embed_motion_video(
        &mut self,
        photo_path: &Path,
        video_path: &Path,
        content_identifier: &str,
    ) -> Result<(), String> {
        let photo_path = photo_path
            .to_str()
            .ok_or_else(|| format!("file path is not valid Unicode: {}", photo_path.display()))?;
        let video_path = video_path
            .to_str()
            .ok_or_else(|| format!("file path is not valid Unicode: {}", video_path.display()))?;
        if content_identifier.contains('\n') || content_identifier.contains('\r') {
            return Err("live photo content identifier contains a line break".to_string());
        }
        self.execute(&[
            "-overwrite_original".to_string(),
            "-XMP-GCamera:MicroVideo=1".to_string(),
            "-XMP-GCamera:MicroVideoVersion=1".to_string(),
            "-XMP-GCamera:MotionPhoto=1".to_string(),
            "-XMP-GCamera:MotionPhotoVersion=1".to_string(),
            format!("-XMP-GCamera:ContentIdentifier={content_identifier}"),
            format!("-XMP:ContentIdentifier={content_identifier}"),
            format!("-EmbeddedVideoFile={video_path}"),
            photo_path.to_string(),
        ])?;
        Ok(())
    }

    fn read_original_offset(&mut self, path: &str) -> Result<Option<String>, String> {
        let output = self.execute(&[
            "-s".to_string(),
            "-s".to_string(),
            "-s".to_string(),
            "-OffsetTimeOriginal".to_string(),
            path.to_string(),
        ])?;
        Ok(output.lines().map(str::trim).find_map(|line| {
            (!line.is_empty() && !line.starts_with("Warning:") && !line.starts_with("Error:"))
                .then(|| line.to_string())
        }))
    }

    fn set_modified_time(
        &self,
        path: &Path,
        metadata: &TakeoutMetadata,
    ) -> Result<Vec<String>, String> {
        let Some(timestamp) = metadata.taken_timestamp() else {
            return Ok(Vec::new());
        };
        let datetime = DateTime::<Utc>::from_timestamp(timestamp, 0)
            .ok_or_else(|| format!("invalid Takeout timestamp: {timestamp}"))?;
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .map_err(|error| format!("open output to set its modified time: {error}"))?;
        file.set_modified(SystemTime::from(datetime))
            .map_err(|error| format!("set output modified time: {error}"))?;
        Ok(Vec::new())
    }

    fn execute(&mut self, args: &[String]) -> Result<String, String> {
        for arg in args {
            if arg.contains('\n') || arg.contains('\r') {
                return Err("ExifTool arguments cannot contain line breaks".to_string());
            }
            writeln!(self.input, "{arg}")
                .map_err(|error| format!("send command to ExifTool: {error}"))?;
        }
        writeln!(self.input, "-execute")
            .and_then(|_| self.input.flush())
            .map_err(|error| format!("send command to ExifTool: {error}"))?;

        let mut response = String::new();
        loop {
            let mut line = String::new();
            let bytes = self
                .output
                .read_line(&mut line)
                .map_err(|error| format!("read ExifTool response: {error}"))?;
            if bytes == 0 {
                return Err("bundled ExifTool stopped responding".to_string());
            }
            if line.trim_end() == "{ready}" {
                break;
            }
            response.push_str(&line);
        }

        if let Some(error) = response.lines().find(|line| line.starts_with("Error:")) {
            return Err(error.trim().to_string());
        }
        Ok(response)
    }
}

impl Drop for ExifTool {
    fn drop(&mut self) {
        let _ = writeln!(self.input, "-stay_open");
        let _ = writeln!(self.input, "False");
        let _ = self.input.flush();
        let _ = self.child.wait();
    }
}

fn metadata_tags(
    source_file: &str,
    metadata: &TakeoutMetadata,
    time_zone: TimeZoneMode,
    original_offset: Option<&str>,
    write_gps: bool,
    write_descriptions: bool,
    rate_favorites: bool,
) -> Result<Map<String, Value>, String> {
    let mut tags = Map::new();
    let is_video = matches!(
        Path::new(source_file)
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("mp4" | "mov" | "m4v")
    );
    tags.insert(
        "SourceFile".to_string(),
        Value::String(source_file.to_string()),
    );

    if let Some(timestamp) = metadata.taken_timestamp() {
        let utc = DateTime::<Utc>::from_timestamp(timestamp, 0)
            .ok_or_else(|| format!("invalid Takeout timestamp: {timestamp}"))?;
        let datetime = datetime_with_offset(utc, time_zone, original_offset, metadata);
        let iso_datetime = datetime.format("%Y-%m-%dT%H:%M:%S%:z").to_string();
        let utc_datetime = utc.format("%Y:%m:%d %H:%M:%S").to_string();
        if is_video {
            for key in [
                "QuickTime:CreateDate",
                "QuickTime:ModifyDate",
                "QuickTime:TrackCreateDate",
                "QuickTime:TrackModifyDate",
                "QuickTime:MediaCreateDate",
                "QuickTime:MediaModifyDate",
            ] {
                tags.insert(key.to_string(), Value::String(utc_datetime.clone()));
            }
        } else {
            let exif_datetime = datetime.format("%Y:%m:%d %H:%M:%S").to_string();
            for key in [
                "EXIF:DateTimeOriginal",
                "EXIF:CreateDate",
                "EXIF:ModifyDate",
            ] {
                tags.insert(key.to_string(), Value::String(exif_datetime.clone()));
            }
            for key in [
                "EXIF:OffsetTimeOriginal",
                "EXIF:OffsetTimeDigitized",
                "EXIF:OffsetTime",
            ] {
                tags.insert(
                    key.to_string(),
                    Value::String(datetime.format("%:z").to_string()),
                );
            }
        }
        for key in ["XMP:DateCreated", "XMP:CreateDate", "XMP:ModifyDate"] {
            tags.insert(key.to_string(), Value::String(iso_datetime.clone()));
        }
    }

    if write_gps {
        if let (Some(latitude), Some(longitude)) = (metadata.latitude, metadata.longitude) {
            if is_video {
                let coordinates = match metadata.altitude {
                    Some(altitude) => {
                        format!("{latitude:+.6}{longitude:+.6}{altitude:+.2}/")
                    }
                    None => format!("{latitude:+.6}{longitude:+.6}/"),
                };
                tags.insert(
                    "QuickTime:GPSCoordinates".to_string(),
                    Value::String(coordinates),
                );
            } else {
                tags.insert("EXIF:GPSLatitude".to_string(), Value::from(latitude));
                tags.insert("EXIF:GPSLongitude".to_string(), Value::from(longitude));
                tags.insert("XMP-exif:GPSLatitude".to_string(), Value::from(latitude));
                tags.insert("XMP-exif:GPSLongitude".to_string(), Value::from(longitude));
                if let Some(altitude) = metadata.altitude {
                    tags.insert("EXIF:GPSAltitude".to_string(), Value::from(altitude));
                    tags.insert("XMP-exif:GPSAltitude".to_string(), Value::from(altitude));
                }
            }
            if let Some(altitude) = metadata.altitude {
                if is_video {
                    tags.insert("XMP-exif:GPSAltitude".to_string(), Value::from(altitude));
                }
            }
        }
    }

    if write_descriptions {
        if let Some(description) = metadata.description.as_ref() {
            tags.insert(
                "XMP-dc:Description".to_string(),
                Value::String(description.clone()),
            );
            if !is_video {
                tags.insert(
                    "EXIF:ImageDescription".to_string(),
                    Value::String(description.clone()),
                );
            }
        }
        if !metadata.people.is_empty() {
            tags.insert(
                "XMP-iptcExt:PersonInImage".to_string(),
                Value::Array(metadata.people.iter().cloned().map(Value::String).collect()),
            );
        }
    }

    if rate_favorites && metadata.favorited {
        tags.insert("XMP:Rating".to_string(), Value::from(5));
    }

    Ok(tags)
}

fn datetime_with_offset(
    utc: DateTime<Utc>,
    time_zone: TimeZoneMode,
    original_offset: Option<&str>,
    metadata: &TakeoutMetadata,
) -> DateTime<FixedOffset> {
    match time_zone {
        TimeZoneMode::Utc => utc.with_timezone(&utc.offset().fix()),
        TimeZoneMode::Computer => {
            let offset = utc.with_timezone(&Local).offset().fix();
            utc.with_timezone(&offset)
        }
        TimeZoneMode::Local => {
            if let Some(offset) = original_offset.and_then(parse_offset) {
                return utc.with_timezone(&offset);
            }
            if let Some(offset) = gps_timezone_offset(utc, metadata) {
                return utc.with_timezone(&offset);
            }
            let offset = utc.with_timezone(&Local).offset().fix();
            utc.with_timezone(&offset)
        }
    }
}

fn parse_offset(value: &str) -> Option<FixedOffset> {
    let value = value.trim();
    if value == "Z" {
        return FixedOffset::east_opt(0);
    }
    let sign = if value.starts_with('-') { -1 } else { 1 };
    let digits = value.trim_start_matches(['+', '-']).replace(':', "");
    if digits.len() != 4 || !digits.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    let hours = digits[..2].parse::<i32>().ok()?;
    let minutes = digits[2..].parse::<i32>().ok()?;
    if hours > 23 || minutes > 59 {
        return None;
    }
    FixedOffset::east_opt(sign * (hours * 3600 + minutes * 60))
}

fn gps_timezone_offset(utc: DateTime<Utc>, metadata: &TakeoutMetadata) -> Option<FixedOffset> {
    let (latitude, longitude) = (metadata.latitude?, metadata.longitude?);
    static FINDER: OnceLock<tzf_rs::DefaultFinder> = OnceLock::new();
    let finder = FINDER.get_or_init(tzf_rs::DefaultFinder::new);
    let timezone_name = finder.get_tz_name(longitude, latitude);
    let timezone = timezone_name.parse::<chrono_tz::Tz>().ok()?;
    Some(utc.with_timezone(&timezone).offset().fix())
}

fn tool_warnings(output: &str) -> Vec<String> {
    output
        .lines()
        .filter(|line| line.starts_with("Warning:"))
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgb};
    use std::path::PathBuf;

    #[test]
    fn parses_takeout_fields_and_string_timestamps() {
        let json = br#"{
            "title":"photo.jpg",
            "description":"A description",
            "photoTakenTime":{"timestamp":"1700000000"},
            "creationTime":{"timestamp":1699999999},
            "geoData":{"latitude":0,"longitude":0,"altitude":0},
            "geoDataExif":{"latitude":47.6,"longitude":-122.3,"altitude":12.5},
            "people":[{"name":"Ada"},{"name":"Lin"}],
            "favorited":true,
            "archived":true,
            "trashed":true,
            "url":"https://photos.example/item"
        }"#;
        let metadata = TakeoutMetadata::from_reader(json.as_slice()).expect("parse metadata");

        assert_eq!(metadata.taken_timestamp(), Some(1_700_000_000));
        assert_eq!(metadata.latitude, Some(47.6));
        assert_eq!(metadata.longitude, Some(-122.3));
        assert_eq!(metadata.altitude, Some(12.5));
        assert_eq!(metadata.people, vec!["Ada", "Lin"]);
        assert!(metadata.favorited && metadata.archived && metadata.trashed);
        assert_eq!(metadata.url.as_deref(), Some("https://photos.example/item"));
    }

    #[test]
    fn metadata_tags_respect_toggles_and_timezone() {
        let metadata = TakeoutMetadata {
            photo_taken_time: Some(1_700_000_000),
            description: Some("A caption".to_string()),
            latitude: Some(47.6),
            longitude: Some(-122.3),
            altitude: Some(12.5),
            people: vec!["Ada".to_string()],
            favorited: true,
            ..Default::default()
        };
        let tags = metadata_tags(
            "out.jpg",
            &metadata,
            TimeZoneMode::Utc,
            None,
            false,
            false,
            false,
        )
        .expect("build tags");

        assert_eq!(
            tags.get("EXIF:DateTimeOriginal"),
            Some(&Value::String("2023:11:14 22:13:20".to_string()))
        );
        assert!(!tags.contains_key("EXIF:GPSLatitude"));
        assert!(!tags.contains_key("XMP-dc:Description"));
        assert!(!tags.contains_key("XMP:Rating"));

        let enabled_tags = metadata_tags(
            "out.jpg",
            &metadata,
            TimeZoneMode::Utc,
            None,
            true,
            true,
            true,
        )
        .expect("build enabled tags");
        assert_eq!(
            enabled_tags.get("EXIF:GPSLatitude"),
            Some(&Value::from(47.6))
        );
        assert_eq!(
            enabled_tags.get("XMP-dc:Description"),
            Some(&Value::String("A caption".to_string()))
        );
        assert_eq!(
            enabled_tags.get("XMP-iptcExt:PersonInImage"),
            Some(&Value::Array(vec![Value::String("Ada".to_string())]))
        );
        assert_eq!(enabled_tags.get("XMP:Rating"), Some(&Value::from(5)));

        let utc = DateTime::<Utc>::from_timestamp(1_700_000_000, 0).expect("valid timestamp");
        let local = datetime_with_offset(utc, TimeZoneMode::Local, Some("+02:00"), &metadata);
        assert_eq!(local.format("%:z").to_string(), "+02:00");

        let video_tags = metadata_tags(
            "clip.mov",
            &metadata,
            TimeZoneMode::Utc,
            None,
            true,
            true,
            true,
        )
        .expect("build video tags");
        assert!(video_tags.contains_key("QuickTime:CreateDate"));
        assert!(!video_tags.contains_key("EXIF:DateTimeOriginal"));
        assert!(video_tags.contains_key("QuickTime:GPSCoordinates"));
    }

    #[test]
    fn supported_formats_include_raw_and_common_video() {
        assert!(supports_embedded_metadata(Path::new("IMG_1.dng")));
        assert!(supports_embedded_metadata(Path::new("clip.mov")));
        assert!(!supports_embedded_metadata(Path::new("archive.bin")));
    }

    #[test]
    fn png_pixel_hash_is_unchanged_when_exiftool_is_available() {
        let executable = std::env::var_os("TAKEBACK_EXIFTOOL")
            .map(PathBuf::from)
            .or_else(|| which_exiftool());
        let Some(executable) = executable else {
            return;
        };
        let temp = tempfile::tempdir().expect("create test directory");
        let image_path = temp.path().join("synthetic.png");
        let pixels = ImageBuffer::from_fn(3, 2, |x, y| {
            Rgb([(x * 40) as u8, (y * 80) as u8, (x + y) as u8])
        });
        pixels.save(&image_path).expect("write test image");
        let before = pixel_hash(&image_path);
        let mut exiftool = ExifTool::new(&executable).expect("start ExifTool");
        let metadata = TakeoutMetadata {
            photo_taken_time: Some(1_700_000_000),
            description: Some("Synthetic test image".to_string()),
            ..Default::default()
        };
        exiftool
            .write(
                &image_path,
                &metadata,
                TimeZoneMode::Utc,
                false,
                true,
                false,
            )
            .expect("write metadata");
        drop(exiftool);

        assert_eq!(before, pixel_hash(&image_path));
        assert_eq!(
            std::fs::read_dir(temp.path())
                .expect("list outputs")
                .count(),
            1,
            "temporary JSON metadata must be removed"
        );
    }

    fn pixel_hash(path: &Path) -> blake3::Hash {
        let image = image::open(path).expect("decode image");
        blake3::hash(&image.to_rgb8().into_raw())
    }

    fn which_exiftool() -> Option<PathBuf> {
        let name = if cfg!(windows) {
            "exiftool.exe"
        } else {
            "exiftool"
        };
        std::env::split_paths(&std::env::var_os("PATH")?).find_map(|folder| {
            let candidate = folder.join(name);
            candidate.is_file().then_some(candidate)
        })
    }
}
