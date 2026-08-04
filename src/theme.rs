//! Color palette (Tokyo Night inspired) and shared style helpers.

use ratatui::style::{Color, Modifier, Style};

use crate::model::Method;

pub const FG: Color = Color::Rgb(0xC0, 0xCA, 0xF5);
pub const PALE: Color = Color::Rgb(0x9A, 0xA5, 0xCE);
pub const DIM: Color = Color::Rgb(0x56, 0x5F, 0x89);
pub const BORDER: Color = Color::Rgb(0x3B, 0x42, 0x61);
pub const SEL_BG: Color = Color::Rgb(0x28, 0x34, 0x57);
pub const CHIP_FG: Color = Color::Rgb(0x16, 0x16, 0x1E);

pub const ACCENT: Color = Color::Rgb(0x7A, 0xA2, 0xF7);
pub const PURPLE: Color = Color::Rgb(0xBB, 0x9A, 0xF7);
pub const GREEN: Color = Color::Rgb(0x9E, 0xCE, 0x6A);
pub const RED: Color = Color::Rgb(0xF7, 0x76, 0x8E);
pub const YELLOW: Color = Color::Rgb(0xE0, 0xAF, 0x68);
pub const ORANGE: Color = Color::Rgb(0xFF, 0x9E, 0x64);
pub const CYAN: Color = Color::Rgb(0x7D, 0xCF, 0xFF);
pub const TEAL: Color = Color::Rgb(0x73, 0xDA, 0xCA);

pub fn method_color(m: Method) -> Color {
    match m {
        Method::Get => GREEN,
        Method::Post => ACCENT,
        Method::Put => YELLOW,
        Method::Patch => PURPLE,
        Method::Delete => RED,
        Method::Head => CYAN,
        Method::Options => DIM,
    }
}

pub fn status_color(code: u16) -> Color {
    match code {
        100..=199 => CYAN,
        200..=299 => GREEN,
        300..=399 => YELLOW,
        400..=499 => ORANGE,
        _ => RED,
    }
}

pub fn border(focused: bool) -> Style {
    if focused {
        Style::new().fg(ACCENT)
    } else {
        Style::new().fg(BORDER)
    }
}

pub fn title(focused: bool) -> Style {
    if focused {
        Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(DIM)
    }
}

pub fn key_hint() -> Style {
    Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)
}

pub fn hint_text() -> Style {
    Style::new().fg(DIM)
}
