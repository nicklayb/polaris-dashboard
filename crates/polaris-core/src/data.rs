//! Data produced by fetching widgets, consumed by renderers.

use std::collections::BTreeMap;

use jiff::Timestamp;

use crate::source::SourceError;
use crate::style::Level;

/// Labels attached to a Prometheus series.
pub type Labels = BTreeMap<String, String>;

/// One value of an instant query.
#[derive(Debug, Clone, PartialEq)]
pub struct Sample {
    pub labels: Labels,
    pub value: f64,
}

/// One series of a range query; points are `(unix seconds, value)`.
#[derive(Debug, Clone, PartialEq)]
pub struct Series {
    pub labels: Labels,
    pub points: Vec<(f64, f64)>,
}

/// An alert instance reported by Grafana.
#[derive(Debug, Clone, PartialEq)]
pub struct Alert {
    pub name: String,
    pub summary: Option<String>,
    pub state: String,
    pub labels: Labels,
    pub active_since: Option<Timestamp>,
}

/// Resolved data of one widget, by widget kind.
#[derive(Debug, Clone, PartialEq)]
pub enum WidgetData {
    Stat(StatData),
    Table(TableData),
    Alerts(Vec<AlertEntry>),
    Timeseries(Vec<NamedSeries>),
}

/// Value of a stat widget and its optional total.
#[derive(Debug, Clone, PartialEq)]
pub struct StatData {
    pub value: Option<f64>,
    pub total: Option<f64>,
}

/// Rows of a table widget, values ordered like the configured columns.
#[derive(Debug, Clone, PartialEq)]
pub struct TableData {
    pub rows: Vec<TableRow>,
}

/// A table row: its key and one optional value per column.
#[derive(Debug, Clone, PartialEq)]
pub struct TableRow {
    pub key: String,
    pub values: Vec<Option<TableCell>>,
}

/// Content of a table cell: a metric value or a label value.
#[derive(Debug, Clone, PartialEq)]
pub enum TableCell {
    Number(f64),
    Text(String),
}

/// An alert ready to display, with its resolved level.
#[derive(Debug, Clone, PartialEq)]
pub struct AlertEntry {
    pub alert: Alert,
    pub level: Level,
}

/// A chart series with its rendered legend name.
#[derive(Debug, Clone, PartialEq)]
pub struct NamedSeries {
    pub name: String,
    pub points: Vec<(f64, f64)>,
}

/// Every widget result of one refresh, indexed like the configured widgets.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub fetched_at: Timestamp,
    pub results: Vec<Result<WidgetData, SourceError>>,
}

impl Snapshot {
    /// Counts widgets whose fetch failed.
    pub fn error_count(&self) -> usize {
        self.results.iter().filter(|result| result.is_err()).count()
    }
}
