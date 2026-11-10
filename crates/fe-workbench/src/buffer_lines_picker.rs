//! Search Lines in Buffer (`search.buffer_lines`, LazyVim's `<leader>sb`,
//! telescope's `current_buffer_fuzzy_find`): a [`Picker`] over the active
//! editor's rows that aren't blank, each as `row  text`, its text from the
//! first that isn't whitespace.
//!
//! Rows match the query by subsequence over their text, ignoring case, the
//! chars matched highlighted, in buffer order. Only the first
//! [`MAX_ROWS`] rows of the buffer are listed; a longer buffer says so in
//! the status line under the input. A row's whole text is matched; a row
//! longer than [`MAX_ROW_CHARS`] chars shows as that many around its first
//! matched char ([`excerpt`]).
//!
//! Moving the selection previews the row in the editor as the symbol
//! picker previews a symbol ([`RowPreview`]): it scrolls there, opening a
//! fold that hides it until the selection moves on, and leaves the caret
//! alone; dismissing puts the scroll and the folds back. Enter puts the
//! caret at the row's first char that isn't whitespace and focuses the
//! editor, a jump that back returns from. A row the text no longer has
//! (edited meanwhile) clamps to the text's end.

use std::ops::{Range, RangeInclusive};

use fe_buffer::{ResolvedSelection, Version};
use fe_gpui::EditorView;
use fe_text::Text;
use fe_ui::{HighlightedLabel, char_ranges};
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, FocusHandle, Focusable as _,
    IntoElement as _, ParentElement as _, SharedString, Styled as _, Subscription, WeakEntity,
    Window, div,
};
use kiln_l10n::{text, tr};

use crate::{
    Workbench,
    picker::{Picker, PickerDelegate},
    toolbar_picker::subsequence,
    workbench::Palette,
};

/// The buffer rows listed at most.
pub(crate) const MAX_ROWS: u32 = 50_000;

/// The chars of a row shown at most.
pub(crate) const MAX_ROW_CHARS: usize = 1_000;

/// The chars an excerpt shows before its first matched char.
const EXCERPT_LEAD: usize = 20;

/// The folds before a preview opened some, and what says they still apply.
struct FoldsBefore {
    folds: Vec<RangeInclusive<u32>>,
    /// The text they're rows of.
    version: Version,
    /// None of them hid a caret.
    selections: Vec<ResolvedSelection>,
}

/// Scrolls an editor to rows a picker's selection moves to, and puts it
/// back: see the module docs.
pub(crate) struct RowPreview {
    editor: Option<WeakEntity<EditorView>>,
    /// The row last scrolled to, or the one not to scroll to: the first
    /// row of a list that only just opened.
    previewed: Option<u32>,
    /// The editor's scroll before the first preview, to go back to.
    scroll_before: Option<f32>,
    /// What a preview that opened folds found.
    folds_before: Option<FoldsBefore>,
}

impl RowPreview {
    pub(crate) fn new(editor: Option<WeakEntity<EditorView>>) -> Self {
        Self {
            editor,
            previewed: None,
            scroll_before: None,
            folds_before: None,
        }
    }

    /// The editor previewed in, while it's open.
    pub(crate) fn editor(&self) -> Option<Entity<EditorView>> {
        self.editor.as_ref()?.upgrade()
    }

    /// Takes `row` as previewed without scrolling: the first row of a list
    /// that only just opened, which isn't one moved to.
    pub(crate) fn opened_at(&mut self, row: Option<u32>) {
        self.previewed = row;
    }

    /// Scrolls the editor to `row` if it hasn't been yet, a third of the
    /// way down the view, opening a fold that hides it and closing what the
    /// last preview opened. The caret stays where it is.
    pub(crate) fn show(&mut self, row: u32, cx: &mut App) {
        if self.previewed.replace(row) == Some(row) {
            return;
        }
        let Some(editor) = self.editor() else {
            return;
        };
        editor.update(cx, |editor, cx| {
            // No frame yet: nothing to scroll from.
            let Some((_, rows)) = editor.viewport_rows() else {
                return;
            };
            self.scroll_before.get_or_insert(editor.scroll_top());
            self.close_folds(editor);
            if editor.model().display().hidden_rows(row).is_some() {
                let model = editor.model();
                let folds = model.display().folds();
                self.folds_before = Some(FoldsBefore {
                    folds: folds.iter().map(|fold| fold.start..=fold.end).collect(),
                    version: model.snapshot().version(),
                    selections: model.selections(),
                });
                editor.model_mut().unfold_rows(row..=row);
            }
            // The rows above it are screen rows: a row of the text may wrap
            // to more than the view holds.
            editor.scroll_to_buffer_line(row as f32, cx);
            let top = editor.scroll_top() - (rows / 3) as f32;
            editor.set_scroll_top(top, cx);
        });
    }

    /// Closes the folds a preview opened, unless the text or a caret moved
    /// since: their rows may no longer say which they were, or may now hide
    /// the caret. The selections are left as they are.
    pub(crate) fn close_folds(&mut self, editor: &mut EditorView) {
        let Some(before) = self.folds_before.take() else {
            return;
        };
        let model = editor.model();
        if model.snapshot().version() == before.version && model.selections() == before.selections {
            editor.model_mut().restore_folds(before.folds);
        }
    }

    /// A pick landed: the preview's scroll stands, and the folds it opened
    /// are left to the pick.
    pub(crate) fn keep(&mut self) {
        self.scroll_before = None;
        self.folds_before = None;
    }

    /// Dismissed: closes the folds the preview opened and puts the scroll
    /// back.
    pub(crate) fn restore(&mut self, cx: &mut App) {
        if self.scroll_before.is_none() && self.folds_before.is_none() {
            return;
        }
        let Some(editor) = self.editor() else {
            return;
        };
        let top = self.scroll_before.take();
        editor.update(cx, |editor, cx| {
            // Folds first: the scroll is in rows as they were folded.
            self.close_folds(editor);
            if let Some(top) = top {
                editor.set_scroll_top(top, cx);
            }
        });
    }
}

/// A row that isn't blank.
struct Line {
    /// Its zero-based buffer row.
    row: u32,
    /// Its text from its first char that isn't whitespace, trailing
    /// whitespace and its line break left out.
    text: String,
    /// Its first char that isn't whitespace, in bytes from the row's start.
    indent: usize,
}

/// The rows of `text` that aren't blank, of its first [`MAX_ROWS`], and
/// whether it has more rows than that.
fn lines(text: &Text) -> (Vec<Line>, bool) {
    let count = text.row_count();
    let mut lines = Vec::new();
    for row in 0..count.min(MAX_ROWS) {
        let start = text.row_start(row);
        let end = start + text.row_len(row) as usize;
        let mut indent = 0;
        let mut shown = String::new();
        let chars = text.chunks_in_range(start..end).flat_map(str::chars);
        for ch in chars {
            if shown.is_empty() && ch.is_whitespace() {
                indent += ch.len_utf8();
                continue;
            }
            shown.push(ch);
        }
        let kept = shown.trim_end().len();
        shown.truncate(kept);
        if shown.is_empty() {
            continue;
        }
        lines.push(Line {
            row,
            text: shown,
            indent,
        });
    }
    (lines, count > MAX_ROWS)
}

pub(crate) struct BufferLinesPicker {
    /// Records the caret before a pick jumps.
    workbench: WeakEntity<Workbench>,
    preview: RowPreview,
    lines: Vec<Line>,
    /// The buffer has more rows than are listed.
    capped: bool,
    query: String,
    /// The lines matching the query, with the byte ranges matched.
    matches: Vec<(usize, Vec<Range<usize>>)>,
    /// Focused before it opened, and again once it's dismissed.
    previous_focus: Option<FocusHandle>,
    _follows: Option<Subscription>,
    _released: Option<Subscription>,
}

impl BufferLinesPicker {
    fn new(
        workbench: WeakEntity<Workbench>,
        editor: Option<WeakEntity<EditorView>>,
        previous_focus: Option<FocusHandle>,
        cx: &App,
    ) -> Self {
        let listed = editor
            .as_ref()
            .and_then(WeakEntity::upgrade)
            .map(|editor| lines(editor.read(cx).model().buffer().text()));
        let (lines, capped) = listed.unwrap_or_default();
        let mut preview = RowPreview::new(editor);
        preview.opened_at(lines.first().map(|line| line.row));
        let matches = (0..lines.len()).map(|ix| (ix, Vec::new())).collect();
        Self {
            workbench,
            preview,
            lines,
            capped,
            query: String::new(),
            matches,
            previous_focus,
            _follows: None,
            _released: None,
        }
    }

    /// Previews the selected row from now on as the selection moves, and
    /// puts the preview back however the picker goes unconfirmed: the modal
    /// layer hides it without a dismissal when focus moves elsewhere, which
    /// stays where it went.
    fn follow(&mut self, cx: &mut Context<Picker<Self>>) {
        // A pick or a dismissal has already kept or restored it, leaving
        // nothing to restore.
        self._released = Some(cx.on_release(|picker, cx| picker.delegate.preview.restore(cx)));
        // `Picker::select` is the only way the selection moves, and it
        // notifies; the delegate has no hook for it.
        self._follows = Some(cx.observe_self(|picker, cx| {
            let selected = picker.selected();
            if let Some(&(line, _)) = picker.delegate.matches.get(selected) {
                let row = picker.delegate.lines[line].row;
                picker.delegate.preview.show(row, cx);
            }
        }));
    }

    fn refilter(&mut self, cx: &mut Context<Picker<Self>>) {
        let query: Vec<char> = self.query.chars().flat_map(char::to_lowercase).collect();
        self.matches = (self.lines.iter().enumerate())
            .filter_map(|(ix, line)| {
                let indices = subsequence(&query, &line.text)?;
                Some((ix, char_ranges(&line.text, &indices)))
            })
            .collect();
        let picker = cx.entity();
        // The picker is being updated now.
        cx.defer(move |cx| picker.update(cx, |picker, cx| picker.matches_updated(true, cx)));
    }
}

#[cfg(test)]
impl BufferLinesPicker {
    /// The matches: each one-based row, its text and the byte ranges of it
    /// highlighted.
    pub(crate) fn rows(&self) -> Vec<(u32, String, Vec<Range<usize>>)> {
        (self.matches.iter())
            .map(|(line, ranges)| {
                let line = &self.lines[*line];
                (line.row + 1, line.text.clone(), ranges.clone())
            })
            .collect()
    }
}

impl PickerDelegate for BufferLinesPicker {
    fn update_matches(&mut self, query: String, cx: &mut Context<Picker<Self>>) {
        self.query = query;
        self.refilter(cx);
    }

    fn match_count(&self) -> usize {
        self.matches.len()
    }

    fn match_id(&self, ix: usize) -> SharedString {
        self.lines[self.matches[ix].0].row.to_string().into()
    }

    fn rows_fill_width(&self) -> bool {
        true
    }

    fn render_match(&self, ix: usize, _: bool, cx: &App) -> AnyElement {
        let ui = fe_ui::ui_theme(cx);
        let (line, highlights) = &self.matches[ix];
        let line = &self.lines[*line];
        let (text, highlights) = excerpt(&line.text, highlights);
        let label = HighlightedLabel::new(text, highlights, &ui);
        div()
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .gap_3()
            .child(
                div()
                    .flex_none()
                    .min_w(gpui::px(40.))
                    .text_color(ui.muted_text)
                    .child((line.row + 1).to_string()),
            )
            .child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(label),
            )
            .into_any_element()
    }

    /// Puts the caret at the row's first char that isn't whitespace and
    /// focuses the editor.
    fn confirm(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Picker<Self>>) -> bool {
        let Some(&(line, _)) = self.matches.get(ix) else {
            return false;
        };
        // Its editor closed meanwhile: nothing to go to.
        let Some(editor) = self.preview.editor() else {
            return true;
        };
        let line = &self.lines[line];
        if let Some(workbench) = self.workbench.upgrade() {
            workbench.update(cx, |workbench, cx| {
                workbench.nav_state.goto.next();
                workbench.push_history(cx)
            });
        }
        let (row, indent) = (line.row, line.indent);
        editor.update(cx, |editor, cx| {
            self.preview.close_folds(editor);
            let buffer = editor.model().buffer();
            let text = buffer.text();
            let row = row.min(text.row_count() - 1);
            let start = text.row_start(row);
            let end = start + text.row_len(row) as usize;
            let at = text.clip_offset((start + indent).min(end), fe_text::Bias::Left);
            drop(buffer);
            editor.select_and_reveal(at..at, cx)
        });
        window.focus(&editor.focus_handle(cx), cx);
        self.preview.keep();
        self.previous_focus = None;
        true
    }

    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        self.preview.restore(cx);
        if let Some(focus) = self.previous_focus.take() {
            window.focus(&focus, cx);
        }
    }

    fn status(&self) -> Option<SharedString> {
        if self.preview.editor.is_none() {
            Some(tr!("workbench-navigation-buffer-lines-no-editor"))
        } else if self.lines.is_empty() {
            Some(tr!("workbench-navigation-buffer-lines-none"))
        } else if self.matches.is_empty() {
            Some(tr!("workbench-navigation-buffer-lines-no-matches"))
        } else if self.capped {
            Some(tr!(
                "workbench-navigation-buffer-lines-capped",
                count = MAX_ROWS,
                shown = group(MAX_ROWS)
            ))
        } else {
            None
        }
    }
}

/// What a row of `text` shows, with its `highlights` (byte ranges of
/// `text`) as byte ranges of what it shows: all of it, or for a row longer
/// than [`MAX_ROW_CHARS`] chars, that many from a little before its first
/// highlight, an ellipsis standing for what's cut off at either end.
fn excerpt(text: &str, highlights: &[Range<usize>]) -> (String, Vec<Range<usize>>) {
    const ELLIPSIS: &str = "…";
    let chars = text.chars().count();
    if chars <= MAX_ROW_CHARS {
        return (text.to_owned(), highlights.to_vec());
    }
    let first = highlights
        .first()
        .map_or(0, |range| text[..range.start].chars().count());
    let start = first
        .saturating_sub(EXCERPT_LEAD)
        .min(chars - MAX_ROW_CHARS);
    let byte = |char_ix: usize| {
        text.char_indices()
            .nth(char_ix)
            .map_or(text.len(), |(at, _)| at)
    };
    let (from, to) = (byte(start), byte(start + MAX_ROW_CHARS));
    let lead = if from > 0 { ELLIPSIS } else { "" };
    let trail = if to < text.len() { ELLIPSIS } else { "" };
    let shown = format!("{lead}{}{trail}", &text[from..to]);
    let shifted = (highlights.iter())
        .filter(|range| range.end > from && range.start < to)
        .map(|range| {
            let (start, end) = (range.start.max(from), range.end.min(to));
            start - from + lead.len()..end - from + lead.len()
        })
        .collect();
    (shown, shifted)
}

/// `n` with its thousands grouped by commas.
fn group(n: u32) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (ix, ch) in digits.chars().enumerate() {
        if ix > 0 && (digits.len() - ix).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

impl Workbench {
    /// The buffer-lines picker, while it shows.
    #[cfg(test)]
    pub(crate) fn buffer_lines_picker(
        &self,
        cx: &App,
    ) -> Option<Entity<Picker<BufferLinesPicker>>> {
        self.shown_palette(Palette::BufferLines, cx)
    }

    /// Shows the buffer-lines picker, focused, over the active editor's rows,
    /// in place of any other palette, or afresh if it shows.
    pub(crate) fn show_buffer_lines(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dismiss_palette(window, cx);
        let previous = window.focused(cx);
        let editor = self.active_editor(cx).map(Entity::downgrade);
        let delegate = BufferLinesPicker::new(cx.entity().downgrade(), editor, previous, cx);
        let placeholder = text!("workbench-navigation-buffer-lines-placeholder");
        let picker = cx.new(|cx| Picker::new(delegate, placeholder, fe_ui::ui_theme(cx), cx));
        picker.update(cx, |picker, cx| {
            picker.delegate.follow(cx);
            picker.show(window, cx);
        });
        self.show_palette(Palette::BufferLines, picker, window, cx);
    }
}

#[cfg(test)]
#[path = "buffer_lines_picker_tests.rs"]
mod buffer_lines_picker_tests;
