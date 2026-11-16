#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchPair {
    pub photo: String,
    pub video: String,
}

pub fn match_live_pairs(items: &[&str]) -> Vec<MatchPair> {
    let mut out = Vec::new();
    let mut seen: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();

    for item in items {
        if item.ends_with(".jpg") || item.ends_with(".jpeg") || item.ends_with(".png") {
            let key = item
                .trim_end_matches(|c| {
                    c == '.' || c == 'j' || c == 'p' || c == 'g' || c == 'e' || c == 'n'
                })
                .trim_end_matches('_');
            seen.insert(key, seen.get(key).copied().unwrap_or_default() + 1);
        }
    }

    for item in items {
        if item.ends_with(".mp4") || item.ends_with(".mov") {
            let key = item.trim_end_matches(|c| {
                c == '.' || c == 'm' || c == 'p' || c == '4' || c == 'o' || c == 'v'
            });
            if let Some(_) = seen.get(key) {
                out.push(MatchPair {
                    photo: format!("{}.jpg", key),
                    video: item.to_string(),
                });
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::match_live_pairs;

    #[test]
    fn matches_live_photo_pairs_by_shared_prefix() {
        let pairs = match_live_pairs(&["IMG_001.jpg", "IMG_001.mp4"]);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].photo, "IMG_001.jpg");
        assert_eq!(pairs[0].video, "IMG_001.mp4");
    }
}
