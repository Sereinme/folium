use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
};

use anyhow::{anyhow, Context as _, Result};
use gpui::{
    div, img, point, px, rgb, size, App, AppContext, Application, Bounds, Context, Entity,
    InteractiveElement, IntoElement, MouseButton, ParentElement, PathPromptOptions, Render,
    RenderImage, SharedString, Styled, StyledImage, TitlebarOptions, Window, WindowBounds,
    WindowOptions,
};
use gpui::prelude::FluentBuilder;
use gpui_component::{
    button::{Button, ButtonVariants},
    menu::AppMenuBar,
    scroll::ScrollableElement,
    Root, TitleBar,
};
use hayro::{
    hayro_interpret::InterpreterSettings,
    hayro_syntax::{
        object::{Array, Dict, Name, Object, ObjectIdentifier},
        xref::XRef,
        Pdf,
    },
    vello_cpu::color::palette::css::WHITE,
    render, RenderCache, RenderSettings,
};
use image::{Frame, RgbaImage};
use smallvec::SmallVec;

const DEFAULT_SCALE: f32 = 2.0;
const THUMB_SCALE: f32 = 0.25;

#[derive(Clone, Copy, PartialEq, Eq)]
enum SidebarTab {
    Thumbnails,
    Outline,
}

struct PdfPageImage {
    image: Arc<RenderImage>,
    width: u32,
    height: u32,
}

struct OutlineItem {
    title: String,
    page_index: Option<usize>,
    children: Vec<OutlineItem>,
}

struct FlatOutlineItem {
    path: Vec<usize>,
    title: String,
    page_index: Option<usize>,
    depth: usize,
    has_children: bool,
}

struct PdfDocument {
    path: PathBuf,
    pdf: Pdf,
    page_count: usize,
    pages: Vec<Option<PdfPageImage>>,
    thumbnails: Vec<Option<PdfPageImage>>,
    outline: Vec<OutlineItem>,
}

impl PdfDocument {
    fn open(path: PathBuf) -> Result<Self> {
        let data = std::fs::read(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let pdf = Pdf::new(data).map_err(|error| anyhow!("failed to parse PDF: {error:?}"))?;
        let page_count = pdf.pages().len();

        let outline = {
            let xref = pdf.xref();
            let mut page_map: HashMap<ObjectIdentifier, usize> = HashMap::new();
            for (i, page) in pdf.pages().iter().enumerate() {
                if let Some(id) = page.raw().obj_id() {
                    page_map.insert(id, i);
                }
            }
            let catalog: Option<Dict> = xref.get(xref.root_id());
            match catalog {
                Some(ref catalog) => parse_outline(xref, catalog, &page_map),
                None => Vec::new(),
            }
        };

        Ok(Self {
            path,
            pdf,
            page_count,
            pages: (0..page_count).map(|_| None).collect(),
            thumbnails: (0..page_count).map(|_| None).collect(),
            outline,
        })
    }

    fn page_image(&mut self, page_index: usize) -> Result<Option<&PdfPageImage>> {
        self.render_page(page_index, false)?;
        Ok(self.pages.get(page_index).and_then(Option::as_ref))
    }

    fn thumbnail(&mut self, page_index: usize) -> Result<Option<&PdfPageImage>> {
        self.render_page(page_index, true)?;
        Ok(self.thumbnails.get(page_index).and_then(Option::as_ref))
    }

    fn render_page(&mut self, page_index: usize, thumbnail: bool) -> Result<()> {
        let target = if thumbnail {
            &mut self.thumbnails
        } else {
            &mut self.pages
        };

        if target.get(page_index).and_then(Option::as_ref).is_some() {
            return Ok(());
        }

        let page = match self.pdf.pages().get(page_index) {
            Some(page) => page,
            None => return Ok(()),
        };
        let scale = if thumbnail { THUMB_SCALE } else { DEFAULT_SCALE };
        let cache = RenderCache::new();
        let pixmap = render(
            page,
            &cache,
            &InterpreterSettings::default(),
            &RenderSettings {
                x_scale: scale,
                y_scale: scale,
                bg_color: WHITE,
                ..Default::default()
            },
        );
        let width = u32::from(pixmap.width());
        let height = u32::from(pixmap.height());
        let mut pixels = pixmap.data_as_u8_slice().to_vec();

        for pixel in pixels.chunks_exact_mut(4) {
            pixel.swap(0, 2);
            if pixel[3] > 0 {
                let alpha = pixel[3] as f32 / 255.0;
                pixel[0] = (pixel[0] as f32 / alpha).clamp(0.0, 255.0) as u8;
                pixel[1] = (pixel[1] as f32 / alpha).clamp(0.0, 255.0) as u8;
                pixel[2] = (pixel[2] as f32 / alpha).clamp(0.0, 255.0) as u8;
            }
        }

        let buffer = RgbaImage::from_raw(width, height, pixels)
            .context("renderer returned an invalid pixel buffer")?;
        target[page_index] = Some(PdfPageImage {
            image: Arc::new(RenderImage::new(SmallVec::from_elem(Frame::new(buffer), 1))),
            width,
            height,
        });

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// PDF outline (table of contents) parsing
// ---------------------------------------------------------------------------

fn decode_pdf_string(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        let utf16: Vec<u16> = bytes[2..]
            .chunks(2)
            .filter(|c| c.len() == 2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&utf16)
    } else if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
        let utf16: Vec<u16> = bytes[2..]
            .chunks(2)
            .filter(|c| c.len() == 2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&utf16)
    } else if let Ok(s) = std::str::from_utf8(bytes) {
        s.to_string()
    } else {
        bytes.iter().map(|&b| b as char).collect()
    }
}

fn parse_outline(
    xref: &XRef,
    catalog: &Dict,
    page_map: &HashMap<ObjectIdentifier, usize>,
) -> Vec<OutlineItem> {
    let outlines_root: Option<Dict> = catalog.get(b"Outlines");
    let outlines_root = match outlines_root {
        Some(o) => o,
        None => return Vec::new(),
    };
    let mut result = Vec::new();
    if let Some(first) = outlines_root.get::<Dict>(b"First") {
        collect_outline_items(xref, catalog, page_map, &first, &mut result);
    }
    result
}

fn collect_outline_items(
    xref: &XRef,
    catalog: &Dict,
    page_map: &HashMap<ObjectIdentifier, usize>,
    item: &Dict,
    result: &mut Vec<OutlineItem>,
) {
    let title = item
        .get::<hayro::hayro_syntax::object::String>(b"Title")
        .map(|s| decode_pdf_string(&s))
        .unwrap_or_default();

    let page_index = resolve_outline_dest(item, xref, catalog, page_map);

    let mut children = Vec::new();
    if let Some(first_child) = item.get::<Dict>(b"First") {
        collect_outline_items(xref, catalog, page_map, &first_child, &mut children);
    }

    result.push(OutlineItem {
        title,
        page_index,
        children,
    });

    if let Some(next) = item.get::<Dict>(b"Next") {
        collect_outline_items(xref, catalog, page_map, &next, result);
    }
}

fn resolve_outline_dest(
    item: &Dict,
    xref: &XRef,
    catalog: &Dict,
    page_map: &HashMap<ObjectIdentifier, usize>,
) -> Option<usize> {
    if let Some(dest_array) = item.get::<Array>(b"Dest") {
        if let Some(page_index) = page_from_dest_array(&dest_array, page_map) {
            return Some(page_index);
        }
    }

    if let Some(dest_name) = item.get::<Name>(b"Dest") {
        if let Some(dests) = catalog.get::<Dict>(b"Dests") {
            if let Some(dest_array) = dests.get::<Array>(dest_name.as_ref()) {
                if let Some(page_index) = page_from_dest_array(&dest_array, page_map) {
                    return Some(page_index);
                }
            }
        }
    }

    if let Some(dest_str) = item.get::<hayro::hayro_syntax::object::String>(b"Dest") {
        if let Some(dests) = catalog.get::<Dict>(b"Dests") {
            if let Some(dest_array) = dests.get::<Array>(dest_str.as_ref()) {
                if let Some(page_index) = page_from_dest_array(&dest_array, page_map) {
                    return Some(page_index);
                }
            }
        }
    }

    if let Some(names) = catalog.get::<Dict>(b"Names") {
        if let Some(dests_tree) = names.get::<Dict>(b"Dests") {
            if let Some(page_index) = lookup_name_tree(&dests_tree, xref, catalog, page_map) {
                return Some(page_index);
            }
        }
    }

    None
}

fn page_from_dest_array(
    arr: &Array,
    page_map: &HashMap<ObjectIdentifier, usize>,
) -> Option<usize> {
    let mut iter = arr.iter::<Object>();
    let first = iter.next()?;
    match first {
        Object::Dict(page_dict) => {
            let id = page_dict.obj_id()?;
            page_map.get(&id).copied()
        }
        _ => None,
    }
}

fn lookup_name_tree(
    node: &Dict,
    xref: &XRef,
    catalog: &Dict,
    page_map: &HashMap<ObjectIdentifier, usize>,
) -> Option<usize> {
    if let Some(names_arr) = node.get::<Array>(b"Names") {
        let items: Vec<Object> = names_arr.iter().collect();
        for chunk in items.chunks(2) {
            if let [_, dest_obj] = chunk {
                if let Object::Array(arr) = dest_obj {
                    if let Some(index) = page_from_dest_array(arr, page_map) {
                        return Some(index);
                    }
                }
            }
        }
    }

    if let Some(kids) = node.get::<Array>(b"Kids") {
        for kid in kids.iter::<Dict>() {
            if let Some(index) = lookup_name_tree(&kid, xref, catalog, page_map) {
                return Some(index);
            }
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Outline flattening for UI display
// ---------------------------------------------------------------------------

fn flatten_outline(items: &[OutlineItem], collapsed: &HashSet<Vec<usize>>) -> Vec<FlatOutlineItem> {
    let mut result = Vec::new();
    flatten_recursive(items, collapsed, &mut result, &[]);
    result
}

fn flatten_recursive(
    items: &[OutlineItem],
    collapsed: &HashSet<Vec<usize>>,
    result: &mut Vec<FlatOutlineItem>,
    base_path: &[usize],
) {
    for (i, item) in items.iter().enumerate() {
        let mut path = base_path.to_vec();
        path.push(i);
        let has_children = !item.children.is_empty();
        let is_collapsed = collapsed.contains(&path);
        result.push(FlatOutlineItem {
            path,
            title: item.title.clone(),
            page_index: item.page_index,
            depth: base_path.len(),
            has_children,
        });
        if !is_collapsed && has_children {
            let mut child_path = base_path.to_vec();
            child_path.push(i);
            flatten_recursive(&item.children, collapsed, result, &child_path);
        }
    }
}

// ---------------------------------------------------------------------------
// PDF Reader application
// ---------------------------------------------------------------------------

pub struct PdfReader {
    document: Option<PdfDocument>,
    current_page: usize,
    sidebar_tab: SidebarTab,
    status: Option<String>,
    app_menu_bar: Entity<AppMenuBar>,
    outline_collapsed: HashSet<Vec<usize>>,
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
        };

        if let Some(path) = initial_path {
            this.load_pdf(path);
        }

        this
    }

    fn load_pdf(&mut self, path: PathBuf) {
        match PdfDocument::open(path) {
            Ok(document) => {
                self.current_page = 0;
                self.status = None;
                self.outline_collapsed.clear();
                self.document = Some(document);
            }
            Err(error) => {
                self.document = None;
                self.current_page = 0;
                self.status = Some(error.to_string());
            }
        }
    }

    fn open_pdf(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn select_page(&mut self, page_index: usize, cx: &mut Context<Self>) {
        self.current_page = page_index;
        cx.notify();
    }

    fn previous_page(&mut self, cx: &mut Context<Self>) {
        if self.current_page > 0 {
            self.current_page -= 1;
            cx.notify();
        }
    }

    fn next_page(&mut self, cx: &mut Context<Self>) {
        if let Some(document) = &self.document {
            if self.current_page + 1 < document.page_count {
                self.current_page += 1;
                cx.notify();
            }
        }
    }

    fn toggle_outline_item(&mut self, path: Vec<usize>, cx: &mut Context<Self>) {
        if self.outline_collapsed.contains(&path) {
            self.outline_collapsed.remove(&path);
        } else {
            self.outline_collapsed.insert(path);
        }
        cx.notify();
    }

    fn sidebar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match self.sidebar_tab {
            SidebarTab::Thumbnails => self.thumbnail_list(cx).into_any_element(),
            SidebarTab::Outline => self.outline_list(cx).into_any_element(),
        };

        div()
            .w(px(220.0))
            .h_full()
            .flex_none()
            .border_r_1()
            .border_color(rgb(0xd8dde6))
            .bg(rgb(0xf7f8fa))
            .child(
                div()
                    .flex()
                    .gap_1()
                    .p_2()
                    .border_b_1()
                    .border_color(rgb(0xd8dde6))
                    .child(self.sidebar_tab_button("Thumbnails", SidebarTab::Thumbnails, cx))
                    .child(self.sidebar_tab_button("Outline", SidebarTab::Outline, cx)),
            )
            .child(content)
    }

    fn sidebar_tab_button(
        &self,
        label: &'static str,
        tab: SidebarTab,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selected = self.sidebar_tab == tab;
        div()
            .px_2()
            .py_1()
            .rounded_md()
            .text_sm()
            .text_color(if selected { rgb(0xffffff) } else { rgb(0x303540) })
            .bg(if selected { rgb(0x2f6fed) } else { rgb(0xe8ebf0) })
            .hover(|style| style.cursor_pointer().bg(rgb(0xdfe5ee)))
            .on_mouse_up(MouseButton::Left, cx.listener(move |this, _, _, cx| {
                this.sidebar_tab = tab;
                cx.notify();
            }))
            .child(label)
    }

    fn thumbnail_list(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut list = div()
            .overflow_y_scrollbar()
            .h_full()
            .p_2()
            .flex()
            .flex_col()
            .gap_2();

        if let Some(document) = &mut self.document {
            for page_index in 0..document.page_count {
                let selected = self.current_page == page_index;
                let image = match document.thumbnail(page_index) {
                    Ok(Some(page)) => Some((page.image.clone(), page.width, page.height)),
                    Ok(None) => None,
                    Err(error) => {
                        self.status = Some(error.to_string());
                        None
                    }
                };

                let mut item = div()
                    .p_2()
                    .rounded_md()
                    .border_1()
                    .border_color(if selected { rgb(0x2f6fed) } else { rgb(0xd8dde6) })
                    .bg(if selected { rgb(0xeaf1ff) } else { rgb(0xffffff) })
                    .hover(|style| style.cursor_pointer().bg(rgb(0xeef3fa)))
                    .on_mouse_up(MouseButton::Left, cx.listener(move |this, _, _, cx| {
                        this.select_page(page_index, cx);
                    }))
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0x677080))
                            .mb_1()
                            .child(format!("Page {}", page_index + 1)),
                    );

                if let Some((image, width, height)) = image {
                    let ratio = height as f32 / width.max(1) as f32;
                    item = item.child(
                        img(image)
                            .w_full()
                            .h(px(170.0 * ratio))
                            .object_fit(gpui::ObjectFit::Contain),
                    );
                }

                list = list.child(item);
            }
        } else {
            list = list.child(self.empty_sidebar());
        }

        list
    }

    fn outline_list(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut list = div().overflow_y_scrollbar().h_full().p_2().flex().flex_col();

        if let Some(document) = &self.document {
            if document.outline.is_empty() {
                list = list.child(self.empty_outline());
            } else {
                let collapsed = &self.outline_collapsed;
                let flat_items = flatten_outline(&document.outline, collapsed);
                for item in &flat_items {
                    list = list.child(self.render_outline_item(item, cx));
                }
            }
        } else {
            list = list.child(self.empty_sidebar());
        }

        list
    }

    fn render_outline_item(&self, item: &FlatOutlineItem, cx: &mut Context<Self>) -> impl IntoElement {
        let page_index = item.page_index;
        let has_children = item.has_children;
        let indent_px = (item.depth * 16) as f32;
        let is_current = page_index.map_or(false, |p| self.current_page == p);
        let display_title = if item.title.is_empty() {
            SharedString::from("(untitled)")
        } else {
            SharedString::from(item.title.clone())
        };

        let row = div()
            .flex()
            .items_center()
            .w_full()
            .px_1()
            .py(px(4.0))
            .rounded_md()
            .text_sm()
            .text_color(if is_current { rgb(0x1a4bdb) } else { rgb(0x303540) })
            .bg(if is_current { rgb(0xeaf1ff) } else { rgb(0xffffff) })
            .hover(|style| style.cursor_pointer().bg(rgb(0xe8eef8)));

        let row = if has_children {
            let p = item.path.clone();
            row.child(
                div()
                    .w(px(16.0))
                    .flex_none()
                    .text_center()
                    .text_xs()
                    .text_color(rgb(0x677080))
                    .hover(|style| style.cursor_pointer().bg(rgb(0xdde3ec)).rounded_md())
                    .on_mouse_up(MouseButton::Left, cx.listener(move |this, _, _, cx| {
                        this.toggle_outline_item(p.clone(), cx);
                    }))
                    .child(if self.outline_collapsed.contains(&item.path) { "+" } else { "-" }),
            )
        } else {
            row.child(div().w(px(16.0)).flex_none())
        };

        let row = row.child(div().w(px(indent_px)).flex_none());

        let row = row.child(
            div()
                .flex_1()
                .overflow_x_hidden()
                .child(display_title),
        );

        row.on_mouse_up(MouseButton::Left, cx.listener(move |this, _, _, cx| {
            if let Some(pi) = page_index {
                this.select_page(pi, cx);
            }
        }))
    }

    fn empty_sidebar(&self) -> impl IntoElement {
        div()
            .p_3()
            .text_sm()
            .text_color(rgb(0x677080))
            .child("Open a PDF to show pages.")
    }

    fn empty_outline(&self) -> impl IntoElement {
        div()
            .p_3()
            .text_sm()
            .text_color(rgb(0x677080))
            .child("No outline is available for this document.")
    }

    fn reader_body(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut body = div()
            .flex_1()
            .h_full()
            .overflow_y_scrollbar()
            .bg(rgb(0xe7e9ee))
            .p_6()
            .flex()
            .justify_center();

        if let Some(document) = &mut self.document {
            match document.page_image(self.current_page) {
                Ok(Some(page)) => {
                    let display_width = (page.width as f32 / DEFAULT_SCALE).min(980.0);
                    let display_height =
                        display_width * page.height as f32 / page.width.max(1) as f32;

                    body = body.child(
                        div()
                            .bg(rgb(0xffffff))
                            .border_1()
                            .border_color(rgb(0xc9ced8))
                            .shadow_lg()
                            .child(
                                img(page.image.clone())
                                    .w(px(display_width))
                                    .h(px(display_height))
                                    .object_fit(gpui::ObjectFit::Contain),
                            ),
                    );
                }
                Ok(None) => {
                    body = body.child("Page is not available.");
                }
                Err(error) => {
                    self.status = Some(error.to_string());
                    body = body.child("Failed to render page.");
                }
            }
        } else {
            body = body.items_center().child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_3()
                    .text_color(rgb(0x3d4655))
                    .child("No PDF open")
                    .child(
                        Button::new("open-empty")
                            .primary()
                            .label("Open PDF")
                            .on_click(cx.listener(|this: &mut PdfReader, _, window, cx| {
                                this.open_pdf(window, cx);
                            })),
                    ),
            );
        }

        body
    }
}

impl Render for PdfReader {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
                                    .text_color(rgb(0x303540))
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
                    .child(self.sidebar(cx))
                    .child(self.reader_body(cx)),
            )
            .when_some(self.status.clone(), |this, status| {
                this.child(
                    div()
                        .border_t_1()
                        .border_color(rgb(0xd8dde6))
                        .bg(rgb(0xfff7ed))
                        .text_color(rgb(0x9a3412))
                        .text_sm()
                        .px_3()
                        .py_2()
                        .child(status),
                )
            })
    }
}

fn initial_pdf_path() -> Option<PathBuf> {
    std::env::args_os()
        .skip(1)
        .map(PathBuf::from)
        .find(|path| path.extension().is_some_and(|extension| extension == "pdf"))
}

fn main() {
    let app = Application::new();
    let initial_path = initial_pdf_path();

    app.run(move |cx: &mut App| {
        gpui_component::init(cx);
        cx.activate(true);

        let window_options = WindowOptions {
            titlebar: Some(TitlebarOptions {
                title: Some(SharedString::from("gpdf")),
                appears_transparent: false,
                ..Default::default()
            }),
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: point(px(160.0), px(120.0)),
                size: size(px(1180.0), px(820.0)),
            })),
            ..Default::default()
        };

        cx.open_window(window_options, |window, cx| {
            let view = cx.new(|cx| PdfReader::new(window, cx, initial_path.clone()));
            cx.new(|cx| Root::new(view, window, cx))
        })
        .expect("failed to open window");
    });
}
