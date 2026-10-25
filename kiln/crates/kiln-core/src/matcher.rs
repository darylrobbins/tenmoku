// SPDX-License-Identifier: LicenseRef-Proprietary

//! Matching keystrokes as they're pressed against a keymap, including
//! chords such as `cmd-k cmd-0`.

use std::sync::Arc;

use kiln_command::{CommandId, ContextStack, Invocation};

use crate::{
    keymap::{Binding, Keymap},
    keystroke::{Keystroke, Modifiers},
};

/// What a keystroke did.
#[derive(Clone, Debug, PartialEq)]
pub enum KeyOutcome {
    /// The keys pressed so far match a binding exactly, and no longer
    /// binding in the context starts with them: run it. Its source is
    /// [`kiln_command::Source::Keyboard`] and its context the stack.
    Dispatch(Invocation),
    /// The keys pressed so far match a binding exactly, and a longer binding
    /// in a shallower context starts with them too: run the match now, and keep
    /// listening for the rest of the longer one (`escape` closes Find, and
    /// `escape escape` leaves Zen Mode). The prefix is spent: if the chord
    /// then fails, or [`Matcher::flush`] ends it, the prefix is neither run
    /// again nor handed back in `Unmatched`, and the keystroke that failed
    /// it is matched again alone, as a fresh one.
    ///
    /// Otherwise the keys are [`KeyOutcome::Pending`]: a prefix with no
    /// binding of its own, or whose binding is in the longer one's context
    /// or a shallower, or whose last key has a modifier (`ctrl-k`, a chord's
    /// usual opener, whose own binding may move focus and end the chord).
    DispatchPending(Invocation),
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
    /// How many leading keystrokes of `pending` already ran, as
    /// [`KeyOutcome::DispatchPending`].
    spent: usize,
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
        // The deepest context a longer binding starting with the keys is in.
        let mut longer: Option<usize> = None;
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
                longer = longer.max(Some(depth));
            } else if exact.is_none_or(|best| (keys.len(), depth, ix) > best) {
                exact = Some((keys.len(), depth, ix));
            }
        }
        let invocation = |ix: usize| self.keymap.bindings()[ix].invocation(stack);
        if let Some(longer_depth) = longer {
            return match exact {
                // The whole of the keys pressed has a binding of its own,
                // in a deeper context than the longer binding's. Only
                // for a bare key such as `escape`: a modified key such as
                // `ctrl-k` is a chord's usual opener, and running its own
                // binding first (say, moving focus) would end the chord.
                Some((len, depth, ix))
                    if len == self.pending.len()
                        && depth > longer_depth
                        && self.pending[len - 1].modifiers == Modifiers::default() =>
                {
                    self.spent = len;
                    self.exact = None;
                    KeyOutcome::DispatchPending(invocation(ix))
                }
                // A prefix already ran, so only a longer one is flushed.
                Some((len, _, ix)) if len > self.spent => {
                    self.exact = Some(invocation(ix));
                    KeyOutcome::Pending
                }
                _ => {
                    self.exact = None;
                    KeyOutcome::Pending
                }
            };
        }
        let outcome = match exact {
            Some((len, _, ix)) if len == self.pending.len() => KeyOutcome::Dispatch(invocation(ix)),
            _ if self.spent > 0 => {
                // The chord failed after its prefix ran: what follows the
                // prefix goes back, and a lone keystroke is matched afresh.
                let mut rest = self.pending.split_off(self.spent);
                self.clear();
                if rest.len() == 1 {
                    return self.push(rest.remove(0), stack);
                }
                return KeyOutcome::Unmatched(rest);
            }
            _ => KeyOutcome::Unmatched(std::mem::take(&mut self.pending)),
        };
        self.clear();
        outcome
    }

    /// Ends a chord in progress, as when the host's chord timer fires:
    /// dispatches the best exact match of a prefix of the keys pressed (see
    /// [`Matcher::push`]) that hasn't run yet, or hands them all back as
    /// `Unmatched`, less any prefix that ran as `DispatchPending`; that is
    /// `Unmatched([])` if nothing is pending. Keys pressed after that prefix
    /// are dropped with it.
    pub fn flush(&mut self) -> KeyOutcome {
        let spent = self.spent;
        let outcome = match self.exact.take() {
            Some(invocation) => KeyOutcome::Dispatch(invocation),
            None => KeyOutcome::Unmatched(self.pending.split_off(spent.min(self.pending.len()))),
        };
        self.clear();
        outcome
    }

    fn clear(&mut self) {
        self.pending.clear();
        self.exact = None;
        self.spent = 0;
    }
}
