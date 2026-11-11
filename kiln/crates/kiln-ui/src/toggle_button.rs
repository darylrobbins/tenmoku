// SPDX-License-Identifier: Apache-2.0

use gpui::{
    App, ClickEvent, ElementId, Hsla, InteractiveElement as _, IntoElement, ParentElement as _,
    RenderOnce, Role, SharedString, StatefulInteractiveElement as _, Styled as _, Toggled, Window,
    div, prelude::FluentBuilder as _, px, svg,
};

use crate::{
    ActiveTheme as _, ClickHandler, Icon, Tokens, UiTheme,
    controls::{
        Control, DISABLED_OPACITY, TOGGLE_ON_ALPHA, assert_named, key_focus, name_tooltip,
        with_alpha, with_key_focus,
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
/// One showing an icon in place of a label ([`ToggleButton::icon_only`])
/// must be given a name: drawing one without is a debug assertion.
#[derive(IntoElement)]
pub struct ToggleButton {
    id: ElementId,
    label: SharedString,
    icon: Option<Icon>,
    /// An icon before the label, and its colour while on.
    glyph: Option<(Icon, Hsla)>,
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
            icon: None,
            glyph: None,
            name: None,
            shortcut: None,
            on: false,
            disabled: false,
            on_click: None,
        }
    }

    /// A toggle showing only `icon`, 16 px, in the toggle's text colour
    /// (muted while off). Give it a [`ToggleButton::name`].
    pub fn icon_only(id: impl Into<ElementId>, icon: Icon) -> Self {
        Self {
            id: id.into(),
            label: SharedString::default(),
            icon: Some(icon),
            glyph: None,
            name: None,
            shortcut: None,
            on: false,
            disabled: false,
            on_click: None,
        }
    }

    /// The icon an [`ToggleButton::icon_only`] toggle shows.
    pub fn icon(&self) -> Option<Icon> {
        self.icon
    }

    /// Its accessible name: [`ToggleButton::name`]'s, else its label.
    pub fn accessible_name(&self) -> Option<&SharedString> {
        self.name
            .as_ref()
            .or(Some(&self.label).filter(|label| !label.is_empty()))
    }

    /// Draws `icon`, 14 px, before the label: in `color` while on, such as a
    /// severity filter's glyph in its severity's colour, and in the toggle's
    /// muted text colour while off.
    pub fn glyph(mut self, icon: Icon, color: Hsla) -> Self {
        self.glyph = Some((icon, color));
        self
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
        if self.icon.is_some() {
            assert_named(
                self.name.as_ref(),
                kiln_l10n::verbatim!("ToggleButton::icon_only"),
            );
        }
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
            .when_some(self.icon, |button, icon| {
                button.child(
                    svg()
                        .path(icon.path())
                        .size(px(16.))
                        .flex_none()
                        .text_color(text),
                )
            })
            .when_some(self.glyph, |button, (icon, color)| {
                button.gap_1().child(
                    svg()
                        .path(icon.path())
                        .size(px(14.))
                        .flex_none()
                        .text_color(if self.on { color } else { text }),
                )
            })
            .when(self.icon.is_none(), |button| button.child(self.label))
            .when_some(self.on_click.filter(|_| !disabled), |button, on_click| {
                button.on_click(on_click)
            });
        Control::new(
            with_key_focus(button, focus, radius::CONTROL, px(0.), &colors),
            disabled,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_icon_toggle_is_named_by_its_name_a_labelled_one_by_its_label() {
        let theme = UiTheme::default();
        let icon = ToggleButton::icon_only("regex", Icon::Regex);
        assert_eq!(icon.icon(), Some(Icon::Regex));
        assert_eq!(icon.accessible_name(), None);
        let icon = icon.name("Use Regular Expression");
        assert_eq!(
            icon.accessible_name().map(SharedString::as_ref),
            Some("Use Regular Expression")
        );
        let labelled = ToggleButton::new("case", "Aa", &theme);
        assert_eq!(labelled.icon(), None);
        assert_eq!(
            labelled.accessible_name().map(SharedString::as_ref),
            Some("Aa")
        );
        let named = labelled.name("Match Case");
        assert_eq!(
            named.accessible_name().map(SharedString::as_ref),
            Some("Match Case")
        );
    }
}
