// SPDX-License-Identifier: Apache-2.0

use gpui::{
    App, ClickEvent, ElementId, Hsla, InteractiveElement as _, IntoElement, ParentElement as _,
    RenderOnce, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _, px, svg,
};

use crate::{
    ActiveTheme as _, ClickHandler, Icon, Role, States, Tokens, UiTheme,
    controls::{
        Control, DISABLED_OPACITY, TOGGLE_ON_ALPHA, assert_named, key_focus, name_tooltip,
        with_alpha, with_key_focus,
    },
    node_with, radius,
};

/// A small button that is on or off, such as a search option: on, it sits
/// on a tint of the accent (the accent at alpha [`TOGGLE_ON_ALPHA`], not a
/// solid fill); off, its label is muted.
///
/// Its constructors require its accessible name, which a screen reader
/// reads, apart from the caption it shows: a search option captioned `Aa`
/// is named "Match Case". One whose name isn't its caption shows the name,
/// and its key if it has one, in a tooltip. Drawing one whose name is
/// blank is a debug assertion.
#[derive(IntoElement)]
pub struct ToggleButton {
    id: ElementId,
    label: SharedString,
    icon: Option<Icon>,
    /// An icon before the label, and its colour while on.
    glyph: Option<(Icon, Hsla)>,
    name: SharedString,
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
    /// A toggle showing `label`, named `name` for a screen reader: the same
    /// text, unless the label is a symbol such as `Aa`. Its colours come
    /// from `cx.theme()` as it renders. `_theme` is ignored: deprecated,
    /// kept so callers that pass a [`UiTheme`] build.
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        name: impl Into<SharedString>,
        _theme: &UiTheme,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon: None,
            glyph: None,
            name: name.into(),
            shortcut: None,
            on: false,
            disabled: false,
            on_click: None,
        }
    }

    /// A toggle showing only `icon`, 16 px, in the toggle's text colour
    /// (muted while off), named `name` for a screen reader and its tooltip.
    pub fn icon_only(id: impl Into<ElementId>, icon: Icon, name: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: SharedString::default(),
            icon: Some(icon),
            glyph: None,
            name: name.into(),
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

    /// Its accessible name, which a screen reader reads.
    pub fn accessible_name(&self) -> &SharedString {
        &self.name
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
        assert_named(Some(&self.name), kiln_l10n::verbatim!("ToggleButton::new"));
        let colors = cx.theme().colors;
        let hover = colors.state.hover;
        let (background, text) = toggle_colors(self.on, &colors);
        // A caption that says its name needs no tooltip to repeat it.
        let tooltip = (self.name != self.label || self.shortcut.is_some())
            .then(|| name_tooltip(self.name.clone(), self.shortcut.clone()));
        let disabled = self.disabled;
        let focus = key_focus(Some(self.id.clone()), None, disabled, window, cx);
        let name = self.name;
        let button = div()
            .id(self.id)
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
        let button = with_key_focus(button, focus, radius::CONTROL, px(0.), &colors);
        let states = States {
            toggled: Some(self.on),
            ..States::default()
        };
        Control::new(
            node_with(button, window, cx, Role::Button, name, states),
            disabled,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_toggle_is_named_by_its_name_not_its_caption() {
        let theme = UiTheme::default();
        let icon = ToggleButton::icon_only("regex", Icon::Regex, "Use Regular Expression");
        assert_eq!(icon.icon(), Some(Icon::Regex));
        assert_eq!(icon.accessible_name().as_ref(), "Use Regular Expression");
        let captioned = ToggleButton::new("case", "Aa", "Match Case", &theme);
        assert_eq!(captioned.icon(), None);
        assert_eq!(captioned.accessible_name().as_ref(), "Match Case");
    }
}
