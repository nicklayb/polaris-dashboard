//! Background refresh loop feeding renderers with snapshots.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use jiff::Timestamp;

use crate::data::Snapshot;
use crate::refresh::WidgetRefresher;
use crate::source::DataSource;
use crate::widget::Widget;

/// Spawns a thread producing a snapshot each `interval`.
///
/// Each snapshot only refetches the widgets that are due: `interval` for
/// widgets without their own `refresh`, the widget `refresh` otherwise.
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
    let refresher = WidgetRefresher::new(widgets, interval);
    let handle = thread::spawn(move || poll_forever(refresher, &source, interval, &sender));
    (receiver, handle)
}

fn poll_forever(
    mut refresher: WidgetRefresher,
    source: &dyn DataSource,
    interval: Duration,
    sender: &Sender<Snapshot>,
) {
    loop {
        let snapshot = refresher.refresh(source, Timestamp::now());
        if sender.send(snapshot).is_err() {
            return;
        }
        thread::sleep(interval);
    }
}
