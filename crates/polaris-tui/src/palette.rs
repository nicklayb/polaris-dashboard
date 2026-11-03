//! Maps semantic levels to terminal colors.
//!
//! Only the 16 base colors are used so the dashboard renders identically on
//! the Linux framebuffer console.

use polaris_core::style::Level;
use ratatui::style::Color;

const SERIES_COLORS: [Color; 6] = [
    Color::Cyan,
    Color::Magenta,
    Color::Yellow,
    Color::Green,
    Color::Blue,
    Color::LightRed,
];

/// Text color for a level.
pub fn level_color(level: Level) -> Color {
    match level {
        Level::Neutral => Color::Reset,
        Level::Ok => Color::Green,
        Level::Warning => Color::Yellow,
        Level::Critical => Color::Red,
    }
}

/// Color of the chart series at `index`, cycling through the palette.
pub fn series_color(index: usize) -> Color {
    SERIES_COLORS[index % SERIES_COLORS.len()]
}

/// Color of borders and secondary text.
pub const MUTED: Color = Color::DarkGray;
