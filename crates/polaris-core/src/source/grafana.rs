//! Grafana backed data source: PromQL through the datasource proxy and
//! Grafana-managed alerts through the Prometheus compatible alerting API.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use jiff::Timestamp;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use ureq::Agent;
use ureq::tls::{RootCerts, TlsConfig};

use super::{DataSource, SourceError, TimeRange};
use crate::config::GrafanaConfig;
use crate::data::{Alert, Sample, Series};

const TOKEN_ENVIRONMENT_VARIABLE: &str = "POLARIS_GRAFANA_TOKEN";
const CREDENTIALS_DIRECTORY_VARIABLE: &str = "CREDENTIALS_DIRECTORY";
const TOKEN_CREDENTIAL_NAME: &str = "grafana-token";
const ALERTS_PATH: &str = "/api/prometheus/grafana/api/v1/alerts";
const ALERT_NAME_LABEL: &str = "alertname";
const ALERT_SUMMARY_ANNOTATION: &str = "summary";
const MAXIMUM_ERROR_BODY_CHARACTERS: usize = 120;

/// Talks to a Grafana instance with a service account token.
pub struct GrafanaSource {
    agent: Agent,
    base_url: String,
    datasource_uid: String,
    authorization: String,
}

impl GrafanaSource {
    /// Builds a source for `config`, authenticating with `token`.
    pub fn new(config: &GrafanaConfig, token: &str) -> Self {
        // The platform verifier trusts the system CA store, which holds
        // private homelab CAs that bundled web roots do not.
        let tls_config = TlsConfig::builder()
            .root_certs(RootCerts::PlatformVerifier)
            .build();
        let agent = Agent::config_builder()
            .timeout_global(Some(config.timeout))
            .tls_config(tls_config)
            .http_status_as_error(false)
            .build()
            .into();
        Self {
            agent,
            base_url: config.url.trim_end_matches('/').to_string(),
            datasource_uid: config.datasource_uid.clone(),
            authorization: format!("Bearer {}", token.trim()),
        }
    }

    fn prometheus_url(&self, endpoint: &str) -> String {
        format!(
            "{}/api/datasources/proxy/uid/{}/api/v1/{endpoint}",
            self.base_url, self.datasource_uid
        )
    }

    fn get_json<T: DeserializeOwned>(
        &self,
        url: &str,
        parameters: &[(&str, String)],
    ) -> Result<T, SourceError> {
        let mut request = self
            .agent
            .get(url)
            .header("Authorization", &self.authorization);
        for (name, value) in parameters {
            request = request.query(*name, value);
        }
        let mut response = request
            .call()
            .map_err(|error| SourceError::Request(error.to_string()))?;
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
}

impl DataSource for GrafanaSource {
    fn instant_query(&self, query: &str) -> Result<Vec<Sample>, SourceError> {
        let response: PrometheusResponse<VectorResult> = self.get_json(
            &self.prometheus_url("query"),
            &[("query", query.to_string())],
        )?;
        response
            .into_data()?
            .into_iter()
            .map(VectorResult::into_sample)
            .collect()
    }

    fn range_query(&self, query: &str, range: TimeRange) -> Result<Vec<Series>, SourceError> {
        let parameters = [
            ("query", query.to_string()),
            ("start", range.start.as_second().to_string()),
            ("end", range.end.as_second().to_string()),
            ("step", range.step_seconds.to_string()),
        ];
        let response: PrometheusResponse<MatrixResult> =
            self.get_json(&self.prometheus_url("query_range"), &parameters)?;
        response
            .into_data()?
            .into_iter()
            .map(MatrixResult::into_series)
            .collect()
    }

    fn alerts(&self) -> Result<Vec<Alert>, SourceError> {
        let response: AlertsResponse =
            self.get_json(&format!("{}{ALERTS_PATH}", self.base_url), &[])?;
        Ok(response
            .data
            .alerts
            .into_iter()
            .map(GrafanaAlert::into_alert)
            .collect())
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
struct PrometheusResponse<T> {
    status: String,
    error: Option<String>,
    data: Option<PrometheusData<T>>,
}

#[derive(Deserialize)]
struct PrometheusData<T> {
    result: Vec<T>,
}

#[derive(Debug, Deserialize)]
struct VectorResult {
    metric: BTreeMap<String, String>,
    value: (f64, String),
}

#[derive(Deserialize)]
struct MatrixResult {
    metric: BTreeMap<String, String>,
    values: Vec<(f64, String)>,
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

impl<T> PrometheusResponse<T> {
    fn into_data(self) -> Result<Vec<T>, SourceError> {
        match self.data {
            Some(data) if self.status == "success" => Ok(data.result),
            _ => Err(SourceError::Response(self.error.unwrap_or(self.status))),
        }
    }
}

impl VectorResult {
    fn into_sample(self) -> Result<Sample, SourceError> {
        Ok(Sample {
            labels: self.metric,
            value: parse_sample_value(&self.value.1)?,
        })
    }
}

impl MatrixResult {
    fn into_series(self) -> Result<Series, SourceError> {
        let points = self
            .values
            .iter()
            .map(|(time, value)| Ok((*time, parse_sample_value(value)?)))
            .collect::<Result<_, SourceError>>()?;
        Ok(Series {
            labels: self.metric,
            points,
        })
    }
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

fn parse_sample_value(value: &str) -> Result<f64, SourceError> {
    value
        .parse()
        .map_err(|_| SourceError::Response(format!("invalid sample value \"{value}\"")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_vector_response() {
        let body = r#"{"status":"success","data":{"resultType":"vector","result":[
            {"metric":{"node":"evergreen-terrace"},"value":[1793722065.9,"0.742"]}]}}"#;

        let response: PrometheusResponse<VectorResult> = serde_json::from_str(body).unwrap();
        let samples: Vec<Sample> = response
            .into_data()
            .unwrap()
            .into_iter()
            .map(|result| result.into_sample().unwrap())
            .collect();

        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].labels["node"], "evergreen-terrace");
        assert_eq!(samples[0].value, 0.742);
    }

    #[test]
    fn parses_matrix_response_with_special_values() {
        let body = r#"{"status":"success","data":{"resultType":"matrix","result":[
            {"metric":{},"values":[[1.0,"1"],[2.0,"NaN"],[3.0,"+Inf"]]}]}}"#;

        let response: PrometheusResponse<MatrixResult> = serde_json::from_str(body).unwrap();
        let series = response
            .into_data()
            .unwrap()
            .pop()
            .unwrap()
            .into_series()
            .unwrap();

        assert_eq!(series.points[0], (1.0, 1.0));
        assert!(series.points[1].1.is_nan());
        assert!(series.points[2].1.is_infinite());
    }

    #[test]
    fn reports_prometheus_errors() {
        let body = r#"{"status":"error","errorType":"bad_data","error":"parse error: D'oh!"}"#;

        let response: PrometheusResponse<VectorResult> = serde_json::from_str(body).unwrap();

        assert_eq!(
            response.into_data().unwrap_err(),
            SourceError::Response("parse error: D'oh!".into())
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
