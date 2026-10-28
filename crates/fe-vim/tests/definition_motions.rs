//! `]m` `[m` `]M` `[M`: to the next or previous start or last char of a
//! function, from the functions the host's syntax lists, as
//! nvim-treesitter-textobjects' `@function.outer` motions go. A host here
//! gives canned ones, found again in the text after each key.

use std::{collections::VecDeque, ops::Range};

use fe_buffer::{Buffer, ResolvedSelection};
use fe_command::{Invocation, Registry};
use fe_editor::{EditorModel, commands::EditorHost};
use fe_vim::{
    Mode, Registers, Step, VimCommands, VimKey, VimState, VimTarget, context, parse_keys, registry,
};

/// The host's syntax: the functions it knows, by start.
struct Host {
    functions: Vec<Range<usize>>,
    clipboard: Option<String>,
}

impl EditorHost for Host {
    fn page_rows(&self) -> u32 {
        20
    }

    fn read_clipboard(&mut self) -> Option<String> {
        self.clipboard.clone()
    }

    fn write_clipboard(&mut self, text: String) {
        self.clipboard = Some(text);
    }

    fn fold_range_at(&self, _row: u32) -> Option<std::ops::RangeInclusive<u32>> {
        None
    }

    fn fold_ranges(&self, _rows: Range<u32>) -> Vec<std::ops::RangeInclusive<u32>> {
        Vec::new()
    }

    fn function_bounds(&self) -> Vec<Range<usize>> {
        self.functions.clone()
    }
}

/// Each function as its first bytes and the bytes it ends with: the
/// function runs from the first `start` to the end of the first `end`
/// after it.
type Needles = &'static [(&'static str, &'static str)];

/// An editor fed keys as the GPUI host routes them, replaying macros as it
/// does: a key that fails drops the rest of the replay.
struct Vim {
    model: EditorModel,
    state: VimState,
    host: Host,
    registers: Registers,
    registry: Registry<VimCommands>,
    /// The functions, found in the text before each key; `None` for a
    /// host without syntax.
    needles: Option<Needles>,
}

impl Vim {
    fn new(text: &str, needles: Option<Needles>) -> Self {
        Self {
            model: EditorModel::new(Buffer::new(text), None),
            state: VimState::new(),
            host: Host {
                functions: Vec::new(),
                clipboard: None,
            },
            registers: Registers::new(),
            registry: registry(),
            needles,
        }
    }

    fn caret(&mut self, offset: usize) {
        self.model.set_selections([ResolvedSelection {
            range: offset..offset,
            reversed: false,
        }]);
    }

    fn dispatch(&mut self, invocation: &Invocation) {
        let invocation = invocation.clone().context(context(&self.state));
        let mut target = VimTarget {
            model: &mut self.model,
            host: &mut self.host,
            state: &mut self.state,
            registers: &mut self.registers,
        };
        fe_vim::dispatch(&self.registry, &mut target, &invocation).unwrap();
    }

    fn feed(&mut self, keys: &str) {
        let mut queue: VecDeque<(VimKey, bool)> = (parse_keys(keys).unwrap().into_iter())
            .map(|key| (key, false))
            .collect();
        while let Some((key, replayed)) = queue.pop_front() {
            self.host.functions = match self.needles {
                Some(needles) => bounds(&self.text(), needles),
                None => Vec::new(),
            };
            self.key(key);
            if replayed && self.state.failed() {
                while queue.front().is_some_and(|&(_, replayed)| replayed) {
                    queue.pop_front();
                }
                self.state.stop_replay();
                continue;
            }
            for key in self.state.take_replay().into_iter().rev() {
                queue.push_front((key, true));
            }
        }
    }

    fn key(&mut self, key: VimKey) {
        if self.state.mode() == Mode::Insert {
            self.state.record_key(key);
            match key {
                VimKey::Char(ch) => self.model.insert(ch.encode_utf8(&mut [0; 4])),
                VimKey::Esc => self.dispatch(&Invocation::of("vim.normal_mode").unwrap()),
                _ => panic!("{key:?} in insert mode"),
            }
            return;
        }
        self.state.sync_visual(&mut self.model);
        match self.state.feed(key) {
            Step::Commit(invocation) => self.dispatch(&invocation),
            Step::Wait(_) | Step::Cancel => {}
            Step::Forward => panic!("{key:?} was forwarded"),
        }
    }

    fn text(&self) -> String {
        let text = self.model.text();
        text.slice_to_string(0..text.len())
    }

    /// The caret's offset; in a visual mode, the vim cursor's.
    fn cursor(&self) -> usize {
        (self.state.visual_cursor(&self.model))
            .unwrap_or_else(|| self.model.primary_selection().head())
    }

    fn register(&self) -> String {
        (self.registers.get('"').map(|r| r.text.clone())).unwrap_or_default()
    }

    /// The jump mark (`''`), as an offset.
    fn jump_mark(&self) -> Option<usize> {
        let (row, column) = self.state.mark('\'')?.get(self.model.snapshot())?;
        Some(self.model.text().row_start(row) + column)
    }
}

/// The functions `needles` find in `text`, by start.
fn bounds(text: &str, needles: Needles) -> Vec<Range<usize>> {
    let mut found: Vec<Range<usize>> = (needles.iter())
        .filter_map(|(start, end)| {
            let start = text.find(start)?;
            Some(start..start + text[start..].find(end)? + end.len())
        })
        .collect();
    found.sort_by_key(|range| range.start);
    found
}

const CODE: &str = "\
use x;

fn one() {
    a();
}

impl S {
    fn two() {
        fn inner() { b(); }
        c();
    }

    fn three() {}
}
";

const FUNCTIONS: Needles = &[
    ("fn one", "\n}"),
    ("fn two", "c();\n    }"),
    ("fn inner", "}"),
    ("fn three", "}"),
];

fn at(needle: &str) -> usize {
    CODE.find(needle)
        .unwrap_or_else(|| panic!("{needle:?} not in the code"))
}

/// The offset of the last char of the function starting with `needle`.
fn end_of(needle: &str) -> usize {
    bounds(CODE, FUNCTIONS)
        .into_iter()
        .find(|range| CODE[range.start..].starts_with(needle))
        .unwrap()
        .end
        - 1
}

fn vim_at(needle: &str) -> Vim {
    let mut vim = Vim::new(CODE, Some(FUNCTIONS));
    vim.caret(at(needle));
    vim
}

fn after(needle: &str, keys: &str) -> Vim {
    let mut vim = vim_at(needle);
    vim.feed(keys);
    vim
}

/// Where `keys` from `needle` leave the caret.
fn lands(needle: &str, keys: &str) -> usize {
    after(needle, keys).cursor()
}

#[test]
fn function_motions_move_between_functions() {
    // `]m`: the starts after the caret, nested ones counting.
    assert_eq!(lands("use x", "]m"), at("fn one"));
    assert_eq!(lands("use x", "2]m"), at("fn two"));
    assert_eq!(lands("use x", "3]m"), at("fn inner"));
    assert_eq!(lands("use x", "4]m"), at("fn three"));
    // A count past the last goes as far as there are.
    assert_eq!(lands("use x", "9]m"), at("fn three"));
    assert_eq!(lands("use x", "]m]m]m"), at("fn inner"));
    // From inside a function, from its start, from between functions.
    assert_eq!(lands("a();", "]m"), at("fn two"));
    assert_eq!(lands("fn two", "]m"), at("fn inner"));
    assert_eq!(lands("c();", "]m"), at("fn three"));
    assert_eq!(lands("impl S", "]m"), at("fn two"));
    // `[m`: the starts before it.
    assert_eq!(lands("c();", "[m"), at("fn inner"));
    assert_eq!(lands("c();", "2[m"), at("fn two"));
    assert_eq!(lands("c();", "3[m"), at("fn one"));
    assert_eq!(lands("c();", "9[m"), at("fn one"));
    assert_eq!(lands("b();", "[m"), at("fn inner"));
    assert_eq!(lands("fn inner", "[m"), at("fn two"));
    assert_eq!(lands("impl S", "[m"), at("fn one"));
    // `]M`: the last chars after it, a nested one's before the one around.
    assert_eq!(lands("use x", "]M"), end_of("fn one"));
    assert_eq!(lands("a();", "]M"), end_of("fn one"));
    assert_eq!(lands("fn two", "]M"), end_of("fn inner"));
    assert_eq!(lands("fn two", "2]M"), end_of("fn two"));
    assert_eq!(lands("fn two", "3]M"), end_of("fn three"));
    assert_eq!(lands("c();", "]M"), end_of("fn two"));
    assert_eq!(lands("impl S", "]M"), end_of("fn inner"));
    // `[M`: those before it.
    assert_eq!(lands("fn three", "[M"), end_of("fn two"));
    assert_eq!(lands("fn three", "2[M"), end_of("fn inner"));
    assert_eq!(lands("fn three", "3[M"), end_of("fn one"));
    assert_eq!(lands("c();", "[M"), end_of("fn inner"));
    assert_eq!(lands("impl S", "[M"), end_of("fn one"));
    // Each is a jump: `''` goes back.
    for keys in ["]m", "2[m", "]M", "[M"] {
        let mut vim = vim_at("c();");
        vim.feed(keys);
        assert_eq!(vim.jump_mark(), Some(at("c();")), "{keys}");
        vim.feed("``");
        assert_eq!(vim.cursor(), at("c();"), "{keys}");
    }
}

#[test]
fn function_motions_fail_at_the_ends() {
    let failing = [
        ("fn three", "]m"),
        ("fn three", "9]m"),
        ("fn one", "[m"),
        ("use x", "[m"),
        ("a();", "[M"),
        ("fn one", "[M"),
    ];
    for (needle, keys) in failing {
        let mut vim = vim_at(needle);
        let before = vim.cursor();
        vim.feed(keys);
        assert!(vim.state.failed(), "{keys} from {needle:?}");
        assert_eq!(vim.cursor(), before, "{keys} from {needle:?}");
    }
    // From the last function's last char nothing ends after it: an impl
    // isn't a function.
    let mut vim = Vim::new(CODE, Some(FUNCTIONS));
    vim.caret(end_of("fn three"));
    vim.feed("]M");
    assert!(vim.state.failed());
    assert_eq!(vim.cursor(), end_of("fn three"));
    // A failing one sets no jump mark, and keeps the one there was.
    let mut vim = vim_at("fn three");
    vim.feed("]m");
    assert_eq!(vim.jump_mark(), None);
    vim.feed("[m");
    assert_eq!(vim.jump_mark(), Some(at("fn three")));
    vim.feed("[m[m[m[m");
    assert_eq!(vim.cursor(), at("fn one"));
    assert_eq!(vim.jump_mark(), Some(at("fn two")));
    // It stops a macro: `j` after it doesn't run.
    let mut vim = vim_at("use x");
    vim.feed("qa]mjq");
    assert_eq!(vim.cursor(), at("a();") - 4);
    vim.caret(at("fn three"));
    vim.feed("@a");
    assert_eq!(vim.cursor(), at("fn three"));
    // A host without syntax lists none: each fails, as nowhere to go.
    for keys in ["]m", "[m", "]M", "[M", "d]m", "v]m"] {
        let mut vim = Vim::new(CODE, None);
        vim.caret(at("c();"));
        vim.feed(keys);
        assert!(vim.state.failed(), "{keys}");
        assert_eq!(vim.cursor(), at("c();"), "{keys}");
        assert_eq!(vim.text(), CODE, "{keys}");
        assert_eq!(vim.jump_mark(), None, "{keys}");
    }
    let mut vim = Vim::new(CODE, None);
    vim.feed("qa]mjq");
    assert_eq!(vim.cursor(), at("\nfn one"));
    vim.caret(0);
    vim.feed("@a");
    assert_eq!(vim.cursor(), 0);
}

#[test]
fn function_motions_with_operators_and_visual() {
    // `d]m` is exclusive: within the text, up to the start.
    let vim = after("a();", "d]m");
    assert_eq!(
        vim.text(),
        format!("{}{}", &CODE[..at("a();")], &CODE[at("fn two")..])
    );
    // From a row's first column to another's, it takes whole rows
    // (`:help exclusive-linewise`).
    let vim = after("use x", "d]m");
    assert_eq!(vim.text(), &CODE[at("fn one")..]);
    // `y]M` is inclusive: through the closing brace.
    let vim = after("fn two", "y]M");
    assert_eq!(vim.text(), CODE);
    assert_eq!(vim.register(), "fn two() {\n        fn inner() { b(); }");
    assert_eq!(vim.cursor(), at("fn two"));
    let vim = after("fn two", "2y]M");
    assert_eq!(vim.register(), &CODE[at("fn two")..=end_of("fn two")]);
    // `c[m` back to a start, exclusive of the caret.
    let vim = after("c();", "c[mX<Esc>");
    assert_eq!(
        vim.text(),
        format!("{}X{}", &CODE[..at("fn inner")], &CODE[at("c();")..])
    );
    // `d[M` back to an end, inclusive: the caret's char too.
    let vim = after("c();", "d[M");
    assert_eq!(
        vim.text(),
        format!("{}{}", &CODE[..end_of("fn inner")], &CODE[at("c();") + 1..])
    );
    // `ys]M` wraps up to the end.
    let vim = after("a();", "ys]Mb");
    assert_eq!(vim.text(), CODE.replace("a();\n}", "(a();\n})"));
    // A failing one changes nothing.
    let vim = after("fn three", "d]m");
    assert_eq!(vim.text(), CODE);
    assert!(vim.state.failed());
    // Visual: the cursor goes there, the selection taking its char.
    let mut vim = after("use x", "v]m");
    assert_eq!(vim.state.mode(), Mode::Visual(fe_vim::VisualKind::Char));
    assert_eq!(vim.cursor(), at("fn one"));
    vim.feed("y");
    assert_eq!(vim.register(), &CODE[..=at("fn one")]);
    let mut vim = after("fn two", "v]M");
    vim.feed("]M");
    assert_eq!(vim.cursor(), end_of("fn two"));
    vim.feed("y");
    assert_eq!(vim.register(), &CODE[at("fn two")..=end_of("fn two")]);
    let mut vim = after("c();", "v[m");
    assert_eq!(vim.cursor(), at("fn inner"));
    vim.feed("d");
    assert_eq!(
        vim.text(),
        format!("{}{}", &CODE[..at("fn inner")], &CODE[at("c();") + 1..])
    );
    // Visual past the last fails, keeping the selection.
    let mut vim = after("fn three", "vl]m");
    assert!(vim.state.failed());
    assert_eq!(vim.cursor(), at("fn three") + 1);
    vim.feed("y");
    assert_eq!(vim.register(), "fn");
    // `.` repeats `d]m`, the functions found again in the changed text.
    const ROWS: &str = "x\nfn a() {}\ny\nfn b() {}\nz\n";
    const ROW_FUNCTIONS: Needles = &[("fn a", "}"), ("fn b", "}")];
    let mut vim = Vim::new(ROWS, Some(ROW_FUNCTIONS));
    vim.feed("d]m");
    assert_eq!(vim.text(), "fn a() {}\ny\nfn b() {}\nz\n");
    vim.feed("j.");
    assert_eq!(vim.text(), "fn a() {}\nfn b() {}\nz\n");
    // Nothing left after: `.` fails.
    vim.feed("j.");
    assert_eq!(vim.text(), "fn a() {}\nfn b() {}\nz\n");
    assert!(vim.state.failed());
}
