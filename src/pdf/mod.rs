pub mod outline;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
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
    handle: RenderHandle,
}

impl PdfDocument {
    pub fn open(path: PathBuf) -> Result<Self> {
        let data = std::fs::read(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;

        let (handle, page_count, outline) = RenderHandle::start(data)?;

        Ok(Self {
            path,
            page_count,
            outline,
            pages: vec![None; page_count],
            thumbnails: vec![None; page_count],
            handle,
        })
    }

    pub fn request_render(&mut self, page_index: usize, scale: ScaleType) {
        if page_index >= self.page_count {
            return;
        }
        if self.is_cached(page_index, scale) {
            return;
        }
        self.handle.submit(page_index, scale);
    }

    pub fn is_cached(&self, page_index: usize, scale: ScaleType) -> bool {
        match scale {
            ScaleType::Full => self.pages.get(page_index).and_then(Option::as_ref).is_some(),
            ScaleType::Thumb => self.thumbnails.get(page_index).and_then(Option::as_ref).is_some(),
        }
    }

    pub fn cached_page(&self, page_index: usize, scale: ScaleType) -> Option<&PdfPageImage> {
        match scale {
            ScaleType::Full => self.pages.get(page_index).and_then(Option::as_ref),
            ScaleType::Thumb => self.thumbnails.get(page_index).and_then(Option::as_ref),
        }
    }

    pub fn poll_render_results(&mut self) {
        while let Some(msg) = self.handle.poll() {
            match msg {
                ToMain::Init { .. } => {}
                ToMain::Done {
                    page_index,
                    scale,
                    result,
                    ..
                } => {
                    if page_index >= self.page_count {
                        continue;
                    }
                    match result {
                        Ok((samples, width, height)) => {
                            let image = build_page_image(samples, width, height);
                            match scale {
                                ScaleType::Full => self.pages[page_index] = Some(image),
                                ScaleType::Thumb => self.thumbnails[page_index] = Some(image),
                            }
                        }
                        Err(e) => {
                            eprintln!("render failed for page {page_index}: {e}");
                        }
                    }
                }
            }
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
