use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use anyhow::{anyhow, Result};
use mupdf::{Colorspace, DisplayList, Document, Matrix};

use crate::pdf::outline::parse_outline;
use crate::types::{OutlineItem, ScaleType};

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
            .spawn(move || {
                Self::render_thread(path, cmd_rx, result_tx);
            })
            .map_err(|e| anyhow!("failed to spawn render thread: {e}"))?;

        Ok(Self {
            cmd_tx,
            result_rx,
            next_id: 0,
        })
    }

    fn render_thread(path: PathBuf, cmd_rx: Receiver<Cmd>, result_tx: Sender<ToMain>) {
        let data = match std::fs::read(&path) {
            Ok(d) => d,
            Err(_) => {
                let _ = result_tx.send(ToMain::Init { page_count: 0 });
                return;
            }
        };

        let doc = match Document::from_bytes(&data, "pdf") {
            Ok(d) => d,
            Err(_e) => {
                let _ = result_tx.send(ToMain::Init { page_count: 0 });
                return;
            }
        };
        drop(data);

        let page_count = doc.page_count().unwrap_or(0) as usize;

        // Send Init ASAP — UI can show layout immediately
        if result_tx.send(ToMain::Init { page_count }).is_err() {
            return;
        }

        // Parse outline in background (can be slow for complex PDFs)
        let outline = parse_outline(&doc);
        let _ = result_tx.send(ToMain::Outline(outline));

        // Per-page DisplayList cache: parse content ONCE, render at any scale
        let mut dls: HashMap<usize, DisplayList> = HashMap::new();

        for cmd in cmd_rx {
            match cmd {
                Cmd::Render {
                    _id,
                    page_index,
                    scale,
                } => {
                    let result = Self::render_page(&doc, &mut dls, page_index, scale);
                    let _ = result_tx.send(ToMain::Done {
                        _id,
                        page_index,
                        scale,
                        result,
                    });
                }
                Cmd::Shutdown => break,
            }
        }
    }

    fn render_page(
        doc: &Document,
        dls: &mut HashMap<usize, DisplayList>,
        page_index: usize,
        scale_type: ScaleType,
    ) -> Result<(Vec<u8>, u32, u32)> {
        let dl = dls.entry(page_index).or_insert_with(|| {
            doc.load_page(page_index as i32)
                .ok()
                .and_then(|p| p.to_display_list(true).ok())
                .unwrap()
        });

        let scale = scale_type.scale_value();
        let ctm = Matrix::new_scale(scale, scale);
        let cs = Colorspace::device_rgb();
        let pixmap = dl
            .to_pixmap(&ctm, &cs, true)
            .map_err(|e| anyhow!("mupdf dl.to_pixmap: {e}"))?;
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
