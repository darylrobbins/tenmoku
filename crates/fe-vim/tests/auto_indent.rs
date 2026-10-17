//! Vim's `o`, `O`, `cc`, `S` and insert-mode `Enter` indent new and emptied
//! rows as the host suggests for the buffer's language
//! (`EditorHost::suggest_indent`), as nvim's do with a filetype's
//! 'indentexpr', and copy as before where it has no suggestion.
//!
//! fe-vim is driven through its public API, as a host drives it: keys go
//! to `VimState::feed` in normal mode; in insert mode a char is typed
//! through the editor's `editor.insert`, `<CR>` goes to
//! `VimState::line_break_in` and `<Esc>` dispatches `vim.normal_mode`.

use std::ops::{Range, RangeInclusive};

use fe_buffer::{Buffer, BufferSnapshot, Point};
use fe_command::{Invocation, Registry};
use fe_editor::{
    EditorModel, IndentStyle, IndentSuggestion,
    commands::{self, Editor, EditorHost, EditorTarget, HeadlessHost},
};
use fe_vim::{
    Mode, Registers, Step, VimCommands, VimKey, VimState, VimTarget, context, parse_keys,
};
use serde_json::json;

/// A host answering fixed suggestions, as a C-like language would: a row
/// after one ending with `{` a level in from it, a row starting with `}` a
/// level out from the row before, and any other the row before's level;
/// a `}` starting a row lines it up with its `{`'s. With `suggests` off it
/// has none, as a buffer without a language, and from row `declines_from`
/// on it has no suggestion. It pairs `{` with `}` only with `pairs`, as
/// vim doesn't.
struct Braces {
    suggests: bool,
    pairs: bool,
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

    fn auto_pairs(&self) -> &[(char, char)] {
        match self.pairs {
            true => &[('{', '}')],
            false => &[],
        }
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

    fn closer_outdents(&self, snapshot: &BufferSnapshot, row: u32) -> Option<u32> {
        let text = snapshot.text();
        if !self.suggests || !text.row_to_string(row).trim_start().starts_with('}') {
            return None;
        }
        let mut depth = 0;
        for above in (0..row).rev() {
            let line = text.row_to_string(above);
            if line.trim_end().ends_with('{') {
                match depth {
                    0 => return Some(above),
                    _ => depth -= 1,
                }
            }
            if line.trim_start().starts_with('}') {
                depth += 1;
            }
        }
        None
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
    editor: Registry<Editor>,
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
                pairs: false,
                declines_from: u32::MAX,
                host: HeadlessHost::new(),
            },
            state: VimState::new(),
            registers: Registers::new(),
            registry: fe_vim::registry(),
            editor: commands::registry(),
        }
    }

    /// The same, its host changed by `change`.
    fn with(mut self, change: impl FnOnce(&mut Braces)) -> Self {
        change(&mut self.host);
        self
    }

    /// Feeds `keys`, in vim notation.
    fn feed(&mut self, keys: &str) -> &mut Self {
        for key in parse_keys(keys).expect("valid keys") {
            match self.state.mode() {
                Mode::Insert => match key {
                    VimKey::Char(ch) => self.type_char(ch),
                    VimKey::Enter => self.state.line_break_in(&mut self.model, &self.host),
                    VimKey::Esc => self.dispatch(Invocation::of("vim.normal_mode").unwrap()),
                    _ => panic!("{key:?} isn't typed in insert mode here"),
                },
                _ => match self.state.feed(key) {
                    Step::Commit(invocation) => self.dispatch(invocation),
                    Step::Wait(_) => {}
                    step => panic!("{key:?} gave {step:?}"),
                },
            }
        }
        self
    }

    /// Types `ch` as the app does in insert mode: `editor.insert`, which
    /// outdents a closer as the host says.
    fn type_char(&mut self, ch: char) {
        let invocation = Invocation::of("editor.insert")
            .unwrap()
            .args(json!({ "text": ch.to_string() }))
            .context(commands::context());
        let mut target = EditorTarget::new(&mut self.model, &mut self.host);
        self.editor.dispatch(&mut target, &invocation).unwrap();
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
}

/// `keys` fed to `text`, with the caret at `cursor`, by a host suggesting
/// indentation or one without: the text it leaves.
fn fed(text: &str, cursor: (u32, u32), suggests: bool, keys: &str) -> String {
    Vim::new(text, cursor, suggests).feed(keys).text()
}

#[test]
fn o_and_enter_open_an_indented_row_after_a_brace() {
    let block = "fn f() {\n}";
    // `o` on the `{` row, and `Enter` at its end, open a row a level in.
    let mut vim = Vim::new(block, (0, 0), true);
    vim.feed("o");
    assert_eq!(vim.text(), "fn f() {\n    \n}");
    assert_eq!(vim.cursor(), (1, 4));
    vim.feed("x<Esc>");
    assert_eq!(vim.text(), "fn f() {\n    x\n}");
    assert_eq!(
        fed(block, (0, 0), true, "A<CR>x<Esc>"),
        "fn f() {\n    x\n}"
    );
    // Without a suggestion they copy the row's indentation, as before.
    assert_eq!(fed(block, (0, 0), false, "ox<Esc>"), "fn f() {\nx\n}");
    assert_eq!(fed(block, (0, 0), false, "A<CR>x<Esc>"), "fn f() {\nx\n}");
    assert_eq!(fed("  ab", (0, 0), false, "ox<Esc>"), "  ab\n  x");
    assert_eq!(fed("  ab", (0, 0), true, "ox<Esc>"), "  ab\n  x");

    // `O` on the row after the brace opens one at the suggested level, not
    // the row's own.
    let mut vim = Vim::new("fn f() {\nx\n}", (1, 0), true);
    vim.feed("O");
    assert_eq!(vim.text(), "fn f() {\n    \nx\n}");
    assert_eq!(vim.cursor(), (1, 4));
    vim.feed("y<Esc>");
    assert_eq!(vim.text(), "fn f() {\n    y\nx\n}");
    assert_eq!(
        fed("fn f() {\nx\n}", (1, 0), false, "Oy<Esc>"),
        "fn f() {\ny\nx\n}"
    );

    // Indenting is part of the change: one undo takes it all out.
    assert_eq!(fed(block, (0, 0), true, "ox<Esc>u"), block);
    assert_eq!(fed(block, (0, 0), true, "A<CR>x<Esc>u"), block);
}

#[test]
fn a_count_indents_each_copy() {
    let block = "fn f() {\n}";
    assert_eq!(
        fed(block, (0, 0), true, "3ox<Esc>"),
        "fn f() {\n    x\n    x\n    x\n}"
    );
    // Each copy is opened as the host suggests below the one before, as
    // nvim opens each with 'indentexpr': a copy ending with `{` puts the
    // next a level further in.
    assert_eq!(
        fed(block, (0, 0), true, "3oif {<Esc>"),
        "fn f() {\n    if {\n        if {\n            if {\n}"
    );
    // Without a suggestion each copy takes the row before's width.
    assert_eq!(
        fed(block, (0, 0), false, "3oif {<Esc>"),
        "fn f() {\nif {\nif {\nif {\n}"
    );
    // The copies are part of the change too.
    assert_eq!(fed(block, (0, 0), true, "3oif {<Esc>u"), block);
}

#[test]
fn cc_and_s_reindent_the_emptied_row() {
    // An over-indented row in a block: the emptied row goes to the block's
    // level...
    let over = "fn f() {\n            x\n}";
    assert_eq!(fed(over, (1, 12), true, "ccy<Esc>"), "fn f() {\n    y\n}");
    assert_eq!(fed(over, (1, 12), true, "Sy<Esc>"), "fn f() {\n    y\n}");
    let mut vim = Vim::new(over, (1, 12), true);
    vim.feed("cc");
    assert_eq!(vim.text(), "fn f() {\n    \n}");
    assert_eq!(vim.cursor(), (1, 4));
    // ...and without a suggestion keeps its own, as before.
    assert_eq!(
        fed(over, (1, 12), false, "ccy<Esc>"),
        "fn f() {\n            y\n}"
    );
    assert_eq!(
        fed(over, (1, 12), false, "Sy<Esc>"),
        "fn f() {\n            y\n}"
    );
    // One undo puts the row back.
    assert_eq!(fed(over, (1, 12), true, "ccy<Esc>u"), over);
    // `.` changes another row the same way.
    assert_eq!(
        fed(
            "fn f() {\n      x\n}\nfn g() {\nz\n}",
            (1, 6),
            true,
            "ccy<Esc>3j."
        ),
        "fn f() {\n    y\n}\nfn g() {\n    y\n}"
    );
}

#[test]
fn indentation_left_empty_goes_at_esc() {
    let block = "fn f() {\n}";
    // A row `o`, `O`, `cc` or `Enter` indented that nothing was typed on
    // loses its indentation at `Esc`, as with nvim's 'autoindent'.
    assert_eq!(fed(block, (0, 0), true, "o<Esc>"), "fn f() {\n\n}");
    assert_eq!(
        fed("fn f() {\nx\n}", (1, 0), true, "O<Esc>"),
        "fn f() {\n\nx\n}"
    );
    assert_eq!(
        fed("fn f() {\n            x\n}", (1, 12), true, "cc<Esc>"),
        "fn f() {\n\n}"
    );
    assert_eq!(fed(block, (0, 0), true, "A<CR><Esc>"), "fn f() {\n\n}");
    assert_eq!(
        fed(block, (0, 0), true, "A<CR><CR>x<Esc>"),
        "fn f() {\n\n    x\n}"
    );
    // So does the caret's row between braces `Enter` splits, the closer
    // going to a row of its own.
    let mut vim = Vim::new("fn f() {}", (0, 8), true).with(|host| host.pairs = true);
    vim.feed("i<CR>");
    assert_eq!(vim.text(), "fn f() {\n    \n}");
    vim.feed("<Esc>");
    assert_eq!(vim.text(), "fn f() {\n\n}");
    let mut vim = Vim::new("fn f() {}", (0, 8), true).with(|host| host.pairs = true);
    vim.feed("i<CR>x<Esc>");
    assert_eq!(vim.text(), "fn f() {\n    x\n}");
}

#[test]
fn dot_repeats_o_with_the_suggested_indentation() {
    // `.` opens its row as the host suggests where it's repeated: a level
    // in from an unindented `{` row, where the row's copy would be none.
    let two = "fn f() {\n}\nfn g() {\n}";
    let mut vim = Vim::new(two, (0, 0), true);
    vim.feed("oa<Esc>");
    assert_eq!(vim.text(), "fn f() {\n    a\n}\nfn g() {\n}");
    vim.feed("Gk.");
    assert_eq!(vim.text(), "fn f() {\n    a\n}\nfn g() {\n    a\n}");
    assert_eq!(vim.cursor(), (4, 4));
    // Without a suggestion it copies, as before.
    assert_eq!(
        fed(two, (0, 0), false, "oa<Esc>Gk."),
        "fn f() {\na\n}\nfn g() {\na\n}"
    );
}

#[test]
fn a_count_keeps_the_blanks_typed_after_the_indentation() {
    let block = "fn f() {\n}";
    // Each copy opens at the suggested level, then gets the blanks typed
    // after it, as nvim's opening each again and typing them does.
    assert_eq!(
        fed(block, (0, 0), true, "3o  x<Esc>"),
        "fn f() {\n      x\n        x\n          x\n}"
    );
    // A tab typed is kept too, after the suggested indentation, which is
    // the basis row's blanks and a level (none here).
    assert_eq!(
        fed(block, (0, 0), true, "3o\tx<Esc>"),
        "fn f() {\n    \tx\n    \t\tx\n    \t\t\tx\n}"
    );
    assert_eq!(
        fed("fn f() {\nz\n}", (1, 0), true, "3O  x<Esc>"),
        "fn f() {\n      x\n        x\n          x\nz\n}"
    );
    // Without a suggestion, as before.
    assert_eq!(
        fed(block, (0, 0), false, "3o  x<Esc>"),
        "fn f() {\n  x\n    x\n      x\n}"
    );
}

#[test]
fn a_copy_without_a_suggestion_follows_the_row_before() {
    // The second copy has no suggestion: it takes the first's width as
    // re-indented, not as the copies were put in.
    let mut vim = Vim::new("fn f() {\n}", (0, 0), true).with(|host| host.declines_from = 3);
    vim.feed("3oif {<Esc>");
    assert_eq!(
        vim.text(),
        "fn f() {\n    if {\n        if {\n        if {\n}"
    );
}

#[test]
fn indenting_a_count_stays_within_a_puts_bytes() {
    // Every copy nests one level further: its indentation alone would grow
    // as the square of the count. What fits of 64 MiB goes in, as a put's
    // does, and no more.
    let mut vim = Vim::new("fn f() {\n}", (0, 0), true);
    vim.feed("100000oif {<Esc>");
    let text = vim.text();
    assert!(text.len() <= 64 << 20, "{} bytes", text.len());
    let rows: Vec<&str> = text.lines().collect();
    assert!(
        rows.len() > 1_000 && rows.len() < 100_000,
        "{} rows",
        rows.len()
    );
    assert_eq!(
        rows[..4],
        ["fn f() {", "    if {", "        if {", "            if {"]
    );
    let last = rows[rows.len() - 2];
    assert_eq!(last.trim_start(), "if {");
    assert_eq!(last.len() - 4, 4 * (rows.len() - 2));
    assert_eq!(rows[rows.len() - 1], "}");
}

#[test]
fn a_closer_typed_outdents_its_row() {
    // `o` in a block opens a level in; `}` typed there goes back out to
    // its opener's, as typing does in the editor without vim.
    let mut vim = Vim::new("fn f() {\n    if x {\n    }\n}", (1, 0), true);
    vim.feed("o}");
    assert_eq!(vim.text(), "fn f() {\n    if x {\n    }\n    }\n}");
    vim.feed("<Esc>");
    assert_eq!(vim.text(), "fn f() {\n    if x {\n    }\n    }\n}");
    assert_eq!(vim.cursor(), (2, 4));
    // `.` types it again, not the blanks before it, and lines it up too.
    let last = vim.state.last_change().unwrap();
    assert_eq!(last.inserted(), Some("}"));
    vim.feed("k.");
    assert_eq!(vim.text(), "fn f() {\n    if x {\n    }\n    }\n    }\n}");
}

#[test]
fn a_closer_typed_after_blanks_already_there_is_typed_alone() {
    // `A` on a blank row deeper than its block: the `}` typed lines the row
    // up with the `if`, and what `.` types is the `}` alone, not the blanks
    // it was lined up after.
    let text = "fn f() {\n    if x {\n        \n    y();\n}";
    let mut vim = Vim::new(text, (2, 0), true);
    vim.feed("A}<Esc>");
    assert_eq!(vim.text(), "fn f() {\n    if x {\n    }\n    y();\n}");
    assert_eq!(vim.state.last_change().unwrap().inserted(), Some("}"));
    vim.feed("j.");
    assert_eq!(vim.text(), "fn f() {\n    if x {\n    }\n    y();}\n}");
}

#[test]
fn a_closer_typed_on_a_row_with_no_blanks_is_typed_alone() {
    // An empty row in a block: the `}` typed goes in to the `if`'s level,
    // and `.` types the `}` alone.
    let text = "fn f() {\n    if x {\n\n    y();\n}";
    let mut vim = Vim::new(text, (2, 0), true);
    vim.feed("A}<Esc>");
    assert_eq!(vim.text(), "fn f() {\n    if x {\n    }\n    y();\n}");
    assert_eq!(vim.state.last_change().unwrap().inserted(), Some("}"));
    vim.feed("j.");
    assert_eq!(vim.text(), "fn f() {\n    if x {\n    }\n    y();}\n}");
}

#[test]
fn repeating_a_closer_keeps_typing_it_alone() {
    // `.` onto an empty row in another block lines its `}` up there, and
    // what it typed is still the `}` alone, which `.` then types after a
    // statement.
    let text = "fn f() {\n    if x {\n\n    }\n    if y {\n\n    }\n    z();\n}";
    let mut vim = Vim::new(text, (2, 0), true);
    vim.feed("A}<Esc>3j.");
    assert_eq!(
        vim.text(),
        "fn f() {\n    if x {\n    }\n    }\n    if y {\n    }\n    }\n    z();\n}"
    );
    assert_eq!(vim.state.last_change().unwrap().inserted(), Some("}"));
    vim.feed("2j.");
    assert_eq!(
        vim.text(),
        "fn f() {\n    if x {\n    }\n    }\n    if y {\n    }\n    }\n    z();}\n}"
    );
}
