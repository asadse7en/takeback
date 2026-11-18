use serde::{Deserialize, Serialize};
use std::path::{Component, Path};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PreviewResult {
    pub files: Vec<String>,
    pub tree: Vec<PreviewNode>,
    pub warnings: Vec<String>,
    pub required_bytes: u64,
    pub free_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PreviewNode {
    pub name: String,
    pub folder: bool,
    pub tag: Option<String>,
    pub children: Vec<PreviewNode>,
}

pub fn tree_from_placements(
    root: &Path,
    placements: &[crate::planner::Placement],
) -> Vec<PreviewNode> {
    let mut root_node = PreviewNode {
        name: root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| root.to_string_lossy().into_owned()),
        folder: true,
        tag: None,
        children: Vec::new(),
    };

    for placement in placements {
        let destination = Path::new(&placement.dest);
        let Ok(relative) = destination.strip_prefix(root) else {
            continue;
        };
        let parts = relative
            .components()
            .filter_map(|component| match component {
                Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
                _ => None,
            })
            .collect::<Vec<_>>();
        insert_preview_path(&mut root_node, &parts, placement.tags.first().cloned());
    }

    vec![root_node]
}

fn insert_preview_path(node: &mut PreviewNode, parts: &[String], tag: Option<String>) {
    let Some((name, remaining)) = parts.split_first() else {
        return;
    };
    let is_file = remaining.is_empty();
    let child_index = node
        .children
        .iter()
        .position(|child| child.name.eq_ignore_ascii_case(name) && child.folder != is_file);
    let index = child_index.unwrap_or_else(|| {
        node.children.push(PreviewNode {
            name: name.clone(),
            folder: !is_file,
            tag: if is_file { tag.clone() } else { None },
            children: Vec::new(),
        });
        node.children.len() - 1
    });
    if !is_file {
        insert_preview_path(&mut node.children[index], remaining, tag);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner::{Placement, PlacementKind};

    #[test]
    fn tree_groups_paths_and_keeps_link_tags() {
        let root = Path::new("Photo Library");
        let placements = vec![
            Placement {
                src: "source/photo.jpg".to_string(),
                dest: root
                    .join("2024")
                    .join("05")
                    .join("photo.jpg")
                    .to_string_lossy()
                    .into(),
                kind: PlacementKind::Original,
                tags: Vec::new(),
                live_action: None,
            },
            Placement {
                src: "source/photo.jpg".to_string(),
                dest: root
                    .join("Favorites")
                    .join("photo.jpg")
                    .to_string_lossy()
                    .into(),
                kind: PlacementKind::Link,
                tags: vec!["Link".to_string()],
                live_action: None,
            },
        ];

        let tree = tree_from_placements(root, &placements);
        assert_eq!(tree[0].name, "Photo Library");
        assert_eq!(tree[0].children.len(), 2);
        assert!(tree[0].children.iter().any(
            |node| node.name == "Favorites" && node.children[0].tag.as_deref() == Some("Link")
        ));
    }
}
