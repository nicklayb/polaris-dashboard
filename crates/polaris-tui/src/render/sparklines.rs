//! Time series drawn as one lane per series: name, sparkline and latest value.

use polaris_core::data::NamedSeries;
use polaris_core::resample::{bucket_averages, latest_value};
use polaris_core::widget::{PresentedValue, TimeseriesWidget};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Style, Stylize};
use ratatui::widgets::{Block, Paragraph, Sparkline, SparklineBar};

use super::timeseries::{time_bounds, value_bounds};
use crate::palette;

/// Integer scale sparkline values are mapped to.
const SPARKLINE_RESOLUTION: u64 = 1000;
/// Vertical steps drawn per cell by the sparkline glyphs (▁ to █).
const LEVELS_PER_ROW: u64 = 8;
const MAXIMUM_NAME_WIDTH: usize = 14;
const COLUMN_GAP: u16 = 1;

/// Draws every series in its own lane, sharing the same value scale so lanes
/// stay comparable. Series beyond the available rows are not drawn.
pub fn draw(
    frame: &mut Frame,
    area: Rect,
    block: Block,
    widget: &TimeseriesWidget,
    series: &[NamedSeries],
) {
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let lane_count = series.len().min(usize::from(inner.height));
    if lane_count == 0 {
        return;
    }
    let points: Vec<Vec<(f64, f64)>> = series.iter().map(|series| series.points.clone()).collect();
    let bounds = value_bounds(widget, &points);
    let window = time_bounds(widget, &points);
    let latest_values: Vec<PresentedValue> = series
        .iter()
        .map(|series| widget.display.present(latest_value(&series.points)))
        .collect();
    let name_width = column_width(
        series
            .iter()
            .map(|series| series.name.chars().count().min(MAXIMUM_NAME_WIDTH)),
    );
    let value_width = column_width(latest_values.iter().map(|value| value.text.chars().count()));
    let lanes = Layout::vertical(vec![Constraint::Fill(1); lane_count]).split(inner);
    for (index, lane) in lanes.iter().enumerate() {
        let [name_area, sparkline_area, value_area] = Layout::horizontal([
            Constraint::Length(name_width),
            Constraint::Fill(1),
            Constraint::Length(value_width),
        ])
        .areas(*lane);
        let color = palette::series_color(index);
        let bars = sparkline_bars(&series[index].points, window, bounds, sparkline_area);
        frame.render_widget(
            Sparkline::default()
                .data(bars)
                .max(SPARKLINE_RESOLUTION)
                .style(Style::new().fg(color)),
            sparkline_area,
        );
        frame.render_widget(
            Paragraph::new(series[index].name.clone()).fg(color),
            bottom_row(name_area),
        );
        let latest = &latest_values[index];
        let value = Paragraph::new(latest.text.clone())
            .fg(palette::level_color(latest.level))
            .bold()
            .alignment(Alignment::Right);
        frame.render_widget(value, bottom_row(value_area));
    }
}

fn column_width(widths: impl Iterator<Item = usize>) -> u16 {
    let width = widths.max().unwrap_or_default();
    u16::try_from(width)
        .unwrap_or(u16::MAX)
        .saturating_add(COLUMN_GAP)
}

/// Labels sit on the lane baseline, next to the bottom of the bars.
fn bottom_row(area: Rect) -> Rect {
    Rect {
        y: area.bottom().saturating_sub(1),
        height: area.height.min(1),
        ..area
    }
}

fn sparkline_bars(
    points: &[(f64, f64)],
    [start, end]: [f64; 2],
    bounds: [f64; 2],
    area: Rect,
) -> Vec<SparklineBar> {
    let smallest_visible = SPARKLINE_RESOLUTION / (LEVELS_PER_ROW * u64::from(area.height.max(1)));
    bucket_averages(points, start, end, usize::from(area.width))
        .into_iter()
        .map(|average| {
            SparklineBar::from(average.map(|value| scale(value, bounds).max(smallest_visible)))
        })
        .collect()
}

/// Maps `value` from `bounds` onto `0..=SPARKLINE_RESOLUTION`, clamped.
fn scale(value: f64, [minimum, maximum]: [f64; 2]) -> u64 {
    let span = maximum - minimum;
    if span <= 0.0 {
        return 0;
    }
    let ratio = ((value - minimum) / span).clamp(0.0, 1.0);
    (ratio * SPARKLINE_RESOLUTION as f64).round() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_maps_bounds_to_resolution() {
        assert_eq!(scale(50.0, [0.0, 100.0]), SPARKLINE_RESOLUTION / 2);
        assert_eq!(scale(150.0, [0.0, 100.0]), SPARKLINE_RESOLUTION);
        assert_eq!(scale(-1.0, [0.0, 100.0]), 0);
        assert_eq!(scale(7.0, [7.0, 7.0]), 0);
    }

    #[test]
    fn present_values_stay_visible_and_gaps_stay_empty() {
        let duff_sales = [(0.0, 0.0), (3.0, 100.0)];

        let bars = sparkline_bars(&duff_sales, [0.0, 4.0], [0.0, 100.0], Rect::new(0, 0, 4, 1));

        let expected: Vec<SparklineBar> = [
            Some(SPARKLINE_RESOLUTION / LEVELS_PER_ROW),
            None,
            None,
            Some(SPARKLINE_RESOLUTION),
        ]
        .into_iter()
        .map(SparklineBar::from)
        .collect();
        assert_eq!(bars, expected);
    }
}
