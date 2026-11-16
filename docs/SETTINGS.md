# Settings and planner (single source of truth)

Write metadata **into supported image/video files without re-encoding pixels**. If a format cannot be safely updated, preserve the copied file content and report a warning; never create an `.xmp` sidecar.

| Key | Values (default) | Effect |
|---|---|---|
| outputFolder | path or null (null) | null: `<Pictures>/Photo Library`. Chosen empty folder: it is the library root. Chosen folder with other content: create `Photo Library` inside it. |
| albumMode | off, move, link, copy (move) | off: no album folders. move: each photo lives in its FIRST album, the rest by date. link/copy: originals stay in the date folder; every album gets a link/copy. |
| albumLoc | top, year (top) | albums at the top level (`Paris trip`) or inside the year (`2024/Paris trip`). Albums are never nested inside month folders. Disabled when albumMode=off. |
| multiAlbum | first, all (first) | albumMode=move only: photo in several albums stays in the first album only, or is also copied into the others. Disabled unless move. |
| dateFolders | off, year, yearMonth (yearMonth) | folders for photos not in an album. `yearMonth` creates `year/month`; off sends them straight into the library root. |
| favoritesMode | off, move, link, copy (link) | Flat `Favorites` folder. move: original lives there. link/copy: original stays, Favorites gets a link/copy. |
| editedMode | off, move, link, copy (move) | Flat `Edited` folder. move: edited photos go there. link/copy: the edited photo stays in its normal destination and Edited gets a link/copy. Disabled when editedVersions=originals. |
| archiveFolder | bool (true) | archived items go directly into the flat `Archive` folder. |
| trashed | skip, include, ownFolder (skip) | show only if the scan found trashed items. ownFolder puts them directly into the flat `Trash` folder. |
| editedVersions | both, originals, edited (both) | which versions are kept. |
| mergeLive | bool (true) | keep photo + video together as a pair. The main page shows the chosen liveStyle next to this toggle. |
| liveStyle | separate, iphonePair, androidMotionPhoto (separate) | separate: two normal files. iphonePair: Live Photo pair (matching content ID). androidMotionPhoto: Motion Photo (video embedded in the JPEG). Both paired styles are experimental: the photo file is modified. |
| splitByType | {photos, videos, live} bools (all false) | adds `Photos`/`Videos`/`Live photos` subfolder in normal destination folders, only for selected types. Never adds subfolders inside special folders. |
| fileNames | keep, dateTime, dateOriginal, custom (keep) | Selects the naming pattern. When `custom`, `customNamePattern` supplies the tokens `{date} {time} {original} {n} {album}`. |
| customNamePattern | string (empty) | Pattern used only when `fileNames=custom`. |
| sameName | number, dateTime, skip (number) | collision rule in one folder. Never overwrite. |
| ignoreDuplicates | bool (true) | same bytes = same photo, compared across the input AND files already in the output folder (so a second export can be added to an existing library). Keep one original, then apply album/favorites rules. |
| noJson | leave, unsorted (unsorted) | files without JSON. unsorted puts them directly into the flat `Unsorted` folder. |
| timeZone | local, computer, utc (local) | local = EXIF offset, else GPS time zone lookup, else computer. |
| noDate | fileName, fileDate, blank (fileName) | when there is no JSON or EXIF date. |
| writeGps | bool (true) | write location. |
| writeDescriptions | bool (true) | description + people tags. |
| rateFavorites | bool (true) | favorites get 5 stars. |
| deleteArchivesAfter | bool (false) | zips/tgz only: delete each archive permanently after it is fully processed and verified. Turning it on shows a confirm dialog: "Each Takeout file is permanently deleted once its photos are saved and checked. It does not go to the Recycle Bin or Trash. To run Takeback again you would need to download your Takeout again. Your Google Photos library is not affected." |
| speed | balanced, gentle (balanced) | gentle limits CPU/disk use. |
| dryRun | bool (false) | plan only, write nothing. |

## Flat special folders

`Archive`, `Favorites`, `Edited`, `Trash`, and `Unsorted` are always flat folders. Never create date, album, or type subfolders inside them.

Albums go at the library root by default (`albumLoc=top`). They go inside their year folder only when `albumLoc=year`; they are never nested inside month folders.

With `editedMode=move`, edited photos go directly into `Edited`. With `favoritesMode=link`, a favorite stays in its normal destination and also appears directly in `Favorites` as a link, labeled `Link` in the output preview.

## Planner (per media item, after matching)
1. Drop by rules: trashed, editedVersions, duplicates (keep one, remember its albums).
2. No JSON: `leave` makes no placement; `unsorted` places the file directly in `Unsorted`.
3. Choose the normal base path: albumMode=move and in an album -> first album (`[year/]album` per albumLoc); otherwise use the date path per dateFolders (`year/month`, `year`, or none).
4. Archive, Trash, Unsorted, Favorites, and Edited placements are flat; do not append date, album, or type folders inside them.
5. Append a type folder only to normal destination paths for selected types.
6. albumMode link/copy: main = date path, plus one Link/Copy per album. move + multiAlbum=all: Copy into the other albums. Album links/copies use albumLoc and may have a selected type folder.
7. Archived + archiveFolder: main = `Archive`. Favorites/Edited move routes the main placement directly to the respective folder; link/copy adds a flat extra placement. In particular, favoritesMode=link keeps the favorite in its normal destination and adds a `Link` placement in `Favorites`.
8. Names, collisions, output root rule.
Returns `Vec<Placement { src, dest, kind: Original|Link|Copy, tags }>`.

## Preferences page layout
Main list: Albums (segmented: Off/Move/Link/Copy), Date folders (segmented: Off/Year/Year › Month), Favorites (segmented: Off/Move/Link/Copy), Edited photos (segmented selector: Keep both/Originals only/Edited only), Live photos (toggle + tag showing liveStyle), Archive (toggle), Advanced options (shows "All defaults" or "N changed from the defaults"). Only Albums, Date folders, and Favorites use segmented controls; Edited photos uses a segmented selector.
Advanced dialog tabs: **Folders** (Where albums go, Photo in several albums, Edited photos folder, Trashed items, Sort into folders by type [Photos/Videos/Live] checkboxes, Folder names), **Names and duplicates** (File names, If two files have the same name, Ignore exact duplicates, Files without JSON), **Dates and metadata** (Time zone, If a photo has no date, Write location, Write descriptions and people tags, Rate favorites 5 stars), **More** (Live photo style, Delete each zip after it is processed, Speed). Every choice row in the Advanced dialog uses a segmented selector sized to its content. Toggle rows remain toggles; the by-type row remains checkboxes.
Hide rows when the scan found none of that kind; show the count under the row ("214 archived photos found").


## Output preview (UI)
Same planner on about 8 sample items. Tree with connector lines (vertical from each folder, elbow into each child, corner on the last). Single-child chains joined with " › ". Link/Copy/Live tags on files. Scrolls with wheel, trackpad and keyboard (`tabIndex=0`), scrollbar hidden. Updates on every change (debounce 100 ms).
