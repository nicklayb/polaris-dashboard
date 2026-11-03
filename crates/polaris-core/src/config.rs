//! Top level configuration file: data access, layout, widgets and
//! renderer-specific sections.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use crate::widget::{Area, Widget, WidgetKind};

const DEFAULT_REFRESH_INTERVAL: Duration = Duration::from_secs(15);
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// The whole dashboard configuration, shared by every renderer.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub grafana: GrafanaConfig,
    #[serde(default)]
    pub refresh: RefreshConfig,
    pub layout: LayoutConfig,
    #[serde(default)]
    pub tui: TuiConfig,
    #[serde(default)]
    pub widgets: Vec<Widget>,
}

/// How to reach Grafana.
#[derive(Debug, Clone, Deserialize)]
pub struct GrafanaConfig {
    pub url: String,
    /// UID of the Prometheus datasource queried through the Grafana proxy.
    pub datasource_uid: String,
    /// File holding a service account token. When absent, the token is read
    /// from the `POLARIS_GRAFANA_TOKEN` environment variable or the
    /// `grafana-token` systemd credential.
    pub token_file: Option<PathBuf>,
    #[serde(default = "default_request_timeout", with = "humantime_serde")]
    pub timeout: Duration,
}

/// How often data is fetched.
#[derive(Debug, Clone, Deserialize)]
pub struct RefreshConfig {
    #[serde(with = "humantime_serde")]
    pub interval: Duration,
}

/// The grid widgets are placed on. Values are relative weights.
#[derive(Debug, Clone, Deserialize)]
pub struct LayoutConfig {
    pub columns: Vec<u16>,
    pub rows: Vec<u16>,
}

/// Options only meaningful to the terminal renderer.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct TuiConfig {
    #[serde(default)]
    pub chart_marker: ChartMarker,
    #[serde(default = "default_true")]
    pub show_status_bar: bool,
}

/// Glyphs used to draw chart lines. The Linux framebuffer console cannot
/// display braille, so the default only relies on block characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChartMarker {
    #[default]
    HalfBlock,
    Block,
    Dot,
    Braille,
}

/// Errors raised while loading or validating a configuration.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("cannot read {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid configuration: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("invalid configuration:\n{}", .0.join("\n"))]
    Invalid(Vec<String>),
}

impl Default for RefreshConfig {
    fn default() -> Self {
        Self {
            interval: DEFAULT_REFRESH_INTERVAL,
        }
    }
}

impl Config {
    /// Reads, parses and validates the configuration file at `path`.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        Self::parse(&content)
    }

    /// Parses and validates a configuration from TOML text.
    pub fn parse(content: &str) -> Result<Self, ConfigError> {
        let config: Self = toml::from_str(content)?;
        let problems = config.validate();
        if problems.is_empty() {
            Ok(config)
        } else {
            Err(ConfigError::Invalid(problems))
        }
    }

    /// Returns every human readable problem found in the configuration.
    pub fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if self.layout.columns.is_empty() || self.layout.rows.is_empty() {
            problems.push("layout needs at least one column and one row".to_string());
        }
        for widget in &self.widgets {
            let widget_problems = area_problem(&widget.area, &self.layout)
                .into_iter()
                .chain(kind_problem(&widget.kind));
            for problem in widget_problems {
                problems.push(format!("widget \"{}\": {problem}", widget.title));
            }
        }
        problems
    }
}

fn area_problem(area: &Area, layout: &LayoutConfig) -> Option<String> {
    if area.column_span == 0 || area.row_span == 0 {
        return Some("spans must be at least 1".to_string());
    }
    if area.column + area.column_span > layout.columns.len() {
        return Some(format!(
            "columns {}..{} exceed the {} grid columns",
            area.column,
            area.column + area.column_span,
            layout.columns.len()
        ));
    }
    if area.row + area.row_span > layout.rows.len() {
        return Some(format!(
            "rows {}..{} exceed the {} grid rows",
            area.row,
            area.row + area.row_span,
            layout.rows.len()
        ));
    }
    None
}

fn kind_problem(kind: &WidgetKind) -> Option<String> {
    let WidgetKind::Table(table) = kind else {
        return None;
    };
    let sort_header = table.sort_by.as_ref()?;
    let is_known_header = table
        .columns
        .iter()
        .any(|column| &column.header == sort_header);
    (!is_known_header).then(|| format!("sort_by \"{sort_header}\" is not a column header"))
}

fn default_request_timeout() -> Duration {
    DEFAULT_REQUEST_TIMEOUT
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPRINGFIELD_CONFIG: &str = r#"
        [grafana]
        url = "http://grafana.springfield.local"
        datasource_uid = "power-plant"

        [layout]
        columns = [1, 1]
        rows = [1]

        [[widgets]]
        type = "stat"
        title = "Donuts eaten"
        area = { column = 0, row = 0 }
        query = "sum(homer_donuts_total)"
        unit = "count"

        [[widgets]]
        type = "alerts"
        title = "Sector 7G"
        area = { column = 1, row = 0 }
    "#;

    #[test]
    fn parses_widgets_with_defaults() {
        let config = Config::parse(SPRINGFIELD_CONFIG).expect("valid configuration");

        assert_eq!(config.refresh.interval, DEFAULT_REFRESH_INTERVAL);
        assert_eq!(config.tui.chart_marker, ChartMarker::HalfBlock);
        assert_eq!(config.widgets.len(), 2);
        assert!(
            matches!(&config.widgets[0].kind, WidgetKind::Stat(stat) if stat.query == "sum(homer_donuts_total)")
        );
        assert!(
            matches!(&config.widgets[1].kind, WidgetKind::Alerts(alerts) if alerts.states == ["Alerting"])
        );
    }

    #[test]
    fn rejects_unknown_sort_column() {
        let content = format!(
            "{SPRINGFIELD_CONFIG}{}",
            r#"
            [[widgets]]
            type = "table"
            title = "Moe's tavern"
            area = { column = 0, row = 0 }
            key_label = "customer"
            key_header = "Customer"
            sort_by = "Tab"

            [[widgets.columns]]
            header = "Beers"
            query = "sum by (customer) (duff_beers_total)"
            "#
        );

        let error = Config::parse(&content).expect_err("sort column does not exist");

        assert!(error.to_string().contains("sort_by \"Tab\""));
    }

    #[test]
    fn rejects_widget_outside_grid() {
        let content = SPRINGFIELD_CONFIG.replace(
            "area = { column = 1, row = 0 }",
            "area = { column = 1, row = 0, column_span = 2 }",
        );

        let error = Config::parse(&content).expect_err("widget overflows the grid");

        assert!(error.to_string().contains("Sector 7G"));
    }
}
