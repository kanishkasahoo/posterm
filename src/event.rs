use std::time::Duration;

use crossterm::event::{Event as CrosstermEvent, EventStream, KeyEvent, KeyEventKind};
use futures_util::StreamExt;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Key(KeyEvent),
    Tick,
    Render,
    Resize(u16, u16),
}

pub struct EventHandler {
    receiver: mpsc::Receiver<Event>,
    shutdown_sender: watch::Sender<bool>,
    tasks: Vec<JoinHandle<()>>,
}

impl EventHandler {
    pub fn new(tick_rate: Duration, render_rate: Duration) -> Self {
        const EVENT_CHANNEL_CAPACITY: usize = 256;

        let (sender, receiver) = mpsc::channel(EVENT_CHANNEL_CAPACITY);
        let (shutdown_sender, shutdown_receiver) = watch::channel(false);

        let input_sender = sender.clone();
        let mut input_shutdown = shutdown_receiver.clone();
        let input_task = tokio::spawn(async move {
            let mut event_stream = EventStream::new();

            loop {
                tokio::select! {
                    changed = input_shutdown.changed() => {
                        if changed.is_ok() {
                            break;
                        }
                    }
                    next_event = event_stream.next() => {
                        let Some(next_event) = next_event else {
                            break;
                        };

                        match next_event {
                            Ok(CrosstermEvent::Key(key_event)) if key_event.kind == KeyEventKind::Press => {
                                if input_sender.send(Event::Key(key_event)).await.is_err() {
                                    break;
                                }
                            }
                            Ok(CrosstermEvent::Resize(width, height)) => {
                                if input_sender.send(Event::Resize(width, height)).await.is_err() {
                                    break;
                                }
                            }
                            Ok(_) => {}
                            Err(_) => break,
                        }
                    }
                }
            }
        });

        let tick_sender = sender.clone();
        let mut tick_shutdown = shutdown_receiver.clone();
        let tick_task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(tick_rate);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                tokio::select! {
                    changed = tick_shutdown.changed() => {
                        if changed.is_ok() {
                            break;
                        }
                    }
                    _ = interval.tick() => {
                        match tick_sender.try_send(Event::Tick) {
                            Ok(()) => {}
                            Err(mpsc::error::TrySendError::Full(_)) => {}
                            Err(mpsc::error::TrySendError::Closed(_)) => break,
                        }
                    }
                }
            }
        });

        let render_sender = sender;
        let mut render_shutdown = shutdown_receiver;
        let render_task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(render_rate);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                tokio::select! {
                    changed = render_shutdown.changed() => {
                        if changed.is_ok() {
                            break;
                        }
                    }
                    _ = interval.tick() => {
                        match render_sender.try_send(Event::Render) {
                            Ok(()) => {}
                            Err(mpsc::error::TrySendError::Full(_)) => {}
                            Err(mpsc::error::TrySendError::Closed(_)) => break,
                        }
                    }
                }
            }
        });

        Self {
            receiver,
            shutdown_sender,
            tasks: vec![input_task, tick_task, render_task],
        }
    }

    pub async fn next(&mut self) -> Option<Event> {
        self.receiver.recv().await
    }
}

impl Drop for EventHandler {
    fn drop(&mut self) {
        let _ = self.shutdown_sender.send(true);
        for task in &self.tasks {
            task.abort();
        }
    }
}
