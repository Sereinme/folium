use std::collections::HashSet;

use gpui::{
    div, px, Context, InteractiveElement, IntoElement, MouseButton, ParentElement, SharedString,
    Styled,
};
use gpui_component::scroll::ScrollableElement;

use crate::types::{FlatOutlineItem, OutlineItem};
use crate::PdfReader;

use super::styles;

pub fn outline_panel(pdfr: &mut PdfReader, cx: &mut Context<PdfReader>) -> impl IntoElement {
    let mut list = div().overflow_y_scrollbar().h_full().p_2().flex().flex_col();

    let Some(document) = &pdfr.document else {
        return list.child(empty_sidebar());
    };

    if document.outline.is_empty() {
        return list.child(empty_outline());
    }

    let flat_items = flatten_outline(&document.outline, &pdfr.outline_collapsed);
    for item in &flat_items {
        list = list.child(render_item(item, pdfr, cx));
    }

    list
}

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
            path: path.clone(),
            title: item.title.clone(),
            page_index: item.page_index,
            depth: base_path.len(),
            has_children,
        });

        if !is_collapsed && has_children {
            flatten_recursive(&item.children, collapsed, result, &path);
        }
    }
}

fn render_item(
    item: &FlatOutlineItem,
    pdfr: &PdfReader,
    cx: &mut Context<PdfReader>,
) -> impl IntoElement {
    let page_index = item.page_index;
    let has_children = item.has_children;
    let indent_px = (item.depth * 16) as f32;
    let is_current = page_index.map_or(false, |p| pdfr.current_page == p);
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
        .text_color(if is_current {
            styles::TEXT_LINK
        } else {
            styles::TEXT_PRIMARY
        })
        .bg(if is_current {
            styles::ACCENT_LIGHT
        } else {
            styles::BG_WHITE
        })
        .hover(|style| style.cursor_pointer().bg(styles::OUTLINE_HOVER));

    let row = if has_children {
        let p = item.path.clone();
        row.child(
            div()
                .w(px(16.0))
                .flex_none()
                .text_center()
                .text_xs()
                .text_color(styles::TEXT_SECONDARY)
                .hover(|style| style.cursor_pointer().bg(styles::EXPANDER_HOVER).rounded_md())
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(move |this: &mut PdfReader, _, _, cx| {
                        this.toggle_outline_item(p.clone(), cx);
                    }),
                )
                .child({
                    let collapsed = pdfr.outline_collapsed.contains(&item.path);
                    if collapsed { "+" } else { "-" }
                }),
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

    row.on_mouse_up(
        MouseButton::Left,
        cx.listener(move |this: &mut PdfReader, _, _, cx| {
            if let Some(pi) = page_index {
                this.select_page(pi, cx);
            }
        }),
    )
}

fn empty_sidebar() -> impl IntoElement {
    div()
        .p_3()
        .text_sm()
        .text_color(styles::TEXT_SECONDARY)
        .child("Open a PDF to show pages.")
}

fn empty_outline() -> impl IntoElement {
    div()
        .p_3()
        .text_sm()
        .text_color(styles::TEXT_SECONDARY)
        .child("No outline is available for this document.")
}
