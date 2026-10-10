//! A read-only editor over a [`TextSource`], such as a mapped file too large
//! for a [`fe_buffer::Buffer`]: one selection, the editor's moves,
//! select-all and copy, and every edit refused.
//!
//! The text's offsets never shift, so the selection is plain offsets rather
//! than anchors. Commands are the editor's IDs (see [`crate::commands`]).

use std::{collections::HashSet, fmt, ops::Range, sync::Arc, sync::OnceLock};

use fe_buffer::{Bias, ResolvedSelection};
use fe_display::DisplayMap;
use fe_text::TextSource;
use serde::Deserialize;
use serde_json::Value;

use crate::{
    Movement,
    commands::{self, Motion, MoveArgs},
};

/// The most a copy reads: 64 MiB.
pub const COPY_LIMIT: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CopyError {
    /// The selection is `len` bytes, more than [`COPY_LIMIT`].
    TooLarge { len: usize },
}

impl fmt::Display for CopyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CopyError::TooLarge { len } => write!(
                f,
                "the selection is {len} bytes, more than the {COPY_LIMIT} a copy takes"
            ),
        }
    }
}

impl std::error::Error for CopyError {}

/// What [`ViewerModel::run`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The selection changed.
    Changed,
    /// The command ran and changed nothing, such as moving left at the start.
    Nothing,
    /// `editor.copy`: the text for the clipboard, or why there's none.
    Copy(Result<String, CopyError>),
    /// The command's args didn't parse.
    BadArgs(String),
    /// The command edits, or saves, the text.
    ReadOnly,
    /// Not a command a viewer runs.
    Unsupported,
}

/// One view over a read-only text: its display rows and a single selection.
pub struct ViewerModel {
    display: DisplayMap,
    tab_size: usize,
    anchor: usize,
    head: usize,
    /// Char column, within its display row, the head tries to keep while
    /// moving vertically.
    goal: Option<usize>,
    page_rows: u32,
}

impl ViewerModel {
    pub fn new(text: Arc<dyn TextSource>, tab_size: usize) -> ViewerModel {
        let tab_size = tab_size.max(1);
        ViewerModel {
            display: DisplayMap::read_only(text, tab_size),
            tab_size,
            anchor: 0,
            head: 0,
            goal: None,
            page_rows: 20,
        }
    }

    pub fn text(&self) -> &dyn TextSource {
        self.display.text()
    }

    pub fn display(&self) -> &DisplayMap {
        &self.display
    }

    pub fn tab_size(&self) -> usize {
        self.tab_size
    }

    /// How many rows `editor.page_up` and `editor.page_down` move; 20 until
    /// set.
    pub fn set_page_rows(&mut self, rows: u32) {
        self.page_rows = rows;
    }

    /// Moves on to `text`, a later snapshot of the same source, clamping the
    /// selection to it.
    pub fn refresh(&mut self, text: Arc<dyn TextSource>) {
        self.display.replace_text(text);
        let text = self.display.text();
        let clamp = |offset: usize| text.clip_offset(offset.min(text.len()), Bias::Left);
        (self.anchor, self.head) = (clamp(self.anchor), clamp(self.head));
    }

    /// The selected offsets, in order.
    pub fn selection(&self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }

    pub fn head(&self) -> usize {
        self.head
    }

    /// The selection as `layout_frame` takes it.
    pub fn resolved_selections(&self) -> Vec<ResolvedSelection> {
        vec![ResolvedSelection {
            range: self.selection(),
            reversed: self.head < self.anchor,
        }]
    }

    /// Selects from `anchor` to `head`, each clipped to a char boundary: for
    /// clicks and drags.
    pub fn set_selection(&mut self, anchor: usize, head: usize) {
        let text = self.text();
        let clip = |offset: usize| text.clip_offset(offset, Bias::Left);
        (self.anchor, self.head) = (clip(anchor), clip(head));
        self.goal = None;
    }

    pub fn select_all(&mut self) {
        (self.anchor, self.head) = (0, self.text().len());
        self.goal = None;
    }

    /// Moves the head as [`crate::EditorModel::move_cursors`] does, but by
    /// char rather than grapheme. Word moves aren't supported yet and leave
    /// the selection as it is.
    pub fn move_cursor(&mut self, movement: Movement, extend: bool) {
        if matches!(movement, Movement::WordLeft | Movement::WordRight) {
            return;
        }
        if !movement.is_vertical() {
            self.goal = None;
        }
        let collapsed = !extend && self.anchor != self.head;
        let head = match movement {
            Movement::Left if collapsed => self.selection().start,
            Movement::Right if collapsed => self.selection().end,
            _ => {
                let mut goal = self.goal;
                let head = self.moved(self.head, movement, &mut goal);
                self.goal = goal;
                head
            }
        };
        self.head = head;
        if !extend {
            self.anchor = head;
        }
    }

    /// The selected text, with `\n` line breaks. Nothing is read for a
    /// selection longer than [`COPY_LIMIT`].
    pub fn copy_text(&self) -> Result<String, CopyError> {
        let range = self.selection();
        if range.len() > COPY_LIMIT {
            return Err(CopyError::TooLarge { len: range.len() });
        }
        Ok(self.text().slice_to_string(range))
    }

    /// Runs the editor command `id`, with `args` (`Null` for none), if a
    /// viewer supports it: the moves but word moves, `editor.move`,
    /// `editor.select_all` and `editor.copy`. Commands that edit and
    /// `editor.save` are [`Outcome::ReadOnly`].
    pub fn run(&mut self, id: &str, args: &Value) -> Outcome {
        if id == "editor.save" || mutating_commands().contains(id) {
            return Outcome::ReadOnly;
        }
        let before = (self.anchor, self.head);
        match id {
            "editor.copy" => return Outcome::Copy(self.copy_text()),
            "editor.select_all" => self.select_all(),
            "editor.move" => {
                let args = match MoveArgs::deserialize(args) {
                    Ok(args) => args,
                    Err(error) => return Outcome::BadArgs(error.to_string()),
                };
                if matches!(args.motion, Motion::WordLeft | Motion::WordRight) {
                    return Outcome::Unsupported;
                }
                for _ in 0..args.count {
                    let before = (self.anchor, self.head);
                    self.move_cursor(args.motion.into(), args.extend);
                    // Moving again from where a move left nothing changed
                    // would change nothing either.
                    if (self.anchor, self.head) == before {
                        break;
                    }
                }
            }
            _ => match movement_command(id, self.page_rows) {
                Some((Movement::WordLeft | Movement::WordRight, _)) | None => {
                    return Outcome::Unsupported;
                }
                Some((movement, extend)) => self.move_cursor(movement, extend),
            },
        }
        if (self.anchor, self.head) == before {
            Outcome::Nothing
        } else {
            Outcome::Changed
        }
    }

    fn moved(&self, offset: usize, movement: Movement, goal: &mut Option<usize>) -> usize {
        let text = self.text();
        match movement {
            Movement::Left if offset == 0 => 0,
            Movement::Left => text.clip_offset(offset - 1, Bias::Left),
            Movement::Right if offset >= text.len() => text.len(),
            Movement::Right => text.clip_offset(offset + 1, Bias::Right),
            Movement::WordLeft | Movement::WordRight => offset,
            Movement::LineStart => {
                let row = text.offset_to_point(offset).row;
                let start = text.row_start(row);
                let end = start + text.row_len(row) as usize;
                let indent = start + indent_len(text, start..end);
                if offset == indent { start } else { indent }
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
        }
    }

    /// Moves `delta` display rows, keeping the goal column. A read-only map
    /// never soft wraps, so every display row ends its row.
    fn vertical(&self, offset: usize, delta: i64, goal: &mut Option<usize>) -> usize {
        let display = &self.display;
        let row = display.row_for_offset(offset);
        let column = *goal.get_or_insert_with(|| {
            let start = display.row(row).range.start;
            self.char_column(start..offset)
        });
        let target = row as i64 + delta;
        if target < 0 {
            return 0;
        }
        if target >= display.row_count() as i64 {
            return self.text().len();
        }
        self.offset_at_char_column(display.row(target as u32).range, column)
    }

    /// The char column `range`, from a row's start, reaches: chars count one
    /// column and tabs reach the next tab stop, as in
    /// [`fe_display::ExpandedRow::char_column`].
    fn char_column(&self, range: Range<usize>) -> usize {
        let mut column = 0;
        for chunk in self.text().chunks_in_range(range) {
            for ch in chunk.chars() {
                column += self.width(ch, column);
            }
        }
        column
    }

    /// The offset in `row` at char `column`, before a tab the column falls
    /// inside, or the row's end if it's shorter.
    fn offset_at_char_column(&self, row: Range<usize>, column: usize) -> usize {
        let (mut at, mut reached) = (row.start, 0);
        for chunk in self.text().chunks_in_range(row.clone()) {
            for ch in chunk.chars() {
                reached += self.width(ch, reached);
                if column < reached {
                    return at;
                }
                at += ch.len_utf8();
            }
        }
        row.end
    }

    fn width(&self, ch: char, column: usize) -> usize {
        if ch == '\t' {
            self.tab_size - column % self.tab_size
        } else {
            1
        }
    }
}

/// The bytes of leading whitespace in `range`, as `str::trim_start` counts
/// it.
fn indent_len(text: &dyn TextSource, range: Range<usize>) -> usize {
    let mut len = 0;
    for chunk in text.chunks_in_range(range) {
        let trimmed = chunk.trim_start();
        len += chunk.len() - trimmed.len();
        if !trimmed.is_empty() {
            break;
        }
    }
    len
}

/// The movement and whether it extends, for `editor.move_*`,
/// `editor.select_*`, `editor.page_*` and `editor.select_page_*`.
fn movement_command(id: &str, page_rows: u32) -> Option<(Movement, bool)> {
    let name = id.strip_prefix("editor.")?;
    let (extend, motion) = match name.strip_prefix("select_") {
        Some(motion) => (true, motion),
        None if name.starts_with("page_") => (false, name),
        None => (
            false,
            name.strip_prefix("move_")
                .filter(|motion| !motion.starts_with("page_"))?,
        ),
    };
    let movement = match motion {
        "left" => Movement::Left,
        "right" => Movement::Right,
        "up" => Movement::Up,
        "down" => Movement::Down,
        "word_left" => Movement::WordLeft,
        "word_right" => Movement::WordRight,
        "line_start" => Movement::LineStart,
        "line_end" => Movement::LineEnd,
        "document_start" => Movement::DocumentStart,
        "document_end" => Movement::DocumentEnd,
        "page_up" => Movement::PageUp(page_rows),
        "page_down" => Movement::PageDown(page_rows),
        _ => return None,
    };
    Some((movement, extend))
}

/// The IDs of the editor commands registered as mutating the buffer.
fn mutating_commands() -> &'static HashSet<String> {
    static IDS: OnceLock<HashSet<String>> = OnceLock::new();
    IDS.get_or_init(|| {
        commands::registry()
            .commands()
            .filter(|meta| meta.mutates_buffer)
            .map(|meta| meta.id.as_str().to_owned())
            .collect()
    })
}
