//! Single value tile.

use polaris_core::data::StatData;
use polaris_core::widget::StatWidget;
use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::Stylize;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};

use crate::palette::{self, MUTED};

/// Draws the value, followed by ` / total` when a total query is set.
pub fn draw(frame: &mut Frame, area: Rect, block: Block, stat: &StatWidget, data: &StatData) {
    let presented = stat.display.present(data.value);
    let mut spans = vec![
        Span::from(presented.text)
            .fg(palette::level_color(presented.level))
            .bold(),
    ];
    if stat.total_query.is_some() {
        spans.push(Span::from(format!(" / {}", stat.display.present(data.total).text)).fg(MUTED));
    }
    let inner_height = block.inner(area).height;
    let mut lines = vec![Line::default(); usize::from(inner_height.saturating_sub(1) / 2)];
    lines.push(Line::from(spans));
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .block(block),
        area,
    );
}
