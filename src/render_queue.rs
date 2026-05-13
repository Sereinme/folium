use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use anyhow::{anyhow, Result};
use mupdf::{Colorspace, DisplayList, Document, Matrix};
use rayon::prelude::*;

use crate::pdf::outline::parse_outline;
use crate::types::{OutlineItem, ScaleType};

/// Extract the raw mupdf context pointer for the current thread.
fn raw_context() -> *mut mupdf_sys::fz_context {
    // mupdf::Context is a single-field struct (inner: *mut fz_context).
    // No Drop — the context is owned by thread-local storage.
    let ctx = mupdf::Context::get();
    let raw = unsafe { std::mem::transmute_copy(&ctx) };
    std::mem::forget(ctx);
    raw
}

/// Shrink the shared mupdf store to 5% of its current size.
fn shrink_mupdf_store() {
    unsafe { mupdf_sys::fz_shrink_store(raw_context(), 5) };
}

// ── LRU DisplayList cache (used for serial, single-item renders) ──────

const DL_CACHE_MAX: usize = 0;

struct DlCache(Vec<(usize, DisplayList)>);

impl DlCache {
    fn new() -> Self {
        Self(Vec::with_capacity(DL_CACHE_MAX))
    }

    fn get_or_create(
        &mut self,
        page_index: usize,
        create: impl FnOnce() -> DisplayList,
    ) -> &DisplayList {
        if let Some(pos) = self.0.iter().position(|(idx, _)| *idx == page_index) {
            let entry = self.0.remove(pos);
            self.0.push(entry);
            return &self.0.last().unwrap().1;
        }

        if self.0.len() >= DL_CACHE_MAX {
            self.0.remove(0);
        }

        self.0.push((page_index, create()));
        &self.0.last().unwrap().1
    }
}

// ── Channels ───────────────────────────────────────────────────────────

#[derive(Debug)]
enum Cmd {
    Render {
        _id: u64,
        page_index: usize,
        scale: ScaleType,
    },
    Shutdown,
}

#[derive(Debug)]
pub enum ToMain {
    Init {
        page_count: usize,
    },
    Outline(Vec<OutlineItem>),
    Done {
        _id: u64,
        page_index: usize,
        scale: ScaleType,
        /// (samples, pixel_w, pixel_h, natural_w, natural_h)
        result: Result<(Vec<u8>, u32, u32, f32, f32)>,
    },
}

// ── RenderHandle ───────────────────────────────────────────────────────

const PARALLEL_BATCH_MIN: usize = 3;

pub struct RenderHandle {
    cmd_tx: Sender<Cmd>,
    result_rx: Receiver<ToMain>,
    next_id: u64,
}

impl RenderHandle {
    pub fn start(path: PathBuf) -> Result<Self> {
        let (cmd_tx, cmd_rx) = mpsc::channel::<Cmd>();
        let (result_tx, result_rx) = mpsc::channel::<ToMain>();

        thread::Builder::new()
            .name("pdf-render".into())
            .spawn(move || Self::render_thread(path, cmd_rx, result_tx))
            .map_err(|e| anyhow!("failed to spawn render thread: {e}"))?;

        Ok(Self {
            cmd_tx,
            result_rx,
            next_id: 0,
        })
    }

    fn render_thread(path: PathBuf, cmd_rx: Receiver<Cmd>, result_tx: Sender<ToMain>) {
        let path_str = path.to_string_lossy();
        let doc = match Document::open(&*path_str) {
            Ok(d) => d,
            Err(_) => {
                let _ = result_tx.send(ToMain::Init { page_count: 0 });
                return;
            }
        };

        let page_count = doc.page_count().unwrap_or(0) as usize;
        if result_tx.send(ToMain::Init { page_count }).is_err() {
            return;
        }

        let outline = parse_outline(&doc);
        let _ = result_tx.send(ToMain::Outline(outline));

        let mut dls = DlCache::new();

        loop {
            let cmd = match cmd_rx.recv() {
                Ok(cmd) => cmd,
                Err(_) => break,
            };

            match cmd {
                Cmd::Render {
                    _id,
                    page_index,
                    scale,
                } => {
                    // Render the first item immediately — minimizes
                    // latency from "open" to first visible page.
                    {
                        let dl = dls.get_or_create(page_index, || {
                            Self::load_display_list(&doc, page_index)
                        });
                        let result = Self::render_from_dl(dl, scale);
                        let _ = result_tx.send(ToMain::Done {
                            _id,
                            page_index,
                            scale,
                            result,
                        });
                    }

                    // Collect remaining commands for parallel batch
                    let mut batch: Vec<(u64, usize, ScaleType)> = Vec::new();
                    while let Ok(cmd) = cmd_rx.try_recv() {
                        match cmd {
                            Cmd::Render {
                                _id,
                                page_index,
                                scale,
                            } => {
                                batch.push((_id, page_index, scale));
                            }
                            Cmd::Shutdown => {
                                if !batch.is_empty() {
                                    Self::process_batch(&doc, &batch, &result_tx);
                                }
                                return;
                            }
                        }
                    }

                    if !batch.is_empty() {
                        Self::process_batch(&doc, &batch, &result_tx);
                    }
                    // Clear DL cache so store shrink can evict pinned objects
                    dls.0.clear();
                    shrink_mupdf_store();
                }
                Cmd::Shutdown => break,
            }
        }
    }

    /// Load a DisplayList for the given page. Extracted so both the
    /// immediate-render path and the serial fallback can use it.
    fn load_display_list(doc: &Document, page_index: usize) -> DisplayList {
        doc.load_page(page_index as i32)
            .ok()
            .and_then(|p| p.to_display_list(false).ok())
            .expect("mupdf: failed to create DisplayList")
    }

    fn process_batch(
        doc: &Document,
        batch: &[(u64, usize, ScaleType)],
        result_tx: &Sender<ToMain>,
    ) {
        if batch.len() >= PARALLEL_BATCH_MIN {
            // ── Parallel path ──────────────────────────────────────
            // Collect results in a scoped block so dl_map is dropped
            // before we empty the mupdf store. DisplayLists hold
            // references into the store; if they're alive during
            // fz_empty_store, their pinned objects can't be evicted.
            let results: Vec<(u64, usize, ScaleType, Result<(Vec<u8>, u32, u32, f32, f32)>)> = {
                let mut dl_map: HashMap<usize, DisplayList> = HashMap::new();
                for (_, page_index, _) in batch {
                    if !dl_map.contains_key(page_index) {
                        let dl = Self::load_display_list(doc, *page_index);
                        dl_map.insert(*page_index, dl);
                    }
                }

                let dl_refs: Vec<&DisplayList> = batch
                    .iter()
                    .map(|(_, page_index, _)| dl_map.get(page_index).unwrap())
                    .collect();

                let results = batch
                    .par_iter()
                    .zip(dl_refs.par_iter())
                    .map(|((_id, page_index, scale), dl)| {
                        let result = Self::render_from_dl(dl, *scale);
                        (*_id, *page_index, *scale, result)
                    })
                    .collect::<Vec<_>>();

                // dl_map and dl_refs dropped here — DisplayLists released,
                // their store references unpinned.
                results
            };

            for (_id, page_index, scale, result) in results {
                let _ = result_tx.send(ToMain::Done {
                    _id,
                    page_index,
                    scale,
                    result,
                });
            }

            // dl_map is dropped; running shrink now evicts objects
            // that were pinned by the batch's DisplayLists.
            shrink_mupdf_store();
        } else {
            // ── Serial path (1–2 items) ───────────────────────────
            for (_id, page_index, scale) in batch {
                let dl = Self::load_display_list(doc, *page_index);
                let result = Self::render_from_dl(&dl, *scale);
                let _ = result_tx.send(ToMain::Done {
                    _id: *_id,
                    page_index: *page_index,
                    scale: *scale,
                    result,
                });
            }
        }
    }

    /// Cap output pixel dimensions per scale type so that pages with very
    /// large natural dimensions (posters, high-res images) don't consume
    /// hundreds of MB per page. Normal A4 pages are unaffected — their
    /// natural dimensions × scale stay below the cap.
    fn max_pixel_dim(scale_type: ScaleType) -> f32 {
        match scale_type {
            ScaleType::Full => 2400.0,   // retina viewport (~1200 px × 2)
            ScaleType::Preview => 1200.0, // standard viewport (~820 px × 1.5)
            ScaleType::Thumb => 300.0,    // sidebar thumbnail width
        }
    }

    fn render_from_dl(
        dl: &DisplayList,
        scale_type: ScaleType,
    ) -> Result<(Vec<u8>, u32, u32, f32, f32)> {
        let requested_scale = scale_type.scale_value();
        let bounds = dl.bounds();
        let natural_w = (bounds.x1 - bounds.x0).abs() as f32;
        let natural_h = (bounds.y1 - bounds.y0).abs() as f32;
        let max_natural = natural_w.max(natural_h);

        // Cap the effective scale so the longest side ≤ max_pixel_dim px.
        // For normal A4 pages (595 pt) at Full (2×): 595 × 2 = 1190 px —
        // well below the 2400 px cap, so the scale is unchanged.
        let effective_scale = if max_natural > 0.0 {
            (Self::max_pixel_dim(scale_type) / max_natural).min(requested_scale)
        } else {
            requested_scale
        };

        let ctm = Matrix::new_scale(effective_scale, effective_scale);
        let pixmap = dl
            .to_pixmap(&ctm, &Colorspace::device_rgb(), true)
            .map_err(|e| anyhow!("mupdf to_pixmap: {e}"))?;

        let width = pixmap.width();
        let height = pixmap.height();
        let samples = pixmap.samples().to_vec();

        Ok((samples, width, height, natural_w, natural_h))
    }

    pub fn submit(&mut self, page_index: usize, scale: ScaleType) {
        let id = self.next_id;
        self.next_id += 1;
        let _ = self.cmd_tx.send(Cmd::Render {
            _id: id,
            page_index,
            scale,
        });
    }

    pub fn poll(&mut self) -> Option<ToMain> {
        self.result_rx.try_recv().ok()
    }
}

impl Drop for RenderHandle {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(Cmd::Shutdown);
    }
}
