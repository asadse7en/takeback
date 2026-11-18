use std::fs;

use takeback_lib::takeout::matcher::{
    match_live_photos, read_video_duration_seconds, verify_android_motion_photo, LivePhotoCandidate,
};

#[test]
fn live_photo_fixture_pairs_unknown_durations_by_name_and_folder() {
    let raw = fs::read_to_string("tests/fixtures/live_photo_cases.json").unwrap();
    let candidates: Vec<LivePhotoCandidate> = serde_json::from_str(&raw).unwrap();
    let detection = match_live_photos(&candidates);

    assert_eq!(detection.pairs.len(), 3);
    for (photo, video) in [
        ("Photos/IMG_100.jpg", "Photos/IMG_100.mov"),
        ("Photos/IMG_400.jpg", "Photos/IMG_400.mp4"),
        ("Photos/IMG_700.jpg", "Photos/IMG_700.mp4"),
    ] {
        assert!(detection
            .pairs
            .iter()
            .any(|pair| { pair.photo_path == photo && pair.video_path == video }));
    }
    assert_eq!(
        detection.embedded_motion_photos,
        vec!["Photos/IMG_500.MP.jpg"]
    );
    assert!(detection.warnings.iter().any(|warning| {
        warning.message.contains("different folders")
            && warning.paths.contains(&"Album A/IMG_300.jpg".to_string())
    }));
    assert!(detection.warnings.iter().any(|warning| {
        warning.message.contains("No Motion Photo XMP marker")
            && warning.paths.contains(&"Photos/IMG_600.MP.jpg".to_string())
    }));
    assert!(detection.warnings.iter().any(|warning| {
        warning.message.contains("More than one")
            && warning.paths.contains(&"Photos/IMG_200.png".to_string())
    }));
    assert!(detection.warnings.iter().any(|warning| {
        warning.message.contains("longer than a typical live photo")
            && warning.paths.contains(&"Photos/IMG_800.mp4".to_string())
    }));
}

#[test]
fn android_motion_photo_requires_motion_xmp_and_an_mp4_signature() {
    let verified = b"<x:xmpmeta>GCamera:MicroVideo</x:xmpmeta> .... ftypisom";
    assert!(verify_android_motion_photo(&verified[..]).unwrap());
    assert!(!verify_android_motion_photo(&b"GCamera:MicroVideo only"[..]).unwrap());
    assert!(!verify_android_motion_photo(&b"ftypisom only"[..]).unwrap());
}

#[test]
fn video_duration_reader_streams_movie_header() {
    fn atom(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&((payload.len() + 8) as u32).to_be_bytes());
        bytes.extend_from_slice(kind);
        bytes.extend_from_slice(payload);
        bytes
    }

    let mut movie_header = vec![0, 0, 0, 0];
    movie_header.extend_from_slice(&0_u32.to_be_bytes());
    movie_header.extend_from_slice(&0_u32.to_be_bytes());
    movie_header.extend_from_slice(&1_000_u32.to_be_bytes());
    movie_header.extend_from_slice(&2_500_u32.to_be_bytes());
    let mvhd = atom(b"mvhd", &movie_header);
    let moov = atom(b"moov", &mvhd);

    assert_eq!(read_video_duration_seconds(&moov[..]).unwrap(), Some(2.5));
}
