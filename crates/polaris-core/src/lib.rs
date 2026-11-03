//! Renderer-agnostic core of Polaris.
//!
//! Holds everything a renderer (TUI today, GUI later) needs without knowing
//! how pixels or cells are drawn: configuration, data acquisition from
//! Grafana, the resulting data model and value formatting.

pub mod config;
pub mod data;
pub mod fetch;
pub mod poller;
pub mod resample;
pub mod source;
pub mod style;
pub mod unit;
pub mod widget;
