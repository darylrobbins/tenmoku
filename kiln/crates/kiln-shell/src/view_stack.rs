// SPDX-License-Identifier: Apache-2.0

//! A view stack: views sharing a column, top to bottom, each under a
//! header naming it, as a dock's panel holds a project tree over an
//! outline. A click on a header, or Enter or Space while it has focus,
//! collapses its view to the header alone, or expands it again.
//!
//! The expanded views share what the headers leave of the column's height
//! by their weights. Collapsing them all leaves the headers stacked at the
//! top, the rest of the column empty: the last expanded view collapses as
//! the others do, to its header, not out of the column.
//!
//! A divider sits on the border between two adjacent expanded views:
//! dragging it moves weight between them, down to [`MIN_VIEW_BODY_HEIGHT`]
//! of body each, and a double-click gives the pair equal weights. A
//! collapsed view has no divider either side.
//!
//! [`ViewStack::save`] keeps each view's collapse and weight by its
//! [`ViewId`], for a saved layout, and [`ViewStack::restore`] puts them
//! back.
//!
//! ```ignore
//! let stack = cx.new(|cx| {
//!     let mut stack = ViewStack::new();
//!     stack.add(StackedView::new(FOLDERS, "Folders", folders), cx);
//!     stack.add(StackedView::new(OUTLINE, "Outline", outline).weight(0.5), cx);
//!     stack
//! });
//! ```

use std::f32::consts::FRAC_PI_2;

use gpui::{
    AnyView, AppContext as _, Bounds, Context, CursorStyle, DragMoveEvent, EntityId, FocusHandle,
    InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent, ParentElement as _, Pixels,
    Point, Render, ScrollHandle, SharedString, StatefulInteractiveElement as _, Styled as _,
    Transformation, Window, div, prelude::FluentBuilder as _, px, radians, svg,
};
use kiln_command::Value;
use kiln_ui::{CHEVRON_WIDTH, Icon, ui_theme};

/// A view header's height, its bottom border included.
pub const VIEW_HEADER_HEIGHT: Pixels = px(22.);

/// The least body height dragging a divider leaves a view.
pub const MIN_VIEW_BODY_HEIGHT: Pixels = px(40.);

/// How tall a divider's handle is, over the top of the header below it.
const DIVIDER_HANDLE: Pixels = px(5.);

/// A view's identity among a stack's views: `"folders"`, `"outline"`, …
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ViewId(pub &'static str);

/// A view to add to a [`ViewStack`]: its id, its header's title and what
/// it shows, expanded with weight 1 unless built otherwise.
pub struct StackedView {
    id: ViewId,
    title: SharedString,
    body: AnyView,
    actions: Option<AnyView>,
    collapsed: bool,
    weight: f32,
}

impl StackedView {
    pub fn new(id: ViewId, title: impl Into<SharedString>, body: impl Into<AnyView>) -> Self {
        Self {
            id,
            title: title.into(),
            body: body.into(),
            actions: None,
            collapsed: false,
            weight: 1.,
        }
    }

    /// Builds it collapsed, or not.
    pub fn collapsed(mut self, collapsed: bool) -> Self {
        self.collapsed = collapsed;
        self
    }

    /// Builds it with `weight`, its share of the column beside the other
    /// expanded views': a weight that isn't positive and finite leaves 1.
    pub fn weight(mut self, weight: f32) -> Self {
        if valid_weight(weight) {
            self.weight = weight;
        }
        self
    }

    /// Draws `actions` at its header's right end, such as buttons that
    /// refresh or collapse what it shows. A click on them doesn't toggle
    /// the view.
    pub fn header_actions(mut self, actions: impl Into<AnyView>) -> Self {
        self.actions = Some(actions.into());
        self
    }
}

fn near_height(a: Pixels, b: Pixels) -> bool {
    (a - b).abs() < px(1.)
}

fn valid_weight(weight: f32) -> bool {
    weight.is_finite() && weight > 0.
}

/// The border above a view, as dragged.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Divider {
    /// The stack it's in, which another stack's drag may reach by GPUI's
    /// dispatch to every `on_drag_move`.
    stack: EntityId,
    /// The view below it.
    after: ViewId,
}

/// A divider's drag draws nothing at the pointer.
struct NoGhost;

impl Render for NoGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}

/// Where a divider's drag started: the pointer, and the two views' weights
/// and body heights either side of it.
struct Press {
    divider: Divider,
    at: Point<Pixels>,
    above: (ViewId, f32, Pixels),
    below: (ViewId, f32, Pixels),
    /// The two weights as the drag last left them, so a change from
    /// elsewhere ends the drag.
    applied: (f32, f32),
}

/// A view in a stack, with its header's focus and where it was drawn.
struct Entry {
    view: StackedView,
    focus: FocusHandle,
    /// Its header, then its body if expanded.
    drawn: ScrollHandle,
}

/// Views stacked in a column under collapsible headers, sharing its height
/// by weight.
pub struct ViewStack {
    views: Vec<Entry>,
    /// The column's views.
    drawn: ScrollHandle,
    press: Option<Press>,
}

impl Default for ViewStack {
    fn default() -> Self {
        Self::new()
    }
}

impl ViewStack {
    /// An empty stack. It draws in the active theme's colours
    /// ([`ui_theme`]), so it follows a theme change.
    pub fn new() -> Self {
        Self {
            views: Vec::new(),
            drawn: ScrollHandle::new(),
            press: None,
        }
    }

    /// Adds `view` below the others, or, if a view with its id is there,
    /// puts it in that one's place, keeping that one's collapse and weight
    /// and its header's focus.
    pub fn add(&mut self, view: StackedView, cx: &mut Context<Self>) {
        match self.entry_mut(view.id) {
            Some(entry) => {
                let (collapsed, weight) = (entry.view.collapsed, entry.view.weight);
                entry.view = StackedView {
                    collapsed,
                    weight,
                    ..view
                };
            }
            None => self.views.push(Entry {
                view,
                focus: cx.focus_handle().tab_stop(true),
                drawn: ScrollHandle::new(),
            }),
        }
        cx.notify();
    }

    /// Adds `view` at `ix` among the views, top to bottom (below them all
    /// past the last), or, if a view with its id is there, moves that one
    /// to `ix` among the others and puts `view` in its place, as
    /// [`ViewStack::add`] does.
    pub fn insert(&mut self, ix: usize, view: StackedView, cx: &mut Context<Self>) {
        let entry = match self.views.iter().position(|entry| entry.view.id == view.id) {
            Some(at) => {
                let mut entry = self.views.remove(at);
                let (collapsed, weight) = (entry.view.collapsed, entry.view.weight);
                entry.view = StackedView {
                    collapsed,
                    weight,
                    ..view
                };
                entry
            }
            None => Entry {
                view,
                focus: cx.focus_handle().tab_stop(true),
                drawn: ScrollHandle::new(),
            },
        };
        self.views.insert(ix.min(self.views.len()), entry);
        cx.notify();
    }

    /// The views' ids, top to bottom.
    pub fn view_ids(&self) -> Vec<ViewId> {
        self.views.iter().map(|entry| entry.view.id).collect()
    }

    /// Whether `view` is collapsed, if it's in the stack.
    pub fn is_collapsed(&self, view: ViewId) -> Option<bool> {
        Some(self.entry(view)?.view.collapsed)
    }

    /// Collapses or expands `view`. False, doing nothing, if it isn't in
    /// the stack.
    pub fn set_collapsed(&mut self, view: ViewId, collapsed: bool, cx: &mut Context<Self>) -> bool {
        let Some(entry) = self.entry_mut(view) else {
            return false;
        };
        entry.view.collapsed = collapsed;
        cx.notify();
        true
    }

    /// Collapses `view` if expanded, else expands it, as a click on its
    /// header does. False, doing nothing, if it isn't in the stack.
    pub fn toggle(&mut self, view: ViewId, cx: &mut Context<Self>) -> bool {
        let Some(entry) = self.entry_mut(view) else {
            return false;
        };
        entry.view.collapsed = !entry.view.collapsed;
        cx.notify();
        true
    }

    /// `view`'s weight, if it's in the stack.
    pub fn weight(&self, view: ViewId) -> Option<f32> {
        Some(self.entry(view)?.view.weight)
    }

    /// Sets `view`'s weight. False, doing nothing, if it isn't in the
    /// stack or the weight isn't positive and finite.
    pub fn set_weight(&mut self, view: ViewId, weight: f32, cx: &mut Context<Self>) -> bool {
        match self.entry_mut(view) {
            Some(entry) if valid_weight(weight) => {
                entry.view.weight = weight;
                cx.notify();
                true
            }
            _ => false,
        }
    }

    /// The focus of `view`'s header, which Enter or Space, pressed and
    /// released, toggles while it has it, if it's in the stack. A click on the header focuses it.
    pub fn header_focus(&self, view: ViewId) -> Option<FocusHandle> {
        Some(self.entry(view)?.focus.clone())
    }

    /// Where `view`'s header was last drawn.
    pub fn header_bounds(&self, view: ViewId) -> Option<Bounds<Pixels>> {
        self.entry(view)?.drawn.bounds_for_item(0)
    }

    /// Where `view`'s body was last drawn: none while collapsed.
    pub fn body_bounds(&self, view: ViewId) -> Option<Bounds<Pixels>> {
        let entry = self.entry(view)?;
        (entry.drawn.children_count() >= 2)
            .then(|| entry.drawn.bounds_for_item(1))
            .flatten()
    }

    /// Where the stack's column was last drawn.
    pub fn bounds(&self) -> Bounds<Pixels> {
        self.drawn.bounds()
    }

    /// Each view's collapse and weight by its id, for a saved layout:
    /// `{"views": {"<id>": {"collapsed": bool, "weight": number}}}`.
    pub fn save(&self) -> Value {
        let views = self.views.iter().map(|entry| {
            let view = &entry.view;
            let state = Value::from_iter([
                ("collapsed", Value::from(view.collapsed)),
                ("weight", Value::from(f64::from(view.weight))),
            ]);
            (view.id.0, state)
        });
        Value::from_iter([("views", Value::from_iter(views))])
    }

    /// Puts back what [`ViewStack::save`] saved for the views in the stack:
    /// a saved id not in it is ignored, and a view not saved, or a value
    /// missing or not a positive weight, is left as it is.
    pub fn restore(&mut self, saved: &Value, cx: &mut Context<Self>) {
        let Some(views) = saved.get("views").and_then(Value::as_object) else {
            return;
        };
        for entry in &mut self.views {
            let Some(state) = views.get(entry.view.id.0) else {
                continue;
            };
            if let Some(collapsed) = state.get("collapsed").and_then(Value::as_bool) {
                entry.view.collapsed = collapsed;
            }
            let weight = state.get("weight").and_then(Value::as_f64);
            if let Some(weight) = weight.map(|weight| weight as f32)
                && valid_weight(weight)
            {
                entry.view.weight = weight;
            }
        }
        cx.notify();
    }

    /// The view above `view`, if both are expanded: the pair a divider
    /// between them resizes.
    fn divided_above(&self, view: ViewId) -> Option<ViewId> {
        let ix = self.views.iter().position(|entry| entry.view.id == view)?;
        let (above, below) = (&self.views[ix.checked_sub(1)?].view, &self.views[ix].view);
        (!above.collapsed && !below.collapsed).then_some(above.id)
    }

    /// A press on the divider above `divider.after`: the start of a drag,
    /// or, pressed twice, equal weights for the pair, keeping their total.
    fn press_divider(&mut self, divider: Divider, event: &MouseDownEvent) -> Option<()> {
        let above = self.divided_above(divider.after)?;
        self.press = None;
        if event.click_count == 2 {
            // In f64, which two f32 weights can't overflow.
            let total = f64::from(self.weight(above)?) + f64::from(self.weight(divider.after)?);
            let even = (total / 2.) as f32;
            self.entry_mut(above)?.view.weight = even;
            self.entry_mut(divider.after)?.view.weight = even;
            return Some(());
        }
        let state = |stack: &Self, view| {
            let height = stack.body_bounds(view)?.size.height;
            Some((view, stack.weight(view)?, height))
        };
        self.press = Some(Press {
            divider,
            at: event.position,
            above: state(self, above)?,
            below: state(self, divider.after)?,
            applied: (self.weight(above)?, self.weight(divider.after)?),
        });
        Some(())
    }

    /// Moves the pressed divider with the pointer, now at `to`, as far as
    /// the views' minimum body heights let it. Only the pair's weights
    /// change, and their total doesn't.
    fn drag_divider(&mut self, divider: Divider, to: Point<Pixels>) -> Option<()> {
        let press = self
            .press
            .as_ref()
            .filter(|press| press.divider == divider)?;
        let ((above, above_weight, above_height), (below, below_weight, below_height)) =
            (press.above, press.below);
        let (at, applied) = (press.at, press.applied);
        // A drag ends when its pair stops being two expanded views, their
        // weights change from elsewhere, or the column's height does.
        let pair_height = above_height + below_height;
        let current = (self.body_bounds(above)).zip(self.body_bounds(below));
        let live = self.divided_above(below) == Some(above)
            && (self.weight(above), self.weight(below)) == (Some(applied.0), Some(applied.1))
            && current
                .is_some_and(|(a, b)| near_height(a.size.height + b.size.height, pair_height));
        if !live {
            self.press = None;
            return None;
        }
        let max = pair_height - MIN_VIEW_BODY_HEIGHT;
        if MIN_VIEW_BODY_HEIGHT > max {
            return None;
        }
        let height = (above_height + (to.y - at.y)).clamp(MIN_VIEW_BODY_HEIGHT, max);
        // The pair's total stays, so the other views' shares do. In f64,
        // which a split of two f32 weights can't overflow.
        let total = f64::from(above_weight) + f64::from(below_weight);
        let share = f64::from(height / pair_height);
        let (mut new_above, mut new_below) = (total * share, total * (1. - share));
        let largest = new_above.max(new_below);
        let max = f64::from(f32::MAX);
        if largest > max {
            // The split doesn't fit an f32 weight: scale every weight in
            // the stack down alike, keeping their relative shares; only a
            // weight scaled to nothing is held at the smallest positive one.
            let scale = max / largest;
            for entry in &mut self.views {
                let scaled = f64::from(entry.view.weight) * scale;
                entry.view.weight = (scaled as f32).max(f32::from_bits(1));
            }
            new_above *= scale;
            new_below *= scale;
        }
        let (new_above, new_below) = (new_above.min(max) as f32, new_below.min(max) as f32);
        if !valid_weight(new_above) || !valid_weight(new_below) {
            return None;
        }
        self.entry_mut(above)?.view.weight = new_above;
        self.entry_mut(below)?.view.weight = new_below;
        if let Some(press) = &mut self.press {
            press.applied = (new_above, new_below);
            // The rest of the drag measures from the scaled weights.
            press.above.1 = new_above;
            press.below.1 = new_below;
        }
        Some(())
    }

    fn entry(&self, view: ViewId) -> Option<&Entry> {
        self.views.iter().find(|entry| entry.view.id == view)
    }

    fn entry_mut(&mut self, view: ViewId) -> Option<&mut Entry> {
        self.views.iter_mut().find(|entry| entry.view.id == view)
    }

    /// A click on `view`'s header, which also focuses it, or Enter or
    /// Space, unmodified, released on it focused: GPUI's click.
    fn header_clicked(&mut self, view: ViewId, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(focus) = self.header_focus(view) {
            window.focus(&focus, cx);
        }
        self.toggle(view, cx);
    }

    /// `ix`'s header, then its body if expanded. Expanded, it grows from
    /// its header's height by its share of `weights`, the expanded views'
    /// total, so the bodies fill what the headers leave between them, by
    /// weight; collapsed, it's its header alone.
    fn section(&self, ix: usize, weights: f64, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let ui = ui_theme(cx);
        let entry = &self.views[ix];
        let view = &entry.view;
        let id = view.id;
        let chevron = svg()
            .path(Icon::ChevronRight.path())
            .size(CHEVRON_WIDTH)
            .flex_none()
            .when(!view.collapsed, |chevron| {
                chevron.with_transformation(Transformation::rotate(radians(FRAC_PI_2)))
            })
            .text_color(ui.chevron);
        let actions = view.actions.clone().map(|actions| {
            // A press on an action isn't one on the header.
            div()
                .flex_none()
                .flex()
                .items_center()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(actions)
        });
        let header = div()
            .id(("view-stack-header", ix))
            .track_focus(&entry.focus)
            .flex_none()
            .h(VIEW_HEADER_HEIGHT)
            .flex()
            .items_center()
            .gap_1()
            .px_1()
            .bg(ui.panel_background)
            .border_b_1()
            .border_color(ui.border)
            .text_color(ui.text)
            .focus_visible(|style| style.border_1().border_color(ui.row_focus_border))
            .on_click(cx.listener(move |stack, _, window, cx| stack.header_clicked(id, window, cx)))
            .child(chevron)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(view.title.clone()),
            )
            .children(actions);
        let divider = self.divided_above(id).map(|_| {
            let divider = Divider {
                stack: cx.entity_id(),
                after: id,
            };
            div()
                .id(("view-stack-divider", ix))
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .h(DIVIDER_HANDLE)
                .occlude()
                .cursor(CursorStyle::ResizeUpDown)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |stack, event: &MouseDownEvent, _, cx| {
                        stack.press_divider(divider, event);
                        cx.notify();
                    }),
                )
                .on_drag(divider, |_, _, _, cx| cx.new(|_| NoGhost))
        });
        let body = (!view.collapsed).then(|| {
            div()
                .flex_1()
                .min_h_0()
                .overflow_hidden()
                .flex()
                .child(view.body.clone())
        });
        div()
            .id(("view-stack-view", ix))
            .flex()
            .flex_col()
            .w_full()
            .relative()
            .min_h(VIEW_HEADER_HEIGHT)
            .map(|section| match view.collapsed {
                true => section.flex_none(),
                false => section
                    .flex_basis(VIEW_HEADER_HEIGHT)
                    // Flex grows by less than the free space while the
                    // growths total under 1, so they're shares of 1.
                    .flex_grow((f64::from(view.weight) / weights) as f32)
                    .flex_shrink(1.)
                    .overflow_hidden(),
            })
            .track_scroll(&entry.drawn)
            .child(header)
            .children(body)
            .children(divider)
    }
}

impl Render for ViewStack {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // In f64, which no number of f32 weights overflows.
        let weights = (self.views.iter())
            .filter(|entry| !entry.view.collapsed)
            .map(|entry| f64::from(entry.view.weight))
            .sum();
        let sections: Vec<_> = (0..self.views.len())
            .map(|ix| self.section(ix, weights, cx))
            .collect();
        div()
            .id("view-stack")
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .track_scroll(&self.drawn)
            .on_drag_move(cx.listener(|stack, event: &DragMoveEvent<Divider>, _, cx| {
                let divider = *event.drag(cx);
                if divider.stack == cx.entity_id() {
                    stack.drag_divider(divider, event.event.position);
                    cx.notify();
                }
            }))
            .children(sections)
    }
}
