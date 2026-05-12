mod app;
mod pdf;
mod render_queue;
mod types;
mod ui;

use std::path::PathBuf;

use gpui::{
    point, px, size, App, AppContext, Application, Bounds, SharedString, TitlebarOptions,
    WindowBounds, WindowOptions,
};
use gpui_component::Root;

use crate::app::PdfReader;

fn initial_pdf_path() -> Option<PathBuf> {
    std::env::args_os()
        .skip(1)
        .map(PathBuf::from)
        .find(|path| path.extension().is_some_and(|ext| ext == "pdf"))
}

fn main() {
    let app = Application::new();
    let initial_path = initial_pdf_path();

    app.run(move |cx: &mut App| {
        gpui_component::init(cx);
        cx.activate(true);

        let window_options = WindowOptions {
            titlebar: Some(TitlebarOptions {
                title: Some(SharedString::from("Folium")),
                appears_transparent: false,
                ..Default::default()
            }),
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: point(px(160.0), px(120.0)),
                size: size(px(1180.0), px(820.0)),
            })),
            ..Default::default()
        };

        cx.open_window(window_options, |window, cx| {
            let view = cx.new(|cx| PdfReader::new(window, cx, initial_path.clone()));
            cx.new(|cx| Root::new(view, window, cx))
        })
        .expect("failed to open window");
    });
}
