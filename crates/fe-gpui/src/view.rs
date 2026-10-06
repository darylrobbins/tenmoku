use std::{ops::Range, time::Duration};

use fe_buffer::{Anchor, Bias, BufferSnapshot};
use fe_display::layout::RowLayout;
use fe_editor::EditorModel;
use fe_syntax::{Language, ParseJob, Spawner, Syntax, SyntaxDriver};
use gpui::{
    App, AppContext as _, Bounds, Context, CursorStyle, EntityInputHandler, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, ParentElement as _, Pixels, Point, Render, ScrollWheelEvent, Styled as _, Task,
    UTF16Selection, Window, div, px,
};

use crate::{
    element::{EditorElement, GlyphWidths},
    timing::FrameTimings,
    *,
};

/// How long a frame may spend reparsing before the parse moves to a
/// background thread. Small edits to files of 10k lines reparse within it.
const SYNC_PARSE_BUDGET: Duration = Duration::from_millis(1);

/// How much of the buffer's start is read for a shebang, when the path names
/// no language.
const DETECT_BYTES: usize = 512;

#[cfg(test)]
pub(crate) type SyntaxHook = Box<dyn FnMut(&Syntax)>;

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
    pub(crate) syntax: Option<SyntaxDriver<Task<()>>>,
    /// The language loading in the background.
    language_task: Option<Task<()>>,
    /// Injected languages loading in the background.
    injected_task: Option<Task<()>>,
    /// Runs just before the view takes the injected languages a frame
    /// lacked, so a test can compile one in between.
    #[cfg(test)]
    pub(crate) before_taking_missing: Option<SyntaxHook>,
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
        model.set_char_widths(glyph_widths(&theme, cx));
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
            language_task: None,
            injected_task: None,
            #[cfg(test)]
            before_taking_missing: None,
            frame_timings: FrameTimings::default(),
        };
        let text = view.model.text();
        let start = text.slice_to_string(0..text.clip_offset(DETECT_BYTES, Bias::Left));
        let language = fe_syntax::detect_language_name(view.model.path(), &start);
        view.set_language(language, cx);
        view
    }

    /// Highlights the buffer as the language called `name`, or as plain text
    /// for `None` or a name that isn't compiled in. Overrides whatever was
    /// detected when the view opened, and sticks until the next call. A
    /// language not loaded yet loads in the background, with the buffer
    /// plain until it's ready.
    pub fn set_language(&mut self, name: Option<&str>, cx: &mut Context<Self>) {
        // Dropping the driver and the load cancels them, so neither can
        // install anything for the old language.
        self.syntax = None;
        self.language_task = None;
        cx.notify();
        let Some(name) = name.and_then(|name| fe_syntax::language_names().find(|&n| n == name))
        else {
            return;
        };
        if let Some(language) = fe_syntax::loaded_language(name) {
            self.syntax = Some(syntax_driver(language, self.model.snapshot()));
            return;
        }
        // Compiling a highlight query can take a few frames.
        self.language_task = Some(cx.spawn(async move |this, cx| {
            let language = cx
                .background_spawn(async move { fe_syntax::language_by_name(name) })
                .await;
            this.update(cx, |view, cx| {
                view.language_task = None;
                view.syntax =
                    language.map(|language| syntax_driver(language, view.model.snapshot()));
                cx.notify();
            })
            .ok();
        }));
    }

    /// The buffer's syntax, once its language has loaded; `None` for plain
    /// text. Its [`Syntax::tree`] has been edited to match
    /// [`Syntax::version`], which may trail the model's snapshot until the
    /// next frame, but the tree may not have been reparsed since the latest
    /// edits: [`Syntax::needs_parse`] says whether it has. Node positions are
    /// right for the text at that version either way; only the structure of
    /// the edited text may be stale.
    pub fn syntax(&self) -> Option<&Syntax> {
        self.syntax.as_ref().map(SyntaxDriver::syntax)
    }

    pub fn model(&self) -> &EditorModel {
        &self.model
    }

    /// Takes the font family, size and line height, the tab size and the
    /// soft-wrap widths from `settings`, and redraws. Soft wrap itself is
    /// left alone: see [`EditorView::reload_settings`].
    pub fn apply_settings(&mut self, settings: &EditorSettings, cx: &mut Context<Self>) {
        self.theme.set_metrics(settings, cx);
        self.model.set_char_widths(glyph_widths(&self.theme, cx));
        if self.model.tab_size() != settings.tab_size {
            self.model.set_tab_size(settings.tab_size);
        }
        cx.notify();
    }

    /// Applies a reload's settings. Soft wrap changes only if the reload
    /// changed it, so a reload of anything else keeps the editor's `alt-z`
    /// toggle.
    pub fn reload_settings(&mut self, event: &SettingsEvent, cx: &mut Context<Self>) {
        let (settings, previous) = (&event.settings.editor, &event.previous.editor);
        if settings == previous {
            return;
        }
        self.apply_settings(settings, cx);
        if settings.soft_wrap != previous.soft_wrap {
            self.model.set_soft_wrap(settings.soft_wrap);
        }
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
    pub(crate) fn changed(&mut self, cx: &mut Context<Self>) {
        self.autoscroll = true;
        cx.notify();
    }

    pub(crate) fn page_rows(&self) -> u32 {
        self.layout.as_ref().map_or(20, |layout| {
            layout.frame.visible_rows().saturating_sub(2).max(1)
        })
    }

    /// The buffer offset under a window position, in the last frame.
    fn offset_for_position(&mut self, position: Point<Pixels>) -> usize {
        let text = self.model.text();
        self.layout
            .as_mut()
            .map_or(0, |layout| layout.offset_at(text, position))
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        if self.toggle_fold_at(event.position) {
            self.drag = None;
            cx.notify();
            return;
        }
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

    /// Of a fold toggle or marker under a window position in the last frame,
    /// unfolds its row if it's folded or folds its candidate if not. Carets
    /// the fold hides move to its header, as with the keyboard. Returns
    /// whether there was one there.
    fn toggle_fold_at(&mut self, position: Point<Pixels>) -> bool {
        let Some(layout) = &self.layout else {
            return false;
        };
        let at = layout.to_frame(position);
        let Some(area) = layout
            .frame
            .fold_click_areas()
            .find(|area| area.rect.contains(at))
        else {
            return false;
        };
        let row = area.buffer_row;
        if area.folded {
            self.model.unfold_rows(row..=row);
        } else if let Some(candidate) = synced_syntax(&self.syntax, &self.model)
            .and_then(|syntax| fe_syntax::fold_range_at(syntax, row))
            .filter(|candidate| *candidate.start() == row)
        {
            self.model.fold_rows(candidate);
        }
        true
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
    /// tree keeps highlighting. A file too big to parse whole gets a window
    /// around the text the last frame showed, parsed in the background.
    fn sync_syntax(&mut self, cx: &mut Context<Self>) {
        let visible = self
            .layout
            .as_ref()
            .map_or(0..0, |layout| shown_text(&layout.frame.rows));
        if let Some(syntax) = &mut self.syntax {
            let language = syntax.syntax().language();
            syntax.sync(
                self.model.snapshot(),
                visible,
                &mut ParseSpawner { cx, language },
            );
        }
    }

    /// Compiles the injected languages the last frame's highlights lacked,
    /// such as a Markdown fence's, in the background, then draws again.
    pub(crate) fn load_injected_languages(&mut self, cx: &mut Context<Self>) {
        let Some(syntax) = &self.syntax else {
            return;
        };
        #[cfg(test)]
        if let Some(hook) = &mut self.before_taking_missing {
            hook(syntax.syntax());
        }
        // Taken even while a task runs: its redraw highlights again, and
        // finds any still missing.
        let missing = syntax.syntax().take_missing_languages();
        if missing.is_empty() || self.injected_task.is_some() {
            return;
        }
        self.injected_task = Some(cx.spawn(async move |this, cx| {
            cx.background_spawn(async move {
                for name in missing {
                    fe_syntax::language_by_name(name);
                }
            })
            .await;
            this.update(cx, |view, cx| {
                view.injected_task = None;
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

/// Measures chars in `theme`'s font, for soft wrap.
fn glyph_widths(theme: &EditorTheme, cx: &App) -> Box<GlyphWidths> {
    let font = gpui::font(theme.font_family.clone());
    Box::new(GlyphWidths::new(
        cx.text_system().clone(),
        &font,
        theme.font_size,
    ))
}

/// The text a frame shows, to parse a window around: of its longest run of
/// rows with nothing hidden between them, from the first shaped byte to the
/// last. A fold hides rows, and only part of a long row is shaped.
fn shown_text<L>(rows: &[RowLayout<L>]) -> Range<usize> {
    rows.chunk_by(|above, below| {
        below.buffer_row <= above.buffer_row + 1
            && above.shown().end == above.end
            && below.shown().start == below.start
    })
    .max_by_key(|run| run.len())
    .map_or(0..0, |run| {
        run[0].shown().start..run[run.len() - 1].shown().end
    })
}

/// The syntax tree, if it's been edited to match `model`'s text.
pub(crate) fn synced_syntax<'a>(
    syntax: &'a Option<SyntaxDriver<Task<()>>>,
    model: &EditorModel,
) -> Option<&'a Syntax> {
    let syntax = syntax.as_ref()?.syntax();
    (syntax.version() == model.snapshot().version()).then_some(syntax)
}

fn syntax_driver(language: &'static Language, snapshot: &BufferSnapshot) -> SyntaxDriver<Task<()>> {
    SyntaxDriver::new(Syntax::new(language, snapshot), SYNC_PARSE_BUDGET)
}

/// Runs a view's parses on GPUI's background executor and hands the results
/// back to it.
struct ParseSpawner<'a, 'b> {
    cx: &'a mut Context<'b, EditorView>,
    /// The language being parsed.
    language: &'static Language,
}

impl Spawner for ParseSpawner<'_, '_> {
    type Task = Task<()>;

    fn spawn(&mut self, job: ParseJob) -> Task<()> {
        let language = self.language;
        self.cx.spawn(async move |this, cx| {
            let parsed = cx.background_spawn(async move { job.run() }).await;
            this.update(cx, |view, cx| {
                // `set_language` drops the driver, cancelling this, but a
                // tree of another language must never be installed: the
                // driver only checks the buffer and version.
                if let Some(syntax) = &mut view.syntax
                    && std::ptr::eq(syntax.syntax().language(), language)
                {
                    syntax.finish(parsed, &mut ParseSpawner { cx, language });
                }
                // Repaint, and reparse whatever changed or scrolled into
                // view meanwhile.
                cx.notify();
            })
            .ok();
        })
    }
}

impl Render for EditorView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_syntax(cx);
        // With no syntax there's no window to ask for.
        let windowed = self.syntax.as_ref().is_some_and(|driver| {
            self.model.snapshot().len() > driver.syntax().language().parse_limit()
        });
        if windowed {
            // This frame may scroll; once it's painted, ask for a window
            // around what it shows.
            cx.defer_in(window, |view, _, cx| view.sync_syntax(cx));
        }
        div()
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
            .on_action(cx.listener(Self::on_run_command))
            .child(EditorElement::new(cx.entity()))
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
        let Some(range) = range else {
            // Typing, at every cursor: recorded like any key binding.
            self.type_text(text, cx);
            return;
        };
        self.model.select_range(range, false);
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
        self.layout.as_mut()?.caret_bounds(range.start)
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

#[cfg(test)]
mod language_tests;

#[cfg(test)]
mod window_tests;
