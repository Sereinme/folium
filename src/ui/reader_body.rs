use gpui::{
    div, img, px, Context, IntoElement, ParentElement, Styled, StyledImage,
};
use gpui_component::scroll::ScrollableElement;

use crate::types::ScaleType;
use crate::PdfReader;
use gpui_component::button::ButtonVariants;

use super::styles;

pub fn reader_body(pdfr: &mut PdfReader, cx: &mut Context<PdfReader>) -> impl IntoElement {
    let mut body = div()
        .flex_1()
        .h_full()
        .overflow_y_scrollbar()
        .bg(styles::BG_READER)
        .p_6()
        .flex()
        .justify_center();

    let Some(document) = &mut pdfr.document else {
        return body.items_center().child(
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

    let page_index = pdfr.current_page;

    if let Some(cached) = document.cached_page(page_index, ScaleType::Full) {
        let scale = ScaleType::Full.scale_value();
        let display_width = (cached.width as f32 / scale).min(980.0);
        let display_height =
            display_width * cached.height as f32 / cached.width.max(1) as f32;

        body = body.child(
            div()
                .bg(styles::BG_WHITE)
                .border_1()
                .border_color(styles::BORDER_STRONG)
                .shadow_lg()
                .child(
                    img(cached.image.clone())
                        .w(px(display_width))
                        .h(px(display_height))
                        .object_fit(gpui::ObjectFit::Contain),
                ),
        );
    } else if let Some(thumb) = document.cached_page(page_index, ScaleType::Thumb) {
        let display_width = (thumb.width as f32 / ScaleType::Thumb.scale_value()).min(980.0);
        let display_height = display_width * thumb.height as f32 / thumb.width.max(1) as f32;
        body = body.child(
            div()
                .bg(styles::BG_WHITE)
                .border_1()
                .border_color(styles::BORDER)
                .child(
                    img(thumb.image.clone())
                        .w(px(display_width))
                        .h(px(display_height))
                        .object_fit(gpui::ObjectFit::Contain),
                ),
        );
    } else {
        body = body.items_center().child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap_2()
                .text_color(styles::TEXT_SECONDARY)
                .child("Loading page..."),
        );
    }

    body
}
