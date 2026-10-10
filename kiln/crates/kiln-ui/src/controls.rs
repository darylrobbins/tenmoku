// SPDX-License-Identifier: LicenseRef-Proprietary

//! Basic controls: [`Button`], [`Checkbox`] and [`Switch`]. Each is a GPUI
//! element built like a `div`: it takes GPUI's styling and interaction
//! methods (`.w_full()`, `.id()`, `.on_mouse_down(..)`, `.when(..)`), with
//! the caller's styles applied over the control's own, and its handlers
//! take GPUI's arguments, so `cx.listener(..)` fits them. Values are the
//! caller's: a checkbox shows `checked` and reports a click with
//! `on_change`, leaving the caller to store it. Colours are the active
//! theme's tokens (`cx.theme()`), read when the control is drawn.

use std::rc::Rc;

use gpui::{
    Action, AnyElement, App, ClickEvent, CursorStyle, Div, ElementId, Hsla, InteractiveElement,
    Interactivity, IntoElement, ParentElement as _, Pixels, Refineable as _, RenderOnce,
    SharedString, Stateful, StatefulInteractiveElement, StyleRefinement, Styled, Window, div,
    prelude::FluentBuilder as _, px, svg,
};

use kiln_command::Source;
use kiln_platform::action_dispatch::dispatch_action;

use crate::{ActiveTheme as _, Icon, Tokens};

type ChangeHandler<T> = Rc<dyn Fn(&T, &mut Window, &mut App)>;
type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// Applies `own` (the control's look) to `base`, then the styles the
/// caller set on it, so the caller's win.
pub(crate) fn styled(
    mut base: Stateful<Div>,
    own: impl FnOnce(Stateful<Div>) -> Stateful<Div>,
) -> Stateful<Div> {
    let caller = std::mem::take(base.style());
    let mut base = own(base);
    base.style().refine(&caller);
    base
}

/// A button's look.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonVariant {
    /// The main action: filled with the accent colour.
    Primary,
    /// Outlined, on the panel colour.
    #[default]
    Secondary,
    /// No background until hovered: for toolbars.
    Ghost,
    /// A destructive action: filled with the error colour.
    Danger,
}

/// A button's size: its height and its text's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonSize {
    Small,
    #[default]
    Medium,
    Large,
}

impl ButtonSize {
    fn height(self) -> Pixels {
        match self {
            ButtonSize::Small => px(22.),
            ButtonSize::Medium => px(26.),
            ButtonSize::Large => px(32.),
        }
    }

    fn text(self) -> Pixels {
        match self {
            ButtonSize::Small => px(11.),
            ButtonSize::Medium => px(12.),
            ButtonSize::Large => px(13.),
        }
    }
}

/// A button with a label, an icon, or both.
///
/// ```ignore
/// Button::new("save", "Save")
///     .primary()
///     .on_click(cx.listener(|this, _, window, cx| this.save(window, cx)))
///     .w_full()
/// ```
#[derive(IntoElement)]
pub struct Button {
    base: Stateful<Div>,
    label: Option<SharedString>,
    icon: Option<Icon>,
    variant: ButtonVariant,
    size: ButtonSize,
    disabled: bool,
    on_click: Option<ClickHandler>,
}

impl Button {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self::bare(id).label(label)
    }

    /// A button showing only `icon`.
    pub fn icon_only(id: impl Into<ElementId>, icon: Icon) -> Self {
        Self::bare(id).icon(icon)
    }

    fn bare(id: impl Into<ElementId>) -> Self {
        Self {
            base: div().id(id),
            label: None,
            icon: None,
            variant: ButtonVariant::default(),
            size: ButtonSize::default(),
            disabled: false,
            on_click: None,
        }
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// An icon before the label.
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn primary(self) -> Self {
        self.variant(ButtonVariant::Primary)
    }

    pub fn ghost(self) -> Self {
        self.variant(ButtonVariant::Ghost)
    }

    pub fn danger(self) -> Self {
        self.variant(ButtonVariant::Danger)
    }

    pub fn size(mut self, size: ButtonSize) -> Self {
        self.size = size;
        self
    }

    pub fn small(self) -> Self {
        self.size(ButtonSize::Small)
    }

    pub fn large(self) -> Self {
        self.size(ButtonSize::Large)
    }

    /// A disabled button is muted, has no hover and runs nothing.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }

    /// Clicking dispatches `action` from the focused element, as a key
    /// bound to it would: a typed command goes as an invocation from the
    /// mouse, so observers hear of it (see
    /// [`kiln_platform::action_dispatch`]). Replaces any `on_click`.
    pub fn command(self, action: impl Action) -> Self {
        self.on_click(move |_, window, cx| {
            dispatch_action(&action, Source::Mouse, window, cx);
        })
    }
}

impl ButtonVariant {
    /// (background, text, border, hover background) for the variant.
    pub(crate) fn colors(self, c: &Tokens<Hsla>) -> (Hsla, Hsla, Hsla, Hsla) {
        match self {
            ButtonVariant::Primary => (
                c.accent.base,
                c.text.on_accent,
                c.accent.base,
                c.accent.hover,
            ),
            ButtonVariant::Secondary => (
                c.surface.panel,
                c.text.primary,
                c.border.default,
                c.state.hover,
            ),
            ButtonVariant::Ghost => (
                gpui::transparent_black(),
                c.text.primary,
                gpui::transparent_black(),
                c.state.hover,
            ),
            ButtonVariant::Danger => (
                c.status.error,
                c.text.on_accent,
                c.status.error,
                c.status.error.opacity(0.85),
            ),
        }
    }
}

impl Styled for Button {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl InteractiveElement for Button {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl StatefulInteractiveElement for Button {}

impl RenderOnce for Button {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let c = cx.theme().colors;
        let (bg, fg, border, hover) = self.variant.colors(&c);
        let (fg, size, disabled) = (
            if self.disabled { c.text.disabled } else { fg },
            self.size,
            self.disabled,
        );
        let icon_only = self.label.is_none();
        styled(self.base, |base| {
            base.flex()
                .flex_none()
                .items_center()
                .justify_center()
                .gap_1()
                .h(size.height())
                .when(icon_only, |base| base.w(size.height()))
                .when(!icon_only, |base| base.px_3())
                .rounded_md()
                .border_1()
                .border_color(border)
                .bg(bg)
                .text_color(fg)
                .text_size(size.text())
                .when(disabled, |base| base.opacity(0.6))
                .when(!disabled, |base| {
                    base.cursor(CursorStyle::PointingHand)
                        .hover(move |style| style.bg(hover))
                })
        })
        .children(
            self.icon
                .map(|icon| svg().path(icon.path()).size(px(14.)).text_color(fg)),
        )
        .children(self.label)
        .when_some(self.on_click.filter(|_| !disabled), |base, on_click| {
            base.on_click(move |event, window, cx| on_click(event, window, cx))
        })
    }
}

/// A box that's checked or not, with an optional label; clicking either
/// reports the new value to `on_change`.
#[derive(IntoElement)]
pub struct Checkbox {
    base: Stateful<Div>,
    checked: bool,
    label: Option<SharedString>,
    disabled: bool,
    on_change: Option<ChangeHandler<bool>>,
}

impl Checkbox {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            base: div().id(id),
            checked: false,
            label: None,
            disabled: false,
            on_change: None,
        }
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Called with the value a click asks for: the opposite of `checked`.
    pub fn on_change(mut self, handler: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

impl Styled for Checkbox {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl InteractiveElement for Checkbox {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl StatefulInteractiveElement for Checkbox {}

impl RenderOnce for Checkbox {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let c = cx.theme().colors;
        let (checked, disabled) = (self.checked, self.disabled);
        let box_ = div()
            .flex()
            .items_center()
            .justify_center()
            .size(px(14.))
            .rounded_sm()
            .border_1()
            .border_color(if checked {
                c.accent.base
            } else {
                c.border.default
            })
            .bg(if checked {
                c.accent.base
            } else {
                c.surface.panel
            })
            .when(checked, |box_| {
                box_.child(
                    svg()
                        .path(Icon::Check.path())
                        .size(px(10.))
                        .text_color(c.text.on_accent),
                )
            });
        toggle_row(
            self.base,
            box_,
            self.label,
            checked,
            disabled,
            self.on_change,
            &c,
        )
    }
}

/// A sliding switch, on or off, with an optional label; clicking either
/// reports the new value to `on_change`.
#[derive(IntoElement)]
pub struct Switch {
    base: Stateful<Div>,
    on: bool,
    label: Option<SharedString>,
    disabled: bool,
    on_change: Option<ChangeHandler<bool>>,
}

impl Switch {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            base: div().id(id),
            on: false,
            label: None,
            disabled: false,
            on_change: None,
        }
    }

    pub fn on(mut self, on: bool) -> Self {
        self.on = on;
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Called with the value a click asks for: the opposite of `on`.
    pub fn on_change(mut self, handler: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

impl Styled for Switch {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl InteractiveElement for Switch {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl StatefulInteractiveElement for Switch {}

impl RenderOnce for Switch {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let c = cx.theme().colors;
        let (on, disabled) = (self.on, self.disabled);
        let track = div()
            .flex()
            .items_center()
            .w(px(28.))
            .h(px(16.))
            .p(px(2.))
            .rounded_full()
            .bg(if on { c.accent.base } else { c.border.default })
            .when(on, |track| track.justify_end())
            .child(div().size(px(12.)).rounded_full().bg(c.text.on_accent));
        toggle_row(
            self.base,
            track,
            self.label,
            on,
            disabled,
            self.on_change,
            &c,
        )
    }
}

/// A checkbox's or switch's row: the control, then its label, the whole
/// row clickable.
fn toggle_row(
    base: Stateful<Div>,
    control: impl IntoElement,
    label: Option<SharedString>,
    value: bool,
    disabled: bool,
    on_change: Option<ChangeHandler<bool>>,
    c: &Tokens<Hsla>,
) -> AnyElement {
    let text = if disabled {
        c.text.disabled
    } else {
        c.text.primary
    };
    styled(base, |base| {
        base.flex()
            .flex_none()
            .items_center()
            .gap_2()
            .text_color(text)
            .when(disabled, |base| base.opacity(0.6))
            .when(!disabled, |base| base.cursor(CursorStyle::PointingHand))
    })
    .child(control)
    .children(label)
    .when_some(on_change.filter(|_| !disabled), |base, on_change| {
        base.on_click(move |_, window, cx| on_change(&!value, window, cx))
    })
    .into_any_element()
}
