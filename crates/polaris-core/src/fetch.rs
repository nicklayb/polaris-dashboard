//! Turns widget definitions into [`WidgetData`] using a [`DataSource`].

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use jiff::{SignedDuration, Timestamp};

use crate::data::{
    AlertEntry, Labels, NamedSeries, Snapshot, StatData, TableCell, TableData, TableRow, WidgetData,
};
use crate::source::{DataSource, SourceError, TimeRange};
use crate::style::Level;
use crate::widget::{
    AlertsWidget, StatWidget, TableColumn, TableWidget, TimeseriesWidget, Widget, WidgetKind,
};

const MINIMUM_STEP_SECONDS: f64 = 1.0;

/// Fetches every widget, evaluated at `now`.
pub fn fetch_snapshot(widgets: &[Widget], source: &dyn DataSource, now: Timestamp) -> Snapshot {
    Snapshot {
        fetched_at: now,
        results: widgets
            .iter()
            .map(|widget| fetch_widget(widget, source, now))
            .collect(),
    }
}

/// Fetches the data of a single widget, evaluated at `now`.
pub fn fetch_widget(
    widget: &Widget,
    source: &dyn DataSource,
    now: Timestamp,
) -> Result<WidgetData, SourceError> {
    match &widget.kind {
        WidgetKind::Stat(stat) => fetch_stat(stat, source).map(WidgetData::Stat),
        WidgetKind::Table(table) => fetch_table(table, source).map(WidgetData::Table),
        WidgetKind::Alerts(alerts) => fetch_alerts(alerts, source).map(WidgetData::Alerts),
        WidgetKind::Timeseries(timeseries) => {
            fetch_timeseries(timeseries, source, now).map(WidgetData::Timeseries)
        }
    }
}

fn fetch_stat(stat: &StatWidget, source: &dyn DataSource) -> Result<StatData, SourceError> {
    let value = first_value(source, &stat.query)?;
    let total = stat
        .total_query
        .as_deref()
        .map(|query| first_value(source, query))
        .transpose()?
        .flatten();
    Ok(StatData { value, total })
}

fn first_value(source: &dyn DataSource, query: &str) -> Result<Option<f64>, SourceError> {
    Ok(source
        .instant_query(query)?
        .first()
        .map(|sample| sample.value))
}

fn fetch_table(table: &TableWidget, source: &dyn DataSource) -> Result<TableData, SourceError> {
    let columns = table
        .columns
        .iter()
        .map(|column| cells_by_key(source, column, &table.key_label))
        .collect::<Result<Vec<_>, _>>()?;
    let keys = if table.keys.is_empty() {
        discovered_keys(&columns)
    } else {
        table.keys.clone()
    };
    let mut rows: Vec<TableRow> = keys
        .into_iter()
        .map(|key| TableRow {
            values: columns
                .iter()
                .map(|cells| cells.get(&key).cloned())
                .collect(),
            key,
        })
        .collect();
    if table.keys.is_empty() {
        sort_rows(&mut rows, table);
    }
    Ok(TableData { rows })
}

fn cells_by_key(
    source: &dyn DataSource,
    column: &TableColumn,
    key_label: &str,
) -> Result<BTreeMap<String, TableCell>, SourceError> {
    Ok(source
        .instant_query(&column.query)?
        .into_iter()
        .filter_map(|sample| {
            let key = sample.labels.get(key_label)?.clone();
            let cell = match &column.value_label {
                Some(value_label) => TableCell::Text(sample.labels.get(value_label)?.clone()),
                None => TableCell::Number(sample.value),
            };
            Some((key, cell))
        })
        .collect())
}

fn discovered_keys(columns: &[BTreeMap<String, TableCell>]) -> Vec<String> {
    columns
        .iter()
        .flat_map(|cells| cells.keys().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Sorts by the `sort_by` column (missing cells last), then by key.
fn sort_rows(rows: &mut [TableRow], table: &TableWidget) {
    let sort_index = table.sort_by.as_ref().and_then(|header| {
        table
            .columns
            .iter()
            .position(|column| &column.header == header)
    });
    let Some(sort_index) = sort_index else {
        return;
    };
    rows.sort_by(|left_row, right_row| {
        compare_cells(
            left_row.values[sort_index].as_ref(),
            right_row.values[sort_index].as_ref(),
        )
        .then_with(|| left_row.key.cmp(&right_row.key))
    });
}

fn compare_cells(left_cell: Option<&TableCell>, right_cell: Option<&TableCell>) -> Ordering {
    match (left_cell, right_cell) {
        (Some(TableCell::Number(left_number)), Some(TableCell::Number(right_number))) => {
            left_number.total_cmp(right_number)
        }
        (Some(TableCell::Text(left_text)), Some(TableCell::Text(right_text))) => {
            left_text.cmp(right_text)
        }
        (Some(TableCell::Number(_)), Some(TableCell::Text(_))) => Ordering::Less,
        (Some(TableCell::Text(_)), Some(TableCell::Number(_))) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn fetch_alerts(
    widget: &AlertsWidget,
    source: &dyn DataSource,
) -> Result<Vec<AlertEntry>, SourceError> {
    let mut entries: Vec<AlertEntry> = source
        .alerts()?
        .into_iter()
        .filter(|alert| widget.states.contains(&alert.state))
        .map(|alert| AlertEntry {
            level: alert_level(&alert.labels, widget),
            alert,
        })
        .collect();
    entries.sort_by(|left_entry, right_entry| {
        right_entry.level.cmp(&left_entry.level).then(
            left_entry
                .alert
                .active_since
                .cmp(&right_entry.alert.active_since),
        )
    });
    Ok(entries)
}

fn alert_level(labels: &Labels, widget: &AlertsWidget) -> Level {
    match labels.get(&widget.severity_label).map(String::as_str) {
        Some("critical") => Level::Critical,
        Some("warning") => Level::Warning,
        Some("info") => Level::Neutral,
        _ => widget.default_level,
    }
}

fn fetch_timeseries(
    widget: &TimeseriesWidget,
    source: &dyn DataSource,
    now: Timestamp,
) -> Result<Vec<NamedSeries>, SourceError> {
    let range = time_range(widget, now)?;
    let mut named_series = Vec::new();
    for query in &widget.queries {
        for series in source.range_query(&query.query, range)? {
            named_series.push(NamedSeries {
                name: render_legend(&query.legend, &series.labels),
                points: series.points,
            });
        }
    }
    Ok(named_series)
}

fn time_range(widget: &TimeseriesWidget, now: Timestamp) -> Result<TimeRange, SourceError> {
    let duration = SignedDuration::try_from(widget.range)
        .map_err(|error| SourceError::Request(error.to_string()))?;
    let start = now
        .checked_sub(duration)
        .map_err(|error| SourceError::Request(error.to_string()))?;
    let step_seconds =
        (widget.range.as_secs_f64() / f64::from(widget.points.max(1))).max(MINIMUM_STEP_SECONDS);
    Ok(TimeRange {
        start,
        end: now,
        step_seconds,
    })
}

/// Replaces `{{label}}` placeholders in `template` with label values.
///
/// An empty template falls back to every label joined by commas.
pub fn render_legend(template: &str, labels: &Labels) -> String {
    if template.is_empty() {
        return labels
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join(",");
    }
    labels
        .iter()
        .fold(template.to_string(), |legend, (name, value)| {
            legend.replace(&format!("{{{{{name}}}}}"), value)
        })
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::time::Duration;

    use super::*;
    use crate::data::{Alert, Sample, Series};
    use crate::widget::{Area, ChartStyle, TableColumn, TimeseriesQuery, ValueDisplay};

    const EVALUATION_TIME: &str = "2026-11-03T12:00:00Z";

    /// In-memory source answering queries from fixtures and recording ranges.
    #[derive(Default)]
    struct FakeSource {
        samples_by_query: BTreeMap<String, Vec<Sample>>,
        series_by_query: BTreeMap<String, Vec<Series>>,
        alerts: Vec<Alert>,
        requested_ranges: RefCell<Vec<TimeRange>>,
    }

    impl DataSource for FakeSource {
        fn instant_query(&self, query: &str) -> Result<Vec<Sample>, SourceError> {
            self.samples_by_query
                .get(query)
                .cloned()
                .ok_or_else(|| SourceError::Request(format!("unknown query {query}")))
        }

        fn range_query(&self, query: &str, range: TimeRange) -> Result<Vec<Series>, SourceError> {
            self.requested_ranges.borrow_mut().push(range);
            Ok(self.series_by_query.get(query).cloned().unwrap_or_default())
        }

        fn alerts(&self) -> Result<Vec<Alert>, SourceError> {
            Ok(self.alerts.clone())
        }
    }

    fn labels(pairs: &[(&str, &str)]) -> Labels {
        pairs
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect()
    }

    fn sample(pairs: &[(&str, &str)], value: f64) -> Sample {
        Sample {
            labels: labels(pairs),
            value,
        }
    }

    fn widget(kind: WidgetKind) -> Widget {
        Widget {
            title: "Springfield".into(),
            area: Area {
                column: 0,
                row: 0,
                column_span: 1,
                row_span: 1,
            },
            kind,
        }
    }

    fn numeric_column(header: &str, query: &str) -> TableColumn {
        TableColumn {
            header: header.into(),
            query: query.into(),
            value_label: None,
            display: ValueDisplay::default(),
        }
    }

    fn now() -> Timestamp {
        EVALUATION_TIME.parse().unwrap()
    }

    #[test]
    fn stat_reads_value_and_total() {
        let source = FakeSource {
            samples_by_query: BTreeMap::from([
                ("ready".into(), vec![sample(&[], 4.0)]),
                ("total".into(), vec![sample(&[], 5.0)]),
            ]),
            ..FakeSource::default()
        };
        let stat = widget(WidgetKind::Stat(StatWidget {
            query: "ready".into(),
            total_query: Some("total".into()),
            display: ValueDisplay::default(),
        }));

        let data = fetch_widget(&stat, &source, now()).unwrap();

        assert_eq!(
            data,
            WidgetData::Stat(StatData {
                value: Some(4.0),
                total: Some(5.0)
            })
        );
    }

    #[test]
    fn table_keeps_configured_keys_even_without_data() {
        let source = FakeSource {
            samples_by_query: BTreeMap::from([(
                "cpu".into(),
                vec![
                    sample(&[("node", "homer")], 0.5),
                    sample(&[("node", "moe")], 0.1),
                ],
            )]),
            ..FakeSource::default()
        };
        let table = widget(WidgetKind::Table(TableWidget {
            key_label: "node".into(),
            key_header: "Node".into(),
            keys: vec!["homer".into(), "marge".into()],
            sort_by: None,
            columns: vec![numeric_column("CPU", "cpu")],
        }));

        let data = fetch_widget(&table, &source, now()).unwrap();

        let expected_rows = vec![
            TableRow {
                key: "homer".into(),
                values: vec![Some(TableCell::Number(0.5))],
            },
            TableRow {
                key: "marge".into(),
                values: vec![None],
            },
        ];
        assert_eq!(
            data,
            WidgetData::Table(TableData {
                rows: expected_rows
            })
        );
    }

    #[test]
    fn table_discovers_keys_from_every_column() {
        let source = FakeSource {
            samples_by_query: BTreeMap::from([
                ("cpu".into(), vec![sample(&[("node", "lisa")], 0.2)]),
                (
                    "temperature".into(),
                    vec![sample(&[("node", "bart")], 61.0)],
                ),
            ]),
            ..FakeSource::default()
        };
        let table = widget(WidgetKind::Table(TableWidget {
            key_label: "node".into(),
            key_header: "Node".into(),
            keys: vec![],
            sort_by: None,
            columns: vec![
                numeric_column("cpu", "cpu"),
                numeric_column("temperature", "temperature"),
            ],
        }));

        let WidgetData::Table(data) = fetch_widget(&table, &source, now()).unwrap() else {
            panic!("expected a table")
        };

        assert_eq!(
            data.rows
                .iter()
                .map(|row| row.key.as_str())
                .collect::<Vec<_>>(),
            ["bart", "lisa"]
        );
        assert_eq!(data.rows[0].values, [None, Some(TableCell::Number(61.0))]);
    }

    #[test]
    fn table_shows_label_values_and_sorts_by_column() {
        let source = FakeSource {
            samples_by_query: BTreeMap::from([
                (
                    "host".into(),
                    vec![
                        sample(&[("name", "itchy"), ("node", "krusty")], 1.0),
                        sample(&[("name", "scratchy"), ("node", "bart")], 1.0),
                        sample(&[("name", "poochie"), ("node", "krusty")], 1.0),
                    ],
                ),
                ("cpu".into(), vec![sample(&[("name", "itchy")], 0.4)]),
            ]),
            ..FakeSource::default()
        };
        let host_column = TableColumn {
            value_label: Some("node".into()),
            ..numeric_column("Host", "host")
        };
        let table = widget(WidgetKind::Table(TableWidget {
            key_label: "name".into(),
            key_header: "VM".into(),
            keys: vec![],
            sort_by: Some("Host".into()),
            columns: vec![host_column, numeric_column("CPU", "cpu")],
        }));

        let WidgetData::Table(data) = fetch_widget(&table, &source, now()).unwrap() else {
            panic!("expected a table")
        };

        let summary: Vec<_> = data
            .rows
            .iter()
            .map(|row| (row.key.as_str(), &row.values[0]))
            .collect();
        let text = |value: &str| Some(TableCell::Text(value.into()));
        assert_eq!(
            summary,
            [
                ("scratchy", &text("bart")),
                ("itchy", &text("krusty")),
                ("poochie", &text("krusty"))
            ]
        );
        assert_eq!(data.rows[1].values[1], Some(TableCell::Number(0.4)));
    }

    #[test]
    fn alerts_are_filtered_and_sorted_by_level() {
        let alert = |name: &str, state: &str, severity: Option<&str>| Alert {
            name: name.into(),
            summary: None,
            state: state.into(),
            labels: severity
                .map(|severity| labels(&[("severity", severity)]))
                .unwrap_or_default(),
            active_since: None,
        };
        let source = FakeSource {
            alerts: vec![
                alert("Out of Duff", "Alerting", None),
                alert("Reactor meltdown", "Alerting", Some("critical")),
                alert("Flanders is waving", "Normal", Some("critical")),
            ],
            ..FakeSource::default()
        };
        let alerts = widget(WidgetKind::Alerts(AlertsWidget {
            states: vec!["Alerting".into()],
            severity_label: "severity".into(),
            default_level: Level::Warning,
        }));

        let WidgetData::Alerts(entries) = fetch_widget(&alerts, &source, now()).unwrap() else {
            panic!("expected alerts")
        };

        let summary: Vec<_> = entries
            .iter()
            .map(|entry| (entry.alert.name.as_str(), entry.level))
            .collect();
        assert_eq!(
            summary,
            [
                ("Reactor meltdown", Level::Critical),
                ("Out of Duff", Level::Warning)
            ]
        );
    }

    #[test]
    fn timeseries_names_series_and_derives_step() {
        let source = FakeSource {
            series_by_query: BTreeMap::from([(
                "cpu".into(),
                vec![Series {
                    labels: labels(&[("node", "skinner")]),
                    points: vec![(1.0, 0.3)],
                }],
            )]),
            ..FakeSource::default()
        };
        let timeseries = widget(WidgetKind::Timeseries(TimeseriesWidget {
            queries: vec![TimeseriesQuery {
                query: "cpu".into(),
                legend: "{{node}} cpu".into(),
            }],
            range: Duration::from_secs(3600),
            points: 120,
            minimum: None,
            maximum: None,
            style: ChartStyle::Lines,
            display: ValueDisplay::default(),
        }));

        let data = fetch_widget(&timeseries, &source, now()).unwrap();

        assert_eq!(
            data,
            WidgetData::Timeseries(vec![NamedSeries {
                name: "skinner cpu".into(),
                points: vec![(1.0, 0.3)]
            }])
        );
        let range = source.requested_ranges.borrow()[0];
        assert_eq!(range.step_seconds, 30.0);
        assert_eq!(range.end, now());
        assert_eq!(range.start, "2026-11-03T11:00:00Z".parse().unwrap());
    }

    #[test]
    fn snapshot_keeps_errors_per_widget() {
        let source = FakeSource::default();
        let stat = widget(WidgetKind::Stat(StatWidget {
            query: "missing".into(),
            total_query: None,
            display: ValueDisplay::default(),
        }));

        let snapshot = fetch_snapshot(&[stat], &source, now());

        assert_eq!(snapshot.error_count(), 1);
    }

    #[test]
    fn legend_falls_back_to_all_labels() {
        assert_eq!(
            render_legend("", &labels(&[("bar", "moes"), ("city", "springfield")])),
            "bar=moes,city=springfield"
        );
    }
}
