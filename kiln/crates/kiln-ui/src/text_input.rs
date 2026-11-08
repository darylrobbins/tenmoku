// SPDX-License-Identifier: Apache-2.0

use std::{ops::Range, rc::Rc};

use gpui::{
    App, Bounds, Context, ElementInputHandler, EntityInputHandler, EventEmitter, FocusHandle,
    Focusable, HighlightStyle, Hsla, InteractiveElement as _, IntoElement, MouseButton,
    ParentElement as _, Pixels, Point, Render, Role, SharedString, StatefulInteractiveElement as _,
    Styled as _, StyledText, TextRun, UTF16Selection, UnderlineStyle, Window, canvas, div,
    prelude::FluentBuilder as _, px, size,
};

use kiln_core::{Motion, TextEdit};
use unicode_segmentation::UnicodeSegmentation as _;

use crate::{ActiveTheme as _, Tokens, UiTheme, focus_ring, radius};

#[cfg(test)]
#[path = "tests/text_input_caret.rs"]
mod caret_tests;

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
    /// Adds this text at the end.
    Insert(String),
    /// Deletes the last char.
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
type BeforeInput = Rc<dyn Fn(&str, &str, &mut Window, &mut App) -> Settled>;

/// A single-line text input. Typed text goes in at the caret, which the
/// `move_*` methods move by kiln-core's [`Motion`] rules (graphemes and
/// words); it starts at the end. The one selection it has is all of the
/// text ([`TextInput::select_all`]): the platform sees it as the selected
/// range, the next edit (typing, a paste, a platform replacement, a
/// delete) replaces it instead of adding to the text, and a move drops it,
/// Left going to the start and Right to the end.
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
    placeholder: SharedString,
    /// Its accessible name; the placeholder stands in without one.
    name: Option<SharedString>,
    focus: FocusHandle,
    bordered: bool,
    /// Whether typed text goes in: see [`TextInput::set_typing`].
    typing: bool,
    /// The composition at the caret, and the selected bytes within it.
    marked: Option<(String, Range<usize>)>,
    /// All of the (non-empty) text is selected.
    selected: bool,
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
}

impl EventEmitter<TextInputEvent> for TextInput {}

impl TextInput {
    pub const KEY_CONTEXT: &str = "text_input";

    /// Its horizontal padding (`px_3`), in rems.
    const PADDING_REMS: f32 = 0.75;

    /// Its colours come from `cx.theme()` as it renders. `_theme` is
    /// ignored: deprecated, kept so callers that pass a [`UiTheme`] build.
    pub fn new(placeholder: impl Into<SharedString>, _theme: &UiTheme, cx: &mut App) -> Self {
        Self {
            text: String::new(),
            caret: 0,
            placeholder: placeholder.into(),
            name: None,
            focus: cx.focus_handle(),
            bordered: false,
            typing: true,
            marked: None,
            selected: false,
            change_owed: false,
            drawn_font: None,
            typing_filter: None,
            #[cfg(test)]
            painted_caret: None,
            before_input: None,
        }
    }

    /// Asks `hook` before an edit from the platform goes in that isn't a
    /// single typed character [`TextInput::set_typing_filter`] took: a
    /// commit of several characters, a replacement or deletion of a range,
    /// and the start of an IME composition. Not a paste or
    /// [`TextInput::insert`]. It is given the text and the platform's
    /// text (empty for a deletion or a composition's start), and answers
    /// with the edits to make first, so a host holding typed characters
    /// back puts them in ahead of the platform's edit, in the order they
    /// came. An insertion at the end of the text stays at the end.
    /// It runs while the input is being updated, so it can't read or edit
    /// the input.
    pub fn set_before_platform_input(
        &mut self,
        hook: Option<impl Fn(&str, &str, &mut Window, &mut App) -> Settled + 'static>,
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
        let settled = hook(&self.text.clone(), platform, window, cx);
        for edit in settled.edits {
            match edit {
                SettleEdit::Insert(text) => self.insert(&text, cx),
                SettleEdit::Backspace => self.backspace(cx),
            }
        }
        if settled.swallow {
            return None;
        }
        let new = self.utf16_len();
        // An insertion at the end stays at the end; any other range keeps
        // the text it named, which settling only added to after.
        Some(
            range.map(|range| match range.is_empty() && range.start >= old {
                true => new..new,
                false => range.start.min(new)..range.end.min(new),
            }),
        )
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

    /// Its accessible name, which a screen reader reads: the placeholder
    /// unless this sets one.
    pub fn name(mut self, name: impl Into<SharedString>) -> Self {
        self.name = Some(name.into());
        self
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
        self.selected = false;
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
        self.selected = !self.text.is_empty();
        self.caret = self.text.len();
        cx.notify();
    }

    /// Whether all of the text is selected.
    pub fn selected(&self) -> bool {
        self.selected
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
        self.selected = false;
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

    /// Deletes the selected text, if any; whether it did.
    fn take_selection(&mut self) -> bool {
        let selected = std::mem::take(&mut self.selected);
        if selected {
            self.text.clear();
            self.caret = 0;
        }
        selected
    }

    /// Where `motion` takes the caret, by [`TextEdit`]'s rules: from the
    /// selection, if all of the text is, else from the caret. A line break
    /// [`TextInput::set_text`] put in (a host's, such as a pasted carriage
    /// return) counts as whitespace, and a CRLF as one grapheme.
    fn target(&self, motion: Motion) -> usize {
        // TextEdit makes a CRLF one space. A tab per byte keeps the offsets,
        // and keeps a combining mark after a break a grapheme of its own,
        // as a tab is a control character too; snapping to this text's
        // graphemes then makes a CRLF one step.
        let flat = self.text.replace(['\n', '\r'], "\t");
        let mut edit = TextEdit::with_text(&flat);
        if self.selected {
            edit.select_all();
        } else {
            edit.move_caret_to(self.caret);
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
        self.selected = false;
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
            None if self.selected => 0..self.utf16_len(),
            Some((text, selected)) => {
                let utf16 = |bytes: usize| text[..bytes].encode_utf16().count();
                start + utf16(selected.start)..start + utf16(selected.end)
            }
            None => start..start,
        };
        Some(UTF16Selection {
            range,
            reversed: false,
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
        if self.selected && (range.is_none() || range == Some(0..self.utf16_len())) {
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
        self.selected = false;
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
        if self.selected {
            let selection = HighlightStyle {
                background_color: Some(text.opacity(0.3)),
                ..HighlightStyle::default()
            };
            highlights.push((0..full.len(), selection));
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
        let name = self
            .name
            .clone()
            .unwrap_or_else(|| self.placeholder.clone());
        let ring = (focused && self.bordered).then(|| {
            focus_ring(radius::CONTROL, px(1.), colors)
                .debug_selector(|| "text-input-focus-ring".into())
        });
        div()
            .id("text-input")
            .role(Role::TextInput)
            .aria_label(name)
            .key_context(Self::KEY_CONTEXT)
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
            .h(px(30.))
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
                row.child(div().text_color(muted).child(self.placeholder.clone()))
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
