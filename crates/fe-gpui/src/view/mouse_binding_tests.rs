//! A modified click bound in the keymap, such as `cmd-click`, in a headless
//! window: it places the caret and dispatches its command, while the word
//! under the pointer is underlined with the modifier held. Unbound clicks
//! behave as before.

use std::{cell::RefCell, ops::Range, path::Path, rc::Rc};

use fe_buffer::{Buffer, ResolvedSelection};
use fe_command::{Invocation, Source};
use fe_editor::EditorModel;
use fe_keymap::{DEFAULT_KEYMAP_PATH, DEFAULT_MACOS, Keymap, KeymapLayer};
use gpui::{
    AppContext as _, Context, Entity, FocusHandle, Focusable as _, InteractiveElement as _,
    IntoElement, Modifiers, MouseButton, MouseDownEvent, MouseExitEvent, ParentElement as _,
    Pixels, Point, Render, ScrollDelta, ScrollWheelEvent, Styled as _, TestAppContext, TouchPhase,
    VisualTestContext, Window, div, point, px,
};

use crate::{commands::RunCommand, *};

use gpui::App;

/// `main` calls `helper`, on row 1; row 0 heads a fold candidate.
const TEXT: &str = "fn main() {\n    helper();\n}\n\nfn helper() {}\n";

/// The test layer: `cmd-click` runs `test.jump` in the editor.
const CMD_CLICK: &str = "\
[[bindings]]
keys = \"cmd-click\"
command = \"test.jump\"
context = \"editor\"
";

/// Editors side by side, as splits, with the commands that propagate past
/// them recorded, and a focusable strip that isn't an editor, as a panel.
struct Host {
    editors: Vec<Entity<EditorView>>,
    seen: Rc<RefCell<Vec<Invocation>>>,
    panel: FocusHandle,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let seen = self.seen.clone();
        let editors =
            (self.editors.iter()).map(|editor| div().flex_1().h_full().child(editor.clone()));
        div()
            .size_full()
            .flex()
            .on_action(move |run: &RunCommand, _, _| seen.borrow_mut().push(run.0.clone()))
            .children(editors)
            .child(div().w(px(10.)).h_full().track_focus(&self.panel))
    }
}

/// An editor on `text`, as `main.rs`.
fn new_editor(text: &str, cx: &mut App) -> Entity<EditorView> {
    let model = EditorModel::new(Buffer::new(text), Some("main.rs".into()));
    cx.new(|cx| EditorView::new(model, EditorTheme::default_dark(cx), cx))
}

/// A focused Rust editor on [`TEXT`], drawn once parsed, the caret at 0,
/// with the default keymap and, if `layer` is given, that layer over it,
/// in an active window.
#[allow(clippy::type_complexity)]
fn editor<'a>(
    layer: Option<&str>,
    cx: &'a mut TestAppContext,
) -> (
    Entity<EditorView>,
    Rc<RefCell<Vec<Invocation>>>,
    &'a mut VisualTestContext,
) {
    editor_on(TEXT, false, layer, cx)
}

/// [`editor`] on `text`, soft wrapped if `soft_wrap`.
#[allow(clippy::type_complexity)]
fn editor_on<'a>(
    text: &str,
    soft_wrap: bool,
    layer: Option<&str>,
    cx: &'a mut TestAppContext,
) -> (
    Entity<EditorView>,
    Rc<RefCell<Vec<Invocation>>>,
    &'a mut VisualTestContext,
) {
    let (default, _) = KeymapLayer::parse(Path::new(DEFAULT_KEYMAP_PATH), DEFAULT_MACOS, None);
    let mut layers = vec![default.unwrap()];
    if let Some(source) = layer {
        let (layer, diagnostics) = KeymapLayer::parse(Path::new("keymap.toml"), source, None);
        assert_eq!(diagnostics, []);
        layers.push(layer.unwrap());
    }
    let keymap = Keymap::from_layers(layers);
    cx.update(|cx| init_with_keymap(cx, keymap));
    let seen = Rc::new(RefCell::new(Vec::new()));
    let (host, cx) = cx.add_window_view({
        let (seen, text) = (seen.clone(), text.to_owned());
        move |_, cx| {
            let editor = new_editor(&text, cx);
            editor.update(cx, |editor, _| editor.model_mut().set_soft_wrap(soft_wrap));
            Host {
                editors: vec![editor],
                seen,
                panel: cx.focus_handle(),
            }
        }
    });
    let view = host.read_with(cx, |host, _| host.editors[0].clone());
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&view.focus_handle(cx), cx);
    });
    cx.run_until_parked();
    let parsed = view.update(cx, |view, _| {
        view.syntax().and_then(fe_syntax::Syntax::tree).is_some()
    });
    assert!(parsed);
    (view, seen, cx)
}

/// Just right of the caret position at `offset`, mid-row, in the last frame.
fn at(view: &Entity<EditorView>, offset: usize, cx: &mut VisualTestContext) -> Point<Pixels> {
    view.update(cx, |view, _| {
        let caret = view.layout.as_mut().unwrap().caret_bounds(offset).unwrap();
        point(caret.left() + px(1.), caret.center().y)
    })
}

/// The start of the `n`th `needle` in [`TEXT`].
fn nth(needle: &str, n: usize) -> usize {
    TEXT.match_indices(needle).nth(n).unwrap().0
}

fn selections(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<ResolvedSelection> {
    view.update(cx, |view, _| view.model().selections())
}

fn caret(offset: usize) -> Vec<ResolvedSelection> {
    vec![ResolvedSelection {
        range: offset..offset,
        reversed: false,
    }]
}

/// The hover range, and whether the last frame underlined it under each of
/// its pieces and showed the pointing hand.
fn hover(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<Range<usize>> {
    view.update(cx, |view, _| {
        let range = view.hover_range();
        let (underlines, hand) = &view.painted_hover;
        assert_eq!(*hand, range.is_some());
        assert_eq!(!underlines.is_empty(), range.is_some());
        range
    })
}

fn move_to(position: Point<Pixels>, modifiers: Modifiers, cx: &mut VisualTestContext) {
    cx.simulate_mouse_move(position, None, modifiers);
    cx.run_until_parked();
}

#[gpui::test]
fn bound_click_dispatches_a_command(cx: &mut TestAppContext) {
    let (view, seen, cx) = editor(Some(CMD_CLICK), cx);
    let clicked = nth("helper", 0) + 2;
    let position = at(&view, clicked, cx);
    cx.simulate_mouse_down(position, MouseButton::Left, Modifiers::command());
    cx.run_until_parked();

    assert_eq!(selections(&view, cx), caret(clicked));
    assert!(view.read_with(cx, |view, _| view.drag.is_none()));
    let seen = std::mem::take(&mut *seen.borrow_mut());
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert_eq!(seen[0].command.as_str(), "test.jump");
    assert_eq!(seen[0].source, Source::Mouse);

    // Moving with the button still held selects nothing.
    let further = at(&view, nth("helper", 1) + 3, cx);
    cx.simulate_mouse_move(further, MouseButton::Left, Modifiers::command());
    cx.run_until_parked();
    cx.simulate_mouse_up(further, MouseButton::Left, Modifiers::command());
    cx.run_until_parked();
    assert_eq!(selections(&view, cx), caret(clicked));
}

#[gpui::test]
fn unbound_clicks_are_unchanged(cx: &mut TestAppContext) {
    // The default keymap binds cmd-click only in the workbench.
    let (view, seen, cx) = editor(None, cx);
    let clicked = nth("helper", 0) + 2;
    let position = at(&view, clicked, cx);
    cx.simulate_click(position, Modifiers::command());
    cx.run_until_parked();
    assert_eq!(selections(&view, cx), caret(clicked));

    let extended = nth("helper", 1) + 3;
    let position = at(&view, extended, cx);
    cx.simulate_click(position, Modifiers::shift());
    cx.run_until_parked();
    let selection = ResolvedSelection {
        range: clicked..extended,
        reversed: false,
    };
    assert_eq!(selections(&view, cx), [selection]);

    let word = nth("main", 0);
    let position = at(&view, word + 1, cx);
    for click_count in 1..=2 {
        cx.simulate_event(MouseDownEvent {
            position,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count,
            first_mouse: false,
        });
        cx.simulate_mouse_up(position, MouseButton::Left, Modifiers::none());
    }
    cx.run_until_parked();
    let selection = ResolvedSelection {
        range: word..word + "main".len(),
        reversed: false,
    };
    assert_eq!(selections(&view, cx), [selection]);

    // The fold column's toggle on row 0.
    let toggle = view.update(cx, |view, _| {
        let layout = view.layout.as_ref().unwrap();
        let area = layout.frame.fold_click_areas().next().unwrap();
        assert_eq!(area.buffer_row, 0);
        layout.bounds(area.rect).center()
    });
    cx.simulate_click(toggle, Modifiers::command());
    cx.run_until_parked();
    let folds = view.update(cx, |view, _| view.model().display().folds().len());
    assert_eq!(folds, 1);

    assert_eq!(*seen.borrow(), []);
}

#[gpui::test]
fn hover_underlines_the_word_under_cmd(cx: &mut TestAppContext) {
    let (view, _, cx) = editor(Some(CMD_CLICK), cx);
    let helper = nth("helper", 0)..nth("helper", 0) + "helper".len();
    let over = at(&view, helper.start + 2, cx);
    let cmd = Modifiers::command();

    move_to(over, Modifiers::none(), cx);
    assert_eq!(hover(&view, cx), None, "cmd isn't held");
    move_to(over, cmd, cx);
    assert_eq!(hover(&view, cx), Some(helper.clone()));
    // Under the word, from its start to its end.
    let (underline, start, end) = view.update(cx, |view, _| {
        let layout = view.layout.as_mut().unwrap();
        let start = layout.caret_bounds(helper.start).unwrap();
        let end = layout.caret_bounds(helper.end).unwrap();
        (view.painted_hover.0[0], start, end)
    });
    assert_eq!(underline.left(), start.left());
    assert_eq!(underline.right(), end.left());
    assert_eq!(underline.size.height, px(1.));
    assert!(start.top() < underline.top() && underline.bottom() <= start.bottom());

    // Over the right half of its last glyph, nearer the `(` after it.
    let last = view.update(cx, |view, _| {
        let layout = view.layout.as_mut().unwrap();
        let end = layout.caret_bounds(helper.end).unwrap();
        point(end.left() - px(1.), end.center().y)
    });
    move_to(last, cmd, cx);
    assert_eq!(
        hover(&view, cx),
        Some(helper.clone()),
        "over the last glyph"
    );
    move_to(over, cmd, cx);

    // Pressing and releasing cmd without moving.
    cx.simulate_modifiers_change(Modifiers::none());
    cx.run_until_parked();
    assert_eq!(hover(&view, cx), None, "cmd released");
    cx.simulate_modifiers_change(cmd);
    cx.run_until_parked();
    assert_eq!(hover(&view, cx), Some(helper.clone()), "cmd pressed");

    // Onto the indentation, then past the row's end.
    move_to(at(&view, nth("helper", 0) - 2, cx), cmd, cx);
    assert_eq!(hover(&view, cx), None, "over whitespace");
    move_to(over, cmd, cx);
    let past = view.update(cx, |view, _| {
        let layout = view.layout.as_ref().unwrap();
        layout.bounds(layout.frame.text_bounds()).right() - px(2.)
    });
    move_to(point(past, over.y), cmd, cx);
    assert_eq!(hover(&view, cx), None, "past the row's end");

    // Leaving the view, editing and scrolling each clear it.
    move_to(over, cmd, cx);
    cx.simulate_event(MouseExitEvent {
        position: over,
        pressed_button: None,
        modifiers: cmd,
    });
    cx.run_until_parked();
    assert_eq!(hover(&view, cx), None, "left the window");
    move_to(over, cmd, cx);
    let gutter = view.update(cx, |view, _| {
        let layout = view.layout.as_ref().unwrap();
        layout.bounds(layout.frame.fold_column).center()
    });
    move_to(point(gutter.x, over.y), cmd, cx);
    assert_eq!(hover(&view, cx), None, "over the gutter");

    move_to(over, cmd, cx);
    assert_eq!(hover(&view, cx), Some(helper.clone()));
    view.update(cx, |view, cx| {
        let mut buffer = view.model_mut().buffer_mut();
        buffer.edit([(TEXT.len()..TEXT.len(), "x")], fe_buffer::Origin::User);
        drop(buffer);
        cx.notify();
    });
    cx.run_until_parked();
    assert_eq!(hover(&view, cx), None, "edited");

    move_to(over, cmd, cx);
    assert_eq!(hover(&view, cx), Some(helper.clone()));
    cx.simulate_event(ScrollWheelEvent {
        position: over,
        delta: ScrollDelta::Pixels(point(px(0.), px(-10.))),
        modifiers: cmd,
        touch_phase: TouchPhase::Moved,
    });
    cx.run_until_parked();
    assert_eq!(hover(&view, cx), None, "scrolled");
    // Where the word is now that the text has moved.
    let over = at(&view, helper.start + 2, cx);

    // So do focus leaving the view and the window going inactive, as when
    // cmd-tab releases cmd in another app.
    move_to(over, cmd, cx);
    assert_eq!(hover(&view, cx), Some(helper.clone()));
    cx.update(|window, cx| window.blur(cx));
    cx.run_until_parked();
    assert_eq!(hover(&view, cx), None, "unfocused");
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.run_until_parked();
    assert_eq!(hover(&view, cx), None, "refocused, until the pointer moves");
    move_to(over, cmd, cx);
    assert_eq!(hover(&view, cx), Some(helper.clone()));
    cx.deactivate_window();
    cx.run_until_parked();
    assert_eq!(hover(&view, cx), None, "the window went inactive");

    // Focus going to another tab's editor, while this one isn't painted,
    // clears it too: back on this one, no underline until the pointer moves.
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    move_to(over, cmd, cx);
    assert_eq!(hover(&view, cx), Some(helper.clone()));
    let host = cx.update(|window, _| window.root::<Host>().flatten().unwrap());
    let show = |editor: &Entity<EditorView>, cx: &mut VisualTestContext| {
        // In one update, as a pane switches tabs, so no frame draws this
        // editor unfocused in between.
        cx.update(|window, cx| {
            host.update(cx, |host, cx| {
                host.editors = vec![editor.clone()];
                cx.notify();
            });
            window.focus(&editor.focus_handle(cx), cx);
        });
        cx.run_until_parked();
    };
    let other = cx.update(|_, cx| new_editor("other", cx));
    show(&other, cx);
    show(&view, cx);
    assert_eq!(hover(&view, cx), None, "back from another tab");

    // The still pointer: a fold moves the word from under it.
    move_to(over, cmd, cx);
    assert_eq!(hover(&view, cx), Some(helper.clone()));
    view.update(cx, |view, cx| {
        view.model_mut().fold_rows(0..=2);
        cx.notify();
    });
    cx.run_until_parked();
    assert_eq!(hover(&view, cx), None, "folded away");
    view.update(cx, |view, cx| {
        view.model_mut().unfold_rows(0..=2);
        cx.notify();
    });
    cx.run_until_parked();

    // With cmd-click unbound here, as the default keymap binds it only in
    // the workbench: at once under the still pointer, then on moves and
    // modifier changes.
    move_to(over, cmd, cx);
    assert_eq!(hover(&view, cx), Some(helper.clone()));
    cx.update(|_, cx| set_user_keymap(cx, default_keymap()));
    cx.run_until_parked();
    assert_eq!(hover(&view, cx), None, "unbound");
    move_to(at(&view, helper.start + 3, cx), cmd, cx);
    assert_eq!(hover(&view, cx), None, "unbound");
    cx.simulate_modifiers_change(Modifiers::none());
    cx.simulate_modifiers_change(cmd);
    cx.run_until_parked();
    assert_eq!(hover(&view, cx), None, "unbound");
}

#[gpui::test]
fn hover_spans_soft_wrapped_rows(cx: &mut TestAppContext) {
    // One word wrapped over many more rows than the window shows.
    let word = "a".repeat(20_000);
    let text = format!("x {word}\n");
    let (view, _, cx) = editor_on(&text, true, Some(CMD_CLICK), cx);
    let (position, rows) = view.update(cx, |view, _| {
        let layout = view.layout.as_ref().unwrap();
        let frame = &layout.frame;
        assert!(frame.rows.len() > 3 && frame.rows.last().unwrap().end < text.len() - 1);
        let row = frame.row(2).unwrap();
        let line = layout.bounds(fe_display::layout::Rect {
            x: frame.text_left + 20.,
            y: frame.row_top(2),
            width: 1.,
            height: frame.line_height,
        });
        assert!(row.start > 2 && row.end < text.len() - 1);
        // Row 0 is `x ` alone.
        let shown = frame.rows.iter().filter(|row| row.end > 2).count();
        (line.center(), shown)
    });
    move_to(position, Modifiers::command(), cx);
    assert_eq!(hover(&view, cx), Some(2..2 + word.len()));
    // Underlined on each row shown, wherever it starts and ends.
    let underlines = view.update(cx, |view, _| view.painted_hover.0.len());
    assert_eq!(underlines, rows);
}

#[gpui::test]
fn hover_in_a_split_that_is_not_focused(cx: &mut TestAppContext) {
    let (view, seen, cx) = editor(Some(CMD_CLICK), cx);
    // A split beside it takes focus; the first is never focused again.
    let host = cx.update(|window, _| window.root::<Host>().flatten().unwrap());
    let other = cx.update(|_, cx| new_editor(TEXT, cx));
    cx.update(|window, cx| {
        host.update(cx, |host, cx| {
            host.editors.push(other.clone());
            cx.notify();
        });
        window.focus(&other.focus_handle(cx), cx);
    });
    cx.run_until_parked();
    let helper = nth("helper", 0)..nth("helper", 0) + "helper".len();
    let over = at(&view, helper.start + 2, cx);
    assert!(over.x < at(&other, 0, cx).x, "side by side");

    move_to(over, Modifiers::command(), cx);
    assert_eq!(hover(&view, cx), Some(helper.clone()));
    assert_eq!(hover(&other, cx), None);
    // The focused split hears the modifiers, and tells this one.
    cx.simulate_modifiers_change(Modifiers::none());
    cx.run_until_parked();
    assert_eq!(hover(&view, cx), None, "cmd released");
    cx.simulate_modifiers_change(Modifiers::command());
    cx.run_until_parked();
    assert_eq!(hover(&view, cx), Some(helper.clone()), "cmd pressed");

    // A click there focuses it, then runs the command.
    cx.simulate_click(over, Modifiers::command());
    cx.run_until_parked();
    assert!(cx.update(|window, cx| view.focus_handle(cx).is_focused(window)));
    assert_eq!(seen.borrow().len(), 1);
}

#[gpui::test]
fn click_lookup_reads_the_live_vim_state(cx: &mut TestAppContext) {
    let layer = "\
[[bindings]]
keys = \"cmd-click\"
command = \"test.jump\"
context = \"editor && vim.mode == normal\"
";
    let (view, seen, cx) = editor(Some(layer), cx);
    let position = at(&view, nth("helper", 0) + 2, cx);
    // Vim turns on and the click comes before any frame shows it.
    cx.update(|window, cx| {
        set_vim_enabled(cx, true);
        window.dispatch_event(
            gpui::PlatformInput::MouseDown(MouseDownEvent {
                position,
                modifiers: Modifiers::command(),
                button: MouseButton::Left,
                click_count: 1,
                first_mouse: false,
            }),
            cx,
        );
    });
    cx.run_until_parked();
    let seen = seen.borrow();
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert_eq!(seen[0].command.as_str(), "test.jump");
}

#[gpui::test]
fn cmd_released_while_a_panel_has_focus_clears_the_hover(cx: &mut TestAppContext) {
    let (view, _, cx) = editor(Some(CMD_CLICK), cx);
    let host = cx.update(|window, _| window.root::<Host>().flatten().unwrap());
    let panel = host.read_with(cx, |host, _| host.panel.clone());
    cx.update(|window, cx| window.focus(&panel, cx));
    cx.run_until_parked();
    let helper = nth("helper", 0)..nth("helper", 0) + "helper".len();
    let over = at(&view, helper.start + 2, cx);

    cx.simulate_modifiers_change(Modifiers::command());
    move_to(over, Modifiers::command(), cx);
    assert_eq!(hover(&view, cx), Some(helper));
    // Only the panel hears the modifier change; the release alone is a
    // keystroke, which every window's interceptor sees.
    cx.simulate_modifiers_change(Modifiers::none());
    cx.run_until_parked();
    assert_eq!(hover(&view, cx), None);
}

#[gpui::test]
fn hover_over_a_composed_glyph(cx: &mut TestAppContext) {
    // `e` and a combining acute accent, shaped as one glyph.
    let text = "let cafe\u{301} = 1;\n";
    let (view, _, cx) = editor_on(text, false, Some(CMD_CLICK), cx);
    let e = text.find("e\u{301}").unwrap();
    let after = e + "e\u{301}".len();
    // Over the right half of the glyph, nearer the boundary after it.
    let position = view.update(cx, |view, _| {
        let layout = view.layout.as_mut().unwrap();
        let end = layout.caret_bounds(after).unwrap();
        point(end.left() - px(1.), end.center().y)
    });
    move_to(position, Modifiers::command(), cx);
    let range = hover(&view, cx).expect("over the word");
    assert!(range.contains(&e), "{range:?}");
}

#[gpui::test]
fn hover_ignores_a_frame_older_than_the_text(cx: &mut TestAppContext) {
    let (view, _, cx) = editor(Some(CMD_CLICK), cx);
    let over = at(&view, nth("helper", 1) + 2, cx);
    // The text empties, and the pointer moves before any frame shows it.
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let mut buffer = view.model_mut().buffer_mut();
            buffer.edit([(0..TEXT.len(), String::new())], fe_buffer::Origin::User);
            drop(buffer);
            cx.notify();
        });
        window.dispatch_event(
            gpui::PlatformInput::MouseMove(gpui::MouseMoveEvent {
                position: over,
                pressed_button: None,
                modifiers: Modifiers::command(),
            }),
            cx,
        );
        assert_eq!(view.read(cx).hover_range(), None);
    });
    cx.run_until_parked();
    assert_eq!(hover(&view, cx), None);
}
