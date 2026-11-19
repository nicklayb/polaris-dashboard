//! Per-widget refresh scheduling: only widgets whose own interval elapsed
//! are fetched, the others keep their last result.

use std::time::Duration;

use jiff::{SignedDuration, Timestamp};

use crate::data::{Snapshot, WidgetData};
use crate::fetch::fetch_widgets;
use crate::source::{DataSource, SourceError};
use crate::widget::Widget;

const NEVER_FETCHED_MESSAGE: &str = "widget was never fetched";

/// Fetches widgets when they are due and remembers their latest result.
pub struct WidgetRefresher {
    widgets: Vec<Widget>,
    default_interval: Duration,
    last_fetches: Vec<Option<LastFetch>>,
}

/// When a widget was last fetched and what came back.
#[derive(Debug, Clone)]
struct LastFetch {
    fetched_at: Timestamp,
    result: Result<WidgetData, SourceError>,
}

impl WidgetRefresher {
    /// Schedules `widgets`; those without their own `refresh` interval use
    /// `default_interval`.
    pub fn new(widgets: Vec<Widget>, default_interval: Duration) -> Self {
        let last_fetches = vec![None; widgets.len()];
        Self {
            widgets,
            default_interval,
            last_fetches,
        }
    }

    /// Fetches every widget due at `now` in one batch and returns a
    /// snapshot of all widgets, reusing the last result of the others.
    pub fn refresh(&mut self, source: &dyn DataSource, now: Timestamp) -> Snapshot {
        let due_indices = self.due_indices(now);
        let due_widgets: Vec<&Widget> = due_indices
            .iter()
            .map(|&index| &self.widgets[index])
            .collect();
        let results = fetch_widgets(&due_widgets, source, now);
        for (index, result) in due_indices.into_iter().zip(results) {
            self.last_fetches[index] = Some(LastFetch {
                fetched_at: now,
                result,
            });
        }
        self.snapshot(now)
    }

    fn due_indices(&self, now: Timestamp) -> Vec<usize> {
        self.widgets
            .iter()
            .zip(&self.last_fetches)
            .enumerate()
            .filter(|(_, (widget, last_fetch))| {
                let interval = widget.refresh.unwrap_or(self.default_interval);
                is_due(last_fetch.as_ref(), interval, now)
            })
            .map(|(index, _)| index)
            .collect()
    }

    fn snapshot(&self, now: Timestamp) -> Snapshot {
        Snapshot {
            fetched_at: now,
            results: self
                .last_fetches
                .iter()
                .map(|last_fetch| match last_fetch {
                    Some(last_fetch) => last_fetch.result.clone(),
                    None => Err(SourceError::Request(NEVER_FETCHED_MESSAGE.into())),
                })
                .collect(),
        }
    }
}

/// A widget is due when it was never fetched, when its last fetch failed
/// (so errors clear as soon as possible), when `interval` elapsed, or when
/// the clock went backwards.
fn is_due(last_fetch: Option<&LastFetch>, interval: Duration, now: Timestamp) -> bool {
    let Some(last_fetch) = last_fetch else {
        return true;
    };
    if last_fetch.result.is_err() {
        return true;
    }
    let elapsed = now.duration_since(last_fetch.fetched_at);
    match SignedDuration::try_from(interval) {
        Ok(interval) => elapsed.is_negative() || elapsed >= interval,
        Err(_) => elapsed.is_negative(),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;
    use crate::data::{Alert, Sample, Series, StatData};
    use crate::source::TimeRange;
    use crate::widget::{Area, StatWidget, ValueDisplay, WidgetKind};

    const GLOBAL_INTERVAL: Duration = Duration::from_secs(15);
    const SLOW_INTERVAL: Duration = Duration::from_secs(60);
    const FIRST_REFRESH: &str = "2026-11-03T12:00:00Z";

    /// Answers every instant query with the number of queries seen so far,
    /// failing queries named `"flaky"` while `is_flaky_failing` is set.
    #[derive(Default)]
    struct CountingSource {
        queried_expressions: RefCell<Vec<String>>,
        is_flaky_failing: RefCell<bool>,
    }

    impl DataSource for CountingSource {
        fn instant_query(&self, query: &str) -> Result<Vec<Sample>, SourceError> {
            if query == "flaky" && *self.is_flaky_failing.borrow() {
                return Err(SourceError::Request("Mr. Burns pulled the plug".into()));
            }
            self.queried_expressions.borrow_mut().push(query.into());
            Ok(vec![Sample {
                labels: Default::default(),
                value: self.queried_expressions.borrow().len() as f64,
            }])
        }

        fn range_query(&self, _query: &str, _range: TimeRange) -> Result<Vec<Series>, SourceError> {
            Ok(Vec::new())
        }

        fn alerts(&self) -> Result<Vec<Alert>, SourceError> {
            Ok(Vec::new())
        }
    }

    fn stat(query: &str, refresh: Option<Duration>) -> Widget {
        Widget {
            title: "Kwik-E-Mart".into(),
            area: Area {
                column: 0,
                row: 0,
                column_span: 1,
                row_span: 1,
            },
            refresh,
            kind: WidgetKind::Stat(StatWidget {
                query: query.into(),
                total_query: None,
                display: ValueDisplay::default(),
            }),
        }
    }

    fn after(seconds: i64) -> Timestamp {
        FIRST_REFRESH.parse::<Timestamp>().unwrap() + SignedDuration::from_secs(seconds)
    }

    fn stat_value(snapshot: &Snapshot, index: usize) -> Option<f64> {
        match &snapshot.results[index] {
            Ok(WidgetData::Stat(StatData { value, .. })) => *value,
            other => panic!("expected a stat, got {other:?}"),
        }
    }

    #[test]
    fn slow_widget_reuses_its_result_until_due() {
        let source = CountingSource::default();
        let mut refresher = WidgetRefresher::new(
            vec![stat("squishee", None), stat("duff", Some(SLOW_INTERVAL))],
            GLOBAL_INTERVAL,
        );

        refresher.refresh(&source, after(0));
        let reused = refresher.refresh(&source, after(15));
        refresher.refresh(&source, after(60));

        assert_eq!(stat_value(&reused, 1), Some(2.0));
        assert_eq!(
            *source.queried_expressions.borrow(),
            ["squishee", "duff", "squishee", "squishee", "duff"]
        );
    }

    #[test]
    fn failed_widget_is_retried_on_next_refresh() {
        let source = CountingSource::default();
        *source.is_flaky_failing.borrow_mut() = true;
        let mut refresher =
            WidgetRefresher::new(vec![stat("flaky", Some(SLOW_INTERVAL))], GLOBAL_INTERVAL);

        let failed = refresher.refresh(&source, after(0));
        *source.is_flaky_failing.borrow_mut() = false;
        let recovered = refresher.refresh(&source, after(15));

        assert_eq!(failed.error_count(), 1);
        assert_eq!(recovered.error_count(), 0);
    }

    #[test]
    fn widget_is_due_when_the_clock_goes_backwards() {
        let last_fetch = LastFetch {
            fetched_at: after(60),
            result: Ok(WidgetData::Alerts(Vec::new())),
        };

        assert!(is_due(Some(&last_fetch), SLOW_INTERVAL, after(0)));
        assert!(!is_due(Some(&last_fetch), SLOW_INTERVAL, after(61)));
    }
}
