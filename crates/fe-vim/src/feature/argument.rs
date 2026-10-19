//! `ia aa`: a function argument, as VSCodeVim's argument object (and
//! targets.vim's) gives it. Charwise.
//!
//! The list is the nearest `( )`, `[ ]` or `{ }` around the cursor, which
//! may span rows; a bracket under the cursor belongs to the argument it's
//! in, not to the list. Its arguments are split at the commas at its own
//! depth, skipping nested brackets (of any kind) and quoted strings, with
//! backslash escapes. A `"` or `` ` `` string may span rows; a `'` one ends
//! at its row's end unless a backslash escapes the line break, so that an
//! apostrophe in prose (`don't`) can't swallow the rows after it. A comma
//! before the closer with only white space after it ends the last argument
//! rather than starting another.
//!
//! `ia` is the argument under the cursor (on a comma or the white space
//! after it, the next one) without the white space around it. `aa` adds one
//! separator: up to the next argument's start, or for the last argument from
//! the previous one's end; an only argument takes itself, and its trailing
//! comma if it has one. A count takes that many arguments from the cursor's.
//! In a visual mode, the object again when the selection already is that
//! object takes the next argument too.
//!
//! Finding the list reads the text once from its start, for the strings,
//! and stops at the list's closer or at the command's deadline.

use std::ops::Range;

use fe_buffer::TransactionId;
use fe_command::{CommandError, Dispatched, Effect};
use fe_text::Text;

use crate::{
    Mode, Pending, SelectArgs, TextObject, VimTarget, VisualKind, feature::Feature, motion::Cursor,
    operator::Span, search::Deadline,
};

pub(crate) const FEATURE: Feature = Feature {
    object,
    ..Feature::NONE
};

/// `a` after `i` or `a`.
fn object(_pending: &Pending, ch: char, inner: bool) -> Option<TextObject> {
    match (ch, inner) {
        ('a', true) => Some(TextObject::InnerArgument),
        ('a', false) => Some(TextObject::AArgument),
        _ => None,
    }
}

/// Whether `object` is one of this module's.
pub(crate) fn is_object(object: TextObject) -> bool {
    matches!(object, TextObject::InnerArgument | TextObject::AArgument)
}

/// How many bytes the scan reads between looks at the deadline.
const BYTES_PER_CHECK: usize = 1 << 12;

/// A bracket the scan holds open, and its list so far.
struct Level {
    /// The opener.
    byte: u8,
    commas: Vec<usize>,
    /// Each argument before the last comma, without its white space (at
    /// its comma if it's only white space).
    args: Vec<Range<usize>>,
    /// The current argument's bytes that aren't white space, so far.
    current: Option<Range<usize>>,
}

impl Level {
    fn new(byte: u8) -> Self {
        Self {
            byte,
            commas: Vec::new(),
            args: Vec::new(),
            current: None,
        }
    }

    /// The byte at `at` isn't white space.
    fn touch(&mut self, at: usize) {
        let start = self.current.as_ref().map_or(at, |current| current.start);
        self.current = Some(start..at + 1);
    }

    /// The argument so far ends at the separator at `at`.
    fn end_arg(&mut self, at: usize) {
        self.args.push(self.current.take().unwrap_or(at..at));
    }
}

fn closer(open: u8) -> u8 {
    match open {
        b'(' => b')',
        b'[' => b']',
        _ => b'}',
    }
}

/// The list around `offset`: its arguments, without the white space around
/// them, the commas between them, and whether a trailing comma ends it.
struct List {
    args: Vec<Range<usize>>,
    commas: Vec<usize>,
    trailing: bool,
}

impl List {
    /// The list around `offset`, or `None` without one (or past
    /// `deadline`). The text is read from its start up to the list's closer,
    /// keeping a level for each open bracket; at `offset` the innermost is
    /// the list's.
    fn around(text: &Text, offset: usize, deadline: &Deadline) -> Option<List> {
        let mut levels: Vec<Level> = Vec::new();
        // Once the scan reaches `offset`: how many levels are open, the
        // innermost the list's.
        let mut depth = None;
        let mut quote = None;
        let mut escaped = false;
        let mut at = 0;
        for chunk in text.chunks() {
            for &byte in chunk.as_bytes() {
                if at % BYTES_PER_CHECK == 0 && deadline.expired() {
                    return None;
                }
                if depth.is_none() && at >= offset {
                    // A closer under the cursor closes its own bracket first.
                    let closes = quote.is_none() && matches!(byte, b')' | b']' | b'}');
                    if at > offset || !closes {
                        depth = Some(levels.len()).filter(|&depth| depth > 0);
                        depth?;
                    }
                }
                let white = byte.is_ascii_whitespace();
                if let Some(open) = quote {
                    if escaped {
                        escaped = false;
                    } else if byte == b'\\' {
                        escaped = true;
                    } else if byte == open || (byte == b'\n' && open == b'\'') {
                        quote = None;
                    }
                    if !white && let Some(level) = levels.last_mut() {
                        level.touch(at);
                    }
                    at += 1;
                    continue;
                }
                match byte {
                    b'(' | b'[' | b'{' => {
                        if let Some(level) = levels.last_mut() {
                            level.touch(at);
                        }
                        levels.push(Level::new(byte));
                    }
                    b')' | b']' | b'}' => {
                        if depth == Some(levels.len()) {
                            let mut level = levels.pop()?;
                            if byte != closer(level.byte) {
                                return None;
                            }
                            level.end_arg(at);
                            return Some(List::of(level));
                        }
                        levels.pop();
                        if let Some(level) = levels.last_mut() {
                            level.touch(at);
                        }
                    }
                    b',' => {
                        if let Some(level) = levels.last_mut() {
                            level.commas.push(at);
                            level.end_arg(at);
                        }
                    }
                    _ => {
                        if byte == b'"' || byte == b'\'' || byte == b'`' {
                            quote = Some(byte);
                        }
                        if !white && let Some(level) = levels.last_mut() {
                            level.touch(at);
                        }
                    }
                }
                at += 1;
            }
        }
        None
    }

    fn of(level: Level) -> List {
        let Level {
            mut args, commas, ..
        } = level;
        let trailing = args.len() > 1 && args.last().is_some_and(|arg| arg.is_empty());
        if trailing {
            args.pop();
        }
        List {
            args,
            commas,
            trailing,
        }
    }

    /// The index of the argument at `offset`: on a comma, or the white
    /// space after it, the next one.
    fn index(&self, offset: usize) -> usize {
        let before = self.commas.partition_point(|&comma| comma < offset);
        let on_comma = self.commas.get(before) == Some(&offset);
        (before + usize::from(on_comma)).min(self.args.len() - 1)
    }

    /// The bytes of `count` arguments from `first`, `inner` or with a
    /// separator; `None` past the last argument.
    fn span(&self, first: usize, count: usize, inner: bool) -> Option<Range<usize>> {
        let last = first.checked_add(count.checked_sub(1)?)?;
        let (from, to) = (self.args.get(first)?, self.args.get(last)?);
        Some(match (inner, self.args.get(last + 1)) {
            (true, _) => from.start..to.end,
            (false, Some(next)) => from.start..next.start,
            (false, None) if first > 0 => self.args[first - 1].end..to.end,
            (false, None) => match self.trailing {
                true => from.start..self.commas[self.commas.len() - 1] + 1,
                false => from.start..to.end,
            },
        })
    }

    /// How many arguments from `first` make `span` exactly `range`, if any:
    /// the last one ends `range`, or (with a separator) the one after it
    /// starts where `range` ends.
    fn count_of(&self, first: usize, inner: bool, range: &Range<usize>) -> Option<usize> {
        let ending = self.args.partition_point(|arg| arg.end < range.end);
        let before_next = self
            .args
            .partition_point(|arg| arg.start < range.end)
            .checked_sub(1);
        [Some(ending), before_next, Some(self.args.len() - 1)]
            .into_iter()
            .flatten()
            .filter(|&last| last >= first && last < self.args.len())
            .map(|last| last - first + 1)
            .find(|&count| self.span(first, count, inner).as_ref() == Some(range))
    }
}

/// The bytes `object` covers around `offset`, `count` arguments.
fn range(
    text: &Text,
    offset: usize,
    object: TextObject,
    count: u32,
    deadline: &Deadline,
) -> Option<Range<usize>> {
    let list = List::around(text, offset, deadline)?;
    let first = list.index(offset);
    list.span(
        first,
        count.max(1) as usize,
        object == TextObject::InnerArgument,
    )
}

/// What `object` covers around the caret at `offset`, `count` times, as
/// `text_object::select` says.
pub(crate) fn select(text: &Text, offset: usize, object: TextObject, count: u32) -> Span {
    match range(text, offset, object, count, &Deadline::new()) {
        Some(range) => Span::Chars(range),
        None => Span::Failed,
    }
}

/// The position a visual selection of `range` (not empty) ends at.
fn last(text: &Text, range: &Range<usize>) -> usize {
    match text.offset_to_point(range.end).column {
        0 => range.end - 1,
        _ => Cursor::at(text, range.end - 1).offset(),
    }
}

/// Where the bytes a visual selection ending at position `hi` cover end:
/// past its grapheme, or its row's line break.
fn end_of(text: &Text, hi: usize) -> usize {
    let point = text.offset_to_point(hi);
    if point.column == text.row_len(point.row) {
        return (hi + 1).min(text.len());
    }
    let cursor = Cursor::at(text, hi);
    let column = cursor.line.starts.get(cursor.ix + 1).copied();
    cursor.line.start + column.unwrap_or(cursor.line.text.len())
}

/// `vim.select` with `ia aa`. Over one position the argument (`count` of
/// them) there; where the selection already is the object over some
/// arguments from its start (over one position, only if this object made
/// it), `count` more. Otherwise the object at the cursor, which must hold
/// the selection. Past the list's last argument, or over an empty one, it
/// fails. The mode becomes charwise.
pub(crate) fn select_visual(
    target: &mut VimTarget<'_>,
    args: SelectArgs,
) -> Result<Dispatched<TransactionId>, CommandError> {
    let VimTarget { model, state, .. } = target;
    model.sync_display();
    let (Mode::Visual(_), Some(anchor), Some(cursor)) = (
        state.mode,
        state.visual_anchor(model),
        state.visual_cursor(model),
    ) else {
        return Err(CommandError::new("no visual selection"));
    };
    let text = model.text().clone();
    let deadline = Deadline::new();
    let count = args.count.unwrap_or(1).max(1) as usize;
    let inner = args.text_object == TextObject::InnerArgument;
    let (lo, hi) = (anchor.min(cursor), anchor.max(cursor));
    let grows = lo != hi || state.visual_by_object();
    let found = 'found: {
        if grows && let Some(list) = List::around(&text, lo, &deadline) {
            let first = list.index(lo);
            let selected = lo..end_of(&text, hi);
            if let Some(more) = list.count_of(first, inner, &selected) {
                break 'found list.span(first, more + count, inner);
            }
        }
        let found = range(&text, cursor, args.text_object, count as u32, &deadline);
        match lo == hi {
            true => found,
            // A wider selection: only an object that holds it.
            false => found.filter(|found| found.start <= lo && found.end >= end_of(&text, hi)),
        }
    };
    let Some(found) = found.filter(|found| !found.is_empty()) else {
        state.failed = true;
        return Ok(Dispatched::new(Effect::Nothing));
    };
    let end = last(&text, &found);
    state.select_visual(model, VisualKind::Char, found.start, end);
    state.set_visual_by_object();
    Ok(Dispatched::new(Effect::Changed))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn list(text: &str, offset: usize) -> Option<Vec<String>> {
        let text = Text::new(text);
        let list = List::around(&text, offset, &Deadline::never())?;
        Some(
            list.args
                .iter()
                .map(|arg| text.slice_to_string(arg.clone()))
                .collect(),
        )
    }

    #[test]
    fn strings_span_rows_but_single_quotes_end_with_theirs() {
        assert_eq!(
            list("f(`a\n)b`, c)", 3),
            Some(vec!["`a\n)b`".to_string(), "c".to_string()])
        );
        assert_eq!(
            list("f(\"a\\\n), b\", c)", 3),
            Some(vec!["\"a\\\n), b\"".to_string(), "c".to_string()])
        );
        // An apostrophe in a comment ends with its row.
        assert_eq!(
            list("// don't\nf(a, b)", 11),
            Some(vec!["a".to_string(), "b".to_string()])
        );
    }

    #[test]
    fn an_expired_deadline_finds_no_list() {
        let text = Text::new(&format!("f({})", "a, ".repeat(100_000)));
        assert!(List::around(&text, 2, &Deadline::after(Duration::ZERO)).is_none());
        assert!(List::around(&text, 2, &Deadline::never()).is_some());
    }

    #[test]
    fn the_scan_stops_at_the_lists_closer() {
        // A closer the scan never reaches can't fail the list.
        assert_eq!(
            list("f(a, b) ]", 2),
            Some(vec!["a".to_string(), "b".to_string()])
        );
    }
}
