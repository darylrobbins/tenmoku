//! When to auto-save: a policy over edit, focus, window, insert-mode and save
//! events, following VS Code's EditorAutoSave. It reads no clock; callers pass
//! `Instant`s in and run their own timer from [`AutoSavePolicy::next_deadline`].
//! Vim insert sessions are never saved midway, so `.` repeat and the single
//! undo step of an insert survive.

use std::time::{Duration, Instant};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AutoSaveMode {
    Off,
    AfterDelay(Duration),
    OnFocusChange,
    OnWindowChange,
    OnInsertLeave,
}

/// What the caller knows about a buffer at an edit.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Facts {
    pub has_path: bool,
    pub read_only: bool,
    /// Vim is in insert mode (or replace) as the edit lands.
    pub vim_insert: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SaveReason {
    Explicit,
    AutoDelay,
    AutoFocus,
    AutoWindow,
    AutoInsertLeave,
}

impl SaveReason {
    /// Whether save participants (format on save, trimming) may edit the
    /// buffer. VS Code runs none for `afterDelay`; none run mid-insert either.
    pub fn edits_allowed(self, vim_insert: bool) -> bool {
        match self {
            SaveReason::Explicit => true,
            SaveReason::AutoDelay => false,
            _ => !vim_insert,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Hold {
    None,
    /// Save failed because the file changed on disk: until saved or reloaded.
    Conflict,
    /// Any other failure: until the next edit.
    UntilEdit,
}

struct State<K> {
    key: K,
    dirty: bool,
    has_path: bool,
    read_only: bool,
    in_insert: bool,
    vim_seen: bool,
    anchor: Option<Instant>,
    deadline: Option<Instant>,
    ready: bool,
    /// Returned from `due` since the last edit; not returned again.
    handed: bool,
    hold: Hold,
}

impl<K> State<K> {
    fn eligible(&self) -> bool {
        self.dirty && self.has_path && !self.read_only && self.hold == Hold::None && !self.handed
    }
}

pub struct AutoSavePolicy<K> {
    mode: AutoSaveMode,
    buffers: Vec<State<K>>,
}

impl<K: Copy + Eq + std::hash::Hash> AutoSavePolicy<K> {
    pub fn new(mode: AutoSaveMode) -> Self {
        Self {
            mode,
            buffers: Vec::new(),
        }
    }

    pub fn mode(&self) -> AutoSaveMode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: AutoSaveMode) {
        self.mode = mode;
        for s in &mut self.buffers {
            s.deadline = None;
            s.ready = false;
            if let AutoSaveMode::AfterDelay(d) = mode
                && s.dirty
                && !s.in_insert
                && let Some(t) = s.anchor
            {
                s.deadline = Some(t + d);
            }
        }
    }

    fn state(&mut self, key: K) -> &mut State<K> {
        let i = match self.buffers.iter().position(|s| s.key == key) {
            Some(i) => i,
            None => {
                self.buffers.push(State {
                    key,
                    dirty: false,
                    has_path: false,
                    read_only: false,
                    in_insert: false,
                    vim_seen: false,
                    anchor: None,
                    deadline: None,
                    ready: false,
                    handed: false,
                    hold: Hold::None,
                });
                self.buffers.len() - 1
            }
        };
        &mut self.buffers[i]
    }

    pub fn edited(&mut self, key: K, now: Instant, facts: Facts) {
        let mode = self.mode;
        let s = self.state(key);
        s.dirty = true;
        s.has_path = facts.has_path;
        s.read_only = facts.read_only;
        s.in_insert = facts.vim_insert;
        s.vim_seen |= facts.vim_insert;
        s.anchor = Some(now);
        s.handed = false;
        s.ready = false;
        s.deadline = None;
        if s.hold == Hold::UntilEdit {
            s.hold = Hold::None;
        }
        match mode {
            AutoSaveMode::AfterDelay(d) if !facts.vim_insert => s.deadline = Some(now + d),
            AutoSaveMode::OnInsertLeave if s.vim_seen && !facts.vim_insert => s.ready = true,
            _ => {}
        }
    }

    pub fn insert_left(&mut self, key: K, now: Instant) {
        let mode = self.mode;
        let s = self.state(key);
        s.in_insert = false;
        s.vim_seen = true;
        if !s.dirty {
            return;
        }
        s.anchor = Some(now);
        match mode {
            AutoSaveMode::AfterDelay(d) => s.deadline = Some(now + d),
            AutoSaveMode::OnInsertLeave => s.ready = true,
            _ => {}
        }
    }

    pub fn focus_lost(&mut self, key: K) {
        let mode = self.mode;
        let s = self.state(key);
        let vim_off = !s.vim_seen;
        if s.dirty
            && (mode == AutoSaveMode::OnFocusChange
                || (mode == AutoSaveMode::OnInsertLeave && vim_off))
        {
            s.ready = true;
        }
    }

    pub fn window_blurred(&mut self) {
        let mode = self.mode;
        for s in self.buffers.iter_mut().filter(|s| s.dirty) {
            if matches!(
                mode,
                AutoSaveMode::OnFocusChange | AutoSaveMode::OnWindowChange
            ) || (mode == AutoSaveMode::OnInsertLeave && !s.vim_seen)
            {
                s.ready = true;
            }
        }
    }

    /// A save succeeded, explicit or automatic.
    pub fn saved(&mut self, key: K) {
        self.clean(key);
    }

    /// The buffer was reloaded from disk and matches it again.
    pub fn reloaded(&mut self, key: K) {
        self.clean(key);
    }

    fn clean(&mut self, key: K) {
        let s = self.state(key);
        s.dirty = false;
        s.hold = Hold::None;
        s.handed = false;
        s.ready = false;
        s.deadline = None;
    }

    pub fn save_failed(&mut self, key: K, conflict: bool) {
        let s = self.state(key);
        if conflict {
            s.hold = Hold::Conflict;
        } else if s.hold != Hold::Conflict {
            s.hold = Hold::UntilEdit;
        }
        s.ready = false;
        s.deadline = None;
    }

    pub fn closed(&mut self, key: K) {
        self.buffers.retain(|s| s.key != key);
    }

    /// Buffers to save now. Each is returned once until edited again.
    pub fn due(&mut self, now: Instant) -> Vec<K> {
        let mut out = Vec::new();
        for s in &mut self.buffers {
            if s.eligible() && (s.ready || s.deadline.is_some_and(|d| d <= now)) {
                s.ready = false;
                s.deadline = None;
                s.handed = true;
                out.push(s.key);
            }
        }
        out
    }

    /// When the caller's timer should next call `due`.
    pub fn next_deadline(&self) -> Option<Instant> {
        self.buffers
            .iter()
            .filter(|s| s.eligible())
            .filter_map(|s| s.deadline)
            .min()
    }
}
