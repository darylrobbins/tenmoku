//! The headless editor: a buffer, its selections, and the commands that move,
//! select and edit. UI layers render it and translate input into these calls.
//! Must never depend on GPUI.

use std::{
    io,
    ops::{Range, RangeInclusive},
    path::{Path, PathBuf},
};

use fe_buffer::{
    Bias, Buffer, BufferEvent, BufferSnapshot, EditKind, Origin, ResolvedSelection, SelectionSet,
    Subscription, Text, TransactionId, UndoError,
};
use fe_display::{CharWidth, DisplayMap, ExpandedRow, Monospace, WRAP_SLICE_BYTES};
use unicode_segmentation::UnicodeSegmentation;

pub mod commands;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Movement {
    Left,
    Right,
    Up,
    Down,
    WordLeft,
    WordRight,
    /// First non-blank character, or column 0 if already there.
    LineStart,
    LineEnd,
    DocumentStart,
    DocumentEnd,
    PageUp(u32),
    PageDown(u32),
}

impl Movement {
    fn is_vertical(self) -> bool {
        matches!(
            self,
            Movement::Up | Movement::Down | Movement::PageUp(_) | Movement::PageDown(_)
        )
    }

    /// Whether a caret moved into folded rows should go past them, rather
    /// than back to their header.
    fn is_forward(self) -> bool {
        matches!(
            self,
            Movement::Right
                | Movement::WordRight
                | Movement::Down
                | Movement::PageDown(_)
                | Movement::DocumentEnd
        )
    }
}

pub struct EditorModel {
    buffer: Buffer,
    selections: SelectionSet,
    /// Char column, within its display row, each selection's head tries to
    /// keep while moving vertically.
    goals: Vec<Option<usize>>,
    tab_size: usize,
    path: Option<PathBuf>,
    events: Subscription,
    /// Display rows: soft wrap and folds. Synced after each edit.
    display: DisplayMap,
    widths: Box<dyn CharWidth>,
    soft_wrap: bool,
    /// The width to wrap at when soft wrap is on, in `widths`' unit.
    wrap_width: f32,
    /// Who edits and undoes: the invoking actor while a command runs (see
    /// [`EditorModel::run_as`]), the user otherwise.
    origin: Origin,
    /// The transactions the running command produced; `None` outside one.
    transactions: Option<Vec<TransactionId>>,
}

impl EditorModel {
    pub fn new(mut buffer: Buffer, path: Option<PathBuf>) -> Self {
        let events = buffer.subscribe();
        let selections = SelectionSet::caret(buffer.current(), 0);
        let display = DisplayMap::new(buffer.current(), 4);
        Self {
            buffer,
            selections,
            goals: Vec::new(),
            tab_size: 4,
            path,
            events,
            display,
            widths: Box::new(Monospace),
            soft_wrap: false,
            wrap_width: 80.,
            origin: Origin::User,
            transactions: None,
        }
    }

    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref();
        let file = std::fs::File::open(path)?;
        let buffer = Buffer::from_reader(io::BufReader::with_capacity(256 * 1024, file))?;
        Ok(Self::new(buffer, Some(path.to_path_buf())))
    }

    /// Saves atomically: writes a new file beside the target and renames it
    /// over the original, so a failed save never truncates the file.
    ///
    /// The new file gets a random name and is created exclusively with
    /// owner-only permissions, so it never follows a planted symlink and is
    /// never readable by others mid-save. The original's permissions are then
    /// applied through the open handle. If the path is a symlink, the file it
    /// points to is updated and the link is left in place.
    pub fn save(&mut self) -> io::Result<()> {
        let path = self
            .path
            .clone()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "buffer has no path"))?;
        let (target, permissions) = match std::fs::canonicalize(&path) {
            Ok(target) => {
                let permissions = std::fs::metadata(&target)?.permissions();
                (target, Some(permissions))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => (path, None),
            Err(error) => return Err(error),
        };
        let dir = match target.parent() {
            Some(dir) if !dir.as_os_str().is_empty() => dir,
            _ => Path::new("."),
        };
        let file_name = target.file_name().unwrap_or_default().to_string_lossy();
        let temp = tempfile::Builder::new()
            .prefix(&format!(".{file_name}."))
            .suffix(".fastedit-save")
            .tempfile_in(dir)?;
        if let Some(permissions) = permissions {
            temp.as_file().set_permissions(permissions)?;
        }
        let mut out = io::BufWriter::new(temp);
        self.buffer.write_to(&mut out)?;
        let temp = out.into_inner().map_err(|error| error.into_error())?;
        temp.as_file().sync_all()?;
        // Dropping `temp` on any error above deletes it.
        temp.persist(&target).map_err(|error| error.error)?;
        self.buffer.did_save();
        Ok(())
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    pub fn buffer_mut(&mut self) -> &mut Buffer {
        &mut self.buffer
    }

    pub fn snapshot(&self) -> &BufferSnapshot {
        self.buffer.current()
    }

    pub fn text(&self) -> &Text {
        self.buffer.text()
    }

    pub fn tab_size(&self) -> usize {
        self.tab_size
    }

    pub fn set_tab_size(&mut self, tab_size: usize) {
        self.tab_size = tab_size.max(1);
        self.display.set_tab_size(self.tab_size, &mut *self.widths);
        self.wrap_near_cursor();
    }

    /// The display rows, as of the last [`EditorModel::sync_display`].
    /// The model syncs after its own edits; call that after editing
    /// through [`EditorModel::buffer_mut`].
    pub fn display(&self) -> &DisplayMap {
        &self.display
    }

    /// Brings the display rows up to date with the buffer. Wrapping rows
    /// still pending is [`EditorModel::wrap_pending`]'s job.
    pub fn sync_display(&mut self) {
        self.display.sync(self.buffer.current(), &mut *self.widths);
    }

    /// Measures chars for soft wrap; [`Monospace`] until set.
    pub fn set_char_widths(&mut self, widths: Box<dyn CharWidth>) {
        self.widths = widths;
        self.sync_display();
        self.display.rewrap(&mut *self.widths);
        self.wrap_near_cursor();
    }

    pub fn soft_wrap(&self) -> bool {
        self.soft_wrap
    }

    pub fn set_soft_wrap(&mut self, soft_wrap: bool) {
        self.soft_wrap = soft_wrap;
        self.apply_wrap();
    }

    pub fn toggle_soft_wrap(&mut self) {
        self.set_soft_wrap(!self.soft_wrap);
    }

    /// Where soft wrap wraps when it's on, in the unit of the char widths.
    pub fn set_wrap_width(&mut self, width: f32) {
        if self.fit_wrap_width(width) {
            self.wrap_near_cursor();
        }
    }

    /// Like [`EditorModel::set_wrap_width`], but wraps nothing: for the frame
    /// loop, which wraps pending rows a slice at a time around the
    /// viewport. Returns whether the wrap width changed.
    pub fn fit_wrap_width(&mut self, width: f32) -> bool {
        self.wrap_width = width;
        self.invalidate_wrap()
    }

    fn apply_wrap(&mut self) {
        if self.invalidate_wrap() {
            self.wrap_near_cursor();
        }
    }

    /// Points the display at the wrap width in force, marking rows pending
    /// if it changed. Returns whether it did.
    fn invalidate_wrap(&mut self) -> bool {
        self.sync_display();
        let width = self.soft_wrap.then_some(self.wrap_width);
        let changed = width != self.display.wrap_width();
        self.display.set_wrap_width(width, &mut *self.widths);
        changed
    }

    /// Wraps one slice of pending rows around the primary cursor's buffer
    /// row, so a small file is wrapped by the time a command returns and a
    /// large one has the rows the cursor is on.
    fn wrap_near_cursor(&mut self) {
        let row = self
            .text()
            .offset_to_point(self.primary_selection().head())
            .row;
        self.wrap_pending(row);
    }

    /// Wraps one [`WRAP_SLICE_BYTES`] slice of pending rows, those from
    /// buffer row `around_row` onward first. Returns whether rows are still
    /// pending; the frame loop calls this until they're not.
    pub fn wrap_pending(&mut self, around_row: u32) -> bool {
        self.display
            .wrap_pending(around_row, WRAP_SLICE_BYTES, &mut *self.widths)
    }

    /// [`EditorModel::wrap_pending`] for a viewport whose top is `top`, in
    /// display rows. Wraps around the top row's buffer row and returns
    /// whether rows are still pending, and the top that keeps the top
    /// row's first char at the same height: on a row that is now wrapped
    /// differently, the row it moved into.
    pub fn wrap_pending_at(&mut self, top: f32) -> (bool, f32) {
        let top = top.max(0.);
        let row = (top as u32).min(self.display.row_count().saturating_sub(1));
        let anchor = self.display.row(row);
        let pending = self.wrap_pending(anchor.buffer_row);
        let row_now = self.display.row_for_offset(anchor.range.start);
        (pending, row_now as f32 + (top - row as f32))
    }

    /// Folds the rows each selection spans or, for a caret, the fold
    /// `candidate_at` gives for its row, else the indented block it heads or
    /// is in. Carets folded away move to the fold's header.
    pub fn fold(&mut self, candidate_at: &dyn Fn(u32) -> Option<RangeInclusive<u32>>) {
        self.sync_display();
        let text = self.text().clone();
        for selection in self.selections() {
            let start = text.offset_to_point(selection.range.start);
            let end = text.offset_to_point(selection.range.end);
            let last = if end.row > start.row && end.column == 0 {
                end.row - 1
            } else {
                end.row
            };
            let rows = if last > start.row {
                Some(start.row..=last)
            } else {
                candidate_at(start.row).or_else(|| indented_block(&text, start.row, self.tab_size))
            };
            if let Some(rows) = rows {
                self.display.fold(rows);
            }
        }
        self.move_folded_carets();
    }

    /// Folds every one of `candidates`, or with none, every indented block.
    /// Carets folded away move to the fold's header.
    pub fn fold_all(&mut self, candidates: impl IntoIterator<Item = RangeInclusive<u32>>) {
        self.sync_display();
        let mut candidates = candidates.into_iter().peekable();
        if candidates.peek().is_some() {
            self.display.fold_many(candidates);
        } else {
            let blocks = indented_blocks(self.text(), self.tab_size);
            self.display.fold_many(blocks);
        }
        self.move_folded_carets();
    }

    /// Folds `rows` behind their first. Carets folded away move to it.
    pub fn fold_rows(&mut self, rows: RangeInclusive<u32>) {
        self.sync_display();
        self.display.fold(rows);
        self.move_folded_carets();
    }

    /// Unfolds the folds whose header or hidden rows overlap `rows`.
    pub fn unfold_rows(&mut self, rows: RangeInclusive<u32>) {
        self.sync_display();
        self.display.unfold(rows);
    }

    /// Unfolds the folds on the rows of each selection.
    pub fn unfold(&mut self) {
        for selection in self.selections() {
            let start = self.text().offset_to_point(selection.range.start).row;
            let end = self.text().offset_to_point(selection.range.end).row;
            self.unfold_rows(start..=end);
        }
    }

    pub fn unfold_all(&mut self) {
        self.sync_display();
        self.display.unfold_all();
    }

    /// Moves the carets in folded rows to their fold's header.
    fn move_folded_carets(&mut self) {
        let selections: Vec<_> = self
            .selections()
            .into_iter()
            .map(|selection| {
                let head = self.skip_folds(selection.head(), false);
                if head == selection.head() {
                    selection
                } else {
                    caret(head)
                }
            })
            .collect();
        self.set_selections(selections);
    }

    /// Moves an offset in folded rows out of them: past them going
    /// `forward` (unless they run to the end), otherwise to the end of
    /// their header.
    fn skip_folds(&self, offset: usize, forward: bool) -> usize {
        let text = self.text();
        let row = text.offset_to_point(offset).row;
        let Some(hidden) = self.display.hidden_rows(row) else {
            return offset;
        };
        if forward && hidden.end < text.row_count() {
            return text.row_start(hidden.end);
        }
        let header = hidden.start - 1;
        text.row_start(header) + text.row_len(header) as usize
    }

    pub fn is_dirty(&self) -> bool {
        self.buffer.is_dirty()
    }

    /// The selections, sorted by position.
    pub fn selections(&self) -> Vec<ResolvedSelection> {
        self.selections.resolve(self.buffer.current())
    }

    /// The selection that scrolling follows: the last one.
    pub fn primary_selection(&self) -> ResolvedSelection {
        self.selections().pop().unwrap_or(ResolvedSelection {
            range: 0..0,
            reversed: false,
        })
    }

    pub fn expanded_row(&self, row: u32) -> ExpandedRow {
        ExpandedRow::new(&self.text().row_to_string(row), self.tab_size)
    }

    /// Replaces all selections. They're sorted and overlapping ones merged.
    pub fn set_selections(&mut self, selections: impl IntoIterator<Item = ResolvedSelection>) {
        self.goals.clear();
        self.selections = SelectionSet::from_resolved(self.buffer.current(), selections);
    }

    /// Places a single caret.
    pub fn set_cursor(&mut self, offset: usize) {
        let offset = self.text().clip_offset(offset, Bias::Left);
        self.goals.clear();
        self.set_selections([caret(offset)]);
    }

    /// Selects `range`; `reversed` puts the head at its start.
    pub fn select_range(&mut self, range: Range<usize>, reversed: bool) {
        let text = self.text();
        let range =
            text.clip_offset(range.start, Bias::Left)..text.clip_offset(range.end, Bias::Right);
        self.goals.clear();
        self.set_selections([ResolvedSelection { range, reversed }]);
    }

    /// Moves the primary selection's head to `offset`, keeping its tail; other
    /// selections are dropped. Used for shift-click and drag.
    pub fn extend_primary_to(&mut self, offset: usize) {
        let offset = self.text().clip_offset(offset, Bias::Left);
        let tail = self.primary_selection().tail();
        self.goals.clear();
        self.set_selections([selection_from(tail, offset)]);
    }

    pub fn select_all(&mut self) {
        let len = self.text().len();
        self.select_range(0..len, false);
    }

    /// The word (or run of punctuation or whitespace) around `offset`.
    pub fn word_range_at(&self, offset: usize) -> Range<usize> {
        let (_, row_start, row_text) = self.row_at(offset);
        let col = offset - row_start;
        let class_at = |ix: usize| row_text[ix..].chars().next().map(CharClass::of);
        // A click just past a word (on the space after it, or at the row's
        // end) selects the word rather than the gap.
        let mut col = col;
        let prev = prev_char_start(&row_text, col);
        let prev_class = prev.and_then(class_at);
        let class = match (class_at(col), prev_class) {
            (None | Some(CharClass::Whitespace), Some(prev_class))
                if prev_class != CharClass::Whitespace =>
            {
                col = prev.expect("has a previous char");
                prev_class
            }
            (Some(class), _) => class,
            (None, Some(prev_class)) => prev_class,
            (None, None) => return offset..offset,
        };
        let mut start = col;
        while let Some(prev) = prev_char_start(&row_text, start) {
            if class_at(prev) != Some(class) {
                break;
            }
            start = prev;
        }
        let mut end = col;
        while let Some(ch) = row_text[end..].chars().next() {
            if CharClass::of(ch) != class {
                break;
            }
            end += ch.len_utf8();
        }
        row_start + start..row_start + end
    }

    /// The whole row around `offset`, including its line break.
    pub fn line_range_at(&self, offset: usize) -> Range<usize> {
        let text = self.text();
        let row = text.offset_to_point(offset).row;
        let start = text.row_start(row);
        let end = if row + 1 < text.row_count() {
            text.row_start(row + 1)
        } else {
            text.len()
        };
        start..end
    }

    pub fn move_cursors(&mut self, movement: Movement, extend: bool) {
        self.sync_display();
        let selections = self.selections();
        if !movement.is_vertical() || self.goals.len() != selections.len() {
            self.goals = vec![None; selections.len()];
        }
        let mut goals = std::mem::take(&mut self.goals);
        let mut moved = Vec::with_capacity(selections.len());
        for (selection, goal) in selections.iter().zip(goals.iter_mut()) {
            let (tail, head) = (selection.tail(), selection.head());
            let collapsed = !extend && !selection.range.is_empty();
            let new_head = match movement {
                Movement::Left if collapsed => selection.range.start,
                Movement::Right if collapsed => selection.range.end,
                _ => self.moved(head, movement, goal),
            };
            moved.push(if extend {
                selection_from(tail, new_head)
            } else {
                caret(new_head)
            });
        }
        self.set_selections(moved);
        // Merging can change the count; goals only survive if it didn't.
        self.goals = if goals.len() == self.selections.len() {
            goals
        } else {
            Vec::new()
        };
    }

    fn moved(&self, offset: usize, movement: Movement, goal: &mut Option<usize>) -> usize {
        let text = self.text();
        let moved = match movement {
            Movement::Left => self.prev_grapheme(offset),
            Movement::Right => self.next_grapheme(offset),
            Movement::WordLeft => self.prev_word(offset),
            Movement::WordRight => self.next_word(offset),
            Movement::LineStart => {
                let (_, row_start, row_text) = self.row_at(offset);
                let indent = row_text.len() - row_text.trim_start().len();
                if offset == row_start + indent {
                    row_start
                } else {
                    row_start + indent
                }
            }
            Movement::LineEnd => {
                let row = text.offset_to_point(offset).row;
                text.row_start(row) + text.row_len(row) as usize
            }
            Movement::DocumentStart => 0,
            Movement::DocumentEnd => text.len(),
            Movement::Up => self.vertical(offset, -1, goal),
            Movement::Down => self.vertical(offset, 1, goal),
            Movement::PageUp(rows) => self.vertical(offset, -(rows.max(1) as i64), goal),
            Movement::PageDown(rows) => self.vertical(offset, rows.max(1) as i64, goal),
        };
        self.skip_folds(moved, movement.is_forward())
    }

    /// Moves `delta` display rows, keeping the goal column.
    fn vertical(&self, offset: usize, delta: i64, goal: &mut Option<usize>) -> usize {
        let display = &self.display;
        let row = display.row_for_offset(offset);
        let column = *goal.get_or_insert_with(|| {
            let range = display.row(row).range;
            let expanded = self.expanded_range(range.clone());
            expanded.char_column(expanded.to_display(offset.saturating_sub(range.start)))
        });
        let target = row as i64 + delta;
        if target < 0 {
            return 0;
        }
        if target >= display.row_count() as i64 {
            return self.text().len();
        }
        let target = display.row(target as u32);
        let expanded = self.expanded_range(target.range.clone());
        let column = expanded.to_buffer(expanded.display_col_for_char_column(column), Bias::Left);
        let offset = target.range.start + column;
        if !target.last && offset >= target.range.end {
            // The end of a soft-wrapped row is the start of the next one.
            let before_end = self.prev_grapheme(target.range.end);
            return before_end.max(target.range.start);
        }
        offset
    }

    fn expanded_range(&self, range: Range<usize>) -> ExpandedRow {
        ExpandedRow::new(&self.text().slice_to_string(range), self.tab_size)
    }

    /// Row index, start offset and text of the row containing `offset`.
    fn row_at(&self, offset: usize) -> (u32, usize, String) {
        let text = self.text();
        let row = text.offset_to_point(offset).row;
        (row, text.row_start(row), text.row_to_string(row))
    }

    fn prev_grapheme(&self, offset: usize) -> usize {
        let (_, row_start, row_text) = self.row_at(offset);
        let col = offset - row_start;
        if col == 0 {
            return offset.saturating_sub(1);
        }
        let prev = row_text
            .grapheme_indices(true)
            .map(|(ix, _)| ix)
            .take_while(|&ix| ix < col)
            .last()
            .unwrap_or(0);
        row_start + prev
    }

    fn next_grapheme(&self, offset: usize) -> usize {
        let (_, row_start, row_text) = self.row_at(offset);
        let col = offset - row_start;
        if col >= row_text.len() {
            return (offset + 1).min(self.text().len());
        }
        let next = row_text
            .grapheme_indices(true)
            .map(|(ix, _)| ix)
            .find(|&ix| ix > col)
            .unwrap_or(row_text.len());
        row_start + next
    }

    fn next_word(&self, offset: usize) -> usize {
        let (_, row_start, row_text) = self.row_at(offset);
        let mut col = offset - row_start;
        if col >= row_text.len() {
            return (offset + 1).min(self.text().len());
        }
        let mut chars = row_text[col..].chars().peekable();
        while let Some(&ch) = chars.peek()
            && ch.is_whitespace()
        {
            col += ch.len_utf8();
            chars.next();
        }
        if let Some(&first) = chars.peek() {
            let class = CharClass::of(first);
            while let Some(&ch) = chars.peek()
                && CharClass::of(ch) == class
            {
                col += ch.len_utf8();
                chars.next();
            }
        }
        row_start + col
    }

    fn prev_word(&self, offset: usize) -> usize {
        let (_, row_start, row_text) = self.row_at(offset);
        let mut col = offset - row_start;
        if col == 0 {
            return offset.saturating_sub(1);
        }
        let mut chars = row_text[..col].chars().rev().peekable();
        while let Some(&ch) = chars.peek()
            && ch.is_whitespace()
        {
            col -= ch.len_utf8();
            chars.next();
        }
        if let Some(&first) = chars.peek() {
            let class = CharClass::of(first);
            while let Some(&ch) = chars.peek()
                && CharClass::of(ch) == class
            {
                col -= ch.len_utf8();
                chars.next();
            }
        }
        row_start + col
    }

    /// Replaces every selection with `text` and leaves a caret after each.
    pub fn insert(&mut self, text: &str) {
        let selections = self.selections();
        let edits = selections
            .iter()
            .map(|selection| (selection.range.clone(), text.to_string()));
        self.apply_with_carets(edits.collect());
    }

    /// Like [`EditorModel::insert`], but if `text` has one line per selection,
    /// each selection gets its own line.
    pub fn paste(&mut self, text: &str) {
        let selections = self.selections();
        let lines: Vec<&str> = text.lines().collect();
        if selections.len() > 1 && lines.len() == selections.len() {
            let edits = selections
                .iter()
                .zip(lines)
                .map(|(selection, line)| (selection.range.clone(), line.to_string()))
                .collect();
            self.apply_with_carets(edits);
        } else {
            self.insert(text);
        }
    }

    /// Inserts a line break, copying the current row's indentation.
    pub fn newline(&mut self) {
        let edits = self
            .selections()
            .into_iter()
            .map(|selection| {
                let (_, row_start, row_text) = self.row_at(selection.range.start);
                let indent_len = row_text.len() - row_text.trim_start().len();
                let indent_len = indent_len.min(selection.range.start - row_start);
                (selection.range, format!("\n{}", &row_text[..indent_len]))
            })
            .collect();
        self.apply_with_carets(edits);
    }

    /// Inserts spaces up to the next tab stop at each caret.
    pub fn tab(&mut self) {
        let edits = self
            .selections()
            .into_iter()
            .map(|selection| {
                let row = self.text().offset_to_point(selection.range.start).row;
                let expanded = self.expanded_row(row);
                let col = selection.range.start - self.text().row_start(row);
                let column = expanded.char_column(expanded.to_display(col));
                let width = self.tab_size - column % self.tab_size;
                (selection.range, " ".repeat(width))
            })
            .collect();
        self.apply_with_carets(edits);
    }

    pub fn backspace(&mut self) {
        self.delete_with(|model, offset| model.prev_grapheme(offset)..offset);
    }

    pub fn delete_forward(&mut self) {
        self.delete_with(|model, offset| offset..model.next_grapheme(offset));
    }

    pub fn delete_word_left(&mut self) {
        self.delete_with(|model, offset| model.prev_word(offset)..offset);
    }

    /// Deletes each selection, or for carets the range `around` gives.
    fn delete_with(&mut self, around: impl Fn(&Self, usize) -> Range<usize>) {
        let edits = self
            .selections()
            .into_iter()
            .map(|selection| {
                let range = if selection.range.is_empty() {
                    around(self, selection.range.start)
                } else {
                    selection.range
                };
                (range, String::new())
            })
            .collect();
        self.apply_with_carets(edits);
    }

    /// The text to copy: each selection, one per line. With only carets, the
    /// caret rows (including line breaks) are copied instead; the flag says so.
    pub fn copy_text(&self) -> (String, bool) {
        let selections = self.selections();
        let text = self.text();
        if selections
            .iter()
            .all(|selection| selection.range.is_empty())
        {
            let mut rows = String::new();
            for selection in &selections {
                rows.push_str(&text.slice_to_string(self.line_range_at(selection.range.start)));
            }
            if !rows.ends_with('\n') {
                rows.push('\n');
            }
            return (rows, true);
        }
        let parts: Vec<String> = selections
            .iter()
            .filter(|selection| !selection.range.is_empty())
            .map(|selection| text.slice_to_string(selection.range.clone()))
            .collect();
        (parts.join("\n"), false)
    }

    /// Copies, then deletes the selections (or caret rows).
    pub fn cut(&mut self) -> String {
        let (copied, whole_rows) = self.copy_text();
        if whole_rows {
            let edits = self
                .selections()
                .into_iter()
                .map(|selection| (self.line_range_at(selection.range.start), String::new()))
                .collect();
            self.apply_with_carets(edits);
        } else {
            self.delete_with(|_, offset| offset..offset);
        }
        copied
    }

    pub fn undo(&mut self) -> Result<Option<TransactionId>, UndoError> {
        self.events.drain();
        let result = self.buffer.undo(|origin| *origin == self.origin);
        self.place_carets_after_history_change(EditKind::Undo);
        self.produced(result)
    }

    pub fn redo(&mut self) -> Result<Option<TransactionId>, UndoError> {
        self.events.drain();
        let result = self.buffer.redo(|origin| *origin == self.origin);
        self.place_carets_after_history_change(EditKind::Redo);
        self.produced(result)
    }

    /// Runs `f` with edits, undo and redo acting as `origin` rather than
    /// the user. Returns its result and the transactions it produced: those
    /// it edited in, and those it undid or redid.
    pub(crate) fn run_as<R>(
        &mut self,
        origin: Origin,
        f: impl FnOnce(&mut Self) -> R,
    ) -> (R, Vec<TransactionId>) {
        let outer = std::mem::replace(&mut self.origin, origin);
        let outer_transactions = self.transactions.replace(Vec::new());
        let result = f(self);
        self.origin = outer;
        let transactions = std::mem::replace(&mut self.transactions, outer_transactions);
        (result, transactions.unwrap_or_default())
    }

    /// Notes an undo's or redo's transaction for [`EditorModel::run_as`].
    fn produced(
        &mut self,
        result: Result<Option<TransactionId>, UndoError>,
    ) -> Result<Option<TransactionId>, UndoError> {
        if let (Ok(Some(id)), Some(transactions)) = (&result, &mut self.transactions) {
            transactions.push(*id);
        }
        result
    }

    fn place_carets_after_history_change(&mut self, kind: EditKind) {
        self.sync_display();
        let last = self
            .events
            .drain()
            .into_iter()
            .rev()
            .find_map(|event| match event {
                BufferEvent::Edited {
                    kind: event_kind,
                    edits,
                    ..
                } if event_kind == kind => Some(edits),
                _ => None,
            });
        if let Some(edits) = last {
            self.goals.clear();
            let carets: Vec<_> = edits.iter().map(|edit| caret(edit.new.end)).collect();
            self.set_selections(carets);
        }
    }

    /// Applies one edit per selection (as a single transaction, from the
    /// user unless a command runs as someone else) and puts
    /// a caret at the end of each edit's new text.
    fn apply_with_carets(&mut self, mut edits: Vec<(Range<usize>, String)>) {
        edits.sort_by_key(|(range, _)| range.start);
        // Overlapping ranges (e.g. two carets deleting the same char) merge.
        let mut merged: Vec<(Range<usize>, String)> = Vec::with_capacity(edits.len());
        for (range, text) in edits {
            match merged.last_mut() {
                Some((last, last_text)) if range.start < last.end => {
                    last.end = last.end.max(range.end);
                    last_text.push_str(&text);
                }
                _ => merged.push((range, text)),
            }
        }
        let mut delta = 0isize;
        let mut carets = Vec::with_capacity(merged.len());
        for (range, text) in &merged {
            let start = range.start.checked_add_signed(delta).expect("sorted edits");
            carets.push(caret(start + text.len()));
            delta += text.len() as isize - range.len() as isize;
        }
        self.events.drain();
        let id = self.buffer.edit(merged, self.origin.clone());
        if let (Some(id), Some(transactions)) = (id, &mut self.transactions) {
            transactions.push(id);
        }
        self.events.drain();
        self.sync_display();
        self.goals.clear();
        self.set_selections(carets);
    }
}

fn caret(offset: usize) -> ResolvedSelection {
    ResolvedSelection {
        range: offset..offset,
        reversed: false,
    }
}

fn selection_from(tail: usize, head: usize) -> ResolvedSelection {
    if head < tail {
        ResolvedSelection {
            range: head..tail,
            reversed: true,
        }
    } else {
        ResolvedSelection {
            range: tail..head,
            reversed: false,
        }
    }
}

/// The column `row`'s text starts at, or `None` for a blank row.
fn indent(text: &Text, row: u32, tab_size: usize) -> Option<usize> {
    let mut column = 0;
    for ch in text.row_to_string(row).chars() {
        match ch {
            ' ' => column += 1,
            '\t' => column += tab_size - column % tab_size,
            ch if ch.is_whitespace() => {}
            _ => return Some(column),
        }
    }
    None
}

/// The rows of the block indented under `row`, with `row` as its header: or
/// if `row` heads none, the block it's in. Blank rows inside belong to the
/// block; trailing ones don't.
fn indented_block(text: &Text, row: u32, tab_size: usize) -> Option<RangeInclusive<u32>> {
    let indent = |row: u32| indent(text, row, tab_size);
    let block_under = |header: u32| {
        let level = indent(header)?;
        let mut last = header;
        for row in header + 1..text.row_count() {
            match indent(row) {
                Some(indent) if indent <= level => break,
                Some(_) => last = row,
                None => {}
            }
        }
        (last > header).then_some(header..=last)
    };
    if let Some(block) = block_under(row) {
        return Some(block);
    }
    let level = indent(row)?;
    let header = (0..row)
        .rev()
        .find(|&above| indent(above).is_some_and(|indent| indent < level))?;
    block_under(header)
}

/// Every block [`indented_block`] finds under a row, in one pass.
fn indented_blocks(text: &Text, tab_size: usize) -> Vec<RangeInclusive<u32>> {
    let mut blocks = Vec::new();
    // The rows heading the blocks open so far, with their indents, outermost
    // first.
    let mut open: Vec<(u32, usize)> = Vec::new();
    let mut last_text_row = 0;
    let mut close = |open: &mut Vec<(u32, usize)>, level: usize, last_text_row: u32| {
        while let Some(&(header, _)) = open.last().filter(|(_, indent)| *indent >= level) {
            open.pop();
            if last_text_row > header {
                blocks.push(header..=last_text_row);
            }
        }
    };
    for row in 0..text.row_count() {
        let Some(level) = indent(text, row, tab_size) else {
            continue;
        };
        close(&mut open, level, last_text_row);
        open.push((row, level));
        last_text_row = row;
    }
    close(&mut open, 0, last_text_row);
    blocks
}

fn prev_char_start(text: &str, col: usize) -> Option<usize> {
    text[..col].char_indices().next_back().map(|(ix, _)| ix)
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum CharClass {
    Whitespace,
    Word,
    Punctuation,
}

impl CharClass {
    fn of(ch: char) -> Self {
        if ch.is_whitespace() {
            CharClass::Whitespace
        } else if ch.is_alphanumeric() || ch == '_' {
            CharClass::Word
        } else {
            CharClass::Punctuation
        }
    }
}

#[cfg(test)]
mod tests;
