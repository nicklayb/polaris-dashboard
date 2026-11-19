//! Data sources: the only place where Polaris talks to the network.
//!
//! Everything above this module depends on the [`DataSource`] trait so that
//! fetching logic is tested against in-memory fakes.

mod data_frame;
mod dns;
mod grafana;
mod query_batch;

use jiff::Timestamp;

pub use dns::{CachingResolver, is_connection_failure};
pub use grafana::{GrafanaSource, read_token};

use crate::data::{Alert, Sample, Series};

/// Time window and resolution of a range query.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimeRange {
    pub start: Timestamp,
    pub end: Timestamp,
    pub step_seconds: f64,
}

/// A PromQL evaluation. Two equal queries always produce the same output
/// within a refresh, which is what deduplication relies on.
#[derive(Debug, Clone, PartialEq)]
pub enum Query {
    /// Evaluates `expression` at the current time.
    Instant { expression: String },
    /// Evaluates `expression` over `range`.
    Range {
        expression: String,
        range: TimeRange,
    },
}

/// Result of a [`Query`], shaped by its kind.
#[derive(Debug, Clone, PartialEq)]
pub enum QueryOutput {
    /// Output of [`Query::Instant`].
    Samples(Vec<Sample>),
    /// Output of [`Query::Range`].
    Series(Vec<Series>),
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

    /// Evaluates every query, returning one result per query in the same
    /// order. A failing query only fails its own result.
    ///
    /// The default implementation runs the queries one by one; sources able
    /// to send several queries at once override it.
    fn query_batch(&self, queries: &[Query]) -> Vec<Result<QueryOutput, SourceError>> {
        queries
            .iter()
            .map(|query| run_single_query(self, query))
            .collect()
    }
}

fn run_single_query<S: DataSource + ?Sized>(
    source: &S,
    query: &Query,
) -> Result<QueryOutput, SourceError> {
    match query {
        Query::Instant { expression } => source.instant_query(expression).map(QueryOutput::Samples),
        Query::Range { expression, range } => source
            .range_query(expression, *range)
            .map(QueryOutput::Series),
    }
}
