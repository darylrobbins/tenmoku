// SPDX-License-Identifier: Apache-2.0

use std::{cell::Cell, ops::Range, rc::Rc};

use gpui::{
    App, Bounds, Context, ElementInputHandler, EntityInputHandler, EventEmitter, FocusHandle,
    Focusable, HighlightStyle, Hsla, InteractiveElement as _, IntoElement, MouseButton,
    ParentElement as _, Pixels, Point, Render, Role, SharedString, StatefulInteractiveElement,
    Styled as _, StyledText, TextRun, UTF16Selection, UnderlineStyle, WeakEntity, Window,
    accesskit, canvas, div, prelude::FluentBuilder as _, px, size,
};

use kiln_core::{Motion, TextEdit};
use kiln_l10n::Text;
use unicode_segmentation::UnicodeSegmentation as _;

use crate::{ActiveTheme as _, Tokens, UiTheme, focus_ring, radius};

#[cfg(test)]
#[path = "tests/text_input_caret.rs"]
mod caret_tests;

/// One line of text as a screen reader sees it: the node of a
/// [`TextInput`] or a [`crate::TextField`], which holds its text as one
/// `TextRun` child, and the selection in that run. Its node also takes a
/// screen reader's `SetTextSelection` ([`A11yLine::attach`]). It's built
/// only while `window.is_a11y_active()`, so without a screen reader it
/// costs nothing.
///
/// A character, to AccessKit, is a grapheme here, so a screen reader moving
/// the caret by one lands where the arrow keys would; one longer than a
/// `character_lengths` entry holds (255 bytes) is split at a char boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct A11yLine {
    pub(crate) role: Role,
    /// What the run reads: the text, or a bullet a grapheme when masked, so
    /// a password's text appears nowhere in the tree.
    pub(crate) text: String,
    /// The selection's ends, as byte offsets in `text`.
    pub(crate) anchor: usize,
    pub(crate) focus: usize,
    /// Keys don't type: a screen reader doesn't take them for typing.
    pub(crate) read_only: bool,
}

impl A11yLine {
    /// The run's character lengths in UTF-8 bytes: one a grapheme.
    pub(crate) fn character_lengths(&self) -> Vec<u8> {
        let mut lengths = Vec::new();
        for grapheme in self.text.graphemes(true) {
            if let Ok(len) = u8::try_from(grapheme.len()) {
                lengths.push(len);
                continue;
            }
            let mut len = 0usize;
            for ch in grapheme.chars() {
                if len + ch.len_utf8() > usize::from(u8::MAX) {
                    lengths.push(len as u8);
                    len = 0;
                }
                len += ch.len_utf8();
            }
            if len > 0 {
                lengths.push(len as u8);
            }
        }
        lengths
    }

    /// The character index of byte `offset` in `text`: of the character it
    /// starts, or of the one holding it.
    pub(crate) fn character_index(&self, offset: usize) -> usize {
        let mut end = 0;
        (self.character_lengths().into_iter())
            .take_while(|len| {
                end += usize::from(*len);
                end <= offset
            })
            .count()
    }

    /// The byte offset in `text` of character `index`, clamped to the end.
    pub(crate) fn byte_offset(&self, index: usize) -> usize {
        (self.character_lengths().into_iter())
            .take(index)
            .map(usize::from)
            .sum()
    }

    /// The run, as a node to put under the line's node as `run`, and the
    /// selection set on `parent`, the line's node.
    pub(crate) fn write(
        &self,
        parent: &mut accesskit::Node,
        run: accesskit::NodeId,
    ) -> accesskit::Node {
        let mut node = accesskit::Node::new(Role::TextRun);
        node.set_value(self.text.clone());
        node.set_character_lengths(self.character_lengths());
        node.set_text_direction(accesskit::TextDirection::LeftToRight);
        let position = |offset| accesskit::TextPosition {
            node: run,
            character_index: self.character_index(offset),
        };
        parent.set_text_selection(accesskit::TextSelection {
            anchor: position(self.anchor),
            focus: position(self.focus),
        });
        if self.read_only {
            parent.set_read_only();
        }
        node
    }

    /// The byte offsets in `text` of a screen reader's `selection`, anchor
    /// first: `None` unless both ends are in `run`.
    pub(crate) fn selection_from(
        &self,
        selection: &accesskit::TextSelection,
        run: accesskit::NodeId,
    ) -> Option<(usize, usize)> {
        let offset = |position: &accesskit::TextPosition| {
            (position.node == run).then(|| self.byte_offset(position.character_index))
        };
        Some((offset(&selection.anchor)?, offset(&selection.focus)?))
    }

    /// `element`, the line's node, with the run as its child and a
    /// `SetTextSelection` handler that gives `select` the byte offsets in
    /// `text` of the new selection's ends. `run` remembers the run's id.
    pub(crate) fn attach<E: StatefulInteractiveElement>(
        self,
        element: E,
        run: Rc<Cell<Option<accesskit::NodeId>>>,
        select: impl Fn(&accesskit::TextSelection, accesskit::NodeId, &mut App) + 'static,
    ) -> E {
        let built = run.clone();
        element
            .a11y_synthetic_children(move |builder| {
                let id = builder.synthetic_node_id("text-run");
                built.set(Some(id));
                let node = self.write(builder.parent_node(), id);
                builder.push_child(id, node);
            })
            .on_a11y_action(accesskit::Action::SetTextSelection, move |data, _, cx| {
                if let (Some(accesskit::ActionData::SetTextSelection(selection)), Some(id)) =
                    (data, run.get())
                {
                    select(selection, id, cx);
                }
            })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextInputEvent {
    /// The text changed.
    Changed,
    /// [`TextInput::submit`] was called.
    Submitted,
}

/// What a [`TextInput::set_typing_filter`] filter makes of a typed character.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Typed {
    /// It types as it came.
    Pass,
    /// It types nothing.
    Swallow,
    /// It types this text, in its place.
    Insert(String),
}

/// Asked of each character typed on its own, before it goes in: see
/// [`TextInput::set_typing_filter`]. It runs while the input is being
/// updated, so it can't read or edit the input: it answers with a
/// [`Typed`], and defers anything else.
type TypingFilter = Rc<dyn Fn(&str, &mut Window, &mut App) -> Typed>;

/// An edit a host makes to the text before the platform's goes in: see
/// [`TextInput::set_before_platform_input`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SettleEdit {
    /// Adds this text at the caret, replacing the selection.
    Insert(String),
    /// Deletes the selection, or the grapheme before the caret.
    Backspace,
}

/// What a host settles before the platform's text goes in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Settled {
    /// Made to the text, in order, before the platform's edit.
    pub edits: Vec<SettleEdit>,
    /// The platform's edit is dropped: the host takes it over.
    pub swallow: bool,
}

/// Asked, with the text and the platform's, before an edit from the
/// platform that isn't a single typed character goes in: see
/// [`TextInput::set_before_platform_input`].
type BeforeInput = Rc<dyn Fn(&str, bool, &str, &mut Window, &mut App) -> Settled>;

/// A single-line text input. Typed text goes in at the caret, which the
/// `move_*` methods move by kiln-core's [`Motion`] rules (graphemes and
/// words); it starts at the end. It has one selection, from an anchor to
/// the caret: all of the text ([`TextInput::select_all`]), or any range a
/// screen reader selects. The platform sees it as the selected range, the
/// next edit (typing, a paste, a platform replacement, a delete) replaces
/// it instead of adding to the text, and a move drops it, Left going to
/// its start and Right to its end.
///
/// An IME composition sits at the caret, underlined. [`TextInput::text`]
/// leaves it out and no [`TextInputEvent::Changed`] is emitted while it's
/// open, so a picker doesn't refilter on each romaji key; committing it
/// inserts its text and emits one `Changed`.
///
/// Its element sets the [`TextInput::KEY_CONTEXT`] key context. Its editing
/// keys are commands the host registers and binds in that context, which
/// call [`TextInput::backspace`], [`TextInput::delete_forward`],
/// [`TextInput::delete_word_left`], [`TextInput::clear`],
/// [`TextInput::paste`] and the `move_*` methods: this crate knows no
/// command registry.
///
/// While [`TextInput::set_typing`] has turned typing off, it takes no
/// text: it registers no input handler, so typed keys insert nothing, and
/// it draws no caret. A picker's vim normal mode does that.
///
/// A click focuses it. It has no border unless [`TextInput::bordered`]
/// gives it one. Its colours are the active theme's, read as it renders.
pub struct TextInput {
    text: String,
    /// Where typed text goes: a grapheme boundary in `text`.
    caret: usize,
    /// Resolved as it renders, so a keyed one follows the locale.
    placeholder: Text,
    /// Its accessible name; the placeholder stands in without one.
    name: Option<SharedString>,
    focus: FocusHandle,
    bordered: bool,
    /// Its height: see [`TextInput::height`].
    height: Pixels,
    /// Whether typed text goes in: see [`TextInput::set_typing`].
    typing: bool,
    /// The composition at the caret, and the selected bytes within it.
    marked: Option<(String, Range<usize>)>,
    /// The selection's other end, the caret being its head: the selection
    /// is between the two (in either order) while this is set, and empty
    /// if they meet. A grapheme boundary in `text`.
    anchor: Option<usize>,
    /// The font and size it was last drawn in, which it inherits from its
    /// parents: the platform asks for bounds outside drawing.
    /// Committed text was deleted under the composition: `Changed` is owed
    /// when it ends.
    change_owed: bool,
    drawn_font: Option<(gpui::Font, Pixels)>,
    /// See [`TextInput::set_typing_filter`].
    typing_filter: Option<TypingFilter>,
    /// The caret it last painted, and the bounds it was painted in.
    #[cfg(test)]
    pub(crate) painted_caret: Option<(Bounds<Pixels>, Bounds<Pixels>)>,
    /// See [`TextInput::set_before_platform_input`].
    before_input: Option<BeforeInput>,
    /// A screen reader sees a `SearchInput`: see [`TextInput::search`].
    search: bool,
    /// The id of its text run in the last accessibility tree.
    a11y_run: Rc<Cell<Option<accesskit::NodeId>>>,
}

impl EventEmitter<TextInputEvent> for TextInput {}

impl TextInput {
    pub const KEY_CONTEXT: &str = "text_input";

    /// Its horizontal padding (`px_3`), in rems.
    const PADDING_REMS: f32 = 0.75;

    /// Its colours come from `cx.theme()` as it renders. `_theme` is
    /// ignored: deprecated, kept so callers that pass a [`UiTheme`] build.
    pub fn new(placeholder: impl Into<Text>, _theme: &UiTheme, cx: &mut App) -> Self {
        Self {
            text: String::new(),
            caret: 0,
            placeholder: placeholder.into(),
            name: None,
            focus: cx.focus_handle(),
            bordered: false,
            height: px(30.),
            typing: true,
            marked: None,
            anchor: None,
            change_owed: false,
            drawn_font: None,
            typing_filter: None,
            #[cfg(test)]
            painted_caret: None,
            before_input: None,
            search: false,
            a11y_run: Rc::default(),
        }
    }

    /// Makes it a search field to a screen reader (a `SearchInput`), for a
    /// find or picker query; it's a `TextInput` otherwise.
    pub fn search(mut self) -> Self {
        self.search = true;
        self
    }

    /// Its role to a screen reader: see [`TextInput::search`].
    pub fn a11y_role(&self) -> Role {
        match self.search {
            true => Role::SearchInput,
            false => Role::TextInput,
        }
    }

    /// What a screen reader is given: the text as the platform sees it
    /// (an open composition in it, at the caret) and the selection: all of
    /// it, the composition's selection, or the caret. It's read-only while
    /// [`TextInput::set_typing`] has turned typing off.
    pub(crate) fn a11y_line(&self) -> A11yLine {
        let text = self.full_text();
        let (anchor, focus) = match &self.marked {
            Some((_, selected)) => (self.caret + selected.start, self.caret + selected.end),
            None => (self.anchor.unwrap_or(self.caret), self.caret),
        };
        A11yLine {
            role: self.a11y_role(),
            text,
            anchor,
            focus,
            read_only: !self.typing,
        }
    }

    /// Takes a screen reader's `SetTextSelection`, with `run` the id of the
    /// text run it names: it selects from the anchor to the focus, each on
    /// a grapheme boundary, the caret at the focus; a collapsed one is just
    /// the caret. A composition is dropped first, as a move drops it.
    pub(crate) fn select_from_a11y(
        &mut self,
        selection: &accesskit::TextSelection,
        run: accesskit::NodeId,
        cx: &mut Context<Self>,
    ) {
        let line = self.a11y_line();
        let Some((anchor, focus)) = line.selection_from(selection, run) else {
            return;
        };
        // Offsets in the composition are its start in the committed text,
        // and those after it move back by its length.
        let marked = self.marked.as_ref().map_or(0, |(text, _)| text.len());
        let caret = self.caret;
        let committed = |offset: usize| match offset {
            at if at <= caret => at,
            at if at >= caret + marked => at - marked,
            _ => caret,
        };
        let (anchor, focus) = (committed(anchor), committed(focus));
        self.drop_composition();
        let len = self.text.len();
        let (anchor, focus) = (
            self.boundary_before(anchor.min(len)),
            self.boundary_before(focus.min(len)),
        );
        self.caret = focus;
        self.anchor = (anchor != focus).then_some(anchor);
        self.finish_edit(false, cx);
    }

    /// Asks `hook` before an edit from the platform goes in that isn't a
    /// single typed character [`TextInput::set_typing_filter`] took: a
    /// commit of several characters, a replacement or deletion of a range,
    /// and the start of an IME composition. Not a paste or
    /// [`TextInput::insert`]. It is given the text and the platform's
    /// text (empty for a deletion or a composition's start), with whether
    /// the text is selected ([`TextInput::selected`]), and answers
    /// with the edits to make first, so a host holding typed characters
    /// back puts them in ahead of the platform's edit, in the order they
    /// came. An insertion at the end of the text stays at the end; a range
    /// keeps naming the text it named, through each edit.
    /// It runs while the input is being updated, so it can't read or edit
    /// the input.
    pub fn set_before_platform_input(
        &mut self,
        hook: Option<impl Fn(&str, bool, &str, &mut Window, &mut App) -> Settled + 'static>,
    ) {
        self.before_input = hook.map(|hook| Rc::new(hook) as BeforeInput);
    }

    /// Makes the edits [`TextInput::set_before_platform_input`] gives for
    /// `platform`, and moves `range` to match. `None`: the platform's edit
    /// is dropped.
    fn before_platform_input(
        &mut self,
        platform: &str,
        range: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Option<Range<usize>>> {
        let Some(hook) = self.before_input.clone() else {
            return Some(range);
        };
        let old = self.utf16_len();
        let settled = hook(&self.text.clone(), self.selected(), platform, window, cx);
        // The range in bytes, moved through each edit; an insertion at the
        // end stays at the end.
        let at_end = range
            .as_ref()
            .is_some_and(|r| r.is_empty() && r.start >= old);
        let mut named = range.map(|range| Self::byte_range(&self.text, &range));
        for edit in settled.edits {
            let (deleted, inserted) = match &edit {
                SettleEdit::Insert(text) => {
                    let line = text.lines().next().unwrap_or_default().len();
                    match self.selected() {
                        true => (0..self.text.len(), line),
                        false => (self.caret..self.caret, line),
                    }
                }
                SettleEdit::Backspace => match self.selected() {
                    true => (0..self.text.len(), 0),
                    false => (self.target(Motion::Left)..self.caret, 0),
                },
            };
            match edit {
                SettleEdit::Insert(text) => self.insert(&text, cx),
                SettleEdit::Backspace => self.backspace(cx),
            }
            if let Some(named) = &mut named {
                *named = Self::through_edit(named, &deleted, inserted);
            }
        }
        if settled.swallow {
            return None;
        }
        let new = self.utf16_len();
        let utf16 = |bytes: usize| {
            let bytes = (0..=bytes.min(self.text.len()))
                .rev()
                .find(|at| self.text.is_char_boundary(*at))
                .unwrap_or(0);
            self.text[..bytes].encode_utf16().count()
        };
        Some(named.map(|named| match at_end {
            true => new..new,
            false => utf16(named.start)..utf16(named.end),
        }))
    }

    /// Where `range` is after `deleted` was replaced by `inserted` bytes:
    /// it keeps the text it named, so the inserted text is outside it and
    /// text deleted from its ends is cut off. An empty range, a position,
    /// moves past text inserted at it.
    fn through_edit(range: &Range<usize>, deleted: &Range<usize>, inserted: usize) -> Range<usize> {
        let shifted = |at: usize| at - deleted.len() + inserted;
        let start = match range.start {
            at if at < deleted.start => at,
            at if at >= deleted.end => shifted(at),
            _ => deleted.start + inserted,
        };
        let end = match range.end {
            at if at <= deleted.start => at,
            at if at >= deleted.end => shifted(at),
            _ => deleted.start,
        };
        match range.is_empty() {
            true => start..start,
            false => start..end.max(start),
        }
    }

    /// Asks `filter` about each character typed on its own, before it goes
    /// in. Not a paste or [`TextInput::insert`], nor text an IME composes
    /// or commits, and not while a composition is open. `None`, the
    /// default, types them all as they come.
    pub fn set_typing_filter(
        &mut self,
        filter: Option<impl Fn(&str, &mut Window, &mut App) -> Typed + 'static>,
    ) {
        self.typing_filter = filter.map(|filter| Rc::new(filter) as TypingFilter);
    }

    /// Draws it as a field on its own: on the panel colour, in a rounded
    /// `border.control` border, with the 2 px accent focus ring (see
    /// [`crate::focus_ring`]) outside it while it has focus.
    pub fn bordered(mut self) -> Self {
        self.bordered = true;
        self
    }

    /// Draws it `height` tall, rather than 30 px: the design's 26 px
    /// control ([`crate::height::CONTROL`]) in a row of other controls.
    pub fn height(mut self, height: Pixels) -> Self {
        self.height = height;
        self
    }

    /// Its accessible name, which a screen reader reads: the placeholder
    /// unless this sets one.
    pub fn name(mut self, name: impl Into<SharedString>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// The name a screen reader reads now: the one [`TextInput::name`] set,
    /// else the placeholder in the active locale.
    pub fn accessible_name(&self) -> SharedString {
        (self.name.clone()).unwrap_or_else(|| self.placeholder.resolve())
    }

    /// The placeholder it shows while empty, in the active locale.
    pub fn placeholder(&self) -> SharedString {
        self.placeholder.resolve()
    }

    /// Its border colour, if it's [`TextInput::bordered`]: focus doesn't
    /// change it, the focus ring shows focus.
    pub(crate) fn border_color(&self, _focused: bool, c: &Tokens<Hsla>) -> Option<Hsla> {
        self.bordered.then_some(c.border.control)
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Replaces the text and drops any composition, without emitting
    /// [`TextInputEvent::Changed`]. The caret goes to the end.
    pub fn set_text(&mut self, text: impl Into<String>, cx: &mut Context<Self>) {
        self.marked = None;
        self.anchor = None;
        self.change_owed = false;
        self.text = text.into();
        self.caret = self.text.len();
        cx.notify();
    }

    /// The caret's byte offset in [`TextInput::text`].
    pub fn caret(&self) -> usize {
        self.caret
    }

    /// Selects all of the text, without emitting [`TextInputEvent::Changed`].
    /// Nothing is selected in empty text.
    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        self.marked = None;
        self.anchor = (!self.text.is_empty()).then_some(0);
        self.caret = self.text.len();
        cx.notify();
    }

    /// Whether all of the text is selected.
    pub fn selected(&self) -> bool {
        !self.text.is_empty() && self.selection() == (0..self.text.len())
    }

    /// The selected bytes of [`TextInput::text`], start first: empty at
    /// the caret when nothing is selected.
    pub fn selection(&self) -> Range<usize> {
        let anchor = self.anchor.unwrap_or(self.caret);
        anchor.min(self.caret)..anchor.max(self.caret)
    }

    /// Whether typed text goes in, as it does unless turned off here. Off,
    /// it registers no input handler and draws no caret; an open
    /// composition is dropped. Its editing methods still edit.
    pub fn set_typing(&mut self, typing: bool, cx: &mut Context<Self>) {
        if self.typing == typing {
            return;
        }
        self.typing = typing;
        if !typing {
            self.drop_composition();
            self.finish_edit(false, cx);
        }
        cx.notify();
    }

    pub fn typing(&self) -> bool {
        self.typing
    }

    /// Inserts `text` at the caret, up to its first line break, leaving the
    /// caret after it. This and the other editing methods cancel a
    /// composition that's open. Replaces the text instead if it's selected.
    pub fn insert(&mut self, text: &str, cx: &mut Context<Self>) {
        self.marked = None;
        let line = text.lines().next().unwrap_or_default();
        let replaced = self.take_selection();
        self.text.insert_str(self.caret, line);
        self.caret = self.boundary_after(self.caret + line.len());
        self.finish_edit(replaced || !line.is_empty(), cx);
    }

    /// Deletes the grapheme before the caret, or the selected text.
    pub fn backspace(&mut self, cx: &mut Context<Self>) {
        self.delete_to(Motion::Left, cx);
    }

    /// Deletes the grapheme after the caret, or the selected text.
    pub fn delete_forward(&mut self, cx: &mut Context<Self>) {
        self.delete_to(Motion::Right, cx);
    }

    /// Deletes the spaces before the caret and the word before them, or the
    /// run of punctuation (such as a `/`) if that comes first; or the
    /// selected text.
    pub fn delete_word_left(&mut self, cx: &mut Context<Self>) {
        self.delete_to(Motion::WordLeft, cx);
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.marked = None;
        self.anchor = None;
        let edited = !self.text.is_empty();
        self.text.clear();
        self.caret = 0;
        self.finish_edit(edited, cx);
    }

    /// Moves the caret a grapheme left, or to the start of the selection.
    pub fn move_left(&mut self, cx: &mut Context<Self>) {
        self.move_by(Motion::Left, cx);
    }

    /// Moves the caret a grapheme right, or to the end of the selection.
    pub fn move_right(&mut self, cx: &mut Context<Self>) {
        self.move_by(Motion::Right, cx);
    }

    /// Moves the caret to the start of the word before it.
    pub fn move_word_left(&mut self, cx: &mut Context<Self>) {
        self.move_by(Motion::WordLeft, cx);
    }

    /// Moves the caret to the end of the word after it.
    pub fn move_word_right(&mut self, cx: &mut Context<Self>) {
        self.move_by(Motion::WordRight, cx);
    }

    pub fn move_home(&mut self, cx: &mut Context<Self>) {
        self.move_by(Motion::Home, cx);
    }

    pub fn move_end(&mut self, cx: &mut Context<Self>) {
        self.move_by(Motion::End, cx);
    }

    /// Adds the clipboard's text, up to its first line break.
    pub fn paste(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.insert(&text, cx);
        }
    }

    /// Emits [`TextInputEvent::Submitted`].
    pub fn submit(&mut self, cx: &mut Context<Self>) {
        cx.emit(TextInputEvent::Submitted);
    }

    /// Deletes the selected text, if any, leaving the caret where it was;
    /// whether it did.
    fn take_selection(&mut self) -> bool {
        let selection = self.selection();
        self.anchor = None;
        if selection.is_empty() {
            return false;
        }
        self.text.replace_range(selection.clone(), "");
        self.caret = self.boundary_after(selection.start);
        true
    }

    /// Where `motion` takes the caret, by [`TextEdit`]'s rules: from the
    /// selection, if there is one, else from the caret. A line break
    /// [`TextInput::set_text`] put in (a host's, such as a pasted carriage
    /// return) counts as whitespace, and a CRLF as one grapheme.
    fn target(&self, motion: Motion) -> usize {
        // TextEdit makes a CRLF one space. A tab per byte keeps the offsets,
        // and keeps a combining mark after a break a grapheme of its own,
        // as a tab is a control character too; snapping to this text's
        // graphemes then makes a CRLF one step.
        let flat = self.text.replace(['\n', '\r'], "\t");
        let mut edit = TextEdit::with_text(&flat);
        match self.anchor {
            Some(anchor) => edit.set_selection(anchor, self.caret),
            None => edit.move_caret_to(self.caret),
        }
        edit.move_by(motion);
        let target = edit.caret();
        if target < self.caret {
            self.boundary_before(target)
        } else {
            self.boundary_after(target)
        }
    }

    /// Drops the composition, if any, putting the caret back on a grapheme
    /// boundary: deleting committed text under it can have joined what was
    /// either side of it.
    fn drop_composition(&mut self) {
        if self.marked.take().is_some() {
            self.caret = self.boundary_after(self.caret);
        }
    }

    /// The grapheme boundary at or before byte `offset`.
    fn boundary_before(&self, offset: usize) -> usize {
        (self.text.grapheme_indices(true))
            .map(|(ix, _)| ix)
            .chain([self.text.len()])
            .take_while(|ix| *ix <= offset)
            .last()
            .unwrap_or(0)
    }

    /// The grapheme boundary at or after byte `offset`: where the caret
    /// goes once an edit has made `offset` part of a grapheme, as a joiner
    /// or a combining mark typed or deleted next to it can.
    fn boundary_after(&self, offset: usize) -> usize {
        (self.text.grapheme_indices(true))
            .map(|(ix, _)| ix)
            .find(|ix| *ix >= offset)
            .unwrap_or(self.text.len())
    }

    /// Moves the caret by `motion`, dropping the selection and the
    /// composition.
    fn move_by(&mut self, motion: Motion, cx: &mut Context<Self>) {
        self.drop_composition();
        self.caret = self.target(motion);
        self.anchor = None;
        self.finish_edit(false, cx);
    }

    /// Deletes from the caret to where `motion` takes it, or the selected
    /// text.
    fn delete_to(&mut self, motion: Motion, cx: &mut Context<Self>) {
        self.drop_composition();
        if self.take_selection() {
            return self.finish_edit(true, cx);
        }
        let target = self.target(motion);
        let range = self.caret.min(target)..self.caret.max(target);
        let edited = !range.is_empty();
        self.text.replace_range(range.clone(), "");
        self.caret = self.boundary_after(range.start);
        self.finish_edit(edited, cx);
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        cx.emit(TextInputEvent::Changed);
        cx.notify();
    }

    /// The committed text with the composition at the caret, as the
    /// platform sees them.
    fn full_text(&self) -> String {
        let marked = self.marked.as_ref().map_or("", |(text, _)| text);
        let (before, after) = self.text.split_at(self.caret);
        format!("{before}{marked}{after}")
    }

    /// The byte offset in `text` of UTF-16 offset `utf16`, clamped.
    fn byte_offset(text: &str, utf16: usize) -> usize {
        let mut seen = 0;
        for (ix, ch) in text.char_indices() {
            if seen >= utf16 {
                return ix;
            }
            seen += ch.len_utf16();
        }
        text.len()
    }

    /// The byte range of UTF-16 range `range` in `text`, clamped to it.
    fn byte_range(text: &str, range: &Range<usize>) -> Range<usize> {
        Self::byte_offset(text, range.start)..Self::byte_offset(text, range.end.max(range.start))
    }

    fn utf16_len(&self) -> usize {
        self.text.encode_utf16().count()
    }

    fn caret_utf16(&self) -> usize {
        self.text[..self.caret].encode_utf16().count()
    }

    /// The selection's UTF-16 range in the text.
    fn selection_utf16(&self) -> Range<usize> {
        let utf16 = |bytes: usize| self.text[..bytes].encode_utf16().count();
        let selection = self.selection();
        utf16(selection.start)..utf16(selection.end)
    }

    /// The composition's UTF-16 range in the full text, at the caret.
    fn marked_utf16(&self) -> Option<Range<usize>> {
        let (text, _) = self.marked.as_ref()?;
        let start = self.caret_utf16();
        Some(start..start + text.encode_utf16().count())
    }

    /// The byte range in the committed text of UTF-16 range `range` of the
    /// full text, the composition left out: an end within it is its start.
    fn committed_range(&self, range: &Range<usize>) -> Range<usize> {
        let marked = self.marked_utf16().unwrap_or(0..0);
        let committed = |utf16: usize| match utf16 {
            at if at <= marked.start => at,
            at if at >= marked.end => at - marked.len(),
            _ => marked.start,
        };
        Self::byte_range(&self.text, &(committed(range.start)..committed(range.end)))
    }

    /// Ends an edit that dropped the composition: emits one `Changed` if
    /// `edited` or the composition's replacement owes one, else redraws.
    fn finish_edit(&mut self, edited: bool, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.change_owed) || edited {
            self.changed(cx);
        } else {
            cx.notify();
        }
    }

    /// Inserts the composition's text at the caret and emits `Changed`.
    fn commit_marked(&mut self, cx: &mut Context<Self>) {
        if let Some((text, _)) = self.marked.take() {
            self.text.insert_str(self.caret, &text);
            self.caret = self.boundary_after(self.caret + text.len());
            self.finish_edit(!text.is_empty(), cx);
        }
    }
}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// Text from the platform: typing replaces no range, so it goes at the
/// caret. A composition is kept apart from the text until it commits.
impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let full = self.full_text();
        let bytes = Self::byte_range(&full, &range);
        let text = full[bytes].to_string();
        let start = range.start.min(full.encode_utf16().count());
        actual_range.replace(start..start + text.encode_utf16().count());
        Some(text)
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let start = self.caret_utf16();
        let range = match &self.marked {
            Some((text, selected)) => {
                let utf16 = |bytes: usize| text[..bytes].encode_utf16().count();
                start + utf16(selected.start)..start + utf16(selected.end)
            }
            None => self.selection_utf16(),
        };
        Some(UTF16Selection {
            range,
            reversed: self.marked.is_none() && self.anchor.is_some_and(|a| a > self.caret),
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked_utf16()
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.commit_marked(cx);
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut commits = range.is_none() || range == self.marked_utf16();
        if commits
            && self.marked.is_none()
            && text.chars().count() == 1
            && let Some(filter) = self.typing_filter.clone()
        {
            match filter(text, window, cx) {
                Typed::Pass => {}
                Typed::Swallow => return,
                Typed::Insert(text) => return self.insert(&text, cx),
            }
        }
        let mut range = range;
        if self.marked.is_none() {
            match self.before_platform_input(text, range, window, cx) {
                Some(moved) => range = moved,
                None => return,
            }
            commits = range.is_none();
        }
        let selection = self.selection_utf16();
        if !selection.is_empty() && (range.is_none() || range == Some(selection)) {
            // The platform replaces the selection, as typing does.
            self.insert(text, cx);
            return;
        }
        if commits {
            // insert cancels the composition; an empty commit just ends it.
            self.insert(text, cx);
            return;
        }
        let bytes = range.map(|range| self.committed_range(&range));
        self.marked = None;
        self.anchor = None;
        match bytes {
            Some(bytes) if bytes != (self.caret..self.caret) => {
                let line = text.lines().next().unwrap_or_default();
                self.text.replace_range(bytes.clone(), line);
                self.caret = self.boundary_after(bytes.start + line.len());
                self.finish_edit(true, cx);
            }
            _ => self.insert(text, cx),
        }
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let line = text.lines().next().unwrap_or_default();
        let mut range = range;
        if self.marked.is_none() && !line.is_empty() {
            match self.before_platform_input("", range, window, cx) {
                Some(moved) => range = moved,
                None => return,
            }
        }
        if line.is_empty() {
            // An empty mark ends the composition, whatever its range, and
            // edits nothing (it replaces a selection, though).
            self.drop_composition();
            let replaced = self.take_selection();
            self.finish_edit(replaced, cx);
            return;
        }
        // A composition replaces the selection too.
        let replaced = self.take_selection();
        self.change_owed |= replaced;
        let caret = self.caret_utf16();
        let mut at = self.marked_utf16().unwrap_or(caret..caret);
        if let Some(range) = &range
            && (range.start > at.end || range.end < at.start)
            && let Some((old, _)) = self.marked.take()
        {
            // A composition wholly before or after the open one: that one
            // stays where it is, as text.
            self.text.insert_str(self.caret, &old);
            self.caret += old.len();
            self.change_owed = true;
            at = at.end..at.end;
        }
        let (mut prefix, mut suffix) = (String::new(), String::new());
        if let Some(range) = range {
            // The composition sits at the caret, so a range reaching past
            // it into the committed text deletes that part of it: after
            // the caret first, as that leaves the caret where it is. A
            // range starting past the caret moves the caret to its start.
            if range.end > at.end {
                let bytes = self.committed_range(&(range.start.max(at.end)..range.end));
                self.change_owed |= !bytes.is_empty();
                self.text.replace_range(bytes.clone(), "");
                if range.start > at.end {
                    self.caret = bytes.start;
                }
            }
            if range.start < at.start {
                let bytes = self.committed_range(&(range.start..range.end.min(at.start)));
                self.change_owed |= !bytes.is_empty();
                self.text.replace_range(bytes.clone(), "");
                self.caret = bytes.start;
            }
            if let Some((old, _)) = &self.marked {
                let rel = range.start.saturating_sub(at.start)..range.end.saturating_sub(at.start);
                let bytes = Self::byte_range(old, &rel);
                prefix = old[..bytes.start].to_string();
                suffix = old[bytes.end..].to_string();
            }
        }
        let start = prefix.len();
        let end = start + line.len();
        let selected = selected
            .map(|range| Self::byte_range(line, &range))
            .map_or(end..end, |range| start + range.start..start + range.end);
        let whole = format!("{prefix}{line}{suffix}");
        self.marked = Some((whole, selected));
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        element_bounds: Bounds<Pixels>,
        window: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let full = self.full_text();
        if full.is_empty() {
            return Some(Bounds::new(
                element_bounds.origin,
                size(px(0.), element_bounds.size.height),
            ));
        }
        let style = window.text_style();
        let rem = window.rem_size();
        let (font, font_size) = self
            .drawn_font
            .clone()
            .unwrap_or_else(|| (style.font(), style.font_size.to_pixels(rem)));
        let run = TextRun {
            len: full.len(),
            font,
            color: style.color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let line = window.text_system().shape_line(
            SharedString::from(full.clone()),
            font_size,
            &[run],
            None,
        );
        let bytes = Self::byte_range(&full, &range);
        // The text starts after the element's `px_3` padding.
        let left = element_bounds.left() + rem * Self::PADDING_REMS;
        Some(Bounds::from_corners(
            gpui::point(left + line.x_for_index(bytes.start), element_bounds.top()),
            gpui::point(left + line.x_for_index(bytes.end), element_bounds.bottom()),
        ))
    }

    fn character_index_for_point(
        &mut self,
        _: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}

impl Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = &cx.theme().colors;
        let (text, muted, panel) = (
            colors.tab.active_text,
            colors.text.muted,
            colors.surface.panel,
        );
        let focused = self.focus.is_focused(window);
        let border = self.border_color(focused, colors);
        let focus = self.focus.clone();
        let typing = self.typing;
        let show_caret = focused && typing;
        let input = cx.entity();
        // The text is shaped as one line, the composition underlined in
        // it, and the caret painted over it where that shaping puts its
        // offset (after the composition's selection, while one is open).
        let full = self.full_text();
        let caret = self.caret + self.marked.as_ref().map_or(0, |(_, selected)| selected.end);
        let mut highlights = Vec::new();
        let selected = self.selection();
        if self.marked.is_none() && !selected.is_empty() {
            let selection = HighlightStyle {
                background_color: Some(text.opacity(0.3)),
                ..HighlightStyle::default()
            };
            highlights.push((selected, selection));
        }
        if let Some((marked, _)) = &self.marked {
            let underline = HighlightStyle {
                underline: Some(UnderlineStyle {
                    thickness: px(1.),
                    color: Some(text),
                    wavy: false,
                }),
                ..HighlightStyle::default()
            };
            highlights.push((self.caret..self.caret + marked.len(), underline));
        }
        let styled = (!full.is_empty()).then(|| StyledText::new(full).with_highlights(highlights));
        let layout = styled.as_ref().map(|styled| styled.layout().clone());
        let name = self.accessible_name();
        let line = window.is_a11y_active().then(|| self.a11y_line());
        let weak = cx.entity().downgrade();
        let run = self.a11y_run.clone();
        let ring = (focused && self.bordered).then(|| {
            focus_ring(radius::CONTROL, px(1.), colors)
                .debug_selector(|| "text-input-focus-ring".into())
        });
        let node = crate::node(div().id("text-input"), window, cx, self.a11y_role(), name);
        let node = match line {
            Some(line) => line.attach(node, run, move |selection, run, cx| {
                select_in(&weak, selection, run, cx)
            }),
            None => node,
        };
        node.key_context(Self::KEY_CONTEXT)
            .track_focus(&self.focus)
            .on_mouse_down(MouseButton::Left, {
                let focus = self.focus.clone();
                move |_, window, cx| window.focus(&focus, cx)
            })
            .when_some(border, |row, border| {
                row.border_1()
                    .border_color(border)
                    .rounded(radius::CONTROL)
                    .bg(panel)
            })
            .relative()
            .flex()
            .items_center()
            .h(self.height)
            .px_3()
            .text_color(text)
            .when_some(styled, |row, styled| {
                row.child(
                    div()
                        .debug_selector(|| "text-input-text".into())
                        .child(styled),
                )
            })
            .when(self.text.is_empty() && self.marked.is_none(), |row| {
                row.child(div().text_color(muted).child(self.placeholder.resolve()))
            })
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, cx| {
                        let style = window.text_style();
                        let rem = window.rem_size();
                        let drawn = (style.font(), style.font_size.to_pixels(rem));
                        let painted = show_caret.then(|| {
                            // Without text, where text would start.
                            let x = (layout.as_ref())
                                .and_then(|layout| layout.position_for_index(caret))
                                .map_or(bounds.left() + rem * Self::PADDING_REMS, |at| at.x);
                            let top = bounds.center().y - px(7.);
                            let quad = Bounds::new(gpui::point(x, top), size(px(1.), px(14.)));
                            window.paint_quad(gpui::fill(quad, text));
                            quad
                        });
                        input.update(cx, |input, _| {
                            input.drawn_font = Some(drawn);
                            #[cfg(test)]
                            {
                                input.painted_caret = painted.map(|quad| (quad, bounds));
                            }
                        });
                        #[cfg(not(test))]
                        let _ = painted;
                        if !typing {
                            return;
                        }
                        window.handle_input(&focus, ElementInputHandler::new(bounds, input), cx)
                    },
                )
                .absolute()
                .inset_0(),
            )
            .children(ring)
    }
}

/// Gives `input` a screen reader's `selection` of text run `run`.
fn select_in(
    input: &WeakEntity<TextInput>,
    selection: &accesskit::TextSelection,
    run: accesskit::NodeId,
    cx: &mut App,
) {
    if let Some(input) = input.upgrade() {
        input.update(cx, |input, cx| input.select_from_a11y(selection, run, cx));
    }
}

/// What a host settles before a platform edit goes in: the edit's range
/// keeps naming the text it named, and the hook sees the selection.
#[cfg(test)]
mod settle_tests {
    use std::{cell::Cell, rc::Rc};

    use gpui::{
        AppContext as _, Context, Entity, EntityInputHandler as _, IntoElement, ParentElement as _,
        Render, Styled as _, TestAppContext, Window, div,
    };

    use super::{SettleEdit, Settled, TextInput};
    use crate::UiTheme;

    struct Host(Entity<TextInput>);

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().text_size(gpui::px(30.)).child(self.0.clone())
        }
    }

    fn input_with<'a>(
        cx: &'a mut TestAppContext,
        text: &str,
    ) -> (Entity<TextInput>, &'a mut gpui::VisualTestContext) {
        let (host, cx) = cx.add_window_view(|_, cx| {
            Host(cx.new(|cx| {
                TextInput::new(kiln_l10n::Text::verbatim("Find"), &UiTheme::default(), cx)
            }))
        });
        cx.run_until_parked();
        let input = host.read_with(cx, |host, _| host.0.clone());
        input.update(cx, |input, cx| input.set_text(text, cx));
        (input, cx)
    }

    /// An insertion settled before a range's target, at a caret in the middle,
    /// leaves the range on the target, not on the inserted text.
    #[gpui::test]
    fn a_settled_insertion_before_the_target_moves_the_range(cx: &mut TestAppContext) {
        let (input, cx) = input_with(cx, "ab");
        input.update_in(cx, |input, window, cx| {
            input.move_left(cx);
            input.set_before_platform_input(Some(|_: &str, _, _: &str, _: &mut _, _: &mut _| {
                Settled {
                    edits: vec![SettleEdit::Insert("j".into())],
                    swallow: false,
                }
            }));
            input.replace_text_in_range(Some(1..2), "X", window, cx);
            assert_eq!(input.text(), "ajX");
        });
    }

    /// Text deleted and put back by settling is not what a range names: the
    /// range keeps what survived of its target.
    #[gpui::test]
    fn a_settled_deletion_and_reinsertion_leaves_the_range_on_the_survivors(
        cx: &mut TestAppContext,
    ) {
        let (input, cx) = input_with(cx, "abc");
        input.update_in(cx, |input, window, cx| {
            input.set_before_platform_input(Some(|_: &str, _, _: &str, _: &mut _, _: &mut _| {
                Settled {
                    edits: vec![SettleEdit::Backspace, SettleEdit::Insert("zz".into())],
                    swallow: false,
                }
            }));
            // "c" is taken out and "zz" put in its place: the range over "b"
            // and "c" is left naming "b" alone.
            input.replace_text_in_range(Some(1..3), "X", window, cx);
            assert_eq!(input.text(), "aXzz");
        });
    }

    /// The hook is told the text is selected, and a settled `Backspace` takes
    /// the selection out.
    #[gpui::test]
    fn the_hook_sees_the_selection(cx: &mut TestAppContext) {
        let (input, cx) = input_with(cx, "ab");
        let seen = Rc::new(Cell::new(false));
        let log = seen.clone();
        input.update_in(cx, |input, window, cx| {
            input.select_all(cx);
            input.set_before_platform_input(Some(
                move |_: &str, selected: bool, _: &str, _: &mut _, _: &mut _| {
                    log.set(selected);
                    Settled {
                        edits: vec![SettleEdit::Backspace],
                        swallow: false,
                    }
                },
            ));
            input.replace_text_in_range(None, "yz", window, cx);
            assert_eq!(input.text(), "yz");
        });
        assert!(seen.get());
    }
}
