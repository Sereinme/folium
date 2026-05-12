use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use anyhow::{anyhow, Result};
use mupdf::{Colorspace, DisplayList, Document, Matrix};
use rayon::prelude::*;

use crate::pdf::outline::parse_outline;
use crate::types::{OutlineItem, ScaleType};

// ── LRU DisplayList cache ─────────────────────────────────────────────

const DL_CACHE_MAX: usize = 20;

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
        result: Result<(Vec<u8>, u32, u32)>,
    },
}

// ── RenderHandle ───────────────────────────────────────────────────────

/// Max batch size for parallel rendering. Below this threshold, render sequentially.
const PARALLEL_BATCH_MIN: usize = 2;

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
        let doc = match Document::open(path.as_path()) {
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
                    // Collect a batch of render requests (non-blocking drain)
                    let mut batch = vec![(_id, page_index, scale)];
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
                                Self::process_batch(&doc, &mut dls, &batch, &result_tx);
                                return;
                            }
                        }
                    }

                    Self::process_batch(&doc, &mut dls, &batch, &result_tx);
                }
                Cmd::Shutdown => break,
            }
        }
    }

    fn process_batch(
        doc: &Document,
        dls: &mut DlCache,
        batch: &[(u64, usize, ScaleType)],
        result_tx: &Sender<ToMain>,
    ) {
        // Pre-create all DisplayLists, keeping raw pointers (DisplayList is Sync).
        // dls is not mutated during the parallel section that follows.
        struct DlPtr(*const DisplayList);
        // SAFETY: DisplayList is Sync and the pointers are valid for the
        // duration of this function call (dls is not mutated after this point).
        unsafe impl Send for DlPtr {}
        unsafe impl Sync for DlPtr {}

        let dl_ptrs: Vec<DlPtr> = batch
            .iter()
            .map(|(_, page_index, _)| {
                let dl = dls.get_or_create(*page_index, || {
                    doc.load_page(*page_index as i32)
                        .ok()
                        .and_then(|p| p.to_display_list(false).ok())
                        .expect("mupdf: failed to create DisplayList")
                });
                DlPtr(dl as *const DisplayList)
            })
            .collect();

        if batch.len() >= PARALLEL_BATCH_MIN {
            let results: Vec<(u64, usize, ScaleType, Result<(Vec<u8>, u32, u32)>)> = batch
                .par_iter()
                .zip(dl_ptrs.par_iter())
                .map(|((_id, page_index, scale), dl_ptr)| {
                    let dl = unsafe { &*dl_ptr.0 };
                    let result = Self::render_from_dl(dl, *scale);
                    (*_id, *page_index, *scale, result)
                })
                .collect();

            for (_id, page_index, scale, result) in results {
                let _ = result_tx.send(ToMain::Done {
                    _id,
                    page_index,
                    scale,
                    result,
                });
            }
        } else {
            for (i, (_id, page_index, scale)) in batch.iter().enumerate() {
                let dl = unsafe { &*dl_ptrs[i].0 };
                let result = Self::render_from_dl(dl, *scale);
                let _ = result_tx.send(ToMain::Done {
                    _id: *_id,
                    page_index: *page_index,
                    scale: *scale,
                    result,
                });
            }
        }
    }

    fn render_from_dl(dl: &DisplayList, scale_type: ScaleType) -> Result<(Vec<u8>, u32, u32)> {
        let scale = scale_type.scale_value();
        let ctm = Matrix::new_scale(scale, scale);
        let pixmap = dl
            .to_pixmap(&ctm, &Colorspace::device_rgb(), true)
            .map_err(|e| anyhow!("mupdf to_pixmap: {e}"))?;

        let width = pixmap.width();
        let height = pixmap.height();
        let samples = pixmap.samples().to_vec();

        Ok((samples, width, height))
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
