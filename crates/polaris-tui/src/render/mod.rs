//! Draws a configuration and its latest snapshot into a frame.

mod alerts;
mod grid;
mod stat;
mod status_bar;
mod table;
mod timeseries;

use polaris_core::config::Config;
use polaris_core::data::{Snapshot, WidgetData};
use polaris_core::source::SourceError;
use polaris_core::style::Level;
use polaris_core::widget::{Widget, WidgetKind};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, Paragraph, Wrap};

use crate::palette::{self, MUTED};

const WAITING_TEXT: &str = "Waiting for data...";
const MISMATCH_TEXT: &str = "Data does not match the widget type";
const STATUS_BAR_HEIGHT: u16 = 1;

/// Draws the whole dashboard; `snapshot` is `None` until the first fetch.
pub fn draw_dashboard(frame: &mut Frame, config: &Config, snapshot: Option<&Snapshot>) {
    let grid_area = if config.tui.show_status_bar {
        let [grid_area, status_area] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(STATUS_BAR_HEIGHT)])
                .areas(frame.area());
        status_bar::draw(frame, status_area, config, snapshot);
        grid_area
    } else {
        frame.area()
    };
    let cells = grid::Cells::split(grid_area, &config.layout);
    for (index, widget) in config.widgets.iter().enumerate() {
        let result = snapshot.and_then(|snapshot| snapshot.results.get(index));
        draw_widget(frame, cells.area_of(&widget.area), config, widget, result);
    }
}

fn draw_widget(
    frame: &mut Frame,
    area: Rect,
    config: &Config,
    widget: &Widget,
    result: Option<&Result<WidgetData, SourceError>>,
) {
    let block = widget_block(&widget.title);
    match (result, &widget.kind) {
        (None, _) => draw_message(frame, area, block, WAITING_TEXT, MUTED),
        (Some(Err(error)), _) => draw_message(
            frame,
            area,
            block,
            &error.to_string(),
            palette::level_color(Level::Critical),
        ),
        (Some(Ok(WidgetData::Stat(data))), WidgetKind::Stat(stat)) => {
            stat::draw(frame, area, block, stat, data)
        }
        (Some(Ok(WidgetData::Table(data))), WidgetKind::Table(table)) => {
            table::draw(frame, area, block, table, data)
        }
        (Some(Ok(WidgetData::Alerts(entries))), WidgetKind::Alerts(_)) => {
            alerts::draw(frame, area, block, entries)
        }
        (Some(Ok(WidgetData::Timeseries(series))), WidgetKind::Timeseries(timeseries)) => {
            timeseries::draw(
                frame,
                area,
                block,
                timeseries,
                series,
                config.tui.chart_marker,
            )
        }
        (Some(Ok(_)), _) => draw_message(frame, area, block, MISMATCH_TEXT, MUTED),
    }
}

fn widget_block(title: &str) -> Block<'_> {
    Block::bordered()
        .border_type(BorderType::Plain)
        .border_style(Style::new().fg(MUTED))
        .title(Line::from(format!(" {title} ")).bold())
}

fn draw_message(frame: &mut Frame, area: Rect, block: Block, message: &str, color: Color) {
    let paragraph = Paragraph::new(message.to_string())
        .style(Style::new().fg(color))
        .wrap(Wrap { trim: true })
        .block(block);
    frame.render_widget(paragraph, area);
}

#[cfg(test)]
mod tests {
    use polaris_core::data::{
        Alert, AlertEntry, NamedSeries, StatData, TableCell, TableData, TableRow,
    };
    use polaris_core::widget::TableWidget;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    const HOMELAB_CONFIG: &str = include_str!("../../../../examples/homelab.toml");
    const KUBERNETES_CONFIG: &str = include_str!("../../../../examples/kubernetes.toml");
    const HOUSE_CONFIG: &str = include_str!("../../../../examples/house.toml");
    /// 1280x400 pixels with an 8x16 console font.
    const SCREEN_COLUMNS: u16 = 160;
    const SCREEN_ROWS: u16 = 25;

    /// Fixed keys when configured, otherwise a few Springfield guests.
    fn fixture_table(table: &TableWidget) -> TableData {
        let keys = if table.keys.is_empty() {
            vec!["krusty-show".to_string(), "itchy-scratchy".to_string()]
        } else {
            table.keys.clone()
        };
        let fixture_values = [1.0, 0.42, 0.81, 64.0];
        let rows = keys
            .into_iter()
            .map(|key| TableRow {
                key,
                values: table
                    .columns
                    .iter()
                    .zip(fixture_values.iter().cycle())
                    .map(|(column, value)| match column.value_label {
                        Some(_) => Some(TableCell::Text("springfield".into())),
                        None => Some(TableCell::Number(*value)),
                    })
                    .collect(),
            })
            .collect();
        TableData { rows }
    }

    fn fixture_result(widget: &Widget) -> Result<WidgetData, SourceError> {
        Ok(match &widget.kind {
            WidgetKind::Stat(_) => WidgetData::Stat(StatData {
                value: Some(3.0),
                total: Some(5.0),
            }),
            WidgetKind::Table(table) => WidgetData::Table(fixture_table(table)),
            WidgetKind::Alerts(_) => WidgetData::Alerts(vec![AlertEntry {
                alert: Alert {
                    name: "Physical host unreachable".into(),
                    summary: Some("Host shelbyville is not responding".into()),
                    state: "Alerting".into(),
                    labels: Default::default(),
                    active_since: None,
                },
                level: Level::Critical,
            }]),
            WidgetKind::Timeseries(_) => WidgetData::Timeseries(
                ["springfield", "shelbyville"]
                    .iter()
                    .enumerate()
                    .map(|(index, name)| NamedSeries {
                        name: name.to_string(),
                        points: (0..60)
                            .map(|step| {
                                (
                                    f64::from(step) * 60.0,
                                    0.2 + 0.1 * index as f64 + 0.05 * (f64::from(step) / 5.0).sin(),
                                )
                            })
                            .collect(),
                    })
                    .collect(),
            ),
        })
    }

    /// Renders `config_text` with fixture data and returns the screen text.
    fn render_on_rack_screen(config_text: &str) -> String {
        let config = Config::parse(config_text).expect("example configuration is valid");
        let snapshot = Snapshot {
            fetched_at: jiff::Timestamp::now(),
            results: config.widgets.iter().map(fixture_result).collect(),
        };
        let mut terminal = Terminal::new(TestBackend::new(SCREEN_COLUMNS, SCREEN_ROWS)).unwrap();

        terminal
            .draw(|frame| draw_dashboard(frame, &config, Some(&snapshot)))
            .unwrap();

        let screen: String = terminal
            .backend()
            .buffer()
            .content()
            .chunks(usize::from(SCREEN_COLUMNS))
            .map(|line| line.iter().map(|cell| cell.symbol()).collect::<String>() + "\n")
            .collect();
        println!("{screen}");
        screen
    }

    fn assert_screen_contains(screen: &str, expected_texts: &[&str]) {
        for expected_text in expected_texts {
            assert!(
                screen.contains(expected_text),
                "missing \"{expected_text}\" in:\n{screen}"
            );
        }
    }

    #[test]
    fn renders_homelab_dashboard_on_rack_screen() {
        let screen = render_on_rack_screen(HOMELAB_CONFIG);

        assert_screen_contains(
            &screen,
            &[
                "Docker hosts",
                "skynet",
                "Proxmox hosts",
                "socrate",
                "Virtual machines",
                "itchy-scratchy",
                "Containers",
                "shelbyville is not responding",
                "updated",
            ],
        );
    }

    #[test]
    fn renders_kubernetes_dashboard_on_rack_screen() {
        let screen = render_on_rack_screen(KUBERNETES_CONFIG);

        assert_screen_contains(
            &screen,
            &["Talos nodes", "Namespaces", "Node CPU (3h)", "PVCs bound"],
        );
    }

    #[test]
    fn renders_house_dashboard_on_rack_screen() {
        let screen = render_on_rack_screen(HOUSE_CONFIG);

        assert_screen_contains(
            &screen,
            &[
                "skynet volumes",
                "Rez-de-chaussée",
                "Extérieur",
                "Wi-Fi clients",
            ],
        );
    }
}
