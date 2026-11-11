// SPDX-License-Identifier: Apache-2.0

//! A view stack's sections from the keyboard (A11Y-16a): in the stack's
//! key context, `view_stack.focus_next_section` and
//! `view_stack.focus_previous_section` focus the headers below and above
//! the section holding focus, wrapping and passing over the lead, and
//! `view_stack.toggle_section` collapses or expands the section holding
//! focus, leaving focus on its header. Driven by keys bound to them as a
//! keymap would, and listed for the palette by `kiln_shell::register`.

use std::{cell::RefCell, rc::Rc};

use gpui::{
    AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, KeyBinding, ParentElement as _, Render, Styled as _, TestAppContext,
    VisualTestContext, Window, div,
};
use kiln_command::{CommandId, ContextFrame, ContextStack, Value};
use kiln_shell::{
    StackedView, ViewId, ViewStack,
    view_stack::{FocusNextSection, FocusPreviousSection, KEY_CONTEXT, ToggleSection},
};

const FOLDERS: ViewId = ViewId("folders");
const OUTLINE: ViewId = ViewId("outline");
const TIMELINE: ViewId = ViewId("timeline");

/// A view's body, which takes focus as a tree would.
struct Body(FocusHandle);

impl Render for Body {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().track_focus(&self.0).size_full()
    }
}

impl Focusable for Body {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.0.clone()
    }
}

/// The stack fills the window.
struct Host(Entity<ViewStack>);

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().flex().child(self.0.clone())
    }
}

struct Opened<'a> {
    stack: Entity<ViewStack>,
    /// The views' bodies, in the stack's order.
    bodies: Vec<Entity<Body>>,
    cx: &'a mut VisualTestContext,
}

/// The views, top to bottom.
const VIEWS: [ViewId; 3] = [FOLDERS, OUTLINE, TIMELINE];

/// A window of the folders as the stack's lead, without a header, then
/// the outline and the timeline; keys bound to the stack's commands.
fn open(cx: &mut TestAppContext) -> Opened<'_> {
    cx.update(|cx| {
        kiln_shell::register(cx);
        cx.bind_keys([
            KeyBinding::new("ctrl-down", FocusNextSection, Some(KEY_CONTEXT)),
            KeyBinding::new("ctrl-up", FocusPreviousSection, Some(KEY_CONTEXT)),
            KeyBinding::new("ctrl-enter", ToggleSection, Some(KEY_CONTEXT)),
        ]);
    });
    let made = Rc::new(RefCell::new(Vec::new()));
    let bodies = made.clone();
    let (host, cx) = cx.add_window_view(move |_, cx| {
        let stack = cx.new(|cx| {
            let mut stack = ViewStack::new();
            for (id, title) in [
                (FOLDERS, "Folders"),
                (OUTLINE, "Outline"),
                (TIMELINE, "Timeline"),
            ] {
                let body = cx.new(|cx| Body(cx.focus_handle()));
                made.borrow_mut().push(body.clone());
                stack.add(StackedView::new(id, title, body).lead(id == FOLDERS), cx);
            }
            stack
        });
        Host(stack)
    });
    let stack = host.read_with(cx, |host, _| host.0.clone());
    // Focus listeners and the focus ring want an active window.
    cx.update(|window, _| window.activate_window());
    let bodies = bodies.take();
    let mut opened = Opened { stack, bodies, cx };
    opened.draw();
    opened
}

impl Opened<'_> {
    fn draw(&mut self) {
        self.cx.run_until_parked();
        self.cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        self.cx.run_until_parked();
    }

    fn keys(&mut self, keys: &str) {
        self.cx.simulate_keystrokes(keys);
        self.draw();
    }

    /// Focuses `view`'s body.
    fn focus_body(&mut self, view: ViewId) {
        let ix = VIEWS.iter().position(|id| *id == view).unwrap();
        let body = self.bodies[ix].clone();
        self.cx
            .update(|window, cx| window.focus(&body.focus_handle(cx), cx));
        self.draw();
    }

    fn body_focused(&mut self, view: ViewId) -> bool {
        let ix = VIEWS.iter().position(|id| *id == view).unwrap();
        let body = self.bodies[ix].clone();
        self.cx
            .update(|window, cx| body.focus_handle(cx).is_focused(window))
    }

    /// The view whose header has focus.
    fn header_focused(&mut self) -> Option<ViewId> {
        let stack = self.stack.clone();
        self.cx.update(|window, cx| {
            let stack = stack.read(cx);
            VIEWS.into_iter().find(|view| {
                !stack.is_headless(*view).unwrap()
                    && stack
                        .header_focus(*view)
                        .is_some_and(|focus| focus.is_focused(window))
            })
        })
    }

    fn collapsed(&mut self, view: ViewId) -> bool {
        self.stack
            .read_with(self.cx, |stack, _| stack.is_collapsed(view))
            .expect("in the stack")
    }
}

#[gpui::test]
fn next_and_previous_focus_the_headers_past_the_lead(cx: &mut TestAppContext) {
    let mut s = open(cx);
    s.focus_body(FOLDERS);

    let mut seen = Vec::new();
    for _ in 0..3 {
        s.keys("ctrl-down");
        seen.push(s.header_focused());
    }
    assert_eq!(
        seen,
        [Some(OUTLINE), Some(TIMELINE), Some(OUTLINE)],
        "down the headers, wrapping past the lead, which has none"
    );
    s.keys("ctrl-up");
    assert_eq!(s.header_focused(), Some(TIMELINE), "up wraps too");
    s.keys("ctrl-up");
    assert_eq!(s.header_focused(), Some(OUTLINE));

    // From inside a view, the next section's header, or the one above.
    s.focus_body(OUTLINE);
    s.keys("ctrl-down");
    assert_eq!(s.header_focused(), Some(TIMELINE));
    s.focus_body(TIMELINE);
    s.keys("ctrl-up");
    assert_eq!(s.header_focused(), Some(OUTLINE));
}

#[gpui::test]
fn toggle_collapses_the_section_holding_focus(cx: &mut TestAppContext) {
    let mut s = open(cx);

    // From inside the outline's view: it collapses, and focus, which its
    // view can no longer hold, goes to its header.
    s.focus_body(OUTLINE);
    s.keys("ctrl-enter");
    assert!(s.collapsed(OUTLINE));
    assert!(!s.collapsed(TIMELINE));
    assert_eq!(s.header_focused(), Some(OUTLINE));

    // From the header, it expands again; the header keeps focus.
    s.keys("ctrl-enter");
    assert!(!s.collapsed(OUTLINE));
    assert_eq!(s.header_focused(), Some(OUTLINE));

    // Moved to the timeline's header, toggling acts on the timeline.
    s.keys("ctrl-down ctrl-enter");
    assert!(s.collapsed(TIMELINE));
    assert!(!s.collapsed(OUTLINE));
    assert_eq!(s.header_focused(), Some(TIMELINE));
}

#[gpui::test]
fn the_lead_does_not_collapse(cx: &mut TestAppContext) {
    let mut s = open(cx);
    s.focus_body(FOLDERS);
    s.keys("ctrl-enter");
    assert!(!s.collapsed(FOLDERS));
    assert!(s.body_focused(FOLDERS), "focus stays in the lead's view");
    assert!(!s.collapsed(OUTLINE) && !s.collapsed(TIMELINE));
}

/// The commands are registered, titled, and listed in the stack's context,
/// as the palette lists them; the catalog checks them.
#[gpui::test]
fn the_commands_are_listed_in_the_stacks_context(cx: &mut TestAppContext) {
    let s = open(cx);
    let ids = [
        ("view_stack.toggle_section", "Toggle Section"),
        ("view_stack.focus_next_section", "Focus Next Section"),
        ("view_stack.focus_previous_section", "Focus Previous Section"),
    ];
    let inside = ContextStack::new().with(ContextFrame::new(KEY_CONTEXT));
    let listed = s
        .cx
        .update(|_, cx| kiln_shell::command_palette::commands_in(&inside, cx));
    let outside = s.cx.update(|_, cx| {
        kiln_shell::command_palette::commands_in(&ContextStack::new(), cx)
    });
    for (id, title) in ids {
        let entry = listed.iter().find(|entry| entry.id.as_str() == id);
        assert_eq!(entry.map(|entry| entry.title.as_ref()), Some(title), "{id}");
        assert!(
            !outside.iter().any(|entry| entry.id.as_str() == id),
            "{id} only in the stack"
        );
        let id = CommandId::parse(id).unwrap();
        assert_eq!(kiln_shell::catalog().check(&id, &Value::Null), Ok(()));
    }
}
