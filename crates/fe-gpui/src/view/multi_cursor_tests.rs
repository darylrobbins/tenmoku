//! Multi-cursor from the keyboard and the mouse, in a headless window: the
//! default keymap's keys run fe-editor's commands with vim off and leave
//! vim's keys alone with it on; alt-click toggles a caret and
//! alt-shift-drag selects a box.

use std::{cell::RefCell, path::Path, rc::Rc};

use fe_buffer::{Buffer, ResolvedSelection};
use fe_command::Invocation;
use fe_editor::EditorModel;
use fe_keymap::{DEFAULT_KEYMAP_PATH, DEFAULT_MACOS, Keymap, KeymapLayer};
use gpui::{
    AppContext as _, Context, Entity, Focusable as _, InteractiveElement as _, IntoElement,
    Modifiers, MouseButton, ParentElement as _, Pixels, Point, Render, Styled as _, TestAppContext,
    VisualTestContext, Window, div, point, px,
};

use crate::{commands::RunCommand, *};

/// `foo` starts every row, for the occurrence commands.
const WORDS: &str = "foo bar\nfoo baz\nfoo qux\nfoo end\nfoo\n";

/// Rows at least six columns long but for the last, for boxes.
const GRID: &str = "0123456789\nabcdefgh\nABCDEFGH\nklmnopqrstu\nxy\n";

/// The test layer: `alt-click` runs `test.jump` in the editor.
const ALT_CLICK: &str = "\
[[bindings]]
keys = \"alt-click\"
command = \"test.jump\"
context = \"editor\"
";

/// The editor, with the commands that propagate past it recorded.
struct Host {
    editor: Entity<EditorView>,
    seen: Rc<RefCell<Vec<Invocation>>>,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let seen = self.seen.clone();
        div()
            .size_full()
            .on_action(move |run: &RunCommand, _, _| seen.borrow_mut().push(run.0.clone()))
            .child(self.editor.clone())
    }
}

/// A focused editor on `text`, the caret at 0, with [`keymap`] of `layer`,
/// in an active window.
#[allow(clippy::type_complexity)]
fn editor<'a>(
    text: &str,
    layer: Option<&str>,
    cx: &'a mut TestAppContext,
) -> (
    Entity<EditorView>,
    Rc<RefCell<Vec<Invocation>>>,
    &'a mut VisualTestContext,
) {
    cx.update(|cx| init_with_keymap(cx, keymap(layer)));
    let seen = Rc::new(RefCell::new(Vec::new()));
    let (host, cx) = cx.add_window_view({
        let (seen, text) = (seen.clone(), text.to_owned());
        move |_, cx| {
            let model = EditorModel::new(Buffer::new(&text), None);
            let editor = cx.new(|cx| EditorView::new(model, EditorTheme::default_dark(cx), cx));
            Host { editor, seen }
        }
    });
    let view = host.read_with(cx, |host, _| host.editor.clone());
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&view.focus_handle(cx), cx);
    });
    cx.run_until_parked();
    (view, seen, cx)
}

/// The macOS default keymap, with `layer` over it if given.
fn keymap(layer: Option<&str>) -> Keymap {
    let (default, _) = KeymapLayer::parse(Path::new(DEFAULT_KEYMAP_PATH), DEFAULT_MACOS, None);
    let mut layers = vec![default.unwrap()];
    if let Some(source) = layer {
        let (layer, diagnostics) = KeymapLayer::parse(Path::new("keymap.toml"), source, None);
        assert_eq!(diagnostics, []);
        layers.push(layer.unwrap());
    }
    Keymap::from_layers(layers)
}

/// A model method a key runs.
type Command = fn(&mut EditorModel);

fn selections(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<ResolvedSelection> {
    view.update(cx, |view, _| view.model().selections())
}

fn text(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    view.update(cx, |view, _| view.model().text().to_string())
}

fn keys(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    cx.run_until_parked();
}

fn selection(range: std::ops::Range<usize>) -> ResolvedSelection {
    ResolvedSelection {
        range,
        reversed: false,
    }
}

fn alt() -> Modifiers {
    Modifiers {
        alt: true,
        ..Modifiers::none()
    }
}

fn alt_shift() -> Modifiers {
    Modifiers {
        shift: true,
        ..alt()
    }
}

/// Mid-row `row`, at display column `column` as row 0 (the longest, in a
/// monospace font) lays it out, in the last frame: past `row`'s end for a
/// column it doesn't have.
fn at(
    view: &Entity<EditorView>,
    row: u32,
    column: usize,
    cx: &mut VisualTestContext,
) -> Point<Pixels> {
    view.update(cx, |view, _| {
        let row_start = view.model().text().row_start(row);
        let layout = view.layout.as_mut().unwrap();
        let x = layout.caret_bounds(column).unwrap().left();
        let y = layout.caret_bounds(row_start).unwrap().center().y;
        point(x + px(1.), y)
    })
}

#[gpui::test]
fn keys_add_carets(cx: &mut TestAppContext) {
    let (view, _, cx) = editor(WORDS, None, cx);
    view.update(cx, |view, _| view.model_mut().set_cursor(1));
    // The same commands on a model of its own.
    let mut expected = EditorModel::new(Buffer::new(WORDS), None);
    expected.set_cursor(1);
    let steps: [(&str, Command); 9] = [
        ("cmd-alt-down", EditorModel::add_caret_below),
        ("cmd-alt-down", EditorModel::add_caret_below),
        ("cmd-d", EditorModel::select_next_occurrence),
        ("cmd-d", EditorModel::select_next_occurrence),
        ("cmd-shift-l", EditorModel::select_all_occurrences),
        ("escape", EditorModel::single_selection),
        ("cmd-a", EditorModel::select_all),
        ("alt-shift-i", EditorModel::split_selection_into_lines),
        ("escape", EditorModel::single_selection),
    ];
    for (key, run) in steps {
        let before = selections(&view, cx);
        run(&mut expected);
        keys(key, cx);
        let after = selections(&view, cx);
        assert_eq!(after, expected.selections(), "{key}");
        assert_ne!(after, before, "{key} changed nothing");
    }
    assert_eq!(selections(&view, cx).len(), 1);
    assert_eq!(text(&view, cx), WORDS);

    // Three carets, then escape leaves one, a caret.
    view.update(cx, |view, _| view.model_mut().set_cursor(1));
    keys("cmd-alt-down cmd-alt-down", cx);
    assert_eq!(selections(&view, cx).len(), 3);
    keys("escape", cx);
    let caret = selections(&view, cx);
    assert_eq!(caret.len(), 1);
    assert!(caret[0].range.is_empty());
}

#[gpui::test]
fn vim_keeps_its_keys(cx: &mut TestAppContext) {
    let (view, _, cx) = editor(WORDS, None, cx);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    cx.run_until_parked();

    // On the drawn key context and the one keys are matched in.
    let enabled = |stack: &fe_command::ContextStack| {
        let frames = stack.frames();
        let editor = frames
            .iter()
            .rfind(|frame| frame.name == fe_editor::commands::CONTEXT)
            .unwrap();
        editor.has(crate::vim::ENABLED)
    };
    let drawn = cx.update(|window, _| crate::keymap::context_stack(&window.context_stack()));
    assert!(enabled(&drawn), "{drawn:?}");
    let live = cx.update(|window, cx| crate::vim::live_stack(drawn.clone(), window, cx).0);
    assert!(enabled(&live), "{live:?}");

    let mode = |cx: &mut VisualTestContext| view.read_with(cx, |view, _| view.vim_mode());
    let pending = |cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| {
            view.vim
                .as_ref()
                .is_some_and(|vim| vim.pending().kind().is_some())
        })
    };
    assert_eq!(mode(cx), Some("normal"));
    // Escape after an operator cancels it: `w` then only moves.
    keys("d", cx);
    assert!(pending(cx));
    keys("escape", cx);
    assert!(!pending(cx));
    keys("w", cx);
    assert_eq!(text(&view, cx), WORDS);
    assert_eq!(mode(cx), Some("normal"));

    for (visual, label) in [("", "normal"), ("v", "visual")] {
        for key in [
            "cmd-d",
            "cmd-alt-down",
            "alt-shift-down",
            "cmd-shift-l",
            "alt-shift-i",
        ] {
            view.update(cx, |view, _| view.model_mut().set_cursor(1));
            keys(visual, cx);
            keys(key, cx);
            // In normal mode the cmd keys add carets, in vim's layer
            // (`vim::visual_tests::multi_caret_tests`); escape drops them.
            if !(visual.is_empty() && key.starts_with("cmd-")) {
                assert_eq!(selections(&view, cx).len(), 1, "{key} in {label} mode");
            }
            keys("escape", cx);
            assert_eq!(selections(&view, cx).len(), 1, "escape in {label} mode");
            assert_eq!(mode(cx), Some("normal"), "{key} in {label} mode");
        }
    }
    assert_eq!(text(&view, cx), WORDS);
}

#[gpui::test]
fn alt_click_and_box(cx: &mut TestAppContext) {
    let (view, seen, cx) = editor(GRID, None, cx);
    let row_start = |row: u32| {
        GRID.split_inclusive('\n')
            .take(row as usize)
            .map(str::len)
            .sum()
    };
    let r = |row: u32, columns: std::ops::Range<usize>| {
        let start: usize = row_start(row);
        selection(start + columns.start..start + columns.end)
    };

    // Alt-click adds a caret, and alt-click on it removes it.
    let position = at(&view, 1, 3, cx);
    cx.simulate_click(position, alt());
    cx.run_until_parked();
    assert_eq!(selections(&view, cx), [r(0, 0..0), r(1, 3..3)]);
    assert!(view.read_with(cx, |view, _| view.drag.is_none()));
    cx.simulate_click(position, alt());
    cx.run_until_parked();
    assert_eq!(selections(&view, cx), [r(0, 0..0)]);

    // Alt-shift-drag from row 0 column 2 to row 3 column 5.
    let (from, to) = (at(&view, 0, 2, cx), at(&view, 3, 5, cx));
    cx.simulate_mouse_down(from, MouseButton::Left, alt_shift());
    let between = at(&view, 2, 4, cx);
    cx.simulate_mouse_move(between, MouseButton::Left, alt_shift());
    cx.simulate_mouse_move(to, MouseButton::Left, alt_shift());
    cx.simulate_mouse_up(to, MouseButton::Left, alt_shift());
    cx.run_until_parked();
    let boxed: Vec<_> = (0..4).map(|row| r(row, 2..5)).collect();
    assert_eq!(selections(&view, cx), boxed);

    // Typing edits every row, as one undo step.
    keys("x", cx);
    assert_eq!(text(&view, cx), "01x56789\nabxfgh\nABxFGH\nklxpqrstu\nxy\n");
    keys("cmd-z", cx);
    assert_eq!(text(&view, cx), GRID);

    // Past a short row's end the column goes on: from row 3 column 1 to
    // row 4 (`xy`) column 6.
    let (from, to) = (at(&view, 3, 1, cx), at(&view, 4, 6, cx));
    cx.simulate_mouse_down(from, MouseButton::Left, alt_shift());
    cx.simulate_mouse_move(to, MouseButton::Left, alt_shift());
    cx.simulate_mouse_up(to, MouseButton::Left, alt_shift());
    cx.run_until_parked();
    assert_eq!(selections(&view, cx), [r(3, 1..6), r(4, 1..2)]);
    assert_eq!(*seen.borrow(), []);

    // Once alt-click is bound, it runs its command, the caret moving there
    // as for any bound click; none is added.
    cx.update(|_, cx| set_user_keymap(cx, keymap(Some(ALT_CLICK))));
    let position = at(&view, 1, 3, cx);
    cx.simulate_click(position, alt());
    cx.run_until_parked();
    assert_eq!(selections(&view, cx), [r(1, 3..3)]);
    let seen = std::mem::take(&mut *seen.borrow_mut());
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert_eq!(seen[0].command.as_str(), "test.jump");
}

#[gpui::test]
fn a_box_keeps_its_column_below_the_last_row(cx: &mut TestAppContext) {
    // The last row short, with no newline after it.
    let text = "abcdefgh\nxy";
    let (view, _, cx) = editor(text, None, cx);
    let r = |start: usize, end: usize| selection(start..end);

    // From row 0 column 1 to row 1 column 6, then straight down into the
    // blank space below the text: the box stays six columns wide.
    let (from, to) = (at(&view, 0, 1, cx), at(&view, 1, 6, cx));
    let boxed = [r(1, 6), r(10, 11)];
    cx.simulate_mouse_down(from, MouseButton::Left, alt_shift());
    cx.simulate_mouse_move(to, MouseButton::Left, alt_shift());
    cx.run_until_parked();
    assert_eq!(selections(&view, cx), boxed);
    let line = view.read_with(cx, |view, _| {
        px(view.layout.as_ref().unwrap().frame.line_height)
    });
    let below = to + point(px(0.), line * 3.);
    cx.simulate_mouse_move(below, MouseButton::Left, alt_shift());
    cx.simulate_mouse_up(below, MouseButton::Left, alt_shift());
    cx.run_until_parked();
    assert_eq!(selections(&view, cx), boxed);
}
