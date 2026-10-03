use std::{ops::Range, time::Duration};

use fe_buffer::Anchor;
use fe_editor::{EditorModel, Movement};
use fe_syntax::Syntax;
use gpui::{
    App, AppContext as _, Bounds, ClipboardItem, Context, CursorStyle, EntityInputHandler,
    FocusHandle, Focusable, InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ParentElement as _, Pixels, Point, Render, ScrollWheelEvent,
    Styled as _, Task, UTF16Selection, Window, div, px,
};

use crate::{
    element::{EditorElement, GlyphWidths},
    timing::FrameTimings,
    *,
};

/// How long a frame may spend reparsing before the parse moves to a
/// background thread. Small edits to files of 10k lines reparse within it.
const SYNC_PARSE_BUDGET: Duration = Duration::from_millis(1);

/// An editor for one buffer.
pub struct EditorView {
    pub(crate) model: EditorModel,
    pub(crate) theme: EditorTheme,
    pub(crate) focus: FocusHandle,
    /// How far the content is scrolled, in pixels (both ≥ 0).
    pub(crate) scroll: Point<Pixels>,
    /// Scroll the primary cursor into view on the next frame.
    pub(crate) autoscroll: bool,
    /// What the last frame laid out, for hit testing and IME.
    pub(crate) layout: Option<crate::element::FrameLayout>,
    drag: Option<Drag>,
    /// Text being composed by an input method.
    marked: Option<Range<Anchor>>,
    /// The buffer's syntax tree, once its language is loaded.
    pub(crate) syntax: Option<Syntax>,
    /// A language load or parse running in the background.
    syntax_task: Option<Task<()>>,
    pub(crate) frame_timings: FrameTimings,
}

/// What a mouse drag extends: characters, or whole words or rows starting
/// from the range first clicked.
#[derive(Clone)]
enum Drag {
    Chars,
    Words(Range<usize>),
    Rows(Range<usize>),
}

impl EditorView {
    /// An editor for `model`, highlighted according to its file's language.
    pub fn new(mut model: EditorModel, theme: EditorTheme, cx: &mut Context<Self>) -> Self {
        let font = gpui::font(theme.font_family.clone());
        model.set_char_widths(Box::new(GlyphWidths::new(
            cx.text_system().clone(),
            &font,
            theme.font_size,
        )));
        let mut view = Self {
            model,
            theme,
            focus: cx.focus_handle(),
            scroll: Point::default(),
            autoscroll: true,
            layout: None,
            drag: None,
            marked: None,
            syntax: None,
            syntax_task: None,
            frame_timings: FrameTimings::default(),
        };
        let language = view
            .model
            .path()
            .and_then(fe_syntax::language_name_for_path);
        if let Some(name) = language {
            if let Some(language) = fe_syntax::loaded_language(name) {
                view.syntax = Some(Syntax::new(language, view.model.snapshot()));
            } else {
                // Compiling a highlight query can take a few frames.
                view.syntax_task = Some(cx.spawn(async move |this, cx| {
                    let language = cx
                        .background_spawn(async move { fe_syntax::language_by_name(name) })
                        .await;
                    this.update(cx, |view, cx| {
                        view.syntax_task = None;
                        view.syntax =
                            language.map(|language| Syntax::new(language, view.model.snapshot()));
                        cx.notify();
                    })
                    .ok();
                }));
            }
        }
        view
    }

    pub fn model(&self) -> &EditorModel {
        &self.model
    }

    pub fn model_mut(&mut self) -> &mut EditorModel {
        &mut self.model
    }

    /// How long the editor took to lay out and paint its recent frames.
    pub fn frame_timings(&self) -> &FrameTimings {
        &self.frame_timings
    }

    pub fn frame_timings_mut(&mut self) -> &mut FrameTimings {
        &mut self.frame_timings
    }

    /// Call after anything that moves the cursor or changes the text.
    fn changed(&mut self, cx: &mut Context<Self>) {
        self.autoscroll = true;
        cx.notify();
    }

    fn movement(&mut self, movement: Movement, extend: bool, cx: &mut Context<Self>) {
        self.model.move_cursors(movement, extend);
        self.changed(cx);
    }

    fn page_rows(&self) -> u32 {
        self.layout.as_ref().map_or(20, |layout| {
            layout.frame.visible_rows().saturating_sub(2).max(1)
        })
    }

    /// The buffer offset under a window position, in the last frame.
    fn offset_for_position(&self, position: Point<Pixels>) -> usize {
        self.layout.as_ref().map_or(0, |layout| {
            layout
                .frame
                .offset_at(self.model.text(), layout.to_frame(position))
        })
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        let offset = self.offset_for_position(event.position);
        self.marked = None;
        match event.click_count {
            2 => {
                let word = self.model.word_range_at(offset);
                self.model.select_range(word.clone(), false);
                self.drag = Some(Drag::Words(word));
            }
            3.. => {
                let row = self.model.line_range_at(offset);
                self.model.select_range(row.clone(), false);
                self.drag = Some(Drag::Rows(row));
            }
            _ => {
                if event.modifiers.shift {
                    self.model.extend_primary_to(offset);
                } else {
                    self.model.set_cursor(offset);
                }
                self.drag = Some(Drag::Chars);
            }
        }
        self.changed(cx);
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = self.drag.clone() else {
            return;
        };
        if event.pressed_button != Some(MouseButton::Left) {
            self.drag = None;
            return;
        }
        let offset = self.offset_for_position(event.position);
        let (origin, under) = match drag {
            Drag::Chars => {
                self.model.extend_primary_to(offset);
                self.changed(cx);
                return;
            }
            Drag::Words(origin) => (origin, self.model.word_range_at(offset)),
            Drag::Rows(origin) => (origin, self.model.line_range_at(offset)),
        };
        // Grow from the originally clicked word or row, whole units at a time.
        if under.start < origin.start {
            self.model.select_range(under.start..origin.end, true);
        } else {
            self.model
                .select_range(origin.start..under.end.max(origin.end), false);
        }
        self.changed(cx);
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.drag = None;
    }

    fn on_scroll(&mut self, event: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        let line_height = self
            .layout
            .as_ref()
            .map_or(px(20.), |layout| px(layout.frame.line_height));
        let delta = event.delta.pixel_delta(line_height);
        self.scroll.x = (self.scroll.x - delta.x).max(px(0.));
        self.scroll.y = (self.scroll.y - delta.y).max(px(0.));
        // The element clamps against the content size on the next frame.
        self.autoscroll = false;
        cx.notify();
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        let (text, _) = self.model.copy_text();
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }

    fn cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        let text = self.model.cut();
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.changed(cx);
    }

    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.model.paste(&text);
            self.changed(cx);
        }
    }

    fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        if let Err(error) = self.model.undo() {
            log::error!("undo failed: {error}");
        }
        self.changed(cx);
    }

    fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        if let Err(error) = self.model.redo() {
            log::error!("redo failed: {error}");
        }
        self.changed(cx);
    }

    fn save(&mut self, _: &Save, _: &mut Window, cx: &mut Context<Self>) {
        if let Err(error) = self.model.save() {
            log::error!("save failed: {error}");
        }
        cx.notify();
    }

    fn utf16_range(&self, range: &Range<usize>) -> Range<usize> {
        let text = self.model.text();
        text.offset_to_utf16(range.start)..text.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        let text = self.model.text();
        text.utf16_to_offset(range.start)..text.utf16_to_offset(range.end)
    }

    /// Brings the syntax tree up to date with the text: within this frame if
    /// the reparse is quick, otherwise on a background thread while the stale
    /// tree keeps highlighting.
    fn sync_syntax(&mut self, cx: &mut Context<Self>) {
        let Some(syntax) = &mut self.syntax else {
            return;
        };
        syntax.edit(self.model.snapshot());
        if self.syntax_task.is_some() || syntax.parse_within(SYNC_PARSE_BUDGET) {
            return;
        }
        let job = syntax.parse_job();
        self.syntax_task = Some(cx.spawn(async move |this, cx| {
            let parsed = cx.background_spawn(async move { job.run() }).await;
            this.update(cx, |view, cx| {
                view.syntax_task = None;
                if let Some(syntax) = &mut view.syntax {
                    syntax.finish(parsed);
                }
                // Repaint, and reparse whatever changed meanwhile.
                cx.notify();
            })
            .ok();
        }));
    }

    fn marked_range(&self) -> Option<Range<usize>> {
        self.marked
            .as_ref()
            .map(|range| self.model.snapshot().resolve_range(range))
    }
}

macro_rules! movement_actions {
    ($($action:ident => $movement:expr, $extend:expr;)*) => {
        impl EditorView {
            fn bind_movements(div: gpui::Stateful<gpui::Div>, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
                div$(.on_action(cx.listener(|this, _: &$action, _, cx| {
                    let movement = $movement(this.page_rows());
                    this.movement(movement, $extend, cx)
                })))*
            }
        }
    };
}

movement_actions! {
    MoveLeft => |_| Movement::Left, false;
    MoveRight => |_| Movement::Right, false;
    MoveUp => |_| Movement::Up, false;
    MoveDown => |_| Movement::Down, false;
    MoveWordLeft => |_| Movement::WordLeft, false;
    MoveWordRight => |_| Movement::WordRight, false;
    MoveLineStart => |_| Movement::LineStart, false;
    MoveLineEnd => |_| Movement::LineEnd, false;
    MoveDocumentStart => |_| Movement::DocumentStart, false;
    MoveDocumentEnd => |_| Movement::DocumentEnd, false;
    PageUp => Movement::PageUp, false;
    PageDown => Movement::PageDown, false;
    SelectLeft => |_| Movement::Left, true;
    SelectRight => |_| Movement::Right, true;
    SelectUp => |_| Movement::Up, true;
    SelectDown => |_| Movement::Down, true;
    SelectWordLeft => |_| Movement::WordLeft, true;
    SelectWordRight => |_| Movement::WordRight, true;
    SelectLineStart => |_| Movement::LineStart, true;
    SelectLineEnd => |_| Movement::LineEnd, true;
    SelectDocumentStart => |_| Movement::DocumentStart, true;
    SelectDocumentEnd => |_| Movement::DocumentEnd, true;
    SelectPageUp => Movement::PageUp, true;
    SelectPageDown => Movement::PageDown, true;
}

impl Render for EditorView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_syntax(cx);
        let div = div()
            .id("editor")
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .cursor(CursorStyle::IBeam)
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .on_action(cx.listener(|this, _: &SelectAll, _, cx| {
                this.model.select_all();
                this.changed(cx);
            }))
            .on_action(cx.listener(|this, _: &Backspace, _, cx| {
                this.model.backspace();
                this.changed(cx);
            }))
            .on_action(cx.listener(|this, _: &Delete, _, cx| {
                this.model.delete_forward();
                this.changed(cx);
            }))
            .on_action(cx.listener(|this, _: &DeleteWordLeft, _, cx| {
                this.model.delete_word_left();
                this.changed(cx);
            }))
            .on_action(cx.listener(|this, _: &Newline, _, cx| {
                this.model.newline();
                this.changed(cx);
            }))
            .on_action(cx.listener(|this, _: &Tab, _, cx| {
                this.model.tab();
                this.changed(cx);
            }))
            .on_action(cx.listener(|this, _: &Fold, _, cx| {
                this.model.fold();
                this.changed(cx);
            }))
            .on_action(cx.listener(|this, _: &Unfold, _, cx| {
                this.model.unfold();
                this.changed(cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleSoftWrap, _, cx| {
                this.model.toggle_soft_wrap();
                this.changed(cx);
            }))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_action(cx.listener(Self::save));
        Self::bind_movements(div, cx).child(EditorElement::new(cx.entity()))
    }
}

impl Focusable for EditorView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl EntityInputHandler for EditorView {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.utf16_range(&range));
        Some(self.model.text().slice_to_string(range))
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let selection = self.model.primary_selection();
        Some(UTF16Selection {
            range: self.utf16_range(&selection.range),
            reversed: selection.reversed,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked_range().map(|range| self.utf16_range(&range))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .map(|range| self.range_from_utf16(&range))
            .or_else(|| self.marked_range());
        if let Some(range) = range {
            self.model.select_range(range, false);
        }
        // With no explicit range this types at every cursor.
        self.model.insert(text);
        self.marked = None;
        self.changed(cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .map(|range| self.range_from_utf16(&range))
            .or_else(|| self.marked_range())
            .unwrap_or_else(|| self.model.primary_selection().range);
        self.model.select_range(range.clone(), false);
        self.model.insert(text);
        let start = range.start;
        let snapshot = self.model.snapshot();
        self.marked = (!text.is_empty())
            .then(|| snapshot.anchor_before(start)..snapshot.anchor_after(start + text.len()));
        if let Some(selected) = new_selected_range_utf16 {
            // Relative to the composed text, in UTF-16.
            let base = self.model.text().offset_to_utf16(start);
            let selected = self.range_from_utf16(&(base + selected.start..base + selected.end));
            self.model.select_range(selected, false);
        }
        self.changed(cx);
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _element_bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let range = self.range_from_utf16(&range_utf16);
        let layout = self.layout.as_ref()?;
        let caret = layout.frame.caret_bounds(range.start)?;
        Some(layout.bounds(caret))
    }

    fn character_index_for_point(
        &mut self,
        position: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let offset = self.offset_for_position(position);
        Some(self.model.text().offset_to_utf16(offset))
    }
}
