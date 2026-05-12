pub mod outline;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use gpui::RenderImage;
use image::{Frame, RgbaImage};
use smallvec::SmallVec;

use crate::render_queue::{RenderHandle, ToMain};
use crate::types::{OutlineItem, PdfPageImage, ScaleType};

pub struct PdfDocument {
    pub path: PathBuf,
    pub page_count: usize,
    pub outline: Vec<OutlineItem>,
    pub pages: Vec<Option<PdfPageImage>>,
    pub thumbnails: Vec<Option<PdfPageImage>>,
    pub preview: Option<PdfPageImage>,
    pub initialized: bool,
    pub inflight: usize,
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
            preview: None,
            initialized: false,
            inflight: 0,
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
                    if page_index >= self.page_count {
                        continue;
                    }
                    match result {
                        Ok((samples, width, height)) => {
                            let image = build_page_image(samples, width, height);
                            match scale {
                                ScaleType::Full => {
                                    self.pages[page_index] = Some(image);
                                    self.preview = None;
                                }
                                ScaleType::Preview => {
                                    self.preview = Some(image);
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

    pub fn can_render(&self) -> bool {
        self.inflight < 10
    }

    pub fn request_render(&mut self, page_index: usize, scale: ScaleType) {
        if !self.initialized || page_index >= self.page_count {
            return;
        }
        if self.is_cached(page_index, scale) {
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
            ScaleType::Preview => false,
            ScaleType::Thumb => self.thumbnails.get(page_index).and_then(Option::as_ref).is_some(),
        }
    }

    pub fn cached_page(&self, page_index: usize, scale: ScaleType) -> Option<&PdfPageImage> {
        if !self.initialized || page_index >= self.page_count {
            return None;
        }
        match scale {
            ScaleType::Full => self.pages.get(page_index).and_then(Option::as_ref),
            ScaleType::Preview => self.preview.as_ref(),
            ScaleType::Thumb => self.thumbnails.get(page_index).and_then(Option::as_ref),
        }
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
