//! An input method composing in the editor when the breakpoint menu opens
//! (Mac T1881 F7, T1888): the menu takes its own focus, so the window's
//! input handler is none while it's open, and the composition is settled
//! at open. GPUI's test platform runs KeyDown before offering a key to the
//! input handler, the reverse of AppKit's; [`mac_key`] models AppKit's
//! order.

use std::{cell::RefCell, rc::Rc};

use fe_buffer::{Buffer, ResolvedSelection};
use fe_editor::EditorModel;
use fe_gpui::{BreakpointHandlers, BreakpointMenuItem, DrawnGutter, EditorTheme, EditorView};
use gpui::{
    Context, Entity, EntityInputHandler as _, Focusable as _, Modifiers, MouseButton,
    MouseDownEvent, MouseUpEvent, Pixels, PlatformInput, Point, TestAppContext, VisualTestContext,
    Window, point, px,
};

/// Twelve rows: `fn f`, ten calls and a closing brace.
fn text() -> String {
    let mut text = String::from("fn f() {\n");
    for n in 0..10 {
        text.push_str(&format!("    call_{n}();\n"));
    }
    text.push_str("}\n");
    text
}

/// A focused Rust editor on [`text`] in the dark theme, the caret at the
/// end of row 1, drawn.
fn editor(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    cx.update(fe_gpui::init);
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let text = text();
        let mut model = EditorModel::new(Buffer::new(&text), Some("main.rs".into()));
        let caret = text.find("();").unwrap() + 3;
        model.set_selections(vec![ResolvedSelection {
            range: caret..caret,
            reversed: false,
        }]);
        let mut theme = EditorTheme::default_dark(cx);
        theme.font_size = px(13.);
        EditorView::new(model, theme, cx)
    });
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&view.focus_handle(cx), cx);
    });
    cx.run_until_parked();
    draw(cx);
    (view, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn drawn(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> DrawnGutter {
    view.read_with(cx, |view, _| view.drawn_gutter().expect("drawn").clone())
}

/// The middle of buffer row `row`'s line-number column.
fn number_at(view: &Entity<EditorView>, row: u32, cx: &mut VisualTestContext) -> Point<Pixels> {
    let drawn = drawn(view, cx);
    let column = drawn.columns[0].union(&drawn.columns[1]);
    let y = view.update(cx, |view, _| {
        let offset = view.model().text().row_start(row);
        view.caret_bounds_at(offset)
            .expect("row in view")
            .center()
            .y
    });
    point(column.center().x, y)
}
/// A host whose menu offers two items, each recorded by its index when
/// run.
fn host_with_menu(
    view: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> Rc<RefCell<Vec<usize>>> {
    let chosen: Rc<RefCell<Vec<usize>>> = Rc::default();
    let items = {
        let chosen = chosen.clone();
        move |_: u32, _: &gpui::App| -> Vec<BreakpointMenuItem> {
            (0..2)
                .map(|ix| {
                    let chosen = chosen.clone();
                    BreakpointMenuItem {
                        title: format!("Item {ix}").into(),
                        run: Rc::new(move |_, _| chosen.borrow_mut().push(ix)),
                    }
                })
                .collect()
        }
    };
    let handlers = BreakpointHandlers {
        on_toggle: Rc::new(|_, _, _| {}),
        menu: Rc::new(items),
    };
    view.update(cx, |view, cx| {
        view.set_breakpoint_handlers(Some(handlers), cx)
    });
    draw(cx);
    chosen
}

/// Right-clicks row 3's number: the menu opens.
fn open_menu(view: &Entity<EditorView>, cx: &mut VisualTestContext) {
    let at = number_at(view, 3, cx);
    cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::none());
    cx.simulate_mouse_up(at, MouseButton::Right, Modifiers::none());
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |view, _| view.breakpoint_menu().is_some()));
}

fn text_of(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |view, _| view.model().text().to_string())
}

fn focused(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| view.focus_handle(cx).is_focused(window))
}

fn marked(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<std::ops::Range<usize>> {
    view.update_in(cx, |view, window, cx| view.marked_text_range(window, cx))
}

/// A key as macOS routes it (gpui-pre-macos `handle_key_down`): with the
/// editor's input handler the window's, which it is only while the
/// editor's focus handle is focused, and text marked, the input method
/// takes the key before GPUI's KeyDown: Enter commits the marked text,
/// Up and Down change the candidate. Otherwise it's a keystroke.
fn mac_key(view: &Entity<EditorView>, key: &str, cx: &mut VisualTestContext) {
    let composing = match focused(view, cx) {
        true => marked(view, cx),
        false => None,
    };
    let Some(range) = composing else {
        cx.simulate_keystrokes(key);
        cx.run_until_parked();
        draw(cx);
        return;
    };
    view.update_in(cx, |view, window, cx| match key {
        "enter" => {
            let text = view
                .text_for_range(range, &mut None, window, cx)
                .unwrap_or_default();
            view.replace_text_in_range(None, &text, window, cx)
        }
        "up" | "down" => view.replace_and_mark_text_in_range(None, "日本", None, window, cx),
        _ => {}
    });
    cx.run_until_parked();
    draw(cx);
}

#[gpui::test]
fn opening_the_breakpoint_menu_mid_composition_settles_it_and_the_menu_gets_the_keys(
    cx: &mut TestAppContext,
) {
    let (view, cx) = editor(cx);
    let chosen = host_with_menu(&view, cx);
    let before = text_of(&view, cx);
    view.update_in(cx, |view, window, cx| {
        view.replace_and_mark_text_in_range(None, "にほ", None, window, cx)
    });
    draw(cx);
    assert!(marked(&view, cx).is_some());

    open_menu(&view, cx);
    assert_eq!(marked(&view, cx), None);
    assert_eq!(text_of(&view, cx), before);
    assert!(!focused(&view, cx));

    mac_key(&view, "down", cx);
    mac_key(&view, "enter", cx);
    assert_eq!(*chosen.borrow(), [1]);
    assert_eq!(text_of(&view, cx), before);
    assert!(view.read_with(cx, |view, _| view.breakpoint_menu().is_none()));
    assert!(focused(&view, cx));

    open_menu(&view, cx);
    mac_key(&view, "escape", cx);
    assert!(view.read_with(cx, |view, _| view.breakpoint_menu().is_none()));
    assert!(focused(&view, cx));
    view.update_in(cx, |view, window, cx| {
        view.replace_and_mark_text_in_range(None, "にほ", None, window, cx);
        view.replace_text_in_range(None, "日本", window, cx)
    });
    draw(cx);
    assert!(text_of(&view, cx).contains("日本"));
}

/// What the probe read: whether the menu was open, the text, the marked range.
type Seen = Rc<RefCell<Option<(bool, String, Option<std::ops::Range<usize>>)>>>;

/// Input-method calls after the menu opens but before a frame is drawn,
/// while the platform may still hold the editor's handler, type and
/// compose nothing.
#[gpui::test]
fn input_method_calls_before_the_first_frame_do_nothing(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    host_with_menu(&view, cx);
    let before = text_of(&view, cx);
    let position = number_at(&view, 3, cx);
    let seen: Seen = Rc::default();
    let sink = seen.clone();
    let handle = view.clone();
    cx.update(|window, cx| {
        for input in [
            PlatformInput::MouseDown(MouseDownEvent {
                button: MouseButton::Right,
                position,
                modifiers: Modifiers::none(),
                click_count: 1,
                first_mouse: false,
            }),
            PlatformInput::MouseUp(MouseUpEvent {
                button: MouseButton::Right,
                position,
                modifiers: Modifiers::none(),
                click_count: 1,
            }),
        ] {
            window.dispatch_event(input, cx);
        }
        // After the view's own deferred open.
        window.defer(cx, move |window: &mut Window, cx| {
            handle.update(cx, |view, cx: &mut Context<EditorView>| {
                let open = view.breakpoint_menu().is_some();
                view.replace_and_mark_text_in_range(None, "かな", Some(2..2), window, cx);
                view.replace_text_in_range(None, "typed", window, cx);
                let marked = view.marked_text_range(window, cx);
                *sink.borrow_mut() = Some((open, view.model().text().to_string(), marked));
            })
        });
    });
    let (open, text, marked) = seen.borrow_mut().take().expect("read");
    assert!(open);
    assert_eq!(text, before);
    assert_eq!(marked, None);
    draw(cx);
    assert_eq!(text_of(&view, cx), before);
}

/// New handlers take an open menu away: its items are the old handlers'.
#[gpui::test]
fn replacing_the_handlers_closes_the_open_menu(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let old = host_with_menu(&view, cx);
    open_menu(&view, cx);
    let new = host_with_menu(&view, cx);
    draw(cx);
    assert!(view.read_with(cx, |view, _| view.breakpoint_menu().is_none()));
    assert!(focused(&view, cx));
    view.update_in(cx, |view, window, cx| {
        view.choose_breakpoint_menu_item(0, window, cx)
    });
    cx.run_until_parked();
    assert!(old.borrow().is_empty() && new.borrow().is_empty());
}
