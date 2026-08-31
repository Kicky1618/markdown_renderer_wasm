#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub css: &'static str,
    pub rgba: [f32; 4],
}

const fn rgb(css: &'static str, r: u8, g: u8, b: u8) -> Color {
    rgba(css, r, g, b, 1.0)
}

const fn rgba(css: &'static str, r: u8, g: u8, b: u8, alpha: f32) -> Color {
    Color {
        css,
        rgba: [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, alpha],
    }
}

pub const BG: Color = rgb("#090c12", 0x09, 0x0c, 0x12);
pub const PANEL: Color = rgb("#0e1a25", 0x0e, 0x1a, 0x25);
pub const FG: Color = rgb("#d1dbe8", 0xd1, 0xdb, 0xe8);
pub const MUTED: Color = rgb("#78879e", 0x78, 0x87, 0x9e);
pub const CYAN: Color = rgb("#40dcdf", 0x40, 0xdc, 0xdf);
pub const GREEN: Color = rgb("#73e09e", 0x73, 0xe0, 0x9e);
pub const ORANGE: Color = rgb("#f2ab61", 0xf2, 0xab, 0x61);
pub const PURPLE: Color = rgb("#c79ef2", 0xc7, 0x9e, 0xf2);
pub const BLUE: Color = rgb("#73b8fa", 0x73, 0xb8, 0xfa);
pub const YELLOW: Color = rgb("#ead67a", 0xea, 0xd6, 0x7a);
pub const OPERATOR: Color = rgb("#adbccc", 0xad, 0xbc, 0xcc);
pub const SELECTION: Color = rgba("#347ac7", 0x34, 0x7a, 0xc7, 0.38);
pub const SEARCH_ACTIVE: Color = rgba("#fa8c33", 0xfa, 0x8c, 0x33, 0.72);
pub const SEARCH_INACTIVE: Color = rgba("#ead33c", 0xea, 0xd3, 0x3c, 0.38);
pub const SCROLLBAR_THUMB: Color = rgb("#24454e", 0x24, 0x45, 0x4e);
pub const THEMATIC_BREAK: Color = rgb("#4c5868", 0x4c, 0x58, 0x68);
pub const TABLE_HEADER: Color = rgb("#1a3040", 0x1a, 0x30, 0x40);
pub const TABLE_STRIPE: Color = rgb("#0e161f", 0x0e, 0x16, 0x1f);
