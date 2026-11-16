use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LivePhotoStyle {
    #[default]
    Separate,
    IphonePair,
    AndroidMotionPhoto,
}
