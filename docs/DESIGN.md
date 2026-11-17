# Design (light, light orange accent)

## Logo
- Primary app icon is the light (cream) card. Use the dark card only on light backgrounds where the cream card would not be visible (in-app logo on the cream UI, light-mode README banner, browser tab, light installer pages).
- Tray icons (future): dark card when the OS theme is light, light card when it is dark.

**Reference files** (look at these when building a page):
- Screenshots: `docs/design/01-add.png`, `02-preferences.png` (+ `02-preferences-advanced-folders|names|dates|more.png`, `02-preferences-delete-warning.png`), `03-progress.png`, `04-done.png`, `05-report.png`, `06-review.png`. Attach the matching image to the chat when you run a prompt.
- HTML mockups with exact spacing, sizes and colors: `docs/design/mockups/*.html`. They are reference only (a design-tool format, not runnable): read the inline styles for px values, and the `{{ ... }}` parts are placeholders for state. Do not copy the markup; rebuild with React + Tailwind using the tokens below.

## Tokens (`src/styles/tokens.css`)
`--bg #F6F3EF · --card #FFFFFF · --border #E8E2DA · --row-line #F0EAE2 · --footer #FBF9F6 · --text #1F1B16 · --muted #6B6359 · --backdrop rgba(31,27,22,.38)`
`--accent #FDB366 (fill) · --accent-border #E89B4C · --accent-ink #2A1500 · --accent-deep #E8892E (progress) · --accent-text #B8530A · --tint #FFF1E2 · --drop-bg #FFF6EC · --drop-border #E8A867`
`--ok #2F7D3C · --danger #C42B1C · --line #DCCBB4 (tree) · --disk-used #8C8479 · --disk-free #E4DED3`
Radius: cards 8, controls 4, pills 16. Disabled: opacity .4. Type: titles Bricolage Grotesque 700 32/36, tracking -0.6px; body Inter 14, descriptions 12-13.
Window 1100x720 (min 960x640), native frame, content padding 8px 48px, footer 68px (top border, `--footer`).

## Components
Row (min-height 44; 30px tinted icon circle; label + 12px description; control right; rows in a card) · Toggle 40x20 (on = accent, dark knob) · Segmented selector (light track, selected white, sized to its content) · Checkbox 18 (checked = accent + dark check) · Buttons 32 high: standard and accent · Tag pill · Disk bar (used / this export / free + legend) · Dialog 680 wide with tabs · Tree preview.

## Pages
1. **Add**: heading; wide dashed drop area "Drop your Takeout here" + Browse; list of added files with FULL names, size, green Indexed, remove; 4-stat strip (Photos, Videos, Live photos, Total GB); Next. Alternate: 2x2 file tiles + right panel "Found in your export" (counts incl. Albums, Favorites, Archived, Without JSON).
2. **Preferences**: left column (never taller than the preview card): compact Output folder card (title "Output folder", path box + Browse, thin disk bar used / this export / free + legend), then the main list (see SETTINGS.md, Preferences page layout): Albums, Date folders, Favorites, Edited photos, Live photos (tag shows the live style), Archive, Advanced options (summary text + Open). The main list fills the remaining column height with evenly spaced, aligned rows. Only Albums, Date folders, and Favorites use segmented controls; Edited photos uses a segmented selector. Toggles remain toggles. Right: "Output preview" card (tree). Footer: "Preview only" checkbox left, Back + Start right. Advanced options opens a 680px dialog with tabs Folders, Names and duplicates, Dates and metadata, More. The Folders tab includes "Sort into folders by type" checkboxes (Photos/Videos/Live). Choice rows use segmented selectors, sized to their content, with selected white, light orange hover, and keyboard use. Turning on "Delete each zip" opens a confirm dialog (text in SETTINGS.md).
3. **Progress**: card with current thumbnail, NOW FIXING, file name, step chips, percentage top right, "n of N", thick smooth bar; tiles Speed (real sparkline), Time left, Warnings (View opens a list dialog); "Recently fixed" 5 thumbnails; Pause, Cancel.
4. **Done**: "All done"; card with "12,310 of 13,710 files fixed" + one segmented bar (fixed, duplicates, no JSON, warnings) + legend; 4 tiles (Dates restored, Locations added, Live photos merged, Need a look + Review); Start over, Export report, Open output folder.
5. **Report**: stats, settings used, needs-attention table; format dropdown (HTML/CSV), Copy summary, Save report.
6. **Review**: filter pills (All, No JSON, Warnings), 4-column thumbnail grid, detail panel (Use the file date / Pick a date / Leave as is); Apply to all similar, Apply.
