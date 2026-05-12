# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Run

```bash
cargo build --release          # build
cargo run --release -- foo.pdf # open a PDF directly
cargo run --release            # launch without a file
```

No tests exist yet. The project targets Rust edition 2024 — note that `gen` is a reserved keyword.

## Architecture

Folium is a read-only PDF viewer built on **GPUI** (GPU-accelerated UI) with **mupdf** for PDF rendering. Rendering is single-pass: mupdf rasterizes pages to RGBA pixel buffers, which are uploaded to GPUI as `RenderImage` textures.

### Render pipeline

Pages are rendered on a **dedicated background thread** (`pdf-render`, see `src/render_queue.rs`). Communication uses two `mpsc` channels:

- `Cmd::Render` → main thread sends render requests to the background thread
- `ToMain::Init / Outline / Done` → background thread sends results back

`PdfDocument::poll_render_results()` (called from the main thread) drains the result channel and caches images into three resolution tiers:

| Tier | Scale | Purpose |
|------|-------|---------|
| `Preview` | 1.0× | Fast readable quality, shown first |
| `Full` | 2.0× | Crisp final quality |
| `Thumb` | 0.25× | Sidebar thumbnail strip |

`Preview` is never cached across frames (`is_cached(Preview)` always returns `false`), so it re-renders each time the current page changes. This avoids stale low-res images while the 2× render completes.

mupdf `DisplayList` objects are cached in an LRU pool (max 20) to avoid re-parsing pages on re-render.

### Render pump (app.rs)

The rendering loop is driven by an async **pump task** (`ensure_pump`), not by `cx.notify()` called from within `render()`. Calling `cx.notify()` from inside `render()` is unreliable in GPUI — it may not schedule a new frame. Instead:

1. `load_pdf` / user navigation → `cx.notify()` → triggers a `render()` frame
2. `render()` calls `poll_and_submit()` to process results and submit new render requests
3. If work is pending (not yet initialized, or `inflight > 0`), `ensure_pump()` spawns a task that polls every ~32ms, calling `poll_and_submit()` + `cx.notify()` on changes
4. The pump exits when the document is fully initialized and `inflight == 0`
5. A `render_gen` counter prevents stale pumps from a previous document from interfering with a new one

`poll_and_submit` uses an expanding-ring strategy: current page ±30 pages get Preview → Thumb → Full; ±80 pages get Thumb only.

### GPUI repaint forcing

GPUI may skip repainting when it sees an identical element tree. `render_stamp` (incremented each frame) is used as the `ElementId` of the reader body wrapper (`"rp-stamp"`), forcing GPUI to treat the subtree as new each frame even when the underlying `Arc<RenderImage>` references haven't changed.

### Key files

- `src/main.rs` — entry point, window creation, parses CLI arg for initial PDF path
- `src/app.rs` — `PdfReader` entity: state, navigation, render queue orchestration, pump, UI layout
- `src/render_queue.rs` — `RenderHandle`: background thread, mpsc channels, LRU DisplayList cache, mupdf rasterization
- `src/pdf/mod.rs` — `PdfDocument`: page cache (three tiers), dimensions, delegates to `RenderHandle`
- `src/pdf/outline.rs` — converts mupdf `Outline` tree into `Vec<OutlineItem>`
- `src/types.rs` — `ScaleType`, `PdfPageImage`, `OutlineItem`, `FlatOutlineItem`, `SidebarTab`
- `src/ui/reader_body.rs` — virtual-scroll reading area (shifted inner div, scroll wheel capture)
- `src/ui/sidebar.rs` — sidebar with Thumbnails/Outline tab switcher
- `src/ui/thumbnail_list.rs` — thumbnail strip with `ScrollHandle`-based sync
- `src/ui/outline_panel.rs` — collapsible outline tree, flatten with collapsed-set filtering
- `src/ui/styles.rs` — color constants (hex RGBA) and layout dimensions

### Dependencies

| Crate | Role |
|-------|------|
| `gpui` 0.2.2 | GPU UI framework (immediate-mode, retained element tree) |
| `gpui-component` 0.5.0 | Pre-built widgets (Button, ScrollableElement, TitleBar, AppMenuBar) |
| `mupdf` 0.6.0 | PDF parsing + rasterization (system-fonts feature) |
| `image` 0.25.9 | `RgbaImage` pixel buffer → `RenderImage` conversion |
| `smallvec` 1.15 | `SmallVec` for single-frame image storage |
| `anyhow` 1.0 | Error propagation |
