use std::fs;

use takeback_lib::matcher::match_live_pairs;

#[test]
fn matcher_fixture_pairs_photo_and_video_by_shared_prefix() {
    let raw = fs::read_to_string("tests/fixtures/matcher_cases.json").unwrap();
    let items: Vec<String> = serde_json::from_str(&raw).unwrap();

    let pairs = match_live_pairs(&items.iter().map(String::as_str).collect::<Vec<_>>());
    assert_eq!(pairs.len(), 2);
    assert_eq!(pairs[0].photo, "IMG_001.jpg");
    assert_eq!(pairs[0].video, "IMG_001.mp4");
}
