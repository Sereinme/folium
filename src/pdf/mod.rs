pub mod outline;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use gpui::RenderImage;
use image::{Frame, RgbaImage};
use smallvec::SmallVec;

use crate::render_queue::{RenderHandle, ToMain};
use crate::types::{OutlineItem, PdfPageImage, ScaleType};

/// Keep Full (2×) only for the current page ±1. Beyond that, discard
/// immediately — SumatraPDF-style: re-render on scroll-back.
const FULL_CACHE_RADIUS: usize = 1;

/// Keep Preview (1×) for a couple of nearby pages so one-wheel-click
/// scrolls don't flash "loading".
const PREVIEW_CACHE_RADIUS: usize = 2;

pub struct PdfDocument {
    pub path: PathBuf,
    pub page_count: usize,
    pub outline: Vec<OutlineItem>,
    pub pages: Vec<Option<PdfPageImage>>,
    pub thumbnails: Vec<Option<PdfPageImage>>,
    pub previews: HashMap<usize, PdfPageImage>,
    pub page_dims: Vec<Option<(f32, f32)>>,
    pub initialized: bool,
    pub inflight: usize,
    submitted: HashSet<(usize, ScaleType)>,
    handle: RenderHandle,
}

impl PdfDocument {
    pub fn open(path: PathBuf) -> Result<Self> {
        let handle = RenderHandle::start(path.clone())?;

        Ok(Self {
            path,
            page_count: 0,
            outline: Vec::new(),
            pages: Vec::new(),
            thumbnails: Vec::new(),
            previews: HashMap::new(),
            page_dims: Vec::new(),
            initialized: false,
            inflight: 0,
            submitted: HashSet::new(),
            handle,
        })
    }

    pub fn poll_render_results(&mut self) -> bool {
        let mut changed = false;
        while let Some(msg) = self.handle.poll() {
            match msg {
                ToMain::Init { page_count } => {
                    self.page_count = page_count;
                    self.pages = vec![None; page_count];
                    self.thumbnails = vec![None; page_count];
                    self.page_dims = vec![None; page_count];
                    self.initialized = true;
                    changed = true;
                }
                ToMain::Outline(outline) => {
                    self.outline = outline;
                    changed = true;
                }
                ToMain::Done {
                    page_index,
                    scale,
                    result,
                    ..
                } => {
                    self.inflight = self.inflight.saturating_sub(1);
                    self.submitted.remove(&(page_index, scale));
                    if page_index >= self.page_count {
                        continue;
                    }
                    match result {
                        Ok((samples, width, height, natural_w, natural_h)) => {
                            if self.page_dims[page_index].is_none() {
                                self.page_dims[page_index] = Some((natural_w, natural_h));
                            }

                            let image = build_page_image(samples, width, height);
                            match scale {
                                ScaleType::Full => {
                                    self.pages[page_index] = Some(image);
                                }
                                ScaleType::Preview => {
                                    self.previews.insert(page_index, image);
                                }
                                ScaleType::Thumb => {
                                    self.thumbnails[page_index] = Some(image);
                                }
                            }
                            changed = true;
                        }
                        Err(e) => {
                            eprintln!("render failed for page {page_index}: {e}");
                        }
                    }
                }
            }
        }
        changed
    }

    pub fn request_render(&mut self, page_index: usize, scale: ScaleType) {
        if !self.initialized || page_index >= self.page_count {
            return;
        }
        if self.is_cached(page_index, scale) {
            return;
        }
        // Don't submit duplicates — a render for this (page, scale) is
        // already in flight.
        if !self.submitted.insert((page_index, scale)) {
            return;
        }
        self.inflight += 1;
        self.handle.submit(page_index, scale);
    }

    pub fn is_cached(&self, page_index: usize, scale: ScaleType) -> bool {
        if !self.initialized || page_index >= self.page_count {
            return false;
        }
        match scale {
            ScaleType::Full => self.pages.get(page_index).and_then(Option::as_ref).is_some(),
            ScaleType::Preview => self.previews.contains_key(&page_index),
            ScaleType::Thumb => self.thumbnails.get(page_index).and_then(Option::as_ref).is_some(),
        }
    }

    pub fn cached_page(&self, page_index: usize, scale: ScaleType) -> Option<&PdfPageImage> {
        if !self.initialized || page_index >= self.page_count {
            return None;
        }
        match scale {
            ScaleType::Full => self.pages.get(page_index).and_then(Option::as_ref),
            ScaleType::Preview => self.previews.get(&page_index),
            ScaleType::Thumb => self.thumbnails.get(page_index).and_then(Option::as_ref),
        }
    }

    /// Drop cached renders that are too far from the current page.
    /// Full (2×, 8 MB/page) is the dominant memory consumer, so its radius is tight.
    /// Preview (1×, 2 MB/page) has a wider radius for smooth nearby scrolling.
    /// Thumbnails (0.25×, 0.12 MB/page) are never evicted — they're negligible.
    pub fn evict_distant(&mut self, current_page: usize) {
        let cur = current_page as isize;

        for (i, slot) in self.pages.iter_mut().enumerate() {
            if (i as isize - cur).unsigned_abs() > FULL_CACHE_RADIUS {
                *slot = None;
            }
        }

        self.previews.retain(|&idx, _| {
            (idx as isize - cur).unsigned_abs() <= PREVIEW_CACHE_RADIUS as usize
        });
    }

    /// Natural page dimensions (from any rendered scale), or a default A4 fallback
    pub fn page_dim(&self, page_index: usize) -> (f32, f32) {
        self.page_dims
            .get(page_index)
            .and_then(|d| *d)
            .unwrap_or((595.0, 842.0))
    }
}

fn build_page_image(samples: Vec<u8>, width: u32, height: u32) -> PdfPageImage {
    let buffer = RgbaImage::from_raw(width, height, samples)
        .expect("mupdf returned invalid pixel buffer");
    PdfPageImage {
        image: Arc::new(RenderImage::new(SmallVec::from_elem(Frame::new(buffer), 1))),
        width,
        height,
    }
}
