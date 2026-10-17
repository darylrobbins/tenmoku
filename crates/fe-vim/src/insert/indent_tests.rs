//! Indenting by language holds where measuring text edits the buffer
//! through another model (see `EditorModel`'s char widths) between the
//! batch a row's indentation is planned from and the one it goes in by:
//! the plan is made from the text as it goes in.

use std::{
    cell::Cell,
    ops::{Range, RangeInclusive},
    rc::Rc,
};

use fe_buffer::{Buffer, BufferSnapshot, Origin, Point};
use fe_command::{Invocation, Registry};
use fe_editor::{
    EditorModel, IndentStyle, IndentSuggestion,
    commands::{EditorHost, HeadlessHost},
};

use crate::{
    Mode, Registers, Step, VimCommands, VimKey, VimState, VimTarget, context, parse_keys,
    tests::Measuring,
};

/// A host answering as a C-like language would: a row after one ending
/// with `{` a level in from it, any other the row before's level; a `}`
/// starting a row lines it up with its `{`'s.
struct Braces(HeadlessHost);

impl EditorHost for Braces {
    fn page_rows(&self) -> u32 {
        self.0.page_rows()
    }

    fn read_clipboard(&mut self) -> Option<String> {
        self.0.read_clipboard()
    }

    fn write_clipboard(&mut self, text: String) {
        self.0.write_clipboard(text);
    }

    fn fold_range_at(&self, row: u32) -> Option<RangeInclusive<u32>> {
        self.0.fold_range_at(row)
    }

    fn fold_ranges(&self, rows: Range<u32>) -> Vec<RangeInclusive<u32>> {
        self.0.fold_ranges(rows)
    }

    fn suggest_indent(&self, snapshot: &BufferSnapshot, row: u32) -> Option<IndentSuggestion> {
        let basis_row = row.checked_sub(1)?;
        let opens = (snapshot.text().row_to_string(basis_row))
            .trim_end()
            .ends_with('{');
        let delta = i32::from(opens);
        Some(IndentSuggestion::Relative { basis_row, delta })
    }

    fn closer_outdents(&self, snapshot: &BufferSnapshot, row: u32) -> Option<u32> {
        let text = snapshot.text();
        if !text.row_to_string(row).trim_start().starts_with('}') {
            return None;
        }
        (0..row)
            .rev()
            .find(|&above| text.row_to_string(above).trim_end().ends_with('{'))
    }
}

/// An editor in vim mode, indenting four spaces a level, and another model
/// of its buffer, for measuring to edit through.
struct Vim {
    model: EditorModel,
    split: EditorModel,
    host: Braces,
    state: VimState,
    registers: Registers,
    registry: Registry<VimCommands>,
}

impl Vim {
    fn new(text: &str, (row, column): (u32, u32)) -> Self {
        let mut model = EditorModel::new(Buffer::new(text), None);
        model.set_indent_style(Some(IndentStyle::Spaces(4)));
        let offset = model.text().point_to_offset(Point::new(row, column));
        model.set_cursor(offset);
        Self {
            split: model.split(),
            model,
            host: Braces(HeadlessHost::new()),
            state: VimState::new(),
            registers: Registers::new(),
            registry: crate::registry(),
        }
    }

    /// Feeds `keys`: in insert mode a char is typed into the model, as
    /// `.` types its text.
    fn feed(&mut self, keys: &str) {
        for key in parse_keys(keys).unwrap() {
            match (self.state.mode(), key) {
                (Mode::Insert, VimKey::Char(ch)) => self.model.insert(&ch.to_string()),
                (Mode::Insert, VimKey::Esc) => {
                    self.dispatch(Invocation::of("vim.normal_mode").unwrap())
                }
                (Mode::Insert, _) => panic!("{key:?} isn't typed here"),
                _ => match self.state.feed(key) {
                    Step::Commit(invocation) => self.dispatch(invocation),
                    Step::Wait(_) => {}
                    step => panic!("{key:?} gave {step:?}"),
                },
            }
        }
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

    /// Feeds `keys` while the first text measured (as an edit ends, with
    /// `after_edit`) puts a row above all, as another origin's edit.
    fn feed_measuring(&mut self, keys: &str, after_edit: bool) {
        self.feed_editing(keys, after_edit, |_| (0..0, "X\n".to_owned()));
    }

    /// [`Vim::feed_measuring`], the edit another origin makes `edit`'s,
    /// from the text then.
    fn feed_editing(
        &mut self,
        keys: &str,
        after_edit: bool,
        mut edit: impl FnMut(&str) -> (Range<usize>, String) + 'static,
    ) {
        let armed = Rc::new(Cell::new(true));
        let mut split = self.split.split();
        let _measuring = Measuring::new({
            let armed = Rc::clone(&armed);
            move |edited| {
                if (edited || !after_edit) && armed.replace(false) {
                    let ai = Origin::Ai {
                        agent: "agent".into(),
                        session: "session".into(),
                    };
                    let text = split.buffer().snapshot().text().to_string();
                    split.buffer_mut().edit([edit(&text)], ai);
                }
            }
        });
        self.feed(keys);
        assert!(!armed.get(), "nothing was measured");
    }

    fn text(&self) -> String {
        self.model.text().to_string()
    }
}

#[test]
fn opening_a_row_is_indented_where_it_went() {
    let mut vim = Vim::new("fn f() {\n}", (0, 0));
    vim.feed_measuring("ox<Esc>", false);
    assert_eq!(vim.text(), "X\nfn f() {\n    x\n}");
    let mut vim = Vim::new("fn f() {\n            x\n}", (1, 12));
    vim.feed_measuring("ccy<Esc>", false);
    assert_eq!(vim.text(), "X\nfn f() {\n    y\n}");
}

#[test]
fn a_counts_copies_are_indented_where_they_went() {
    // The row goes in above while the copies are measured: they're still
    // what's indented, each a level further in.
    let mut vim = Vim::new("fn f() {\n}", (0, 0));
    vim.feed("3oif {");
    vim.feed_measuring("<Esc>", true);
    assert_eq!(
        vim.text(),
        "X\nfn f() {\n    if {\n        if {\n            if {\n}"
    );
}

#[test]
fn a_replayed_closer_is_lined_up_where_it_went() {
    // Typed into the model as it is here, `}` isn't lined up; `.` types
    // it again, then lines its row up with the `if`'s, though a row went
    // in above while what it typed was measured.
    let mut vim = Vim::new("fn f() {\n    if x {\n        y();\n    }\n}", (2, 0));
    vim.feed("o}<Esc>");
    assert_eq!(
        vim.text(),
        "fn f() {\n    if x {\n        y();\n        }\n    }\n}"
    );
    vim.feed_measuring(".", true);
    assert_eq!(
        vim.text(),
        "X\nfn f() {\n    if x {\n        y();\n        }\n    }\n    }\n}"
    );
}

#[test]
fn copies_written_inside_while_measured_are_left_as_they_are() {
    // Another origin joins the second copy onto the first, or rewrites the
    // second's blanks, as the copies are measured: the same bytes, but not
    // the rows they were. Neither the copies nor the row after them are
    // re-indented.
    let mut vim = Vim::new("fn f() {\n}", (0, 0));
    vim.feed("3oif {");
    vim.feed_editing("<Esc>", true, |text| {
        let second = text.match_indices('\n').nth(2).unwrap().0;
        (second..second + 1, " ".to_owned())
    });
    assert_eq!(vim.text(), "fn f() {\n    if {\n    if {     if {\n}");
    let mut vim = Vim::new("fn f() {\n}", (0, 0));
    vim.feed("3oif {");
    vim.feed_editing("<Esc>", true, |text| {
        let second = text.match_indices('\n').nth(2).unwrap().0 + 1;
        (second..second + 4, "\t   ".to_owned())
    });
    assert_eq!(vim.text(), "fn f() {\n    if {\n    if {\n\t   if {\n}");
}

#[test]
fn copies_written_at_their_end_while_measured_are_left_as_they_are() {
    // Another origin puts blanks after the last copy, whose row holds only
    // blanks, as the copies are measured: they'd run on into its row's
    // indentation, so the copies aren't re-indented, and its blanks stay.
    let mut plain = Vim::new("fn f() {\n}", (0, 0));
    plain.feed("3o <Esc>");
    let mut vim = Vim::new("fn f() {\n}", (0, 0));
    vim.feed("3o ");
    vim.feed_editing("<Esc>", true, |text| {
        let end = text.rfind('\n').unwrap();
        (end..end, " \t \t".to_owned())
    });
    let text = vim.text();
    let (copies, after) = text.rsplit_once('\n').unwrap();
    assert_eq!(after, "}");
    assert!(copies.ends_with(" \t \t"), "{copies:?}");
    let before = copies.strip_suffix(" \t \t").unwrap();
    assert_eq!(
        before,
        plain.text().rsplit_once('\n').unwrap().0,
        "{text:?}"
    );
}
