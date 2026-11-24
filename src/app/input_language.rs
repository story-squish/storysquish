use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use eframe::egui;

use crate::keyboard;

/// Checked only while the app repaints, i.e. while someone is using it, so an
/// idle window never polls.
const RECHECK_INTERVAL: Duration = Duration::from_secs(1);

pub type DetectDictionary = Arc<dyn Fn() -> String + Send + Sync>;

pub(super) struct InputLanguage {
    detect: DetectDictionary,
    dictionary: Option<String>,
    checked_at: Option<Instant>,
    pending: Option<Receiver<String>>,
}

impl Default for InputLanguage {
    fn default() -> Self {
        Self::new(Arc::new(keyboard::current_input_dictionary))
    }
}

impl InputLanguage {
    pub fn new(detect: DetectDictionary) -> Self {
        Self {
            detect,
            dictionary: None,
            checked_at: None,
            pending: None,
        }
    }

    pub fn dictionary(&self) -> Option<&str> {
        self.dictionary.as_deref()
    }

    /// Detects off the UI thread, since reading the layout can start a
    /// process. Returns whether the dictionary changed.
    pub fn poll(&mut self, ctx: &egui::Context) -> bool {
        if let Some(receiver) = &self.pending {
            let detected = match receiver.try_recv() {
                Ok(detected) => Some(detected),
                Err(TryRecvError::Empty) => return false,
                Err(TryRecvError::Disconnected) => None,
            };
            self.pending = None;
            return match detected {
                Some(id) if self.dictionary.as_deref() != Some(id.as_str()) => {
                    self.dictionary = Some(id);
                    true
                }
                _ => false,
            };
        }
        if self
            .checked_at
            .is_some_and(|checked| checked.elapsed() < RECHECK_INTERVAL)
        {
            return false;
        }
        self.checked_at = Some(Instant::now());
        let (sender, receiver) = mpsc::channel();
        self.pending = Some(receiver);
        let detect = Arc::clone(&self.detect);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            if sender.send(detect()).is_ok() {
                ctx.request_repaint();
            }
        });
        false
    }
}
