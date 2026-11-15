# Live photo validation checklist

Do not claim the live-photo modes work until the checks below pass on real devices and exports.

## iPhone (via AirDrop or iCloud)

- [ ] Export a live photo from iPhone using AirDrop or iCloud.
- [ ] Confirm the photo is a HEIC/JPG plus an MP4/MOV pair with the same base name.
- [ ] Open the exported pair in Apple Photos and confirm the pair remains connected after import.
- [ ] Verify the content identifier is stable across the same exported pair and file metadata is preserved.
- [ ] Check that the resulting photo file still opens normally and is not corrupted.

## Google Photos export

- [ ] Import a Google Photos Takeout containing live photos exported from an iPhone.
- [ ] Confirm the Takeout includes the correct image/video pairing and matching metadata.
- [ ] Run the app with `liveStyle=iphone` and verify the produced output preserves pairing in Apple Photos.
- [ ] Repeat with `liveStyle=separate` and confirm it stays as two ordinary files.

## Pixel / Samsung gallery motion photos

- [ ] Export a motion photo from a Pixel or Samsung device to the file system.
- [ ] Confirm the output contains a JPEG with embedded motion metadata and a matching video stream.
- [ ] Run the app with `liveStyle=android` and verify the JPEG keeps the motion-photo metadata after export.
- [ ] Confirm the resulting file opens normally in the gallery and the video remains associated with the still image.
- [ ] Check that the file still reads correctly on a machine without the original device metadata.

## Exit criteria

The feature is not ready to claim as supported until each checklist item above passes on a real device export and the output can be opened without corruption.
