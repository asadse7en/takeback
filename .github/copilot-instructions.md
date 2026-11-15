# Takeback: project instructions
Takeback is a desktop app (Windows, macOS, Linux; one codebase) that merges Google Photos Takeout JSON sidecars into photos and organizes them. Stack: Tauri 2, Rust, React 18 + TypeScript + Vite, Tailwind v4, shadcn/ui, Zustand. ExifTool is a sidecar for writing metadata.

Before any task read `docs/SPEC.md`, plus the doc the prompt names (`SETTINGS.md`, `TAKEOUT.md`, `DESIGN.md`). The decisions in SPEC.md are final: follow them, don't re-decide.

Rules
- Never modify or delete input files. Write only to the output folder: temp name first, then rename.
- Stream archives and files; never load a whole archive or big file in memory (exports can be 200 GB).
- One pure planner (`src-tauri/src/planner`) decides where files go. Preview and real run use the same function.
- Rust: typed errors (`thiserror`), no `unwrap()` outside tests, camelCase serde across IPC, long work off the main thread, progress events at most every 500 ms, cancel/pause via `AtomicBool`.
- Frontend: strict TypeScript, no `any`, Zustand store, talks to Rust only through `src/lib/api.ts`. Colors, fonts and radii come from CSS variables in `docs/DESIGN.md`.
- Cross-platform: no OS-specific code outside `src-tauri/src/platform/`. Use Tauri path APIs, never hard-coded separators. Treat file names as case-insensitive and sanitize for Windows rules on every OS.
- Accessible: real buttons, labels, focus rings, keyboard use.
- Add tests for planner and matcher (synthetic fixtures in `tests/fixtures`, never real photos).
- Plan first: list the files you will change, then do it. Small focused diffs.
- Never add telemetry, network calls, or heavy dependencies without saying why.

Project ownership and contribution rules
- Do not add attribution or source-credit text anywhere in the project, including author or copyright lines, generated-by comments, co-author trailers, README files, license files, or metadata.
- Project owner and author: `<Your Name> <you@example.com>`. License: MIT; copyright `<year> <Your Name>`. Use these values wherever a name is needed, or omit the field.
- Never run `git commit`, `git push`, or change Git configuration. The project owner makes all commits.
