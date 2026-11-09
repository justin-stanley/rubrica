//! Placeholder width table.

pub const FONT_FAMILY: &str = "Inter";
pub const FONT_VERSION: &str = "";
pub const FONT_TTF_SHA256: &str = "";
pub const FONT_WOFF2_SHA256: &str = "";
pub const UNITS_PER_EM: u32 = 1000;
pub const ASCENT: u16 = 0;
pub const DESCENT: u16 = 0;
pub const CAP_HEIGHT: u16 = 0;
pub const AVERAGE: u16 = 0;
pub const MISSING: u16 = u16::MAX;

pub(crate) const RANGES: [(u32, u32, &[u16]); 8] = [
    (0x0020, 0x007E, &[]),
    (0x00A0, 0x00FF, &[]),
    (0x0100, 0x017F, &[]),
    (0x0180, 0x024F, &[]),
    (0x0370, 0x03FF, &[]),
    (0x0400, 0x04FF, &[]),
    (0x1E00, 0x1EFF, &[]),
    (0x2000, 0x206F, &[]),
];
