use gpui::{
    div, img, px, Context, IntoElement, ParentElement, Styled,
};
use gpui_component::button::ButtonVariants;
use gpui_component::scroll::ScrollableElement;

use crate::types::ScaleType;
use crate::PdfReader;

use super::styles;

/// How many pages to render in the scrollable window
const SCROLL_WINDOW_MAX: usize = 60;
/// Gap between consecutive pages
const PAGE_GAP: f32 = 16.0;

pub fn reader_body(pdfr: &mut PdfReader, cx: &mut Context<PdfReader>) -> impl IntoElement {
    let mut container = div()
        .flex_1()
        .h_full()
        .overflow_y_scrollbar()
        .bg(styles::BG_READER)
        .p_6()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(PAGE_GAP));

    let Some(document) = &mut pdfr.document else {
        return container.items_center().child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap_3()
                .text_color(styles::TEXT_PRIMARY)
                .child("No PDF open")
                .child(
                    gpui_component::button::Button::new("open-empty")
                        .primary()
                        .label("Open PDF")
                        .on_click(
                            cx.listener(|this: &mut PdfReader, _, window, cx| {
                                this.open_pdf(window, cx);
                            }),
                        ),
                ),
        );
    };

    let start = pdfr.scroll_window_start;
    let end = (start + SCROLL_WINDOW_MAX).min(document.page_count);

    for i in start..end {
        let (nw, nh) = document.page_dim(i);
        let aspect = if nw > 0.0 { nh / nw } else { 1.4 };
        let display_w = 860.0_f32.min(nw);
        let display_h = display_w * aspect;
        let is_current = i == pdfr.current_page;

        let mut page_outer = div()
            .flex_none()
            .w(px(display_w))
            .h(px(display_h))
            .rounded_md()
            .overflow_hidden()
            .border_1()
            .border_color(if is_current {
                styles::ACCENT
            } else {
                styles::BORDER
            })
            .bg(styles::BG_WHITE);

        if let Some(cached) = document.cached_page(i, ScaleType::Full) {
            page_outer = page_outer.child(
                img(cached.image.clone()).w_full().h_full(),
            );
        } else if let Some(preview) = document.cached_page(i, ScaleType::Preview) {
            page_outer = page_outer.child(
                img(preview.image.clone()).w_full().h_full(),
            );
        } else if let Some(thumb) = document.cached_page(i, ScaleType::Thumb) {
            page_outer = page_outer.child(
                img(thumb.image.clone()).w_full().h_full(),
            );
        } else {
            page_outer = page_outer.child(
                div()
                    .w_full()
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(styles::TEXT_SECONDARY)
                    .text_sm()
                    .child(format!("Page {}", i + 1)),
            );
        }

        container = container.child(page_outer);
    }

    container
}
