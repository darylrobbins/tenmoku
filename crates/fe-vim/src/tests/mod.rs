use fe_command::Invocation;
use serde::Serialize;

use crate::{HeadlessVim, Pending, Step, VimState, context, operator::MEASURING, parse_keys, step};

mod commands;
mod edits;
mod insert;
mod motions;
mod operators;
mod reducer;
mod registers;
mod repeat;
mod state;
mod undo;
mod visual;
mod visual_block;

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

/// `command` with `args`, stamped with a normal-mode vim context.
pub(super) fn invocation(command: &str, args: impl Serialize) -> Invocation {
    Invocation::new(command.parse().unwrap())
        .args(serde_json::to_value(args).unwrap())
        .context(context(&VimState::new()))
}

/// The registers `" 0-9 - a-z` that hold anything, as `(name, text,
/// linewise)`.
pub(super) fn registers(vim: &HeadlessVim) -> Vec<(char, String, bool)> {
    "\"0123456789-abcdefghijklmnopqrstuvwxyz"
        .chars()
        .filter_map(|name| {
            let register = vim.registers().get(name)?;
            Some((name, register.text.clone(), register.linewise))
        })
        .collect()
}

/// Runs a hook wherever measuring text would as the model catches up, until
/// dropped (see `operator::MEASURING`): with `true` right after an edit of
/// fe-vim's.
pub(super) struct Measuring;

impl Measuring {
    pub(super) fn new(hook: impl FnMut(bool) + 'static) -> Self {
        MEASURING.set(Some(Box::new(hook)));
        Self
    }
}

impl Drop for Measuring {
    fn drop(&mut self) {
        MEASURING.take();
    }
}
