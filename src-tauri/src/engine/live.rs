use std::path::Path;

use crate::livephoto::LivePhotoStyle;
use crate::metadata::ExifTool;

pub fn apply_live_style(
    exiftool: &mut ExifTool,
    style: LivePhotoStyle,
    photo_path: &Path,
    video_path: Option<&Path>,
    content_identifier: &str,
) -> Result<(), String> {
    match style {
        LivePhotoStyle::Separate => Ok(()),
        LivePhotoStyle::IphonePair => {
            exiftool.write_live_content_identifier(photo_path, content_identifier)
        }
        LivePhotoStyle::AndroidMotionPhoto => {
            let video_path = video_path.ok_or_else(|| {
                "the matched video source is unavailable for this motion photo".to_string()
            })?;
            exiftool.embed_motion_video(photo_path, video_path, content_identifier)
        }
    }
}
