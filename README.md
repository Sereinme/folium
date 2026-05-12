# Folium

A fast, pure PDF reader built with [GPUI](https://www.gpui.rs/) and [mupdf](https://mupdf.com/).

> Currently read-only — no annotations or editing. Those may come later.

## Features

- **GPU-accelerated rendering** via GPUI
- **Background page rendering** — pages are rendered off-thread with mupdf so the UI stays responsive
- **Three-level cache** — Full (2×), Preview (1×), and Thumbnail (0.25×) resolutions, rendered on demand
- **Smart render queue** — prioritizes visible pages first, then nearby pages, then distant thumbnails
- **Virtual scrolling** — smooth vertical scroll through all pages
- **Thumbnail sidebar** — quick page overview and navigation
- **Document outline** — collapsible table of contents from PDF bookmarks
- **Scroll sync** — reader and sidebar stay in sync when navigating

## Build

```bash
cargo build --release
```

Requires a Rust toolchain with edition 2024 support.

## Run

```bash
# Open directly
cargo run --release -- path/to/document.pdf

# Or launch without a file and use File → Open
cargo run --release
```

## Project structure

```
src/
├── main.rs              # Application entry point
├── app.rs               # PdfReader — core state, navigation, render orchestration
├── types.rs             # Shared types (ScaleType, OutlineItem, etc.)
├── render_queue.rs      # Background render thread + LRU DisplayList cache
├── pdf/
│   ├── mod.rs           # PdfDocument — page cache, dimensions, render handle
│   └── outline.rs       # PDF outline/bookmark parser
└── ui/
    ├── mod.rs           # UI module declarations
    ├── styles.rs        # Color palette and layout constants
    ├── sidebar.rs       # Sidebar with Thumbnails/Outline tabs
    ├── thumbnail_list.rs # Thumbnail grid with scroll sync
    ├── outline_panel.rs # Collapsible outline tree view
    └── reader_body.rs   # Main reading area with virtual scroll
```

## Architecture

Pages are rendered by a dedicated background thread (`pdf-render`). The main thread sends render commands and polls for results. The render queue uses an expanding-ring strategy:

1. First pass: current page ± 30 pages → Preview (fast), Thumb, then Full
2. Second pass: ± 80 pages → Thumb only, if not already cached

This ensures the page you're looking at appears quickly at readable quality, then sharpens to full resolution. Surrounding pages are available as thumbnails for the sidebar and smooth scrolling.

mupdf `DisplayList` objects are cached in an LRU pool (max 20) to avoid re-parsing pages during zoom or re-render.

## Dependencies

| Crate | Purpose |
|-------|---------|
| [gpui](https://crates.io/crates/gpui) | GPU-accelerated UI framework |
| [gpui-component](https://crates.io/crates/gpui-component) | Pre-built widgets (buttons, scroll, menus) |
| [mupdf](https://crates.io/crates/mupdf) | PDF rendering engine |
| [image](https://crates.io/crates/image) | Pixel buffer ↔ GPU image conversion |
| [smallvec](https://crates.io/crates/smallvec) | Small-vector optimization for image frames |
| [anyhow](https://crates.io/crates/anyhow) | Error handling |

## License

MIT
