// SPDX-License-Identifier: Apache-2.0

//! The chrome's landmarks and labelled controls, as the a11y recorder sees
//! them: the activity rail is the `Navigation` "Activity bar", the status
//! bar the `Region` "Status bar", every button, toggle, checkbox and switch
//! has a name, and toggles and controls carry their toggled and disabled
//! states.

use kiln_l10n::Text;
use std::{cell::RefCell, rc::Rc};

use gpui::{
    Context, IntoElement, ParentElement as _, Render, StatefulInteractiveElement as _,
    TestAppContext, VisualTestContext, Window, accesskit, div,
};

use crate::{
    ActivityRail, Button, Checkbox, Icon, IconButton, RailButton, Role, StatusBarFrame,
    StatusEntry, Switch, ToggleButton, UiTheme, a11y::Node, nodes,
};

/// A window's chrome: a rail with one panel shown, a row of controls, and
/// a status bar.
struct Chrome {
    disabled: bool,
}

impl Render for Chrome {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let ui = UiTheme::default();
        let d = self.disabled;
        let rail = ActivityRail::new("rail", &ui)
            .children([
                RailButton::new("project", Icon::Folder, &ui)
                    .name("Project")
                    .shortcut("⌘1")
                    .highlighted(true),
                RailButton::new("git", Icon::GitBranch, &ui).name("Source Control"),
            ])
            .footer([RailButton::new("settings", Icon::Settings, &ui).name("Settings")]);
        let controls = div()
            .child(Button::new("save", Text::verbatim("Save")).disabled(d))
            .child(
                Button::icon_only("add", Icon::SquarePlus)
                    .name("Add")
                    .disabled(d),
            )
            .child(IconButton::new("close", Icon::X, "Close", &ui).disabled(d))
            .child(
                IconButton::new("replace", Icon::ChevronDown, "Toggle Replace", &ui).selected(true),
            )
            .child(
                IconButton::new("details", Icon::ChevronRight, "Toggle Details", &ui)
                    .selected(false),
            )
            .child(
                ToggleButton::new("case", "Aa", "Match Case", &ui)
                    .on(true)
                    .disabled(d),
            )
            .child(ToggleButton::new("word", "ab", "Whole Word", &ui))
            .child(
                ToggleButton::icon_only("regex", Icon::Regex, "Use Regular Expression").disabled(d),
            )
            .child(
                Checkbox::new("wrap")
                    .label("Wrap")
                    .checked(true)
                    .disabled(d),
            )
            .child(Switch::new("sync").name("Sync").disabled(d));
        let status = StatusBarFrame::new(&ui)
            .left([StatusEntry::new("mode", "NORMAL", &ui)])
            .right([StatusEntry::new("position", "1:1", &ui)]);
        div().child(rail).child(controls).child(status)
    }
}

fn drawn(disabled: bool, cx: &mut TestAppContext) -> (Vec<Node>, &mut VisualTestContext) {
    let (_, cx) = cx.add_window_view(move |_, _| Chrome { disabled });
    cx.run_until_parked();
    let got = cx.update(|window, cx| nodes(window, cx));
    (got, cx)
}

fn find<'a>(nodes: &'a [Node], id: &str) -> &'a Node {
    nodes
        .iter()
        .find(|n| n.id == gpui::ElementId::from(id.to_owned()))
        .unwrap_or_else(|| panic!("no node {id} in {nodes:#?}"))
}

#[gpui::test]
fn the_rail_and_status_bar_are_landmarks(cx: &mut TestAppContext) {
    let (got, _) = drawn(false, cx);
    let rail = find(&got, "rail");
    assert_eq!(
        (rail.role, rail.label.as_ref()),
        (Role::Navigation, "Activity bar")
    );
    let bar = find(&got, "status-bar");
    // A Region, which VoiceOver lists as a landmark; a Status is none.
    assert_eq!((bar.role, bar.label.as_ref()), (Role::Region, "Status bar"));
    let landmarks: Vec<_> = got
        .iter()
        .filter(|n| matches!(n.role, Role::Navigation | Role::Region))
        .map(|n| n.label.to_string())
        .collect();
    assert_eq!(landmarks, ["Activity bar", "Status bar"]);
}

#[gpui::test]
fn rail_buttons_are_toggles_named_by_their_panel(cx: &mut TestAppContext) {
    let (got, _) = drawn(false, cx);
    let buttons: Vec<_> = ["project", "git", "settings"]
        .iter()
        .map(|id| {
            let n = find(&got, id);
            (n.role, n.label.to_string(), n.states.toggled)
        })
        .collect();
    assert_eq!(
        buttons,
        [
            (Role::Button, "Project".into(), Some(true)),
            (Role::Button, "Source Control".into(), Some(false)),
            (Role::Button, "Settings".into(), Some(false)),
        ]
    );
}

/// The roles a focusable control has.
fn is_control(role: Role) -> bool {
    matches!(role, Role::Button | Role::CheckBox | Role::Switch)
}

#[gpui::test]
fn every_control_the_recorder_sees_has_a_label(cx: &mut TestAppContext) {
    let (got, _) = drawn(false, cx);
    let controls: Vec<_> = got.iter().filter(|n| is_control(n.role)).collect();
    // The rail's three, then the row's ten.
    assert_eq!(controls.len(), 13, "{controls:#?}");
    for node in controls {
        assert!(!node.label.trim().is_empty(), "{node:#?} has no label");
    }
    let labels: Vec<_> = [
        "save", "add", "close", "case", "word", "regex", "wrap", "sync",
    ]
    .iter()
    .map(|id| find(&got, id).label.to_string())
    .collect();
    assert_eq!(
        labels,
        [
            "Save",
            "Add",
            "Close",
            "Match Case",
            "Whole Word",
            "Use Regular Expression",
            "Wrap",
            "Sync",
        ]
    );
}

#[gpui::test]
fn toggles_and_controls_carry_their_toggled_state(cx: &mut TestAppContext) {
    let (got, _) = drawn(false, cx);
    let state = |id: &str| {
        let n = find(&got, id);
        (n.role, n.states.toggled)
    };
    assert_eq!(state("case"), (Role::Button, Some(true)));
    assert_eq!(state("word"), (Role::Button, Some(false)));
    assert_eq!(state("regex"), (Role::Button, Some(false)));
    // An icon button given `selected` is a toggle, on or off.
    assert_eq!(state("replace"), (Role::Button, Some(true)));
    assert_eq!(state("details"), (Role::Button, Some(false)));
    assert_eq!(state("wrap"), (Role::CheckBox, Some(true)));
    assert_eq!(state("sync"), (Role::Switch, Some(false)));
    // A plain button isn't a toggle.
    assert_eq!(state("save"), (Role::Button, None));
    assert_eq!(state("close"), (Role::Button, None));
}

#[gpui::test]
fn disabled_controls_are_still_named(cx: &mut TestAppContext) {
    // Disabled, they leave the tab order, and their nodes say they're
    // disabled (b3_keyboard.rs), but a screen reader still names them.
    let (got, _) = drawn(true, cx);
    for id in ["save", "add", "close", "case", "regex", "wrap", "sync"] {
        let node = find(&got, id);
        assert!(is_control(node.role), "{node:#?}");
        assert!(!node.label.is_empty(), "{node:#?}");
    }
}

/// A switch and a checkbox labelled only through `aria_label`, as the setup
/// wizard labels its switches, with their final accessibility nodes.
struct CallerLabelled {
    nodes: Rc<RefCell<Vec<accesskit::Node>>>,
}

impl Render for CallerLabelled {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let controls = [
            Switch::new("ligatures")
                .aria_label("Ligatures")
                .on(true)
                .element(window, cx),
            Checkbox::new("minimap")
                .aria_label("Minimap")
                .element(window, cx),
        ];
        *self.nodes.borrow_mut() = controls.iter().filter_map(|c| c.a11y_node()).collect();
        div().children(controls)
    }
}

#[gpui::test]
fn a_label_the_caller_gave_through_aria_label_stands(cx: &mut TestAppContext) {
    let nodes = Rc::new(RefCell::new(Vec::new()));
    let seen = nodes.clone();
    let (_, cx) = cx.add_window_view(move |_, _| CallerLabelled { nodes: seen });
    cx.run_until_parked();
    let got: Vec<_> = (nodes.borrow().iter())
        .map(|n| (n.role(), n.label().map(str::to_owned), n.toggled()))
        .collect();
    assert_eq!(
        got,
        [
            (
                Role::Switch,
                Some("Ligatures".to_owned()),
                Some(gpui::Toggled::True)
            ),
            (
                Role::CheckBox,
                Some("Minimap".to_owned()),
                Some(gpui::Toggled::False)
            ),
        ]
    );
}
