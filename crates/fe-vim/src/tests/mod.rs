use fe_command::Invocation;

use crate::{Pending, Step, parse_keys, step};

mod commands;
mod motions;
mod reducer;
mod state;

/// Steps `keys` through the reducer from nothing pending. Every key but the
/// last must wait; returns what the last did.
pub(super) fn run(keys: &str) -> Step {
    let keys = parse_keys(keys).unwrap();
    let (last, init) = keys.split_last().expect("some keys");
    let mut pending = Pending::new();
    for &key in init {
        match step(pending, key) {
            Step::Wait(next) => pending = next,
            other => panic!("{key:?} gave {other:?}, not Wait"),
        }
    }
    step(pending, *last)
}

/// The invocation `keys` commit.
pub(super) fn commit(keys: &str) -> Invocation {
    match run(keys) {
        Step::Commit(invocation) => invocation,
        other => panic!("{keys} gave {other:?}, not Commit"),
    }
}
