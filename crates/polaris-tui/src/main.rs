//! Polaris terminal dashboard.
//!
//! Usage: `polaris-tui <config.toml>` to run, `polaris-tui --check
//! <config.toml>` to only validate the configuration. With `--hold-errors`,
//! startup errors stay on screen instead of exiting, for kiosk consoles
//! where nobody reads standard error.

mod app;
mod palette;
mod render;

use std::path::PathBuf;
use std::process::ExitCode;

use polaris_core::config::Config;
use polaris_core::poller::spawn_poller;
use polaris_core::source::{GrafanaSource, read_token};

const CHECK_FLAG: &str = "--check";
const HOLD_ERRORS_FLAG: &str = "--hold-errors";
const USAGE: &str = "usage: polaris-tui [--check] [--hold-errors] <config.toml>";

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let Err(message) = run(&arguments) else {
        return ExitCode::SUCCESS;
    };
    eprintln!("polaris-tui: {message}");
    if arguments
        .iter()
        .any(|argument| argument == HOLD_ERRORS_FLAG)
    {
        app::show_error(&message);
    }
    ExitCode::FAILURE
}

fn run(arguments: &[String]) -> Result<(), String> {
    let check_only = arguments.iter().any(|argument| argument == CHECK_FLAG);
    let config_path = arguments
        .iter()
        .find(|argument| !argument.starts_with("--"))
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
