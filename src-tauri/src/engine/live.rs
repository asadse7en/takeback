use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::metadata::ExifTool;

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
            Self::Iphone => "iphonePair",
            Self::Android => "androidMotionPhoto",
        }
    }

    pub fn from_value(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "iphone" | "iphonepair" => Self::Iphone,
            "android" | "androidmotionphoto" => Self::Android,
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
    exiftool: &mut ExifTool,
    style: LiveStyle,
    photo_path: &Path,
    video_path: Option<&Path>,
    content_identifier: &str,
) -> Result<(), String> {
    match style {
        LiveStyle::Separate => Ok(()),
        LiveStyle::Iphone => exiftool.write_live_content_identifier(photo_path, content_identifier),
        LiveStyle::Android => {
            let video_path = video_path.ok_or_else(|| {
                "the matched video source is unavailable for this motion photo".to_string()
            })?;
            exiftool.embed_motion_video(photo_path, video_path, content_identifier)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::LiveStyle;

    #[test]
    fn live_style_defaults_to_separate() {
        assert_eq!(LiveStyle::default(), LiveStyle::Separate);
        assert_eq!(LiveStyle::from("iphone"), LiveStyle::Iphone);
        assert_eq!(LiveStyle::from("android"), LiveStyle::Android);
        assert_eq!(LiveStyle::from("iphonePair"), LiveStyle::Iphone);
        assert_eq!(LiveStyle::from("androidMotionPhoto"), LiveStyle::Android);
        assert_eq!(LiveStyle::Iphone.as_str(), "iphonePair");
        assert_eq!(LiveStyle::Android.as_str(), "androidMotionPhoto");
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
