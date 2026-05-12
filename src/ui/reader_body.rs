use gpui::{
    div, img, px, AnyElement, Context, IntoElement, ParentElement, Styled,
};
use gpui_component::button::ButtonVariants;
use gpui_component::scroll::ScrollableElement;

use crate::types::ScaleType;
use crate::PdfReader;

use super::styles;

/// Number of pages rendered ahead from scroll start
const WINDOW_SIZE: usize = 150;

pub fn reader_body(pdfr: &mut PdfReader, cx: &mut Context<PdfReader>) -> AnyElement {
    let Some(document) = &mut pdfr.document else {
        return div()
            .flex_1()
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
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
            )
            .into_any_element();
    };

    let start = pdfr
        .current_page
        .saturating_sub(5)
        .min(document.page_count.saturating_sub(1));
    let end = (start + WINDOW_SIZE).min(document.page_count);

    let mut body = div()
        .flex_1()
        .h_full()
        .overflow_y_scrollbar()
        .bg(styles::BG_READER)
        .p_6()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(12.0));

    for i in start..end {
        let (nw, nh) = document.page_dim(i);
        let aspect = if nw > 0.0 { nh / nw } else { 1.4 };
        let display_w = 860.0_f32.min(nw);
        let display_h = display_w * aspect;
        let is_current = i == pdfr.current_page;

        let mut page_div = div()
            .flex_none()
            .w(px(display_w))
            .h(px(display_h))
            .rounded(px(4.0))
            .overflow_hidden()
            .border_1()
            .border_color(if is_current {
                styles::ACCENT
            } else {
                styles::BORDER
            })
            .bg(styles::BG_WHITE);

        if let Some(cached) = document.cached_page(i, ScaleType::Full) {
            page_div = page_div.child(
                img(cached.image.clone()).w_full().h_full(),
            );
        } else if let Some(preview) = document.cached_page(i, ScaleType::Preview) {
            page_div = page_div.child(
                img(preview.image.clone()).w_full().h_full(),
            );
        } else if let Some(thumb) = document.cached_page(i, ScaleType::Thumb) {
            page_div = page_div.child(
                img(thumb.image.clone()).w_full().h_full(),
            );
        } else {
            page_div = page_div.child(
                div()
                    .w_full()
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(styles::TEXT_SECONDARY)
                    .text_xs()
                    .child(format!("Page {}", i + 1)),
            );
        }

        body = body.child(page_div);
    }

    body.into_any_element()
}
