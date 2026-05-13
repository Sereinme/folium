use gpui::{
    div, img, px, AnyElement, Context, ElementId, InteractiveElement, IntoElement, ParentElement,
    Styled,
};
use gpui_component::button::ButtonVariants;

use crate::types::ScaleType;
use crate::PdfReader;

use super::styles;

const PAGE_GAP: f32 = 16.0;
const MAX_PAGE_W: f32 = 820.0;
const DEFAULT_ASPECT: f32 = 1.414;

pub fn reader_body(pdfr: &mut PdfReader, cx: &mut Context<PdfReader>) -> AnyElement {
    let Some(document) = &mut pdfr.document else {
        return no_pdf_view(cx);
    };

    let (nw, nh) = document.page_dim(pdfr.current_page);
    let aspect = if nw > 0.0 { nh / nw } else { DEFAULT_ASPECT };
    let page_w = MAX_PAGE_W.min(nw.max(595.0));
    let page_h = page_w * aspect;
    let step = page_h + PAGE_GAP;
    let max_page = document.page_count;

    // Clamp scroll offset
    let max_off = ((max_page as f32 * step) - 800.0).max(0.0);
    pdfr.scroll_offset = pdfr.scroll_offset.clamp(0.0, max_off);

    // Fixed viewport with overflow_hidden — inner content shifted by scroll_offset
    let frame_stamp = pdfr.render_stamp;
    let mut frame = div()
        .flex_1()
        .h_full()
        .relative()
        .overflow_hidden()
        .bg(styles::BG_READER);

    // Capture scroll wheel (viewport is NOT scrollable, so GPUI doesn't consume events)
    let max_cap = max_off;
    frame.interactivity().on_scroll_wheel(cx.listener(
        move |this: &mut PdfReader, event: &gpui::ScrollWheelEvent, _window, cx| {
            let px_delta = event.delta.pixel_delta(px(30.0));
            let delta: f32 = f32::from(px_delta.y);
            this.scroll_offset -= delta;
            this.scroll_offset = this.scroll_offset.clamp(0.0, max_cap);
            this.scroll_offset_dirty = true;
            cx.notify();
        },
    ));

    if !document.initialized {
        return div()
            .id(ElementId::named_usize("rp-stamp", frame_stamp))
            .child(
                frame.child(
                    div()
                        .w_full()
                        .h_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(styles::TEXT_SECONDARY)
                        .text_sm()
                        .child("Loading document…"),
                ),
            )
            .into_any_element();
    }

    // Inner content: shifted by -scroll_offset
    let mut inner = div()
        .relative()
        .top(px(-pdfr.scroll_offset))
        .flex()
        .flex_col()
        .items_center()
        .gap(px(PAGE_GAP));

    for i in 0..max_page {
        let display_h = page_h;
        let is_current = i == pdfr.current_page;

        let mut page = div()
            .flex_none()
            .w(px(page_w))
            .h(px(display_h))
            .rounded(px(4.0))
            .overflow_hidden()
            .border_1()
            .border_color(if is_current { styles::ACCENT } else { styles::BORDER })
            .bg(styles::BG_WHITE);

        if let Some(cached) = document.cached_page(i, ScaleType::Full) {
            page = page.child(img(cached.image.clone()).w_full().h_full());
        } else if let Some(pv) = document.cached_page(i, ScaleType::Preview) {
            page = page.child(img(pv.image.clone()).w_full().h_full());
        } else if let Some(thumb) = document.cached_page(i, ScaleType::Thumb) {
            page = page.child(img(thumb.image.clone()).w_full().h_full());
        } else {
            page = page.bg(styles::BG_WHITE).child(
                div()
                    .w_full()
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_xs()
                    .text_color(styles::TEXT_SECONDARY)
                    .child("Loading…"),
            );
        }

        inner = inner.child(page);
    }

    // Stamped wrapper forces GPUI to re-paint even when element tree appears identical
    let wrapper_id = ElementId::named_usize("rp-stamp", frame_stamp);
    div()
        .id(wrapper_id)
        .child(frame.child(inner))
        .into_any_element()
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
