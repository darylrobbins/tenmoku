// SPDX-License-Identifier: Apache-2.0

//! The B3 controls from the keyboard and to a screen reader: each is in the
//! tab order unless disabled, rings itself while focused from the keyboard,
//! acts on Enter or Space, and keeps focus where it was when clicked; a
//! segmented control moves its choice with the arrow keys; and each
//! control's accessibility node carries its name, its state and whether
//! it's disabled.

use std::{cell::RefCell, rc::Rc};

use gpui::{
    Context, Entity, FocusHandle, InteractiveElement as _, IntoElement, Modifiers,
    ParentElement as _, Render, Role, Styled as _, TestAppContext, Toggled, VisualTestContext,
    Window, accesskit, div, px,
};

use crate::{
    Badge, Button, Checkbox, Icon, IconButton, SegmentedControl, Switch, ToggleButton, UiTheme,
    segmented::segment_for_key,
};

/// One of each control, every one counting its activations, plus a focus
/// target of the window's own.
struct Form {
    disabled: bool,
    clicks: Vec<&'static str>,
    scope: usize,
    other: FocusHandle,
    /// Each control's accessibility node, as last drawn.
    nodes: Rc<RefCell<Vec<(&'static str, accesskit::Node)>>>,
}

impl Render for Form {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let d = self.disabled;
        let ui = UiTheme::default();
        let controls = vec![
            (
                "button",
                Button::new("button", "Save")
                    .disabled(d)
                    .on_click(click("button", cx))
                    .element(window, cx),
            ),
            (
                "checkbox",
                Checkbox::new("checkbox")
                    .label("Wrap")
                    .checked(true)
                    .disabled(d)
                    .on_change(change("checkbox", cx))
                    .element(window, cx),
            ),
            (
                "switch",
                Switch::new("switch")
                    .label("Sync")
                    .disabled(d)
                    .on_change(change("switch", cx))
                    .element(window, cx),
            ),
            (
                "toggle",
                ToggleButton::new("toggle", "Aa", "Match Case", &ui)
                    .on(true)
                    .disabled(d)
                    .on_click(click("toggle", cx))
                    .element(window, cx),
            ),
            (
                "icon",
                IconButton::new("icon", Icon::X, "Close", &ui)
                    .disabled(d)
                    .on_click(click("icon", cx))
                    .element(window, cx),
            ),
            (
                "segmented",
                SegmentedControl::new("segmented", "Scope")
                    .segments(["You", "This project", "Workspace"])
                    .selected(self.scope)
                    .disabled(d)
                    .on_change(cx.listener(|form, ix: &usize, _, cx| {
                        form.scope = *ix;
                        form.clicks.push("segmented");
                        cx.notify();
                    }))
                    .element(window, cx),
            ),
        ];
        let mut nodes = self.nodes.borrow_mut();
        nodes.clear();
        for (what, control) in &controls {
            nodes.push((what, control.a11y_node().expect("every control has a role")));
        }
        div()
            .flex()
            .flex_col()
            .items_start()
            .gap_4()
            .p_4()
            .child(div().track_focus(&self.other).size(px(10.)))
            .children(
                controls.into_iter().map(|(what, control)| {
                    div().debug_selector(move || what.into()).child(control)
                }),
            )
    }
}

fn click(
    what: &'static str,
    cx: &mut Context<Form>,
) -> impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static {
    cx.listener(move |form: &mut Form, _: &gpui::ClickEvent, _, cx| {
        form.clicks.push(what);
        cx.notify();
    })
}

fn change(
    what: &'static str,
    cx: &mut Context<Form>,
) -> impl Fn(&bool, &mut Window, &mut gpui::App) + 'static {
    cx.listener(move |form: &mut Form, _: &bool, _, cx| {
        form.clicks.push(what);
        cx.notify();
    })
}

fn open(disabled: bool, cx: &mut TestAppContext) -> (Entity<Form>, &mut VisualTestContext) {
    let (form, cx) = cx.add_window_view(move |_, cx| Form {
        disabled,
        clicks: Vec::new(),
        scope: 0,
        other: cx.focus_handle(),
        nodes: Rc::default(),
    });
    cx.run_until_parked();
    (form, cx)
}

/// Which control holds the focus ring, if any: the one whose box it
/// surrounds.
fn ringed(cx: &mut VisualTestContext) -> Option<&'static str> {
    let ring = cx.debug_bounds("focus-ring")?;
    [
        "button",
        "checkbox",
        "switch",
        "toggle",
        "icon",
        "segmented",
    ]
    .into_iter()
    .find(|what| {
        let control = cx.debug_bounds(what).unwrap();
        ring.contains(&control.center()) && ring.size.width > control.size.width
    })
}

/// Presses and releases `key`: GPUI clicks a focused element when Enter or
/// Space is released.
fn press(key: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(key);
    cx.simulate_event(gpui::KeyUpEvent {
        keystroke: gpui::Keystroke::parse(key).unwrap(),
    });
    cx.run_until_parked();
}

/// Tabs forward from the window's own focus target, as a host binding
/// `tab` to `focus_next` would.
fn tab(cx: &mut VisualTestContext) {
    // A key press makes the keyboard the last input, as tabbing does.
    cx.simulate_keystrokes("f19");
    cx.update(|window, cx| window.focus_next(cx));
    cx.run_until_parked();
}

#[gpui::test]
fn controls_take_keyboard_focus_ring_it_and_act_on_enter(cx: &mut TestAppContext) {
    let (form, cx) = open(false, cx);
    cx.update(|window, cx| {
        let other = form.read(cx).other.clone();
        window.focus(&other, cx);
    });
    cx.run_until_parked();
    assert_eq!(ringed(cx), None);

    let mut visited = Vec::new();
    for _ in 0..6 {
        tab(cx);
        visited.push(ringed(cx).expect("a focused control rings itself"));
    }
    assert_eq!(
        visited,
        [
            "button",
            "checkbox",
            "switch",
            "toggle",
            "icon",
            "segmented"
        ],
        "each in the tab order, in order"
    );

    // Back on the button: Enter clicks it, as does Space on the switch.
    cx.update(|window, cx| {
        let other = form.read(cx).other.clone();
        window.focus(&other, cx);
    });
    tab(cx);
    press("enter", cx);
    tab(cx);
    tab(cx);
    press("space", cx);
    assert_eq!(
        form.read_with(cx, |form, _| form.clicks.clone()),
        ["button", "switch"]
    );

    // Focus moved by the mouse shows no ring.
    let at = cx.debug_bounds("button").unwrap().center();
    cx.simulate_mouse_move(at, None, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(ringed(cx), None, "no ring once the mouse is the last input");
}

#[gpui::test]
fn a_click_leaves_focus_where_it_was(cx: &mut TestAppContext) {
    let (form, cx) = open(false, cx);
    let other = form.read_with(cx, |form, _| form.other.clone());
    cx.update(|window, cx| window.focus(&other, cx));
    cx.run_until_parked();
    for what in ["button", "toggle", "icon", "checkbox"] {
        let at = cx.debug_bounds(what).unwrap().center();
        cx.simulate_click(at, Modifiers::none());
        cx.run_until_parked();
        cx.update(|window, _| assert!(other.is_focused(window), "{what} took focus"));
    }
    assert_eq!(
        form.read_with(cx, |form, _| form.clicks.clone()),
        ["button", "toggle", "icon", "checkbox"],
        "and the clicks still ran"
    );
}

#[gpui::test]
fn disabled_controls_leave_the_tab_order_and_say_so(cx: &mut TestAppContext) {
    let (form, cx) = open(true, cx);
    let other = form.read_with(cx, |form, _| form.other.clone());
    cx.update(|window, cx| window.focus(&other, cx));
    tab(cx);
    assert_eq!(ringed(cx), None, "nothing to tab to");
    press("enter", cx);
    press("space", cx);
    press("right", cx);
    assert!(form.read_with(cx, |form, _| form.clicks.is_empty()));

    let disabled = |form: &Entity<Form>, cx: &mut VisualTestContext| {
        form.read_with(cx, |form, _| {
            (form.nodes.borrow().iter())
                .map(|(what, node)| (*what, node.is_disabled()))
                .collect::<Vec<_>>()
        })
    };
    assert!(
        disabled(&form, cx).iter().all(|(_, off)| *off),
        "{:?}",
        disabled(&form, cx)
    );

    // Enabled again: no longer disabled to a screen reader, and it acts.
    form.update(cx, |form, cx| {
        form.disabled = false;
        cx.notify();
    });
    cx.run_until_parked();
    assert!(
        disabled(&form, cx).iter().all(|(_, off)| !*off),
        "{:?}",
        disabled(&form, cx)
    );
    let at = cx.debug_bounds("button").unwrap().center();
    cx.simulate_click(at, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(
        form.read_with(cx, |form, _| form.clicks.clone()),
        ["button"]
    );

    // And disabled once more.
    form.update(cx, |form, cx| {
        form.disabled = true;
        cx.notify();
    });
    cx.run_until_parked();
    assert!(disabled(&form, cx).iter().all(|(_, off)| *off));
}

#[gpui::test]
fn controls_tell_a_screen_reader_their_name_and_state(cx: &mut TestAppContext) {
    let (form, cx) = open(false, cx);
    let nodes = form.read_with(cx, |form, _| form.nodes.borrow().clone());
    let node = |what: &str| &nodes.iter().find(|(name, _)| *name == what).unwrap().1;
    let expect = [
        ("button", Role::Button, "Save", None),
        ("checkbox", Role::CheckBox, "Wrap", Some(Toggled::True)),
        ("switch", Role::Switch, "Sync", Some(Toggled::False)),
        ("toggle", Role::Button, "Match Case", Some(Toggled::True)),
        ("icon", Role::Button, "Close", None),
        ("segmented", Role::RadioGroup, "Scope", None),
    ];
    for (what, role, label, toggled) in expect {
        let node = node(what);
        assert_eq!(node.role(), role, "{what}");
        assert_eq!(node.label(), Some(label), "{what}");
        assert_eq!(node.toggled(), toggled, "{what}");
    }
}

/// The segments, as a screen reader would hear them: each one's label and
/// whether it's checked.
struct Segments {
    scope: usize,
    nodes: Rc<RefCell<Vec<accesskit::Node>>>,
}

impl Render for Segments {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let control = SegmentedControl::new("scope", "Scope")
            .segments(["You", "This project", "Workspace"])
            .selected(self.scope)
            .on_change(cx.listener(|this, ix: &usize, _, cx| {
                this.scope = *ix;
                cx.notify();
            }));
        let (control, segments) = control.element_with_segments(window, cx);
        *self.nodes.borrow_mut() = segments;
        div().p_4().child(control)
    }
}

#[gpui::test]
fn a_segmented_control_moves_with_the_arrow_keys_and_reports_its_checked_segment(
    cx: &mut TestAppContext,
) {
    assert_eq!(segment_for_key("right", 0, 3), Some(1));
    assert_eq!(segment_for_key("left", 0, 3), Some(0), "no wrapping");
    assert_eq!(segment_for_key("end", 0, 3), Some(2));
    assert_eq!(segment_for_key("home", 2, 3), Some(0));
    assert_eq!(segment_for_key("a", 0, 3), None);
    assert_eq!(segment_for_key("right", 0, 0), None, "no segments");

    let (view, cx) = cx.add_window_view(|_, _| Segments {
        scope: 0,
        nodes: Rc::default(),
    });
    cx.run_until_parked();
    let checked = |view: &Entity<Segments>, cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| {
            (view.nodes.borrow().iter())
                .map(|node| (node.role(), node.toggled()))
                .collect::<Vec<_>>()
        })
    };
    let radio = |on: bool| (Role::RadioButton, Some(Toggled::from(on)));
    assert_eq!(
        checked(&view, cx),
        [radio(true), radio(false), radio(false)]
    );

    cx.simulate_keystrokes("f19");
    cx.update(|window, cx| window.focus_next(cx));
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("focus-ring").is_some(),
        "focused from the keyboard"
    );
    cx.simulate_keystrokes("right");
    assert_eq!(view.read_with(cx, |view, _| view.scope), 1);
    assert_eq!(
        checked(&view, cx),
        [radio(false), radio(true), radio(false)]
    );
    cx.simulate_keystrokes("end");
    assert_eq!(view.read_with(cx, |view, _| view.scope), 2);
    cx.simulate_keystrokes("right");
    assert_eq!(
        view.read_with(cx, |view, _| view.scope),
        2,
        "stays at the end"
    );
    cx.simulate_keystrokes("home left");
    assert_eq!(view.read_with(cx, |view, _| view.scope), 0);
}

#[gpui::test]
fn a_named_badge_is_a_status_and_an_unnamed_one_only_drawn(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let named = Badge::count(3)
            .name("problems", "3 errors")
            .status(cx)
            .expect("a named badge is a node");
        let node = named.a11y_node().unwrap();
        assert_eq!(node.role(), Role::Status);
        assert_eq!(node.label(), Some("3 errors"));
        let dot = Badge::dot().name("unsaved", "Unsaved changes").status(cx);
        let node = dot.expect("a dot too").a11y_node().unwrap();
        assert_eq!(node.label(), Some("Unsaved changes"));
        assert!(Badge::dot().status(cx).is_none(), "unnamed: no node");
    });
}
