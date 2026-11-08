// SPDX-License-Identifier: Apache-2.0

use gpui::{
    App, ClickEvent, ElementId, Hsla, InteractiveElement as _, IntoElement, ParentElement as _,
    RenderOnce, Role, SharedString, StatefulInteractiveElement as _, Styled as _, Toggled, Window,
    div, prelude::FluentBuilder as _, px,
};

use crate::{
    ActiveTheme as _, ClickHandler, Tokens, UiTheme,
    controls::{
        Control, DISABLED_OPACITY, TOGGLE_ON_ALPHA, key_focus, name_tooltip, with_alpha,
        with_key_focus,
    },
    radius,
};

/// A small button that is on or off, such as a search option: on, it sits
/// on a tint of the accent (the accent at alpha [`TOGGLE_ON_ALPHA`], not a
/// solid fill); off, its label is muted.
///
/// Its label is its accessible name unless [`ToggleButton::name`] gives a
/// fuller one, as a search option labelled `Aa` needs ("Match Case"); a
/// named toggle shows its name, and its key if it has one, in a tooltip.
#[derive(IntoElement)]
pub struct ToggleButton {
    id: ElementId,
    label: SharedString,
    name: Option<SharedString>,
    shortcut: Option<SharedString>,
    on: bool,
    disabled: bool,
    on_click: Option<ClickHandler>,
}

/// A toggle's (background, text) for its state; `None` draws no
/// background.
pub(crate) fn toggle_colors(on: bool, c: &Tokens<Hsla>) -> (Option<Hsla>, Hsla) {
    if on {
        (
            Some(with_alpha(c.accent.base, TOGGLE_ON_ALPHA)),
            c.text.primary,
        )
    } else {
        (None, c.text.muted)
    }
}

impl ToggleButton {
    /// Its colours come from `cx.theme()` as it renders. `_theme` is
    /// ignored: deprecated, kept so callers that pass a [`UiTheme`] build.
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>, _theme: &UiTheme) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            name: None,
            shortcut: None,
            on: false,
            disabled: false,
            on_click: None,
        }
    }

    pub fn on(mut self, on: bool) -> Self {
        self.on = on;
        self
    }

    /// Its accessible name, read by a screen reader and shown in its
    /// tooltip: the label unless this sets one.
    pub fn name(mut self, name: impl Into<SharedString>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// The key that toggles it, as a label such as `⌥⌘C`, shown in its
    /// tooltip after its name.
    pub fn shortcut(mut self, shortcut: impl Into<SharedString>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    /// A disabled toggle is drawn at half its enabled look's opacity, has
    /// no hover, takes no focus and runs nothing, and a screen reader hears
    /// that it's unavailable.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for ToggleButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        self.element(window, cx)
    }
}

impl ToggleButton {
    /// The element it renders as: in the tab order, toggled by Enter or
    /// Space while focused, with the focus ring while focused from the
    /// keyboard.
    pub(crate) fn element(self, window: &mut Window, cx: &mut App) -> Control {
        let colors = cx.theme().colors;
        let hover = colors.state.hover;
        let (background, text) = toggle_colors(self.on, &colors);
        let tooltip = (self.name.is_some() || self.shortcut.is_some()).then(|| {
            let name = self.name.clone().unwrap_or_else(|| self.label.clone());
            name_tooltip(name, self.shortcut.clone())
        });
        let disabled = self.disabled;
        let focus = key_focus(Some(self.id.clone()), None, disabled, window, cx);
        let button = div()
            .id(self.id)
            .role(Role::Button)
            .aria_label(self.name.unwrap_or_else(|| self.label.clone()))
            .aria_toggled(Toggled::from(self.on))
            .when_some(self.shortcut, |button, key| button.aria_keyshortcuts(key))
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .h(px(22.))
            .min_w(px(22.))
            .px_1()
            .rounded(radius::CONTROL)
            .text_color(text)
            .when_some(background, |button, background| button.bg(background))
            .when(disabled, |button| button.opacity(DISABLED_OPACITY))
            .when(!disabled && background.is_none(), |button| {
                button.hover(move |style| style.bg(hover))
            })
            .when_some(tooltip, |button, tooltip| button.tooltip(tooltip))
            .child(self.label)
            .when_some(self.on_click.filter(|_| !disabled), |button, on_click| {
                button.on_click(on_click)
            });
        Control::new(
            with_key_focus(button, focus, radius::CONTROL, px(0.), &colors),
            disabled,
        )
    }
}
