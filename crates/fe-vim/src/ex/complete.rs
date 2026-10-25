//! The command line's completion of an ex command's name: [`Ex::completions`].

use super::{Ex, split, table};

/// The full names of the commands `ex.rs`, `lines.rs`, `global.rs`,
/// `normal.rs`, `buffers.rs` and `windows.rs` take (those of the table are
/// added from [`table::COMMANDS`]).
/// Each must parse as a command; a test holds the list to that.
const NAMES: &[&str] = &[
    "Explore",
    "Replace",
    "bNext",
    "bdelete",
    "bfirst",
    "blast",
    "bnext",
    "bprevious",
    "brewind",
    "browse",
    "buffer",
    "buffers",
    "cclose",
    "close",
    "copen",
    "copy",
    "delete",
    "diffget",
    "diffput",
    "diffsplit",
    "edit",
    "ex",
    "exit",
    "files",
    "global",
    "join",
    "lclose",
    "lopen",
    "ls",
    "move",
    "new",
    "nohlsearch",
    "normal",
    "oldfiles",
    "only",
    "qall",
    "quit",
    "quitall",
    "saveas",
    "sort",
    "split",
    "substitute",
    "t",
    "tabNext",
    "tabclose",
    "tabedit",
    "tabfirst",
    "tablast",
    "tabnew",
    "tabnext",
    "tabonly",
    "tabprevious",
    "tabrewind",
    "vglobal",
    "vnew",
    "vsplit",
    "wall",
    "write",
    "wq",
    "xit",
    "yank",
];

impl Ex {
    /// The commands the name at the end of `line` abbreviates, for the
    /// command line's `<Tab>`: the byte offset where the name starts and the
    /// full names of the commands fe-vim runs that it is a prefix of, in
    /// nvim's order (`getcompletion()`'s: by byte, so capitals first).
    /// `None` when the line doesn't end in a name after its range (the
    /// caret is in an argument) or the name is empty.
    pub fn completions(line: &str) -> Option<(usize, Vec<&'static str>)> {
        let (_, _, name) = split(line);
        if name.is_empty() || !name.bytes().all(|byte| byte.is_ascii_alphabetic()) {
            return None;
        }
        let mut names: Vec<&'static str> = NAMES
            .iter()
            .copied()
            .chain(table::COMMANDS.iter().map(|entry| entry.name))
            .filter(|full| full.starts_with(name))
            .collect();
        names.sort_unstable();
        names.dedup();
        Some((line.len() - name.len(), names))
    }
}
