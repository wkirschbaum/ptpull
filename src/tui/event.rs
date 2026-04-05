use std::time::Duration;

use crossterm::event::{self, Event as CrosstermEvent, KeyEvent};
use tokio::sync::mpsc;

/// Application events
#[derive(Debug)]
pub enum Event {
    /// Terminal key event
    Key(KeyEvent),
    /// Terminal resize
    Resize(u16, u16),
    /// Tick for animations/spinners
    Tick,
}

/// Event reader that bridges crossterm events into an async channel
pub struct EventReader {
    rx: mpsc::UnboundedReceiver<Event>,
}

impl EventReader {
    /// Spawn an event reader with a tick rate
    pub fn new(tick_rate: Duration) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();

        tokio::spawn(async move {
            loop {
                let has_event =
                    tokio::task::spawn_blocking(move || event::poll(tick_rate).unwrap_or(false))
                        .await
                        .unwrap_or(false);

                if has_event {
                    let event = tokio::task::spawn_blocking(|| event::read().ok())
                        .await
                        .unwrap_or(None);

                    if let Some(evt) = event {
                        let app_event = match evt {
                            CrosstermEvent::Key(key) => Event::Key(key),
                            CrosstermEvent::Resize(w, h) => Event::Resize(w, h),
                            _ => continue,
                        };
                        if tx.send(app_event).is_err() {
                            break;
                        }
                    }
                } else {
                    // Tick
                    if tx.send(Event::Tick).is_err() {
                        break;
                    }
                }
            }
        });

        Self { rx }
    }

    /// Receive the next event
    pub async fn next(&mut self) -> Option<Event> {
        self.rx.recv().await
    }
}
