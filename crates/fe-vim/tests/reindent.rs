//! Vim's `=` re-indents rows as the host suggests for the buffer's
//! language (`EditorHost::suggest_indent`), as nvim's does with a
//! filetype's 'indentexpr': `==`, `={motion}`, visual `=`, a count, `.`
//! and `u`; a row the host has no suggestion for keeps its indentation.
//!
//! fe-vim is driven through its public API, as a host drives it: keys go
//! to `VimState::feed` in normal and visual mode.

use std::ops::{Range, RangeInclusive};

use fe_buffer::{Buffer, BufferSnapshot, Point};
use fe_command::{Invocation, Registry};
use fe_editor::{
    EditorModel, IndentStyle, IndentSuggestion,
    commands::{EditorHost, HeadlessHost},
};
use fe_vim::{Registers, Step, VimCommands, VimState, VimTarget, context, parse_keys};

/// A host answering fixed suggestions, as a C-like language would: a row
/// after one ending with `{` a level in from it, a row starting with `}` a
/// level out from the row before, and any other the row before's level;
/// row 0 has none. With `suggests` off it has none at all, as a buffer
/// without a language, and from row `declines_from` on it has none.
struct Braces {
    suggests: bool,
    declines_from: u32,
    host: HeadlessHost,
}

impl EditorHost for Braces {
    fn page_rows(&self) -> u32 {
        self.host.page_rows()
    }

    fn read_clipboard(&mut self) -> Option<String> {
        self.host.read_clipboard()
    }

    fn write_clipboard(&mut self, text: String) {
        self.host.write_clipboard(text);
    }

    fn fold_range_at(&self, row: u32) -> Option<RangeInclusive<u32>> {
        self.host.fold_range_at(row)
    }

    fn fold_ranges(&self, rows: Range<u32>) -> Vec<RangeInclusive<u32>> {
        self.host.fold_ranges(rows)
    }

    fn suggest_indent(&self, snapshot: &BufferSnapshot, row: u32) -> Option<IndentSuggestion> {
        if !self.suggests || row == 0 || row >= self.declines_from {
            return None;
        }
        let text = snapshot.text();
        let basis_row = row - 1;
        let delta = match () {
            _ if text.row_to_string(basis_row).trim_end().ends_with('{') => 1,
            _ if text.row_to_string(row).trim_start().starts_with('}') => -1,
            _ => 0,
        };
        Some(IndentSuggestion::Relative { basis_row, delta })
    }
}

/// An editor in vim mode on `text`, its caret at a row and byte column,
/// indenting four spaces a level.
struct Vim {
    model: EditorModel,
    host: Braces,
    state: VimState,
    registers: Registers,
    registry: Registry<VimCommands>,
}

impl Vim {
    fn new(text: &str, (row, column): (u32, u32), suggests: bool) -> Self {
        let mut model = EditorModel::new(Buffer::new(text), None);
        model.set_indent_style(Some(IndentStyle::Spaces(4)));
        let offset = model.text().point_to_offset(Point::new(row, column));
        model.set_cursor(offset);
        Self {
            model,
            host: Braces {
                suggests,
                declines_from: u32::MAX,
                host: HeadlessHost::new(),
            },
            state: VimState::new(),
            registers: Registers::new(),
            registry: fe_vim::registry(),
        }
    }

    /// Feeds `keys`, in vim notation.
    fn feed(&mut self, keys: &str) -> &mut Self {
        for key in parse_keys(keys).expect("valid keys") {
            match self.state.feed(key) {
                Step::Commit(invocation) => self.dispatch(invocation),
                Step::Wait(_) => {}
                step => panic!("{key:?} gave {step:?}"),
            }
        }
        self
    }

    fn dispatch(&mut self, invocation: Invocation) {
        let invocation = invocation.context(context(&self.state));
        let mut target = VimTarget {
            model: &mut self.model,
            host: &mut self.host,
            state: &mut self.state,
            registers: &mut self.registers,
        };
        self.registry.dispatch(&mut target, &invocation).unwrap();
    }

    fn text(&self) -> String {
        let text = self.model.text();
        text.slice_to_string(0..text.len())
    }

    /// The caret's row and byte column.
    fn cursor(&self) -> (u32, u32) {
        let point = (self.model.text()).offset_to_point(self.model.primary_selection().head());
        (point.row, point.column)
    }

    /// Mark `name`'s row and byte column.
    fn mark(&self, name: char) -> Option<(u32, usize)> {
        self.state.mark(name)?.get(self.model.snapshot())
    }
}

/// Two functions, mis-indented, a row of blanks between them.
const MISINDENTED: &str = "\
fn x() {
let a = 1;
      if a {
b();
        }
}

fn y() {
  z();
}";

/// [`MISINDENTED`] as the host indents it, the blank row emptied.
const INDENTED: &str = "\
fn x() {
    let a = 1;
    if a {
        b();
    }
}

fn y() {
    z();
}";

/// `text`'s rows, those in `rows` taken from `from` instead.
fn rows_from(text: &str, from: &str, rows: RangeInclusive<usize>) -> String {
    let from: Vec<&str> = from.split('\n').collect();
    text.split('\n')
        .enumerate()
        .map(|(row, line)| match rows.contains(&row) {
            true => from[row],
            false => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn reindent_operator_follows_the_language() {
    // `==` on a mis-indented row: the caret goes to its first non-blank.
    let one_off = rows_from(INDENTED, "\n\n\n  b();", 3..=3);
    let mut vim = Vim::new(&one_off, (3, 4), true);
    vim.feed("==");
    assert_eq!(vim.text(), INDENTED);
    assert_eq!(vim.cursor(), (3, 8));

    // `=ip`: the paragraph's rows, from the first.
    let mut vim = Vim::new(MISINDENTED, (2, 3), true);
    vim.feed("=ip");
    assert_eq!(vim.text(), rows_from(MISINDENTED, INDENTED, 0..=5));
    assert_eq!(vim.cursor(), (0, 0));

    // `=G`: every row, each from those above as just re-indented; the row
    // of blanks is emptied, and an empty row stays empty.
    let mut vim = Vim::new(MISINDENTED, (1, 0), true);
    vim.feed("=G");
    assert_eq!(vim.text(), INDENTED);
    assert_eq!(vim.cursor(), (1, 4));
    let mut vim = Vim::new(INDENTED, (0, 0), true);
    vim.feed("=G");
    assert_eq!(vim.text(), INDENTED);

    // `u` puts every row back in one step.
    let mut vim = Vim::new(MISINDENTED, (1, 0), true);
    vim.feed("=Gu");
    assert_eq!(vim.text(), MISINDENTED);

    // Visual `=`, linewise and charwise: the selection's rows.
    for keys in ["Vjj=", "vjj=", "<C-v>jj="] {
        let mut vim = Vim::new(MISINDENTED, (1, 0), true);
        vim.feed(keys);
        assert_eq!(
            vim.text(),
            rows_from(MISINDENTED, INDENTED, 1..=3),
            "{keys}"
        );
        assert_eq!(vim.cursor(), (1, 4), "{keys}");
        assert_eq!(vim.state.mode_name(), "normal", "{keys}");
    }

    // A count takes as many rows; `.` repeats over as many from the caret.
    let mut vim = Vim::new(MISINDENTED, (1, 0), true);
    vim.feed("3==");
    assert_eq!(vim.text(), rows_from(MISINDENTED, INDENTED, 1..=3));
    assert_eq!(vim.cursor(), (1, 4));
    vim.feed("jjj.");
    assert_eq!(vim.text(), rows_from(MISINDENTED, INDENTED, 1..=6));
    assert_eq!(vim.cursor(), (4, 4));
    // So does `.` after visual `=`.
    let mut vim = Vim::new(MISINDENTED, (1, 0), true);
    vim.feed("Vj=jj.");
    assert_eq!(vim.text(), rows_from(MISINDENTED, INDENTED, 1..=4));
    // Each `u` takes one back.
    vim.feed("u");
    assert_eq!(vim.text(), rows_from(MISINDENTED, INDENTED, 1..=2));
    vim.feed("u");
    assert_eq!(vim.text(), MISINDENTED);

    // A row the host has no suggestion for keeps its indentation; those
    // after it are indented from it.
    let mut vim = Vim::new(MISINDENTED, (0, 0), true);
    vim.host.declines_from = 8;
    vim.feed("=G");
    assert_eq!(vim.text(), rows_from(INDENTED, MISINDENTED, 8..=9));

    // Marks stay on their rows.
    let mut vim = Vim::new(MISINDENTED, (3, 2), true);
    vim.feed("majmbgg=G");
    assert_eq!(vim.mark('a').map(|(row, _)| row), Some(3));
    assert_eq!(vim.mark('b').map(|(row, _)| row), Some(4));
}

#[test]
fn reindent_indents_in_the_buffers_style() {
    let mut vim = Vim::new("a {\nb {\nc\n}\n}", (0, 0), true);
    vim.model.set_indent_style(Some(IndentStyle::Tabs));
    vim.feed("=G");
    assert_eq!(vim.text(), "a {\n\tb {\n\t\tc\n\t}\n}");
}

#[test]
fn reindent_without_suggestions_keeps_rows() {
    let mut vim = Vim::new(MISINDENTED, (1, 0), false);
    // A change to undo first: `=` adds no step of its own.
    vim.feed("x");
    let changed = vim.text();
    vim.feed("=ip");
    assert_eq!(vim.text(), changed);
    assert_eq!(vim.cursor(), (0, 0));
    vim.feed("u");
    assert_eq!(vim.text(), MISINDENTED);
}
