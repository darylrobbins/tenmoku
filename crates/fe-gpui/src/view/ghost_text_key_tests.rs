//! Ghost text's keys ([`super::ghost_text`]) through the default keymaps:
//! `tab` accepting it as one undo step, or into vim's insert session,
//! accept-word taking its next word, the completion menu's `tab` winning
//! over it, and `escape` and losing focus dropping it, each told to the
//! host as a [`GhostTextEvent`].

use std::{cell::RefCell, path::Path, rc::Rc};

use fe_buffer::{Buffer, Origin};
use fe_editor::{EditorModel, ghost_text::GhostText};
use fe_keymap::{DEFAULT_KEYMAP_PATH, DEFAULT_LINUX, DEFAULT_MACOS, Keymap, KeymapLayer};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::*;

/// The caret is before the `;`.
const TEXT: &str = "foo;\nnext\nlast\n";
const CARET: usize = 3;

/// A platform's resolved default keymap.
fn keymap(source: &str) -> Keymap {
    let (layer, diagnostics) = KeymapLayer::parse(Path::new(DEFAULT_KEYMAP_PATH), source, None);
    assert_eq!(diagnostics, []);
    Keymap::from_layers(vec![layer.unwrap()])
}

/// A focused editor on [`TEXT`], the caret at [`CARET`], vim on or off,
/// with macOS's default keymap.
fn editor(vim: bool, cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    cx.update(|cx| init_with_keymap(cx, keymap(DEFAULT_MACOS)));
    let (view, cx) = cx.add_window_view(|_, cx| {
        let mut model = EditorModel::new(Buffer::new(TEXT), None);
        model.set_cursor(CARET);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&view.focus_handle(cx), cx);
    });
    cx.update(|_, cx| set_vim_enabled(cx, vim));
    cx.run_until_parked();
    (view, cx)
}

fn ai() -> Origin {
    Origin::Ai {
        agent: "agent".into(),
        session: "session".into(),
    }
}

/// Suggests `text` at the view's caret.
fn suggest(view: &Entity<EditorView>, text: &str, cx: &mut VisualTestContext) {
    view.update(cx, |view, cx| {
        let model = view.model();
        let at = model.primary_selection().head();
        let ghost = GhostText::new(model.snapshot(), at, text, ai());
        view.set_ghost_text(Some(ghost), cx);
    });
}

/// The events `view` emits from now on.
fn events(
    view: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> Rc<RefCell<Vec<GhostTextEvent>>> {
    let events = Rc::new(RefCell::new(Vec::new()));
    let seen = events.clone();
    cx.update(|_, cx| {
        let push = move |_, event: &GhostTextEvent, _: &mut gpui::App| {
            seen.borrow_mut().push(event.clone())
        };
        cx.subscribe(view, push).detach();
    });
    events
}

/// The events since the last call: each accept's text and whether it was
/// whole, or `None` for a dismissal.
fn taken(events: &Rc<RefCell<Vec<GhostTextEvent>>>) -> Vec<Option<(String, bool)>> {
    (events.borrow_mut().drain(..))
        .map(|event| match event {
            GhostTextEvent::Accepted { text, whole, .. } => Some((text, whole)),
            GhostTextEvent::Dismissed => None,
        })
        .collect()
}

fn accepted(text: &str, whole: bool) -> Option<(String, bool)> {
    Some((text.into(), whole))
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn keys(keys: &str, cx: &mut VisualTestContext) {
    draw(cx);
    cx.simulate_keystrokes(keys);
    cx.run_until_parked();
}

fn text(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |view, _| view.model().text().to_string())
}

/// What remains of the suggestion to show, if any.
fn remaining(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<String> {
    view.read_with(cx, |view, _| {
        let ghost = view.ghost_text()?;
        Some(ghost.remaining(view.model())?.to_owned())
    })
}

#[gpui::test]
fn tab_accepts_one_undo_step(cx: &mut TestAppContext) {
    let (view, cx) = editor(false, cx);
    let events = events(&view, cx);

    suggest(&view, "bar()", cx);
    keys("tab", cx);
    assert_eq!(text(&view, cx), "foobar();\nnext\nlast\n");
    assert_eq!(taken(&events), [accepted("bar()", true)]);
    assert_eq!(remaining(&view, cx), None);
    draw(cx);
    assert_eq!(taken(&events), [], "accepting drops it without a dismissal");

    // One undo takes exactly that back.
    keys("cmd-z", cx);
    assert_eq!(text(&view, cx), TEXT);
    keys("cmd-shift-z", cx);
    assert_eq!(text(&view, cx), "foobar();\nnext\nlast\n");
    keys("cmd-z", cx);
    assert_eq!(text(&view, cx), TEXT);

    // With none shown, `tab` is the editor's again.
    keys("tab", cx);
    let after = text(&view, cx);
    let indent = &after[CARET..after.len() - (TEXT.len() - CARET)];
    assert!(
        !indent.is_empty() && indent.chars().all(|c| c == ' ' || c == '\t'),
        "{after:?}"
    );
    assert_eq!(taken(&events), []);
}

#[gpui::test]
fn completion_tab_wins(cx: &mut TestAppContext) {
    use super::completion_tests::{editor, keys};
    let (view, _provider, cx) = editor(cx);
    let events = events(&view, cx);
    keys("ctrl-space", cx);
    suggest(&view, "riety", cx);
    let shown = view.read_with(cx, |view, _| {
        (view.completion_shown(), view.ghost_text_shown())
    });
    assert_eq!(shown, (true, true));

    keys("tab", cx);
    assert_eq!(text(&view, cx), "fn main() {\n    value\n}\n");
    assert!(!view.read_with(cx, |view, _| view.completion_shown()));
    // Not the suggestion, which `tab` didn't touch.
    draw(cx);
    let accepts = (events.borrow().iter())
        .filter(|event| matches!(event, GhostTextEvent::Accepted { .. }))
        .count();
    assert_eq!(accepts, 0);
}

#[gpui::test]
fn vim_insert(cx: &mut TestAppContext) {
    let (view, cx) = editor(true, cx);
    let events = events(&view, cx);
    let mode = |cx: &mut VisualTestContext| view.read_with(cx, |view, _| view.vim_mode());
    keys("i", cx);
    assert_eq!(mode(cx), Some("insert"));

    // Typing consumes it; `tab` takes the rest.
    suggest(&view, "bar()", cx);
    cx.simulate_input("b");
    cx.run_until_parked();
    assert_eq!(remaining(&view, cx).as_deref(), Some("ar()"));
    keys("tab", cx);
    assert_eq!(text(&view, cx), "foobar();\nnext\nlast\n");
    assert_eq!(taken(&events), [accepted("ar()", true)]);
    assert_eq!(mode(cx), Some("insert"));

    // One `escape` drops another and leaves insert mode, editing nothing.
    suggest(&view, " + 1", cx);
    assert_eq!(remaining(&view, cx).as_deref(), Some(" + 1"));
    keys("escape", cx);
    assert_eq!(mode(cx), Some("normal"));
    assert_eq!(remaining(&view, cx), None);
    assert_eq!(text(&view, cx), "foobar();\nnext\nlast\n");
    draw(cx);
    assert_eq!(taken(&events), [None]);

    // `u` undoes the session, the accepted text with what was typed.
    keys("u", cx);
    assert_eq!(text(&view, cx), TEXT);
}

#[gpui::test]
fn escape_blur_and_accept_word(cx: &mut TestAppContext) {
    let (view, cx) = editor(false, cx);
    let events = events(&view, cx);

    // `escape` drops it, editing nothing.
    suggest(&view, "bar()", cx);
    keys("escape", cx);
    assert_eq!(remaining(&view, cx), None);
    draw(cx);
    assert_eq!(taken(&events), [None]);
    assert_eq!(text(&view, cx), TEXT);

    // So does losing focus.
    suggest(&view, "bar()", cx);
    cx.update(|window, cx| window.blur(cx));
    cx.run_until_parked();
    assert_eq!(remaining(&view, cx), None);
    draw(cx);
    assert_eq!(taken(&events), [None]);
    assert_eq!(text(&view, cx), TEXT);
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.run_until_parked();

    // `cmd-right` takes its next word; the rest still shows.
    suggest(&view, "bar(baz, qux)", cx);
    keys("cmd-right", cx);
    assert_eq!(text(&view, cx), "foobar;\nnext\nlast\n");
    assert_eq!(remaining(&view, cx).as_deref(), Some("(baz, qux)"));
    assert_eq!(taken(&events), [accepted("bar", false)]);

    // Off macOS, `ctrl-right`.
    cx.update(|_, cx| init_with_keymap(cx, keymap(DEFAULT_LINUX)));
    keys("ctrl-right", cx);
    assert_eq!(text(&view, cx), "foobar(baz;\nnext\nlast\n");
    assert_eq!(remaining(&view, cx).as_deref(), Some(", qux)"));
    keys("ctrl-right ctrl-right", cx);
    assert_eq!(text(&view, cx), "foobar(baz, qux);\nnext\nlast\n");
    assert_eq!(remaining(&view, cx), None);
    // The last word took the rest: whole, and not dismissed after.
    draw(cx);
    assert_eq!(
        taken(&events),
        [
            accepted("(baz", false),
            accepted(", qux", false),
            accepted(")", true)
        ]
    );
}

#[gpui::test]
fn dismiss_and_accept_line_from_the_registry(cx: &mut TestAppContext) {
    let (view, cx) = editor(false, cx);
    let events = events(&view, cx);
    // As the palette runs them, in the context the view last drew.
    let run = |command: &str, cx: &mut VisualTestContext| {
        draw(cx);
        let shown = view.read_with(cx, |view, _| view.ghost_text_shown());
        let context = super::ghost_text::stamp(fe_editor::commands::context(), shown);
        let invocation = fe_command::Invocation::of(command)
            .unwrap()
            .context(context);
        view.update(cx, |view, cx| view.run_command(invocation, None, cx));
    };

    suggest(&view, "bar() {\n    baz\n}", cx);
    run("ghost_text.accept_line", cx);
    assert_eq!(text(&view, cx), "foobar() {\n;\nnext\nlast\n");
    assert_eq!(remaining(&view, cx).as_deref(), Some("    baz\n}"));
    run("ghost_text.dismiss", cx);
    assert_eq!(remaining(&view, cx), None);
    draw(cx);
    assert_eq!(
        taken(&events),
        [accepted("bar() {\n", false), None],
        "dismissed once"
    );
    // With none shown, they do nothing.
    run("ghost_text.accept", cx);
    run("ghost_text.dismiss", cx);
    assert_eq!(text(&view, cx), "foobar() {\n;\nnext\nlast\n");
    assert_eq!(taken(&events), []);
}

#[gpui::test]
fn host_replacing_it_isnt_a_dismissal(cx: &mut TestAppContext) {
    let (view, cx) = editor(false, cx);
    let events = events(&view, cx);
    suggest(&view, "bar()", cx);
    suggest(&view, "baz()", cx);
    view.update(cx, |view, cx| view.set_ghost_text(None, cx));
    draw(cx);
    assert_eq!(taken(&events), []);
    // Typing what isn't suggested ends it: a dismissal.
    suggest(&view, "bar()", cx);
    cx.simulate_input("x");
    cx.run_until_parked();
    draw(cx);
    assert_eq!(taken(&events), [None]);
}

/// An edit through another view of the buffer that ends the suggestion is
/// seen by the next frame: it paints nothing, and the suggestion is gone.
#[gpui::test]
fn shared_buffer_edit_drops_it_in_one_frame(cx: &mut TestAppContext) {
    let (view, cx) = editor(false, cx);
    let events = events(&view, cx);
    let mut other = view.read_with(cx, |view, _| view.model().split());
    suggest(&view, "bar()", cx);
    cx.simulate_input("b");
    cx.run_until_parked();
    draw(cx);
    assert_eq!(remaining(&view, cx).as_deref(), Some("ar()"));

    // The other view puts `z` for the `b` typed.
    other.sync_display();
    other.select_range(CARET..CARET + 1, false);
    other.insert("z");
    assert_eq!(other.text().to_string(), "fooz;\nnext\nlast\n");
    // Read in the update that draws the next frame.
    let (painted, kept) = cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        let view = view.read(cx);
        (view.painted_ghost.clone(), view.ghost_text().is_some())
    });
    assert_eq!((painted, kept), (None, false));
    assert_eq!(taken(&events), [None]);
}

#[gpui::test]
fn blur_repaints_without_another_draw(cx: &mut TestAppContext) {
    let (view, cx) = editor(false, cx);
    suggest(&view, "bar()", cx);
    draw(cx);
    assert!(view.read_with(cx, |view, _| view.painted_ghost.is_some()));
    cx.update(|window, cx| window.blur(cx));
    cx.run_until_parked();
    assert_eq!(remaining(&view, cx), None);
    // No draw is forced: the window repainted on its own.
    assert!(view.read_with(cx, |view, _| view.painted_ghost.is_none()));
}

#[gpui::test]
fn escape_dismisses_before_bindings(cx: &mut TestAppContext) {
    let (view, cx) = editor(false, cx);
    let user = {
        let source = "[[bindings]]\nkeys = \"escape\"\ncommand = \"ghost_text.accept\"\ncontext = \"ghost_text\"\n";
        let (layer, diagnostics) = KeymapLayer::parse(Path::new("user.toml"), source, None);
        assert_eq!(diagnostics, []);
        layer.unwrap()
    };
    let (default, _) = KeymapLayer::parse(Path::new(DEFAULT_KEYMAP_PATH), DEFAULT_MACOS, None);
    cx.update(|_, cx| init_with_keymap(cx, Keymap::from_layers(vec![default.unwrap(), user])));
    let events = events(&view, cx);
    suggest(&view, "bar()", cx);
    keys("escape", cx);
    assert_eq!(remaining(&view, cx), None);
    assert_eq!(text(&view, cx), TEXT);
    draw(cx);
    assert_eq!(taken(&events), [None]);

    // In vim's insert mode, the same escape also leaves it.
    cx.update(|_, cx| set_vim_enabled(cx, true));
    keys("i", cx);
    suggest(&view, "bar()", cx);
    keys("escape", cx);
    assert_eq!(
        view.read_with(cx, |view, _| view.vim_mode()),
        Some("normal")
    );
    assert_eq!(remaining(&view, cx), None);
    assert_eq!(text(&view, cx), TEXT);
    draw(cx);
    assert_eq!(taken(&events), [None]);
}

#[gpui::test]
fn host_edits_end_the_suggestion_blur_dropped(cx: &mut TestAppContext) {
    let (view, cx) = editor(false, cx);
    let events = events(&view, cx);
    // The `t` ending row 1, `next`.
    let at = TEXT.find("next").unwrap() + 3;
    let mut expected = TEXT.to_owned();
    for (split, with) in [(false, "u"), (true, "v")] {
        cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
        cx.run_until_parked();
        suggest(&view, "bar()", cx);
        draw(cx);
        cx.update(|window, cx| window.blur(cx));
        cx.run_until_parked();
        assert_eq!(remaining(&view, cx), None);
        events.borrow_mut().clear();

        // As a formatter reply does, or another view of the buffer.
        expected.replace_range(at..at + 1, with);
        if split {
            let mut other = view.read_with(cx, |view, _| view.model().split());
            other.buffer_mut().edit([(at..at + 1, with)], Origin::User);
        } else {
            view.update(cx, |view, _| {
                let mut buffer = view.model_mut().buffer_mut();
                buffer.edit([(at..at + 1, with)], Origin::User);
            });
        }
        view.update(cx, |_, cx| cx.notify());

        let context = super::ghost_text::stamp(fe_editor::commands::context(), true);
        let invocation = fe_command::Invocation::of("ghost_text.accept")
            .unwrap()
            .source(fe_command::Source::Palette)
            .context(context);
        view.update_in(cx, |view, window, cx| {
            view.run_command(invocation, Some(window), cx)
        });
        assert_eq!(text(&view, cx), expected);
        assert_eq!(remaining(&view, cx), None);
        assert!(events.borrow().is_empty());
    }
}
