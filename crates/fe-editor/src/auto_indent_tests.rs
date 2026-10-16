use std::{
    cell::Cell,
    ops::{Range, RangeInclusive},
    rc::Rc,
    time::Instant,
};

use fe_buffer::BufferSnapshot;
use fe_command::Invocation;
use serde_json::json;

use super::*;
use crate::{
    TYPING_PAUSE,
    commands::{self, EditorTarget},
    tests::{parse, render},
};

/// Brackets and double quotes, as Rust has them.
const BRACKETS: &[(char, char)] = &[('(', ')'), ('{', '}'), ('"', '"')];

/// A host answering fixed suggestions: every row `delta` levels in from the
/// row before, and every closer lining up with `closer`'s row, or with the
/// row before if `closer_before`.
#[derive(Default)]
struct IndentHost {
    delta: Option<i32>,
    zero: bool,
    closer: Option<u32>,
    closer_before: bool,
    pairs: &'static [(char, char)],
    /// A model's clock, which answering moves on past a typing pause, as
    /// a slow parse would.
    clock: Option<Rc<Cell<Instant>>>,
}

impl IndentHost {
    fn take_time(&self) {
        if let Some(clock) = &self.clock {
            clock.set(clock.get() + TYPING_PAUSE * 2);
        }
    }
}

impl EditorHost for IndentHost {
    fn page_rows(&self) -> u32 {
        20
    }
    fn read_clipboard(&mut self) -> Option<String> {
        None
    }
    fn write_clipboard(&mut self, _: String) {}
    fn fold_range_at(&self, _: u32) -> Option<RangeInclusive<u32>> {
        None
    }
    fn fold_ranges(&self, _: Range<u32>) -> Vec<RangeInclusive<u32>> {
        Vec::new()
    }
    fn auto_pairs(&self) -> &[(char, char)] {
        self.pairs
    }
    fn suggest_indent(&self, _: &BufferSnapshot, row: u32) -> Option<IndentSuggestion> {
        self.take_time();
        if self.zero {
            return Some(IndentSuggestion::Zero);
        }
        Some(IndentSuggestion::Relative {
            basis_row: row - 1,
            delta: self.delta?,
        })
    }
    fn closer_outdents(&self, _: &BufferSnapshot, row: u32) -> Option<u32> {
        self.take_time();
        match self.closer_before {
            true => Some(row - 1),
            false => self.closer,
        }
    }
}

fn indenting(delta: i32) -> IndentHost {
    IndentHost {
        delta: Some(delta),
        ..IndentHost::default()
    }
}

fn run(model: &mut EditorModel, host: &mut IndentHost, command: &str, args: serde_json::Value) {
    let invocation = Invocation::of(command)
        .unwrap()
        .args(args)
        .context(commands::context());
    commands::registry()
        .dispatch(&mut EditorTarget::new(model, host), &invocation)
        .unwrap_or_else(|error| panic!("{command}: {error}"));
}

/// `marked` with `style`, after `editor.newline` with `host`.
fn newlined(marked: &str, style: Option<IndentStyle>, mut host: IndentHost) -> String {
    let mut model = parse(marked);
    model.set_indent_style(style);
    run(&mut model, &mut host, "editor.newline", json!({}));
    render(&model)
}

/// `marked` after typing `text` through `editor.insert` with `host`.
fn typed(marked: &str, text: &str, mut host: IndentHost) -> String {
    let mut model = parse(marked);
    for ch in text.chars() {
        run(
            &mut model,
            &mut host,
            "editor.insert",
            json!({ "text": ch.to_string() }),
        );
    }
    render(&model)
}

#[test]
fn newline() {
    let spaces = Some(IndentStyle::Spaces(2));
    let tabs = Some(IndentStyle::Tabs);
    // A level in, in the buffer's unit: the detected width of spaces, tabs,
    // or with neither the tab size.
    assert_eq!(newlined("  b {|", spaces, indenting(1)), "  b {\n    |");
    assert_eq!(newlined("\tb {|", tabs, indenting(1)), "\tb {\n\t\t|");
    assert_eq!(newlined("b {|", None, indenting(1)), "b {\n    |");
    // Text after the caret moves down, its blanks replaced.
    assert_eq!(
        newlined("  f(a,| b)", spaces, indenting(1)),
        "  f(a,\n    |b)"
    );
    // Out a level, and to no indentation.
    assert_eq!(
        newlined("      x|", spaces, indenting(-1)),
        "      x\n    |"
    );
    assert_eq!(newlined("\t\tx|", tabs, indenting(-1)), "\t\tx\n\t|");
    assert_eq!(newlined("  x|", spaces, indenting(-3)), "  x\n|");
    let zero = IndentHost {
        zero: true,
        ..IndentHost::default()
    };
    assert_eq!(newlined("  #x|", spaces, zero), "  #x\n|");
    // With no suggestion, the row's indentation is copied, as without a
    // host's syntax.
    assert_eq!(
        newlined("  b {|", spaces, IndentHost::default()),
        "  b {\n  |"
    );
    assert_eq!(
        newlined("    x|y", None, IndentHost::default()),
        "    x\n    |y"
    );

    // A suggestion takes the caret past blanks the line break carried down,
    // even where they're the suggested indentation already, alone or with
    // other rows re-indented; without one, the caret stays before them.
    for (marked, entered) in [
        ("  {|  x", "  {\n    |x"),
        ("a {|\n  {|  x", "a {\n  |\n  {\n    |x"),
    ] {
        let mut model = parse(marked);
        model.set_indent_style(spaces);
        run(&mut model, &mut indenting(1), "editor.newline", json!({}));
        assert_eq!(render(&model), entered);
        model.undo().unwrap();
        assert_eq!(render(&model), marked);
        model.redo().unwrap();
        assert_eq!(render(&model), entered);
    }
    assert_eq!(
        newlined("  {|  x", spaces, IndentHost::default()),
        "  {\n  |  x"
    );

    // A row's basis is as this indents it, when it's another new row.
    assert_eq!(newlined("{|{|", spaces, indenting(1)), "{\n  |{\n    |");

    // At every caret, as one undo step.
    let marked = "a {|\n  b {|\nc|";
    let mut model = parse(marked);
    model.set_indent_style(spaces);
    run(&mut model, &mut indenting(1), "editor.newline", json!({}));
    assert_eq!(render(&model), "a {\n  |\n  b {\n    |\nc\n  |");
    model.undo().unwrap();
    assert_eq!(render(&model), marked);
    // Redoing puts a caret back at each new row's indentation.
    model.redo().unwrap();
    assert_eq!(render(&model), "a {\n  |\n  b {\n    |\nc\n  |");
}

#[test]
fn between_pair() {
    let spaces = Some(IndentStyle::Spaces(2));
    let host = || IndentHost {
        delta: Some(1),
        pairs: BRACKETS,
        ..IndentHost::default()
    };
    // The closer goes to its own row at the opener's indentation, the caret
    // to an indented row between.
    let marked = "  f() {|}";
    let mut model = parse(marked);
    model.set_indent_style(spaces);
    run(&mut model, &mut host(), "editor.newline", json!({}));
    assert_eq!(render(&model), "  f() {\n    |\n  }");
    model.undo().unwrap();
    assert_eq!(render(&model), marked);
    model.redo().unwrap();
    assert_eq!(render(&model), "  f() {\n    |\n  }");

    assert_eq!(newlined("f(|)", spaces, host()), "f(\n  |\n)");
    // A split closer lines up with its opener as this indents the opener's
    // row, when that's another new row.
    let marked = "{|{|}";
    let mut model = parse(marked);
    model.set_indent_style(spaces);
    run(&mut model, &mut host(), "editor.newline", json!({}));
    let nested = "{\n  |{\n    |\n  }";
    assert_eq!(render(&model), nested);
    model.undo().unwrap();
    assert_eq!(render(&model), marked);
    model.redo().unwrap();
    assert_eq!(render(&model), nested);
    // And a later row counts from that closer's row as this indents it.
    let marked = "{|{|} {|}";
    let mut model = parse(marked);
    model.set_indent_style(spaces);
    run(&mut model, &mut host(), "editor.newline", json!({}));
    let chained = "{\n  |{\n    |\n  } {\n    |\n  }";
    assert_eq!(render(&model), chained);
    model.undo().unwrap();
    assert_eq!(render(&model), marked);
    model.redo().unwrap();
    assert_eq!(render(&model), chained);
    assert_eq!(
        newlined("{|}\n  {|}", spaces, host()),
        "{\n  |\n}\n  {\n    |\n  }"
    );
    // Without a suggestion, the row between is a level in from the opener.
    let unsuggested = IndentHost {
        pairs: BRACKETS,
        ..IndentHost::default()
    };
    assert_eq!(newlined("  {|}", spaces, unsuggested), "  {\n    |\n  }");
    // A caret on a row that isn't re-indented stays where the line break
    // left it, before the blanks after it, redone too.
    let marked = "{|}\n  |  x";
    let mut model = parse(marked);
    model.set_indent_style(spaces);
    let mut unsuggested = IndentHost {
        pairs: BRACKETS,
        ..IndentHost::default()
    };
    run(&mut model, &mut unsuggested, "editor.newline", json!({}));
    let split = "{\n  |\n}\n  \n  |  x";
    assert_eq!(render(&model), split);
    model.undo().unwrap();
    assert_eq!(render(&model), marked);
    model.redo().unwrap();
    assert_eq!(render(&model), split);
    // Quotes and closers that don't match aren't pairs, nor are pairs the
    // host doesn't know.
    assert_eq!(newlined("\"|\"", spaces, host()), "\"\n  |\"");
    assert_eq!(newlined("(|}", spaces, host()), "(\n  |}");
    assert_eq!(newlined("{|}", spaces, indenting(1)), "{\n  |}");
}

#[test]
fn closer() {
    let host = || IndentHost {
        closer: Some(0),
        pairs: BRACKETS,
        ..IndentHost::default()
    };
    // The row lines up with the opener's, in the edit typing makes.
    let marked = "  fn f() {\n      x\n        |";
    let mut model = parse(marked);
    let mut lined_up = host();
    run(
        &mut model,
        &mut lined_up,
        "editor.insert",
        json!({ "text": "}" }),
    );
    assert_eq!(render(&model), "  fn f() {\n      x\n  }|");
    // One undo takes out both.
    model.undo().unwrap();
    assert_eq!(render(&model), marked);

    // In from too little too, at every caret, and with text after.
    assert_eq!(
        typed(
            "\t{\n|\n\t{\n    |x",
            "}",
            IndentHost {
                closer: Some(0),
                ..IndentHost::default()
            }
        ),
        "\t{\n\t}|\n\t{\n\t}|x"
    );
    // Only as the row's first non-blank.
    assert_eq!(typed("{\n    x|", "}", host()), "{\n    x}|");
    // Nothing without an answer, or when it's lined up already.
    assert_eq!(typed("{\n    |", "}", IndentHost::default()), "{\n    }|");
    assert_eq!(typed("  {\n  |", "}", host()), "  {\n  }|");
    // Each row lines up with its basis as it's re-indented here.
    let before = IndentHost {
        closer_before: true,
        ..IndentHost::default()
    };
    assert_eq!(typed("  {\n|\n    |", "}", before), "  {\n  }|\n  }|");

    // Redoing puts back a caret after each char typed, re-indented or not.
    let marked = "  {\n      |\n  {\n  |";
    let mut model = parse(marked);
    run(
        &mut model,
        &mut host(),
        "editor.insert",
        json!({ "text": "}" }),
    );
    let typed_twice = "  {\n  }|\n  {\n  }|";
    assert_eq!(render(&model), typed_twice);
    model.undo().unwrap();
    assert_eq!(render(&model), marked);
    model.redo().unwrap();
    assert_eq!(render(&model), typed_twice);

    // The re-indent joins the char's undo step however long the host takes
    // to answer, and the run of typing ends there.
    let marked = "{\n    |";
    let mut model = parse(marked);
    let clock = Rc::new(Cell::new(Instant::now()));
    let now = Rc::clone(&clock);
    model.set_clock(Rc::new(move || now.get()));
    let mut slow = IndentHost {
        clock: Some(clock),
        ..host()
    };
    for ch in ["}", ";"] {
        run(
            &mut model,
            &mut slow,
            "editor.insert",
            json!({ "text": ch }),
        );
    }
    assert_eq!(render(&model), "{\n};|");
    model.undo().unwrap();
    assert_eq!(render(&model), "{\n}|");
    model.undo().unwrap();
    assert_eq!(render(&model), marked);
    model.redo().unwrap();
    assert_eq!(render(&model), "{\n}|");
}
