//! Line chart of the configured range.

use polaris_core::config::ChartMarker;
use polaris_core::data::NamedSeries;
use polaris_core::unit::{Unit, format_value};
use polaris_core::widget::TimeseriesWidget;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Style, Stylize};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Axis, Block, Chart, Dataset, GraphType};

use crate::palette::{self, MUTED};

/// Fraction of the data span added above and below when bounds are automatic.
const AUTOMATIC_BOUND_MARGIN: f64 = 0.05;
/// Span used when every point has the same value.
const FLAT_SERIES_SPAN: f64 = 1.0;
const LEGEND_BULLET: &str = " ■ ";

/// Draws every series with its legend, x axis from `-range` to `now`.
pub fn draw(
    frame: &mut Frame,
    area: Rect,
    block: Block,
    widget: &TimeseriesWidget,
    series: &[NamedSeries],
    marker: ChartMarker,
) {
    let finite_points: Vec<Vec<(f64, f64)>> = series
        .iter()
        .map(|series| {
            series
                .points
                .iter()
                .copied()
                .filter(|(_, value)| value.is_finite())
                .collect()
        })
        .collect();
    let datasets = series
        .iter()
        .zip(&finite_points)
        .enumerate()
        .map(|(index, (series, points))| {
            Dataset::default()
                .name(series.name.clone())
                .marker(terminal_marker(marker))
                .graph_type(GraphType::Line)
                .style(Style::new().fg(palette::series_color(index)))
                .data(points)
        })
        .collect();
    let [minimum, maximum] = value_bounds(widget, &finite_points);
    let [start, end] = time_bounds(widget, &finite_points);
    let y_axis = Axis::default()
        .bounds([minimum, maximum])
        .labels([
            widget.display.present(Some(minimum)).text,
            widget.display.present(Some(maximum)).text,
        ])
        .style(Style::new().fg(MUTED));
    let x_axis = Axis::default()
        .bounds([start, end])
        .labels([
            format!(
                "-{}",
                format_value(widget.range.as_secs_f64(), Unit::Seconds, 0)
            ),
            "now".to_string(),
        ])
        .style(Style::new().fg(MUTED));
    let chart = Chart::new(datasets)
        .block(block.title_bottom(legend_line(series)))
        .x_axis(x_axis)
        .y_axis(y_axis)
        .legend_position(None);
    frame.render_widget(chart, area);
}

/// Legend drawn in the bottom border, so the plot keeps its full area.
fn legend_line(series: &[NamedSeries]) -> Line<'static> {
    let spans = series.iter().enumerate().flat_map(|(index, series)| {
        [
            Span::from(LEGEND_BULLET).fg(palette::series_color(index)),
            Span::from(format!("{} ", series.name)),
        ]
    });
    Line::from(spans.collect::<Vec<_>>())
}

fn terminal_marker(marker: ChartMarker) -> Marker {
    match marker {
        ChartMarker::HalfBlock => Marker::HalfBlock,
        ChartMarker::Block => Marker::Block,
        ChartMarker::Dot => Marker::Dot,
        ChartMarker::Braille => Marker::Braille,
    }
}

pub(super) fn value_bounds(widget: &TimeseriesWidget, points: &[Vec<(f64, f64)>]) -> [f64; 2] {
    let values = || points.iter().flatten().map(|(_, value)| *value);
    let data_minimum = values().fold(f64::INFINITY, f64::min);
    let data_maximum = values().fold(f64::NEG_INFINITY, f64::max);
    if !data_minimum.is_finite() {
        return [
            widget.minimum.unwrap_or(0.0),
            widget.maximum.unwrap_or(FLAT_SERIES_SPAN),
        ];
    }
    let span = if data_maximum > data_minimum {
        data_maximum - data_minimum
    } else {
        FLAT_SERIES_SPAN
    };
    let margin = span * AUTOMATIC_BOUND_MARGIN;
    [
        widget.minimum.unwrap_or(data_minimum - margin),
        widget.maximum.unwrap_or(data_maximum + margin),
    ]
}

/// Ends at the latest point and spans the configured range, so that gaps at
/// the start of the window stay visible.
pub(super) fn time_bounds(widget: &TimeseriesWidget, points: &[Vec<(f64, f64)>]) -> [f64; 2] {
    let end = points
        .iter()
        .flatten()
        .map(|(time, _)| *time)
        .fold(f64::NEG_INFINITY, f64::max);
    let range_seconds = widget.range.as_secs_f64();
    if end.is_finite() {
        [end - range_seconds, end]
    } else {
        [0.0, range_seconds]
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use polaris_core::widget::{ChartStyle, ValueDisplay};

    use super::*;

    fn kwik_e_mart_widget(minimum: Option<f64>) -> TimeseriesWidget {
        TimeseriesWidget {
            queries: vec![],
            range: Duration::from_secs(600),
            points: 10,
            minimum,
            maximum: None,
            style: ChartStyle::Lines,
            display: ValueDisplay::default(),
        }
    }

    #[test]
    fn value_bounds_add_margin_around_data() {
        let points = vec![vec![(0.0, 10.0), (1.0, 30.0)]];

        assert_eq!(
            value_bounds(&kwik_e_mart_widget(None), &points),
            [9.0, 31.0]
        );
        assert_eq!(
            value_bounds(&kwik_e_mart_widget(Some(0.0)), &points),
            [0.0, 31.0]
        );
    }

    #[test]
    fn value_bounds_widen_flat_series() {
        let [minimum, maximum] = value_bounds(&kwik_e_mart_widget(None), &[vec![(0.0, 5.0)]]);

        assert!(minimum < 5.0 && maximum > 5.0);
    }

    #[test]
    fn time_bounds_span_the_configured_range() {
        assert_eq!(
            time_bounds(
                &kwik_e_mart_widget(None),
                &[vec![(1000.0, 1.0), (1500.0, 2.0)]]
            ),
            [900.0, 1500.0]
        );
    }
}
