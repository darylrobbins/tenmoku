//! The command lines the app's terminals have run, for the live prompt's
//! faint suggestion. In memory only: a command line can hold a secret, so
//! nothing here is written to disk.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};

/// The most command lines kept; the oldest are dropped.
pub const CAP: usize = 2000;

/// Command lines, newest first, each once. A handle: clones share the
/// lines, so every terminal of the app sees the others' commands.
#[derive(Clone, Debug, Default)]
pub struct History {
    lines: Arc<Mutex<VecDeque<String>>>,
}

impl History {
    pub fn new() -> Self {
        Self::default()
    }

    /// Remembers `line` as the newest. Skipped: a blank line, one starting
    /// with a space (as `HISTCONTROL=ignorespace`) and one of several lines.
    /// An earlier copy of it is dropped.
    pub fn record(&self, line: &str) {
        let line = line.trim_end();
        if line.trim().is_empty() || line.starts_with(' ') || line.contains(['\n', '\r']) {
            return;
        }
        let mut lines = self.lock();
        if let Some(at) = lines.iter().position(|kept| kept == line) {
            lines.remove(at);
        }
        lines.push_front(line.to_owned());
        lines.truncate(CAP);
    }

    /// The newest line that starts with `typed` and is longer, whole.
    pub fn newest_match(&self, typed: &str) -> Option<String> {
        if typed.is_empty() {
            return None;
        }
        let lines = self.lock();
        lines
            .iter()
            .find(|line| line.len() > typed.len() && line.starts_with(typed))
            .cloned()
    }

    /// What follows `typed` in [`History::newest_match`].
    pub fn remainder(&self, typed: &str) -> Option<String> {
        let line = self.newest_match(typed)?;
        Some(line[typed.len()..].to_owned())
    }

    /// The lines, newest first.
    pub fn lines(&self) -> Vec<String> {
        self.lock().iter().cloned().collect()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, VecDeque<String>> {
        self.lines.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
