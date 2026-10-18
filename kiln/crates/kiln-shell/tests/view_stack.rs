// SPDX-License-Identifier: LicenseRef-Proprietary

//! A view stack: a click on a header, or Enter or Space pressed and
//! released on a focused one, collapses its view to the header and expands
//! it again; the expanded views fill the column by weight, and with all
//! collapsed the headers stack at the top; save and restore keep collapse
//! and weight by view id; and whatever changes them redraws.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gpui::{
    AppContext as _, Context, Entity, FocusHandle, InteractiveElement as _, IntoElement,
    KeyDownEvent, KeyUpEvent, Keystroke, Modifiers, ParentElement as _, Pixels, Render,
    StatefulInteractiveElement as _, Styled as _, TestAppContext, VisualTestContext, Window, div,
    px,
};
use kiln_command::Value;
use kiln_shell::{StackedView, ViewId, ViewStack, view_stack::VIEW_HEADER_HEIGHT};

const FOLDERS: ViewId = ViewId("folders");
const OUTLINE: ViewId = ViewId("outline");
const TIMELINE: ViewId = ViewId("timeline");

/// A view's body: it fills what it's given.
struct Body;

impl Render for Body {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full()
    }
}

/// A focusable header action that counts its clicks.
struct Action {
    clicks: Rc<Cell<usize>>,
    focus: FocusHandle,
}

impl Render for Action {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let clicks = self.clicks.clone();
        div()
            .id("action")
            .track_focus(&self.focus)
            .size(px(16.))
            .debug_selector(|| "action".into())
            .on_click(move |_, _, _| clicks.set(clicks.get() + 1))
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
    action: Entity<Action>,
    clicks: Rc<Cell<usize>>,
    cx: &'a mut VisualTestContext,
}

/// A window of folders, weight 1 with a header action; the outline,
/// weight 2; and the timeline, weight 1.
fn open(cx: &mut TestAppContext) -> Opened<'_> {
    let clicks = Rc::new(Cell::new(0));
    let made = Rc::new(RefCell::new(None));
    let (counted, made_action) = (clicks.clone(), made.clone());
    let (host, cx) = cx.add_window_view(move |_, cx| {
        let stack = cx.new(|cx| {
            let mut stack = ViewStack::new();
            let body = |cx: &mut Context<ViewStack>| cx.new(|_| Body);
            let action = cx.new(|cx| Action {
                clicks: counted,
                focus: cx.focus_handle(),
            });
            made_action.replace(Some(action.clone()));
            let folders = StackedView::new(FOLDERS, "Folders", body(cx)).header_actions(action);
            stack.add(folders, cx);
            let outline = StackedView::new(OUTLINE, "Outline", body(cx)).weight(2.);
            stack.add(outline, cx);
            stack.add(StackedView::new(TIMELINE, "Timeline", body(cx)), cx);
            stack
        });
        Host(stack)
    });
    let stack = host.read_with(cx, |host, _| host.0.clone());
    let action = made.take().expect("made");
    let mut opened = Opened {
        stack,
        action,
        clicks,
        cx,
    };
    opened.draw();
    opened
}

impl Opened<'_> {
    fn draw(&mut self) {
        self.cx.run_until_parked();
        self.cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    fn collapsed(&mut self, view: ViewId) -> bool {
        self.stack
            .read_with(self.cx, |stack, _| stack.is_collapsed(view))
            .expect("in the stack")
    }

    fn click_header(&mut self, view: ViewId) {
        let header = self
            .stack
            .read_with(self.cx, |stack, _| stack.header_bounds(view))
            .expect("drawn");
        self.cx.simulate_click(header.center(), Modifiers::none());
        self.draw();
    }

    /// Presses `key`, without releasing it.
    fn key_down(&mut self, key: &str) {
        self.cx.simulate_event(KeyDownEvent {
            keystroke: Keystroke::parse(key).unwrap(),
            is_held: false,
            prefer_character_input: false,
        });
        self.draw();
    }

    fn key_up(&mut self, key: &str) {
        self.cx.simulate_event(KeyUpEvent {
            keystroke: Keystroke::parse(key).unwrap(),
        });
        self.draw();
    }

    /// Presses and releases `key`, as a real key press does.
    fn press(&mut self, key: &str) {
        self.key_down(key);
        self.key_up(key);
    }

    fn header_focused(&mut self, view: ViewId) -> bool {
        let stack = self.stack.clone();
        self.cx.update(|window, cx| {
            let focus = stack.read(cx).header_focus(view).expect("in the stack");
            focus.is_focused(window)
        })
    }

    /// Each view's header top and body height, none collapsed, and the
    /// column's top and bottom.
    fn layout(&mut self) -> (Vec<(Pixels, Option<Pixels>)>, Pixels, Pixels) {
        self.stack.read_with(self.cx, |stack, _| {
            let views = (stack.view_ids().into_iter())
                .map(|view| {
                    let header = stack.header_bounds(view).expect("drawn");
                    assert_eq!(header.size.height, VIEW_HEADER_HEIGHT, "{view:?}");
                    let body = stack.body_bounds(view).map(|body| {
                        assert_eq!(body.top(), header.bottom(), "{view:?}");
                        body.size.height
                    });
                    (header.top(), body)
                })
                .collect();
            let column = stack.bounds();
            (views, column.top(), column.bottom())
        })
    }

    /// Sets each view's collapse and weight, then draws.
    fn set(&mut self, views: [(ViewId, bool, f32); 3]) {
        self.stack.update(self.cx, |stack, cx| {
            for (view, collapsed, weight) in views {
                assert!(stack.set_collapsed(view, collapsed, cx));
                assert!(stack.set_weight(view, weight, cx));
            }
        });
        self.draw();
    }

    /// The expanded views' bodies each have their share of what the
    /// headers leave of the column, filling it; the collapsed are headers
    /// alone, and with all collapsed the column below them is empty.
    fn assert_shares(&mut self, shares: [Option<f32>; 3]) {
        let (views, top, bottom) = self.layout();
        let free = bottom - top - VIEW_HEADER_HEIGHT * 3.;
        assert!(free > px(30.), "{free:?}");
        assert_eq!(views[0].0, top);
        for ((_, body), share) in views.iter().zip(shares) {
            match (body, share) {
                (Some(body), Some(share)) => {
                    assert!(near(*body, free * share), "{views:?} {shares:?} {free:?}")
                }
                (body, share) => assert_eq!(body.is_some(), share.is_some(), "{views:?}"),
            }
        }
        let filled = views[2].0 + VIEW_HEADER_HEIGHT + views[2].1.unwrap_or_default();
        match shares.iter().any(Option::is_some) {
            true => assert!(near(filled, bottom), "{filled:?} {bottom:?}"),
            false => assert!(filled < bottom, "{filled:?} {bottom:?}"),
        }
    }
}

fn near(a: Pixels, b: Pixels) -> bool {
    (f32::from(a) - f32::from(b)).abs() <= 1.
}

#[gpui::test]
fn a_header_click_or_enter_or_space_toggles_its_view(cx: &mut TestAppContext) {
    let mut s = open(cx);
    assert!(!s.collapsed(OUTLINE));

    // A click collapses the outline to its header, and focuses it.
    s.click_header(OUTLINE);
    assert!(s.collapsed(OUTLINE));
    assert!(s.header_focused(OUTLINE));
    let (views, _, _) = s.layout();
    assert_eq!(views[1].1, None, "collapsed, the header alone");
    assert_eq!(views[2].0, views[1].0 + VIEW_HEADER_HEIGHT);
    // The others are as they were.
    assert!(!s.collapsed(FOLDERS) && !s.collapsed(TIMELINE));

    // Space on the focused header toggles it once, as it's released.
    s.key_down("space");
    assert!(s.collapsed(OUTLINE));
    s.key_up("space");
    assert!(!s.collapsed(OUTLINE));
    assert!(s.layout().0[1].1.is_some());
    // So does Enter; modified, they don't.
    s.press("enter");
    assert!(s.collapsed(OUTLINE));
    s.press("enter");
    assert!(!s.collapsed(OUTLINE));
    s.press("shift-enter");
    s.press("cmd-space");
    assert!(!s.collapsed(OUTLINE));

    // A click again collapses it.
    s.click_header(OUTLINE);
    assert!(s.collapsed(OUTLINE));

    // A click on a header's action runs it, without toggling the view.
    let action = s.cx.debug_bounds("action").expect("drawn");
    s.cx.simulate_click(action.center(), Modifiers::none());
    s.draw();
    assert_eq!(s.clicks.get(), 1);
    assert!(!s.collapsed(FOLDERS));

    // So do Enter and Space on the action focused.
    let action = s.action.clone();
    s.cx.update(|window, cx| window.focus(&action.read(cx).focus.clone(), cx));
    s.draw();
    s.press("enter");
    s.press("space");
    assert_eq!(s.clicks.get(), 3);
    assert!(!s.collapsed(FOLDERS));
}

#[gpui::test]
fn expanded_views_fill_the_column_by_weight(cx: &mut TestAppContext) {
    let mut s = open(cx);

    // Weights 1, 2 and 1: the bodies split what the headers leave 1:2:1.
    s.assert_shares([Some(0.25), Some(0.5), Some(0.25)]);

    // The outline collapsed, folders and timeline share the rest 1:1.
    s.click_header(OUTLINE);
    s.assert_shares([Some(0.5), None, Some(0.5)]);

    // Weights totalling less than 1 fill it all the same.
    s.set([
        (FOLDERS, false, 0.25),
        (OUTLINE, true, 2.),
        (TIMELINE, false, 0.125),
    ]);
    s.assert_shares([Some(2. / 3.), None, Some(1. / 3.)]);
    // As does a lone expanded view of weight 0.5.
    s.set([
        (FOLDERS, true, 1.),
        (OUTLINE, false, 0.5),
        (TIMELINE, true, 1.),
    ]);
    s.assert_shares([None, Some(1.), None]);
    // And views of the least and the most weight, the most taking it all.
    s.set([
        (FOLDERS, false, f32::MIN_POSITIVE),
        (OUTLINE, false, f32::MAX),
        (TIMELINE, false, f32::MAX),
    ]);
    s.assert_shares([Some(0.), Some(0.5), Some(0.5)]);

    // With all collapsed, the headers stack at the top, the rest empty.
    s.set([
        (FOLDERS, true, 1.),
        (OUTLINE, true, 1.),
        (TIMELINE, true, 1.),
    ]);
    s.assert_shares([None, None, None]);
    let (views, top, _) = s.layout();
    for (ix, (header, _)) in views.iter().enumerate() {
        assert_eq!(*header, top + VIEW_HEADER_HEIGHT * ix as f32);
    }

    // Expanding one brings its body back, filling the rest.
    s.click_header(TIMELINE);
    s.assert_shares([None, None, Some(1.)]);
}

#[gpui::test]
fn save_and_restore_keep_collapse_and_weight_by_id(cx: &mut TestAppContext) {
    let s = open(cx);
    let saved = s.stack.update(s.cx, |stack, cx| {
        stack.set_collapsed(OUTLINE, true, cx);
        stack.set_weight(TIMELINE, 0.5, cx);
        stack.save()
    });
    assert_eq!(
        saved,
        serde_json::json!({"views": {
            "folders": {"collapsed": false, "weight": 1.0},
            "outline": {"collapsed": true, "weight": 2.0},
            "timeline": {"collapsed": false, "weight": 0.5},
        }})
    );

    // A stack of the same views, built another way, takes it back.
    let restored = s.cx.new(|cx| {
        let mut stack = ViewStack::new();
        for view in [FOLDERS, OUTLINE, TIMELINE] {
            let body = cx.new(|_| Body);
            let built = StackedView::new(view, view.0, body).collapsed(view == FOLDERS);
            stack.add(built, cx);
        }
        stack.restore(&saved, cx);
        stack
    });
    assert_eq!(restored.read_with(s.cx, |stack, _| stack.save()), saved);

    // An unknown id is ignored; a view missing from what was saved, a
    // missing value or a weight that isn't positive keeps its own.
    let partial = serde_json::json!({"views": {
        "search": {"collapsed": true, "weight": 9.0},
        "outline": {"weight": 0.0},
        "timeline": {"collapsed": true},
    }});
    s.stack.update(s.cx, |stack, cx| {
        stack.restore(&partial, cx);
        assert_eq!(stack.view_ids(), [FOLDERS, OUTLINE, TIMELINE]);
        assert_eq!(stack.is_collapsed(FOLDERS), Some(false));
        assert_eq!(stack.weight(FOLDERS), Some(1.));
        assert_eq!(stack.is_collapsed(OUTLINE), Some(true));
        assert_eq!(stack.weight(OUTLINE), Some(2.));
        assert_eq!(stack.is_collapsed(TIMELINE), Some(true));
        assert_eq!(stack.weight(TIMELINE), Some(0.5));
        // Nothing saved at all leaves it as it is.
        stack.restore(&Value::Null, cx);
        assert_eq!(stack.is_collapsed(TIMELINE), Some(true));
    });
}

#[gpui::test]
fn each_change_redraws_the_stack(cx: &mut TestAppContext) {
    let s = open(cx);
    let notified = Rc::new(Cell::new(0));
    let count = notified.clone();
    let stack = s.stack.clone();
    let _observing =
        s.cx.update(|_, cx| cx.observe(&stack, move |_, _| count.set(count.get() + 1)));
    let mut changed = |change: &dyn Fn(&mut ViewStack, &mut Context<ViewStack>)| {
        let before = notified.get();
        stack.update(s.cx, |stack, cx| change(stack, cx));
        s.cx.run_until_parked();
        notified.get() > before
    };
    assert!(changed(&|stack, cx| assert!(stack.toggle(OUTLINE, cx))));
    assert!(changed(&|stack, cx| assert!(
        stack.set_collapsed(OUTLINE, false, cx)
    )));
    assert!(changed(&|stack, cx| assert!(
        stack.set_weight(OUTLINE, 3., cx)
    )));
    assert!(changed(&|stack, cx| {
        let saved = serde_json::json!({"views": {"outline": {"collapsed": true}}});
        stack.restore(&saved, cx);
    }));
    assert!(changed(&|stack, cx| {
        let body = cx.new(|_| Body);
        stack.add(StackedView::new(ViewId("search"), "Search", body), cx);
    }));
}
