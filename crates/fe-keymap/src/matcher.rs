//! Matching keystrokes as they're pressed against a keymap, including
//! chords such as `cmd-k cmd-0`.

use std::sync::Arc;

use fe_command::{CommandId, ContextStack, Invocation};

use crate::{
    keymap::{Binding, Keymap},
    keystroke::Keystroke,
};

/// What a keystroke did.
#[derive(Clone, Debug, PartialEq)]
pub enum KeyOutcome {
    /// The keys pressed so far match a binding exactly, and no longer
    /// binding in the context starts with them: run it. Its source is
    /// [`fe_command::Source::Keyboard`] and its context the stack.
    Dispatch(Invocation),
    /// The keys pressed so far start a longer binding: wait for the next
    /// keystroke, or for the host's chord timer to call [`Matcher::flush`].
    Pending,
    /// Nothing matches. These are every keystroke swallowed while pending
    /// plus this one, for the host to deliver as ordinary input.
    Unmatched(Vec<Keystroke>),
}

/// Matches keystrokes against a [`Keymap`], remembering the ones pressed
/// so far of a chord. It has no clock: the host times chords out by calling
/// [`Matcher::flush`].
#[derive(Clone, Debug, Default)]
pub struct Matcher {
    keymap: Arc<Keymap>,
    /// The keystrokes of a chord in progress.
    pending: Vec<Keystroke>,
    /// What the best binding matching a prefix of `pending` exactly would
    /// run, as of the latest keystroke and its stack, for [`Matcher::flush`].
    exact: Option<Invocation>,
}

impl Matcher {
    pub fn new(keymap: impl Into<Arc<Keymap>>) -> Self {
        Self {
            keymap: keymap.into(),
            ..Self::default()
        }
    }

    pub fn keymap(&self) -> &Arc<Keymap> {
        &self.keymap
    }

    /// Swaps the keymap, as on hot reload, dropping any chord in progress.
    pub fn set_keymap(&mut self, keymap: impl Into<Arc<Keymap>>) {
        self.keymap = keymap.into();
        self.clear();
    }

    /// The keystrokes of a chord in progress.
    pub fn pending(&self) -> &[Keystroke] {
        &self.pending
    }

    /// The bindings that run `command`: see [`Keymap::bindings_for`].
    pub fn bindings_for(&self, command: &CommandId) -> Vec<&Binding> {
        self.keymap.bindings_for(command)
    }

    /// Adds `keystroke` to the keys pressed so far and matches them against
    /// the bindings whose context holds in `stack`. Among bindings for the
    /// same keys, the deepest context match wins (see [`Binding::depth`]),
    /// then the later layer, then the later entry.
    ///
    /// While a chord is pending, the best exact match for it is worked out
    /// again on every keystroke, in that keystroke's stack: the longest
    /// prefix of the keys pressed that a binding matches, then precedence.
    /// So a context that changes mid-chord is honoured, and the invocation
    /// [`Matcher::flush`] dispatches carries the latest stack.
    pub fn push(&mut self, keystroke: Keystroke, stack: &ContextStack) -> KeyOutcome {
        self.pending.push(keystroke);
        // The best exact match of a prefix, as (its length, depth, index).
        let mut exact: Option<(usize, usize, usize)> = None;
        let mut longer = false;
        for (ix, binding) in self.keymap.bindings().iter().enumerate() {
            let keys = binding.keys.keystrokes();
            let matches_prefix = keys.len() <= self.pending.len() && self.pending.starts_with(keys);
            let extends = keys.len() > self.pending.len() && keys.starts_with(&self.pending);
            if !matches_prefix && !extends {
                continue;
            }
            let Some(depth) = binding.depth(stack) else {
                continue;
            };
            if extends {
                longer = true;
            } else if exact.is_none_or(|best| (keys.len(), depth, ix) > best) {
                exact = Some((keys.len(), depth, ix));
            }
        }
        let invocation = |ix: usize| self.keymap.bindings()[ix].invocation(stack);
        if longer {
            self.exact = exact.map(|(_, _, ix)| invocation(ix));
            return KeyOutcome::Pending;
        }
        let outcome = match exact {
            Some((len, _, ix)) if len == self.pending.len() => KeyOutcome::Dispatch(invocation(ix)),
            _ => KeyOutcome::Unmatched(std::mem::take(&mut self.pending)),
        };
        self.clear();
        outcome
    }

    /// Ends a chord in progress, as when the host's chord timer fires:
    /// dispatches the best exact match of a prefix of the keys pressed (see
    /// [`Matcher::push`]), or hands them all back as `Unmatched`, which is
    /// `Unmatched([])` if nothing is pending. Keys pressed after that prefix
    /// are dropped with it.
    pub fn flush(&mut self) -> KeyOutcome {
        let outcome = match self.exact.take() {
            Some(invocation) => KeyOutcome::Dispatch(invocation),
            None => KeyOutcome::Unmatched(std::mem::take(&mut self.pending)),
        };
        self.clear();
        outcome
    }

    fn clear(&mut self) {
        self.pending.clear();
        self.exact = None;
    }
}
