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
/// Extra pages to render above and below the visible viewport.
const VIRTUAL_BUFFER: usize = 3;
const VIEWPORT_H: f32 = 800.0;

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
    let max_off = ((max_page as f32 * step) - VIEWPORT_H).max(0.0);
    pdfr.scroll_offset = pdfr.scroll_offset.clamp(0.0, max_off);

    let frame_stamp = pdfr.render_stamp;
    let mut frame = div()
        .flex_1()
        .h_full()
        .relative()
        .overflow_hidden()
        .bg(styles::BG_READER);

    let max_cap = max_off;
    frame.interactivity().on_scroll_wheel(cx.listener(
        move |this: &mut PdfReader, event: &gpui::ScrollWheelEvent, _window, cx| {
            let px_delta = event.delta.pixel_delta(px(30.0));
            let delta: f32 = f32::from(px_delta.y);
            this.scroll_offset -= delta;
            this.scroll_offset = this.scroll_offset.clamp(0.0, max_cap);
            this.scroll_offset_dirty = true;
            let prev_page = this.current_page;
            this.sync_current_page();
            if this.current_page != prev_page {
                this.submit_renders();
            }
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

    // Determine visible page range (± buffer for smooth scrolling)
    let visible_start = (pdfr.scroll_offset / step).floor() as usize;
    let visible_end = ((pdfr.scroll_offset + VIEWPORT_H) / step).ceil() as usize;
    let range_start = visible_start.saturating_sub(VIRTUAL_BUFFER);
    let range_end = (visible_end + VIRTUAL_BUFFER).min(max_page);

    let top_spacer_h = range_start as f32 * step;

    // Inner content shifted by scroll_offset, starting from the first rendered page
    let mut inner = div()
        .relative()
        .top(px(-pdfr.scroll_offset))
        .flex()
        .flex_col()
        .items_center()
        .gap(px(PAGE_GAP));

    // Spacer for unrendered pages above the visible range
    if top_spacer_h > 0.0 {
        inner = inner.child(div().h(px(top_spacer_h)).flex_none());
    }

    for i in range_start..range_end {
        let is_current = i == pdfr.current_page;

        let mut page = div()
            .flex_none()
            .w(px(page_w))
            .h(px(page_h))
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
            page = page.child(
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
