//! List of alerts, most severe first.

use jiff::Timestamp;
use polaris_core::data::AlertEntry;
use polaris_core::style::Level;
use polaris_core::unit::{Unit, format_value};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, Paragraph};

use crate::palette::{self, MUTED};

const ALL_CLEAR_TEXT: &str = "All clear";
/// A glyph present in the CP437 set, hence in every Linux console font.
const BULLET: &str = "■ ";

/// Draws one line per alert: level bullet, summary and age.
pub fn draw(frame: &mut Frame, area: Rect, block: Block, entries: &[AlertEntry]) {
    if entries.is_empty() {
        let all_clear = Paragraph::new(ALL_CLEAR_TEXT)
            .fg(palette::level_color(Level::Ok))
            .block(block);
        frame.render_widget(all_clear, area);
        return;
    }
    let now = Timestamp::now();
    let lines = entries.iter().map(|entry| alert_line(entry, now));
    frame.render_widget(List::new(lines).block(block), area);
}

fn alert_line(entry: &AlertEntry, now: Timestamp) -> Line<'static> {
    let color = palette::level_color(entry.level);
    let text = entry
        .alert
        .summary
        .clone()
        .unwrap_or_else(|| entry.alert.name.clone());
    let mut spans = vec![Span::from(BULLET).fg(color), Span::from(text)];
    if let Some(active_since) = entry.alert.active_since {
        let age_seconds = now.duration_since(active_since).as_secs_f64();
        spans.push(
            Span::from(format!(" {}", format_value(age_seconds, Unit::Seconds, 0))).fg(MUTED),
        );
    }
    Line::from(spans)
}
