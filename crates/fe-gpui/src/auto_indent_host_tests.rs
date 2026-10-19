use std::time::{Duration, Instant};

use crate::tests::{editor, state};
use fe_buffer::{Buffer, Origin, ResolvedSelection};
use fe_command::{Invocation, Source};
use fe_syntax::{Syntax, language_by_name};
use gpui::{Entity, TestAppContext, VisualTestContext};

use super::reparsed;
use crate::EditorView;

/// A view of `language` showing `marked`, once its tree is parsed. `marked`
/// has one caret, `|`, and its braces are text.
fn view_of<'a>(
    cx: &'a mut TestAppContext,
    language: &str,
    marked: &str,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    let (view, cx) = editor(cx, "|");
    let caret = marked.find('|').expect("a caret");
    view.update(cx, |view, cx| {
        let model = view.model_mut();
        model.insert(&marked.replace('|', ""));
        model.set_selections(vec![ResolvedSelection {
            range: caret..caret,
            reversed: false,
        }]);
        view.set_language(Some(language), cx);
    });
    cx.run_until_parked();
    (view, cx)
}

fn newline(view: &mut EditorView, cx: &mut gpui::Context<EditorView>) {
    let newline = Invocation::of("editor.newline")
        .expect("valid command ID")
        .source(Source::Keyboard);
    view.run_command(newline, None, cx);
}

/// A view of `language` showing `marked`, after `editor.newline` and then,
/// on a later frame, typing `typed` a char at a time, as keys do.
fn entered(cx: &mut TestAppContext, language: &str, marked: &str, typed: &str) -> String {
    let (view, cx) = view_of(cx, language, marked);
    view.update(cx, newline);
    cx.run_until_parked();
    view.update(cx, |view, cx| {
        for ch in typed.chars() {
            view.type_text(&ch.to_string(), cx);
        }
    });
    state(&view, cx)
}

#[gpui::test]
fn indents_by_language(cx: &mut TestAppContext) {
    // Rust: in a level after an opening brace, and between braces typing
    // paired, the closer on its own row.
    assert_eq!(
        entered(cx, "rust", "fn main() {|", ""),
        "fn main() {\n    |"
    );
    assert_eq!(
        entered(cx, "rust", "fn main() {|}", ""),
        "fn main() {\n    |\n}"
    );
    assert_eq!(
        entered(cx, "rust", "mod m {\n  fn f() {|}\n}", ""),
        "mod m {\n  fn f() {\n      |\n  }\n}"
    );
    // A closer typed at the new row's start goes back out to its opener's.
    assert_eq!(
        entered(cx, "rust", "fn main() {\n    let x = 1;|", "}"),
        "fn main() {\n    let x = 1;\n}|"
    );
    // Python: in a level after a colon.
    assert_eq!(entered(cx, "python", "def f():|", ""), "def f():\n    |");
    assert_eq!(
        entered(cx, "python", "class C:\n    def f(self):|", ""),
        "class C:\n    def f(self):\n        |"
    );
    // A language without indent rules copies the indentation, as plain
    // text does.
    assert_eq!(entered(cx, "markdown", "  - a {|", ""), "  - a {\n  |");
}

#[gpui::test]
fn before_the_tree_catches_up(cx: &mut TestAppContext) {
    // An opener typed and Enter before a frame brings the tree up to it.
    let (view, cx) = view_of(cx, "python", "def f()|");
    view.update(cx, |view, cx| {
        view.type_text(":", cx);
        newline(view, cx);
    });
    assert_eq!(state(&view, cx), "def f():\n    |");

    // A closer typed straight after Enter.
    let (view, cx) = view_of(cx, "rust", "fn main() {\n    x;|");
    view.update(cx, |view, cx| {
        newline(view, cx);
        view.type_text("}", cx);
    });
    assert_eq!(state(&view, cx), "fn main() {\n    x;\n}|");
}

#[test]
fn a_stale_tree_is_parsed_within_the_time() {
    let item = "fn f(x: u32) -> u32 {\n    let y = x + 1;\n    if y > 2 { y } else { x }\n}\n";
    let mut buffer = Buffer::new(&item.repeat(8000));
    let rust = language_by_name("rust").expect("rust is built in");
    let mut syntax = Syntax::new(rust, &buffer.snapshot());
    syntax.finish(syntax.parse_job().run());

    // A tree parsed at its version seeds the copy.
    buffer.edit([(0..0, "\n")], Origin::User);
    let caught_up = reparsed(
        &syntax,
        &buffer.snapshot(),
        Instant::now() + Duration::from_secs(60),
    )
    .expect("in time");
    assert_eq!(caught_up.version(), buffer.snapshot().version());
    assert!(!caught_up.needs_parse());

    // Edited and waiting on a reparse that redoes the whole file, as an
    // unclosed block around it does, the tree isn't reparsed: the text is
    // parsed afresh until the time runs out.
    buffer.edit([(0..0, "mod m {\n")], Origin::User);
    syntax.edit(&buffer.snapshot());
    assert!(syntax.needs_parse());
    // The fastest of interleaved runs, so a thread descheduled under load
    // in one run doesn't decide it.
    let (mut bounded, mut stale) = (Duration::MAX, Duration::MAX);
    for _ in 0..5 {
        let started = Instant::now();
        let deadline = started + Duration::from_millis(2);
        assert!(reparsed(&syntax, &buffer.snapshot(), deadline).is_none());
        bounded = bounded.min(started.elapsed());
        let started = Instant::now();
        syntax.parse_job().run();
        stale = stale.min(started.elapsed());
    }
    assert!(bounded * 2 < stale, "{bounded:?} bounded, {stale:?} stale");
}

#[test]
fn a_slow_seed_stops_at_the_deadline() {
    // Parsed at its version, but malformed throughout: reparsing it reuses
    // little of the tree, so seeding a copy costs about a parse. The seed
    // stops at the deadline, on this thread: nothing parses on after.
    let item = "fn f(x: u32 -> u32 {\n    let y = x + ;\n    if y > 2 { y  else { x }\n}}\n";
    let mut buffer = Buffer::new(&item.repeat(8000));
    let rust = language_by_name("rust").expect("rust is built in");
    let mut syntax = Syntax::new(rust, &buffer.snapshot());
    syntax.finish(syntax.parse_job().run());
    buffer.edit([(0..0, "\n")], Origin::User);
    assert!(!syntax.needs_parse());

    // The fastest of interleaved runs, so a thread descheduled under load
    // in one run doesn't decide it.
    let (mut bounded, mut seed) = (Duration::MAX, Duration::MAX);
    for _ in 0..5 {
        let started = Instant::now();
        let deadline = started + Duration::from_millis(2);
        assert!(reparsed(&syntax, &buffer.snapshot(), deadline).is_none());
        bounded = bounded.min(started.elapsed());
        let started = Instant::now();
        syntax.parse_job().run();
        seed = seed.min(started.elapsed());
    }
    assert!(bounded * 2 < seed, "{bounded:?} bounded, {seed:?} seeding");
}

#[test]
fn a_slow_seed_still_catches_up_by_a_long_deadline() {
    // Malformed throughout, so seeding a copy takes far longer than the
    // 20 ms an indent is given: a long deadline still gets the copy.
    let item = "fn f(x: u32 -> u32 {\n    let y = x + ;\n    if y > 2 { y  else { x }\n}}\n";
    let mut buffer = Buffer::new(&item.repeat(8000));
    let rust = language_by_name("rust").expect("rust is built in");
    let mut syntax = Syntax::new(rust, &buffer.snapshot());
    syntax.finish(syntax.parse_job().run());
    buffer.edit([(0..0, "\n")], Origin::User);

    // What the call did, not how long it took: how slow a seed is depends on
    // the machine, and a fast one made a lower bound on time fail.
    let snapshot = buffer.snapshot();
    let caught_up = reparsed(&syntax, &snapshot, Instant::now() + Duration::from_secs(60))
        .expect("a long deadline gets the copy");
    assert_eq!(caught_up.version(), snapshot.version());
    assert!(caught_up.tree().is_some());
    assert!(!caught_up.needs_parse());
}
