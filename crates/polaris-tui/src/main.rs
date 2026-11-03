//! Polaris terminal dashboard.
//!
//! Usage: `polaris-tui <config.toml>` to run, `polaris-tui --check
//! <config.toml>` to only validate the configuration.

mod app;
mod palette;
mod render;

use std::path::PathBuf;
use std::process::ExitCode;

use polaris_core::config::Config;
use polaris_core::poller::spawn_poller;
use polaris_core::source::{GrafanaSource, read_token};

const CHECK_FLAG: &str = "--check";
const USAGE: &str = "usage: polaris-tui [--check] <config.toml>";

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("polaris-tui: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(arguments: Vec<String>) -> Result<(), String> {
    let check_only = arguments.iter().any(|argument| argument == CHECK_FLAG);
    let config_path = arguments
        .iter()
        .find(|argument| *argument != CHECK_FLAG)
        .map(PathBuf::from)
        .ok_or(USAGE)?;
    let config = Config::load(&config_path).map_err(|error| error.to_string())?;
    if check_only {
        println!(
            "{} is valid ({} widgets)",
            config_path.display(),
            config.widgets.len()
        );
        return Ok(());
    }
    let token = read_token(&config.grafana).map_err(|error| error.to_string())?;
    let source = GrafanaSource::new(&config.grafana, &token);
    let (snapshots, _poller) =
        spawn_poller(config.widgets.clone(), source, config.refresh.interval);
    app::run(&config, &snapshots).map_err(|error| error.to_string())
}
