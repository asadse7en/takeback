# Google Takeout (Google Photos) notes
Items marked (verify) must be confirmed on a real export; record findings here.

- **Archives**: .zip or .tgz in numbered parts (`takeout-<date>T<time>Z-001.zip`). A photo and its JSON can be in different parts: index all parts first. Detect numbering gaps.
- **Layout**: `Takeout/<Google Photos folder>/<album or "Photos from YYYY">/`. The Google Photos folder name is **localized** (e.g. `Google Fotos`): find it by contents (media + .json), never by name. Album folders hold copies of photos that are also in the year folders. Album folders have `metadata.json` (album title).
- **Sidecar fields**: `title`, `description`, `photoTakenTime.timestamp` (UTC seconds), `creationTime`, `geoData` {latitude, longitude, altitude} (0,0 = none; also check `geoDataExif`), `people[].name`, `favorited`, `archived`, `trashed` (verify whether trashed items appear), `url`.
- **JSON names (verify)**: `IMG_1.jpg.json`; newer `IMG_1.jpg.supplemental-metadata.json`, sometimes truncated; long names cut at about 46 chars; duplicates `IMG_1(1).jpg` <-> `IMG_1.jpg(1).json`; `IMG_1-edited.jpg` has no JSON of its own (use the original's; suffix is translated in other languages, keep a list). Match order: exact, supplemental, truncated, numbered duplicate, edited suffix stripped, then by `title` inside JSON.
- **Live photos (verify)**: iPhone Live Photo = image (HEIC/JPG) + video (MP4/MOV), same base name. Pair by base name + folder + close taken time. Android Motion Photo may be `.MP.jpg` (video embedded) and/or a separate video.
- **Time**: taken time is UTC. Local time needs EXIF `OffsetTime*`, else GPS lookup, else computer time zone.
- **(verify)**: exact match order should be confirmed against real exports; the edited-suffix locale list and Motion Photo `.MP.jpg` pairing are still uncertain and need a sample export for validation.
