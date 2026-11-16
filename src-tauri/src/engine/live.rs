use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LiveStyle {
    Separate,
    Iphone,
    Android,
}

impl Default for LiveStyle {
    fn default() -> Self {
        Self::Separate
    }
}

impl LiveStyle {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Separate => "separate",
            Self::Iphone => "iphone",
            Self::Android => "android",
        }
    }

    pub fn from_value(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "iphone" => Self::Iphone,
            "android" => Self::Android,
            _ => Self::Separate,
        }
    }

    pub fn is_experimental(self) -> bool {
        matches!(self, Self::Iphone | Self::Android)
    }

    pub fn warning_text(self) -> Option<&'static str> {
        if self.is_experimental() {
            Some("The photo file is modified.")
        } else {
            None
        }
    }
}

impl From<&str> for LiveStyle {
    fn from(value: &str) -> Self {
        Self::from_value(value)
    }
}

impl From<String> for LiveStyle {
    fn from(value: String) -> Self {
        Self::from_value(&value)
    }
}

impl std::fmt::Display for LiveStyle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

pub fn apply_live_style(
    style: LiveStyle,
    photo_path: &Path,
    video_path: &Path,
) -> Result<(), String> {
    match style {
        LiveStyle::Separate => Ok(()),
        LiveStyle::Iphone => apply_iphone_live_photo(photo_path, video_path),
        LiveStyle::Android => apply_android_motion_photo(photo_path, video_path),
    }
}

fn apply_iphone_live_photo(photo_path: &Path, video_path: &Path) -> Result<(), String> {
    let content_id = build_content_identifier(photo_path, video_path, "live-photo");
    let photo_args = vec![
        "-overwrite_original".to_string(),
        format!("-XMP:ContentIdentifier={content_id}"),
        format!("-ContentIdentifier={content_id}"),
        photo_path.to_string_lossy().to_string(),
    ];
    let video_args = vec![
        "-overwrite_original".to_string(),
        format!("-XMP:ContentIdentifier={content_id}"),
        format!("-ContentIdentifier={content_id}"),
        video_path.to_string_lossy().to_string(),
    ];

    run_exiftool(&photo_args)?;
    run_exiftool(&video_args)?;
    Ok(())
}

fn apply_android_motion_photo(photo_path: &Path, video_path: &Path) -> Result<(), String> {
    let content_id = build_content_identifier(photo_path, video_path, "motion-photo");
    let args = vec![
        "-overwrite_original".to_string(),
        "-XMP-GCamera:MicroVideo=1".to_string(),
        "-XMP-GCamera:MicroVideoVersion=1".to_string(),
        "-XMP-GCamera:MotionPhoto=1".to_string(),
        format!("-XMP-GCamera:ContentIdentifier={content_id}"),
        format!("-XMP:ContentIdentifier={content_id}"),
        format!("-EmbeddedVideoFile={}", video_path.to_string_lossy()),
        photo_path.to_string_lossy().to_string(),
    ];

    run_exiftool(&args)?;
    Ok(())
}

fn build_content_identifier(photo_path: &Path, video_path: &Path, prefix: &str) -> String {
    let photo_name = photo_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("photo")
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>();
    let video_name = video_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("video")
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>();
    let pair = format!("{prefix}-{photo_name}-{video_name}");
    let safe = pair.to_ascii_lowercase();
    let trimmed = safe.trim_start_matches('-');
    if trimmed.is_empty() {
        "takeback-live-photo".to_string()
    } else {
        trimmed.to_string()
    }
}

fn run_exiftool(args: &[String]) -> Result<(), String> {
    if !has_exiftool() {
        return Ok(());
    }

    let status = Command::new("exiftool")
        .args(args)
        .status()
        .map_err(|err| format!("run exiftool: {err}"))?;

    if !status.success() {
        return Err("exiftool failed while preparing a live photo pair".to_string());
    }

    Ok(())
}

fn has_exiftool() -> bool {
    Command::new("exiftool")
        .arg("-ver")
        .output()
        .map(|result| result.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::LiveStyle;

    #[test]
    fn live_style_defaults_to_separate() {
        assert_eq!(LiveStyle::default(), LiveStyle::Separate);
        assert_eq!(LiveStyle::from("iphone"), LiveStyle::Iphone);
        assert_eq!(LiveStyle::from("android"), LiveStyle::Android);
    }

    #[test]
    fn experimental_styles_show_the_warning() {
        assert_eq!(
            LiveStyle::Iphone.warning_text(),
            Some("The photo file is modified.")
        );
        assert_eq!(
            LiveStyle::Android.warning_text(),
            Some("The photo file is modified.")
        );
        assert_eq!(LiveStyle::Separate.warning_text(), None);
    }
}
