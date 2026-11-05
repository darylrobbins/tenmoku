// SPDX-License-Identifier: Apache-2.0

//! Bindings that hide or delay each other.

use std::fmt;

use kiln_command::{ContextFrame, ContextStack};

use crate::keymap::{Binding, Keymap};

/// A problem [`Keymap::report`] finds between two bindings.
#[derive(Clone, Debug, PartialEq)]
pub enum KeymapIssue {
    /// Same keys and context: the later layer or entry wins everywhere.
    Overridden { winner: Binding, loser: Binding },
    /// Same keys, different contexts that both hold in `context`, where
    /// `winner` takes precedence.
    Shadowed {
        winner: Binding,
        loser: Binding,
        context: ContextStack,
    },
    /// `prefix`'s keys start `longer`'s, both hold in `context`, and no
    /// other binding for `prefix`'s keys beats it there, so there `prefix`
    /// runs only when the chord times out.
    PrefixDelay {
        prefix: Binding,
        longer: Binding,
        context: ContextStack,
    },
}

impl fmt::Display for KeymapIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KeymapIssue::Overridden { winner, loser } => write!(
                f,
                "{}: `{}` for {} overrides {} at {}",
                winner.location, winner.keys, winner.command, loser.command, loser.location
            ),
            KeymapIssue::Shadowed {
                winner,
                loser,
                context,
            } => write!(
                f,
                "{}: `{}` for {} shadows {} at {} in {}",
                winner.location,
                winner.keys,
                winner.command,
                loser.command,
                loser.location,
                Frames(context)
            ),
            KeymapIssue::PrefixDelay {
                prefix,
                longer,
                context,
            } => write!(
                f,
                "{}: `{}` for {} waits for the chord timeout in {}, as `{}` for {} at {} starts with it",
                prefix.location,
                prefix.keys,
                prefix.command,
                Frames(context),
                longer.keys,
                longer.command,
                longer.location
            ),
        }
    }
}

/// A stack's frame names, as `[workbench, editor]`.
struct Frames<'a>(&'a ContextStack);

impl fmt::Display for Frames<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<&str> = self
            .0
            .frames()
            .iter()
            .map(|frame| frame.name.as_str())
            .collect();
        write!(f, "[{}]", names.join(", "))
    }
}

impl Keymap {
    /// [`Keymap::report_with`] in the one context `[editor]`.
    pub fn report(&self) -> Vec<KeymapIssue> {
        self.report_with(&[ContextStack::new().with(ContextFrame::new("editor"))])
    }

    /// Every binding a later one overrides, and each pair of bindings that
    /// shadow or delay each other in one of `report_contexts`, which are
    /// the contexts the host has. Overridden bindings are left out of the
    /// rest, as they never run.
    pub fn report_with(&self, report_contexts: &[ContextStack]) -> Vec<KeymapIssue> {
        let bindings = self.bindings();
        let mut issues = Vec::new();
        let mut live = Vec::new();
        for (ix, loser) in bindings.iter().enumerate() {
            match self.overrider(ix) {
                Some(winner) => issues.push(KeymapIssue::Overridden {
                    winner: bindings[winner].clone(),
                    loser: loser.clone(),
                }),
                None => live.push(ix),
            }
        }
        // Where two bindings both hold, with their depths there.
        let overlap = |a: &Binding, b: &Binding| {
            report_contexts
                .iter()
                .find_map(|stack| Some((stack, a.depth(stack)?, b.depth(stack)?)))
        };
        // Whether the binding at `ix` runs for its keys in `stack`: no other
        // live binding for them takes precedence there.
        let runs = |ix: usize, stack: &ContextStack| {
            let rank = |ix: usize| bindings[ix].depth(stack).map(|depth| (depth, ix));
            let own = rank(ix);
            own.is_some()
                && live.iter().all(|&other| {
                    other == ix || bindings[other].keys != bindings[ix].keys || rank(other) < own
                })
        };
        for (n, &a) in live.iter().enumerate() {
            for &b in &live[n + 1..] {
                let (first, second) = (&bindings[a], &bindings[b]);
                if first.keys == second.keys {
                    let Some((stack, first_depth, second_depth)) = overlap(first, second) else {
                        continue;
                    };
                    // `b` is later, so it wins a tie.
                    let (winner, loser) = if first_depth > second_depth {
                        (first, second)
                    } else {
                        (second, first)
                    };
                    issues.push(KeymapIssue::Shadowed {
                        winner: winner.clone(),
                        loser: loser.clone(),
                        context: stack.clone(),
                    });
                } else if let Some((prefix, longer)) = strict_prefix(a, b, bindings) {
                    // Only where the prefix would run at all.
                    let delayed = report_contexts.iter().find(|stack| {
                        bindings[longer].depth(stack).is_some() && runs(prefix, stack)
                    });
                    if let Some(stack) = delayed {
                        issues.push(KeymapIssue::PrefixDelay {
                            prefix: bindings[prefix].clone(),
                            longer: bindings[longer].clone(),
                            context: stack.clone(),
                        });
                    }
                }
            }
        }
        issues
    }
}

/// The one of the bindings at `a` and `b` whose keys strictly start the
/// other's, then the other.
fn strict_prefix(a: usize, b: usize, bindings: &[Binding]) -> Option<(usize, usize)> {
    let (a_keys, b_keys) = (bindings[a].keys.keystrokes(), bindings[b].keys.keystrokes());
    if a_keys.len() < b_keys.len() && b_keys.starts_with(a_keys) {
        Some((a, b))
    } else if b_keys.len() < a_keys.len() && a_keys.starts_with(b_keys) {
        Some((b, a))
    } else {
        None
    }
}
