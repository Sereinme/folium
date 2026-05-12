use gpui::{
    div, img, px, AnyElement, Context, InteractiveElement, IntoElement, ParentElement, Styled,
};
use gpui_component::button::ButtonVariants;

use crate::types::ScaleType;
use crate::PdfReader;

use super::styles;

/// Gap between consecutive pages
const PAGE_GAP: f32 = 16.0;
/// Max display width
const MAX_PAGE_W: f32 = 860.0;
/// Default aspect: A4 = 1.414
const DEFAULT_ASPECT: f32 = 1.414;

pub fn reader_body(pdfr: &mut PdfReader, cx: &mut Context<PdfReader>) -> AnyElement {
    let Some(document) = &mut pdfr.document else {
        return no_pdf_view(cx);
    };

    let viewport_h = 800.0; // approximate; GPUI will clip

    // Compute page display dimensions (first rendered page gives aspect)
    let (nw, nh) = document.page_dim(pdfr.current_page);
    let aspect = if nw > 0.0 { nh / nw } else { DEFAULT_ASPECT };
    let page_display_w = MAX_PAGE_W.min(nw.max(595.0));
    let page_h = page_display_w * aspect;
    let step = page_h + PAGE_GAP; // Y offset per page

    // Clamp scroll
    let max_scroll = ((document.page_count as f32 * step) - viewport_h).max(0.0);
    if pdfr.scroll_offset > max_scroll {
        pdfr.scroll_offset = max_scroll;
    }
    // Update current_page from scroll
    let new_page = (pdfr.scroll_offset / step).round() as usize;
    if new_page != pdfr.current_page && new_page < document.page_count {
        pdfr.current_page = new_page;
    }

    // Find visible range
    let first_vis = (pdfr.scroll_offset / step).floor() as usize;
    let last_vis = ((pdfr.scroll_offset + viewport_h) / step).ceil() as usize + 1;
    let render_start = first_vis.saturating_sub(2);
    let render_end = (last_vis + 2).min(document.page_count);

    let mut frame = div()
        .flex_1()
        .h_full()
        .relative()
        .overflow_hidden()
        .bg(styles::BG_READER);

    // Handle scroll wheel
    let max_captured = max_scroll;
    frame.interactivity().on_scroll_wheel(cx.listener(
        move |this: &mut PdfReader, event: &gpui::ScrollWheelEvent, _window, cx| {
            let px = event.delta.pixel_delta(px(30.0));
            let delta: f32 = f32::from(px.y);
            let mut off = this.scroll_offset - delta;
            off = off.clamp(0.0, max_captured);
            this.scroll_offset = off;
            cx.notify();
        },
    ));

    // Inner content shifted by scroll_offset
    let mut inner = div()
        .relative()
        .top(px(-pdfr.scroll_offset))
        .flex()
        .flex_col()
        .items_center()
        .gap(px(PAGE_GAP));

    for i in render_start..render_end {
        let display_h = page_h;
        let is_current = i == pdfr.current_page;

        let mut page = div()
            .flex_none()
            .w(px(page_display_w))
            .h(px(display_h))
            .rounded(px(4.0))
            .overflow_hidden()
            .border_1()
            .border_color(if is_current { styles::ACCENT } else { styles::BORDER })
            .bg(styles::BG_WHITE);

        if let Some(cached) = document.cached_page(i, ScaleType::Full) {
            page = page.child(img(cached.image.clone()).w_full().h_full());
        } else if let Some(preview) = document.cached_page(i, ScaleType::Preview) {
            page = page.child(img(preview.image.clone()).w_full().h_full());
        } else if let Some(thumb) = document.cached_page(i, ScaleType::Thumb) {
            page = page.child(img(thumb.image.clone()).w_full().h_full());
        } else {
            page = page.child(
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

        inner = inner.child(page);
    }

    frame.child(inner).into_any_element()
}

fn no_pdf_view(cx: &mut Context<PdfReader>) -> AnyElement {
    div()
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
        .into_any_element()
}
