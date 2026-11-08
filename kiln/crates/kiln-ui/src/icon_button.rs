// SPDX-License-Identifier: Apache-2.0

use gpui::{
    AnyView, App, ClickEvent, CursorStyle, ElementId, FocusHandle, InteractiveElement as _,
    IntoElement, ParentElement as _, RenderOnce, Role, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, div, prelude::FluentBuilder as _, px,
    svg,
};

use crate::{
    ActiveTheme as _, ClickHandler, Icon, UiTheme,
    controls::{Control, DISABLED_OPACITY, assert_named, key_focus, name_tooltip, with_key_focus},
    radius,
};

type TooltipBuilder = Box<dyn Fn(&mut Window, &mut App) -> AnyView>;

/// A 16 px icon in a 22 px square that runs a handler when clicked. A
/// disabled one is drawn at half its enabled look's opacity, has no hover,
/// takes no focus and runs nothing.
///
/// [`IconButton::name`] gives it its accessible name, which a screen
/// reader reads and its tooltip shows, with its key if
/// [`IconButton::shortcut`] gave one; a [`IconButton::tooltip`] of the
/// caller's own replaces that tooltip. Every icon button needs a name:
/// drawing one without is a debug assertion.
///
/// It's in the tab order, runs its handler on Enter or Space while
/// focused, and shows the focus ring while focused from the keyboard.
#[derive(IntoElement)]
pub struct IconButton {
    id: SharedString,
    icon: Icon,
    name: Option<SharedString>,
    shortcut: Option<SharedString>,
    focus: Option<FocusHandle>,
    disabled: bool,
    selected: bool,
    on_click: Option<ClickHandler>,
    tooltip: Option<TooltipBuilder>,
}

impl IconButton {
    /// `id` names the button's element and its `icon-button:<id>` debug
    /// selector. Its colours come from `cx.theme()` as it renders;
    /// `_theme` is ignored: deprecated, kept so callers that pass a
    /// [`UiTheme`] build.
    pub fn new(id: impl Into<SharedString>, icon: Icon, _theme: &UiTheme) -> Self {
        Self {
            id: id.into(),
            icon,
            name: None,
            shortcut: None,
            focus: None,
            disabled: false,
            selected: false,
            on_click: None,
            tooltip: None,
        }
    }

    /// Its accessible name: read by a screen reader, and shown in its
    /// tooltip unless the caller builds its own.
    pub fn name(mut self, name: impl Into<SharedString>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// The key that runs it, as a label such as `⌘W`: shown after its name
    /// in its tooltip and told to a screen reader.
    pub fn shortcut(mut self, shortcut: impl Into<SharedString>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    /// The focus handle it tracks, for a caller that moves focus to it;
    /// without one it keeps its own.
    pub fn focus_handle(mut self, focus: &FocusHandle) -> Self {
        self.focus = Some(focus.clone());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Draws the button on a highlighted background, as a toggle that is on.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }

    /// Shown on hover, built when it shows: see [`crate::Tooltip`].
    pub fn tooltip(mut self, build: impl Fn(&mut Window, &mut App) -> AnyView + 'static) -> Self {
        self.tooltip = Some(Box::new(build));
        self
    }
}

impl RenderOnce for IconButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        self.element(window, cx)
    }
}

impl IconButton {
    /// The element it renders as.
    pub(crate) fn element(self, window: &mut Window, cx: &mut App) -> Control {
        assert_named(self.name.as_ref(), "IconButton::new");
        let colors = cx.theme().colors;
        let color = colors.list.icon;
        let hover = colors.list.row_hover;
        let selected_bg = colors.list.row_selected;
        let id = self.id.clone();
        let element_id = ElementId::Name(self.id);
        let disabled = self.disabled;
        let focus = key_focus(Some(element_id.clone()), self.focus, disabled, window, cx);
        let tooltip = match (self.tooltip, &self.name) {
            (Some(tooltip), _) => Some(tooltip),
            (None, Some(name)) => {
                Some(Box::new(name_tooltip(name.clone(), self.shortcut.clone())) as TooltipBuilder)
            }
            (None, None) => None,
        };
        let button = div()
            .id(element_id)
            .debug_selector(|| format!("icon-button:{id}"))
            .role(Role::Button)
            .when_some(self.name, |button, name| button.aria_label(name))
            .when_some(self.shortcut, |button, key| button.aria_keyshortcuts(key))
            .when(self.selected, |button| button.bg(selected_bg))
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(22.))
            .rounded(radius::CONTROL)
            .when(disabled, |button| button.opacity(DISABLED_OPACITY))
            .child(
                div()
                    .flex_none()
                    .size(px(16.))
                    .debug_selector(|| format!("icon-button-icon:{id}:{}", self.icon.path()))
                    .child(
                        svg()
                            .path(self.icon.path())
                            .size(px(16.))
                            .flex_none()
                            .text_color(color),
                    ),
            )
            .when(!disabled, |button| {
                button
                    .cursor(CursorStyle::PointingHand)
                    .hover(move |style| style.bg(hover))
                    .when_some(self.on_click, |button, on_click| button.on_click(on_click))
            })
            .when_some(tooltip, |button, tooltip| {
                button.tooltip(move |window, cx| tooltip(window, cx))
            });
        Control::new(
            with_key_focus(button, focus, radius::CONTROL, px(0.), &colors),
            disabled,
        )
    }
}
