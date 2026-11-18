use crate::report::ReportDocument;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::planner::{MediaItem, Placement, PlacementKind};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessingStartResult {
    pub accepted: bool,
    pub required_bytes: u64,
    pub free_bytes: u64,
    pub shortfall_bytes: u64,
    pub warnings: Vec<String>,
    pub report: Option<ReportDocument>,
}

pub fn estimate_required_bytes(
    placements: &[Placement],
    items: &[MediaItem],
) -> Result<u64, String> {
    let sizes = items
        .iter()
        .map(|item| (item.source.as_str(), item.size_bytes))
        .collect::<HashMap<_, _>>();
    let total = placements
        .iter()
        .filter(|placement| {
            matches!(
                placement.kind,
                PlacementKind::Original | PlacementKind::Copy | PlacementKind::Link
            )
        })
        .try_fold(0_u64, |total, placement| {
            let size = sizes
                .get(placement.src.as_str())
                .ok_or_else(|| format!("Could not find the size for source {}", placement.src))?;
            total
                .checked_add(*size)
                .ok_or_else(|| "The required output size is too large to calculate.".to_string())
        })?;
    let overhead = total
        .checked_add(19)
        .ok_or_else(|| "The required output size is too large to calculate.".to_string())?
        / 20;
    total
        .checked_add(overhead)
        .ok_or_else(|| "The required output size is too large to calculate.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_includes_link_bytes_for_copy_fallback() {
        let items = ["original", "copy", "link"]
            .into_iter()
            .zip([100_u64, 50, 200])
            .map(|(source, size_bytes)| MediaItem {
                source: source.to_string(),
                filename: format!("{source}.jpg"),
                album: None,
                additional_albums: Vec::new(),
                date: None,
                time: None,
                tz: None,
                size_bytes,
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
            })
            .collect::<Vec<_>>();
        let placements = vec![
            placement("original", PlacementKind::Original),
            placement("copy", PlacementKind::Copy),
            placement("link", PlacementKind::Link),
        ];

        assert_eq!(estimate_required_bytes(&placements, &items), Ok(368));
    }

    fn placement(source: &str, kind: PlacementKind) -> Placement {
        Placement {
            src: source.to_string(),
            dest: format!("out/{source}.jpg"),
            kind,
            tags: Vec::new(),
            live_action: None,
        }
    }
}
