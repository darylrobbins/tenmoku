// SPDX-License-Identifier: LicenseRef-Proprietary
// Adapted from gpui-pre 0.3.7 examples/input.rs (zed@1a28cff); Apache-2.0

//! [`TextField`]: a single-line text field with a caret, a selection, IME
//! composition, undo and the clipboard. It's a GPUI view: create it with
//! `cx.new(|cx| TextField::new(window, cx).placeholder("Search"))`, render
//! the entity as a child, and follow it with `cx.subscribe_in(&field, ..)`
//! for its [`TextFieldEvent`]s. Its editing state is a
//! [`kiln_core::TextEdit`], so the editing rules are tested headless.
//!
//! Text arrives through GPUI's input handler ([`EntityInputHandler`]):
//! typed characters, pastes from the platform and input-method
//! compositions. The editing keys (arrows, backspace, select all, copy,
//! undo, enter) are GPUI actions in the `text_field` namespace, handled in
//! the [`TextField::KEY_CONTEXT`] key context, and [`TextField::key_bindings`]
//! binds the usual keys to them. Actions rather than raw key-down handlers,
//! so a host can rebind them and kiln's keymap can bind them as commands
//! (`text_field.select_all`) once kiln's key dispatch reaches widgets.
//!
//! Adapted from GPUI's input example: its element shaping one line, its
//! selection and caret quads, its IME marked-range underline and its
//! `bounds_for_range`. Added here: the kiln-core model (grapheme and word
//! motions, undo), horizontal scrolling that keeps the caret visible,
//! double- and triple-click selection, events, focus ring and theme.

use std::ops::Range;

use gpui::{
    App, Bounds, ClipboardItem, ContentMask, Context, CursorStyle, Element, ElementId,
    ElementInputHandler, Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable,
    GlobalElementId, Hsla, InteractiveElement as _, IntoElement, KeyBinding, LayoutId, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement as _, Pixels, Point, Render,
    ShapedLine, SharedString, Style, Styled as _, Subscription, TextRun, UTF16Selection,
    UnderlineStyle, Window, actions, div, fill, point, px, relative, size,
};
use kiln_core::{Motion, TextEdit};

use crate::ActiveTheme as _;

actions!(
    text_field,
    [
        /// Deletes the selection or the grapheme before the caret.
        Backspace,
        /// Deletes the selection or the grapheme after the caret.
        Delete,
        /// Deletes back to the start of the word before the caret.
        DeleteWordLeft,
        /// Deletes up to the end of the word after the caret.
        DeleteWordRight,
        Left,
        Right,
        WordLeft,
        WordRight,
        Home,
        End,
        SelectLeft,
        SelectRight,
        SelectWordLeft,
        SelectWordRight,
        SelectHome,
        SelectEnd,
        SelectAll,
        Copy,
        Cut,
        Paste,
        Undo,
        Redo,
        /// Emits [`TextFieldEvent::Submitted`].
        Submit,
        /// Opens the platform's emoji and symbol picker.
        ShowCharacterPalette,
    ]
);

/// What a [`TextField`] tells its subscribers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextFieldEvent {
    /// The text changed, by the user or an input method (not by
    /// [`TextField::set_value`]).
    Changed,
    /// Enter was pressed ([`Submit`]).
    Submitted,
    /// The field gained focus.
    Focused,
    /// The field lost focus.
    Blurred,
}

/// The caret's width.
const CARET_WIDTH: Pixels = px(1.5);

/// A single-line text field. See the [module docs](self).
pub struct TextField {
    edit: TextEdit,
    placeholder: SharedString,
    /// The text's size.
    font_size: Pixels,
    focus: FocusHandle,
    /// How far the text is scrolled left, to keep the caret in view.
    scroll_x: Pixels,
    /// The last frame's shaped line and its bounds, for the mouse and the
    /// input method.
    last_line: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    /// While the mouse button is down after a click in the field.
    selecting: bool,
    _subscriptions: [Subscription; 2],
}

impl EventEmitter<TextFieldEvent> for TextField {}

impl TextField {
    /// The key context the field's element sets, where its actions are
    /// bound.
    pub const KEY_CONTEXT: &str = "text_field";

    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let subscriptions = [
            cx.on_focus(&focus, window, |_, _, cx| {
                cx.emit(TextFieldEvent::Focused);
                cx.notify();
            }),
            cx.on_blur(&focus, window, |this, _, cx| {
                this.selecting = false;
                cx.emit(TextFieldEvent::Blurred);
                cx.notify();
            }),
        ];
        Self {
            edit: TextEdit::new(),
            placeholder: SharedString::default(),
            font_size: px(13.),
            focus,
            scroll_x: px(0.),
            last_line: None,
            last_bounds: None,
            selecting: false,
            _subscriptions: subscriptions,
        }
    }

    /// Shown, dimmed, while the field is empty.
    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// The initial text, with the caret at its end.
    pub fn text(mut self, text: &str) -> Self {
        self.edit.set_text(text);
        self
    }

    /// The default key bindings for the field's actions, in
    /// [`TextField::KEY_CONTEXT`]: `macos` picks the Mac's word and line
    /// keys (alt and cmd with the arrows) over the PC's (ctrl with the
    /// arrows, home and end).
    pub fn key_bindings(macos: bool) -> Vec<KeyBinding> {
        let cx = Some(Self::KEY_CONTEXT);
        let mut bindings = vec![
            KeyBinding::new("backspace", Backspace, cx),
            KeyBinding::new("shift-backspace", Backspace, cx),
            KeyBinding::new("delete", Delete, cx),
            KeyBinding::new("left", Left, cx),
            KeyBinding::new("right", Right, cx),
            KeyBinding::new("shift-left", SelectLeft, cx),
            KeyBinding::new("shift-right", SelectRight, cx),
            KeyBinding::new("home", Home, cx),
            KeyBinding::new("end", End, cx),
            KeyBinding::new("shift-home", SelectHome, cx),
            KeyBinding::new("shift-end", SelectEnd, cx),
            KeyBinding::new("secondary-a", SelectAll, cx),
            KeyBinding::new("secondary-c", Copy, cx),
            KeyBinding::new("secondary-x", Cut, cx),
            KeyBinding::new("secondary-v", Paste, cx),
            KeyBinding::new("secondary-z", Undo, cx),
            KeyBinding::new("secondary-shift-z", Redo, cx),
            KeyBinding::new("enter", Submit, cx),
        ];
        let word = if macos { "alt" } else { "ctrl" };
        bindings.extend([
            KeyBinding::new(&format!("{word}-left"), WordLeft, cx),
            KeyBinding::new(&format!("{word}-right"), WordRight, cx),
            KeyBinding::new(&format!("{word}-shift-left"), SelectWordLeft, cx),
            KeyBinding::new(&format!("{word}-shift-right"), SelectWordRight, cx),
            KeyBinding::new(&format!("{word}-backspace"), DeleteWordLeft, cx),
            KeyBinding::new(&format!("{word}-delete"), DeleteWordRight, cx),
        ]);
        if macos {
            bindings.extend([
                KeyBinding::new("cmd-left", Home, cx),
                KeyBinding::new("cmd-right", End, cx),
                KeyBinding::new("cmd-shift-left", SelectHome, cx),
                KeyBinding::new("cmd-shift-right", SelectEnd, cx),
                KeyBinding::new("ctrl-cmd-space", ShowCharacterPalette, cx),
            ]);
        } else {
            bindings.push(KeyBinding::new("ctrl-y", Redo, cx));
        }
        bindings
    }

    /// The text's size, 13 pixels unless set.
    pub fn font_size(&self) -> Pixels {
        self.font_size
    }

    /// Sets the text's size.
    pub fn set_font_size(&mut self, size: Pixels, cx: &mut Context<Self>) {
        if self.font_size != size {
            self.font_size = size;
            cx.notify();
        }
    }

    pub fn value(&self) -> &str {
        self.edit.text()
    }

    /// Replaces the text, puts the caret at its end and forgets the undo
    /// history, without emitting [`TextFieldEvent::Changed`]: for the host
    /// setting the value.
    pub fn set_value(&mut self, text: &str, cx: &mut Context<Self>) {
        self.edit.set_text(text);
        cx.notify();
    }

    /// The editing state: the text, selection, composition and history.
    pub fn model(&self) -> &TextEdit {
        &self.edit
    }

    /// The selected bytes, start first.
    pub fn selected_range(&self) -> Range<usize> {
        self.edit.selected_range()
    }

    /// Selects bytes `anchor..head` (snapped to grapheme boundaries).
    pub fn set_selection(&mut self, anchor: usize, head: usize, cx: &mut Context<Self>) {
        self.edit.set_selection(anchor, head);
        cx.notify();
    }

    // Actions.

    fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        let changed = self.edit.backspace();
        self.edited(changed, cx);
    }

    fn delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
        let changed = self.edit.delete();
        self.edited(changed, cx);
    }

    fn delete_word_left(&mut self, _: &DeleteWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        let changed = self.edit.delete_word_left();
        self.edited(changed, cx);
    }

    fn delete_word_right(&mut self, _: &DeleteWordRight, _: &mut Window, cx: &mut Context<Self>) {
        let changed = self.edit.delete_word_right();
        self.edited(changed, cx);
    }

    fn move_by(&mut self, motion: Motion, cx: &mut Context<Self>) {
        self.edit.move_by(motion);
        cx.notify();
    }

    fn select_by(&mut self, motion: Motion, cx: &mut Context<Self>) {
        self.edit.select_by(motion);
        cx.notify();
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.edit.select_all();
        cx.notify();
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        let selected = self.edit.selected_text();
        if !selected.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(selected.to_string()));
        }
    }

    fn cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = self.edit.cut() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            self.edited(true, cx);
        }
    }

    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            let changed = self.edit.paste(&text);
            self.edited(changed, cx);
        }
    }

    fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        let changed = self.edit.undo();
        self.edited(changed, cx);
    }

    fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        let changed = self.edit.redo();
        self.edited(changed, cx);
    }

    fn submit(&mut self, _: &Submit, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(TextFieldEvent::Submitted);
    }

    fn show_character_palette(
        &mut self,
        _: &ShowCharacterPalette,
        window: &mut Window,
        _: &mut Context<Self>,
    ) {
        window.show_character_palette();
    }

    fn edited(&mut self, changed: bool, cx: &mut Context<Self>) {
        if changed {
            cx.emit(TextFieldEvent::Changed);
        }
        cx.notify();
    }

    // The mouse.

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        let offset = self.offset_for_point(event.position);
        match event.click_count {
            0 | 1 if event.modifiers.shift => self.edit.select_to(offset),
            0 | 1 => self.edit.move_caret_to(offset),
            2 => {
                let word = self.edit.word_range_at(offset);
                self.edit.set_selection(word.start, word.end);
            }
            _ => self.edit.select_all(),
        }
        self.selecting = event.click_count <= 1;
        cx.notify();
    }

    fn mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.selecting && event.dragging() {
            self.edit.select_to(self.offset_for_point(event.position));
            cx.notify();
        }
    }

    fn mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.selecting = false;
    }

    /// The byte offset nearest window point `position`, by the last frame.
    fn offset_for_point(&self, position: Point<Pixels>) -> usize {
        let (Some(bounds), Some(line)) = (self.last_bounds, self.last_line.as_ref()) else {
            return self.edit.text().len();
        };
        if self.edit.is_empty() {
            return 0;
        }
        line.closest_index_for_x(position.x - bounds.left() + self.scroll_x)
    }

    /// Scrolls so the caret, at `caret_x` along a line `line_width` wide,
    /// shows in `width`.
    fn scroll_to_caret(&mut self, caret_x: Pixels, line_width: Pixels, width: Pixels) {
        let visible = (width - CARET_WIDTH).max(px(0.));
        if caret_x < self.scroll_x {
            self.scroll_x = caret_x;
        } else if caret_x > self.scroll_x + visible {
            self.scroll_x = caret_x - visible;
        }
        // No blank space after the text once it's scrolled.
        let max = (line_width - visible).max(px(0.));
        self.scroll_x = self.scroll_x.min(max).max(px(0.));
    }

    /// How far the text is scrolled left, in pixels.
    pub fn scroll_x(&self) -> Pixels {
        self.scroll_x
    }

    /// The line's bounds in the window, by the last frame.
    pub fn last_bounds(&self) -> Option<Bounds<Pixels>> {
        self.last_bounds
    }

    /// The window point of byte `offset`, halfway down the line, by the
    /// last frame.
    pub fn point_for_offset(&self, offset: usize) -> Option<Point<Pixels>> {
        let bounds = self.last_bounds?;
        let x = self
            .last_line
            .as_ref()
            .map_or(px(0.), |line| line.x_for_index(offset));
        Some(point(bounds.left() - self.scroll_x + x, bounds.center().y))
    }
}

impl Focusable for TextField {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// The platform's text input: typing, pasting from the platform, and input
/// methods composing text. Ranges are UTF-16, as the platforms give them.
impl EntityInputHandler for TextField {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let bytes = self.edit.range_from_utf16(&range);
        actual_range.replace(self.edit.range_to_utf16(&bytes));
        Some(self.edit.text()[bytes].to_string())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let selection = self.edit.selection();
        Some(UTF16Selection {
            range: self.edit.range_to_utf16(&selection.range()),
            reversed: selection.reversed(),
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.edit
            .marked_range()
            .map(|range| self.edit.range_to_utf16(&range))
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.edit.unmark();
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let changed = match range {
            Some(range) => {
                let bytes = self.edit.range_from_utf16(&range);
                self.edit.replace_range(bytes, text)
            }
            None => self.edit.insert(text),
        };
        self.edited(changed, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range.map(|range| self.edit.range_from_utf16(&range));
        // `selected` is UTF-16 within `text`.
        let selected = selected.map(|selected| {
            let to_bytes = |utf16: usize| {
                let mut seen = 0;
                text.char_indices()
                    .find(|(_, ch)| {
                        let found = seen >= utf16;
                        seen += ch.len_utf16();
                        found
                    })
                    .map_or(text.len(), |(ix, _)| ix)
            };
            to_bytes(selected.start)..to_bytes(selected.end)
        });
        let changed = self.edit.replace_and_mark(range, text, selected);
        self.edited(changed, cx);
    }

    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        // Empty, the composition starts at the field's left edge.
        let Some(line) = self.last_line.as_ref() else {
            return Some(Bounds::new(bounds.origin, size(px(0.), bounds.size.height)));
        };
        let range = self.edit.range_from_utf16(&range);
        let left = bounds.left() - self.scroll_x;
        Some(Bounds::from_corners(
            point(left + line.x_for_index(range.start), bounds.top()),
            point(left + line.x_for_index(range.end), bounds.bottom()),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let bounds = self.last_bounds?;
        let line = self.last_line.as_ref()?;
        let offset = line.index_for_x(point.x - bounds.left() + self.scroll_x)?;
        Some(self.edit.offset_to_utf16(offset))
    }
}

impl Render for TextField {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors;
        let focused = self.focus.is_focused(window);
        div()
            .key_context(Self::KEY_CONTEXT)
            .track_focus(&self.focus)
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::delete_word_left))
            .on_action(cx.listener(Self::delete_word_right))
            .on_action(cx.listener(|this, _: &Left, _, cx| this.move_by(Motion::Left, cx)))
            .on_action(cx.listener(|this, _: &Right, _, cx| this.move_by(Motion::Right, cx)))
            .on_action(cx.listener(|this, _: &WordLeft, _, cx| this.move_by(Motion::WordLeft, cx)))
            .on_action(
                cx.listener(|this, _: &WordRight, _, cx| this.move_by(Motion::WordRight, cx)),
            )
            .on_action(cx.listener(|this, _: &Home, _, cx| this.move_by(Motion::Home, cx)))
            .on_action(cx.listener(|this, _: &End, _, cx| this.move_by(Motion::End, cx)))
            .on_action(cx.listener(|this, _: &SelectLeft, _, cx| this.select_by(Motion::Left, cx)))
            .on_action(
                cx.listener(|this, _: &SelectRight, _, cx| this.select_by(Motion::Right, cx)),
            )
            .on_action(
                cx.listener(|this, _: &SelectWordLeft, _, cx| this.select_by(Motion::WordLeft, cx)),
            )
            .on_action(
                cx.listener(|this, _: &SelectWordRight, _, cx| {
                    this.select_by(Motion::WordRight, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &SelectHome, _, cx| this.select_by(Motion::Home, cx)))
            .on_action(cx.listener(|this, _: &SelectEnd, _, cx| this.select_by(Motion::End, cx)))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_action(cx.listener(Self::submit))
            .on_action(cx.listener(Self::show_character_palette))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::mouse_up))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .flex()
            .items_center()
            .w_full()
            .h(px(26.))
            .px_2()
            .rounded_sm()
            .border_1()
            .border_color(if focused {
                colors.list.row_focus_border
            } else {
                colors.border.default
            })
            .bg(colors.surface.background)
            .text_size(self.font_size)
            .line_height(px(18.))
            .text_color(colors.tab.active_text)
            .overflow_hidden()
            .child(TextLine {
                field: cx.entity(),
                placeholder_color: colors.text.muted,
                selection_color: colors.list.row_selected,
                caret_color: colors.tab.active_text,
            })
    }
}

/// The field's line of text, its selection and its caret.
struct TextLine {
    field: Entity<TextField>,
    placeholder_color: Hsla,
    selection_color: Hsla,
    caret_color: Hsla,
}

struct LinePaint {
    line: ShapedLine,
    /// Where the line starts: the bounds' left, less the scroll.
    origin: Point<Pixels>,
    selection: Option<Bounds<Pixels>>,
    caret: Option<Bounds<Pixels>>,
}

impl IntoElement for TextLine {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextLine {
    type RequestLayoutState = ();
    type PrepaintState = LinePaint;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = window.line_height().into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let field = self.field.read(cx);
        let edit = &field.edit;
        let style = window.text_style();
        let (text, color) = if edit.is_empty() {
            (field.placeholder.clone(), self.placeholder_color)
        } else {
            (SharedString::from(edit.text().to_string()), style.color)
        };
        let run = TextRun {
            len: text.len(),
            font: style.font(),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        // The composition is underlined.
        let runs = match edit.marked_range() {
            Some(marked) if !edit.is_empty() => [
                (0..marked.start, None),
                (
                    marked.clone(),
                    Some(UnderlineStyle {
                        color: Some(color),
                        thickness: px(1.),
                        wavy: false,
                    }),
                ),
                (marked.end..text.len(), None),
            ]
            .into_iter()
            .filter(|(range, _)| !range.is_empty())
            .map(|(range, underline)| TextRun {
                len: range.len(),
                underline,
                ..run.clone()
            })
            .collect(),
            _ => vec![run],
        };
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line = window
            .text_system()
            .shape_line(text, font_size, &runs, None);

        let selection = edit.selected_range();
        let caret_x = if edit.is_empty() {
            px(0.)
        } else {
            line.x_for_index(edit.caret())
        };
        let line_width = if edit.is_empty() {
            px(0.)
        } else {
            line.width()
        };
        let scroll_x = self.field.update(cx, |field, _| {
            field.scroll_to_caret(caret_x, line_width, bounds.size.width);
            field.scroll_x
        });
        let origin = point(bounds.left() - scroll_x, bounds.top());
        let (selection, caret) = if selection.is_empty() {
            let caret = Bounds::new(
                point(origin.x + caret_x, bounds.top()),
                size(CARET_WIDTH, bounds.size.height),
            );
            (None, Some(caret))
        } else {
            let selection = Bounds::from_corners(
                point(origin.x + line.x_for_index(selection.start), bounds.top()),
                point(origin.x + line.x_for_index(selection.end), bounds.bottom()),
            );
            (Some(selection), None)
        };
        LinePaint {
            line,
            origin,
            selection,
            caret,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        paint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus = self.field.read(cx).focus.clone();
        window.handle_input(
            &focus,
            ElementInputHandler::new(bounds, self.field.clone()),
            cx,
        );
        let focused = focus.is_focused(window);
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            if let Some(selection) = paint.selection {
                window.paint_quad(fill(selection, self.selection_color));
            }
            paint
                .line
                .paint(
                    paint.origin,
                    window.line_height(),
                    gpui::TextAlign::Left,
                    None,
                    window,
                    cx,
                )
                .ok();
            if focused && let Some(caret) = paint.caret {
                window.paint_quad(fill(caret, self.caret_color));
            }
        });
        let line = paint.line.clone();
        let has_text = !self.field.read(cx).edit.is_empty();
        self.field.update(cx, |field, _| {
            field.last_line = has_text.then_some(line);
            field.last_bounds = Some(bounds);
        });
    }
}
