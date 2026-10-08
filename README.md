# Shattr

A screenshot tool for people who count pixels: capture, inspect, measure, annotate, and hand off to a model. Rust and GPUI (through [gpui-kit](https://docs.rs/gpui-kit)). Windows is the first fully working platform; macOS and Linux compile against the same code with fallback backends that still need work (see Platform status).

![The Shattr editor with arrows, a step counter and a blurred region](docs/screenshots/editor.png)

## Run it

```bash
cargo run --release -p shattr-app
```

The toolchain is pinned to Rust 1.98 in `rust-toolchain.toml` (gpui-kit 0.7.1 does not build on 1.92). Shattr lives in the system tray; there is no main window. `shattr --library` opens the Library on launch.

| Action | Windows / Linux | macOS (planned) |
| --- | --- | --- |
| Capture area (overlay) | `Ctrl+Shift+S`, `PrtSc` | `⌘⇧2` |
| Capture window | `Alt+PrtSc` | overlay key `2` |
| Capture screen | `Ctrl+PrtSc` | overlay key `3` |
| Repeat last area | `Ctrl+Shift+PrtSc` | `⌃⌘2` |
| Copy text from screen | `Alt+Shift+PrtSc` | `⌥⌘2` |
| Search existing screenshots (Library) | `Ctrl+Shift+K` | `⌘⇧K` |

Inside the overlay: `1`–`7` switch mode (area, window, screen, scrolling, last area, 3 s delay, text). Releasing a drag captures immediately; hold `Ctrl` while releasing to open the capture in the editor. A click without dragging captures the window under the cursor. `Z` toggles a pixel magnifier, `C` copies the colour under the cursor, `Esc` cancels. In Last area mode the previous region is shown with handles: adjust it, `Tab` applies the auto-adjust suggestion, `Enter` captures.

After a capture the image is already on the clipboard. The corner card offers Copy, Save (to `Pictures\Shattr`), Pin, Send to AI, and drag-out: drag the preview into any app that accepts files. If the capture matches a screenshot you already have, the card says so and offers to open it instead.

Ctrl-releasing a drag in the overlay, Edit on the thumbnail, or double-clicking a Library tile opens the editor. Every tool has a single key (V select, C crop, A arrow, L line, R rectangle, O oval, P freehand, H highlighter, T text, N step, S spotlight, B blur/pixelate, E erase, Z magnifier, M measure, G guides, I image overlay, F backdrop), `Ctrl+K` opens the command palette, `Ctrl+I` the inspector (dimensions, colour formats with Tab-to-copy, WCAG and APCA contrast, average colour), and `Ctrl+C` / `Ctrl+S` / `Ctrl+P` copy, save and pin the exported image. The editor and the exporter draw from the same geometry, so the saved PNG matches the canvas.

Every global shortcut can be changed in **Settings…** (tray menu): click a shortcut, press the new keys, and it applies immediately. Settings live in `%APPDATA%\Shattr\settings.json`.

## Screenshots

| | |
| --- | --- |
| ![Capture overlay selecting an area with the pixel magnifier](docs/screenshots/overlay.png) | ![Corner card after a capture with Copy, Save, Pin and Send to AI](docs/screenshots/card.png) |
| Capture overlay with the pixel magnifier | Corner card after a capture |
| ![Inspector showing colour formats and WCAG/APCA contrast](docs/screenshots/inspector.png) | ![Library window with duplicate groups](docs/screenshots/library.png) |
| Inspector: colour formats and contrast | Library and duplicate matching |

## Layout

| Crate | Owns |
| --- | --- |
| `shattr-core` | Shared types (`Bitmap` BGRA8, `RectPx`, `Shot`), auto-adjust trimming, area downscale, scroll stitching. No GUI. |
| `shattr-library` | The index of screenshots already on disk: discovery per OS, perceptual matching, duplicate groups, SQLite store, thumbnails. No GUI. |
| `shattr-annotate` | The annotation document and its single geometry pipeline (`Prim` polygons and image patches) plus tiny-skia export, so the editor shows exactly what is exported. |
| `shattr-ui` | Design tokens, icons, small widgets shared by every window. |
| `shattr-editor` | The editor window. |
| `shattr-app` | The resident app: DXGI capture, tray, hotkeys, overlay, thumbnail, pin, OCR/QR toast, scrolling HUD, Library window, sharing. |

## Speed

Measured on a 2560x1600 HDR display at 150% (RTX 4080 laptop), release build:

- Hotkey to pixels: 7–14 ms with a persistent DXGI duplication session (GDI BitBlt was 44–52 ms, Windows Graphics Capture 29–36 ms).
- Hotkey to overlay drawn: about 30–45 ms.
- OCR of a 25-line region: about 155 ms (Windows.Media.Ocr).
- Duplicate lookup against the library: about 1.6 ms.

## Platform status

- **Windows:** everything above works and was driven end to end on a real desktop.
- **macOS, Linux:** capture falls back to `xcap` (slow path), there is no OCR engine wired, drag-out and the multi-format AI clipboard are Windows-only, and nothing has been built or run on those systems yet. Wayland adds portal permission prompts and GNOME offers no always-on-top for pins.

## Diagnostics

- `RUST_LOG=info` (default) logs capture timings, thumbnail placement, saves, and pins.
- `SHATTR_DUMP_SCROLL=<dir>` writes every scrolling-capture frame to `<dir>` for stitching analysis.
- `SHATTR_AI_URL=<url>` changes where Send to AI opens (default `https://claude.ai/new`; any non-http value opens nothing).
