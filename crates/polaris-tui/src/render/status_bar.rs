//! One line footer showing freshness of the data.

use jiff::Timestamp;
use jiff::tz::TimeZone;
use polaris_core::config::Config;
use polaris_core::data::Snapshot;
use polaris_core::style::Level;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};

use crate::palette::{self, MUTED};

/// A snapshot older than this many refresh intervals is flagged as stale.
const STALE_INTERVAL_COUNT: u32 = 3;
const SEPARATOR: &str = " | ";

/// Draws the application name, last update time and error count.
pub fn draw(frame: &mut Frame, area: Rect, config: &Config, snapshot: Option<&Snapshot>) {
    let mut spans = vec![Span::from(" polaris").bold()];
    if let Some(snapshot) = snapshot {
        spans.push(Span::from(SEPARATOR).fg(MUTED));
        spans.push(updated_span(snapshot, config));
        let error_count = snapshot.error_count();
        if error_count > 0 {
            spans.push(Span::from(SEPARATOR).fg(MUTED));
            spans.push(
                Span::from(format!("{error_count} widget errors"))
                    .fg(palette::level_color(Level::Critical)),
            );
        }
    }
    frame.render_widget(Line::from(spans).style(Style::new()), area);
}

fn updated_span(snapshot: &Snapshot, config: &Config) -> Span<'static> {
    let local_time = snapshot
        .fetched_at
        .to_zoned(TimeZone::system())
        .strftime("%H:%M:%S")
        .to_string();
    let stale_after = config.refresh.interval * STALE_INTERVAL_COUNT;
    let is_stale = Timestamp::now()
        .duration_since(snapshot.fetched_at)
        .unsigned_abs()
        > stale_after;
    if is_stale {
        Span::from(format!("STALE since {local_time}"))
            .fg(palette::level_color(Level::Critical))
            .bold()
    } else {
        Span::from(format!("updated {local_time}")).fg(MUTED)
    }
}
