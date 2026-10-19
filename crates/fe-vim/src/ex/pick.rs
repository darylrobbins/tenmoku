//! A list the host shows to pick from, [`HostCommand::Pick`]: what
//! `:marks` and `:registers` leave, and the commands that list things after it.

use fe_command::{CommandId, Invocation};

use crate::{MarkMotion, Motion};

/// A list to pick a row from, as the host shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pick {
    /// What the picker is called (its placeholder).
    pub title: &'static str,
    /// The column heads, as the command prints them.
    pub header: &'static str,
    pub rows: Vec<PickRow>,
}

/// One row of a [`Pick`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickRow {
    /// The row's columns, unpadded, in the order they are shown.
    pub cells: Vec<String>,
    /// What choosing the row does.
    pub then: PickThen,
}

/// What choosing a [`PickRow`] runs in the editor the line was typed in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PickThen {
    /// Jump to the mark, as `` `x `` does.
    Jump(char),
    /// Put the register after the caret, as `"xp` does.
    Put(char),
    /// Go `count` entries through the jumplist, as `<C-o>` (`older`) or
    /// `<C-i>` does.
    Jumps { older: bool, count: u32 },
    /// Go `count` entries through the changelist, as `g;` (`older`) or `g,`
    /// does.
    Changes { older: bool, count: u32 },
    /// Go to the byte column on the row (both from 0), as the list it is in
    /// is at.
    At { row: u32, column: u32 },
}

impl PickThen {
    /// The command that does it.
    pub fn invocation(&self) -> Invocation {
        match self {
            Self::Jump(name) => {
                let motion = Motion::Mark(MarkMotion {
                    name: *name,
                    exact: true,
                });
                let command = CommandId::parse("vim.move").expect("vim command IDs are valid");
                Invocation::new(command)
                    .args(serde_json::json!({"motion": motion, "extend": false}))
            }
            Self::Jumps { older, count } => {
                let command = match older {
                    true => "vim.jump_older",
                    false => "vim.jump_newer",
                };
                let command = CommandId::parse(command).expect("vim command IDs are valid");
                Invocation::new(command).args(serde_json::json!({"count": count}))
            }
            Self::Changes { older, count } => {
                let command = match older {
                    true => "vim.change_older",
                    false => "vim.change_newer",
                };
                let command = CommandId::parse(command).expect("vim command IDs are valid");
                Invocation::new(command).args(serde_json::json!({"count": count}))
            }
            Self::At { row, column } => {
                let motion = Motion::Position {
                    row: *row,
                    column: *column,
                };
                let command = CommandId::parse("vim.move").expect("vim command IDs are valid");
                Invocation::new(command)
                    .args(serde_json::json!({"motion": motion, "extend": false}))
            }
            Self::Put(name) => {
                let command = CommandId::parse("vim.put").expect("vim command IDs are valid");
                Invocation::new(command).args(serde_json::json!({"register": name}))
            }
        }
    }
}

/// `text` with control characters as `^X` and DEL as `^?`, as nvim shows
/// them.
pub(crate) fn printable(text: &str) -> String {
    let mut shown = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\x7f' => shown.push_str("^?"),
            ch if ch.is_ascii_control() => {
                shown.push('^');
                shown.push((ch as u8 ^ 0x40) as char);
            }
            ch => shown.push(ch),
        }
    }
    shown
}
