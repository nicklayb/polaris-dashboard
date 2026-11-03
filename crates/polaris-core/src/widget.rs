//! Widget definitions as declared in the configuration file.

use std::time::Duration;

use serde::Deserialize;

use crate::data::TableCell;
use crate::style::{Level, Threshold, ValueMapping, level_for, mapping_for};
use crate::unit::{Unit, format_value};

const DEFAULT_DECIMALS: usize = 1;
const DEFAULT_TIMESERIES_POINTS: u32 = 120;
const DEFAULT_TIMESERIES_RANGE: Duration = Duration::from_secs(3600);
const DEFAULT_SEVERITY_LABEL: &str = "severity";
const GRAFANA_FIRING_STATE: &str = "Alerting";
const MISSING_VALUE_TEXT: &str = "-";

/// A widget: a title, a place in the grid and a kind-specific body.
#[derive(Debug, Clone, Deserialize)]
pub struct Widget {
    pub title: String,
    pub area: Area,
    #[serde(flatten)]
    pub kind: WidgetKind,
}

/// Position of a widget in the layout grid, in grid cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct Area {
    pub column: usize,
    pub row: usize,
    #[serde(default = "default_span")]
    pub column_span: usize,
    #[serde(default = "default_span")]
    pub row_span: usize,
}

/// The kind of a widget, selected by the `type` key.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WidgetKind {
    Stat(StatWidget),
    Table(TableWidget),
    Alerts(AlertsWidget),
    Timeseries(TimeseriesWidget),
}

/// A single value, optionally displayed against a total (`3 / 5`).
#[derive(Debug, Clone, Deserialize)]
pub struct StatWidget {
    pub query: String,
    pub total_query: Option<String>,
    #[serde(flatten)]
    pub display: ValueDisplay,
}

/// A table whose rows are label values and whose columns are queries.
#[derive(Debug, Clone, Deserialize)]
pub struct TableWidget {
    /// Label identifying a row in every column query result.
    pub key_label: String,
    /// Header of the key column.
    pub key_header: String,
    /// Fixed rows, in order. Rows missing from every query are still shown,
    /// which makes a dead node visible instead of silently disappearing.
    #[serde(default)]
    pub keys: Vec<String>,
    /// Header of the column rows are sorted by, then by key. Ignored when
    /// `keys` fixes the order.
    pub sort_by: Option<String>,
    pub columns: Vec<TableColumn>,
}

/// A table column backed by one instant query.
#[derive(Debug, Clone, Deserialize)]
pub struct TableColumn {
    pub header: String,
    pub query: String,
    /// Shows the value of this label instead of the sample value.
    pub value_label: Option<String>,
    #[serde(flatten)]
    pub display: ValueDisplay,
}

/// The list of Grafana-managed alerts in the selected states.
#[derive(Debug, Clone, Deserialize)]
pub struct AlertsWidget {
    #[serde(default = "default_alert_states")]
    pub states: Vec<String>,
    /// Label whose value (`warning`, `critical`, ...) gives the alert level.
    #[serde(default = "default_severity_label")]
    pub severity_label: String,
    /// Level used when the severity label is missing or unknown.
    #[serde(default = "default_alert_level")]
    pub default_level: Level,
}

/// A line chart over a recent time range.
#[derive(Debug, Clone, Deserialize)]
pub struct TimeseriesWidget {
    pub queries: Vec<TimeseriesQuery>,
    #[serde(default = "default_timeseries_range", with = "humantime_serde")]
    pub range: Duration,
    /// Number of points requested per series; drives the query step.
    #[serde(default = "default_timeseries_points")]
    pub points: u32,
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
    #[serde(flatten)]
    pub display: ValueDisplay,
}

/// A range query and how to name each resulting series.
#[derive(Debug, Clone, Deserialize)]
pub struct TimeseriesQuery {
    pub query: String,
    /// Series name template, `{{label}}` placeholders are replaced.
    #[serde(default)]
    pub legend: String,
}

/// How a numeric value is formatted and colored.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ValueDisplay {
    #[serde(default)]
    pub unit: Unit,
    #[serde(default = "default_decimals")]
    pub decimals: usize,
    #[serde(default)]
    pub thresholds: Vec<Threshold>,
    #[serde(default)]
    pub mappings: Vec<ValueMapping>,
}

/// A value ready to be drawn: its text and its level.
#[derive(Debug, Clone, PartialEq)]
pub struct PresentedValue {
    pub text: String,
    pub level: Level,
}

impl TableColumn {
    /// Presents a table cell: label values are shown as is and stay
    /// neutral, numbers go through the column display rules.
    pub fn present(&self, cell: Option<&TableCell>) -> PresentedValue {
        match cell {
            Some(TableCell::Text(text)) => PresentedValue {
                text: text.clone(),
                level: Level::Neutral,
            },
            Some(TableCell::Number(value)) => self.display.present(Some(*value)),
            None => self.display.present(None),
        }
    }
}

impl ValueDisplay {
    /// Formats `value` and resolves its level, mappings taking precedence
    /// over thresholds. A missing value is shown as `-` and stays neutral.
    pub fn present(&self, value: Option<f64>) -> PresentedValue {
        let Some(value) = value else {
            return PresentedValue {
                text: MISSING_VALUE_TEXT.to_string(),
                level: Level::Neutral,
            };
        };
        match mapping_for(value, &self.mappings) {
            Some(mapping) => PresentedValue {
                text: mapping.text.clone(),
                level: mapping.level,
            },
            None => PresentedValue {
                text: format_value(value, self.unit, self.decimals),
                level: level_for(value, &self.thresholds),
            },
        }
    }
}

fn default_span() -> usize {
    1
}

fn default_decimals() -> usize {
    DEFAULT_DECIMALS
}

fn default_alert_states() -> Vec<String> {
    vec![GRAFANA_FIRING_STATE.to_string()]
}

fn default_severity_label() -> String {
    DEFAULT_SEVERITY_LABEL.to_string()
}

fn default_alert_level() -> Level {
    Level::Warning
}

fn default_timeseries_range() -> Duration {
    DEFAULT_TIMESERIES_RANGE
}

fn default_timeseries_points() -> u32 {
    DEFAULT_TIMESERIES_POINTS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reactor_display() -> ValueDisplay {
        ValueDisplay {
            unit: Unit::Celsius,
            decimals: 0,
            thresholds: vec![
                Threshold {
                    value: 0.0,
                    level: Level::Ok,
                },
                Threshold {
                    value: 80.0,
                    level: Level::Critical,
                },
            ],
            mappings: vec![ValueMapping {
                value: -1.0,
                text: "Homer asleep".into(),
                level: Level::Warning,
            }],
        }
    }

    #[test]
    fn present_formats_and_applies_thresholds() {
        assert_eq!(
            reactor_display().present(Some(91.0)),
            PresentedValue {
                text: "91°C".into(),
                level: Level::Critical
            }
        );
    }

    #[test]
    fn present_prefers_mappings() {
        assert_eq!(
            reactor_display().present(Some(-1.0)),
            PresentedValue {
                text: "Homer asleep".into(),
                level: Level::Warning
            }
        );
    }

    #[test]
    fn present_marks_missing_values() {
        assert_eq!(
            reactor_display().present(None),
            PresentedValue {
                text: "-".into(),
                level: Level::Neutral
            }
        );
    }
}
