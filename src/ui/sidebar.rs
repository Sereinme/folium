use gpui::{
    div, px, rgb, Context, InteractiveElement, IntoElement, MouseButton, ParentElement, Styled,
};

use crate::types::SidebarTab;
use crate::PdfReader;

use super::thumbnail_list::thumbnail_list;
use super::outline_panel::outline_panel;
use super::styles;

pub fn sidebar(pdfr: &mut PdfReader, cx: &mut Context<PdfReader>) -> impl IntoElement {
    let content = if pdfr.sidebar_tab == SidebarTab::Thumbnails {
        thumbnail_list(pdfr, cx).into_any_element()
    } else {
        outline_panel(pdfr, cx).into_any_element()
    };

    div()
        .w(px(styles::SIDEBAR_WIDTH))
        .h_full()
        .flex_none()
        .border_r_1()
        .border_color(styles::BORDER)
        .bg(styles::BG_SIDEBAR)
        .child(
            div()
                .flex()
                .gap_1()
                .p_2()
                .border_b_1()
                .border_color(styles::BORDER)
                .child(tab_button("Thumbnails", SidebarTab::Thumbnails, pdfr, cx))
                .child(tab_button("Outline", SidebarTab::Outline, pdfr, cx)),
        )
        .child(content)
}

fn tab_button(
    label: &'static str,
    tab: SidebarTab,
    pdfr: &PdfReader,
    cx: &mut Context<PdfReader>,
) -> impl IntoElement {
    let selected = pdfr.sidebar_tab == tab;
    div()
        .px_2()
        .py_1()
        .rounded_md()
        .text_sm()
        .text_color(if selected {
            rgb(0xffffff)
        } else {
            styles::TEXT_PRIMARY
        })
        .bg(if selected { styles::ACCENT } else { styles::TAB_BG })
        .hover(|style| style.cursor_pointer().bg(styles::TAB_HOVER))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(move |this: &mut PdfReader, _, _, cx| {
                this.sidebar_tab = tab;
                cx.notify();
            }),
        )
        .child(label)
}
