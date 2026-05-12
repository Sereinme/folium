use gpui::{
    div, img, px, AnyElement, Context, ElementId, InteractiveElement, IntoElement, MouseButton,
    ParentElement, StatefulInteractiveElement, Styled, StyledImage,
};

use crate::types::ScaleType;
use crate::PdfReader;

use super::styles;

pub fn thumbnail_list(pdfr: &mut PdfReader, cx: &mut Context<PdfReader>) -> AnyElement {
    let Some(document) = &mut pdfr.document else {
        return div()
            .flex_1()
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .p_3()
                    .text_sm()
                    .text_color(styles::TEXT_SECONDARY)
                    .child("Open a PDF to show pages."),
            )
            .into_any_element();
    };

    // Fixed element ID — track_scroll + ScrollHandle handle programmatic scroll
    let mut list = div()
        .id(ElementId::named_usize("sidebar-thumbnails", 0))
        .track_scroll(&pdfr.sidebar_scroll_handle)
        .overflow_y_scroll()
        .h_full()
        .p_2()
        .flex()
        .flex_col()
        .gap_2();

    for page_index in 0..document.page_count {
        let selected = pdfr.current_page == page_index;

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
