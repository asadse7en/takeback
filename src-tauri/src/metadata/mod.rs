use std::fs;
use std::io::{BufRead, BufReader, Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime};

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
    pub fn from_reader(mut reader: impl Read) -> Result<Self, String> {
        let mut prefix = [0; 3];
        let mut prefix_len = 0;
        while prefix_len < prefix.len() {
            match reader.read(&mut prefix[prefix_len..]) {
                Ok(0) => break,
                Ok(count) => prefix_len += count,
                Err(error) => {
                    return Err(format!("read Takeout JSON sidecar: {error}"));
                }
            }
        }
        let prefix = if prefix_len == 3 && prefix == [0xEF, 0xBB, 0xBF] {
            Vec::new()
        } else {
            prefix[..prefix_len].to_vec()
        };
        let reader = Cursor::new(prefix).chain(reader);
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
                | "jfif"
                | "png"
                | "webp"
                | "avif"
                | "heic"
                | "heif"
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
                | "mpeg"
                | "3g2"
                | "mp"
        )
    )
}

pub const EXIFTOOL_VIDEO_MAX_BYTES: u64 = 500_000_000;
pub const EXIFTOOL_BATCH_SIZE: usize = 25;

pub fn is_large_video(path: &Path, size_bytes: u64) -> bool {
    size_bytes > EXIFTOOL_VIDEO_MAX_BYTES
        && matches!(
            path.extension()
                .and_then(|extension| extension.to_str())
                .map(str::to_ascii_lowercase)
                .as_deref(),
            Some(
                "mp4"
                    | "mov"
                    | "m4v"
                    | "avi"
                    | "mkv"
                    | "3gp"
                    | "mts"
                    | "m2ts"
                    | "webm"
                    | "mpg"
                    | "mpeg"
                    | "wmv"
                    | "flv"
                    | "3g2"
                    | "mp"
            )
        )
}

#[derive(Clone)]
pub struct MetadataWriteRequest {
    pub path: PathBuf,
    pub metadata: TakeoutMetadata,
    pub time_zone: TimeZoneMode,
    pub timezone_name: Option<String>,
    pub write_gps: bool,
    pub write_descriptions: bool,
    pub rate_favorites: bool,
}

pub struct ExifTool {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    stderr: Receiver<String>,
    stderr_reader: Option<JoinHandle<()>>,
}

impl ExifTool {
    pub fn new(executable: &Path) -> Result<Self, String> {
        let mut command = Command::new(executable);
        command
            .args([
                "-stay_open",
                "True",
                "-@",
                "-",
                "-common_args",
                "-charset",
                "filename=UTF8",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command
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
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| "could not open ExifTool stderr".to_string())?;
        let (stderr_sender, stderr_receiver) = mpsc::channel();
        let stderr_reader = thread::spawn(move || {
            for line in BufReader::new(stderr).lines() {
                match line {
                    Ok(line) => {
                        if stderr_sender.send(line).is_err() {
                            break;
                        }
                    }
                    Err(error) => {
                        let _ = stderr_sender
                            .send(format!("Error: could not read ExifTool stderr: {error}"));
                        break;
                    }
                }
            }
        });
        let mut tool = Self {
            child,
            input,
            output: BufReader::new(output),
            stderr: stderr_receiver,
            stderr_reader: Some(stderr_reader),
        };
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
        timezone_name: Option<&str>,
        write_gps: bool,
        write_descriptions: bool,
        rate_favorites: bool,
    ) -> Result<Vec<String>, String> {
        let request = MetadataWriteRequest {
            path: path.to_path_buf(),
            metadata: metadata.clone(),
            time_zone,
            timezone_name: timezone_name.map(str::to_string),
            write_gps,
            write_descriptions,
            rate_favorites,
        };
        let warnings = self
            .write_batch(std::slice::from_ref(&request))?
            .into_iter()
            .next()
            .ok_or_else(|| "ExifTool did not return a metadata result".to_string())?;
        if let Some(error) = warnings
            .iter()
            .find(|warning| is_exiftool_failure_warning(warning))
        {
            return Err(error.clone());
        }
        Ok(warnings)
    }

    pub fn write_batch(
        &mut self,
        requests: &[MetadataWriteRequest],
    ) -> Result<Vec<Vec<String>>, String> {
        let mut warnings = vec![Vec::new(); requests.len()];
        for batch in metadata_batch_ranges(requests.len()) {
            let mut objects = Vec::new();
            let mut object_indices = Vec::new();

            for index in batch {
                let request = &requests[index];
                if is_large_video(
                    &request.path,
                    fs::metadata(&request.path)
                        .map_err(|error| {
                            format!(
                                "inspect video before metadata write {}: {error}",
                                request.path.display()
                            )
                        })?
                        .len(),
                ) {
                    if let Err(error) = self.set_modified_time(&request.path, &request.metadata) {
                        warnings[index].push(format!("Error: {error}"));
                    }
                    warnings[index].push(
                        "ExifTool metadata rewrite was skipped for a video over 500 MB; only the file modified time was set when available.".to_string(),
                    );
                    continue;
                }
                let Some(source_file) = request.path.to_str() else {
                    warnings[index].push(format!(
                        "Error: file path is not valid Unicode: {}",
                        request.path.display()
                    ));
                    continue;
                };
                let original_offset = if request.time_zone == TimeZoneMode::Local
                    && request.timezone_name.is_none()
                    && request.metadata.taken_timestamp().is_some()
                {
                    match self.read_original_offset(source_file) {
                        Ok(offset) => offset,
                        Err(error) => {
                            warnings[index].push(format!("Error: {error}"));
                            continue;
                        }
                    }
                } else {
                    None
                };
                let tags = match metadata_tags(
                    source_file,
                    &request.metadata,
                    request.time_zone,
                    request.timezone_name.as_deref(),
                    original_offset.as_deref(),
                    request.write_gps,
                    request.write_descriptions,
                    request.rate_favorites,
                ) {
                    Ok(tags) => tags,
                    Err(error) => {
                        warnings[index].push(format!("Error: {error}"));
                        continue;
                    }
                };
                if tags.len() <= 1 {
                    if let Err(error) = self.set_modified_time(&request.path, &request.metadata) {
                        warnings[index].push(format!("Error: {error}"));
                    }
                    continue;
                }
                objects.push(Value::Object(tags));
                object_indices.push(index);
            }

            if objects.is_empty() {
                continue;
            }
            let first_path = &requests[object_indices[0]].path;
            let parent = first_path
                .parent()
                .ok_or_else(|| format!("output path has no parent: {}", first_path.display()))?;
            let mut json_file = tempfile::Builder::new()
                .prefix(".takeback-metadata-")
                .suffix(".json")
                .tempfile_in(parent)
                .map_err(|error| format!("create temporary metadata file: {error}"))?;
            serde_json::to_writer(&mut json_file, &Value::Array(objects)).map_err(|error| {
                format!(
                    "serialize metadata batch starting at {}: {error}",
                    first_path.display()
                )
            })?;
            json_file
                .flush()
                .map_err(|error| format!("flush temporary metadata file: {error}"))?;
            let json_file_path = json_file.into_temp_path();
            let json_path = json_file_path
                .to_str()
                .ok_or_else(|| "temporary metadata path is not valid Unicode".to_string())?;
            let output = self.execute_raw(&[
                "-overwrite_original".to_string(),
                format!("-json={json_path}"),
            ]);
            drop(json_file_path);

            let diagnostics = match output {
                Ok(output) => tool_warnings(&output),
                Err(error) => vec![format!("Error: {error}")],
            };
            distribute_batch_warnings(&diagnostics, &object_indices, requests, &mut warnings);

            for index in object_indices {
                if let Err(error) =
                    self.set_modified_time(&requests[index].path, &requests[index].metadata)
                {
                    warnings[index].push(format!("Error: {error}"));
                }
            }
        }
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
        let response = self.execute_raw(args)?;
        if let Some(error) = response
            .lines()
            .find(|line| line.trim_start().starts_with("Error:"))
        {
            return Err(error.trim().to_string());
        }
        Ok(response)
    }

    fn execute_raw(&mut self, args: &[String]) -> Result<String, String> {
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

        loop {
            match self.stderr.recv_timeout(Duration::from_millis(10)) {
                Ok(line) => {
                    response.push_str(&line);
                    response.push('\n');
                }
                Err(mpsc::RecvTimeoutError::Timeout | mpsc::RecvTimeoutError::Disconnected) => {
                    break;
                }
            }
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
        if let Some(stderr_reader) = self.stderr_reader.take() {
            let _ = stderr_reader.join();
        }
    }
}

fn metadata_tags(
    source_file: &str,
    metadata: &TakeoutMetadata,
    time_zone: TimeZoneMode,
    timezone_name: Option<&str>,
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
        Some("mp4" | "mov" | "m4v" | "mpeg" | "3g2" | "mp")
    );
    tags.insert(
        "SourceFile".to_string(),
        Value::String(source_file.to_string()),
    );

    if let Some(timestamp) = metadata.taken_timestamp() {
        let utc = DateTime::<Utc>::from_timestamp(timestamp, 0)
            .ok_or_else(|| format!("invalid Takeout timestamp: {timestamp}"))?;
        let datetime = datetime_with_offset(utc, time_zone, timezone_name, original_offset)?;
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
    timezone_name: Option<&str>,
    original_offset: Option<&str>,
) -> Result<DateTime<FixedOffset>, String> {
    if let Some(name) = timezone_name {
        let timezone = name.parse::<chrono_tz::Tz>().map_err(|_| {
            format!("The media item's time zone is not a recognized IANA zone: {name}")
        })?;
        return Ok(utc.with_timezone(&timezone).fixed_offset());
    }

    Ok(match time_zone {
        TimeZoneMode::Utc => utc.with_timezone(&utc.offset().fix()),
        TimeZoneMode::Computer => {
            let offset = utc.with_timezone(&Local).offset().fix();
            utc.with_timezone(&offset)
        }
        TimeZoneMode::Local => {
            if let Some(offset) = original_offset.and_then(parse_offset) {
                return Ok(utc.with_timezone(&offset));
            }
            let offset = utc.with_timezone(&Local).offset().fix();
            utc.with_timezone(&offset)
        }
    })
}

pub fn computer_timezone_name() -> Result<String, String> {
    let name = iana_time_zone::get_timezone()
        .map_err(|error| format!("Could not determine the computer time zone: {error}"))?;
    name.parse::<chrono_tz::Tz>()
        .map(|timezone| timezone.name().to_string())
        .map_err(|_| format!("The computer time zone is not a recognized IANA zone: {name}"))
}

pub fn timezone_name_for_item(
    time_zone: crate::settings::TimeZoneMode,
    metadata: Option<&TakeoutMetadata>,
    computer_timezone: Option<&str>,
) -> Result<String, String> {
    match time_zone {
        crate::settings::TimeZoneMode::Utc => Ok("UTC".to_string()),
        crate::settings::TimeZoneMode::Computer => computer_timezone
            .map(str::to_string)
            .ok_or_else(|| "The computer time zone was not resolved.".to_string()),
        crate::settings::TimeZoneMode::Local => gps_timezone_name(metadata)
            .or_else(|| computer_timezone.map(str::to_string))
            .ok_or_else(|| "The computer time zone was not resolved.".to_string()),
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

fn gps_timezone_name(metadata: Option<&TakeoutMetadata>) -> Option<String> {
    let metadata = metadata?;
    let (latitude, longitude) = (metadata.latitude?, metadata.longitude?);
    static FINDER: OnceLock<tzf_rs::DefaultFinder> = OnceLock::new();
    let finder = FINDER.get_or_init(tzf_rs::DefaultFinder::new);
    let timezone_name = finder.get_tz_name(longitude, latitude);
    timezone_name
        .parse::<chrono_tz::Tz>()
        .ok()
        .map(|timezone| timezone.name().to_string())
}

fn tool_warnings(output: &str) -> Vec<String> {
    output
        .lines()
        .filter(|line| {
            let line = line.trim_start();
            let lowercase = line.to_ascii_lowercase();
            line.starts_with("Warning:")
                || line.starts_with("Error:")
                || (lowercase.contains("files weren") && lowercase.contains("updated"))
        })
        .map(str::to_string)
        .collect()
}

fn metadata_batch_ranges(request_count: usize) -> impl Iterator<Item = std::ops::Range<usize>> {
    (0..request_count)
        .step_by(EXIFTOOL_BATCH_SIZE)
        .map(move |start| start..(start + EXIFTOOL_BATCH_SIZE).min(request_count))
}

fn is_exiftool_failure_warning(warning: &str) -> bool {
    let lowercase = warning.to_ascii_lowercase();
    warning.trim_start().starts_with("Error:")
        || (lowercase.contains("files weren") && lowercase.contains("updated"))
}

fn distribute_batch_warnings(
    diagnostics: &[String],
    object_indices: &[usize],
    requests: &[MetadataWriteRequest],
    warnings: &mut [Vec<String>],
) {
    let failed_indices = diagnostics
        .iter()
        .filter(|line| line.trim_start().starts_with("Error:"))
        .flat_map(|line| matching_request_indices(line, object_indices, requests))
        .collect::<std::collections::HashSet<_>>();

    for diagnostic in diagnostics {
        let normalized_diagnostic = normalize_diagnostic_path(diagnostic);
        let matching = object_indices
            .iter()
            .copied()
            .filter(|index| {
                let path = normalize_diagnostic_path(&requests[*index].path.to_string_lossy());
                normalized_diagnostic.contains(&path)
            })
            .collect::<Vec<_>>();
        let lowercase = diagnostic.to_ascii_lowercase();
        let targets = if lowercase.contains("files weren")
            && lowercase.contains("updated")
            && !failed_indices.is_empty()
        {
            failed_indices.iter().copied().collect::<Vec<_>>()
        } else if matching.is_empty() {
            object_indices.to_vec()
        } else {
            matching
        };
        for index in targets {
            warnings[index].push(diagnostic.clone());
        }
    }
}

fn matching_request_indices(
    diagnostic: &str,
    object_indices: &[usize],
    requests: &[MetadataWriteRequest],
) -> Vec<usize> {
    let normalized_diagnostic = normalize_diagnostic_path(diagnostic);
    object_indices
        .iter()
        .copied()
        .filter(|index| {
            let path = normalize_diagnostic_path(&requests[*index].path.to_string_lossy());
            normalized_diagnostic.contains(&path)
        })
        .collect()
}

fn normalize_diagnostic_path(value: &str) -> String {
    value.replace('\\', "/").to_lowercase()
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
    fn strips_utf8_bom_before_parsing() {
        let json = b"\xEF\xBB\xBF{\"title\":\"photo.jpg\"}";

        let metadata = TakeoutMetadata::from_reader(json.as_slice()).expect("parse metadata");

        assert_eq!(metadata.title.as_deref(), Some("photo.jpg"));
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
        let local = datetime_with_offset(
            utc,
            TimeZoneMode::Local,
            Some("America/Los_Angeles"),
            Some("+02:00"),
        )
        .expect("use item time zone");
        assert_eq!(local.format("%:z").to_string(), "-08:00");
        let offset_fallback = datetime_with_offset(utc, TimeZoneMode::Local, None, Some("+02:00"))
            .expect("use original offset fallback");
        assert_eq!(offset_fallback.format("%:z").to_string(), "+02:00");

        let video_tags = metadata_tags(
            "clip.mov",
            &metadata,
            TimeZoneMode::Utc,
            None,
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
        assert!(supports_embedded_metadata(Path::new("clip.mpeg")));
        assert!(supports_embedded_metadata(Path::new("photo.avif")));
        assert!(supports_embedded_metadata(Path::new("photo.jfif")));
        for path in [
            "animation.gif",
            "image.bmp",
            "clip.webm",
            "clip.mpg",
            "clip.wmv",
            "clip.flv",
        ] {
            assert!(!supports_embedded_metadata(Path::new(path)));
        }
        assert!(!supports_embedded_metadata(Path::new("archive.bin")));
    }

    #[test]
    fn large_video_metadata_is_skipped_above_the_limit_only() {
        assert!(!is_large_video(
            Path::new("clip.mp4"),
            EXIFTOOL_VIDEO_MAX_BYTES
        ));
        assert!(is_large_video(
            Path::new("clip.mp4"),
            EXIFTOOL_VIDEO_MAX_BYTES + 1
        ));
        assert!(is_large_video(
            Path::new("clip.webm"),
            EXIFTOOL_VIDEO_MAX_BYTES + 1
        ));
        assert!(!is_large_video(
            Path::new("photo.jpg"),
            EXIFTOOL_VIDEO_MAX_BYTES + 1
        ));
    }

    #[test]
    fn metadata_batches_contain_at_most_twenty_five_requests() {
        let batch_lengths = metadata_batch_ranges(51)
            .map(|range| range.len())
            .collect::<Vec<_>>();
        assert_eq!(batch_lengths, [25, 25, 1]);
    }

    #[test]
    fn batch_error_and_not_updated_lines_are_file_warnings() {
        let requests = ["C:/Photos/first.jpg", "C:/Photos/second.jpg"]
            .into_iter()
            .map(|path| MetadataWriteRequest {
                path: PathBuf::from(path),
                metadata: TakeoutMetadata::default(),
                time_zone: TimeZoneMode::Utc,
                timezone_name: None,
                write_gps: false,
                write_descriptions: false,
                rate_favorites: false,
            })
            .collect::<Vec<_>>();
        let diagnostics = vec![
            "Error: could not write C:\\Photos\\first.jpg".to_string(),
            "1 files weren't updated due to errors".to_string(),
        ];
        let mut warnings = vec![Vec::new(); requests.len()];

        distribute_batch_warnings(&diagnostics, &[0, 1], &requests, &mut warnings);

        assert_eq!(warnings[0], diagnostics);
        assert!(warnings[1].is_empty());
        assert_eq!(tool_warnings(&diagnostics.join("\n")), diagnostics);
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
                None,
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
