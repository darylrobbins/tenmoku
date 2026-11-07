//! The quickfix and location lists, as nvim keeps them (`:help quickfix`):
//! plain data, with no gpui, which `quickfix_commands.rs` lands from.
//!
//! A [`QuickfixList`] is a titled list of items, each a place in a file
//! with a message, and the item last selected. A [`QuickfixStack`] holds
//! up to [`STACK_DEPTH`] lists: a new one goes on after the current list,
//! dropping any newer than it, and past the depth the oldest goes, as
//! nvim's do; `:colder` and `:cnewer` move through them. [`Quickfixes`] is
//! a window's: one quickfix stack, and one location stack per editor pane,
//! keyed by the pane and dropped with it.
//!
//! Moving through a list doesn't wrap: past either end is nvim's `E553: No
//! more items` (a count that overshoots stops at the end, as nvim's does,
//! as long as it moves at all), and with no list or no items, `E42: No
//! Errors`. `:cc N` past the last item selects the last.
//!
//! Item paths are absolute; nothing here touches the disk.

use std::{
    collections::{HashMap, VecDeque},
    hash::Hash,
    path::{Path, PathBuf},
};

/// How many lists a stack keeps, as nvim's `LISTCOUNT`.
pub(crate) const STACK_DEPTH: usize = 10;

/// How many items `:clist` puts in the status line.
pub(crate) const LIST_CAP: usize = 10;

/// What an item reports, as nvim's `type` (`e`, `w`, `i`, `n`, or none)
/// names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum QuickfixKind {
    Error,
    Warning,
    #[cfg_attr(not(test), expect(dead_code, reason = "no filler reports infos yet"))]
    Info,
    #[cfg_attr(not(test), expect(dead_code, reason = "no filler reports notes yet"))]
    Note,
    /// No type, as a `:grep` match has.
    Untyped,
}

impl QuickfixKind {
    /// The word nvim shows for it after a space (`qf_types`): none for
    /// [`QuickfixKind::Untyped`].
    fn word(self) -> &'static str {
        match self {
            Self::Error => " error",
            Self::Warning => " warning",
            Self::Info => " info",
            Self::Note => " note",
            Self::Untyped => "",
        }
    }
}

/// One place in a list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct QuickfixItem {
    /// Absolute.
    pub path: PathBuf,
    /// From 0.
    pub row: u32,
    /// From 0, in chars.
    pub col: u32,
    pub text: String,
    pub kind: QuickfixKind,
}

/// Why a quickfix command didn't move, as nvim says it ([`Display`]).
///
/// [`Display`]: std::fmt::Display
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum QuickfixError {
    /// No list, or one with no items.
    NoErrors,
    /// Already at the end the command moves towards.
    NoMoreItems,
    /// `:colder` at the oldest list, or with none.
    AtBottom,
    /// `:cnewer` at the newest list, or with none.
    AtTop,
    /// An `:l` command in a pane with no location list.
    NoLocationList,
}

impl std::fmt::Display for QuickfixError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NoErrors => "E42: No Errors",
            Self::NoMoreItems => "E553: No more items",
            Self::AtBottom => "E380: At bottom of quickfix stack",
            Self::AtTop => "E381: At top of quickfix stack",
            Self::NoLocationList => "E776: No location list",
        })
    }
}

/// A titled list of items and the one selected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct QuickfixList {
    pub title: String,
    pub items: Vec<QuickfixItem>,
    /// The selected item's index, from 0; 0 for an empty list.
    pub current: usize,
}

impl QuickfixList {
    /// `items` under `title`, the first selected.
    pub(crate) fn new(title: impl Into<String>, items: Vec<QuickfixItem>) -> Self {
        Self {
            title: title.into(),
            items,
            current: 0,
        }
    }

    /// The selected item, none in an empty list.
    pub(crate) fn current_item(&self) -> Option<&QuickfixItem> {
        self.items.get(self.current)
    }

    /// Selects the item `count` after the current, or the last if fewer
    /// follow: E553 at the last.
    pub(crate) fn next(&mut self, count: u32) -> Result<&QuickfixItem, QuickfixError> {
        let last = self.last_index()?;
        if self.current >= last {
            return Err(QuickfixError::NoMoreItems);
        }
        self.current = self.current.saturating_add(count as usize).min(last);
        Ok(&self.items[self.current])
    }

    /// Selects the item `count` before the current, or the first if fewer
    /// precede: E553 at the first.
    pub(crate) fn previous(&mut self, count: u32) -> Result<&QuickfixItem, QuickfixError> {
        self.last_index()?;
        if self.current == 0 {
            return Err(QuickfixError::NoMoreItems);
        }
        self.current = self.current.saturating_sub(count as usize);
        Ok(&self.items[self.current])
    }

    /// `:cc [N]`: selects item `number`, from 1 (past the last, the last),
    /// or with none the current one again.
    pub(crate) fn go(&mut self, number: Option<u32>) -> Result<&QuickfixItem, QuickfixError> {
        let last = self.last_index()?;
        if let Some(number) = number {
            self.current = (number.max(1) as usize - 1).min(last);
        }
        Ok(&self.items[self.current])
    }

    /// Selects the first item.
    pub(crate) fn first(&mut self) -> Result<&QuickfixItem, QuickfixError> {
        self.go(Some(1))
    }

    /// Selects the last item.
    pub(crate) fn last(&mut self) -> Result<&QuickfixItem, QuickfixError> {
        let last = self.last_index()?;
        self.current = last;
        Ok(&self.items[last])
    }

    /// `:cnfile`: selects the first item after the current whose file
    /// isn't the one before it, `count` times, stopping at the last file:
    /// E553 with no file after the current item's.
    pub(crate) fn next_file(&mut self, count: u32) -> Result<&QuickfixItem, QuickfixError> {
        self.last_index()?;
        let step = |from: usize| {
            let path = &self.items[from].path;
            (from + 1..self.items.len()).find(|&ix| self.items[ix].path != *path)
        };
        self.current = self.steps(count, step)?;
        Ok(&self.items[self.current])
    }

    /// `:cNfile`: selects the nearest item before the current whose file
    /// isn't the current item's (the last of the file before), `count`
    /// times, as nvim does: E553 with no file before.
    pub(crate) fn previous_file(&mut self, count: u32) -> Result<&QuickfixItem, QuickfixError> {
        self.last_index()?;
        let step = |from: usize| {
            let path = &self.items[from].path;
            (0..from).rev().find(|&ix| self.items[ix].path != *path)
        };
        self.current = self.steps(count, step)?;
        Ok(&self.items[self.current])
    }

    /// Where `count` of `step` from the current item end, stopping where
    /// `step` finds nothing: E553 if the first finds nothing.
    fn steps(
        &self,
        count: u32,
        step: impl Fn(usize) -> Option<usize>,
    ) -> Result<usize, QuickfixError> {
        let mut at = step(self.current).ok_or(QuickfixError::NoMoreItems)?;
        for _ in 1..count {
            match step(at) {
                Some(next) => at = next,
                None => break,
            }
        }
        Ok(at)
    }

    /// The last item's index: E42 with none.
    fn last_index(&self) -> Result<usize, QuickfixError> {
        self.items
            .len()
            .checked_sub(1)
            .ok_or(QuickfixError::NoErrors)
    }

    /// nvim's message on landing at the selected item: `(2 of 3) error:
    /// text`, or untyped `(2 of 3): text`, the text's leading blanks skipped and each line break (and
    /// the blanks after it) one space. Empty for an empty list.
    pub(crate) fn landed_message(&self) -> String {
        let Some(item) = self.current_item() else {
            return String::new();
        };
        format!(
            "({} of {}){}: {}",
            self.current + 1,
            self.items.len(),
            item.kind.word(),
            one_line(&item.text)
        )
    }

    /// `:clist`, on one line: up to [`LIST_CAP`] items from the first,
    /// each as nvim lists it (`1 src/a.rs:3 col 5 error: text`, untyped
    /// `1 src/a.rs:3 col 5: text`, a path
    /// under `root` relative to it), and how many more there are.
    pub(crate) fn listing(&self, root: Option<&Path>) -> String {
        let mut rows: Vec<String> = (self.items.iter().enumerate().take(LIST_CAP))
            .map(|(ix, item)| {
                let path = root
                    .and_then(|root| item.path.strip_prefix(root).ok())
                    .unwrap_or(&item.path);
                format!(
                    "{} {}:{} col {}{}: {}",
                    ix + 1,
                    path.display(),
                    item.row as u64 + 1,
                    item.col as u64 + 1,
                    item.kind.word(),
                    one_line(&item.text)
                )
            })
            .collect();
        if self.items.len() > LIST_CAP {
            rows.push(format!("… {} more", self.items.len() - LIST_CAP));
        }
        rows.join(" | ")
    }
}

/// `text` as nvim shows an item's on one line (`qf_fmt_text`).
fn one_line(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.trim_start().chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\n' {
            out.push(' ');
            while chars.next_if(|ch| ch.is_whitespace()).is_some() {}
        } else {
            out.push(ch);
        }
    }
    out
}

/// Up to [`STACK_DEPTH`] lists, oldest first, and the one in use.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct QuickfixStack {
    lists: VecDeque<QuickfixList>,
    /// The list in use, an index into `lists`; 0 with none.
    current: usize,
}

impl QuickfixStack {
    /// Puts `list` after the one in use, dropping those newer than it and,
    /// past [`STACK_DEPTH`], the oldest, and uses it.
    pub(crate) fn push(&mut self, list: QuickfixList) {
        if !self.lists.is_empty() {
            self.lists.truncate(self.current + 1);
        }
        if self.lists.len() == STACK_DEPTH {
            self.lists.pop_front();
        }
        self.lists.push_back(list);
        self.current = self.lists.len() - 1;
    }

    /// How many lists it holds.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.lists.len()
    }

    /// The list in use, none in an empty stack.
    pub(crate) fn current(&self) -> Option<&QuickfixList> {
        self.lists.get(self.current)
    }

    /// The list in use: E42 in an empty stack.
    pub(crate) fn current_mut(&mut self) -> Result<&mut QuickfixList, QuickfixError> {
        self.lists
            .get_mut(self.current)
            .ok_or(QuickfixError::NoErrors)
    }

    /// `:colder`: uses the list `count` older, or the oldest if fewer are
    /// (E380 at the oldest), and says which (as [`QuickfixStack::summary`]).
    pub(crate) fn older(&mut self, count: u32) -> Result<String, QuickfixError> {
        if self.current == 0 {
            return Err(QuickfixError::AtBottom);
        }
        self.current = self.current.saturating_sub(count as usize);
        Ok(self.summary())
    }

    /// `:cnewer`: uses the list `count` newer, or the newest if fewer are
    /// (E381 at the newest), and says which.
    pub(crate) fn newer(&mut self, count: u32) -> Result<String, QuickfixError> {
        if self.current + 1 >= self.lists.len() {
            return Err(QuickfixError::AtTop);
        }
        let last = self.lists.len() - 1;
        self.current = self.current.saturating_add(count as usize).min(last);
        Ok(self.summary())
    }

    /// nvim's line for the list in use (`qf_msg`): `error list 1 of 2; 3
    /// errors `, padded to 34 columns, then the title.
    pub(crate) fn summary(&self) -> String {
        let Some(list) = self.current() else {
            return String::new();
        };
        let lead = format!(
            "error list {} of {}; {} errors ",
            self.current + 1,
            self.lists.len(),
            list.items.len()
        );
        format!("{lead:<34}{}", list.title)
    }
}

/// A window's lists: its quickfix stack, and a location stack for each
/// pane `P` that has had one.
#[derive(Debug)]
pub(crate) struct Quickfixes<P> {
    pub quickfix: QuickfixStack,
    locations: HashMap<P, QuickfixStack>,
}

impl<P> Default for Quickfixes<P> {
    fn default() -> Self {
        Self {
            quickfix: QuickfixStack::default(),
            locations: HashMap::new(),
        }
    }
}

impl<P: Hash + Eq> Quickfixes<P> {
    /// `pane`'s location stack: E776 if it has none.
    pub(crate) fn location(&mut self, pane: &P) -> Result<&mut QuickfixStack, QuickfixError> {
        self.locations
            .get_mut(pane)
            .ok_or(QuickfixError::NoLocationList)
    }

    /// The quickfix stack, or with `location` `pane`'s location stack
    /// (E776 if it has none).
    pub(crate) fn stack(
        &mut self,
        location: bool,
        pane: &P,
    ) -> Result<&mut QuickfixStack, QuickfixError> {
        match location {
            true => self.location(pane),
            false => Ok(&mut self.quickfix),
        }
    }

    /// Puts `list` on `pane`'s location stack, starting one if it has none.
    pub(crate) fn push_location(&mut self, pane: P, list: QuickfixList) {
        self.locations.entry(pane).or_default().push(list);
    }

    /// Forgets `pane`'s location stack, as its pane closes.
    pub(crate) fn drop_pane(&mut self, pane: &P) {
        self.locations.remove(pane);
    }

    /// Whether `pane` has a location stack.
    #[cfg(test)]
    pub(crate) fn has_location(&self, pane: &P) -> bool {
        self.locations.contains_key(pane)
    }
}
