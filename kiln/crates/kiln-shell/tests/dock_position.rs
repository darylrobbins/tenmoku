// SPDX-License-Identifier: Apache-2.0

//! A dock moved to another edge: at a side it draws a vertical rail on the
//! window's edge with its column towards the centre, at the bottom its
//! buttons in a row, keeping its panels, active panel, visibility and rail
//! actions; and it keeps a width for the sides and a height for the
//! bottom, each changed only there, both saved and restored. A tabbed
//! dock, a bottom panel's, has a 34 px tab strip for its title row, a tab
//! per open panel, ending in the pane controls, wherever it is; and a rail
//! action can carry a badge.

use gpui::{
    AppContext as _, Bounds, Context, Entity, FocusHandle, Focusable, IntoElement, Modifiers,
    MouseButton, ParentElement as _, Pixels, Point, Render, SharedString, Styled as _,
    TestAppContext, VisualTestContext, Window, div, point, px,
};
use kiln_command::Value;
use kiln_shell::{
    Dock, DockItem, DockPosition, DockState, DockView as _, PanelId, RailAction,
    dock::{DrawnDock, HIDE_BUTTON, MIN_DOCK_HEIGHT, MIN_DOCK_WIDTH, RailBadge, track_centre},
    pane_group::{MIN_PANE_HEIGHT, MIN_PANE_WIDTH},
};
use kiln_ui::{BadgeTone, Icon, TAB_HEIGHT};

/// The size the dock is built with.
const BUILT: Pixels = px(200.);
const NOTES: PanelId = PanelId("notes");
const LOG: PanelId = PanelId("log");
const ACTION: PanelId = PanelId("action");

struct Panel {
    id: PanelId,
    icon: Icon,
    focus: FocusHandle,
}

impl Render for Panel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full()
    }
}

impl Focusable for Panel {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl DockItem for Panel {
    fn id(&self) -> PanelId {
        self.id
    }

    fn title(&self, _: &gpui::App) -> SharedString {
        self.id.0.into()
    }

    fn icon(&self, _: &gpui::App) -> Icon {
        self.icon
    }
}

/// A centre with the dock at its position: beside it at a side, below it
/// at the bottom, as the shell places it.
struct Host {
    dock: Entity<Dock>,
    position: DockPosition,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let at = |position| (self.position == position).then(|| self.dock.clone());
        let centre = div()
            .relative()
            .flex_1()
            .min_w(MIN_PANE_WIDTH)
            .h_full()
            .child(track_centre());
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_1()
                    .min_h(MIN_PANE_HEIGHT)
                    .w_full()
                    .flex()
                    .children(at(DockPosition::Left))
                    .child(centre)
                    .children(at(DockPosition::Right)),
            )
            .children(at(DockPosition::Bottom))
    }
}

struct Opened<'a> {
    host: Entity<Host>,
    dock: Entity<Dock>,
    cx: &'a mut VisualTestContext,
}

impl Opened<'_> {
    fn draw(&mut self) {
        for _ in 0..4 {
            self.cx.run_until_parked();
            self.cx.update(|window, cx| window.draw(cx).clear(cx));
            if self.cx.update(|window, cx| window.simulate_next_frame(cx)) == 0 {
                return;
            }
        }
    }

    /// Moves the dock to `position`, as a host would: the state's position,
    /// then the view's place.
    fn move_to(&mut self, position: DockPosition) {
        self.dock.update(self.cx, |dock, cx| {
            dock.dock_mut().set_position(position);
            cx.notify();
        });
        self.host.update(self.cx, |host, cx| {
            host.position = position;
            cx.notify();
        });
        self.draw();
    }

    fn drawn(&mut self) -> DrawnDock {
        self.dock
            .read_with(self.cx, |dock, _| dock.dock().drawn().clone())
    }

    fn size(&mut self) -> Pixels {
        self.dock
            .read_with(self.cx, |dock, _| dock.dock().current_size())
    }

    /// Drags the handle on the dock's inner edge by `by`.
    fn drag(&mut self, by: Point<Pixels>) {
        let at = self.drawn().handle_bounds().expect("shown").center();
        let to = at + by;
        let none = Modifiers::none();
        self.cx.simulate_mouse_down(at, MouseButton::Left, none);
        self.cx
            .simulate_mouse_move(at + point(px(3.), px(3.)), MouseButton::Left, none);
        self.cx.simulate_mouse_move(to, MouseButton::Left, none);
        self.draw();
        self.cx.simulate_mouse_up(to, MouseButton::Left, none);
        self.draw();
    }

    /// The panels, the active one, whether it shows and the rail's icons.
    fn state(&mut self) -> (Vec<PanelId>, Option<PanelId>, bool, Vec<Icon>) {
        let drawn = self.drawn();
        self.dock.read_with(self.cx, |dock, _| {
            let dock = dock.dock();
            (
                dock.panel_ids(),
                dock.active(),
                dock.is_visible(),
                drawn.rail_icons(),
            )
        })
    }
}

/// A window with a dock at `position` holding two panels, the second
/// shown, and an action, drawn.
fn open(position: DockPosition, cx: &mut TestAppContext) -> Opened<'_> {
    open_built(position, |state| state, cx)
}

/// [`open`], its dock's state as `build` makes it.
fn open_built(
    position: DockPosition,
    build: fn(DockState) -> DockState,
    cx: &mut TestAppContext,
) -> Opened<'_> {
    let (host, cx) = cx.add_window_view(move |window, cx| {
        let panel = |id, icon, cx: &mut Context<Host>| {
            cx.new(|cx| Panel {
                id,
                icon,
                focus: cx.focus_handle(),
            })
        };
        let notes = panel(NOTES, Icon::Files, cx);
        let log = panel(LOG, Icon::ListTree, cx);
        let dock = cx.new(|cx| {
            let state = build(DockState::new(position).size(BUILT));
            let mut dock = Dock::with_state(state);
            dock.add_panel(notes, cx);
            dock.add_panel(log, cx);
            dock.add_action(
                RailAction {
                    id: ACTION.0,
                    icon: Icon::FileTerminal,
                    title: "Action".into(),
                    command: "term.toggle",
                },
                cx,
            );
            dock.show(LOG, window, cx);
            dock
        });
        Host { dock, position }
    });
    let dock = host.read_with(cx, |host, _| host.dock.clone());
    let mut opened = Opened { host, dock, cx };
    opened.draw();
    opened
}

/// The bounds of the rail's buttons, panels' then the action's.
fn buttons(drawn: &DrawnDock) -> Vec<Bounds<Pixels>> {
    [NOTES, LOG, ACTION]
        .into_iter()
        .map(|id| drawn.rail_button_bounds(id).expect("on the rail"))
        .collect()
}

/// The buttons sit in one row, left to right, in the column's title row,
/// at the top of the column, before the title.
fn assert_in_the_title_row(drawn: &DrawnDock) {
    let buttons = buttons(drawn);
    for pair in buttons.windows(2) {
        assert_eq!(pair[0].top(), pair[1].top(), "{buttons:?}");
        assert!(pair[0].right() <= pair[1].left(), "{buttons:?}");
    }
    let title = drawn.title_bounds().expect("shown");
    let column = drawn.column_bounds().expect("shown");
    // Below the column's top border.
    assert_eq!(title.top(), column.top() + px(1.));
    assert_eq!(title.left(), column.left());
    for button in &buttons {
        assert!(title.top() <= button.top(), "{button:?} in {title:?}");
        assert!(button.bottom() <= title.bottom(), "{button:?} in {title:?}");
        assert!(title.left() <= button.left(), "{button:?} in {title:?}");
    }
    let rail = drawn.rail_bounds().expect("drawn");
    assert_eq!((rail.left(), rail.top()), (title.left(), title.top()));
    assert_eq!(rail.right(), buttons[buttons.len() - 1].right());
}

#[gpui::test]
fn a_bottom_dock_moved_left_draws_a_rail_on_the_left_edge_and_back(cx: &mut TestAppContext) {
    let mut s = open(DockPosition::Bottom, cx);
    let before = s.state();
    assert_eq!(before.1, Some(LOG));
    assert!(before.2);
    let drawn = s.drawn();
    assert_in_the_title_row(&drawn);
    let column = drawn.column_bounds().expect("shown");
    assert_eq!(column.size.height, BUILT);

    s.move_to(DockPosition::Left);
    assert_eq!(s.state(), before);
    let drawn = s.drawn();
    let rail = drawn.rail_bounds().expect("drawn");
    assert_eq!(rail.left(), px(0.));
    assert!(rail.size.height > rail.size.width, "{rail:?}");
    // Panels' buttons from the top, the action pinned below them.
    let column_of_buttons = buttons(&drawn);
    for pair in column_of_buttons.windows(2) {
        assert_eq!(pair[0].left(), pair[1].left(), "{column_of_buttons:?}");
        assert!(pair[0].bottom() <= pair[1].top(), "{column_of_buttons:?}");
    }
    assert!(
        (column_of_buttons.iter()).all(|button| rail.contains(&button.center())),
        "{column_of_buttons:?}"
    );
    let column = drawn.column_bounds().expect("shown");
    assert_eq!(column.left(), rail.right());
    assert_eq!(column.size.width, BUILT);
    assert_eq!(column.size.height, rail.size.height);
    assert_eq!(
        drawn.handle_cursor(),
        Some(gpui::CursorStyle::ResizeLeftRight)
    );

    s.move_to(DockPosition::Bottom);
    assert_eq!(s.state(), before);
    let drawn = s.drawn();
    assert_in_the_title_row(&drawn);
    let column = drawn.column_bounds().expect("shown");
    assert_eq!(column.size.height, BUILT);
    assert_eq!(drawn.handle_cursor(), Some(gpui::CursorStyle::ResizeUpDown));
}

#[gpui::test]
fn a_hidden_dock_moved_stays_hidden(cx: &mut TestAppContext) {
    let mut s = open(DockPosition::Bottom, cx);
    s.dock.update(s.cx, |dock, cx| dock.toggle(cx));
    s.draw();
    // Hidden at the bottom, its buttons alone, in a row on the window's
    // bottom edge.
    let viewport = s.cx.update(|window, _| window.viewport_size());
    let drawn = s.drawn();
    assert!(drawn.column_bounds().is_none());
    let rail = drawn.rail_bounds().expect("drawn");
    assert_eq!(rail.bottom(), viewport.height);
    assert!(
        buttons(&drawn)
            .iter()
            .all(|button| button.top() == rail.top())
    );

    s.move_to(DockPosition::Right);
    let (_, active, visible, _) = s.state();
    assert_eq!((active, visible), (Some(LOG), false));
    let drawn = s.drawn();
    assert!(drawn.column_bounds().is_none());
    assert_eq!(drawn.rail_bounds().expect("drawn").right(), viewport.width);
}

#[gpui::test]
fn the_width_at_a_side_and_the_height_at_the_bottom_are_kept_apart(cx: &mut TestAppContext) {
    let mut s = open(DockPosition::Bottom, cx);
    s.drag(point(px(0.), px(-60.)));
    assert_eq!(s.size(), BUILT + px(60.));

    s.move_to(DockPosition::Left);
    assert_eq!(s.size(), BUILT);
    assert_eq!(s.drawn().column_bounds().expect("shown").size.width, BUILT);
    s.drag(point(px(100.), px(0.)));
    assert_eq!(s.size(), BUILT + px(100.));

    s.move_to(DockPosition::Right);
    assert_eq!(s.size(), BUILT + px(100.));
    s.move_to(DockPosition::Bottom);
    assert_eq!(s.size(), BUILT + px(60.));
    let column = s.drawn().column_bounds().expect("shown");
    assert_eq!(column.size.height, BUILT + px(60.));
    // `set_size` too changes only the size at its position.
    s.dock
        .update(s.cx, |dock, _| dock.dock_mut().set_size(px(120.)));
    let sizes = s.dock.read_with(s.cx, |dock, _| {
        (dock.dock().side_size(), dock.dock().bottom_size())
    });
    assert_eq!(sizes, (BUILT + px(100.), px(120.)));
}

#[gpui::test]
fn both_sizes_are_saved_and_restored(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut dock = DockState::new(DockPosition::Bottom).size(BUILT);
        dock.set_size(px(300.));
        dock.set_position(DockPosition::Right);
        dock.set_size(px(260.));
        let saved = dock.save(cx);
        let get = |key| saved.get(key).and_then(Value::as_f64);
        assert_eq!(get("size"), Some(260.));
        assert_eq!(get("side_size"), Some(260.));
        assert_eq!(get("bottom_size"), Some(300.));

        // Restored at either position, both come back.
        for position in [DockPosition::Bottom, DockPosition::Left] {
            let mut restored = DockState::new(position).size(BUILT);
            restored.restore(&saved, cx);
            assert_eq!(restored.side_size(), px(260.), "{position:?}");
            assert_eq!(restored.bottom_size(), px(300.), "{position:?}");
        }

        // Each at least its own least.
        let small = Value::from_iter([
            ("side_size", Value::from(20.)),
            ("bottom_size", Value::from(20.)),
        ]);
        let mut restored = DockState::new(DockPosition::Bottom).size(BUILT);
        restored.restore(&small, cx);
        assert_eq!(restored.side_size(), MIN_DOCK_WIDTH);
        assert_eq!(restored.bottom_size(), MIN_DOCK_HEIGHT);

        // Saved with only a size, it's the size at the dock's position.
        let old = Value::from_iter([("size", Value::from(310.))]);
        let mut restored = DockState::new(DockPosition::Bottom).size(BUILT);
        restored.restore(&old, cx);
        assert_eq!(restored.bottom_size(), px(310.));
        assert_eq!(restored.side_size(), BUILT);
    });
}

#[gpui::test]
fn a_tabbed_dock_has_a_tab_per_open_panel_ending_in_the_controls(cx: &mut TestAppContext) {
    let build = |state: DockState| state.without_rail().with_tabs();
    let mut s = open_built(DockPosition::Bottom, build, cx);
    // Built shown, NOTES showed before LOG: both have tabs. Closing NOTES
    // leaves the shown panel's tab alone.
    assert_eq!(s.drawn().tabs(), [NOTES, LOG]);
    s.dock
        .update_in(s.cx, |dock, window, cx| dock.tab_closed(NOTES, window, cx));
    s.draw();
    assert_eq!(s.state().1, Some(LOG));
    // The shown panel's tab alone: a 34 px strip, the controls at its end.
    let drawn = s.drawn();
    assert_eq!(drawn.tabs(), [LOG]);
    let title = drawn.title_bounds().expect("shown");
    assert_eq!(title.size.height, TAB_HEIGHT);
    let controls = drawn.controls();
    assert_eq!(controls.last().map(|(id, _)| id.as_str()), Some(HIDE_BUTTON));
    let hide = (s.cx.debug_bounds("icon-button:dock-hide")).expect("drawn");
    let log = drawn.tab_bounds(LOG).expect("drawn");
    assert_eq!(log.left(), title.left());
    assert!(log.right() < hide.left(), "{log:?} {hide:?}");
    assert!(title.right() - hide.right() <= px(8.), "{hide:?} {title:?}");

    // Another shown opens its tab, in the order the panels were added.
    s.dock
        .update_in(s.cx, |dock, window, cx| dock.show(NOTES, window, cx));
    s.draw();
    assert_eq!(s.drawn().tabs(), [NOTES, LOG]);
    // A click on a tab shows its panel.
    let log = s.drawn().tab_bounds(LOG).expect("drawn");
    s.cx.simulate_click(log.center(), Modifiers::none());
    s.draw();
    assert_eq!(s.state().1, Some(LOG));

    // Moved to a side, it keeps its strip.
    s.move_to(DockPosition::Left);
    let drawn = s.drawn();
    assert_eq!(drawn.tabs(), [NOTES, LOG]);
    assert_eq!(drawn.title_bounds().expect("shown").size.height, TAB_HEIGHT);
    s.move_to(DockPosition::Bottom);

    // Closing the shown tab shows the one before; closing the last hides
    // the dock, its strip empty.
    s.dock
        .update_in(s.cx, |dock, window, cx| dock.tab_closed(LOG, window, cx));
    s.draw();
    assert_eq!(s.drawn().tabs(), [NOTES]);
    assert_eq!(s.state().1, Some(NOTES));
    s.dock
        .update_in(s.cx, |dock, window, cx| dock.tab_closed(NOTES, window, cx));
    s.draw();
    assert!(!s.state().2);
    assert!(s.drawn().tabs().is_empty());
    // Shown again, the panel shown has its tab.
    s.dock
        .update_in(s.cx, |dock, window, cx| dock.show(LOG, window, cx));
    s.draw();
    assert_eq!(s.drawn().tabs(), [LOG]);
}

#[gpui::test]
fn a_rail_action_carries_the_badge_set_on_its_id(cx: &mut TestAppContext) {
    let mut s = open(DockPosition::Left, cx);
    let badge = RailBadge::dot(BadgeTone::Error, "1 error");
    s.dock.update(s.cx, |dock, cx| {
        dock.dock_mut().set_badge(ACTION, Some(badge.clone()));
        cx.notify();
    });
    s.draw();
    assert_eq!(s.drawn().rail_badges(), [(ACTION, badge)]);
    s.dock.update(s.cx, |dock, cx| {
        dock.dock_mut().set_badge(ACTION, None);
        cx.notify();
    });
    s.draw();
    assert!(s.drawn().rail_badges().is_empty());
}

#[gpui::test]
fn a_tabbed_docks_shown_panel_keeps_its_tab_however_it_came_to_show(cx: &mut TestAppContext) {
    // Built shown, its first panel shows without a `show`: another shown,
    // the first keeps its tab.
    let (dock, cx) = cx.add_window_view(|_, cx| {
        let panel = |id, icon, cx: &mut Context<Dock>| {
            cx.new(|cx| Panel {
                id,
                icon,
                focus: cx.focus_handle(),
            })
        };
        let mut dock = Dock::with_state(DockState::new(DockPosition::Bottom).with_tabs());
        let notes = panel(NOTES, Icon::Files, cx);
        let log = panel(LOG, Icon::ListTree, cx);
        dock.add_panel(notes, cx);
        dock.add_panel(log, cx);
        dock
    });
    let tabs = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        dock.read_with(cx, |dock, _| dock.dock().drawn().tabs())
    };
    assert_eq!(tabs(cx), [NOTES]);
    dock.update_in(cx, |dock, window, cx| dock.show(LOG, window, cx));
    assert_eq!(tabs(cx), [NOTES, LOG]);

    // Restored shown on a panel, then another shown: both keep tabs.
    dock.update(cx, |dock, cx| {
        let state = dock.dock_mut();
        state.close_tab(NOTES);
        state.close_tab(LOG);
        assert!(!state.is_visible());
        let saved = Value::from_iter([
            ("visible", Value::from(true)),
            ("active", Value::from(LOG.0)),
        ]);
        state.restore(&saved, cx);
    });
    assert_eq!(tabs(cx), [LOG]);
    dock.update_in(cx, |dock, window, cx| dock.show(NOTES, window, cx));
    assert_eq!(tabs(cx), [NOTES, LOG]);

    // Hidden then shown again by `set_visible`, the same.
    dock.update(cx, |dock, _| {
        let state = dock.dock_mut();
        state.close_tab(LOG);
        state.set_visible(false);
        state.set_visible(true);
    });
    assert_eq!(tabs(cx), [NOTES]);
}
