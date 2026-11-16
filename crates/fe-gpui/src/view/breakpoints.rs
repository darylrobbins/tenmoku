//! A debugger's marks over the line numbers
//! (`docs/design/design-system.md`, Debugger; `docs/decisions/r1b9dw3g0g-debugger.md`
//! §5, card D6): [`EditorView::set_breakpoints`] and
//! [`EditorView::set_breakpoint_handlers`].
//!
//! - **Where.** B3's gutter stays 81 px, so there is no lane: the marks
//!   draw in the line-number column (the inset and the numbers), which has
//!   two slots, the breakpoint's and the frame arrow's
//!   ([`breakpoint_slot`], [`frame_slot`]). A row with a mark in either
//!   draws no number; its tooltip and accessible name give it. The problem
//!   dot, the host's glyph ([`crate::GutterGlyph`]), the fold column and
//!   the change bar don't move.
//! - **Rows** are buffer rows of the text at the marks' version, anchored
//!   at each row's start with [`Bias::Right`] as the breakpoint store
//!   anchors them, so they follow edits until the host sets them again.
//!   Two on one row show as one: the first's breakpoint, and the current
//!   frame's arrow over a caller's.
//! - **Pointer.** With handlers, the column is a click target on every
//!   display row of a line. A plain left click toggles a breakpoint on
//!   that buffer row ([`BreakpointHandlers::on_toggle`]) and leaves the
//!   carets be; a press that moves [`DRAG_SLOP`] before its release is a
//!   drag, as anywhere else in the gutter. A right click opens the host's
//!   menu for the row ([`BreakpointHandlers::menu`]), drawn by the view,
//!   its items named for assistive technology. Over the column the hovered
//!   row's number sits on the fold chevrons' hover tile, and after a rest
//!   its tooltip says what's there ("Add breakpoint, line 7"), which is the
//!   row's accessible name too.
//! - Without a gutter ([`EditorView::set_gutter`]) nothing here draws or
//!   is clicked; the debugger's bands are decorations, which still draw.

use std::{path::Path, rc::Rc};

use fe_buffer::{Anchor, Bias, Version};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AccessibleAction, AnyElement, App, AppContext as _, AvailableSpace, Bounds, Context,
    InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent, ParentElement as _, Pixels,
    Point, SharedString, StatefulInteractiveElement as _, Styled as _, WeakEntity, Window,
    anchored, deferred, div, px, size,
};
use kiln_l10n::tr;

use crate::{EditorView, element::FrameLayout};

/// How far a press on the column may move before it's a drag rather than a
/// click.
pub(crate) const DRAG_SLOP: f32 = 3.;

/// A breakpoint's state, as its glyph shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BreakpointState {
    /// Enabled and verified, or no session to verify it: a filled dot.
    Enabled,
    /// Enabled, and the session's adapter hasn't bound it: a ring. Its
    /// tooltip carries the adapter's message, if it gave one.
    Unverified { message: Option<SharedString> },
    /// Disabled: a grey dot.
    Disabled,
}

/// A stopped session's frame arrow, in the slot right of the breakpoint's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameArrow {
    /// The top frame of the stopped thread: a filled arrow.
    Current,
    /// A caller frame selected in the call stack: an outlined arrow.
    Caller,
}

/// What one buffer row shows over its line number.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BreakpointMark {
    /// The buffer row, from 0, in the text at the marks' version.
    pub row: u32,
    pub breakpoint: Option<BreakpointState>,
    pub frame: Option<FrameArrow>,
    /// The buffer row the breakpoint drawn here is set on, when that's not
    /// this one (one the adapter moved): a click and the menu here act on
    /// that row. It follows edits as the mark does.
    pub target: Option<u32>,
}

/// An item of a row's breakpoint menu.
#[derive(Clone)]
pub struct BreakpointMenuItem {
    /// Its title, which names it for assistive technology too.
    pub title: SharedString,
    /// What choosing it does, once the view is done with the click.
    #[allow(clippy::type_complexity)]
    pub run: Rc<dyn Fn(&mut Window, &mut App)>,
}

/// What a click and a right click on the line-number column do.
#[derive(Clone)]
pub struct BreakpointHandlers {
    /// A plain click on buffer row `row`'s numbers: add or remove one.
    #[allow(clippy::type_complexity)]
    pub on_toggle: Rc<dyn Fn(u32, &mut Window, &mut App)>,
    /// The menu a right click on buffer row `row` opens; none opens for
    /// an empty list.
    #[allow(clippy::type_complexity)]
    pub menu: Rc<dyn Fn(u32, &App) -> Vec<BreakpointMenuItem>>,
}

/// A screen reader's Click on a row's target toggles, as a click does; the
/// pointer's own go through the editor. The handler is kept in `clicks`
/// too, since a window only registers it once assistive technology is
/// connected, which a test's never is.
fn on_a11y_click<E: gpui::InteractiveElement + gpui::StatefulInteractiveElement>(
    target: E,
    row: u32,
    this: WeakEntity<EditorView>,
    clicks: &mut Vec<(u32, A11yClick)>,
) -> E {
    let click: A11yClick = Rc::new(move |window, cx| {
        (this.update(cx, |view, cx| view.activate_breakpoint_row(row, window, cx))).ok();
    });
    clicks.push((row, click.clone()));
    target.on_a11y_action(AccessibleAction::Click, move |_, window, cx| {
        click(window, cx)
    })
}

/// What a row's node does for an accessibility Click.
type A11yClick = Rc<dyn Fn(&mut Window, &mut App)>;

/// A row's open menu, for the text and marks it was opened on.
struct OpenMenu {
    position: Point<Pixels>,
    items: Vec<BreakpointMenuItem>,
    /// The buffer's version and file then: an edit, a Save As or new marks
    /// make its row and items stale, and it closes.
    version: Version,
    path: Option<std::path::PathBuf>,
    /// The item the keyboard is on, from 0.
    selected: usize,
}

/// The view's marks and what's under way with the pointer.
#[derive(Default)]
pub(crate) struct Breakpoints {
    /// Each mark, anchored at its row's start, and its target's row.
    marks: Vec<(Anchor, BreakpointMark, Option<Anchor>)>,
    handlers: Option<BreakpointHandlers>,
    /// The Click each row's node in the last frame was given.
    a11y_clicks: Vec<(u32, A11yClick)>,
    /// A press on the column waiting for its release: its row and event.
    pending: Option<(u32, MouseDownEvent)>,
    /// While a press that became a drag is handed on.
    handing_on: bool,
    menu: Option<OpenMenu>,
    /// The column's rows the last frame laid out, in window coordinates:
    /// each buffer row, all its display rows, and its mark.
    pub(crate) targets: Vec<(u32, Bounds<Pixels>, Option<BreakpointMark>)>,
}

impl EditorView {
    /// Shows `marks` over the line numbers, replacing any shown: see
    /// [`crate::view::breakpoints`]. Their rows are those of the text at
    /// `version`, from which they follow edits. Marks for another version
    /// than the buffer's, whose rows it can't tell, show nothing until the
    /// host sets them again.
    pub fn set_breakpoints(
        &mut self,
        version: Version,
        marks: Vec<BreakpointMark>,
        cx: &mut Context<Self>,
    ) {
        let snapshot = self.model.snapshot();
        let marks = match version == snapshot.version() {
            true => marks,
            false => Vec::new(),
        };
        // Marks that changed make an open menu's items stale.
        let shown: Vec<&BreakpointMark> = self
            .breakpoints
            .marks
            .iter()
            .map(|(_, mark, _)| mark)
            .collect();
        if shown.len() != marks.len() || shown.iter().zip(&marks).any(|(old, new)| *old != new) {
            self.breakpoints.menu = None;
        }
        let text = self.model.text();
        let last = text.row_count().saturating_sub(1);
        self.breakpoints.marks = (marks.into_iter())
            .map(|mark| {
                let at = |row: u32| snapshot.anchor_at(text.row_start(row.min(last)), Bias::Right);
                let target = mark.target.map(at);
                (at(mark.row), mark, target)
            })
            .collect();
        cx.notify();
    }

    /// Makes the line-number column a click target, toggling and offering
    /// a menu through `handlers`, or not (`None`).
    pub fn set_breakpoint_handlers(
        &mut self,
        handlers: Option<BreakpointHandlers>,
        cx: &mut Context<Self>,
    ) {
        let off = handlers.is_none();
        self.breakpoints.handlers = handlers;
        if off {
            self.breakpoints.pending = None;
            self.breakpoints.menu = None;
        }
        cx.notify();
    }

    /// Whether the line-number column takes clicks.
    pub fn has_breakpoint_handlers(&self) -> bool {
        self.breakpoints.handlers.is_some()
    }

    /// The marks, each on its row as edits have moved it, by row: one per
    /// row, the first's breakpoint and the current frame's arrow over a
    /// caller's.
    pub fn breakpoint_marks(&self) -> Vec<BreakpointMark> {
        let snapshot = self.model.snapshot();
        let text = self.model.text();
        let floor = snapshot.oldest_version();
        // An anchor the log was compacted past can't be read: its mark goes
        // until the host sets the marks again.
        let mut marks: Vec<BreakpointMark> = (self.breakpoints.marks.iter())
            .filter(|(anchor, _, target)| {
                anchor.version() >= floor && target.is_none_or(|target| target.version() >= floor)
            })
            .map(|(anchor, mark, target)| {
                let row = |anchor: &Anchor| text.offset_to_point(snapshot.resolve(anchor)).row;
                BreakpointMark {
                    row: row(anchor),
                    target: target.as_ref().map(row),
                    ..mark.clone()
                }
            })
            .collect();
        marks.sort_by_key(|mark| mark.row);
        let mut merged: Vec<BreakpointMark> = Vec::with_capacity(marks.len());
        for mark in marks {
            match merged.last_mut() {
                Some(last) if last.row == mark.row => {
                    if last.breakpoint.is_none() {
                        last.breakpoint = mark.breakpoint;
                        last.target = mark.target;
                    }
                    last.frame = match (last.frame, mark.frame) {
                        (Some(FrameArrow::Current), _) | (_, Some(FrameArrow::Current)) => {
                            Some(FrameArrow::Current)
                        }
                        (frame, other) => frame.or(other),
                    };
                }
                _ => merged.push(mark),
            }
        }
        merged.retain(|mark| mark.breakpoint.is_some() || mark.frame.is_some());
        merged
    }

    /// The row a click or the menu on buffer row `row` acts on: the target
    /// of the breakpoint drawn there, else the row itself.
    fn action_row(&self, row: u32) -> u32 {
        (self.breakpoint_marks().into_iter())
            .find(|mark| mark.row == row)
            .and_then(|mark| mark.target)
            .unwrap_or(row)
    }

    /// What a screen reader's Click on buffer row `row`'s line number does:
    /// toggles a breakpoint there, as a click does. For tests outside the
    /// crate too, which have no screen reader to ask.
    #[doc(hidden)]
    pub fn activate_breakpoint_row(
        &mut self,
        row: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(handlers) = self.breakpoints.handlers.clone().filter(|_| self.gutter) else {
            return;
        };
        let (row, on_toggle) = (self.action_row(row), handlers.on_toggle);
        window.defer(cx, move |window, cx| on_toggle(row, window, cx));
    }

    /// The buffer row whose line-number column, in the last frame, holds
    /// window `position`: with handlers and a gutter only.
    pub(crate) fn breakpoint_row_at(&self, position: Point<Pixels>) -> Option<u32> {
        if !self.gutter || self.breakpoints.handlers.is_none() {
            return None;
        }
        let layout = self.layout.as_ref()?;
        let column = layout.bounds(number_column(layout));
        if !column.contains(&position) {
            return None;
        }
        let frame = &layout.frame;
        let y = f32::from(position.y - layout.origin.y);
        (frame.rows.iter())
            .find(|row| {
                let top = frame.row_top(row.row);
                top <= y && y < top + frame.line_height
            })
            .map(|row| row.buffer_row)
    }

    /// A left press, before anything else in the gutter takes it: on the
    /// line-number column, a plain one waits for its release to toggle a
    /// breakpoint ([`Self::release_breakpoint`]). Whether it took it.
    pub(crate) fn press_breakpoint(&mut self, event: &MouseDownEvent) -> bool {
        if self.breakpoints.handing_on
            || event.button != MouseButton::Left
            || event.modifiers.modified()
        {
            return false;
        }
        let Some(row) = self.breakpoint_row_at(event.position) else {
            return false;
        };
        self.breakpoints.pending = Some((row, event.clone()));
        true
    }

    /// The pointer moved with a press on the column waiting: past
    /// [`DRAG_SLOP`] it's a drag, handed on as a press there would have
    /// gone without breakpoints. Whether one was waiting and still is.
    pub(crate) fn drag_breakpoint_press(
        &mut self,
        position: Point<Pixels>,
        pressed: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some((_, down)) = &self.breakpoints.pending else {
            return false;
        };
        if !pressed {
            self.breakpoints.pending = None;
            return false;
        }
        let moved = position - down.position;
        let far = f32::from(moved.x).hypot(f32::from(moved.y)) > DRAG_SLOP;
        if !far {
            return true;
        }
        let Some((_, down)) = self.breakpoints.pending.take() else {
            return false;
        };
        self.breakpoints.handing_on = true;
        self.press_proceeds(&down, window, cx);
        self.breakpoints.handing_on = false;
        false
    }

    /// A left release: a press on the column waiting, released on its row,
    /// toggles that row's breakpoint. Whether one was waiting.
    pub(crate) fn release_breakpoint(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some((row, _)) = self.breakpoints.pending.take() else {
            return false;
        };
        let on_row = self.breakpoint_row_at(position) == Some(row);
        if let Some(handlers) = self.breakpoints.handlers.clone().filter(|_| on_row) {
            // The host updates this view, which is busy until then.
            let (row, on_toggle) = (self.action_row(row), handlers.on_toggle);
            window.defer(cx, move |window, cx| on_toggle(row, window, cx));
        }
        true
    }

    /// A right press: on the line-number column, opens the host's menu for
    /// its row where it was pressed, once the press is handled.
    pub(crate) fn on_right_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(row) = self.breakpoint_row_at(event.position) else {
            return;
        };
        let Some(handlers) = self.breakpoints.handlers.clone() else {
            return;
        };
        cx.stop_propagation();
        let row = self.action_row(row);
        // Asked once this view is done with the press, as the host reads it.
        let (view, position) = (cx.entity().downgrade(), event.position);
        window.defer(cx, move |window, cx| {
            let items = (handlers.menu)(row, cx);
            if items.is_empty() {
                return;
            }
            (view.update(cx, |view, cx| {
                let version = view.model.snapshot().version();
                let path = view.model.path().map(Path::to_owned);
                view.breakpoints.menu = Some(OpenMenu {
                    position,
                    items,
                    version,
                    path,
                    selected: 0,
                });
                window.focus(&view.focus, cx);
                cx.notify();
            }))
            .ok();
        });
    }

    /// What buffer row `row`'s line number says, as its tooltip and its
    /// accessible name. For tests outside the crate.
    #[doc(hidden)]
    pub fn breakpoint_row_label(&self, row: u32) -> SharedString {
        let marks = self.breakpoint_marks();
        row_label(row, marks.iter().find(|mark| mark.row == row))
    }

    /// What the accessibility Click the last frame gave row `row`'s node
    /// does, as a screen reader's request would run it, if it has a node.
    /// For tests.
    #[cfg(test)]
    pub(crate) fn a11y_click_of_breakpoint_row(&self, row: u32) -> Option<A11yClick> {
        (self.breakpoints.a11y_clicks.iter())
            .find_map(|(at, click)| (*at == row).then(|| click.clone()))
    }

    /// The open menu's items' titles. For tests outside the crate.
    #[doc(hidden)]
    pub fn breakpoint_menu(&self) -> Option<Vec<SharedString>> {
        let menu = self.breakpoints.menu.as_ref()?;
        Some(menu.items.iter().map(|item| item.title.clone()).collect())
    }

    /// Closes the menu.
    pub(crate) fn close_breakpoint_menu(&mut self, cx: &mut Context<Self>) {
        if self.breakpoints.menu.take().is_some() {
            cx.notify();
        }
    }

    /// A key while the menu is open: Up and Down move through its items,
    /// Home and End to the first and last, Enter and Space choose, Escape
    /// closes it. The editor doesn't see the keys it takes (the key context holds them from
    /// the keymap); focus stays on
    /// the editor, so closing leaves it where it was. Whether it took it.
    pub(crate) fn breakpoint_menu_key(
        &mut self,
        keystroke: &gpui::Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(menu) = self.breakpoints.menu.as_mut() else {
            return false;
        };
        // The platform modifier's keys stay the app's; every other key is
        // the menu's while it's open, shifted or not, and the editor, vim
        // included, doesn't see it.
        if keystroke.modifiers.platform {
            return false;
        }
        let last = menu.items.len().saturating_sub(1);
        match keystroke.key.as_str() {
            "escape" => self.close_breakpoint_menu(cx),
            "down" => menu.selected = (menu.selected + 1).min(last),
            "up" => menu.selected = menu.selected.saturating_sub(1),
            "home" => menu.selected = 0,
            "end" => menu.selected = last,
            "enter" | "space" => {
                let ix = menu.selected;
                self.choose_breakpoint_menu_item(ix, window, cx);
            }
            // Typed text and the vim keys among it are the menu's, not the
            // editor's, while it's open.
            _ => {}
        }
        cx.notify();
        true
    }

    /// Whether the menu is open: the editor's key context then holds its
    /// keys (kiln's `HOLDS_KEYS`), so no binding runs for them and
    /// [`EditorView::breakpoint_menu_key`] reads them.
    pub(crate) fn breakpoint_menu_open(&self) -> bool {
        self.breakpoints.menu.is_some()
    }

    /// The open menu's item the keyboard is on. For tests.
    #[cfg(test)]
    pub(crate) fn breakpoint_menu_selected(&self) -> Option<usize> {
        Some(self.breakpoints.menu.as_ref()?.selected)
    }

    /// Chooses item `ix` of the open menu, closing it. For the menu's rows
    /// and for tests outside the crate.
    #[doc(hidden)]
    pub fn choose_breakpoint_menu_item(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(menu) = self.breakpoints.menu.take() else {
            return;
        };
        cx.notify();
        if !self.menu_is_current(&menu) {
            return;
        }
        if let Some(item) = menu.items.get(ix) {
            let run = item.run.clone();
            window.defer(cx, move |window, cx| run(window, cx));
        }
    }

    /// Whether `menu` was opened on the text and file the view has now.
    fn menu_is_current(&self, menu: &OpenMenu) -> bool {
        menu.version == self.model.snapshot().version() && menu.path.as_deref() == self.model.path()
    }

    /// The open menu, over the window where it was asked for; closed if
    /// the text or the file changed under it.
    pub(crate) fn breakpoint_menu_layer(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        use fe_ui::ActiveTheme as _;
        if (self.breakpoints.menu.as_ref()).is_some_and(|menu| !self.menu_is_current(menu)) {
            self.breakpoints.menu = None;
        }
        let menu = self.breakpoints.menu.as_ref()?;
        let colors = cx.theme().colors;
        let mut rows = Vec::with_capacity(menu.items.len());
        let selected = menu.selected;
        for (ix, item) in menu.items.iter().enumerate() {
            let row = div()
                .id(("breakpoint-menu-item", ix))
                .flex()
                .items_center()
                .h(fe_ui::height::ROW)
                .px_3()
                .rounded(fe_ui::radius::CONTROL)
                .hover(|row| row.bg(colors.state.hover))
                .when(ix == selected, |row| row.bg(colors.state.hover))
                .child(item.title.clone())
                .on_click(cx.listener(move |view, _, window, cx| {
                    cx.stop_propagation();
                    view.choose_breakpoint_menu_item(ix, window, cx);
                }));
            let row = fe_ui::node(row, window, cx, fe_ui::Role::MenuItem, item.title.clone());
            rows.push(row);
        }
        let panel = div()
            .id("breakpoint-menu")
            .debug_selector(|| "breakpoint-menu".into())
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down_out(cx.listener(|view, _, _, cx| view.close_breakpoint_menu(cx)))
            .min_w(px(160.))
            .flex()
            .flex_col()
            .p_1()
            .bg(colors.surface.panel)
            .border_1()
            .border_color(colors.border.default)
            .rounded(fe_ui::radius::MENU)
            .shadow_lg()
            .text_color(colors.text.primary)
            .children(rows);
        let panel = fe_ui::node(
            panel,
            window,
            cx,
            fe_ui::Role::Menu,
            tr!("gpui-debug-breakpoint-menu"),
        );
        Some(
            deferred(
                anchored()
                    .position(menu.position)
                    .snap_to_window()
                    .child(panel),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// The column's rows as the last frame laid them out, each a target
    /// that names its row for assistive technology and, after a rest under
    /// the pointer, shows the same words as a tooltip.
    pub(crate) fn layout_breakpoint_targets(
        &mut self,
        this: WeakEntity<Self>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        let targets = &self.breakpoints.targets;
        let mut clicks = Vec::new();
        let first = targets.first()?;
        let mut column = first.1;
        for (_, bounds, _) in targets {
            column = column.union(bounds);
        }
        let ui = fe_ui::ui_theme(cx);
        // Named only for a screen reader: a frame's labels aren't free.
        let named = window.is_a11y_active();
        let mut children = Vec::with_capacity(targets.len());
        for (row, bounds, mark) in targets {
            let (row, mark, this) = (*row, mark.clone(), this.clone());
            let label = named.then(|| row_label(row, mark.as_ref()));
            let target = div()
                .id(("breakpoint-row", row as usize))
                .absolute()
                .left(bounds.left() - column.left())
                .top(bounds.top() - column.top())
                .w(bounds.size.width)
                .h(bounds.size.height)
                .tooltip(move |_, cx| {
                    let tip = row_label(row, mark.as_ref());
                    cx.new(move |_| fe_ui::Tooltip::new(tip, None::<SharedString>, &ui))
                        .into()
                })
                // A screen reader's Click toggles, as a click does; the
                // pointer's own go through the editor.
                ;
            let target = on_a11y_click(target, row, this.clone(), &mut clicks);
            let target = match label {
                Some(label) => fe_ui::node(target, window, cx, fe_ui::Role::Button, label),
                None => target,
            };
            children.push(target);
        }
        self.breakpoints.a11y_clicks = clicks;
        let mut element = div()
            .w(column.size.width)
            .h(column.size.height)
            .children(children)
            .into_any_element();
        let space = size(
            AvailableSpace::Definite(column.size.width),
            AvailableSpace::Definite(column.size.height),
        );
        element.layout_as_root(space, window, cx);
        element.prepaint_at(column.origin, window, cx);
        Some(element)
    }
}

/// The line-number column of `layout`'s frame, in frame coordinates: the
/// room before the numbers and the numbers, after any annotation column.
pub(crate) fn number_column(layout: &FrameLayout) -> fe_display::layout::Rect {
    let frame = &layout.frame;
    let left = frame.annotation_column.x + frame.annotation_column.width;
    fe_display::layout::Rect {
        x: left,
        y: 0.,
        width: (frame.problem_column.x - left).max(0.),
        height: frame.height,
    }
}

/// The design's line-number column at 13 px code: 42 px, the breakpoint's
/// slot x 14–26 and the frame arrow's x 29–41 in it, from its left. Wider
/// numbers widen it to the left, so the slots keep to its right edge.
const COLUMN: f32 = 42.;
const BREAKPOINT_X: f32 = 14.;
const FRAME_X: f32 = 29.;

/// `width` at 13 px code, scaled to `font_size` and rounded, as the
/// gutter's columns are.
fn scaled(width: f32, font_size: f32) -> f32 {
    (width * font_size / 13.).round()
}

/// The breakpoint's slot on a row whose top is `top`, in frame coordinates,
/// for a column ending at `right`: a square [`fe_ui::glyph::BREAKPOINT`]
/// across, centred in the row.
pub(crate) fn breakpoint_slot(
    right: f32,
    top: f32,
    line_height: f32,
    font_size: f32,
) -> fe_display::layout::Rect {
    let side = scaled(f32::from(fe_ui::glyph::BREAKPOINT), font_size);
    fe_display::layout::Rect {
        x: right - scaled(COLUMN - BREAKPOINT_X, font_size),
        y: top + ((line_height - side) / 2.).round(),
        width: side,
        height: side,
    }
}

/// The frame arrow's slot, as [`breakpoint_slot`]: [`fe_ui::glyph::BREAKPOINT`]
/// wide and [`fe_ui::glyph::ARROW_HEIGHT`] tall.
pub(crate) fn frame_slot(
    right: f32,
    top: f32,
    line_height: f32,
    font_size: f32,
) -> fe_display::layout::Rect {
    let width = scaled(f32::from(fe_ui::glyph::BREAKPOINT), font_size);
    let height = scaled(f32::from(fe_ui::glyph::ARROW_HEIGHT), font_size);
    fe_display::layout::Rect {
        x: right - scaled(COLUMN - FRAME_X, font_size),
        y: top + ((line_height - height) / 2.).round(),
        width,
        height,
    }
}

/// What a row's tooltip and accessible name say: what its mark is, or
/// that a click adds a breakpoint, with its line number (`row` from 0).
pub(crate) fn row_label(row: u32, mark: Option<&BreakpointMark>) -> SharedString {
    let line = row + 1;
    let mut parts: Vec<String> = Vec::new();
    match mark.and_then(|mark| mark.frame) {
        Some(FrameArrow::Current) => {
            parts.push(tr!("gpui-debug-stopped-here", line = line).to_string())
        }
        Some(FrameArrow::Caller) => {
            parts.push(tr!("gpui-debug-caller-frame", line = line).to_string())
        }
        None => {}
    }
    match mark.and_then(|mark| mark.breakpoint.as_ref()) {
        Some(BreakpointState::Enabled) => {
            parts.push(tr!("gpui-debug-breakpoint", line = line).to_string())
        }
        Some(BreakpointState::Unverified {
            message: Some(message),
        }) => parts.push(
            tr!(
                "gpui-debug-breakpoint-unbound",
                line = line,
                message = message.as_ref()
            )
            .to_string(),
        ),
        Some(BreakpointState::Unverified { message: None }) => {
            parts.push(tr!("gpui-debug-breakpoint-unbound-no-message", line = line).to_string())
        }
        Some(BreakpointState::Disabled) => {
            parts.push(tr!("gpui-debug-breakpoint-disabled", line = line).to_string())
        }
        None => {}
    }
    if parts.is_empty() {
        return tr!("gpui-debug-add-breakpoint", line = line);
    }
    parts.join("\n").into()
}

#[cfg(test)]
#[path = "breakpoints_tests.rs"]
mod tests;
