// SPDX-License-Identifier: LicenseRef-Proprietary

use std::{ops::Range, rc::Rc};

use gpui::{
    App, Bounds, Context, ElementInputHandler, EntityInputHandler, EventEmitter, FocusHandle,
    Focusable, Hsla, InteractiveElement as _, IntoElement, MouseButton, ParentElement as _, Pixels,
    Point, Render, SharedString, Styled as _, TextRun, UTF16Selection, Window, canvas, div,
    prelude::FluentBuilder as _, px, size,
};

use crate::{ActiveTheme as _, Tokens, UiTheme};

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

/// A single-line text input. Typed text goes at the end: there's no caret
/// movement. The one selection it has is all of the text
/// ([`TextInput::select_all`]): the platform sees it as the selected range,
/// and the next edit (typing, a paste, a platform replacement, a delete)
/// replaces it instead of adding to the text.
///
/// An IME composition sits after the text, underlined. [`TextInput::text`]
/// leaves it out and no [`TextInputEvent::Changed`] is emitted while it's
/// open, so a picker doesn't refilter on each romaji key; committing it
/// appends its text and emits one `Changed`.
///
/// Its element sets the [`TextInput::KEY_CONTEXT`] key context. Its editing
/// keys are commands the host registers and binds in that context, which
/// call [`TextInput::backspace`], [`TextInput::delete_word_left`],
/// [`TextInput::clear`] and [`TextInput::paste`]: this crate knows no
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
    placeholder: SharedString,
    focus: FocusHandle,
    bordered: bool,
    /// Whether typed text goes in: see [`TextInput::set_typing`].
    typing: bool,
    /// The composition after the text, and the selected bytes within it.
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
            placeholder: placeholder.into(),
            focus: cx.focus_handle(),
            bordered: false,
            typing: true,
            marked: None,
            selected: false,
            change_owed: false,
            drawn_font: None,
            typing_filter: None,
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
    /// border that takes the accent (`border.focused`) while it has focus.
    pub fn bordered(mut self) -> Self {
        self.bordered = true;
        self
    }

    /// Its border colour, if it's [`TextInput::bordered`], as focused or
    /// not.
    pub(crate) fn border_color(&self, focused: bool, c: &Tokens<Hsla>) -> Option<Hsla> {
        self.bordered.then_some(if focused {
            c.border.focused
        } else {
            c.border.default
        })
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Replaces the text and drops any composition, without emitting
    /// [`TextInputEvent::Changed`].
    pub fn set_text(&mut self, text: impl Into<String>, cx: &mut Context<Self>) {
        self.marked = None;
        self.selected = false;
        self.change_owed = false;
        self.text = text.into();
        cx.notify();
    }

    /// Selects all of the text, without emitting [`TextInputEvent::Changed`].
    /// Nothing is selected in empty text.
    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        self.marked = None;
        self.selected = !self.text.is_empty();
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
            self.marked = None;
            self.finish_edit(false, cx);
        }
        cx.notify();
    }

    pub fn typing(&self) -> bool {
        self.typing
    }

    /// Adds `text` at the end, up to its first line break. This and the other
    /// editing methods cancel a composition that's open.
    /// Replaces the text instead if it's selected.
    pub fn insert(&mut self, text: &str, cx: &mut Context<Self>) {
        self.marked = None;
        let line = text.lines().next().unwrap_or_default();
        let replaced = self.take_selection();
        self.text.push_str(line);
        self.finish_edit(replaced || !line.is_empty(), cx);
    }

    /// Deletes the last char, or the selected text.
    pub fn backspace(&mut self, cx: &mut Context<Self>) {
        self.marked = None;
        if self.take_selection() {
            return self.finish_edit(true, cx);
        }
        let edited = self.text.pop().is_some();
        self.finish_edit(edited, cx);
    }

    /// Deletes the trailing spaces and the word before them, or the run of
    /// punctuation (such as a `/`) if that comes first.
    pub fn delete_word_left(&mut self, cx: &mut Context<Self>) {
        self.marked = None;
        if self.take_selection() {
            return self.finish_edit(true, cx);
        }
        let word = |ch: char| ch.is_alphanumeric() || ch == '_';
        let trimmed = self.text.trim_end();
        let punctuation = trimmed.ends_with(|ch: char| !word(ch));
        let kept =
            trimmed.trim_end_matches(|ch: char| word(ch) != punctuation && !ch.is_whitespace());
        let edited = kept.len() < self.text.len();
        if edited {
            self.text.truncate(kept.len());
        }
        self.finish_edit(edited, cx);
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.marked = None;
        self.selected = false;
        let edited = !self.text.is_empty();
        self.text.clear();
        self.finish_edit(edited, cx);
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
        }
        selected
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        cx.emit(TextInputEvent::Changed);
        cx.notify();
    }

    /// The committed text and the composition, as the platform sees them.
    fn full_text(&self) -> String {
        let marked = self.marked.as_ref().map_or("", |(text, _)| text);
        format!("{}{marked}", self.text)
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

    /// The composition's UTF-16 range, after the text.
    fn marked_utf16(&self) -> Option<Range<usize>> {
        let (text, _) = self.marked.as_ref()?;
        let start = self.utf16_len();
        Some(start..start + text.encode_utf16().count())
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

    /// Appends the composition's text and emits `Changed`.
    fn commit_marked(&mut self, cx: &mut Context<Self>) {
        if let Some((text, _)) = self.marked.take() {
            self.text.push_str(&text);
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
/// end. A composition is kept apart from the text until it commits.
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
        let start = self.utf16_len();
        let range = match &self.marked {
            None if self.selected => 0..start,
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
        let commits = range.is_none() || range == self.marked_utf16();
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
        self.marked = None;
        self.selected = false;
        match range.map(|range| Self::byte_range(&self.text, &range)) {
            Some(bytes) if bytes != (self.text.len()..self.text.len()) => {
                let line = text.lines().next().unwrap_or_default();
                self.text.replace_range(bytes, line);
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
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let line = text.lines().next().unwrap_or_default();
        if line.is_empty() {
            // An empty mark ends the composition, whatever its range, and
            // edits nothing (it replaces a selection, though).
            self.marked = None;
            let replaced = self.take_selection();
            self.finish_edit(replaced, cx);
            return;
        }
        // A composition replaces the selection too.
        let replaced = self.take_selection();
        self.change_owed |= replaced;
        let committed = self.utf16_len();
        let (mut prefix, mut suffix) = (String::new(), String::new());
        if let Some(range) = range {
            // The composition is always last, so a range reaching into the
            // committed text deletes that part of it.
            if range.start < committed {
                let bytes = Self::byte_range(&self.text, &range);
                self.change_owed |= !bytes.is_empty();
                self.text.replace_range(bytes, "");
            }
            if let Some((old, _)) = &self.marked {
                let rel =
                    range.start.saturating_sub(committed)..range.end.saturating_sub(committed);
                let bytes = Self::byte_range(old, &rel);
                if range.start >= committed {
                    prefix = old[..bytes.start].to_string();
                    suffix = old[bytes.end..].to_string();
                } else {
                    suffix = old[bytes.end..].to_string();
                }
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
        let selected = self.selected;
        let highlight = text.opacity(0.3);
        let input = cx.entity();
        div()
            .key_context(Self::KEY_CONTEXT)
            .track_focus(&self.focus)
            .on_mouse_down(MouseButton::Left, {
                let focus = self.focus.clone();
                move |_, window, cx| window.focus(&focus, cx)
            })
            .when_some(border, |row, border| {
                row.border_1().border_color(border).rounded_md().bg(panel)
            })
            .relative()
            .flex()
            .items_center()
            .h(px(30.))
            .px_3()
            .text_color(text)
            .child(
                div()
                    .when(selected, |text| text.bg(highlight))
                    .child(self.text.clone()),
            )
            .when_some(self.marked.as_ref(), |row, (marked, _)| {
                row.child(div().underline().child(marked.clone()))
            })
            .when(focused && typing, |row| {
                row.child(div().w(px(1.)).h(px(14.)).bg(text))
            })
            .when(self.text.is_empty() && self.marked.is_none(), |row| {
                row.child(div().text_color(muted).child(self.placeholder.clone()))
            })
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, cx| {
                        let style = window.text_style();
                        let drawn = (style.font(), style.font_size.to_pixels(window.rem_size()));
                        input.update(cx, |input, _| input.drawn_font = Some(drawn));
                        if !typing {
                            return;
                        }
                        window.handle_input(&focus, ElementInputHandler::new(bounds, input), cx)
                    },
                )
                .absolute()
                .size_full(),
            )
    }
}
