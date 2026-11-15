# Takeback: spec

## What it does
User adds Google Photos Takeout archives (zip, tgz or an extracted folder), picks preferences, and gets photos back with dates, GPS, descriptions and favorites restored from the JSON sidecars, organized in folders. Lossless: only metadata and file times are written. Six pages: Add, Preferences, Progress, Done, Report, Review (see DESIGN.md).

## Platforms and build
Windows 10/11, macOS (Apple Silicon + Intel), Linux (x64). **One codebase.** Build each OS on its own runner (GitHub Actions matrix + `tauri-apps/tauri-action`); no cross-compiling. Outputs: Windows NSIS/MSI, macOS universal .dmg, Linux AppImage + .deb. Minimum OS versions: follow Tauri 2 requirements.

## Decisions (final)
1. **Native OS window frame** on all platforms. No custom title bar. (The mockups' title bar is illustrative.)
2. **Fonts bundled** locally: Inter (body), Bricolage Grotesque (page titles). No Segoe/system font reliance.
3. **"Link" = hard link** on every OS (same volume). Different volume or filesystem without hard links (exFAT, FAT, network share): fall back to copy and log a warning. No symlinks in v1.
4. **File names**: compare case-insensitively on every OS; sanitize for Windows rules on every OS (no `<>:"/\|?*`, no reserved names like CON/NUL, no trailing dot/space, max 255 bytes per part, Windows long-path prefix when needed).
5. **ExifTool** bundled as a Tauri sidecar per target (Windows: exe distribution; macOS/Linux: Perl distribution, needs system `perl`: check at startup and show a clear error). Run in `-stay_open` batch mode.
6. **Thumbnails**: JPEG/PNG/WebP via the `image` crate (downscaled, cached). HEIC, RAW, video: ExifTool embedded preview if present, else a type icon. No HEIC decoding in v1.
7. **Default output**: OS Pictures folder from Tauri path API; disk space via `sysinfo`/`fs4`. Settings stored in the Tauri app config dir.
8. **Input**: Tauri drag-drop event + dialog plugin; accept zip, tgz and folders silently (UI never mentions folders or tgz).
9. **Unsigned builds in v1.** README explains Windows SmartScreen, macOS Gatekeeper (right-click Open, or `xattr -dr com.apple.quarantine <app>`), Linux `chmod +x`. No auto-update.
10. **No telemetry, no network.**
11. macOS: handle folder permission prompts (Pictures, Downloads, external drives); if reading fails with a permission error, show a plain message with how to grant access.

## Cross-platform risks to test early
WebView differs (WebView2, WKWebView, WebKitGTK): test every page on all three; avoid brand-new CSS. Hidden scrollbar needs `scrollbar-width:none` and `::-webkit-scrollbar{display:none}`. Linux WebKitGTK can be slower: keep the preview tree light. Filesystems: case rules, hard-link support, illegal characters, long paths.

## Storage and low disk space
- Output is about the same size as the input (originals are kept, nothing is recompressed). Links (hard links) cost no extra space; copies do; skipIdentical removes duplicates (Takeout stores album copies).
- Nothing is extracted to a temp folder: files stream from the archive straight to the output. Temp use is only the SQLite index (tens of MB) and one file at a time.
- **Before Start**: needed = sum of sizes of Original + Copy placements, plus 5%. If needed > free on the output drive: block Start and show the shortfall with choices: pick another drive, switch Copy to Link, turn on "delete each zip after it is processed", or lower options. The Preferences disk bar turns red with "Short by X GB".
- **During a run**: check free space every few seconds. Below 1 GB: pause, remove any half-written temp file, tell the user, resume when space is available. Never leave a partial file.
- **Resumable**: run state (processed items) is stored in SQLite; after a crash or pause the run resumes without redoing finished files.
- **Delete each zip after it is processed** (off by default, zips/tgz only): process archives one at a time; after an archive is fully processed and every output file is verified, delete it. Peak space needed is then about the largest single part, so 2 GB parts work with 10 GB free. Strong confirmation text ("This deletes your Takeout files. You will need to download them again to re-run."). Tip for the user: choose smaller split sizes when creating the Takeout.
- Process order with this option: sidecars are already in the index from the scan, so a photo in one part can use JSON from another.

## Architecture
```
src/            pages/ components/ store/ lib/api.ts styles/tokens.css
src-tauri/src/  takeout/ (source, index, matcher)  planner/ (settings, plan, tree)
                engine/ (run, exif, links, live, report)  platform/ (paths, links, disk)
                commands.rs events.rs error.rs
tests/fixtures/ synthetic Takeouts (+ private/ ignored)
```
Flow: `scan_sources` reads archives without extracting, builds a temp SQLite index (media, sidecars, albums) and emits `scan:progress` -> `matcher` pairs media with JSON, edited versions, live pairs -> `planner::plan(index, settings)` returns placements; `planner::preview` runs the same logic on a sample -> `start_run(settings, dryRun)` streams files out, writes metadata, times, links/copies, emits `run:progress` (~2/s) -> `get_report`.
Commands: `scan_sources, preview_tree, start_run, pause_run, resume_run, cancel_run, get_report, get_review_items, apply_review_fix, disk_space`. Events: `scan:progress, run:progress, run:finished`.
Hashing: size, then partial, then full `blake3` only for same-size candidates. tgz: pass 1 reads JSON only, pass 2 media.

## Milestones
M0 setup and shell · M1 Add + scan · M2 matcher · M3 planner + Preferences · M4 engine · M5 Progress/Done/Report/Review · M6 release + CI · M7 live photos (experimental).
