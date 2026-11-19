//! Packs PromQL queries into Grafana `POST /api/ds/query` requests and maps
//! the per-`refId` results back to the queries.
//!
//! Grafana applies the top level `from`/`to` to every query of a request and
//! ignores per-query time ranges (checked against Grafana 13.2), so range
//! queries are grouped by time window, one request per window. Instant
//! queries are evaluated at `to`: they ride along with the first group, or
//! form their own group when there is no range query.

use jiff::Timestamp;
use serde::Serialize;

use super::data_frame::{QueryDataResponse, QueryResult};
use super::{Query, QueryOutput, SourceError};

const REFERENCE_PREFIX: &str = "Q";
const LATEST_WINDOW_FROM: &str = "now-5m";
const LATEST_WINDOW_TO: &str = "now";
const MILLISECONDS_PER_SECOND: f64 = 1000.0;
const MINIMUM_INTERVAL_MILLISECONDS: u64 = 1;
/// Grafana widens the step when `range / maxDataPoints` exceeds it, and caps
/// resolution at 11 000 points like Prometheus. Asking for that many keeps
/// the requested step (`intervalMs`) untouched.
const MAXIMUM_DATA_POINTS: u64 = 11_000;

/// Queries sharing one time window, sent as a single request.
#[derive(Debug, PartialEq)]
pub struct QueryGroup<'a> {
    window: Option<Window>,
    members: Vec<GroupMember<'a>>,
}

/// A query of a group and its position in the original batch.
#[derive(Debug, PartialEq)]
struct GroupMember<'a> {
    batch_index: usize,
    query: &'a Query,
}

/// Explicit `from`/`to` of a group; groups without one use `now`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Window {
    start: Timestamp,
    end: Timestamp,
}

/// JSON body of `POST /api/ds/query`.
#[derive(Debug, Serialize)]
pub struct QueryRequest<'a> {
    from: String,
    to: String,
    queries: Vec<PrometheusQuery<'a>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PrometheusQuery<'a> {
    ref_id: String,
    datasource: DatasourceReference<'a>,
    expr: &'a str,
    instant: bool,
    range: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    interval_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_data_points: Option<u64>,
}

#[derive(Debug, Serialize)]
struct DatasourceReference<'a> {
    uid: &'a str,
}

/// Splits `queries` into groups sharing a time window, in first-seen order.
pub fn group_by_window(queries: &[Query]) -> Vec<QueryGroup<'_>> {
    let mut groups: Vec<QueryGroup> = Vec::new();
    let mut instant_members = Vec::new();
    for (batch_index, query) in queries.iter().enumerate() {
        let member = GroupMember { batch_index, query };
        match query_window(query) {
            Some(window) => group_for(&mut groups, window).members.push(member),
            None => instant_members.push(member),
        }
    }
    attach_instant_members(&mut groups, instant_members);
    groups
}

fn query_window(query: &Query) -> Option<Window> {
    match query {
        Query::Instant { .. } => None,
        Query::Range { range, .. } => Some(Window {
            start: range.start,
            end: range.end,
        }),
    }
}

fn group_for<'group, 'query>(
    groups: &'group mut Vec<QueryGroup<'query>>,
    window: Window,
) -> &'group mut QueryGroup<'query> {
    let position = groups
        .iter()
        .position(|group| group.window == Some(window))
        .unwrap_or_else(|| {
            groups.push(QueryGroup {
                window: Some(window),
                members: Vec::new(),
            });
            groups.len() - 1
        });
    &mut groups[position]
}

fn attach_instant_members<'a>(groups: &mut Vec<QueryGroup<'a>>, members: Vec<GroupMember<'a>>) {
    if members.is_empty() {
        return;
    }
    match groups.first_mut() {
        Some(group) => group.members.extend(members),
        None => groups.push(QueryGroup {
            window: None,
            members,
        }),
    }
}

impl<'a> QueryGroup<'a> {
    /// Builds the request body sending every query of the group to the
    /// Prometheus datasource `datasource_uid`.
    pub fn request(&self, datasource_uid: &'a str) -> QueryRequest<'a> {
        let (from, to) = match self.window {
            Some(window) => (
                epoch_milliseconds(window.start),
                epoch_milliseconds(window.end),
            ),
            None => (LATEST_WINDOW_FROM.to_string(), LATEST_WINDOW_TO.to_string()),
        };
        let queries = self
            .members
            .iter()
            .enumerate()
            .map(|(position, member)| prometheus_query(position, member.query, datasource_uid))
            .collect();
        QueryRequest { from, to, queries }
    }

    /// Pairs each query of the group, by batch index, with its result in
    /// `response`. A failed response fails every query of the group.
    pub fn outputs(
        &self,
        response: Result<QueryDataResponse, SourceError>,
    ) -> Vec<(usize, Result<QueryOutput, SourceError>)> {
        let mut response = match response {
            Ok(response) => response,
            Err(error) => return self.failed_outputs(&error),
        };
        self.members
            .iter()
            .enumerate()
            .map(|(position, member)| {
                let result = response.results.remove(&reference_id(position));
                (member.batch_index, query_output(member.query, result))
            })
            .collect()
    }

    fn failed_outputs(
        &self,
        error: &SourceError,
    ) -> Vec<(usize, Result<QueryOutput, SourceError>)> {
        self.members
            .iter()
            .map(|member| (member.batch_index, Err(error.clone())))
            .collect()
    }
}

fn prometheus_query<'a>(
    position: usize,
    query: &'a Query,
    datasource_uid: &'a str,
) -> PrometheusQuery<'a> {
    let datasource = DatasourceReference {
        uid: datasource_uid,
    };
    match query {
        Query::Instant { expression } => PrometheusQuery {
            ref_id: reference_id(position),
            datasource,
            expr: expression,
            instant: true,
            range: false,
            interval_ms: None,
            max_data_points: None,
        },
        Query::Range { expression, range } => PrometheusQuery {
            ref_id: reference_id(position),
            datasource,
            expr: expression,
            instant: false,
            range: true,
            interval_ms: Some(step_milliseconds(range.step_seconds)),
            max_data_points: Some(MAXIMUM_DATA_POINTS),
        },
    }
}

fn query_output(query: &Query, result: Option<QueryResult>) -> Result<QueryOutput, SourceError> {
    let result = result
        .ok_or_else(|| SourceError::Response("Grafana returned no result for the query".into()))?;
    match query {
        Query::Instant { .. } => result.into_samples().map(QueryOutput::Samples),
        Query::Range { .. } => result.into_series().map(QueryOutput::Series),
    }
}

fn reference_id(position: usize) -> String {
    format!("{REFERENCE_PREFIX}{position}")
}

fn epoch_milliseconds(timestamp: Timestamp) -> String {
    timestamp.as_millisecond().to_string()
}

fn step_milliseconds(step_seconds: f64) -> u64 {
    ((step_seconds * MILLISECONDS_PER_SECOND).round() as u64).max(MINIMUM_INTERVAL_MILLISECONDS)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::source::TimeRange;

    const DATASOURCE_UID: &str = "power-plant";
    const EVALUATION_TIME: &str = "2026-11-03T12:00:00Z";
    const ONE_HOUR_AGO: &str = "2026-11-03T11:00:00Z";
    const SIX_HOURS_AGO: &str = "2026-11-03T06:00:00Z";

    fn instant(expression: &str) -> Query {
        Query::Instant {
            expression: expression.into(),
        }
    }

    fn range(expression: &str, start: &str, step_seconds: f64) -> Query {
        Query::Range {
            expression: expression.into(),
            range: TimeRange {
                start: start.parse().unwrap(),
                end: EVALUATION_TIME.parse().unwrap(),
                step_seconds,
            },
        }
    }

    fn batch_indices(group: &QueryGroup) -> Vec<usize> {
        group
            .members
            .iter()
            .map(|member| member.batch_index)
            .collect()
    }

    #[test]
    fn groups_ranges_by_window_and_attaches_instants() {
        let queries = [
            instant("donuts"),
            range("reactor", ONE_HOUR_AGO, 60.0),
            range("duff", SIX_HOURS_AGO, 360.0),
            range("squishee", ONE_HOUR_AGO, 60.0),
        ];

        let groups = group_by_window(&queries);

        assert_eq!(groups.len(), 2);
        assert_eq!(batch_indices(&groups[0]), [1, 3, 0]);
        assert_eq!(batch_indices(&groups[1]), [2]);
    }

    #[test]
    fn instants_alone_use_the_latest_window() {
        let queries = [instant("donuts")];

        let groups = group_by_window(&queries);
        let request = serde_json::to_value(groups[0].request(DATASOURCE_UID)).unwrap();

        assert_eq!(
            request,
            json!({
                "from": "now-5m",
                "to": "now",
                "queries": [{
                    "refId": "Q0",
                    "datasource": {"uid": "power-plant"},
                    "expr": "donuts",
                    "instant": true,
                    "range": false
                }]
            })
        );
    }

    #[test]
    fn range_request_carries_window_and_step() {
        let queries = [range("reactor", ONE_HOUR_AGO, 60.0)];

        let groups = group_by_window(&queries);
        let request = serde_json::to_value(groups[0].request(DATASOURCE_UID)).unwrap();

        assert_eq!(request["from"], "1793703600000");
        assert_eq!(request["to"], "1793707200000");
        assert_eq!(request["queries"][0]["intervalMs"], 60_000);
        assert_eq!(request["queries"][0]["maxDataPoints"], MAXIMUM_DATA_POINTS);
        assert_eq!(request["queries"][0]["range"], true);
    }

    #[test]
    fn outputs_map_results_back_and_isolate_errors() {
        let queries = [instant("donuts"), instant("sum(")];
        let groups = group_by_window(&queries);
        let response: QueryDataResponse = serde_json::from_value(json!({"results": {
            "Q0": {"status": 200, "frames": []},
            "Q1": {"status": 400, "error": "parse error: D'oh!", "frames": []}
        }}))
        .unwrap();

        let outputs = groups[0].outputs(Ok(response));

        assert_eq!(outputs[0], (0, Ok(QueryOutput::Samples(vec![]))));
        assert_eq!(
            outputs[1],
            (1, Err(SourceError::Response("parse error: D'oh!".into())))
        );
    }

    #[test]
    fn failed_response_fails_every_query_of_the_group() {
        let queries = [instant("donuts"), instant("duff")];
        let groups = group_by_window(&queries);
        let error = SourceError::Request("HTTP 502".into());

        let outputs = groups[0].outputs(Err(error.clone()));

        assert_eq!(outputs, [(0, Err(error.clone())), (1, Err(error))]);
    }

    #[test]
    fn missing_result_is_an_error() {
        let queries = [instant("donuts")];
        let groups = group_by_window(&queries);
        let response = QueryDataResponse {
            results: Default::default(),
        };

        let outputs = groups[0].outputs(Ok(response));

        assert!(outputs[0].1.is_err());
    }
}
