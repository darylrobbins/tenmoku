//! The outline of the bracket pair at the caret, and pairs coloured by depth
//! while a reparse is pending.

use fe_buffer::{Buffer, ResolvedSelection};
use fe_editor::EditorModel;
use gpui::{Bounds, Pixels, TestAppContext, VisualTestContext};

use super::*;
use crate::init;

/// An editor on `text` at `path`, parsed, 10 rows tall and drawn.
fn editor<'a>(
    text: &str,
    path: Option<&str>,
    cx: &'a mut TestAppContext,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    cx.update(init);
    let (text, path) = (text.to_owned(), path.map(Into::into));
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let model = EditorModel::new(Buffer::new(&text), path);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.run_until_parked();
    let line_height = draw(&view, cx);
    cx.simulate_resize(size(px(600.), px(line_height * 10.)));
    draw(&view, cx);
    (view, cx)
}

/// Draws a frame; its line height.
fn draw(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> f32 {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    view.read_with(cx, |view, _| {
        view.layout.as_ref().unwrap().frame.line_height
    })
}

fn painted(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<Bounds<Pixels>> {
    view.read_with(cx, |view, _| view.painted_brackets.clone())
}

/// The left edges of the outlines, in window coordinates, of the characters
/// at `offsets`.
fn lefts(view: &Entity<EditorView>, offsets: &[usize], cx: &mut VisualTestContext) -> Vec<Pixels> {
    view.update(cx, |view, _| {
        let layout = view.layout.as_mut().unwrap();
        let mut at = |offset| layout.caret_bounds(offset).unwrap().origin.x;
        offsets.iter().map(|&offset| at(offset)).collect()
    })
}

fn xs(rects: &[Bounds<Pixels>]) -> Vec<Pixels> {
    rects.iter().map(|rect| rect.origin.x).collect()
}

#[gpui::test]
fn outlines_both(cx: &mut TestAppContext) {
    let (view, cx) = editor("fn f() { (a) }\n", Some("main.rs"), cx);
    view.update(cx, |view, _| view.model.set_cursor(9));
    draw(&view, cx);
    let rects = painted(&view, cx);
    assert_eq!(rects.len(), 2);
    assert_eq!(xs(&rects), lefts(&view, &[9, 11], cx));
    assert!(rects.iter().all(|rect| rect.size.width > px(0.)));

    // `((x)a)`, the caret after the new `)`: the new pair, once parsed.
    view.update(cx, |view, cx| {
        view.model.set_cursor(10);
        view.model.insert("(x)");
        view.changed(cx);
    });
    let stale = lefts(&view, &[9, 11], cx);
    draw(&view, cx);
    assert_ne!(xs(&painted(&view, cx)), stale, "never the old pair");
    cx.run_until_parked();
    draw(&view, cx);
    let rects = painted(&view, cx);
    assert_eq!(rects.len(), 2);
    assert_eq!(xs(&rects), lefts(&view, &[10, 12], cx));
}

#[gpui::test]
fn nothing_to_outline(cx: &mut TestAppContext) {
    let (view, cx) = editor("fn f() { (a) }\n", Some("main.rs"), cx);
    let none = |view: &Entity<EditorView>, cx: &mut VisualTestContext, why: &str| {
        draw(view, cx);
        assert!(painted(view, cx).is_empty(), "{why}");
    };
    view.update(cx, |view, _| view.model.set_cursor(9));
    draw(&view, cx);
    assert_eq!(painted(&view, cx).len(), 2);

    view.update(cx, |view, _| view.model.select_range(9..10, false));
    none(&view, cx, "a selection");
    view.update(cx, |view, _| {
        let caret = |at| ResolvedSelection {
            range: at..at,
            reversed: false,
        };
        view.model.set_selections([caret(9), caret(11)]);
    });
    none(&view, cx, "two carets");
    view.update(cx, |view, _| view.model.set_cursor(0));
    none(&view, cx, "a caret off the brackets");

    let (plain, cx) = editor("fn f() { (a) }\n", None, cx);
    plain.update(cx, |view, _| view.model.set_cursor(9));
    none(&plain, cx, "plain text");

    // The `)` is scrolled out of view: only the `(` is outlined.
    let text = format!("fn f() {{\n{}}}\n", "    let a = 1;\n".repeat(40));
    let (tall, cx) = editor(&text, Some("main.rs"), cx);
    tall.update(cx, |view, _| view.model.set_cursor(7));
    draw(&tall, cx);
    assert_eq!(xs(&painted(&tall, cx)), lefts(&tall, &[7], cx));
}

#[gpui::test]
fn nothing_while_a_reparse_runs(cx: &mut TestAppContext) {
    let rust = fe_syntax::language_by_name("rust").expect("rust loads");
    // Long enough that a reparse overruns the clock's budget at once.
    let text = format!("fn f() {{ (a) }}\n{}", "fn g() {}\n".repeat(200));
    let (view, cx) = editor(&text, Some("main.rs"), cx);
    view.update(cx, |view, cx| {
        // A clock that moves a second on at each read is always over the sync
        // budget, so a reparse goes to the background.
        let start = std::time::Instant::now();
        let elapsed = std::cell::Cell::new(std::time::Duration::ZERO);
        let clock = move || {
            elapsed.set(elapsed.get() + std::time::Duration::from_secs(1));
            start + elapsed.get()
        };
        view.syntax = Some(
            SyntaxDriver::new(
                Syntax::new(rust, view.model.snapshot()),
                std::time::Duration::from_millis(1),
            )
            .with_clock(clock),
        );
        // `((b)+a)`: the caret before the `(` pairs it with a later `)` than
        // the old tree, edited, has.
        view.model.set_cursor(10);
        view.model.insert("(b)+");
        view.model.set_cursor(9);
        view.changed(cx);
    });
    draw(&view, cx);
    assert!(painted(&view, cx).is_empty(), "the tree lags the text");
    view.update(cx, |view, cx| {
        assert!(view.syntax.as_ref().unwrap().is_parsing());
        // Again, with the first parse still running.
        view.model.set_cursor(10);
        view.model.insert("(c)+");
        view.model.set_cursor(9);
        view.changed(cx);
    });
    draw(&view, cx);
    assert!(painted(&view, cx).is_empty(), "the tree lags the text");
    // The first parse is for the text before the second edit; a frame after
    // it starts the next.
    for _ in 0..2 {
        cx.run_until_parked();
        draw(&view, cx);
    }
    let rects = painted(&view, cx);
    assert_eq!(xs(&rects), lefts(&view, &[9, 19], cx));
}

/// Quotes typed around brackets while reparses run in the background: the
/// edited tree still has the quoted brackets as leaves, so no pair is
/// coloured until the tree is reparsed, and then the quoted ones aren't.
#[gpui::test]
fn depth_colours_wait_for_a_reparse(cx: &mut TestAppContext) {
    let rust = fe_syntax::language_by_name("rust").expect("rust loads");
    let text = format!(
        "fn f() {{ g([1, [2] , [3]]); }}\n{}",
        "fn g() {}\n".repeat(200)
    );
    let (view, cx) = editor(&text, Some("main.rs"), cx);
    let inner = text.find("[2]").unwrap();
    view.update(cx, |view, _| {
        view.theme.bracket_pair_colors = true;
        // A clock that moves a second on at each read is always over the sync
        // budget, so a reparse goes to the background.
        let start = std::time::Instant::now();
        let elapsed = std::cell::Cell::new(std::time::Duration::ZERO);
        let clock = move || {
            elapsed.set(elapsed.get() + std::time::Duration::from_secs(1));
            start + elapsed.get()
        };
        let mut syntax = Syntax::new(rust, view.model.snapshot());
        syntax.finish(syntax.parse_job().run());
        view.syntax =
            Some(SyntaxDriver::new(syntax, std::time::Duration::from_millis(1)).with_clock(clock));
    });
    draw(&view, cx);
    let colors = view.read_with(cx, |view, _| view.theme.brackets);
    let g = text.find("g(").unwrap() + 1;
    assert_eq!(
        view.read_with(cx, |view, _| view.drawn_color(g)),
        Some(colors[1])
    );

    // `"[2] "`: a quote before `[2]`, drawn, then one after it, drawn.
    for at in [inner, inner + 5] {
        view.update(cx, |view, cx| {
            view.model.set_cursor(at);
            view.model.insert("\"");
            view.changed(cx);
        });
        draw(&view, cx);
        view.read_with(cx, |view, _| {
            assert!(view.syntax.as_ref().unwrap().syntax().needs_parse());
            let text = view.model.text().to_string();
            for (at, _) in text[..40].match_indices(['(', ')', '[', ']', '{', '}']) {
                let color = view.drawn_color(at).unwrap();
                assert!(!colors.contains(&color), "{at} of {text:?}");
            }
        });
    }
    for _ in 0..2 {
        cx.run_until_parked();
        draw(&view, cx);
    }
    view.read_with(cx, |view, _| {
        assert!(!view.syntax().unwrap().needs_parse());
        let quote = view.drawn_color(inner).unwrap();
        assert!(!colors.contains(&quote));
        for at in inner..inner + 6 {
            assert_eq!(view.drawn_color(at), Some(quote), "{at}");
        }
        assert_eq!(view.drawn_color(g), Some(colors[1]));
        // `[3]` is three deep, in the first colour again.
        let three = view.model.text().to_string().find("[3]").unwrap();
        assert_eq!(view.drawn_color(three), Some(colors[0]));
    });
}
