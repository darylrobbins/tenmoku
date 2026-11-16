//! The breakpoint menu's keyboard and a screen reader's Click on a row
//! (T1849 R3 F15, F17): in-crate, since they read what the view
//! records of its rendered nodes.

use std::{cell::RefCell, rc::Rc};

use fe_buffer::{Buffer, ResolvedSelection};
use fe_editor::EditorModel;
use gpui::{
    Entity, Focusable as _, Modifiers, MouseButton, Pixels, Point, TestAppContext,
    VisualTestContext, point, px,
};

use crate::{
    BreakpointGlyph, BreakpointHandlers, BreakpointMark, BreakpointMenuItem, BreakpointState,
    DrawnGutter, EditorTheme, EditorView, FrameArrow,
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
    cx.update(crate::init);
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

fn mark(
    row: u32,
    breakpoint: Option<BreakpointState>,
    frame: Option<FrameArrow>,
) -> BreakpointMark {
    BreakpointMark {
        row,
        breakpoint,
        frame,
        target: None,
    }
}

/// Handlers that keep a set of enabled breakpoints as a host's store
/// would, showing it on each toggle; the rows toggled, in order.
fn host(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Rc<RefCell<Vec<u32>>> {
    let toggled = Rc::new(RefCell::new(Vec::new()));
    let set: Rc<RefCell<Vec<u32>>> = Rc::default();
    let handle = view.downgrade();
    let handlers = BreakpointHandlers {
        on_toggle: Rc::new({
            let toggled = toggled.clone();
            move |row, _, cx| {
                toggled.borrow_mut().push(row);
                let mut rows = set.borrow_mut();
                match rows.iter().position(|r| *r == row) {
                    Some(at) => {
                        rows.remove(at);
                    }
                    None => rows.push(row),
                }
                let marks = (rows.iter())
                    .map(|&row| mark(row, Some(BreakpointState::Enabled), None))
                    .collect();
                (handle.update(cx, |view, cx| {
                    let version = view.model().snapshot().version();
                    view.set_breakpoints(version, marks, cx);
                }))
                .ok();
            }
        }),
        menu: Rc::new(|_, _| Vec::<BreakpointMenuItem>::new()),
    };
    view.update(cx, |view, cx| {
        view.set_breakpoint_handlers(Some(handlers), cx)
    });
    draw(cx);
    toggled
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
/// The glyphs drawn, as `(row, glyph)`.
fn glyphs(drawn: &DrawnGutter) -> Vec<(u32, BreakpointGlyph)> {
    (drawn.breakpoints.iter())
        .map(|drawn| (drawn.row, drawn.glyph))
        .collect()
}

/// the rendered node was given, toggles a breakpoint there, as a click
/// does. A row without a node has none.
#[gpui::test]
fn a_screen_readers_click_on_a_row_toggles_it(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let toggled = host(&view, cx);
    let click = view.read_with(cx, |view, _| view.a11y_click_of_breakpoint_row(4));
    let nowhere = view.read_with(cx, |view, _| view.a11y_click_of_breakpoint_row(40));
    assert!(nowhere.is_none());
    let click = click.expect("row 4's node has a Click");
    cx.update(|window, cx| click(window, cx));
    cx.run_until_parked();
    draw(cx);
    assert_eq!(*toggled.borrow(), [4]);
    assert_eq!(glyphs(&drawn(&view, cx)), [(4, BreakpointGlyph::Dot)]);
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

/// R3 F17: Escape closes the open menu and leaves the editor focused and
/// its text alone.
#[gpui::test]
fn escape_closes_the_breakpoint_menu(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let chosen = host_with_menu(&view, cx);
    let before = text_of(&view, cx);
    open_menu(&view, cx);
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(view.read_with(cx, |view, _| view.breakpoint_menu().is_none()));
    assert!(chosen.borrow().is_empty());
    assert_eq!(text_of(&view, cx), before);
    let focused = cx.update(|window, cx| view.focus_handle(cx).is_focused(window));
    assert!(focused, "the editor has focus again");
}

/// R3 F17: Down and Up move through the items, clamped, and Enter runs the
/// one the keyboard is on instead of the editor's newline.
#[gpui::test]
fn the_breakpoint_menu_takes_arrows_and_enter(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let chosen = host_with_menu(&view, cx);
    let before = text_of(&view, cx);
    let selected = |view: &Entity<EditorView>, cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| view.breakpoint_menu_selected())
    };
    open_menu(&view, cx);
    assert_eq!(selected(&view, cx), Some(0));
    cx.simulate_keystrokes("down down");
    assert_eq!(selected(&view, cx), Some(1));
    cx.simulate_keystrokes("up up");
    assert_eq!(selected(&view, cx), Some(0));
    cx.simulate_keystrokes("down");
    cx.simulate_keystrokes("enter");

    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |view, _| view.breakpoint_menu().is_none()));
    assert_eq!(text_of(&view, cx), before, "Enter didn't edit the file");
    assert_eq!(*chosen.borrow(), [1]);
    assert_eq!(text_of(&view, cx), before, "Enter didn't edit the file");
}

/// R3 F17: a printable key while the menu is open is the menu's: it
/// neither edits the file nor closes the menu.
#[gpui::test]
fn typed_keys_leave_the_open_breakpoint_menu_and_the_text_be(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let chosen = host_with_menu(&view, cx);
    let before = text_of(&view, cx);
    open_menu(&view, cx);
    cx.simulate_keystrokes("x a shift-a shift-;");
    draw(cx);
    assert_eq!(text_of(&view, cx), before);
    assert!(view.read_with(cx, |view, _| view.breakpoint_menu().is_some()));
    assert!(chosen.borrow().is_empty());
}

/// R3 F3: in vim normal mode the open menu still has every key: operators,
/// motions and shifted keys never reach vim, and Escape closes the menu
/// before it would leave a mode.
#[gpui::test]
fn vim_normal_mode_keys_do_not_reach_the_editor_behind_the_menu(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    cx.update(|_, cx| crate::vim::set_vim_enabled(cx, true));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    let chosen = host_with_menu(&view, cx);
    let before = text_of(&view, cx);
    let caret = view.read_with(cx, |view, _| view.model().primary_selection().head());
    open_menu(&view, cx);
    cx.simulate_keystrokes("d d x shift-x shift-d j l ctrl-d");
    draw(cx);
    assert_eq!(text_of(&view, cx), before);
    let now = view.read_with(cx, |view, _| view.model().primary_selection().head());
    assert_eq!(now, caret, "no motion reached vim either");
    assert!(view.read_with(cx, |view, _| view.breakpoint_menu().is_some()));
    assert!(chosen.borrow().is_empty());
}
