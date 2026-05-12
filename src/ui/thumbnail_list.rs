use gpui::{
    div, img, px, AnyElement, Context, ElementId, InteractiveElement, IntoElement, MouseButton,
    ParentElement, Styled, StyledImage,
};
use gpui_component::scroll::ScrollableElement;

use crate::types::ScaleType;
use crate::PdfReader;

use super::styles;

/// Thumbnail window: pages before/after current_page shown in sidebar
const THUMB_RADIUS: usize = 30;

pub fn thumbnail_list(pdfr: &mut PdfReader, cx: &mut Context<PdfReader>) -> AnyElement {
    let Some(document) = &mut pdfr.document else {
        return div()
            .overflow_y_scrollbar()
            .h_full()
            .p_2()
            .flex()
            .flex_col()
            .gap_2()
            .child(empty_sidebar())
            .into_any_element();
    };

    // Element ID encodes current_page → fresh scroll state on navigate
    // makes current_page thumbnail visible at the window's center
    let scroll_id = ElementId::named_usize("thumb-scroll", pdfr.current_page);
    let cur = pdfr.current_page;
    let start = cur.saturating_sub(THUMB_RADIUS);
    let end = (cur + THUMB_RADIUS + 1).min(document.page_count);

    let mut list = div()
        .id(scroll_id)
        .overflow_y_scrollbar()
        .h_full()
        .p_2()
        .flex()
        .flex_col()
        .gap_2();

    for page_index in start..end {
        let selected = cur == page_index;

        let mut item = div()
            .p_2()
            .rounded_md()
            .border_1()
            .border_color(if selected {
                styles::ACCENT
            } else {
                styles::BORDER
            })
            .bg(if selected {
                styles::ACCENT_LIGHT
            } else {
                styles::BG_WHITE
            })
            .hover(|style| style.cursor_pointer().bg(styles::THUMB_HOVER))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this: &mut PdfReader, _, _, cx| {
                    this.select_page(page_index, cx);
                }),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(styles::TEXT_SECONDARY)
                    .mb_1()
                    .child(format!("Page {}", page_index + 1)),
            );

        if let Some(cached) = document.cached_page(page_index, ScaleType::Thumb) {
            let ratio = cached.height as f32 / cached.width.max(1) as f32;
            item = item.child(
                img(cached.image.clone())
                    .w_full()
                    .h(px(styles::THUMB_MAX_HEIGHT * ratio))
                    .object_fit(gpui::ObjectFit::Contain),
            );
        } else {
            item = item.child(
                div()
                    .w_full()
                    .h(px(80.0))
                    .bg(styles::BORDER)
                    .rounded_sm(),
            );
        }

        list = list.child(item);
    }

    list.into_any_element()
}

fn empty_sidebar() -> impl IntoElement {
    div()
        .p_3()
        .text_sm()
        .text_color(styles::TEXT_SECONDARY)
        .child("Open a PDF to show pages.")
}
