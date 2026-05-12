use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;

use gpui::{
    div, px, Context, Entity, IntoElement, ParentElement, PathPromptOptions, Render, SharedString,
    Styled, Window,
};
use gpui_component::{
    button::Button,
    menu::AppMenuBar,
    TitleBar,
};
use gpui::prelude::FluentBuilder;

use crate::pdf::PdfDocument;
use crate::types::{ScaleType, SidebarTab};
use crate::ui::{self, styles};

const BATCH_SIZE: usize = 3;
const QUEUE_REFILL_THRESHOLD: usize = 8;
const RENDER_RANGE: usize = 10;

pub struct PdfReader {
    pub document: Option<PdfDocument>,
    pub current_page: usize,
    pub sidebar_tab: SidebarTab,
    pub status: Option<String>,
    pub app_menu_bar: Entity<AppMenuBar>,
    pub outline_collapsed: HashSet<Vec<usize>>,
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
        self.rebuild_render_queue();
        cx.notify();
    }

    fn previous_page(&mut self, cx: &mut Context<Self>) {
        if self.current_page > 0 {
            self.current_page -= 1;
            self.rebuild_render_queue();
            cx.notify();
        }
    }

    fn next_page(&mut self, cx: &mut Context<Self>) {
        if let Some(document) = &self.document {
            if self.current_page + 1 < document.page_count {
                self.current_page += 1;
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
        let Some(document) = &self.document else {
            return;
        };
        self.render_queue.clear();

        let cur = self.current_page;

        // Current page full-scale (highest priority)
        if !document.is_cached(cur, ScaleType::Full) {
            self.render_queue.push_back((cur, ScaleType::Full));
        }

        // Thumbnails around current page (descending proximity)
        let start = cur.saturating_sub(RENDER_RANGE);
        let end = (cur + RENDER_RANGE + 1).min(document.page_count);

        if cur + 1 < end {
            self.render_queue
                .push_back((cur + 1, ScaleType::Thumb));
        }
        if cur > 0 {
            self.render_queue.push_back((cur - 1, ScaleType::Thumb));
        }

        for i in (start..end).rev() {
            if i == cur || i == cur + 1 || i == cur.wrapping_sub(1) {
                continue;
            }
            self.render_queue.push_back((i, ScaleType::Thumb));
        }
    }

    fn poll_and_submit(&mut self) {
        let Some(document) = &mut self.document else {
            return;
        };

        // Drain completed renders
        document.poll_render_results();

        // Submit batch from queue
        let mut submitted = 0;
        while submitted < BATCH_SIZE {
            let Some((idx, scale)) = self.render_queue.pop_front() else {
                break;
            };
            if !document.is_cached(idx, scale) {
                document.request_render(idx, scale);
            }
            submitted += 1;
        }

        // Refill if running low
        if self.render_queue.len() < QUEUE_REFILL_THRESHOLD {
            let cur = self.current_page;
            let start = cur.saturating_sub(RENDER_RANGE);
            let end = (cur + RENDER_RANGE + 1).min(document.page_count);

            for i in start..end {
                let scale = if i == cur {
                    ScaleType::Full
                } else {
                    ScaleType::Thumb
                };
                if !document.is_cached(i, scale) {
                    let entry = (i, scale);
                    if !self.render_queue.contains(&entry) {
                        self.render_queue.push_back(entry);
                    }
                }
            }
        }
    }
}

impl Render for PdfReader {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.poll_and_submit();

        let has_pending = !self.render_queue.is_empty()
            || self.document.as_ref().is_some_and(|d| {
                let cur = d.page_count;
                for i in 0..cur {
                    if !d.is_cached(i, ScaleType::Thumb) {
                        return true;
                    }
                }
                false
            });

        if has_pending {
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
