//! Data sources: the only place where Polaris talks to the network.
//!
//! Everything above this module depends on the [`DataSource`] trait so that
//! fetching logic is tested against in-memory fakes.

mod grafana;

use jiff::Timestamp;

pub use grafana::{GrafanaSource, read_token};

use crate::data::{Alert, Sample, Series};

/// Time window and resolution of a range query.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimeRange {
    pub start: Timestamp,
    pub end: Timestamp,
    pub step_seconds: f64,
}

/// Failure while acquiring data.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SourceError {
    #[error("request failed: {0}")]
    Request(String),
    #[error("unexpected response: {0}")]
    Response(String),
}

/// Provides metrics and alerts to widgets.
pub trait DataSource {
    /// Evaluates a PromQL expression at the current time.
    fn instant_query(&self, query: &str) -> Result<Vec<Sample>, SourceError>;

    /// Evaluates a PromQL expression over `range`.
    fn range_query(&self, query: &str, range: TimeRange) -> Result<Vec<Series>, SourceError>;

    /// Lists every alert instance with its current state.
    fn alerts(&self) -> Result<Vec<Alert>, SourceError>;
}
