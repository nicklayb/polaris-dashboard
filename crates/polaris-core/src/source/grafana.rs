//! Grafana backed data source: PromQL through the batched
//! `POST /api/ds/query` endpoint and Grafana-managed alerts through the
//! Prometheus compatible alerting API.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use jiff::Timestamp;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use ureq::Agent;
use ureq::http::{Response, Uri};
use ureq::tls::{RootCerts, TlsConfig};
use ureq::unversioned::transport::DefaultConnector;

use super::data_frame::QueryDataResponse;
use super::dns::{CachingResolver, is_connection_failure};
use super::query_batch::{QueryRequest, group_by_window};
use super::{DataSource, Query, QueryOutput, SourceError, TimeRange};
use crate::config::GrafanaConfig;
use crate::data::{Alert, Sample, Series};

const TOKEN_ENVIRONMENT_VARIABLE: &str = "POLARIS_GRAFANA_TOKEN";
const CREDENTIALS_DIRECTORY_VARIABLE: &str = "CREDENTIALS_DIRECTORY";
const TOKEN_CREDENTIAL_NAME: &str = "grafana-token";
const ALERTS_PATH: &str = "/api/prometheus/grafana/api/v1/alerts";
const QUERY_PATH: &str = "/api/ds/query";
const ALERT_NAME_LABEL: &str = "alertname";
const ALERT_SUMMARY_ANNOTATION: &str = "summary";
const MAXIMUM_ERROR_BODY_CHARACTERS: usize = 120;
const UNSENT_QUERY_MESSAGE: &str = "query was not sent";
const MISMATCHED_OUTPUT_MESSAGE: &str = "query output does not match the query kind";

/// Talks to a Grafana instance with a service account token.
pub struct GrafanaSource {
    agent: Agent,
    resolver: CachingResolver,
    base_url: String,
    datasource_uid: String,
    authorization: String,
}

impl GrafanaSource {
    /// Builds a source for `config`, authenticating with `token`.
    ///
    /// Host name answers are cached for `config.dns_cache_ttl` and dropped
    /// as soon as a request fails at the connection level.
    pub fn new(config: &GrafanaConfig, token: &str) -> Self {
        // The platform verifier trusts the system CA store, which holds
        // private homelab CAs that bundled web roots do not.
        let tls_config = TlsConfig::builder()
            .root_certs(RootCerts::PlatformVerifier)
            .build();
        let agent_config = Agent::config_builder()
            .timeout_global(Some(config.timeout))
            .tls_config(tls_config)
            .http_status_as_error(false)
            .build();
        let resolver = CachingResolver::with_system_resolver(config.dns_cache_ttl);
        let agent = Agent::with_parts(agent_config, DefaultConnector::default(), resolver.clone());
        Self {
            agent,
            resolver,
            base_url: config.url.trim_end_matches('/').to_string(),
            datasource_uid: config.datasource_uid.clone(),
            authorization: format!("Bearer {}", token.trim()),
        }
    }

    fn get_json<T: DeserializeOwned>(&self, url: &str) -> Result<T, SourceError> {
        let response = self
            .agent
            .get(url)
            .header("Authorization", &self.authorization)
            .call();
        let mut response = self.connected(url, response)?;
        let status = response.status();
        if !status.is_success() {
            let body = response.body_mut().read_to_string().unwrap_or_default();
            return Err(SourceError::Request(describe_http_failure(
                status.as_u16(),
                &body,
            )));
        }
        response
            .body_mut()
            .read_json()
            .map_err(|error| SourceError::Response(error.to_string()))
    }

    fn post_query(&self, request: &QueryRequest) -> Result<QueryDataResponse, SourceError> {
        let url = format!("{}{QUERY_PATH}", self.base_url);
        let response = self
            .agent
            .post(&url)
            .header("Authorization", &self.authorization)
            .send_json(request);
        let mut response = self.connected(&url, response)?;
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(|error| SourceError::Response(error.to_string()))?;
        parse_query_response(status, &body)
    }

    /// Turns a transport failure into an error, forgetting the cached
    /// address of the host so the next request resolves it again.
    fn connected(
        &self,
        url: &str,
        response: Result<Response<ureq::Body>, ureq::Error>,
    ) -> Result<Response<ureq::Body>, SourceError> {
        response.map_err(|error| {
            if is_connection_failure(&error) {
                self.forget_address(url);
            }
            SourceError::Request(error.to_string())
        })
    }

    fn forget_address(&self, url: &str) {
        if let Ok(uri) = url.parse::<Uri>() {
            self.resolver.invalidate(&uri);
        }
    }

    fn single_query(&self, query: Query) -> Result<QueryOutput, SourceError> {
        self.query_batch(&[query])
            .pop()
            .unwrap_or_else(|| Err(SourceError::Request(UNSENT_QUERY_MESSAGE.into())))
    }
}

impl DataSource for GrafanaSource {
    fn instant_query(&self, query: &str) -> Result<Vec<Sample>, SourceError> {
        let output = self.single_query(Query::Instant {
            expression: query.to_string(),
        })?;
        match output {
            QueryOutput::Samples(samples) => Ok(samples),
            QueryOutput::Series(_) => Err(SourceError::Response(MISMATCHED_OUTPUT_MESSAGE.into())),
        }
    }

    fn range_query(&self, query: &str, range: TimeRange) -> Result<Vec<Series>, SourceError> {
        let output = self.single_query(Query::Range {
            expression: query.to_string(),
            range,
        })?;
        match output {
            QueryOutput::Series(series) => Ok(series),
            QueryOutput::Samples(_) => Err(SourceError::Response(MISMATCHED_OUTPUT_MESSAGE.into())),
        }
    }

    fn alerts(&self) -> Result<Vec<Alert>, SourceError> {
        let response: AlertsResponse = self.get_json(&format!("{}{ALERTS_PATH}", self.base_url))?;
        Ok(response
            .data
            .alerts
            .into_iter()
            .map(GrafanaAlert::into_alert)
            .collect())
    }

    /// Sends one request per time window (see [`group_by_window`]).
    fn query_batch(&self, queries: &[Query]) -> Vec<Result<QueryOutput, SourceError>> {
        let mut outputs: Vec<Result<QueryOutput, SourceError>> = queries
            .iter()
            .map(|_| Err(SourceError::Request(UNSENT_QUERY_MESSAGE.into())))
            .collect();
        for group in group_by_window(queries) {
            let response = self.post_query(&group.request(&self.datasource_uid));
            for (batch_index, output) in group.outputs(response) {
                outputs[batch_index] = output;
            }
        }
        outputs
    }
}

/// Reads the Grafana token from, in order: `token_file`, the
/// `POLARIS_GRAFANA_TOKEN` environment variable, or the `grafana-token`
/// systemd credential.
pub fn read_token(config: &GrafanaConfig) -> Result<String, SourceError> {
    if let Some(path) = &config.token_file {
        return read_token_file(path);
    }
    if let Ok(token) = std::env::var(TOKEN_ENVIRONMENT_VARIABLE) {
        return Ok(token);
    }
    match std::env::var_os(CREDENTIALS_DIRECTORY_VARIABLE) {
        Some(directory) => read_token_file(&PathBuf::from(directory).join(TOKEN_CREDENTIAL_NAME)),
        None => Err(SourceError::Request(format!(
            "no token: set grafana.token_file or {TOKEN_ENVIRONMENT_VARIABLE}"
        ))),
    }
}

fn read_token_file(path: &Path) -> Result<String, SourceError> {
    std::fs::read_to_string(path)
        .map(|token| token.trim().to_string())
        .map_err(|error| {
            SourceError::Request(format!("cannot read token {}: {error}", path.display()))
        })
}

#[derive(Deserialize)]
struct GrafanaErrorBody {
    message: String,
}

#[derive(Deserialize)]
struct AlertsResponse {
    data: AlertsData,
}

#[derive(Deserialize)]
struct AlertsData {
    alerts: Vec<GrafanaAlert>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GrafanaAlert {
    labels: BTreeMap<String, String>,
    #[serde(default)]
    annotations: BTreeMap<String, String>,
    state: String,
    active_at: Option<String>,
}

impl GrafanaAlert {
    fn into_alert(self) -> Alert {
        Alert {
            name: self
                .labels
                .get(ALERT_NAME_LABEL)
                .cloned()
                .unwrap_or_default(),
            summary: self.annotations.get(ALERT_SUMMARY_ANNOTATION).cloned(),
            state: self.state,
            active_since: self
                .active_at
                .and_then(|active_at| active_at.parse::<Timestamp>().ok()),
            labels: self.labels,
        }
    }
}

/// Reads a `/api/ds/query` body. Grafana answers HTTP 400 when any query
/// of the batch fails but still reports every query in `results`, so such
/// a body is kept and only the failing queries become errors.
fn parse_query_response(status: u16, body: &str) -> Result<QueryDataResponse, SourceError> {
    let is_success = (200..300).contains(&status);
    match serde_json::from_str::<QueryDataResponse>(body) {
        Ok(response) if is_success || !response.results.is_empty() => Ok(response),
        Err(error) if is_success => Err(SourceError::Response(error.to_string())),
        _ => Err(SourceError::Request(describe_http_failure(status, body))),
    }
}

/// Builds an error message from a failed response, keeping Grafana's own
/// explanation (`{"message": "..."}`) when there is one.
fn describe_http_failure(status: u16, body: &str) -> String {
    let message = serde_json::from_str::<GrafanaErrorBody>(body)
        .map(|error_body| error_body.message)
        .unwrap_or_else(|_| {
            body.trim()
                .chars()
                .take(MAXIMUM_ERROR_BODY_CHARACTERS)
                .collect()
        });
    if message.is_empty() {
        format!("HTTP {status}")
    } else {
        format!("HTTP {status}: {message}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_response_keeps_per_query_errors_of_a_failed_batch() {
        let body = r#"{"results":{"Q0":{"status":400,"error":"parse error: D'oh!"}}}"#;

        let response = parse_query_response(400, body).unwrap();

        assert_eq!(response.results.len(), 1);
    }

    #[test]
    fn query_response_without_results_is_a_request_failure() {
        let body = r#"{"message":"Invalid API key"}"#;

        assert_eq!(
            parse_query_response(401, body).unwrap_err(),
            SourceError::Request("HTTP 401: Invalid API key".into())
        );
    }

    #[test]
    fn http_failure_keeps_grafana_message() {
        let body = r#"{"message":"Invalid API key","traceID":""}"#;

        assert_eq!(
            describe_http_failure(401, body),
            "HTTP 401: Invalid API key"
        );
    }

    #[test]
    fn http_failure_falls_back_to_raw_body() {
        assert_eq!(
            describe_http_failure(401, "Unauthorized\n"),
            "HTTP 401: Unauthorized"
        );
        assert_eq!(describe_http_failure(502, ""), "HTTP 502");
    }

    #[test]
    fn parses_grafana_alert() {
        let body = r#"{"status":"success","data":{"alerts":[{
            "labels":{"alertname":"Core temperature","severity":"critical"},
            "annotations":{"summary":"Reactor 7G is overheating"},
            "state":"Alerting","activeAt":"2026-10-24T03:23:40Z","value":"1e+00"}]}}"#;

        let response: AlertsResponse = serde_json::from_str(body).unwrap();
        let alert = response
            .data
            .alerts
            .into_iter()
            .next()
            .unwrap()
            .into_alert();

        assert_eq!(alert.name, "Core temperature");
        assert_eq!(alert.summary.as_deref(), Some("Reactor 7G is overheating"));
        assert_eq!(
            alert.active_since,
            Some("2026-10-24T03:23:40Z".parse().unwrap())
        );
    }
}
