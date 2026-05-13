use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::time::Duration;

use gpui::{
    div, px, Context, Entity, IntoElement, ParentElement, PathPromptOptions, Render, ScrollHandle,
    SharedString, Styled, Window,
};
use gpui::prelude::FluentBuilder;
use gpui_component::{
    button::Button,
    menu::AppMenuBar,
    TitleBar,
};

use crate::pdf::PdfDocument;
use crate::types::{ScaleType, SidebarTab};
use crate::ui::{self, styles};

/// Render Full (2×) only for the current page and its immediate neighbours.
/// SumatraPDF-style: discard when out of view, re-render on scroll-back.
const RENDER_FULL_RADIUS: usize = 1;
/// Only pre-render thumbnails for a few nearby pages.
const RENDER_THUMB_RADIUS: usize = 5;

pub struct PdfReader {
    pub document: Option<PdfDocument>,
    pub current_page: usize,
    pub sidebar_tab: SidebarTab,
    pub status: Option<String>,
    pub app_menu_bar: Entity<AppMenuBar>,
    pub outline_collapsed: HashSet<Vec<usize>>,
    pub scroll_offset: f32,                  // manual scroll: how far we've scrolled in pixels
    pub sidebar_scroll_handle: ScrollHandle, // sidebar thumbnail scroll
    render_queue: VecDeque<(usize, ScaleType)>,
    pub render_stamp: usize,                 // increment each render() to force GPUI repaint
    pub scroll_offset_dirty: bool, // true when modified by scroll wheel
    pump_active: bool,
    render_gen: usize,
}

impl PdfReader {
    pub fn new(window: &mut Window, cx: &mut Context<Self>, initial_path: Option<PathBuf>) -> Self {
        let app_menu_bar = AppMenuBar::new(window, cx);
        let mut this = Self {
            document: None,
            current_page: 0,
            sidebar_tab: SidebarTab::Thumbnails,
            status: None,
            app_menu_bar,
            outline_collapsed: HashSet::new(),
            scroll_offset: 0.0,
            sidebar_scroll_handle: ScrollHandle::new(),
            render_queue: VecDeque::new(),
            render_stamp: 0,
            scroll_offset_dirty: false,
            pump_active: false,
            render_gen: 0,
        };

        if let Some(path) = initial_path {
            this.load_pdf(path, cx);
        }

        this
    }

    pub fn load_pdf(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        match PdfDocument::open(path) {
            Ok(document) => {
                self.current_page = 0;
                self.scroll_offset = 0.0;
                self.scroll_offset_dirty = false;
                self.status = None;
                self.outline_collapsed.clear();
                self.render_queue.clear();
                self.pump_active = false;
                self.render_gen = self.render_gen.wrapping_add(1);
                self.document = Some(document);
                cx.notify();
            }
            Err(error) => {
                self.document = None;
                self.current_page = 0;
                self.status = Some(error.to_string());
                cx.notify();
            }
        }
    }

    pub fn open_pdf(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(SharedString::from("Open PDF")),
        });

        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = receiver.await {
                if let Some(path) = paths.into_iter().next() {
                    this.update(cx, |this, cx| {
                        this.load_pdf(path, cx);
                    })?;
                }
            }
            Ok::<_, anyhow::Error>(())
        })
        .detach();
    }

    pub fn select_page(&mut self, page_index: usize, cx: &mut Context<Self>) {
        self.current_page = page_index;
        self.scroll_offset_dirty = false;
        if let Some(doc) = &mut self.document {
            doc.evict_distant(page_index);
            let (nw, nh) = doc.page_dim(page_index);
            let a = if nw > 0.0 { nh / nw } else { 1.414 };
            let step = (820.0_f32.min(nw.max(595.0)) * a) + 16.0;
            self.scroll_offset = page_index as f32 * step;
        }
        self.sidebar_scroll_handle.scroll_to_item(page_index);
        self.rebuild_render_queue();
        cx.notify();
    }

    fn previous_page(&mut self, cx: &mut Context<Self>) {
        if self.current_page > 0 {
            self.current_page -= 1;
            self.scroll_offset_dirty = false;
            if let Some(doc) = &self.document {
                let (nw, nh) = doc.page_dim(self.current_page);
                let a = if nw > 0.0 { nh / nw } else { 1.414 };
                self.scroll_offset = self.current_page as f32 * ((820.0_f32.min(nw.max(595.0)) * a) + 16.0);
            }
            self.sidebar_scroll_handle.scroll_to_item(self.current_page);
            self.rebuild_render_queue();
            cx.notify();
        }
    }

    fn next_page(&mut self, cx: &mut Context<Self>) {
        if let Some(document) = &self.document {
            if self.current_page + 1 < document.page_count {
                self.current_page += 1;
                self.scroll_offset_dirty = false;
                if let Some(doc) = &self.document {
                    let (nw, nh) = doc.page_dim(self.current_page);
                    let a = if nw > 0.0 { nh / nw } else { 1.414 };
                    self.scroll_offset = self.current_page as f32 * ((820.0_f32.min(nw.max(595.0)) * a) + 16.0);
                }
                self.sidebar_scroll_handle.scroll_to_item(self.current_page);
                self.rebuild_render_queue();
                cx.notify();
            }
        }
    }

    /// Sync current_page from scroll_offset (called before UI build)
    pub fn sync_current_page(&mut self) {
        let Some(doc) = &mut self.document else { return };
        if !doc.initialized { return; }
        let (nw, nh) = doc.page_dim(self.current_page);
        let a = if nw > 0.0 { nh / nw } else { 1.414 };
        let step = (820.0_f32.min(nw.max(595.0)) * a) + 16.0;
        let new_page = (self.scroll_offset / step).round() as usize;
        if new_page < doc.page_count && new_page != self.current_page {
            if self.scroll_offset_dirty {
                // User scrolled: update current_page to match scroll position
                self.current_page = new_page;
                doc.evict_distant(new_page);
                self.scroll_offset_dirty = false;
            } else {
                // select_page set scroll_offset with stale/fallback dimensions
                // that have since been updated by a page render. Keep
                // current_page, recalculate scroll_offset.
                self.scroll_offset = self.current_page as f32 * step;
            }
            self.sidebar_scroll_handle.scroll_to_item(self.current_page);
        }
    }

    pub fn toggle_outline_item(&mut self, path: Vec<usize>, cx: &mut Context<Self>) {
        if self.outline_collapsed.contains(&path) {
            self.outline_collapsed.remove(&path);
        } else {
            self.outline_collapsed.insert(path);
        }
        cx.notify();
    }

    fn rebuild_render_queue(&mut self) {
        let Some(document) = &self.document else { return };
        if !document.initialized { return; }

        self.render_queue.clear();
        let cur = self.current_page;
        let max = document.page_count;

        // radius=0: only the current page (avoid pushing it twice)
        if cur < max {
            if !document.is_cached(cur, ScaleType::Preview) {
                self.render_queue.push_back((cur, ScaleType::Preview));
            }
            if !document.is_cached(cur, ScaleType::Thumb) {
                self.render_queue.push_back((cur, ScaleType::Thumb));
            }
            if !document.is_cached(cur, ScaleType::Full) {
                self.render_queue.push_back((cur, ScaleType::Full));
            }
        }

        for radius in 1..=RENDER_FULL_RADIUS {
            for &i in &[cur.wrapping_sub(radius), cur + radius] {
                if i >= max { continue; }
                if !document.is_cached(i, ScaleType::Preview) {
                    self.render_queue.push_back((i, ScaleType::Preview));
                }
                if !document.is_cached(i, ScaleType::Thumb) {
                    self.render_queue.push_back((i, ScaleType::Thumb));
                }
                if !document.is_cached(i, ScaleType::Full) {
                    self.render_queue.push_back((i, ScaleType::Full));
                }
            }
        }

        for radius in (RENDER_FULL_RADIUS + 1)..=RENDER_THUMB_RADIUS {
            for &i in &[cur.wrapping_sub(radius), cur + radius] {
                if i >= max { continue; }
                if !document.is_cached(i, ScaleType::Thumb) {
                    self.render_queue.push_back((i, ScaleType::Thumb));
                }
            }
        }
    }

    fn poll_and_submit(&mut self) -> bool {
        let changed = self
            .document
            .as_mut()
            .map_or(false, |d| d.poll_render_results());

        let inited = self.document.as_ref().is_some_and(|d| d.initialized);

        if !inited {
            return changed || true;
        }

        let needs_rebuild = self.render_queue.is_empty()
            && self.document.as_ref().map_or(false, |d| d.inflight == 0)
            && self.document.as_ref().is_some_and(|d| {
                let cur = self.current_page;
                let full_start = cur.saturating_sub(RENDER_FULL_RADIUS);
                let full_end = (cur + RENDER_FULL_RADIUS + 1).min(d.page_count);
                let needs_full = (full_start..full_end)
                    .any(|i| !d.is_cached(i, ScaleType::Full)
                        || !d.is_cached(i, ScaleType::Thumb));

                let thumb_start = cur.saturating_sub(RENDER_THUMB_RADIUS);
                let thumb_end = (cur + RENDER_THUMB_RADIUS + 1).min(d.page_count);
                let needs_thumb = (thumb_start..thumb_end)
                    .any(|i| !d.is_cached(i, ScaleType::Thumb));

                needs_full || needs_thumb
            });

        if needs_rebuild {
            self.rebuild_render_queue();
        }

        let batch: Vec<_> = self.render_queue.drain(..).collect();
        let mut submitted = false;
        if let Some(document) = &mut self.document {
            for (idx, scale) in batch {
                if document.is_cached(idx, scale) && scale != ScaleType::Preview {
                    continue;
                }
                document.request_render(idx, scale);
                submitted = true;
            }
        }
        changed || submitted
    }

    fn print_memory_diag(&self) {
        if let Some(doc) = &self.document {
            let full_mem: u64 = doc.pages.iter()
                .filter_map(|p| p.as_ref())
                .map(|p| p.width as u64 * p.height as u64 * 4)
                .sum();
            let preview_mem: u64 = doc.previews.values()
                .map(|p| p.width as u64 * p.height as u64 * 4)
                .sum();
            let thumb_mem: u64 = doc.thumbnails.iter()
                .filter_map(|t| t.as_ref())
                .map(|t| t.width as u64 * t.height as u64 * 4)
                .sum();
            let full_n = doc.pages.iter().filter(|p| p.is_some()).count();
            let prev_n = doc.previews.len();
            let thumb_n = doc.thumbnails.iter().filter(|t| t.is_some()).count();
            eprintln!(
                "[mem] Full={}×{:.1}MB Preview={}×{:.1}MB Thumb={}×{:.1}MB img={:.1}MB | page_count={} inflight={}",
                full_n, full_mem as f64 / 1_048_576.0,
                prev_n, preview_mem as f64 / 1_048_576.0,
                thumb_n, thumb_mem as f64 / 1_048_576.0,
                (full_mem + preview_mem + thumb_mem) as f64 / 1_048_576.0,
                doc.page_count,
                doc.inflight,
            );
        }
    }

    /// Returns true if the render pump should keep running.
    fn needs_pump(&self) -> bool {
        self.document.as_ref().is_some_and(|d| !d.initialized || d.inflight > 0)
    }

    fn ensure_pump(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pump_active {
            return;
        }
        let needs = self.needs_pump();
        if !needs {
            return;
        }

        self.pump_active = true;
        let generation = self.render_gen;

        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(32))
                    .await;

                let keep_going = this.update(cx, |this, cx| {
                    if this.render_gen != generation {
                        return false;
                    }
                    let changed = this.poll_and_submit();
                    if changed {
                        cx.notify();
                    }
                    this.needs_pump()
                })?;

                if !keep_going {
                    break;
                }
            }

            this.update(cx, |this, _| {
                if this.render_gen == generation {
                    this.pump_active = false;
                    this.print_memory_diag();
                }
            })?;

            Ok::<_, anyhow::Error>(())
        })
        .detach();
    }
}

impl Render for PdfReader {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_stamp = self.render_stamp.wrapping_add(1);
        self.sync_current_page();
        let changed = self.poll_and_submit();

        // Start or restart the pump when there's pending work (load / render)
        if changed || self.needs_pump() {
            self.ensure_pump(window, cx);
        }

        let title = self
            .document
            .as_ref()
            .and_then(|document| document.path.file_name())
            .and_then(|name| name.to_str())
            .unwrap_or("Folium");
        let page_label = self
            .document
            .as_ref()
            .filter(|d| d.initialized)
            .map(|document| format!("{} / {}", self.current_page + 1, document.page_count))
            .unwrap_or_else(|| "- / -".to_string());

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                TitleBar::new()
                    .child(self.app_menu_bar.clone())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .w_full()
                            .text_sm()
                            .child(
                                div()
                                    .flex_1()
                                    .text_color(styles::TEXT_PRIMARY)
                                    .child(title.to_string()),
                            )
                            .child(Button::new("open").label("Open").on_click(
                                cx.listener(|this: &mut PdfReader, _, window, cx| {
                                    this.open_pdf(window, cx);
                                }),
                            ))
                            .child(Button::new("prev").label("Prev").on_click(
                                cx.listener(|this: &mut PdfReader, _, _, cx| {
                                    this.previous_page(cx);
                                }),
                            ))
                            .child(div().min_w(px(64.0)).text_center().child(page_label))
                            .child(Button::new("next").label("Next").on_click(
                                cx.listener(|this: &mut PdfReader, _, _, cx| {
                                    this.next_page(cx);
                                }),
                            )),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(ui::sidebar::sidebar(self, cx))
                    .child(ui::reader_body::reader_body(self, cx)),
            )
            .when_some(self.status.clone(), |this, status| {
                this.child(
                    div()
                        .border_t_1()
                        .border_color(styles::BORDER)
                        .bg(styles::STATUS_BG)
                        .text_color(styles::STATUS_TEXT)
                        .text_sm()
                        .px_3()
                        .py_2()
                        .child(status),
                )
            })
    }
}
