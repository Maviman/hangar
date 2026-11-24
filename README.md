# HANGAR

A tiny native launcher for Windows. Drag `.exe`, `.lnk`, `.html`, `.bat` (or any
file/folder) onto the grid; they dock as bubbles orbiting named clusters you can
pan, zoom and arrange. Click a bubble to launch it.

Rust + egui/eframe (glow renderer). Single 4.6 MB exe, no installer, no runtime.

## Use

| Action | Do |
|---|---|
| Dock files | Drop on a cluster's name plate to dock straight there. Drop anywhere else and a picker opens: all clusters listed (nearest one preselected), type to filter, or type a new name to create a cluster. ↑↓ + Enter, Esc cancels |
| Move via picker | Drag a bubble far out into empty space and release: the same picker opens to move it |
| Launch | Click a bubble (or Ctrl+F, type, Enter) |
| Move | Drag a bubble; release over another cluster to re-dock. Drag a plate to move the cluster |
| Rename | Double-click a plate; F2 or right-click on a bubble |
| Pan / zoom | Drag empty space or middle-drag / mouse wheel. `F` fits everything |
| Menus | Right-click bubble, plate, or canvas (accent colour, move to, open file location, remove) |
| Drift | `DRIFT ON/OFF` in the toolbar: clusters bob and bubbles sway at 20 fps. Pauses (0 CPU) when the window is unfocused or minimised. Off = fully static, redraws only on input |
| Icons | Shell icon of each file (48 px). Put `<name>.ico` next to a file (e.g. `start.bat` + `start.ico`) to override it |
| Missing / broken | Shown with a dashed orange ring and `NOT FOUND`. Clicking one (or right-click > Relocate…) opens a Windows file picker in the nearest folder that still exists. A shortcut whose target is gone counts as broken. If a whole folder moved, other missing items from that folder are re-linked automatically |
| CLI | `hangar.exe file1 file2 …` docks into an `INBOX` cluster (works as a Send To target) |

## Safety net

- **Undo:** Ctrl+Z reverses dock, move, rename, remove, relocate and delete (30 steps, per session).
- **Save file:** written to a temp file, flushed, then swapped in. The previous good save is kept as `hangar.json.bak`. A damaged file is copied to `hangar.json.corrupt` and the backup is loaded instead. If the file exists but cannot be read, HANGAR runs read-only rather than overwrite it.
- **One copy:** starting HANGAR twice focuses the running window. Files passed to the second copy are handed over through `inbox.txt` and dock into `INBOX`.
- **Crashes:** written to `crash.log` beside the layout file, with a message box.
- **Slow drives:** icons and missing-file checks run on a background thread; the file dialog runs on its own thread.

## Tests

`cargo test` runs 9 tests: name parsers, a 200k-input fuzz test of the HTML/BAT/LNK parsers, save/load/backup recovery, layout sanitising, and physics stability.

## Names

- `.exe`: VERSIONINFO `FileDescription`, then `ProductName`, then file name.
- `.lnk`: target exe's `FileDescription` if the target is an exe that exists, otherwise the shortcut name.
- `.html`: `<title>`.
- `.bat` / `.cmd`: a `title …` line if present, otherwise file name.
- Anything else: file/folder name. Right-click > Rename overrides; Reset Name re-reads it.

## Data

`%APPDATA%\Hangar\hangar.json` (with `.bak` of the previous save). Portable mode:
create an empty `hangar.json` next to the exe and it is used instead. Saves 1 s after any change and on exit. Removing things from
HANGAR never touches the files on disk. The window size is remembered.

## Build

```
# native (Windows, MSVC or GNU)
cargo build --release
# cross from Linux
rustup target add x86_64-pc-windows-gnu
sudo apt install gcc-mingw-w64-x86-64
cargo build --release --target x86_64-pc-windows-gnu
```

Source map: `model.rs` (data, force layout, persistence), `meta.rs` (name
extraction + a small MS-SHLLINK parser), `platform.rs` (Win32: ShellExecute,
icons, version info, cursor), `app.rs` (canvas/UI), `theme.rs` + `DESIGN.md`
(design tokens).

Fonts: Oxanium and JetBrains Mono, both SIL OFL 1.1 (see `assets/fonts`).
Visual style is original; no third-party game assets or logos.
