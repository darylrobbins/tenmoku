//! The editor's right-click menu ([`super`]): where it opens and what it
//! does to the carets, its keys and pointer, running an item, what closes
//! it, adding items later, and its focus.

use std::{cell::RefCell, ops::Range, rc::Rc};

use fe_buffer::{Buffer, Origin, ResolvedSelection};
use fe_command::{Invocation, Source};
use fe_editor::EditorModel;
use gpui::{
    AppContext as _, Context, Entity, EntityInputHandler as _, FocusHandle, Focusable as _,
    InteractiveElement as _, IntoElement, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent,
    ParentElement as _, Pixels, PlatformInput, Point, Render, ScrollDelta, ScrollWheelEvent,
    Styled as _, TestAppContext, TouchPhase, VisualTestContext, Window, div, point, px,
};

use crate::{
    BreakpointHandlers, BreakpointMenuItem, DiffSide, DiffView, EditorMenuItem, EditorMenuRequest,
    EditorTheme, EditorView, commands::RunCommand, set_editor_menu_provider,
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

/// Where `word` starts in [`text`].
fn at(word: &str) -> usize {
    text().find(word).unwrap()
}

/// The editor inside a parent that records each command the editor
/// passes on, and whether the editor had focus as it arrived.
struct Host {
    editor: Entity<EditorView>,
    seen: Rc<RefCell<Vec<(Invocation, bool)>>>,
    /// Something else to focus.
    other: FocusHandle,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let (seen, editor) = (self.seen.clone(), self.editor.clone());
        div()
            .size_full()
            .on_action(move |run: &RunCommand, window, cx| {
                let focused = editor.focus_handle(cx).is_focused(window);
                seen.borrow_mut().push((run.0.clone(), focused));
            })
            .child(self.editor.clone())
            .child(div().track_focus(&self.other).size(px(10.)))
    }
}

type Seen = Rc<RefCell<Vec<(Invocation, bool)>>>;

/// What a test reads inside the view, once.
type Probe<T> = Rc<RefCell<Option<T>>>;

/// Selections as `(start, end)`.
type Ranges = Vec<(usize, usize)>;

/// A focused Rust editor on [`text`], the caret at `call_1`'s start, in an
/// active window, drawn; and what its host saw.
fn editor(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext, Seen) {
    cx.update(crate::init);
    let seen: Seen = Rc::default();
    let sink = seen.clone();
    let (host, cx) = cx.add_window_view(move |_, cx| {
        let editor = cx.new(|cx| {
            let text = text();
            let mut model = EditorModel::new(Buffer::new(&text), Some("main.rs".into()));
            let caret = at("call_1");
            model.set_selections(vec![ResolvedSelection {
                range: caret..caret,
                reversed: false,
            }]);
            let mut theme = EditorTheme::default_dark(cx);
            theme.font_size = px(13.);
            EditorView::new(model, theme, cx)
        });
        Host {
            editor,
            seen: sink,
            other: cx.focus_handle(),
        }
    });
    let view = host.read_with(cx, |host, _| host.editor.clone());
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&view.focus_handle(cx), cx);
    });
    cx.run_until_parked();
    draw(cx);
    (view, cx, seen)
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn item(title: &'static str, command: &str, enabled: bool) -> Option<EditorMenuItem> {
    Some(EditorMenuItem {
        title: title.into(),
        shortcut: None,
        invocation: Invocation::of(command).unwrap(),
        enabled,
    })
}

/// Cut, Copy | Select All | a disabled Probe and Probe.
fn items() -> Vec<Option<EditorMenuItem>> {
    vec![
        item("Cut", "editor.cut", true),
        item("Copy", "editor.copy", true),
        None,
        item("Select All", "editor.select_all", true),
        None,
        item("Off", "test.off", false),
        item("Probe", "test.probe", true),
    ]
}

/// Registers a provider answering `entries`; the requests it's asked.
fn provide(
    cx: &mut VisualTestContext,
    entries: Vec<Option<EditorMenuItem>>,
) -> Rc<RefCell<Vec<EditorMenuRequest>>> {
    let asked: Rc<RefCell<Vec<EditorMenuRequest>>> = Rc::default();
    let sink = asked.clone();
    cx.update(|_, cx| {
        set_editor_menu_provider(
            cx,
            Rc::new(move |request, _, _| {
                sink.borrow_mut().push(request.clone());
                entries.clone()
            }),
        )
    });
    asked
}

/// The middle of the caret at `offset`, in the window.
fn point_at(view: &Entity<EditorView>, offset: usize, cx: &mut VisualTestContext) -> Point<Pixels> {
    view.update(cx, |view, _| {
        view.caret_bounds_at(offset).expect("on screen").center()
    })
}

/// A right press and release at `offset`, without drawing after.
fn right_press(view: &Entity<EditorView>, offset: usize, cx: &mut VisualTestContext) {
    let position = point_at(view, offset, cx);
    cx.simulate_mouse_down(position, MouseButton::Right, Modifiers::none());
    cx.simulate_mouse_up(position, MouseButton::Right, Modifiers::none());
    cx.run_until_parked();
}

/// A right press and release at `offset`, then `then` on the view once the
/// menu has opened, before any frame is drawn.
fn right_press_then(
    view: &Entity<EditorView>,
    offset: usize,
    cx: &mut VisualTestContext,
    then: impl FnOnce(&mut EditorView, &mut Window, &mut Context<EditorView>) + 'static,
) {
    let position = point_at(view, offset, cx);
    let view = view.clone();
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
        window.defer(cx, move |window, cx| {
            view.update(cx, |view, cx| then(view, window, cx))
        });
    });
    cx.run_until_parked();
}

/// A right click at `offset`, drawn after.
fn right_click(view: &Entity<EditorView>, offset: usize, cx: &mut VisualTestContext) {
    right_press(view, offset, cx);
    draw(cx);
}

/// The open menu's titles, `None` for a separator.
fn titles(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<Vec<Option<String>>> {
    view.read_with(cx, |view, _| {
        let entries = view.editor_menu_entries()?;
        Some(
            (entries.iter())
                .map(|entry| entry.as_ref().map(|item| item.title.to_string()))
                .collect(),
        )
    })
}

fn is_open(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> bool {
    titles(view, cx).is_some()
}

fn selections(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<(usize, usize)> {
    view.read_with(cx, |view, _| {
        (view.model().selections().iter())
            .map(|selection| (selection.range.start, selection.range.end))
            .collect()
    })
}

fn set_selections(
    view: &Entity<EditorView>,
    ranges: &[(usize, usize)],
    cx: &mut VisualTestContext,
) {
    view.update(cx, |view, cx| {
        let selections = ranges.iter().map(|&(start, end)| ResolvedSelection {
            range: start..end,
            reversed: false,
        });
        view.model_mut().set_selections(selections);
        cx.notify();
    });
    draw(cx);
}

fn text_of(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |view, _| view.model().text().to_string())
}

fn editor_focused(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| view.focus_handle(cx).is_focused(window))
}

fn highlighted(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<usize> {
    view.read_with(cx, |view, _| view.editor_menu_highlighted())
}

#[gpui::test]
fn right_click_outside_the_selection_moves_the_caret_and_opens_the_menu(cx: &mut TestAppContext) {
    let (view, cx, _) = editor(cx);
    let asked = provide(cx, items());
    set_selections(&view, &[(0, 2), (at("call_5"), at("call_5") + 4)], cx);

    let call = at("call_3");
    right_click(&view, call, cx);
    assert_eq!(selections(&view, cx), [(call, call)], "one caret, there");
    assert_eq!(
        titles(&view, cx).unwrap(),
        [
            Some("Cut".into()),
            Some("Copy".into()),
            None,
            Some("Select All".into()),
            None,
            Some("Off".into()),
            Some("Probe".into()),
        ]
    );
    let asked = asked.borrow();
    assert_eq!(asked.len(), 1);
    let request = &asked[0];
    assert_eq!((request.offset, request.row), (call, 4));
    assert!(!request.inside_selection);
    assert!(!request.read_only);
    assert_eq!(request.editor.entity_id(), view.entity_id());
    // The editor's frame, with its live state, is the stack's last.
    let last = request.contexts.frames().last().expect("a frame");
    assert_eq!(last.name, crate::KEY_CONTEXT);
}

#[gpui::test]
fn right_click_inside_a_selection_keeps_it(cx: &mut TestAppContext) {
    let (view, cx, _) = editor(cx);
    let asked = provide(cx, items());
    let ranges = [(at("call_2"), at("call_4")), (at("call_6"), at("call_7"))];
    set_selections(&view, &ranges, cx);

    right_click(&view, at("call_3"), cx);
    assert_eq!(selections(&view, cx), ranges, "every selection kept");
    assert!(is_open(&view, cx));
    assert!(asked.borrow()[0].inside_selection);
}

#[gpui::test]
fn no_provider_opens_no_menu(cx: &mut TestAppContext) {
    let (view, cx, _) = editor(cx);
    let before = selections(&view, cx);
    right_click(&view, at("call_3"), cx);
    assert!(!is_open(&view, cx));
    assert_eq!(selections(&view, cx), before, "nothing happened");

    // An answer with no items opens nothing either.
    let asked = provide(cx, vec![None, None]);
    right_click(&view, at("call_3"), cx);
    assert_eq!(asked.borrow().len(), 1);
    assert!(!is_open(&view, cx));
    assert!(editor_focused(&view, cx));
}

/// The middle of buffer row `row`'s line-number column.
fn number_at(view: &Entity<EditorView>, row: u32, cx: &mut VisualTestContext) -> Point<Pixels> {
    let drawn = view.read_with(cx, |view, _| view.drawn_gutter().expect("drawn").clone());
    let column = drawn.columns[0].union(&drawn.columns[1]);
    let offset = view.read_with(cx, |view, _| view.model().text().row_start(row));
    point(column.center().x, point_at(view, offset, cx).y)
}

#[gpui::test]
fn the_line_number_column_keeps_the_breakpoint_menu(cx: &mut TestAppContext) {
    let (view, cx, _) = editor(cx);
    let asked = provide(cx, items());
    let handlers = BreakpointHandlers {
        on_toggle: Rc::new(|_, _, _| {}),
        menu: Rc::new(|_, _| {
            vec![BreakpointMenuItem {
                title: "Add Breakpoint".into(),
                run: Rc::new(|_, _| {}),
            }]
        }),
    };
    view.update(cx, |view, cx| {
        view.set_breakpoint_handlers(Some(handlers), cx)
    });
    draw(cx);
    let breakpoint_menu = |view: &Entity<EditorView>, cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| view.breakpoint_menu())
    };

    let number = number_at(&view, 3, cx);
    cx.simulate_mouse_down(number, MouseButton::Right, Modifiers::none());
    cx.simulate_mouse_up(number, MouseButton::Right, Modifiers::none());
    draw(cx);
    assert_eq!(
        breakpoint_menu(&view, cx),
        Some(vec!["Add Breakpoint".into()])
    );
    assert!(!is_open(&view, cx));
    assert!(asked.borrow().is_empty(), "the provider wasn't asked");

    // The text's menu closes the breakpoint menu, and the other way round.
    right_click(&view, at("call_5"), cx);
    assert!(is_open(&view, cx));
    assert_eq!(breakpoint_menu(&view, cx), None);
    cx.simulate_mouse_down(number, MouseButton::Right, Modifiers::none());
    cx.simulate_mouse_up(number, MouseButton::Right, Modifiers::none());
    draw(cx);
    assert!(breakpoint_menu(&view, cx).is_some());
    assert!(!is_open(&view, cx));
}

#[gpui::test]
fn arrows_pass_separators_and_disabled_items(cx: &mut TestAppContext) {
    let (view, cx, _) = editor(cx);
    provide(cx, items());
    right_click(&view, at("call_3"), cx);
    assert_eq!(highlighted(&view, cx), None);
    // Cut 0, Copy 1, | 2, Select All 3, | 4, Off 5 (disabled), Probe 6.
    for expected in [0, 1, 3, 6, 0] {
        cx.simulate_keystrokes("down");
        assert_eq!(highlighted(&view, cx), Some(expected));
    }
    for expected in [6, 3, 1] {
        cx.simulate_keystrokes("up");
        assert_eq!(highlighted(&view, cx), Some(expected));
    }
    cx.simulate_keystrokes("end");
    assert_eq!(highlighted(&view, cx), Some(6));
    cx.simulate_keystrokes("home");
    assert_eq!(highlighted(&view, cx), Some(0));
    assert!(is_open(&view, cx));
}

#[gpui::test]
fn a_disabled_item_does_not_run(cx: &mut TestAppContext) {
    let (view, cx, seen) = editor(cx);
    provide(cx, items());
    right_click(&view, at("call_3"), cx);

    // Its row takes no click: the menu stays open, nothing runs.
    let off = cx.debug_bounds("menu-item:Off").expect("Off is drawn");
    cx.simulate_click(off.center(), Modifiers::none());
    draw(cx);
    assert!(is_open(&view, cx));
    assert!(seen.borrow().is_empty());

    // Chosen anyway, it closes the menu and runs nothing.
    cx.update(|window, cx| view.update(cx, |view, cx| view.choose_editor_menu_item(5, window, cx)));
    draw(cx);
    assert!(!is_open(&view, cx));
    assert!(seen.borrow().is_empty());
    assert!(editor_focused(&view, cx));
}

#[gpui::test]
fn a_chosen_item_runs_on_the_editor_after_focus_returns(cx: &mut TestAppContext) {
    let (view, cx, seen) = editor(cx);
    provide(cx, items());

    // A click on an item: one the editor doesn't know reaches its host,
    // from the editor, which has focus again by then.
    right_click(&view, at("call_3"), cx);
    let probe = cx.debug_bounds("menu-item:Probe").expect("Probe is drawn");
    cx.simulate_click(probe.center(), Modifiers::none());
    draw(cx);
    assert!(!is_open(&view, cx));
    {
        let seen = seen.borrow();
        assert_eq!(seen.len(), 1);
        let (invocation, focused) = &seen[0];
        assert_eq!(invocation.command.as_str(), "test.probe");
        assert_eq!(invocation.source, Source::Menu);
        assert!(*focused, "the editor had focus as it ran");
        // In the editor's context at the press.
        let last = invocation.context.frames().last().expect("a context");
        assert_eq!(last.name, crate::KEY_CONTEXT);
    }

    // Enter runs the highlighted one: the editor's own.
    right_click(&view, at("call_3"), cx);
    cx.simulate_keystrokes("down down down enter");
    draw(cx);
    assert!(!is_open(&view, cx));
    assert_eq!(selections(&view, cx), [(0, text().len())]);
    assert!(editor_focused(&view, cx));
}

#[gpui::test]
fn scroll_edit_and_save_as_close_it(cx: &mut TestAppContext) {
    let (view, cx, seen) = editor(cx);
    provide(cx, items());
    // Above the menu, which covers the rows below the press.
    let elsewhere = point_at(&view, at("call_0"), cx);

    right_click(&view, at("call_3"), cx);
    cx.simulate_event(ScrollWheelEvent {
        position: elsewhere,
        delta: ScrollDelta::Lines(point(0., 1.)),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
    });
    draw(cx);
    assert!(!is_open(&view, cx), "scrolled");
    assert!(editor_focused(&view, cx));

    right_click(&view, at("call_3"), cx);
    view.update(cx, |view, cx| {
        let mut buffer = view.model_mut().buffer_mut();
        buffer.edit([(0..0, "// ")], Origin::User).unwrap();
        drop(buffer);
        cx.notify();
    });
    draw(cx);
    assert!(!is_open(&view, cx), "edited");
    assert!(editor_focused(&view, cx));

    right_click(&view, at("call_3"), cx);
    view.update(cx, |view, cx| {
        view.model_mut().set_path(Some("other.rs".into()));
        cx.notify();
    });
    draw(cx);
    assert!(!is_open(&view, cx), "saved as");
    assert!(editor_focused(&view, cx));
    assert!(seen.borrow().is_empty(), "nothing ran");
}

#[gpui::test]
fn extend_inserts_after_its_anchor_and_keeps_the_highlight(cx: &mut TestAppContext) {
    let (view, cx, _) = editor(cx);
    provide(cx, items());
    right_click(&view, at("call_3"), cx);
    cx.simulate_keystrokes("down down");
    assert_eq!(highlighted(&view, cx), Some(1), "on Copy");
    let ticket = view.read_with(cx, |view, _| view.editor_menu_ticket().unwrap());

    view.update(cx, |view, cx| {
        view.extend_editor_menu(
            ticket,
            "editor.cut",
            vec![item("Fix", "test.fix", true)],
            cx,
        )
    });
    let titles_now = titles(&view, cx).unwrap();
    assert_eq!(
        titles_now[..3],
        [Some("Cut".into()), Some("Fix".into()), Some("Copy".into())]
    );
    assert_eq!(highlighted(&view, cx), Some(2), "still on Copy");

    // No such item: a new last group.
    view.update(cx, |view, cx| {
        view.extend_editor_menu(
            ticket,
            "test.none",
            vec![item("Last", "test.last", true)],
            cx,
        )
    });
    let titles_now = titles(&view, cx).unwrap();
    assert_eq!(
        titles_now[titles_now.len() - 2..],
        [None, Some("Last".into())]
    );
    assert_eq!(highlighted(&view, cx), Some(2));
    draw(cx);
    assert!(cx.debug_bounds("menu-item:Fix").is_some(), "drawn");
}

#[gpui::test]
fn extend_before_inserts_ahead_of_its_anchor_and_keeps_the_highlight(cx: &mut TestAppContext) {
    let (view, cx, _) = editor(cx);
    provide(cx, items());
    right_click(&view, at("call_3"), cx);
    cx.simulate_keystrokes("down");
    assert_eq!(highlighted(&view, cx), Some(0), "on Cut");
    let ticket = view.read_with(cx, |view, _| view.editor_menu_ticket().unwrap());

    // A first group, ahead of the menu's first item.
    view.update(cx, |view, cx| {
        view.extend_editor_menu_before(
            ticket,
            "editor.cut",
            vec![item("Fix", "test.fix", true), None],
            cx,
        )
    });
    let titles_now = titles(&view, cx).unwrap();
    assert_eq!(
        titles_now[..3],
        [Some("Fix".into()), None, Some("Cut".into())]
    );
    assert_eq!(highlighted(&view, cx), Some(2), "still on Cut");

    // No such item: a new last group, as for `extend_editor_menu`.
    view.update(cx, |view, cx| {
        view.extend_editor_menu_before(
            ticket,
            "test.none",
            vec![item("Last", "test.last", true)],
            cx,
        )
    });
    let titles_now = titles(&view, cx).unwrap();
    assert_eq!(
        titles_now[titles_now.len() - 2..],
        [None, Some("Last".into())]
    );
    draw(cx);
    assert!(cx.debug_bounds("menu-item:Fix").is_some(), "drawn");
}

#[gpui::test]
fn a_stale_ticket_extends_nothing(cx: &mut TestAppContext) {
    let (view, cx, _) = editor(cx);
    provide(cx, items());
    right_click(&view, at("call_3"), cx);
    let old = view.read_with(cx, |view, _| view.editor_menu_ticket().unwrap());
    cx.simulate_keystrokes("escape");
    draw(cx);

    // Closed: nothing to extend.
    view.update(cx, |view, cx| {
        view.extend_editor_menu(old, "editor.cut", vec![item("Fix", "test.fix", true)], cx)
    });
    assert!(!is_open(&view, cx));

    // Another menu: not the old ticket's.
    right_click(&view, at("call_4"), cx);
    let before = titles(&view, cx).unwrap();
    view.update(cx, |view, cx| {
        view.extend_editor_menu(old, "editor.cut", vec![item("Fix", "test.fix", true)], cx)
    });
    assert_eq!(titles(&view, cx).unwrap(), before);
}

#[gpui::test]
fn the_menu_takes_its_own_focus_and_no_input_handler(cx: &mut TestAppContext) {
    let (view, cx, _) = editor(cx);
    provide(cx, items());
    right_click(&view, at("call_3"), cx);

    let menu = view.read_with(cx, |view, _| view.editor_menu_focus().expect("open"));
    cx.update(|window, cx| {
        assert!(menu.is_focused(window), "the menu's own handle");
        assert!(!view.focus_handle(cx).is_focused(window));
        // The editor's element registers its input handler only for its
        // own focused handle, and the menu registers none.
        let own = window.context_stack().last().cloned().expect("a context");
        assert!(own.contains(super::MENU_KEY_CONTEXT), "{own:?}");
        assert!(own.contains(kiln_platform::keys::HOLDS_KEYS), "{own:?}");
    });

    // Typed text goes nowhere.
    let before = text_of(&view, cx);
    cx.simulate_input("abc");
    draw(cx);
    assert_eq!(text_of(&view, cx), before);
    assert!(is_open(&view, cx));
}

#[gpui::test]
fn opening_mid_composition_settles_it(cx: &mut TestAppContext) {
    let (view, cx, _) = editor(cx);
    provide(cx, items());
    let before = text_of(&view, cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.replace_and_mark_text_in_range(None, "かな", Some(2..2), window, cx)
        })
    });
    let marked = |view: &Entity<EditorView>, cx: &mut VisualTestContext| {
        cx.update(|window, cx| view.update(cx, |view, cx| view.marked_text_range(window, cx)))
    };
    assert!(marked(&view, cx).is_some(), "composing");

    // Read as the menu opens, before a frame: so not by the editor's blur,
    // which comes with a frame.
    let seen: Probe<(bool, Option<Range<usize>>, String)> = Rc::default();
    let sink = seen.clone();
    right_press_then(&view, at("call_3"), cx, move |view, window, cx| {
        let open = view.editor_menu_entries().is_some();
        let marked = view.marked_text_range(window, cx);
        *sink.borrow_mut() = Some((open, marked, view.model().text().to_string()));
    });
    let (open, marked_then, text_then) = seen.borrow_mut().take().expect("read");
    assert!(open, "open by then");
    assert_eq!(marked_then, None);
    assert_eq!(text_then, before, "nothing committed");
    assert_eq!(marked(&view, cx), None);
    draw(cx);
    assert_eq!(text_of(&view, cx), before);
}

/// F1: an input method's calls that come once the menu has opened, before
/// its first frame takes the editor's input handler from the platform, type
/// nothing, compose nothing and move no caret.
#[gpui::test]
fn input_method_calls_before_the_first_frame_do_nothing(cx: &mut TestAppContext) {
    let (view, cx, _) = editor(cx);
    provide(cx, items());
    let before = text_of(&view, cx);
    let seen: Probe<(bool, String, Ranges, Option<Range<usize>>)> = Rc::default();
    let sink = seen.clone();
    right_press_then(&view, at("call_3"), cx, move |view, window, cx| {
        let open = view.editor_menu_entries().is_some();
        view.replace_and_mark_text_in_range(None, "かな", Some(2..2), window, cx);
        view.replace_text_in_range(None, "typed", window, cx);
        view.replace_text_in_range(Some(0..2), "xy", window, cx);
        view.unmark_text(window, cx);
        let selections = (view.model().selections().iter())
            .map(|selection| (selection.range.start, selection.range.end))
            .collect();
        let marked = view.marked_text_range(window, cx);
        *sink.borrow_mut() = Some((open, view.model().text().to_string(), selections, marked));
    });
    let (open, text, selections, marked) = seen.borrow_mut().take().expect("read");
    assert!(open, "open by then");
    assert_eq!(text, before);
    let call = at("call_3");
    assert_eq!(selections, [(call, call)], "the press's caret only");
    assert_eq!(marked, None);
    draw(cx);
    assert_eq!(text_of(&view, cx), before);
    assert!(is_open(&view, cx));
}

/// F2: focus moving to something else closes the menu, and the frame after
/// takes it off the screen with no other reason to draw.
#[gpui::test]
fn focus_moving_elsewhere_takes_the_menu_away(cx: &mut TestAppContext) {
    let (view, cx, _) = editor(cx);
    provide(cx, items());
    right_click(&view, at("call_3"), cx);
    assert!(cx.debug_bounds("editor-menu").is_some(), "drawn");

    // The host's other element, focused as a panel takes focus.
    cx.update(|window, cx| {
        let host = window.root::<Host>().flatten().expect("the host");
        let other = host.read(cx).other.clone();
        window.focus(&other, cx);
    });
    cx.run_until_parked();
    assert!(!is_open(&view, cx));
    assert!(cx.debug_bounds("editor-menu").is_none(), "no longer drawn");
    assert!(!editor_focused(&view, cx), "focus stays where it went");
}

/// F3: a scroll the editor didn't take itself (the other side of a diff
/// following) closes the menu; revealing the caret the press put doesn't.
#[gpui::test]
fn a_linked_diff_scroll_closes_it(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let long: String = (0..300).map(|n| format!("line {n}\n")).collect();
    let (diff, cx) = cx.add_window_view(move |_, cx| {
        let side = |text: &str| EditorModel::new(Buffer::new(text), Some("a.txt".into()));
        let changed = long.replace("line 5\n", "line five\n");
        DiffView::new(
            "a.txt",
            DiffSide::new("Saved", side(&long)),
            DiffSide::new("Working Tree", side(&changed)),
            cx,
        )
        .with_folding(false)
    });
    cx.update(|window, _| window.activate_window());
    draw(cx);
    provide(cx, items());
    let [left, right] = diff.read_with(cx, |diff, _| [diff.left().clone(), diff.right().clone()]);
    let text = text_of(&left, cx);

    // The last row on screen, whose caret the press may reveal, which
    // scrolls nothing the menu minds: still open.
    let low = (0..300)
        .rev()
        .map(|row| text.find(&format!("line {row}\n")).unwrap())
        .find(|&at| {
            left.update(cx, |left, _| left.caret_bounds_at(at))
                .is_some()
        })
        .expect("rows on screen");
    right_click(&left, low, cx);
    draw(cx);
    assert!(is_open(&left, cx));

    // The right side's wheel, which the left follows.
    let right_text = text_of(&right, cx);
    let top = (0..300)
        .filter_map(|row| right_text.find(&format!("line {row}\n")))
        .find(|&at| {
            right
                .update(cx, |right, _| right.caret_bounds_at(at))
                .is_some()
        })
        .expect("rows on screen");
    let before = left.read_with(cx, |left, _| left.scroll_top());
    let position = point_at(&right, top, cx);
    cx.simulate_event(ScrollWheelEvent {
        position,
        delta: ScrollDelta::Lines(point(0., -5.)),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
    });
    draw(cx);
    draw(cx);
    let scrolled = left.read_with(cx, |left, _| left.scroll_top());
    assert!(scrolled > before, "the left side followed");
    assert!(!is_open(&left, cx), "closed by the linked scroll");
}

/// F4: selections are half-open: a press on a selection's last character
/// keeps it, one on the character after puts a caret there, with several
/// selections and in vim's visual mode.
#[gpui::test]
fn the_character_after_a_selection_is_outside_it(cx: &mut TestAppContext) {
    let (view, cx, _) = editor(cx);
    let asked = provide(cx, items());
    let (a, b) = (at("call_2"), at("call_2") + 4);
    let ranges = [(a, b), (at("call_6"), at("call_7"))];
    set_selections(&view, &ranges, cx);
    right_click(&view, b - 1, cx);
    assert_eq!(selections(&view, cx), ranges, "its last character");
    cx.simulate_keystrokes("escape");
    draw(cx);
    right_click(&view, b, cx);
    assert_eq!(selections(&view, cx), [(b, b)], "the character after");
    assert!(!asked.borrow()[1].inside_selection);
    cx.simulate_keystrokes("escape");
    draw(cx);

    // Vim's visual mode over `cal` of `call_1`.
    cx.update(|_, cx| crate::vim::set_vim_enabled(cx, true));
    cx.simulate_keystrokes("escape");
    draw(cx);
    let start = at("call_1");
    view.update(cx, |view, cx| {
        view.model_mut().set_cursor(start);
        cx.notify();
    });
    draw(cx);
    cx.simulate_keystrokes("v l l");
    draw(cx);
    let visual = selections(&view, cx);
    assert_eq!(visual, [(start, start + 3)]);
    right_click(&view, start + 2, cx);
    assert_eq!(selections(&view, cx), visual, "its last character");
    cx.simulate_keystrokes("escape");
    draw(cx);
    right_click(&view, start + 3, cx);
    let visual_now = view.read_with(cx, |view, _| view.vim_visual_kind());
    assert_eq!(visual_now, None, "out of visual mode");
    let head = view.read_with(cx, |view, _| view.model().primary_selection().head());
    assert_eq!(head, start + 3);
}

/// F5: a caret move that edits the text (leaving a blank line in vim's
/// insert mode takes out its unused autoindent) leaves the provider the
/// clicked place in the text after it.
#[gpui::test]
fn the_request_follows_an_edit_the_caret_move_makes(cx: &mut TestAppContext) {
    let (view, cx, _) = editor(cx);
    let asked = provide(cx, items());
    cx.update(|_, cx| crate::vim::set_vim_enabled(cx, true));
    cx.simulate_keystrokes("escape");
    draw(cx);
    // A new line under `call_1();`, autoindented and left blank.
    let end_of_call_1 = at("call_2") - "\n    ".len();
    view.update(cx, |view, cx| {
        view.model_mut().set_cursor(end_of_call_1);
        cx.notify();
    });
    draw(cx);
    cx.simulate_keystrokes("a enter");
    draw(cx);
    let with_indent = text_of(&view, cx);
    assert!(with_indent.contains("call_1();\n    \n"), "{with_indent:?}");

    let clicked = with_indent.find("call_5").unwrap();
    right_click(&view, clicked, cx);
    let after = text_of(&view, cx);
    assert_ne!(after, with_indent, "the unused indent went");
    let call_5 = after.find("call_5").unwrap();
    let request = asked.borrow()[0].clone();
    assert_eq!(request.offset, call_5);
    let row = after[..call_5].matches('\n').count() as u32;
    assert_eq!(request.row, row);
    let head = view.read_with(cx, |view, _| view.model().primary_selection().head());
    assert_eq!(head, call_5);
}

#[gpui::test]
fn escape_choose_and_outside_press_give_focus_back(cx: &mut TestAppContext) {
    let (view, cx, _) = editor(cx);
    provide(cx, items());

    right_click(&view, at("call_3"), cx);
    assert!(!editor_focused(&view, cx));
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(!is_open(&view, cx));
    assert!(editor_focused(&view, cx), "after Escape");

    right_click(&view, at("call_3"), cx);
    cx.simulate_keystrokes("down down enter");
    draw(cx);
    assert!(!is_open(&view, cx));
    assert!(editor_focused(&view, cx), "after choosing");

    right_click(&view, at("call_3"), cx);
    // Above the menu, which covers the rows below the press.
    let elsewhere = point_at(&view, at("call_0"), cx);
    cx.simulate_click(elsewhere, Modifiers::none());
    draw(cx);
    assert!(!is_open(&view, cx));
    assert!(editor_focused(&view, cx), "after a press outside");

    // Closed by the host, without a window: at the next render.
    right_click(&view, at("call_3"), cx);
    view.update(cx, |view, cx| view.close_editor_menu(cx));
    draw(cx);
    assert!(!is_open(&view, cx));
    assert!(editor_focused(&view, cx), "after a close without a window");
}

#[gpui::test]
fn vim_keys_do_not_reach_the_buffer_while_open(cx: &mut TestAppContext) {
    let (view, cx, _) = editor(cx);
    cx.update(|_, cx| crate::vim::set_vim_enabled(cx, true));
    cx.simulate_keystrokes("escape");
    draw(cx);
    provide(cx, items());
    let before = text_of(&view, cx);

    let call = at("call_3");
    right_click(&view, call, cx);
    let caret = selections(&view, cx);
    cx.simulate_keystrokes("j x d i");
    draw(cx);
    assert_eq!(text_of(&view, cx), before);
    assert_eq!(selections(&view, cx), caret, "no motion reached vim");
    assert!(is_open(&view, cx));

    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(!is_open(&view, cx));
    cx.simulate_keystrokes("x");
    draw(cx);
    let mut deleted = before.clone();
    deleted.remove(call);
    assert_eq!(text_of(&view, cx), deleted, "vim works again");
}
