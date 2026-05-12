use mupdf::{Document, Outline};

use crate::types::OutlineItem;

pub fn parse_outline(doc: &Document) -> Vec<OutlineItem> {
    doc.outlines()
        .unwrap_or_default()
        .iter()
        .map(|o| convert_outline(o))
        .collect()
}

fn convert_outline(o: &Outline) -> OutlineItem {
    let page_index = o
        .dest
        .as_ref()
        .and_then(|d| Some(d.loc.page_number as usize));

    OutlineItem {
        title: o.title.clone(),
        page_index,
        children: o.down.iter().map(convert_outline).collect(),
    }
}
