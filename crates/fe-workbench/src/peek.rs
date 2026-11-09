//! Peek Definition (`editor.peek_definition`, alt-F12), as VS Code's
//! editor.action.peekDefinition: the definitions at the caret open in a
//! widget under the caret's line instead of in the pane.
//!
//! The editor reserves the rows under the caret's line, after a fold
//! there ([`HostBlockKind::Reserve`], [`ROWS`] tall, fewer in a short
//! view), pushing the rows after them down, and the pane draws a
//! [`PeekView`] over them as the editor's frame laid them out ([`layer`]),
//! so it scrolls with the text; the editor scrolls once to show it whole.
//! It has a header (the file's name and the definition's `line:col`, a
//! close button), a read-only editor on the definition's file with the
//! definition highlighted and as many rows above it as fit, up to
//! [`CONTEXT_ROWS`], and, for more than one definition, a list of them on
//! the left that picks the one shown, found where it is as it's picked.
//!
//! Esc (`peek.close`) closes it, in it or in the editor, focusing the
//! editor with its caret where it was; Enter (`peek.open`) or a
//! double-click in the excerpt opens the file there in the pane, at the
//! excerpt's selection, and closes it. It closes too when the editor's
//! text is edited above its rows, when the editor's caret leaves its row,
//! when the editor leaves the pane and when another peek opens in the
//! pane. An answer that lands after the caret moved or the text changed
//! opens nothing; one with no definitions says so in a toast, as go to
//! definition does in its picker. In a diff's side, which the pane doesn't
//! draw a peek over, it goes to the definition instead.
//!
//! Peek References (`editor.peek_references`, shift-F12) opens the same
//! widget on the uses of the word at the caret: see [`references`].

use std::{cell::RefCell, collections::HashMap, ops::Range, path::PathBuf, rc::Rc, sync::Arc};

use fe_buffer::{Anchor, Bias, Buffer, BufferSnapshot, ResolvedSelection, Version};
use fe_editor::{
    EditorModel, Hunk, TextDiff,
    decorations::{Decoration, DecorationKind, DecorationProvider, Tint},
};
use fe_gpui::{EditorTheme, EditorView, HostBlock, HostBlockKind};
use fe_nav::Location;
use fe_text::Text;
use gpui::{
    AnyElement, App, AppContext as _, AvailableSpace, ContentMask, Context, Entity, FocusHandle,
    Focusable as _, InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent,
    ParentElement as _, Pixels, Render, SharedString, Size, StatefulInteractiveElement as _,
    Styled as _, Subscription, Task, WeakEntity, Window, canvas, div, prelude::FluentBuilder as _,
    px,
};
use imara_diff::{Algorithm, Diff, InternedInput, TokenSource};

use crate::{Pane, PaneEvent, ReferencesPanel, Workbench, loader, pane::Reveal};

#[path = "peek_references.rs"]
mod references;

/// The key context of the peek, around its excerpt's editor.
pub(crate) const KEY_CONTEXT: &str = "peek";

/// In the key context of an editor showing a peek, for its Esc.
pub(crate) const SHOWN: &str = "peek_shown";

/// How many rows the peek takes, header and all, if the view has as many.
pub(crate) const ROWS: u32 = 12;

/// The rows above the definition the excerpt shows, where it has them and
/// they fit.
const CONTEXT_ROWS: u32 = 3;

/// The peek's block's id among the editor's host blocks.
const BLOCK_ID: u64 = 0x7065_656b;

/// How wide the list of definitions is.
const LIST_WIDTH: Pixels = px(240.);

/// The header's height.
const HEADER_HEIGHT: Pixels = px(22.);

/// The accent borders' width, above and below.
const BORDER: Pixels = px(2.);

/// A peek open in a pane: [`Pane::peek`].
pub(crate) struct Peek {
    pub(crate) view: Entity<PeekView>,
    /// The editor it's in.
    editor: WeakEntity<EditorView>,
}

/// One definition, or reference, the peek can show.
pub(crate) struct Target {
    location: Location,
    /// Its path as the answer gave it, and 1-based line, for the list.
    label: SharedString,
}

impl Target {
    fn at(location: Location) -> Self {
        let label = format!("{}:{}", location.rel.display(), location.row + 1);
        Self {
            location,
            label: label.into(),
        }
    }
}

/// Where a target is now: the file as the pane would open it, the
/// definition in its text ([`Workbench::target_of`]), and a model sharing
/// the buffer of an editor it's open in.
struct Source {
    path: PathBuf,
    reveal: Reveal,
    open: Option<EditorModel>,
}

/// The excerpt of a target: a read-only editor on its file.
struct Excerpt {
    view: Entity<EditorView>,
    /// Where the definition starts, in its text as edited since.
    start: Anchor,
    /// Its text is the file as read, not an open editor's buffer.
    read: bool,
}

impl Excerpt {
    /// The definition's 1-based line and column where it is now, for the
    /// header.
    fn place(&self, cx: &App) -> (u32, usize) {
        let model = self.view.read(cx).model();
        let text = model.text();
        let start = model.snapshot().resolve(&self.start);
        let row = text.offset_to_point(start).row;
        let column = text
            .slice_to_string(text.row_start(row)..start)
            .chars()
            .count();
        (row + 1, column + 1)
    }
}

/// What Enter or a double-click opens: the selected target's file, with
/// `range` selected in the excerpt's text. If that's the file as `read`,
/// and it's open in an editor now, the range is mapped through the
/// buffer's changes from it ([`map_through`]).
struct Opening {
    path: PathBuf,
    range: Range<usize>,
    read: Option<Arc<str>>,
}

/// Where a peek under a row goes: above the first row shown after it, past
/// a fold there, or after the last row (`at_end`); `limit` is where that
/// row starts, before which an edit is above the peek.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Placement {
    above: u32,
    limit: usize,
    at_end: bool,
}

impl Placement {
    /// Under `row` of `model`, as it's folded now.
    fn under(model: &EditorModel, row: u32) -> Self {
        let text = model.text();
        let display = model.display();
        let mut at = display.row_for_buffer_row(row);
        let after = loop {
            at += 1;
            if at >= display.row_count() {
                break None;
            }
            let shown = display.row(at);
            if shown.buffer_row != row && shown.first {
                break Some(shown.buffer_row);
            }
        };
        match after {
            Some(after) => Self {
                above: after,
                limit: text.row_start(after),
                at_end: false,
            },
            None => Self {
                above: text.row_count(),
                limit: text.len(),
                at_end: true,
            },
        }
    }
}

/// Whether `pane` shows `editor`: its active item, not a Markdown preview
/// in its place.
fn shows(pane: &Pane, editor: &Entity<EditorView>) -> bool {
    let preview = pane.active_tab().is_some_and(|tab| tab.preview().is_some());
    pane.active_editor() == Some(editor) && !preview
}

/// How many rows the peek takes in a view showing `shown`: [`ROWS`], or
/// fewer to leave the caret's row in view above it.
fn fit(shown: u32) -> u32 {
    ROWS.min(shown.saturating_sub(1).max(1))
}

/// Puts the peek's block, `rows` tall, above `above` in `editor`, in place
/// of any it had.
fn reserve(editor: &mut EditorView, above: u32, rows: u32, cx: &mut Context<EditorView>) {
    let mut blocks: Vec<HostBlock> = (editor.blocks().iter())
        .filter(|block| block.id != BLOCK_ID)
        .cloned()
        .collect();
    blocks.push(HostBlock {
        above,
        rows,
        kind: HostBlockKind::Reserve,
        id: BLOCK_ID,
    });
    editor.set_blocks(blocks, cx);
}

/// `range` of `old`, a target's file as read, where it is in `new`, the
/// buffer open on it now. Each end follows a byte of the definition
/// ([`Mapper`]): the start its first, so text inserted at it goes before
/// it, and the end its last, so text inserted there goes after it, as
/// [`Workbench::target_of`] maps. Where either byte is gone or can't be
/// followed, the definition's rows are found again, whole ([`Mapper::find`]);
/// where they aren't there just once, it's its start if that was followed,
/// else the row the start would be near.
fn map_through(old: &str, new: &Text, range: Range<usize>) -> Reveal {
    let new_string = new.to_string();
    let old_text = Text::new(old);
    let clip = |offset: usize, bias| old_text.clip_offset(offset.min(old.len()), bias);
    let start = clip(range.start, Bias::Left);
    let range = start..clip(range.end, Bias::Right).max(start);
    let mapper = Mapper::new(old, &old_text, &new_string, new);
    let start = mapper.map(range.start, Bias::Right);
    let end = match range.is_empty() {
        true => start,
        false => mapper.map(range.end, Bias::Left),
    };
    // Bytes in order follow in order, so an end at or before the start is
    // lost too.
    if let (Ok(start), Ok(end)) = (&start, end)
        && (end > *start || range.is_empty())
    {
        return Reveal::bytes(*start..end);
    }
    if let Some(found) = mapper.find(range) {
        return Reveal::bytes(found);
    }
    match start {
        Ok(start) => Reveal::bytes(start..start),
        Err(near) => Reveal {
            row: near,
            columns: 0..0,
            within_row: false,
        },
    }
}

/// Over this many bytes on either side, rows a row diff changed aren't
/// diffed by token.
const TOKEN_DIFF_BYTES: usize = 64 * 1024;

/// How many rows around a definition [`Mapper::find`] adds on each side to
/// tell where it is.
const FIND_CONTEXT_ROWS: u32 = 8;

/// Follows bytes of `old` into `new`: in the rows the two start and end
/// with alike, where they are; in the rows between, by their row diff, at
/// the same column of a row it kept, and by a diff of their tokens
/// ([`tokens`]) in rows it changed. A byte either diff drops, or that isn't
/// the same byte where it lands, isn't followed.
struct Mapper<'a> {
    old: &'a str,
    old_text: &'a Text,
    new: &'a str,
    new_text: &'a Text,
    /// The bytes of whole rows the texts start with alike, and end with.
    prefix: usize,
    suffix: usize,
    /// The rows between, in each, and their row diff: `None` if either is
    /// too large to diff.
    between: Option<(Text, Text, TextDiff)>,
}

impl<'a> Mapper<'a> {
    fn new(old: &'a str, old_text: &'a Text, new: &'a str, new_text: &'a Text) -> Self {
        let (a, b) = (old.as_bytes(), new.as_bytes());
        let alike = a.iter().zip(b).take_while(|(a, b)| a == b).count();
        let prefix = (a[..alike].iter())
            .rposition(|&c| c == b'\n')
            .map_or(0, |at| at + 1);
        let limit = a.len().min(b.len()) - prefix;
        let alike = (a.iter().rev().zip(b.iter().rev()))
            .take(limit)
            .take_while(|(a, b)| a == b)
            .count();
        let starts_row = |text: &[u8], at: usize| at == prefix || text[at - 1] == b'\n';
        let suffix = match starts_row(a, a.len() - alike) && starts_row(b, b.len() - alike) {
            true => alike,
            false => {
                let from = a.len() - alike;
                let first = a[from..].iter().position(|&c| c == b'\n');
                first.map_or(0, |at| alike - at - 1)
            }
        };
        let (old_between, new_between) = (
            &old[prefix..old.len() - suffix],
            &new[prefix..new.len() - suffix],
        );
        let small =
            old_between.len().max(new_between.len()) <= fe_editor::text_diff::MAX_DIFF_BYTES;
        let between = small.then(|| {
            let diff = TextDiff::new(old_between, new_between);
            (Text::new(old_between), Text::new(new_between), diff)
        });
        Self {
            old,
            old_text,
            new,
            new_text,
            prefix,
            suffix,
            between,
        }
    }

    /// Where `offset` of `old` is in `new`: before the byte at it for
    /// [`Bias::Right`], after the byte before it for [`Bias::Left`]. Else
    /// `Err`, with the row of `new` it'd be near.
    fn map(&self, offset: usize, bias: Bias) -> Result<usize, u32> {
        match bias {
            Bias::Right if offset >= self.old.len() => Ok(self.new.len()),
            Bias::Right => self.follow(offset),
            Bias::Left if offset == 0 => Ok(0),
            Bias::Left => self.follow(offset - 1).map(|at| at + 1),
        }
    }

    /// Where the byte at `at` of `old` is in `new`.
    fn follow(&self, at: usize) -> Result<usize, u32> {
        let (old_len, new_len) = (self.old.len(), self.new.len());
        let to = if at < self.prefix {
            Ok(at)
        } else if at >= old_len - self.suffix {
            Ok(at + new_len - old_len)
        } else {
            let rows = self.new_text.offset_to_point(self.prefix).row;
            (self.follow_between(at - self.prefix))
                .map(|to| to + self.prefix)
                .map_err(|row| row + rows)
        }?;
        match self.new.as_bytes().get(to) == Some(&self.old.as_bytes()[at]) {
            true => Ok(to),
            false => Err(self.new_text.offset_to_point(to.min(new_len)).row),
        }
    }

    /// Where the byte at `at` of the old rows between is in the new.
    fn follow_between(&self, at: usize) -> Result<usize, u32> {
        let Some((old, new, diff)) = &self.between else {
            let row = |offset| self.old_text.offset_to_point(offset).row;
            return Err(row(self.prefix + at) - row(self.prefix));
        };
        let row = old.offset_to_point(at).row;
        let mut delta: i64 = 0;
        for hunk in diff.hunks() {
            if hunk.left.contains(&row) {
                return follow_in_hunk(hunk, old, new, at);
            }
            if hunk.left.start > row {
                break;
            }
            delta += hunk.right.len() as i64 - hunk.left.len() as i64;
        }
        let moved = (i64::from(row) + delta) as u32;
        match moved < new.row_count() {
            true => Ok(new.row_start(moved) + (at - old.row_start(row))),
            false => Err(new.row_count().saturating_sub(1)),
        }
    }
}

/// Where the byte at `at` of `old`, in rows `hunk` changed, is in `new`, by
/// a diff of the rows' tokens ([`tokens`]). Past [`TOKEN_DIFF_BYTES`], by
/// the whole words the rows start and end with alike, and by token between
/// those, if that's within it. In a word a diff of them changed, as one
/// word, by the bytes it starts and ends with alike. Else `Err`, with the
/// row of `new` it'd be near.
fn follow_in_hunk(hunk: &Hunk, old: &Text, new: &Text, at: usize) -> Result<usize, u32> {
    let row = old.offset_to_point(at).row - hunk.left.start;
    let near = hunk.right.start + row.min(hunk.right.len().saturating_sub(1) as u32);
    let span = |text: &Text, rows: &Range<u32>| {
        let end = |row| match row < text.row_count() {
            true => text.row_start(row),
            false => text.len(),
        };
        end(rows.start)..end(rows.end)
    };
    let (from, to) = (span(old, &hunk.left), span(new, &hunk.right));
    let (was, is) = (
        old.slice_to_string(from.clone()),
        new.slice_to_string(to.clone()),
    );
    let at = at - from.start;
    let (head, tail) = match was.len().max(is.len()) <= TOKEN_DIFF_BYTES {
        true => (0, 0),
        false => {
            let (head, tail) = alike_ends(&was, &is, true);
            if at < head {
                return Ok(to.start + at);
            }
            if at >= was.len() - tail {
                return Ok(to.end - (was.len() - at));
            }
            (head, tail)
        }
    };
    let (was, is) = (&was[head..was.len() - tail], &is[head..is.len() - tail]);
    if was.len().max(is.len()) > TOKEN_DIFF_BYTES {
        return Err(near);
    }
    let (at, to) = (at - head, to.start + head..to.end - tail);
    let (before, after) = (Tokens(tokens(was)), Tokens(tokens(is)));
    let starts = |tokens: &Tokens| {
        let lens = tokens.0.iter().map(|token| token.len());
        lens.scan(0, |at, len| Some(std::mem::replace(at, *at + len)))
            .collect::<Vec<_>>()
    };
    let (before_starts, after_starts) = (starts(&before), starts(&after));
    let ix = before_starts.partition_point(|&start| start <= at) - 1;
    let input = InternedInput::new(before, after);
    let mut delta: i64 = 0;
    let span = |starts: &[usize], len: usize, tokens: Range<u32>| {
        let end = |ix: u32| starts.get(ix as usize).copied().unwrap_or(len);
        end(tokens.start)..end(tokens.end)
    };
    for hunk in Diff::compute(Algorithm::Myers, &input).hunks() {
        if hunk.before.contains(&(ix as u32)) {
            if hunk.before.len() != 1 || hunk.after.len() != 1 {
                return Err(near);
            }
            let changed = span(&before_starts, was.len(), hunk.before);
            let into = span(&after_starts, is.len(), hunk.after);
            return follow_by_ends(&was[changed.clone()], &is[into.clone()], at - changed.start)
                .map(|at| to.start + into.start + at)
                .ok_or(near);
        }
        if hunk.before.start > ix as u32 {
            break;
        }
        delta += hunk.after.len() as i64 - hunk.before.len() as i64;
    }
    let moved = (ix as i64 + delta) as usize;
    Ok(to.start + after_starts[moved] + (at - before_starts[ix]))
}

/// Where the byte at `at` of `was`, a token that became `is`, is in `is`:
/// in as many bytes as they start with alike, from their start, and in as
/// many as they end with alike, from their end ([`alike_ends`]).
fn follow_by_ends(was: &str, is: &str, at: usize) -> Option<usize> {
    let (head, tail) = alike_ends(was, is, false);
    if at < head {
        Some(at)
    } else if at >= was.len() - tail {
        Some(is.len() - (was.len() - at))
    } else {
        None
    }
}

/// How many bytes `a` and `b` start with alike, and end with after those,
/// in whole chars, and in whole `words` too if asked: so that what's alike
/// doesn't end, or start, in the middle of a word of either.
fn alike_ends(a: &str, b: &str, words: bool) -> (usize, usize) {
    let splits = |text: &str, at: usize| {
        words
            && text[..at].chars().next_back().is_some_and(is_word)
            && text[at..].chars().next().is_some_and(is_word)
    };
    let agree = |a: &mut dyn Iterator<Item = u8>, b: &mut dyn Iterator<Item = u8>| {
        a.zip(b).take_while(|(a, b)| a == b).count()
    };
    let mut head = agree(&mut a.bytes(), &mut b.bytes());
    while !a.is_char_boundary(head) || !b.is_char_boundary(head) {
        head -= 1;
    }
    while splits(a, head) || splits(b, head) {
        head = a[..head].char_indices().next_back().map_or(0, |(at, _)| at);
    }
    let mut tail =
        agree(&mut a.bytes().rev(), &mut b.bytes().rev()).min(a.len().min(b.len()) - head);
    while !a.is_char_boundary(a.len() - tail) || !b.is_char_boundary(b.len() - tail) {
        tail -= 1;
    }
    while splits(a, a.len() - tail) || splits(b, b.len() - tail) {
        tail -= a[a.len() - tail..].chars().next().map_or(0, char::len_utf8);
    }
    (head, tail)
}

/// A text's tokens, for a diff of them: words, runs of white space, and
/// each other char.
struct Tokens<'a>(Vec<&'a str>);

impl<'a> TokenSource for Tokens<'a> {
    type Token = &'a str;
    type Tokenizer = std::vec::IntoIter<&'a str>;

    fn tokenize(&self) -> Self::Tokenizer {
        self.0.clone().into_iter()
    }

    fn estimate_tokens(&self) -> u32 {
        self.0.len() as u32
    }
}

/// Whether `c` is in a word, for [`tokens`].
fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `text` split into [`Tokens`].
fn tokens(text: &str) -> Vec<&str> {
    let class = |c: char| match c {
        c if is_word(c) => 0,
        c if c.is_whitespace() => 1,
        _ => 2,
    };
    let mut out = Vec::new();
    let (mut start, mut last) = (0, None);
    for (at, c) in text.char_indices() {
        let kind = class(c);
        if at > start && (kind == 2 || last != Some(kind)) {
            out.push(&text[start..at]);
            start = at;
        }
        last = Some(kind);
    }
    if start < text.len() {
        out.push(&text[start..]);
    }
    out
}

impl Mapper<'_> {
    /// `range` of `old` found again in `new`, where [`Mapper::map`] lost
    /// it: where its rows are, whole, in `new` just once, with as many rows
    /// around them as it takes to tell, up to [`FIND_CONTEXT_ROWS`] on each
    /// side. Where the texts were diffed, only in rows the diff says are new
    /// ([`Mapper::fresh`]); where not, with a row around them at least.
    /// `None` where they aren't there, or are in more than one place.
    fn find(&self, range: Range<usize>) -> Option<Range<usize>> {
        let (old, old_text) = (self.old, self.old_text);
        let rows = old_text.row_count();
        let row_of = |offset| old_text.offset_to_point(offset).row;
        let (mut first, mut last) = (row_of(range.start), row_of(range.start));
        if !range.is_empty() {
            last = row_of(old_text.clip_offset(range.end - 1, Bias::Left));
        }
        if self.between.is_none() {
            if first == 0 && last + 1 >= rows {
                return None;
            }
            (first, last) = (first.saturating_sub(1), (last + 1).min(rows - 1));
        }
        let row_end = |row: u32| match row + 1 < rows {
            true => old_text.row_start(row + 1),
            false => old.len(),
        };
        for _ in 0..=FIND_CONTEXT_ROWS {
            let from = old_text.row_start(first);
            let fresh = |at: usize| self.fresh(at + range.start - from);
            match whole_rows(self.new, &old[from..row_end(last)], fresh) {
                (Some(at), false) => return Some(at + range.start - from..at + range.end - from),
                (None, _) => return None,
                (Some(_), true) => {}
            }
            if first == 0 && last + 1 >= rows {
                return None;
            }
            (first, last) = (first.saturating_sub(1), (last + 1).min(rows - 1));
        }
        None
    }

    /// Whether `at` of `new` is in rows the row diff says are new: not
    /// rows it kept, which are other rows of `old`. Without a diff, any.
    fn fresh(&self, at: usize) -> bool {
        let Some((_, new, diff)) = &self.between else {
            return true;
        };
        if at < self.prefix || at >= self.new.len() - self.suffix {
            return false;
        }
        let row = new.offset_to_point(at - self.prefix).row;
        diff.hunks().iter().any(|hunk| hunk.right.contains(&row))
    }
}

/// Where `rows`, whole rows of a text, are whole rows of `text`, at places
/// `ok`: the first, and whether there's another. Rows that don't end in a
/// line break end the text they're from, and end a row of `text`.
fn whole_rows(text: &str, rows: &str, ok: impl Fn(usize) -> bool) -> (Option<usize>, bool) {
    let Some(first) = rows.chars().next() else {
        return (None, false);
    };
    let bytes = text.as_bytes();
    let starts_row = |at: usize| at == 0 || matches!(bytes[at - 1], b'\n' | b'\r');
    let ends_row = |end: usize| {
        rows.ends_with('\n') || end == text.len() || matches!(bytes[end], b'\n' | b'\r')
    };
    let (mut found, mut from) = (None, 0);
    while let Some(at) = text[from..].find(rows).map(|at| from + at) {
        if starts_row(at) && ends_row(at + rows.len()) && ok(at) {
            if found.is_some() {
                return (found, true);
            }
            found = Some(at);
        }
        from = at + first.len_utf8();
    }
    (found, false)
}

/// The widget: see the [module docs](self).
pub(crate) struct PeekView {
    workbench: WeakEntity<Workbench>,
    pane: WeakEntity<Pane>,
    /// The caret's row the peek sits under.
    row: u32,
    /// How many rows its block is.
    rows: u32,
    /// The editor's buffer version last checked, and where the peek goes
    /// in it: an edit before its `limit` is above the peek, and at the
    /// text's end, `at_end`, an insertion there is too.
    checked: Version,
    placed: Placement,
    targets: Vec<Target>,
    selected: usize,
    /// Peek References' list, which its targets follow
    /// ([`references`]).
    references: Option<Entity<ReferencesPanel>>,
    /// The selected target's file.
    path: Option<PathBuf>,
    excerpt: Option<Excerpt>,
    /// Why the selected target's file couldn't be read.
    error: Option<SharedString>,
    /// Reading the selected target's file.
    loading: Option<Task<()>>,
    theme: EditorTheme,
    /// Around the excerpt, which takes focus from it once it's there.
    focus: FocusHandle,
    /// The block's size in the last frame.
    size: Size<Pixels>,
    /// The editor's text and gutter in the last frame: when they're
    /// resized, the peek is shown whole again.
    viewport: Option<Size<Pixels>>,
    /// Scrolls the editor once so the whole peek is in view.
    reveal: bool,
    closed: bool,
    /// For the locations in files as on disk, each file as read when the
    /// peek opened ([`baseline`]): a buffer opened on one since maps the
    /// location through its edits ([`map_through`]).
    baselines: HashMap<PathBuf, Arc<str>>,
    /// The files whose baselines are being read, and whether the selected
    /// target waits for its file's to show ([`PeekView::place`]).
    reading: Vec<PathBuf>,
    waiting: bool,
    /// Streamed targets' files waiting for a read, and whether one is
    /// running ([`PeekView::read_streamed`]).
    queued: Vec<PathBuf>,
    fetching: bool,
    /// Files asked for that got no baseline: not asked for again.
    skipped: Vec<PathBuf>,
    _baselines: Task<()>,
    _streamed: Option<Task<()>>,
    _observe: Subscription,
    _references: Vec<Subscription>,
}

/// Where `editor`'s caret is and its text's version, for an answer to land
/// at.
pub(crate) fn asked_at(editor: &Entity<EditorView>, cx: &App) -> (usize, Version) {
    let model = editor.read(cx).model();
    (model.primary_selection().head(), model.snapshot().version())
}

impl Workbench {
    /// Whether a peek can open in the active editor: it's the pane's own,
    /// not a diff's side.
    pub(crate) fn can_peek(&self, cx: &App) -> bool {
        let editor = self.active_editor(cx);
        editor.is_some_and(|editor| shows(self.pane().read(cx), editor))
    }

    /// Where `location` is now ([`Source`]).
    fn peek_source(&self, location: &Location, cx: &App) -> Option<Source> {
        let (path, reveal) = self.target_of(location, cx)?;
        let open = (self.open_editors(cx).into_iter())
            .find(|editor| editor.read(cx).model().path() == Some(path.as_path()))
            .map(|editor| editor.read(cx).model().split());
        Some(Source { path, reveal, open })
    }

    /// Opens a peek in `editor`, the active one, on `items`, under its
    /// caret's row, focused: in place of the pane's last.
    pub(crate) fn open_peek(
        &mut self,
        editor: &Entity<EditorView>,
        items: Vec<fe_nav::Definition>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let targets = (items.into_iter()).map(|item| Target::at(item.location));
        self.open_peek_on(editor, targets.collect(), None, window, cx);
    }

    /// Opens a peek in `editor` on `targets`, as [`Workbench::open_peek`],
    /// or on what `references` finds as it finds it, that list focused.
    fn open_peek_on(
        &mut self,
        editor: &Entity<EditorView>,
        targets: Vec<Target>,
        references: Option<Entity<ReferencesPanel>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pane = self.pane().clone();
        close(&pane, false, window, cx);
        // The files the locations on disk are in, each once.
        let mut on_disk: Vec<PathBuf> = (targets.iter())
            .filter(|target| target.location.snapshot.is_none())
            .filter_map(|target| Some(self.target_of(&target.location, cx)?.0))
            .collect();
        on_disk.sort();
        on_disk.dedup();
        let reading = on_disk.clone();
        let requested = on_disk.clone();
        let source = (targets.first()).and_then(|first| self.peek_source(&first.location, cx));
        if source.is_none() && references.is_none() {
            return;
        }
        let (row, placed, version, shown) = {
            let view = editor.read(cx);
            let model = view.model();
            let row = model
                .text()
                .offset_to_point(model.primary_selection().head())
                .row;
            let shown = view.viewport_rows().map_or(ROWS, |(_, rows)| rows);
            let placed = Placement::under(model, row);
            (row, placed, model.snapshot().version(), shown)
        };
        let rows = fit(shown);
        editor.update(cx, |editor, cx| {
            reserve(editor, placed.above, rows, cx);
            editor.set_key_context_flag(SHOWN, true, cx);
        });
        let theme = pane.read(cx).theme().clone();
        let workbench = cx.entity().downgrade();
        let (weak_pane, weak_editor) = (pane.downgrade(), editor.downgrade());
        let view = cx.new(|cx| {
            let observe = cx.observe_in(editor, window, PeekView::editor_changed);
            let followed = (references.as_ref())
                .map(|panel| PeekView::follow(panel, window, cx))
                .unwrap_or_default();
            #[cfg(test)]
            let hold = HOLD_BASELINES.with(|hold| hold.borrow_mut().take());
            let baselines = cx.spawn_in(window, async move |view, cx| {
                #[cfg(test)]
                if let Some(hold) = hold {
                    hold.recv().await.ok();
                }
                let read = cx
                    .background_spawn(async move { baselines(on_disk, BASELINE_BUDGET) })
                    .await;
                view.update_in(cx, |view, window, cx| {
                    view.baselines_read(requested, read, window, cx)
                })
                .ok();
            });
            let mut view = PeekView {
                workbench,
                pane: weak_pane,
                row,
                rows,
                checked: version,
                placed,
                targets,
                selected: 0,
                references,
                path: None,
                excerpt: None,
                error: None,
                loading: None,
                theme,
                focus: cx.focus_handle(),
                size: Size::default(),
                viewport: None,
                reveal: true,
                closed: false,
                baselines: HashMap::new(),
                reading,
                waiting: false,
                queued: Vec::new(),
                fetching: false,
                skipped: Vec::new(),
                _baselines: baselines,
                _streamed: None,
                _observe: observe,
                _references: followed,
            };
            let focus = match &view.references {
                Some(panel) => panel.focus_handle(cx),
                None => view.focus.clone(),
            };
            window.focus(&focus, cx);
            if let Some(source) = source {
                view.place(0, source, window, cx);
            }
            view
        });
        pane.update(cx, |pane, cx| {
            pane.peek = Some(Peek {
                view,
                editor: weak_editor,
            });
            cx.notify();
        });
    }

    /// The pane whose peek, or whose peek's editor, has focus, else the
    /// active pane.
    fn peek_pane(&self, window: &Window, cx: &App) -> Entity<Pane> {
        let focused = self.panes(cx).into_iter().find(|pane| {
            (pane.read(cx).peek.as_ref()).is_some_and(|peek| {
                let editor = peek.editor.upgrade();
                peek.view.read(cx).focus.contains_focused(window, cx)
                    || editor.is_some_and(|editor| editor.focus_handle(cx).is_focused(window))
            })
        });
        focused.unwrap_or_else(|| self.pane().clone())
    }

    /// Closes the focused peek, focusing its editor: Esc. A peek still
    /// being asked for won't open.
    pub(crate) fn close_peek(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.nav_state.goto.next();
        let pane = self.peek_pane(window, cx);
        close(&pane, true, window, cx);
        // Its Esc flag for a peek still asked for goes with the request.
        if let Some(editor) = self.active_editor(cx).cloned() {
            editor.update(cx, |_, cx| cx.notify());
        }
    }

    /// Closes the peek open in `editor`, if any: focusing the editor if the
    /// peek had focus.
    pub(crate) fn close_peek_in(
        &mut self,
        editor: &Entity<EditorView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pane = self.panes(cx).into_iter().find(|pane| {
            (pane.read(cx).peek.as_ref()).is_some_and(|peek| peek.editor == editor.downgrade())
        });
        if let Some(pane) = pane {
            let peek = pane.read(cx).peek.as_ref().map(|peek| peek.view.clone());
            let focused = peek.is_some_and(|peek| peek.read(cx).focus.contains_focused(window, cx));
            close(&pane, focused, window, cx);
        }
    }

    /// Opens the focused peek's file at its excerpt's selection, as Enter
    /// does in it.
    pub(crate) fn open_from_peek(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pane = self.peek_pane(window, cx);
        let peek = pane.read(cx).peek.as_ref().map(|peek| peek.view.clone());
        if let Some(opening) = peek.and_then(|peek| peek.read(cx).opening(cx)) {
            self.open_peeked(opening, window, cx);
        }
    }

    /// Closes the active pane's peek and opens `path` there with `range`
    /// selected, recording where the editor's caret was. A click in a peek
    /// makes its pane the active one.
    fn open_peeked(&mut self, opening: Opening, window: &mut Window, cx: &mut Context<Self>) {
        let Opening { path, range, read } = opening;
        let open = (self.open_editors(cx).into_iter())
            .find(|editor| editor.read(cx).model().path() == Some(path.as_path()));
        let reveal = match (read, open) {
            (Some(read), Some(open)) => map_through(&read, open.read(cx).model().text(), range),
            _ => Reveal::bytes(range),
        };
        let pane = self.pane().clone();
        close(&pane, false, window, cx);
        self.nav_state.goto.next();
        self.push_history(cx);
        pane.update(cx, |pane, cx| pane.open_at(path, Some(reveal), window, cx));
    }
}

/// Closes `pane`'s peek, if it has one: its rows go from its editor, which
/// gets focus if `refocus`. It doesn't update the peek, which may be what
/// closes it.
fn close(pane: &Entity<Pane>, refocus: bool, window: &mut Window, cx: &mut App) {
    let Some(peek) = pane.update(cx, |pane, cx| {
        cx.notify();
        pane.peek.take()
    }) else {
        return;
    };
    let Some(editor) = peek.editor.upgrade() else {
        return;
    };
    unreserve(&editor, cx);
    if refocus {
        window.focus(&editor.focus_handle(cx), cx);
    }
}

/// Takes a peek's rows and key context flag out of `editor`.
fn unreserve(editor: &Entity<EditorView>, cx: &mut App) {
    editor.update(cx, |editor, cx| {
        let blocks = (editor.blocks().iter())
            .filter(|block| block.id != BLOCK_ID)
            .cloned()
            .collect();
        editor.set_blocks(blocks, cx);
        editor.set_key_context_flag(SHOWN, false, cx);
    });
}

/// Before `pane` draws: a peek whose editor left the pane closes, and an
/// editor that came into it with a peek's rows, its peek left in another
/// pane, loses them.
pub(crate) fn sync(pane: &mut Pane, cx: &mut Context<Pane>) {
    let has = |pane: &Pane, editor: &Entity<EditorView>| {
        (pane.tabs().iter()).any(|tab| tab.editor() == Some(editor))
    };
    let left = (pane.peek.as_ref()).is_some_and(|peek| {
        peek.editor
            .upgrade()
            .is_none_or(|editor| !has(pane, &editor))
    });
    let mut stale = Vec::new();
    if left && let Some(peek) = pane.peek.take() {
        stale.extend(peek.editor.upgrade());
    }
    let mine = pane.peek.as_ref().and_then(|peek| peek.editor.upgrade());
    if let Some(editor) = pane.active_editor()
        && Some(editor) != mine.as_ref()
        && editor
            .read(cx)
            .blocks()
            .iter()
            .any(|block| block.id == BLOCK_ID)
    {
        stale.push(editor.clone());
    }
    if !stale.is_empty() {
        cx.defer(move |cx| {
            for editor in stale {
                unreserve(&editor, cx);
            }
        });
    }
}

/// The pane's peek drawn over its rows, while its editor shows in the
/// pane: prepainted after the editor, so where the editor's frame laid out
/// the rows ([`EditorView::reserved_blocks`]), clipped to the editor. The
/// first frame that has them scrolls the editor to show them whole, the
/// caret's row above them.
pub(crate) fn layer(pane: &Pane) -> Option<impl IntoElement + use<>> {
    let peek = pane.peek.as_ref()?;
    let editor = peek.editor.upgrade()?;
    if !shows(pane, &editor) {
        return None;
    }
    let view = peek.view.clone();
    let layer = canvas(
        move |_, window, cx| {
            let block = (editor.read(cx).reserved_blocks().iter())
                .find(|block| block.id == BLOCK_ID)
                .copied()?;
            let (bounds, clip) = (block.bounds, block.clip);
            let reveal = view.update(cx, |view, _| {
                view.size = bounds.size;
                // Resized, whether or not its rows are.
                let was = view.viewport.replace(clip.size);
                let resized = was.is_some_and(|was| was != clip.size);
                std::mem::take(&mut view.reveal) || resized
            });
            let rows = view.read(cx).rows.max(1) as f32;
            let line_height = f32::from(bounds.size.height) / rows;
            // Resized: the block fits the view again, from the next frame.
            let shown = (f32::from(clip.size.height) / line_height).floor() as u32;
            if fit(shown) != view.read(cx).rows {
                let (view, editor) = (view.clone(), editor.clone());
                window.defer(cx, move |_, cx| {
                    view.update(cx, |view, cx| view.refit(shown, &editor, cx));
                });
            }
            // Below the view, it scrolls up as far as the rows run past
            // it; above it, down to the caret's row.
            let caret_top = bounds.top() - gpui::px(line_height);
            let past = if bounds.bottom() > clip.bottom() {
                (f32::from(bounds.bottom() - clip.bottom()) / line_height).ceil()
            } else if caret_top < clip.top() {
                (f32::from(caret_top - clip.top()) / line_height).floor()
            } else {
                0.
            };
            if reveal && past != 0. {
                let editor = editor.clone();
                window.defer(cx, move |_, cx| {
                    editor.update(cx, |editor, cx| {
                        let top = editor.scroll_top() + past;
                        editor.set_scroll_top(top, cx);
                    });
                });
            }
            let mut element: AnyElement = view.clone().into_any_element();
            window.with_content_mask(Some(ContentMask { bounds: clip }), |window| {
                let space = bounds.size.map(AvailableSpace::Definite);
                element.prepaint_as_root(bounds.origin, space, window, cx);
            });
            Some((element, clip))
        },
        |_, prepainted, window, cx| {
            if let Some((mut element, clip)) = prepainted {
                window.with_content_mask(Some(ContentMask { bounds: clip }), |window| {
                    element.paint(window, cx)
                });
            }
        },
    );
    Some(layer.absolute().top_0().left_0().size_full())
}

impl PeekView {
    /// The editor's text, caret or folds changed: an edit above the peek,
    /// or the caret off its row, closes it; a fold opened or closed under
    /// the caret's row moves it to the first row shown after it.
    fn editor_changed(
        &mut self,
        editor: Entity<EditorView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.closed {
            return;
        }
        let model = editor.read(cx).model();
        let snapshot = model.snapshot();
        let (version, placed) = (self.checked, self.placed);
        // At the text's end the rows above run to it, an insertion there
        // included.
        let above = |edit: &fe_buffer::Edit| {
            edit.old.start < placed.limit || (placed.at_end && edit.old.start == placed.limit)
        };
        let edited_above = version != snapshot.version()
            && (version < snapshot.oldest_version()
                || (snapshot.edits_since(version)).any(|batch| batch.iter().any(above)));
        self.checked = snapshot.version();
        let text = model.text();
        let caret_row = text.offset_to_point(model.primary_selection().head()).row;
        if edited_above || caret_row != self.row {
            return self.close(window, cx);
        }
        let now = Placement::under(model, self.row);
        if now != placed {
            self.placed = now;
            let rows = self.rows;
            editor.update(cx, |editor, cx| reserve(editor, now.above, rows, cx));
        }
    }

    /// The baselines are `read`: a target waiting for its file's shows
    /// now ([`PeekView::place`]). `requested` are the files asked for, read
    /// or skipped.
    fn baselines_read(
        &mut self,
        requested: Vec<PathBuf>,
        read: Vec<(PathBuf, Arc<str>)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.baselines.extend(read);
        self.reading.retain(|path| !requested.contains(path));
        if std::mem::take(&mut self.waiting) {
            self.reselect(self.selected, window, cx);
        }
    }

    /// Notes the `requested` files `read` has none of.
    fn skip_unread(&mut self, requested: &[PathBuf], read: &[(PathBuf, Arc<str>)]) {
        let unread = requested
            .iter()
            .filter(|path| !read.iter().any(|(p, _)| p == *path));
        self.skipped.extend(unread.cloned());
    }

    /// Reads baselines for the files of `targets` that came in since the
    /// peek opened, each file once, within the budget the files read so far
    /// leave.
    fn read_streamed(&mut self, targets: &[Target], window: &mut Window, cx: &mut Context<Self>) {
        let Some(workbench) = self.workbench.upgrade() else {
            return;
        };
        let workbench = workbench.read(cx);
        for target in targets.iter().filter(|t| t.location.snapshot.is_none()) {
            let Some((path, _)) = workbench.target_of(&target.location, cx) else {
                continue;
            };
            let known = self.baselines.contains_key(&path)
                || self.reading.contains(&path)
                || self.skipped.contains(&path);
            if !known {
                self.reading.push(path.clone());
                self.queued.push(path);
            }
        }
        if self.fetching || self.queued.is_empty() {
            return;
        }
        self.fetching = true;
        self._streamed = Some(cx.spawn_in(window, async move |view, cx| {
            loop {
                let next = view.update(cx, |view, _| {
                    if view.queued.is_empty() {
                        view.fetching = false;
                        return None;
                    }
                    let spent = (view.baselines.values()).map(|text| text.len() as u64);
                    let left = BASELINE_BUDGET.saturating_sub(spent.sum());
                    Some((std::mem::take(&mut view.queued), left))
                });
                let Ok(Some((paths, left))) = next else {
                    return;
                };
                let requested = paths.clone();
                let read = cx
                    .background_spawn(async move { baselines(paths, left) })
                    .await;
                let done = view.update_in(cx, |view, window, cx| {
                    view.skip_unread(&requested, &read);
                    view.baselines_read(requested, read, window, cx)
                });
                if done.is_err() {
                    return;
                }
            }
        }));
    }

    /// The view shows `shown` rows now: the block fits them again, the
    /// definition, where it is now, back in the excerpt's view, and the
    /// editor scrolls to show it whole.
    fn refit(&mut self, shown: u32, editor: &Entity<EditorView>, cx: &mut Context<Self>) {
        let rows = fit(shown);
        if rows == self.rows {
            return;
        }
        self.rows = rows;
        self.reveal = true;
        let above = self.placed.above;
        editor.update(cx, |editor, cx| reserve(editor, above, rows, cx));
        let context = self.context();
        if let Some(excerpt) = &self.excerpt {
            let (line, _) = excerpt.place(cx);
            let top = (line - 1).saturating_sub(context) as f32;
            excerpt
                .view
                .update(cx, |view, cx| view.set_scroll_top(top, cx));
        }
        cx.notify();
    }

    /// Closes it from its close button: a peek still being asked for won't
    /// open either.
    fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        (self.workbench)
            .update(cx, |workbench, _| workbench.nav_state.goto.next())
            .ok();
        self.close(window, cx);
    }

    /// Closes it, focusing the editor if it had focus.
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.closed = true;
        let focused = self.focus.contains_focused(window, cx);
        if let Some(pane) = self.pane.upgrade() {
            close(&pane, focused, window, cx);
        }
    }

    /// The file to open and what to select there: the selected target's,
    /// at its excerpt's selection.
    fn opening(&self, cx: &App) -> Option<Opening> {
        let path = self.shown_path(cx)?;
        let excerpt = self.excerpt.as_ref()?;
        let model = excerpt.view.read(cx).model();
        let range = model.primary_selection().range.clone();
        let read = (excerpt.read).then(|| model.text().to_string().into());
        Some(Opening { path, range, read })
    }

    /// The file shown: an open editor's buffer's where it is now, renamed
    /// or saved elsewhere since, or the file read.
    fn shown_path(&self, cx: &App) -> Option<PathBuf> {
        match &self.excerpt {
            Some(excerpt) if !excerpt.read => {
                let model = excerpt.view.read(cx).model();
                model
                    .path()
                    .map(ToOwned::to_owned)
                    .or_else(|| self.path.clone())
            }
            _ => self.path.clone(),
        }
    }

    /// Opens what [`PeekView::opening`] says, from a double-click.
    fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(opening) = self.opening(cx) else {
            return;
        };
        self.closed = true;
        (self.workbench)
            .update(cx, |workbench, cx| {
                workbench.open_peeked(opening, window, cx)
            })
            .ok();
    }

    /// Shows the target at `ix` in the excerpt, where it is now, keeping
    /// focus there.
    fn select(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix == self.selected && self.excerpt.is_some() {
            return;
        }
        self.reselect(ix, window, cx);
    }

    /// Shows the target at `ix`, where it is now, though it shows already.
    fn reselect(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let target = &self.targets[ix];
        let source = (self.workbench.upgrade())
            .and_then(|workbench| workbench.read(cx).peek_source(&target.location, cx));
        let Some(source) = source else {
            return;
        };
        // Kept for the next excerpt as this one goes.
        let excerpt = self
            .excerpt
            .as_ref()
            .map(|excerpt| excerpt.view.focus_handle(cx));
        if excerpt.is_some_and(|excerpt| excerpt.contains_focused(window, cx)) {
            window.focus(&self.focus, cx);
        }
        self.place(ix, source, window, cx);
    }

    /// Shows the target at `ix`, found at `source`. A location on disk in a
    /// file open in an editor is mapped from the file's baseline into its
    /// buffer ([`map_through`]), and shows nothing until that's read; with
    /// no baseline, it shows from the file as on disk, which the location
    /// is in.
    fn place(
        &mut self,
        ix: usize,
        mut source: Source,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let location = &self.targets[ix].location;
        if location.snapshot.is_none()
            && let Some(open) = &source.open
        {
            match self.baselines.get(&source.path) {
                Some(baseline) => {
                    source.reveal = map_through(baseline, open.text(), location.range.clone());
                }
                None if self.reading.contains(&source.path) => {
                    self.clear(ix, source.path);
                    self.waiting = true;
                    return cx.notify();
                }
                None => source.open = None,
            }
        }
        self.show(ix, source, window, cx);
    }

    /// Selects the target at `ix`, in the file at `path`, showing none yet.
    fn clear(&mut self, ix: usize, path: PathBuf) {
        self.selected = ix;
        self.excerpt = None;
        self.error = None;
        self.loading = None;
        self.waiting = false;
        self.path = Some(path);
    }

    /// Shows the target at `ix`, found at `source`: sharing its open
    /// editor's buffer, or read from disk in the background.
    fn show(&mut self, ix: usize, source: Source, window: &mut Window, cx: &mut Context<Self>) {
        self.clear(ix, source.path.clone());
        cx.notify();
        let Source { path, reveal, open } = source;
        if let Some(model) = open {
            return self.set_excerpt(model, &reveal, false, window, cx);
        }
        self.loading = Some(cx.spawn_in(window, async move |view, cx| {
            let read = {
                let path = path.clone();
                cx.background_spawn(async move { read(&path) }).await
            };
            view.update_in(cx, |view, window, cx| {
                view.loading = None;
                match read {
                    Ok(buffer) => {
                        let model = EditorModel::new(buffer, Some(path));
                        view.set_excerpt(model, &reveal, true, window, cx);
                    }
                    Err(error) => {
                        let name = crate::pane::file_name(Some(&path));
                        view.error = Some(format!("Couldn't show {name}: {error}").into());
                        cx.notify();
                    }
                }
            })
            .ok();
        }));
    }

    /// How many of [`CONTEXT_ROWS`] to show above the definition: as many
    /// as leave it in the excerpt's view, under the header.
    fn context(&self) -> u32 {
        let theme = &self.theme;
        let row = f32::from((theme.font_size * theme.line_height).round()).max(1.);
        let height = row * self.rows as f32 - f32::from(HEADER_HEIGHT + BORDER * 2.);
        let shown = (height / row).floor().max(0.) as u32;
        CONTEXT_ROWS.min(shown.saturating_sub(1) / 2)
    }

    /// Shows `model`, the selected target's file (as `read` from disk, or
    /// an open editor's), read-only, with the definition at `reveal`
    /// selected and highlighted, as many of [`CONTEXT_ROWS`] above it as
    /// leave it in view ([`PeekView::context`]), and across to it.
    fn set_excerpt(
        &mut self,
        mut model: EditorModel,
        reveal: &Reveal,
        read: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = reveal.range(model.text());
        let text = model.text();
        let point = text.offset_to_point(range.start);
        let row_start = text.row_start(point.row);
        let snapshot = model.snapshot();
        let highlight_start = snapshot.anchor_after(range.start);
        let highlight = Highlight {
            start: highlight_start,
            end: snapshot.anchor_before(range.end),
            row: snapshot.anchor_after(row_start),
        };
        model.set_soft_wrap(false);
        model.add_decoration_provider(Rc::new(RefCell::new(highlight)));
        model.set_selections([ResolvedSelection {
            range: range.clone(),
            reversed: false,
        }]);
        let context = self.context();
        let theme = self.theme.clone();
        let view = cx.new(|cx| {
            let mut view = EditorView::new(model, theme, cx);
            view.set_read_only(true, cx);
            view.set_scroll_top(point.row.saturating_sub(context) as f32, cx);
            // Across to it, a long row's far end too; down only if the rows
            // above it don't leave it in view.
            view.select_and_reveal(range, cx);
            view
        });
        if self.focus.is_focused(window) {
            window.focus(&view.focus_handle(cx), cx);
        }
        self.excerpt = Some(Excerpt {
            view,
            start: highlight_start,
            read,
        });
        cx.notify();
    }

    /// The definitions' labels, in the list when there's more than one.
    #[cfg(test)]
    pub(crate) fn list(&self) -> Vec<SharedString> {
        match self.targets.len() > 1 {
            true => self
                .targets
                .iter()
                .map(|target| target.label.clone())
                .collect(),
            false => Vec::new(),
        }
    }

    /// The excerpt's editor, once it's there.
    #[cfg(test)]
    pub(crate) fn excerpt(&self) -> Option<&Entity<EditorView>> {
        self.excerpt.as_ref().map(|excerpt| &excerpt.view)
    }

    /// The header's text: the file's name and the definition's line:col.
    #[cfg(test)]
    pub(crate) fn header(&self, cx: &App) -> Option<String> {
        let (line, column) = self.excerpt.as_ref()?.place(cx);
        let name = crate::pane::file_name(self.shown_path(cx).as_deref());
        Some(format!("{name} {line}:{column}"))
    }
}

/// Reads `path` as the pane would load it ([`loader::read`]): unless it's
/// one the pane opens in a viewer, unread ([`loader::is_large`]).
fn read(path: &std::path::Path) -> Result<Buffer, String> {
    if loader::is_large(path) {
        return Err("too large to show here".into());
    }
    match loader::read(path.to_owned()) {
        Ok(Some(buffer)) => Ok(buffer),
        Ok(None) => Err("not found".into()),
        Err(error) => Err(error.to_string()),
    }
}

/// The most bytes of baselines a peek reads and keeps, all its files'.
const BASELINE_BUDGET: u64 = 32 * 1024 * 1024;

/// Baselines of `paths`, in order, as long as the files read stay within
/// `budget` bytes on disk: the rest, and any file [`baseline`] skips, get
/// none, which leaves their locations as the answer gave them.
fn baselines(paths: Vec<PathBuf>, budget: u64) -> Vec<(PathBuf, Arc<str>)> {
    let mut spent = 0;
    let mut out = Vec::new();
    for path in paths {
        // Only files a baseline is read for count: on disk before, as kept
        // after.
        let Some(len) = baseline_len(&path) else {
            continue;
        };
        if spent + len > budget {
            continue;
        }
        if let Some(text) = baseline(&path) {
            spent += text.len() as u64;
            out.push((path, text));
        }
    }
    out
}

/// How many bytes `path` is on disk, if [`baseline`] would read it.
fn baseline_len(path: &std::path::Path) -> Option<u64> {
    let len = std::fs::metadata(path).ok()?.len();
    let eligible = len <= fe_editor::text_diff::MAX_DIFF_BYTES as u64 && !loader::is_large(path);
    eligible.then_some(len)
}

#[cfg(test)]
thread_local! {
    /// The next peek's baselines wait for this to close, in tests.
    pub(crate) static HOLD_BASELINES: RefCell<Option<async_channel::Receiver<()>>> =
        const { RefCell::new(None) };
}

/// `path`'s text as the pane would load it, for a baseline: not for a file
/// too large to diff ([`fe_editor::text_diff::MAX_DIFF_BYTES`]), which
/// [`map_through`] can't map through.
fn baseline(path: &std::path::Path) -> Option<Arc<str>> {
    baseline_len(path)?;
    let buffer = read(path).ok()?;
    Some(buffer.snapshot().text().to_string().into())
}

/// The definition, highlighted in the excerpt: its range, or its row if
/// it's empty.
struct Highlight {
    start: Anchor,
    end: Anchor,
    row: Anchor,
}

impl DecorationProvider for Highlight {
    fn decorations(
        &self,
        snapshot: &BufferSnapshot,
        range: Range<usize>,
        out: &mut Vec<Decoration>,
    ) {
        let (start, end) = (snapshot.resolve(&self.start), snapshot.resolve(&self.end));
        let decoration = match start < end {
            true => Decoration::new(start..end, DecorationKind::Highlight, Tint::Accent),
            false => {
                let at = snapshot.resolve(&self.row);
                Decoration::new(at..at, DecorationKind::RowBackground, Tint::Accent)
            }
        };
        if decoration.range.start <= range.end && decoration.range.end >= range.start {
            out.push(decoration);
        }
    }
}

impl Render for PeekView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ui = fe_ui::ui_theme(cx);
        // Peek References' has none until a reference is found.
        let name = (self.shown_path(cx)).map(|path| crate::pane::file_name(Some(&path)));
        let place = (self.excerpt.as_ref()).map(|excerpt| {
            let (line, column) = excerpt.place(cx);
            format!("{line}:{column}")
        });
        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .flex_shrink_0()
            .h(HEADER_HEIGHT)
            .px_2()
            .gap_2()
            .bg(ui.panel_background)
            .border_b_1()
            .border_color(ui.border)
            .children(name.map(|name| div().text_color(ui.text).child(name)))
            .children(place.map(|place| div().text_color(ui.muted_text).child(place)))
            .child(div().flex_1())
            .child(
                div()
                    .id("peek-close")
                    .px_1()
                    .cursor_pointer()
                    .text_color(ui.muted_text)
                    .hover(|style| style.bg(ui.row_hover))
                    .child("✕")
                    .debug_selector(|| "peek-close".into())
                    .on_click(cx.listener(|view, _, window, cx| view.dismiss(window, cx))),
            );
        // Peek References' list, in place of the definitions'.
        let references = self.references.clone().map(|panel| {
            div()
                .id("peek-list")
                .flex_shrink_0()
                .w(LIST_WIDTH)
                .h_full()
                .flex()
                .border_r_1()
                .border_color(ui.border)
                .child(panel)
        });
        let list = (references.is_none() && self.targets.len() > 1).then(|| {
            let rows = self.targets.iter().enumerate().map(|(ix, target)| {
                div()
                    .id(("peek-row", ix))
                    .debug_selector(move || format!("peek-row-{ix}"))
                    .px_2()
                    .truncate()
                    .cursor_pointer()
                    .when(ix == self.selected, |row| row.bg(ui.row_selected))
                    .hover(|style| style.bg(ui.row_hover))
                    .child(target.label.clone())
                    .on_click(cx.listener(move |view, _, window, cx| view.select(ix, window, cx)))
            });
            div()
                .id("peek-list")
                .flex_shrink_0()
                .w(LIST_WIDTH)
                .h_full()
                .overflow_y_scroll()
                .bg(ui.panel_background)
                .border_r_1()
                .border_color(ui.border)
                .children(rows)
        });
        let body = match (&self.excerpt, &self.error) {
            (Some(excerpt), _) => div()
                .size_full()
                .child(excerpt.view.clone())
                .into_any_element(),
            (None, Some(error)) => div()
                .p_2()
                .text_color(ui.error_text)
                .child(error.clone())
                .into_any_element(),
            (None, None) => div().into_any_element(),
        };
        div()
            .id("peek")
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .occlude()
            // Over the pane's own listener, which it hides.
            .capture_any_mouse_down(cx.listener(|view, _, _, cx| {
                if let Some(pane) = view.pane.upgrade() {
                    pane.update(cx, |_, cx| cx.emit(PaneEvent::Activated));
                }
            }))
            .w(self.size.width)
            .h(self.size.height)
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(ui.background)
            .border_t(BORDER)
            .border_b(BORDER)
            .border_color(ui.accent)
            .child(header)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_row()
                    .children(references)
                    .children(list)
                    .child(
                        div()
                            .id("peek-excerpt")
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            // Once the editor has selected the word.
                            .capture_any_mouse_down(cx.listener(
                                |_, event: &MouseDownEvent, window, cx| {
                                    if event.button == MouseButton::Left && event.click_count == 2 {
                                        cx.defer_in(window, |view, window, cx| {
                                            view.open(window, cx)
                                        });
                                    }
                                },
                            ))
                            .child(body),
                    ),
            )
    }
}

#[cfg(test)]
#[path = "peek_tests.rs"]
mod peek_tests;
