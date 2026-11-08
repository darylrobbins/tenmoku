// SPDX-License-Identifier: Apache-2.0

//! [`SegmentedControl`]: a row of segments, one of them chosen, for a
//! scope switch such as "You / This project".

use std::rc::Rc;

use gpui::{
    App, CursorStyle, ElementId, Hsla, InteractiveElement as _, IntoElement, KeyDownEvent,
    ParentElement as _, RenderOnce, Role, SharedString, StatefulInteractiveElement as _,
    Styled as _, Toggled, Window, div, prelude::FluentBuilder as _, px,
};

use crate::{
    ActiveTheme as _, ButtonSize, Tokens,
    controls::{Control, DISABLED_OPACITY, key_focus, with_key_focus},
    radius,
};

type ChangeHandler = Rc<dyn Fn(&usize, &mut Window, &mut App)>;

/// A row of labelled segments in one `border.control` outline, one of them
/// chosen: the chosen one on `state.selected_inactive` (the design's
/// pressed segment) in `text.primary`, the others muted, hovered on
/// `state.hover`. Clicking another segment reports its index to
/// `on_change`; the choice is the caller's to keep.
///
/// Each segment's label is its accessible name, and [`SegmentedControl`]'s
/// own name, from [`SegmentedControl::new`], names the group. A screen
/// reader hears the segments as radio buttons, the chosen one checked.
///
/// The control is one stop in the tab order: while it has focus, `left`
/// and `right` (or `up` and `down`) choose the segment before or after the
/// chosen one, and `home` and `end` the first and last, as the platform's
/// segmented controls do; it shows the focus ring while focused from the
/// keyboard. A disabled one takes no focus and reports nothing.
///
/// ```ignore
/// SegmentedControl::new("scope", "Settings scope")
///     .segments(["You", "This project"])
///     .selected(self.scope as usize)
///     .on_change(cx.listener(|this, ix: &usize, _, cx| this.set_scope(*ix, cx)))
/// ```
#[derive(IntoElement)]
pub struct SegmentedControl {
    id: ElementId,
    name: SharedString,
    segments: Vec<SharedString>,
    selected: usize,
    size: ButtonSize,
    disabled: bool,
    on_change: Option<ChangeHandler>,
}

/// A segment's (background, text) for whether it's chosen; `None` draws
/// no background.
pub(crate) fn segment_colors(chosen: bool, c: &Tokens<Hsla>) -> (Option<Hsla>, Hsla) {
    if chosen {
        (Some(c.state.selected_inactive), c.text.primary)
    } else {
        (None, c.text.muted)
    }
}

impl SegmentedControl {
    /// `name` names the group for a screen reader.
    pub fn new(id: impl Into<ElementId>, name: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            segments: Vec::new(),
            selected: 0,
            size: ButtonSize::Medium,
            disabled: false,
            on_change: None,
        }
    }

    /// Adds a segment labelled `label`.
    pub fn segment(mut self, label: impl Into<SharedString>) -> Self {
        self.segments.push(label.into());
        self
    }

    /// Adds a segment for each label.
    pub fn segments(mut self, labels: impl IntoIterator<Item = impl Into<SharedString>>) -> Self {
        self.segments.extend(labels.into_iter().map(Into::into));
        self
    }

    /// The chosen segment's index.
    pub fn selected(mut self, ix: usize) -> Self {
        self.selected = ix;
        self
    }

    /// Its height, as a button's: 26 px, or 22 px small.
    pub fn size(mut self, size: ButtonSize) -> Self {
        self.size = size;
        self
    }

    pub fn small(self) -> Self {
        self.size(ButtonSize::Small)
    }

    /// A disabled control is drawn at half its enabled look's opacity and
    /// reports nothing.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Called with the index of a segment clicked while not chosen.
    pub fn on_change(mut self, handler: impl Fn(&usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

/// The segment a key asks for, from `selected` of `count`: `None` for a
/// key that isn't the control's.
pub(crate) fn segment_for_key(key: &str, selected: usize, count: usize) -> Option<usize> {
    let last = count.checked_sub(1)?;
    let selected = selected.min(last);
    match key {
        "left" | "up" => Some(selected.saturating_sub(1)),
        "right" | "down" => Some((selected + 1).min(last)),
        "home" => Some(0),
        "end" => Some(last),
        _ => None,
    }
}

impl RenderOnce for SegmentedControl {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        self.element(window, cx)
    }
}

impl SegmentedControl {
    /// The element it renders as.
    pub(crate) fn element(self, window: &mut Window, cx: &mut App) -> Control {
        self.build(window, cx, None)
    }

    /// The element it renders as, and each segment's accessibility node.
    #[cfg(test)]
    pub(crate) fn element_with_segments(
        self,
        window: &mut Window,
        cx: &mut App,
    ) -> (Control, Vec<gpui::accesskit::Node>) {
        let mut nodes = Vec::new();
        let control = self.build(window, cx, Some(&mut nodes));
        (control, nodes)
    }

    /// The element, noting each segment's accessibility node in `nodes`.
    fn build(
        self,
        window: &mut Window,
        cx: &mut App,
        #[cfg_attr(not(test), allow(unused_variables))] nodes: Option<
            &mut Vec<gpui::accesskit::Node>,
        >,
    ) -> Control {
        let c = cx.theme().colors;
        let hover = c.state.hover;
        let disabled = self.disabled;
        let (selected, count) = (self.selected, self.segments.len());
        let on_change = self.on_change.filter(|_| !disabled);
        let segments = self.segments.into_iter().enumerate().map(|(ix, label)| {
            let chosen = ix == selected;
            let (background, text) = segment_colors(chosen, &c);
            let selector = format!("segment:{label}");
            let segment = div()
                .id(ix)
                .debug_selector(move || selector)
                .role(Role::RadioButton)
                .aria_label(label.clone())
                .aria_toggled(Toggled::from(chosen))
                .aria_position_in_set(ix + 1)
                .aria_size_of_set(count)
                .flex()
                .items_center()
                .justify_center()
                .h_full()
                .px_3()
                // Inside the outline's 1 px border and 1 px inset.
                .rounded(radius::CONTROL - px(1.))
                .text_color(text)
                .when_some(background, |segment, background| segment.bg(background))
                .when(!chosen && !disabled, |segment| {
                    segment
                        .cursor(CursorStyle::PointingHand)
                        .hover(move |style| style.bg(hover))
                })
                .when_some(
                    on_change.clone().filter(|_| !chosen),
                    |segment, on_change| {
                        segment.on_click(move |_, window, cx| on_change(&ix, window, cx))
                    },
                )
                .child(label);
            Control::new(segment, disabled)
        });
        let segments: Vec<_> = segments.collect();
        #[cfg(test)]
        if let Some(nodes) = nodes {
            nodes.extend(segments.iter().filter_map(Control::a11y_node));
        }
        let id = self.id.clone();
        let focus = key_focus(Some(self.id.clone()), None, disabled, window, cx);
        let control = div()
            .id(self.id)
            .debug_selector(move || format!("segmented:{id}"))
            .role(Role::RadioGroup)
            .aria_label(self.name)
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(1.))
            .h(self.size.height())
            .p(px(1.))
            .rounded(radius::CONTROL)
            .border_1()
            .border_color(c.border.control)
            .text_size(self.size.text())
            .when(disabled, |control| control.opacity(DISABLED_OPACITY))
            .when_some(on_change, |control, on_change| {
                control.on_key_down(move |event: &KeyDownEvent, window, cx| {
                    let keystroke = &event.keystroke;
                    if keystroke.modifiers.modified() {
                        return;
                    }
                    let Some(ix) = segment_for_key(&keystroke.key, selected, count) else {
                        return;
                    };
                    cx.stop_propagation();
                    if ix != selected {
                        on_change(&ix, window, cx);
                    }
                })
            })
            .children(segments);
        Control::new(
            with_key_focus(control, focus, radius::CONTROL, px(1.), &c),
            disabled,
        )
    }
}
