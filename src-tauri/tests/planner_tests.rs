use std::fs;

use takeback_lib::planner::{
    plan, AlbumLocation, AlbumMode, DateFolderMode, FavoritesMode, PlacementKind, PlannerSettings,
    SplitByType,
};

#[test]
fn planner_fixture_uses_top_level_albums_and_flat_favorite_links() {
    let raw = fs::read_to_string("tests/fixtures/planner_cases.json").unwrap();
    let items: Vec<takeback_lib::planner::MediaItem> = serde_json::from_str(&raw).unwrap();

    let settings = PlannerSettings {
        album_mode: AlbumMode::Link,
        favorites_mode: FavoritesMode::Link,
        date_folders: DateFolderMode::Month,
        split_by_type: SplitByType { photos: false, videos: true, live: false },
        ..PlannerSettings::default()
    };

    let placements = plan(&items, &settings);
    assert_eq!(settings.album_loc, AlbumLocation::Top);
    assert!(placements.iter().any(|item| {
        item.dest.ends_with("/2024/05/IMG_0134.jpg") && item.kind == PlacementKind::Original
    }));
    assert!(placements.iter().any(|item| {
        item.dest.ends_with("/Paris trip/IMG_0134.jpg") && item.kind == PlacementKind::Link
    }));
    assert!(placements.iter().any(|item| {
        item.dest.ends_with("/Favorites/IMG_0134.jpg")
            && item.kind == PlacementKind::Link
            && item.tags == ["Link"]
    }));
    assert!(placements.iter().any(|item| item.dest.ends_with("/2024/05/Videos/IMG_0135.mov")));
}
