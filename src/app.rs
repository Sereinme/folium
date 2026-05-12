use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;

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

const RENDER_FULL_RADIUS: usize = 30;
const RENDER_THUMB_RADIUS: usize = 80;

pub struct PdfReader {
    pub document: Option<PdfDocument>,
    pub current_page: usize,
    pub sidebar_tab: SidebarTab,
    pub status: Option<String>,
    pub app_menu_bar: Entity<AppMenuBar>,
    pub outline_collapsed: HashSet<Vec<usize>>,
    pub scroll_handle: ScrollHandle,        // reader body scroll
    pub sidebar_scroll_handle: ScrollHandle, // sidebar thumbnail scroll
    skip_sync: bool,                         // skip one frame after programmatic scroll
    render_queue: VecDeque<(usize, ScaleType)>,
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
            scroll_handle: ScrollHandle::new(),
            sidebar_scroll_handle: ScrollHandle::new(),
            skip_sync: false,
            render_queue: VecDeque::new(),
        };

        if let Some(path) = initial_path {
            this.load_pdf(path);
        }

        this
    }

    pub fn load_pdf(&mut self, path: PathBuf) {
        match PdfDocument::open(path) {
            Ok(document) => {
                self.current_page = 0;
                self.status = None;
                self.outline_collapsed.clear();
                self.render_queue.clear();
                self.document = Some(document);
            }
            Err(error) => {
                self.document = None;
                self.current_page = 0;
                self.status = Some(error.to_string());
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
                        this.load_pdf(path);
                        cx.notify();
                    })?;
                }
            }
            Ok::<_, anyhow::Error>(())
        })
        .detach();
    }

    pub fn select_page(&mut self, page_index: usize, cx: &mut Context<Self>) {
        self.current_page = page_index;
        self.skip_sync = true;
        self.scroll_handle.scroll_to_top_of_item(page_index);
        self.sidebar_scroll_handle.scroll_to_item(page_index);
        self.rebuild_render_queue();
        cx.notify();
    }

    fn previous_page(&mut self, cx: &mut Context<Self>) {
        if self.current_page > 0 {
            self.current_page -= 1;
            self.skip_sync = true;
            self.scroll_handle.scroll_to_top_of_item(self.current_page);
            self.sidebar_scroll_handle.scroll_to_item(self.current_page);
            self.rebuild_render_queue();
            cx.notify();
        }
    }

    fn next_page(&mut self, cx: &mut Context<Self>) {
        if let Some(document) = &self.document {
            if self.current_page + 1 < document.page_count {
                self.current_page += 1;
                self.skip_sync = true;
                self.scroll_handle.scroll_to_top_of_item(self.current_page);
                self.sidebar_scroll_handle.scroll_to_item(self.current_page);
                self.rebuild_render_queue();
                cx.notify();
            }
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

        for radius in 0..=RENDER_FULL_RADIUS {
            for &i in &[cur.wrapping_sub(radius), cur + radius] {
                if i >= max { continue; }
                if !document.is_cached(i, ScaleType::Thumb) {
                    self.render_queue.push_back((i, ScaleType::Thumb));
                }
                self.render_queue.push_back((i, ScaleType::Preview));
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
            && self.document.as_ref().is_some_and(|d| {
                let cur = self.current_page;
                let end = (cur + RENDER_FULL_RADIUS + 1).min(d.page_count);
                (cur..end)
                    .any(|i| !d.is_cached(i, ScaleType::Full) || !d.is_cached(i, ScaleType::Thumb))
            });

        if needs_rebuild {
            self.rebuild_render_queue();
        }

        let batch: Vec<_> = self.render_queue.drain(..).collect();
        let mut submitted = false;
        let mut remaining = Vec::new();
        if let Some(document) = &mut self.document {
            for item in batch {
                if !document.can_render() {
                    remaining.push(item);
                    continue;
                }
                let (idx, scale) = item;
                if !document.is_cached(idx, scale) {
                    document.request_render(idx, scale);
                    submitted = true;
                }
            }
        } else {
            remaining = batch;
        }
        self.render_queue.extend(remaining);
        changed || submitted
    }
}

impl PdfReader {
    /// Find which page occupies the most viewport area.
    fn majority_page(&self) -> Option<usize> {
        let vp = self.scroll_handle.bounds();
        let vp_top: f32 = f32::from(vp.top());
        let vp_bot: f32 = f32::from(vp.bottom());
        let top = self.scroll_handle.top_item();
        let bot = self.scroll_handle.bottom_item();

        let mut best = None;
        let mut best_visible = 0.0_f32;

        for i in top..=bot {
            if let Some(b) = self.scroll_handle.bounds_for_item(i) {
                let item_top: f32 = f32::from(b.top());
                let item_bot: f32 = f32::from(b.bottom());
                let visible = item_bot.min(vp_bot) - item_top.max(vp_top);
                if visible > best_visible {
                    best_visible = visible;
                    best = Some(i);
                }
            }
        }
        best
    }

    /// Every frame: syncs current_page + sidebar scroll from reader scroll position.
    /// Skips one frame after programmatic scroll (select_page/nav) to avoid stale top_item().
    fn sync_sidebar_from_scroll(&mut self) {
        let Some(document) = &self.document else { return };
        if !document.initialized { return; }

        if self.skip_sync {
            self.skip_sync = false;
            return;
        }

        if let Some(page) = self.majority_page() {
            if page < document.page_count && page != self.current_page {
                self.current_page = page;
                self.sidebar_scroll_handle.scroll_to_item(page);
            }
        }
    }
}

impl Render for PdfReader {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_sidebar_from_scroll();
        let needs_refresh = self.poll_and_submit();

        if needs_refresh
            || self.document.as_ref().is_some_and(|d| {
                if !d.initialized { return true; }
                d.inflight > 0 || !self.render_queue.is_empty()
            })
        {
            cx.notify();
        }

        let title = self
            .document
            .as_ref()
            .and_then(|document| document.path.file_name())
            .and_then(|name| name.to_str())
            .unwrap_or("gpdf");
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
