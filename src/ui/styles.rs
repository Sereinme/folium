use gpui::Rgba;

const fn hex(hex: u32) -> Rgba {
    Rgba {
        r: ((hex >> 16) & 0xFF) as f32 / 255.0,
        g: ((hex >> 8) & 0xFF) as f32 / 255.0,
        b: (hex & 0xFF) as f32 / 255.0,
        a: 1.0,
    }
}

pub const SIDEBAR_WIDTH: f32 = 220.0;
pub const THUMB_MAX_HEIGHT: f32 = 170.0;

pub const ACCENT: Rgba = hex(0x2f6fed);
pub const ACCENT_LIGHT: Rgba = hex(0xeaf1ff);
pub const BG_SIDEBAR: Rgba = hex(0xf7f8fa);
pub const BG_READER: Rgba = hex(0xe7e9ee);
pub const BG_WHITE: Rgba = hex(0xffffff);
pub const BORDER: Rgba = hex(0xd8dde6);
pub const TEXT_PRIMARY: Rgba = hex(0x303540);
pub const TEXT_SECONDARY: Rgba = hex(0x677080);
pub const TEXT_LINK: Rgba = hex(0x1a4bdb);
pub const TAB_BG: Rgba = hex(0xe8ebf0);
pub const TAB_HOVER: Rgba = hex(0xdfe5ee);
pub const THUMB_HOVER: Rgba = hex(0xeef3fa);
pub const OUTLINE_HOVER: Rgba = hex(0xe8eef8);
pub const EXPANDER_HOVER: Rgba = hex(0xdde3ec);
pub const STATUS_BG: Rgba = hex(0xfff7ed);
pub const STATUS_TEXT: Rgba = hex(0x9a3412);
