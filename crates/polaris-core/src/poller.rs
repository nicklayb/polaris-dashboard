//! Background refresh loop feeding renderers with snapshots.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use jiff::Timestamp;

use crate::data::Snapshot;
use crate::fetch::fetch_snapshot;
use crate::source::DataSource;
use crate::widget::Widget;

/// Spawns a thread fetching every widget each `interval`.
///
/// Returns the receiving end of the snapshot channel; the thread stops once
/// the receiver is dropped.
pub fn spawn_poller<S>(
    widgets: Vec<Widget>,
    source: S,
    interval: Duration,
) -> (Receiver<Snapshot>, JoinHandle<()>)
where
    S: DataSource + Send + 'static,
{
    let (sender, receiver) = channel();
    let handle = thread::spawn(move || poll_forever(&widgets, &source, interval, &sender));
    (receiver, handle)
}

fn poll_forever(
    widgets: &[Widget],
    source: &dyn DataSource,
    interval: Duration,
    sender: &Sender<Snapshot>,
) {
    loop {
        let snapshot = fetch_snapshot(widgets, source, Timestamp::now());
        if sender.send(snapshot).is_err() {
            return;
        }
        thread::sleep(interval);
    }
}
