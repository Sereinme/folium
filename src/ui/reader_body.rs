use gpui::{
    div, img, px, AnyElement, Context, ElementId, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled,
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

    let (nw, nh) = document.page_dim(pdfr.current_page.max(1) - 1);
    let aspect = if nw > 0.0 { nh / nw } else { DEFAULT_ASPECT };
    let page_w = MAX_PAGE_W.min(nw.max(595.0));
    let page_h = page_w * aspect;
    let step = page_h + PAGE_GAP;
    let page_count = document.page_count;

    // Scrollable container
    let mut frame = div()
        .id(ElementId::named_usize("reader-body-scroll", 0))
        .track_scroll(&pdfr.scroll_handle)
        .overflow_y_scroll()
        .flex_1()
        .h_full()
        .bg(styles::BG_READER)
        .p_6()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(PAGE_GAP));

    // Loading state
    if !document.initialized {
        frame = frame.child(
            div()
                .flex_1()
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(styles::TEXT_SECONDARY)
                .text_sm()
                .child("Loading document…"),
        );
        return frame.into_any_element();
    }

    // Wheel → update wheel_accumulator + current_page + sidebar scroll
    let step_cap = step;
    let max_page = page_count;
    frame.interactivity().on_scroll_wheel(cx.listener(
        move |this: &mut PdfReader, event: &gpui::ScrollWheelEvent, _window, cx| {
            let px_delta = event.delta.pixel_delta(px(30.0));
            let delta: f32 = f32::from(px_delta.y);
            this.wheel_accumulator += delta;

            let new_page = (this.wheel_accumulator / step_cap).round() as usize;
            if new_page < max_page && new_page != this.current_page {
                this.current_page = new_page;
                this.sidebar_scroll_handle.scroll_to_item(new_page);
            }
            cx.notify();
        },
    ));

    // Render all pages
    for i in 0..page_count {
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

        frame = frame.child(page);
    }

    frame.into_any_element()
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
