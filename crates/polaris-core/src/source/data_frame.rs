//! Parsing of the data frames returned by Grafana's `POST /api/ds/query`.
//!
//! Prometheus results come back as one frame per series, holding a `time`
//! field (epoch milliseconds) and a `number` field carrying the series
//! labels. JSON cannot hold NaN or infinities: Grafana writes `null` and
//! lists the affected row indices in `data.entities`.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

use super::SourceError;
use crate::data::{Labels, Sample, Series};

const TIME_FIELD_TYPE: &str = "time";
const NUMBER_FIELD_TYPE: &str = "number";
const MILLISECONDS_PER_SECOND: f64 = 1000.0;

/// Body of a `/api/ds/query` response: one result per query `refId`.
#[derive(Debug, Deserialize)]
pub struct QueryDataResponse {
    pub results: BTreeMap<String, QueryResult>,
}

/// Outcome of one query: an error message or data frames.
#[derive(Debug, Deserialize)]
pub struct QueryResult {
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    frames: Vec<DataFrame>,
}

#[derive(Debug, Deserialize)]
struct DataFrame {
    schema: FrameSchema,
    #[serde(default)]
    data: Option<FrameData>,
}

#[derive(Debug, Deserialize)]
struct FrameSchema {
    #[serde(default)]
    fields: Vec<FieldSchema>,
}

#[derive(Debug, Deserialize)]
struct FieldSchema {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    labels: Option<Labels>,
}

#[derive(Debug, Default, Deserialize)]
struct FrameData {
    #[serde(default)]
    values: Vec<Vec<Value>>,
    #[serde(default)]
    entities: Vec<Option<SpecialValues>>,
}

/// Row indices of a field holding values JSON cannot represent.
#[derive(Debug, Default, Deserialize)]
struct SpecialValues {
    #[serde(rename = "NaN", default)]
    not_a_number: Vec<usize>,
    #[serde(rename = "Inf", default)]
    positive_infinity: Vec<usize>,
    #[serde(rename = "NegInf", default)]
    negative_infinity: Vec<usize>,
}

/// A labelled column of `(unix seconds, value)` points read from a frame.
struct FrameSeries {
    labels: Labels,
    points: Vec<(f64, f64)>,
}

impl QueryResult {
    /// Reads the result of an instant query: the latest value of every
    /// series.
    pub fn into_samples(self) -> Result<Vec<Sample>, SourceError> {
        Ok(self
            .into_frame_series()?
            .into_iter()
            .filter_map(|series| {
                let (_, value) = *series.points.last()?;
                Some(Sample {
                    labels: series.labels,
                    value,
                })
            })
            .collect())
    }

    /// Reads the result of a range query.
    pub fn into_series(self) -> Result<Vec<Series>, SourceError> {
        Ok(self
            .into_frame_series()?
            .into_iter()
            .map(|series| Series {
                labels: series.labels,
                points: series.points,
            })
            .collect())
    }

    fn into_frame_series(self) -> Result<Vec<FrameSeries>, SourceError> {
        if let Some(error) = self.error {
            return Err(SourceError::Response(error));
        }
        Ok(self
            .frames
            .into_iter()
            .flat_map(DataFrame::into_frame_series)
            .collect())
    }
}

impl DataFrame {
    fn into_frame_series(self) -> Vec<FrameSeries> {
        let data = self.data.unwrap_or_default();
        let fields = &self.schema.fields;
        let Some(time_index) = fields
            .iter()
            .position(|field| field.kind == TIME_FIELD_TYPE)
        else {
            return Vec::new();
        };
        let times = column_seconds(&data, time_index);
        fields
            .iter()
            .enumerate()
            .filter(|(_, field)| field.kind == NUMBER_FIELD_TYPE)
            .map(|(index, field)| FrameSeries {
                labels: field.labels.clone().unwrap_or_default(),
                points: column_points(&data, index, &times),
            })
            .collect()
    }
}

fn column_seconds(data: &FrameData, index: usize) -> Vec<Option<f64>> {
    data.values
        .get(index)
        .map(|values| {
            values
                .iter()
                .map(|value| Some(value.as_f64()? / MILLISECONDS_PER_SECOND))
                .collect()
        })
        .unwrap_or_default()
}

/// Pairs each value with its time, skipping rows missing either.
fn column_points(data: &FrameData, index: usize, times: &[Option<f64>]) -> Vec<(f64, f64)> {
    let Some(values) = data.values.get(index) else {
        return Vec::new();
    };
    let special_values = data.entities.get(index).and_then(Option::as_ref);
    values
        .iter()
        .zip(times)
        .enumerate()
        .filter_map(|(row, (value, time))| {
            let value = value
                .as_f64()
                .or_else(|| special_value(special_values?, row))?;
            Some(((*time)?, value))
        })
        .collect()
}

fn special_value(special_values: &SpecialValues, row: usize) -> Option<f64> {
    if special_values.not_a_number.contains(&row) {
        Some(f64::NAN)
    } else if special_values.positive_infinity.contains(&row) {
        Some(f64::INFINITY)
    } else if special_values.negative_infinity.contains(&row) {
        Some(f64::NEG_INFINITY)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shape captured from Grafana 13.2 for an instant query.
    const INSTANT_RESPONSE: &str = r#"{"results":{"Q0":{"status":200,"frames":[
        {"schema":{"refId":"Q0","meta":{"type":"numeric-multi"},"fields":[
            {"name":"Time","type":"time","typeInfo":{"frame":"time.Time"}},
            {"name":"Value","type":"number","labels":{"node":"evergreen-terrace"}}]},
         "data":{"values":[[1795121511037],[0.742]]}}]}}}"#;

    /// Shape captured from Grafana 13.2 for a range query with NaN and a
    /// missing point.
    const RANGE_RESPONSE: &str = r#"{"results":{"Q0":{"status":200,"frames":[
        {"schema":{"refId":"Q0","fields":[
            {"name":"Time","type":"time"},
            {"name":"Value","type":"number","labels":{"node":"moes-tavern"}}]},
         "data":{"values":[[1000,2000,3000,4000],[1.5,null,null,null]],
                 "entities":[null,{"NaN":[1],"Inf":[2]}]}}]}}}"#;

    const ERROR_RESPONSE: &str = r#"{"results":{"Q0":{"status":400,
        "error":"bad_data: parse error: D'oh!",
        "frames":[{"schema":{"refId":"Q0","fields":[]},"data":{"values":[]}}]}}}"#;

    const EMPTY_RESPONSE: &str = r#"{"results":{"Q0":{"status":200,
        "frames":[{"schema":{"refId":"Q0","fields":[]},"data":{"values":[]}}]}}}"#;

    fn first_result(body: &str) -> QueryResult {
        let mut response: QueryDataResponse = serde_json::from_str(body).unwrap();
        response.results.remove("Q0").unwrap()
    }

    #[test]
    fn reads_instant_samples() {
        let samples = first_result(INSTANT_RESPONSE).into_samples().unwrap();

        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].labels["node"], "evergreen-terrace");
        assert_eq!(samples[0].value, 0.742);
    }

    #[test]
    fn reads_range_series_with_special_values() {
        let series = first_result(RANGE_RESPONSE).into_series().unwrap();

        let points = &series[0].points;
        assert_eq!(series[0].labels["node"], "moes-tavern");
        assert_eq!(points.len(), 3);
        assert_eq!(points[0], (1.0, 1.5));
        assert!(points[1].1.is_nan());
        assert_eq!(points[2], (3.0, f64::INFINITY));
    }

    #[test]
    fn reports_query_errors() {
        assert_eq!(
            first_result(ERROR_RESPONSE).into_samples().unwrap_err(),
            SourceError::Response("bad_data: parse error: D'oh!".into())
        );
    }

    #[test]
    fn empty_frames_have_no_data() {
        assert_eq!(first_result(EMPTY_RESPONSE).into_series().unwrap(), []);
    }
}
