//! Single value tile.

use polaris_core::data::StatData;
use polaris_core::widget::StatWidget;
use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};

use super::big_text::{self, StyledText};
use crate::palette::{self, MUTED};

/// Separator between the value and the total in plain text.
const PLAIN_TOTAL_SEPARATOR: &str = " / ";
/// Separator before the small total next to large digits, kept tight so
/// two digit values with a total still fit narrow tiles.
const LARGE_TOTAL_SEPARATOR: &str = "/";
/// Blank cells between the large value and its small total.
const LARGE_TOTAL_GAP: &str = " ";

/// Texts and styles shown by a stat tile.
#[derive(Debug, Clone, PartialEq)]
pub struct StatTexts {
    /// Formatted value.
    pub value: String,
    /// Style of the value, colored by its threshold level.
    pub value_style: Style,
    /// Formatted total, when a total query is set.
    pub total: Option<String>,
}

impl StatTexts {
    /// Formats the value and optional total of `data` for `stat`.
    pub fn new(stat: &StatWidget, data: &StatData) -> Self {
        let presented = stat.display.present(data.value);
        Self {
            value: presented.text,
            value_style: Style::new()
                .fg(palette::level_color(presented.level))
                .bold(),
            total: stat
                .total_query
                .as_ref()
                .map(|_| stat.display.present(data.total).text),
        }
    }
}

/// Draws the value, followed by `/ total` when a total query is set. The
/// value uses large block digits when they fit, with the total kept small
/// on their bottom row; everything falls back to plain text otherwise.
pub fn draw(frame: &mut Frame, area: Rect, block: Block, stat: &StatWidget, data: &StatData) {
    let inner = block.inner(area);
    let lines = tile_lines(&StatTexts::new(stat, data), inner.width, inner.height);
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .block(block),
        area,
    );
}

/// Lines filling an inner area of `width` by `height` cells, vertically
/// centered: large text when it fits, the plain text line otherwise.
pub fn tile_lines(texts: &StatTexts, width: u16, height: u16) -> Vec<Line<'static>> {
    let content = large_lines(texts, width, height).unwrap_or_else(|| vec![plain_line(texts)]);
    let content_height = u16::try_from(content.len()).unwrap_or(u16::MAX);
    let padding = height.saturating_sub(content_height) / 2;
    let mut lines = vec![Line::default(); usize::from(padding)];
    lines.extend(content);
    lines
}

/// Large value with the small total appended to its bottom row, or `None`
/// when the value cannot be drawn large in the remaining width.
fn large_lines(texts: &StatTexts, width: u16, height: u16) -> Option<Vec<Line<'static>>> {
    let suffix = large_total_suffix(texts);
    let suffix_width = u16::try_from(suffix.chars().count()).ok()?;
    let value_width = width.checked_sub(suffix_width)?;
    let value_segments = [StyledText {
        text: &texts.value,
        style: texts.value_style,
    }];
    let mut lines = big_text::render_lines(&value_segments, value_width, height)?;
    attach_suffix(&mut lines, suffix);
    Some(lines)
}

fn large_total_suffix(texts: &StatTexts) -> String {
    texts
        .total
        .as_ref()
        .map(|total| format!("{LARGE_TOTAL_GAP}{LARGE_TOTAL_SEPARATOR}{total}"))
        .unwrap_or_default()
}

/// Appends `suffix` to the last line and pads the others with blanks of the
/// same width, so centering keeps the glyph rows aligned.
fn attach_suffix(lines: &mut [Line<'static>], suffix: String) {
    if suffix.is_empty() {
        return;
    }
    let blank = " ".repeat(suffix.chars().count());
    let last_index = lines.len().saturating_sub(1);
    for (index, line) in lines.iter_mut().enumerate() {
        let span = if index == last_index {
            Span::from(suffix.clone()).fg(MUTED)
        } else {
            Span::from(blank.clone())
        };
        line.spans.push(span);
    }
}

fn plain_line(texts: &StatTexts) -> Line<'static> {
    let mut spans = vec![Span::styled(texts.value.clone(), texts.value_style)];
    if let Some(total) = &texts.total {
        spans.push(Span::from(format!("{PLAIN_TOTAL_SEPARATOR}{total}")).fg(MUTED));
    }
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use ratatui::style::Color;

    use super::*;

    const HOMER_STYLE: Style = Style::new().fg(Color::Green);
    /// Interior of a stat tile on the homelab rack screen.
    const RACK_TILE_WIDTH: u16 = 14;
    const RACK_TILE_HEIGHT: u16 = 3;

    fn donuts(value: &str, total: Option<&str>) -> StatTexts {
        StatTexts {
            value: value.into(),
            value_style: HOMER_STYLE,
            total: total.map(String::from),
        }
    }

    fn line_texts(lines: &[Line]) -> Vec<String> {
        lines.iter().map(|line| line.to_string()).collect()
    }

    fn rack_tile(value: &str, total: Option<&str>) -> Vec<String> {
        line_texts(&tile_lines(
            &donuts(value, total),
            RACK_TILE_WIDTH,
            RACK_TILE_HEIGHT,
        ))
    }

    #[test]
    fn tile_lines_draws_total_small_on_the_bottom_row() {
        let lines = tile_lines(&donuts("3", Some("5")), RACK_TILE_WIDTH, RACK_TILE_HEIGHT);

        assert_eq!(line_texts(&lines), ["▀▀█   ", "▀▀█   ", "▀▀▀ /5"]);
        assert_eq!(lines[2].spans[0].style, HOMER_STYLE);
        assert_eq!(lines[2].spans[1].style, Style::new().fg(MUTED));
    }

    #[test]
    fn tile_lines_fits_typical_values_on_rack_tiles() {
        let springfield_stats = [
            ("30", None),
            ("22", Some("30")),
            ("22", Some("22")),
            ("100%", None),
            ("5", Some("31")),
        ];
        for (value, total) in springfield_stats {
            let lines = rack_tile(value, total);
            assert_eq!(lines.len(), 3, "{value} fell back to plain text");
        }
    }

    #[test]
    fn tile_lines_centers_large_text_vertically() {
        let lines = tile_lines(&donuts("3", None), 6, 9);

        assert_eq!(line_texts(&lines)[..2], ["", ""]);
        assert_eq!(lines.len(), 7);
    }

    #[test]
    fn tile_lines_caps_the_digit_size() {
        let lines = tile_lines(&donuts("3", None), 80, 40);
        let content_height = lines.iter().filter(|line| line.width() > 0).count();

        assert_eq!(
            content_height,
            usize::from(big_text::rendered_height(big_text::MAXIMUM_SCALE))
        );
    }

    #[test]
    fn tile_lines_falls_back_to_plain_text_when_too_small() {
        let lines = tile_lines(&donuts("3", Some("5")), 5, 3);

        assert_eq!(line_texts(&lines), ["", "3 / 5"]);
    }

    #[test]
    fn tile_lines_falls_back_to_plain_text_for_mapped_labels() {
        assert_eq!(
            line_texts(&tile_lines(&donuts("Homer asleep", None), 40, 10)),
            ["", "", "", "", "Homer asleep"]
        );
    }
}
