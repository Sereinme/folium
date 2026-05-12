use std::sync::Arc;

use gpui::RenderImage;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SidebarTab {
    Thumbnails,
    Outline,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScaleType {
    Full,
    Thumb,
}

impl ScaleType {
    pub fn scale_value(self) -> f32 {
        match self {
            ScaleType::Full => 1.5,
            ScaleType::Thumb => 0.25,
        }
    }
}

#[derive(Clone)]
pub struct PdfPageImage {
    pub image: Arc<RenderImage>,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug)]
pub struct OutlineItem {
    pub title: String,
    pub page_index: Option<usize>,
    pub children: Vec<OutlineItem>,
}

pub struct FlatOutlineItem {
    pub path: Vec<usize>,
    pub title: String,
    pub page_index: Option<usize>,
    pub depth: usize,
    pub has_children: bool,
}
