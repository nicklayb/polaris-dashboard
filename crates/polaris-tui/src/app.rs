//! Terminal lifecycle and event loop.

use std::io;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use polaris_core::config::Config;
use polaris_core::data::Snapshot;

use crate::render;

const INPUT_POLL_INTERVAL: Duration = Duration::from_millis(250);

/// Draws the dashboard until the user quits or the poller stops.
pub fn run(config: &Config, snapshots: &Receiver<Snapshot>) -> io::Result<()> {
    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, config, snapshots);
    ratatui::restore();
    result
}

/// Shows `message` full screen until the user quits. Rendering failures are
/// ignored: the message was already written to standard error.
pub fn show_error(message: &str) {
    let mut terminal = ratatui::init();
    let _ = error_loop(&mut terminal, message);
    ratatui::restore();
}

fn error_loop(terminal: &mut ratatui::DefaultTerminal, message: &str) -> io::Result<()> {
    loop {
        terminal.draw(|frame| render::draw_error(frame, message))?;
        if event::poll(INPUT_POLL_INTERVAL)? && is_quit_event(&event::read()?) {
            return Ok(());
        }
    }
}

fn event_loop(
    terminal: &mut ratatui::DefaultTerminal,
    config: &Config,
    snapshots: &Receiver<Snapshot>,
) -> io::Result<()> {
    let mut latest_snapshot: Option<Snapshot> = None;
    loop {
        match snapshots.try_recv() {
            Ok(snapshot) => latest_snapshot = Some(snapshot),
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => return Err(io::Error::other("data poller stopped")),
        }
        terminal.draw(|frame| render::draw_dashboard(frame, config, latest_snapshot.as_ref()))?;
        if event::poll(INPUT_POLL_INTERVAL)? && is_quit_event(&event::read()?) {
            return Ok(());
        }
    }
}

fn is_quit_event(event: &Event) -> bool {
    match event {
        Event::Key(KeyEvent {
            code: KeyCode::Char('q') | KeyCode::Esc,
            ..
        }) => true,
        Event::Key(KeyEvent {
            code: KeyCode::Char('c'),
            modifiers,
            ..
        }) => modifiers.contains(KeyModifiers::CONTROL),
        _ => false,
    }
}
