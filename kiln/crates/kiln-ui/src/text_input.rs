// SPDX-License-Identifier: LicenseRef-Proprietary

use std::ops::Range;

use gpui::{
    App, Bounds, Context, ElementInputHandler, EntityInputHandler, EventEmitter, FocusHandle,
    Focusable, InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Point, Render,
    SharedString, Styled as _, UTF16Selection, Window, canvas, div, prelude::FluentBuilder as _,
    px,
};

use crate::UiTheme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextInputEvent {
    /// The text changed.
    Changed,
    /// [`TextInput::submit`] was called.
    Submitted,
}

/// A single-line text input. Typed text goes at the end: there's no caret
/// movement, selection or IME composition (M6).
///
/// Its element sets the [`TextInput::KEY_CONTEXT`] key context. Its editing
/// keys are commands the host registers and binds in that context, which
/// call [`TextInput::backspace`], [`TextInput::delete_word_left`],
/// [`TextInput::clear`] and [`TextInput::paste`]: this crate knows no
/// command registry.
pub struct TextInput {
    text: String,
    placeholder: SharedString,
    focus: FocusHandle,
    theme: UiTheme,
}

impl EventEmitter<TextInputEvent> for TextInput {}

impl TextInput {
    pub const KEY_CONTEXT: &str = "text_input";

    pub fn new(placeholder: impl Into<SharedString>, theme: &UiTheme, cx: &mut App) -> Self {
        Self {
            text: String::new(),
            placeholder: placeholder.into(),
            focus: cx.focus_handle(),
            theme: *theme,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Replaces the text, without emitting [`TextInputEvent::Changed`].
    pub fn set_text(&mut self, text: impl Into<String>, cx: &mut Context<Self>) {
        self.text = text.into();
        cx.notify();
    }

    /// Adds `text` at the end, up to its first line break.
    pub fn insert(&mut self, text: &str, cx: &mut Context<Self>) {
        let line = text.lines().next().unwrap_or_default();
        if !line.is_empty() {
            self.text.push_str(line);
            self.changed(cx);
        }
    }

    /// Deletes the last char.
    pub fn backspace(&mut self, cx: &mut Context<Self>) {
        if self.text.pop().is_some() {
            self.changed(cx);
        }
    }

    /// Deletes the trailing spaces and the word before them, or the run of
    /// punctuation (such as a `/`) if that comes first.
    pub fn delete_word_left(&mut self, cx: &mut Context<Self>) {
        let word = |ch: char| ch.is_alphanumeric() || ch == '_';
        let trimmed = self.text.trim_end();
        let punctuation = trimmed.ends_with(|ch: char| !word(ch));
        let kept =
            trimmed.trim_end_matches(|ch: char| word(ch) != punctuation && !ch.is_whitespace());
        if kept.len() < self.text.len() {
            self.text.truncate(kept.len());
            self.changed(cx);
        }
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        if !self.text.is_empty() {
            self.text.clear();
            self.changed(cx);
        }
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

    fn changed(&mut self, cx: &mut Context<Self>) {
        cx.emit(TextInputEvent::Changed);
        cx.notify();
    }

    /// The byte range of UTF-16 range `range`, clamped to the text.
    fn byte_range(&self, range: &Range<usize>) -> Range<usize> {
        let offset = |utf16: usize| {
            let mut seen = 0;
            for (ix, ch) in self.text.char_indices() {
                if seen >= utf16 {
                    return ix;
                }
                seen += ch.len_utf16();
            }
            self.text.len()
        };
        offset(range.start)..offset(range.end.max(range.start))
    }

    fn utf16_len(&self) -> usize {
        self.text.encode_utf16().count()
    }
}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// Text from the platform: typing replaces no range, so it goes at the
/// end. Composition (`replace_and_mark_text_in_range`) isn't supported.
impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let bytes = self.byte_range(&range);
        let text = self.text[bytes].to_string();
        let start = range.start.min(self.utf16_len());
        actual_range.replace(start..start + text.encode_utf16().count());
        Some(text)
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let end = self.utf16_len();
        Some(UTF16Selection {
            range: end..end,
            reversed: false,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        None
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {}

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match range.map(|range| self.byte_range(&range)) {
            Some(bytes) if bytes != (self.text.len()..self.text.len()) => {
                let line = text.lines().next().unwrap_or_default();
                self.text.replace_range(bytes, line);
                self.changed(cx);
            }
            _ => self.insert(text, cx),
        }
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        _: &str,
        _: Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) {
    }

    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        Some(element_bounds)
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
        let theme = self.theme;
        let focus = self.focus.clone();
        let input = cx.entity();
        div()
            .key_context(Self::KEY_CONTEXT)
            .track_focus(&self.focus)
            .relative()
            .flex()
            .items_center()
            .h(px(30.))
            .px_3()
            .text_color(theme.tab_active_text)
            .child(self.text.clone())
            .when(self.focus.is_focused(window), |row| {
                row.child(div().w(px(1.)).h(px(14.)).bg(theme.tab_active_text))
            })
            .when(self.text.is_empty(), |row| {
                row.child(
                    div()
                        .text_color(theme.muted_text)
                        .child(self.placeholder.clone()),
                )
            })
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, cx| {
                        window.handle_input(&focus, ElementInputHandler::new(bounds, input), cx)
                    },
                )
                .absolute()
                .size_full(),
            )
    }
}
