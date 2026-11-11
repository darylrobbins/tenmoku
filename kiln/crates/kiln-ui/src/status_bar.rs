// SPDX-License-Identifier: Apache-2.0

use std::sync::Arc;

use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, Context, ElementId, Entity, FontWeight, Hsla,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, RenderOnce, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, div, prelude::FluentBuilder as _, px,
    svg,
};

use crate::{
    ActiveTheme as _, ClickHandler, Icon, Tokens, Tooltip, UI_LABEL, UiTheme, height, radius,
    space, ui_theme,
};

/// What a status bar entry's icon says, which picks its colour: the
/// matching `status.*` token, or the text's for `Default`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum StatusTone {
    #[default]
    Default,
    Error,
    Warning,
    Info,
    Success,
}

/// A status bar entry drawn as a pill, as a vim mode is: a rounded 16 px
/// fill in `background` inside the entry's padding, its label and icon in
/// `foreground`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StatusFill {
    pub background: Hsla,
    pub foreground: Hsla,
}

/// The colours a status bar entry draws with ([`status_entry_colors`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StatusEntryColors {
    /// `None` leaves the bar's own background.
    pub background: Option<Hsla>,
    pub text: Hsla,
    pub icon: Hsla,
}

/// An entry's colours: a fill's, background and foreground, if it has
/// one; otherwise no background, `text.primary` text and an icon in the
/// tone's `status.*` colour, `text.primary` for `Default`.
pub fn status_entry_colors(
    tone: StatusTone,
    fill: Option<StatusFill>,
    colors: &Tokens<Hsla>,
) -> StatusEntryColors {
    if let Some(fill) = fill {
        return StatusEntryColors {
            background: Some(fill.background),
            text: fill.foreground,
            icon: fill.foreground,
        };
    }
    let icon = match tone {
        StatusTone::Default => colors.text.primary,
        StatusTone::Error => colors.status.error,
        StatusTone::Warning => colors.status.warning,
        StatusTone::Info => colors.status.info,
        StatusTone::Success => colors.status.success,
    };
    StatusEntryColors {
        background: None,
        text: colors.text.primary,
        icon,
    }
}

/// The side length of an entry's icon.
const ICON_SIZE: f32 = 14.;

/// A filled entry's pill: the bar's height less a 4 px inset above and
/// below.
const PILL_HEIGHT: gpui::Pixels = px(16.);

/// The status bar: a full-width row along the window's bottom edge,
/// [`height::STATUS_BAR`] tall on `surface.chrome` with a 1 px line on top,
/// its text the UI label role (12/16, 500), its left children from the left
/// edge and its right children against the right edge.
#[derive(IntoElement)]
pub struct StatusBarFrame {
    left: Vec<AnyElement>,
    right: Vec<AnyElement>,
}

impl StatusBarFrame {
    /// Its colours come from `cx.theme()` as it renders. `_theme` is
    /// ignored: deprecated, kept so callers that pass a [`UiTheme`] build.
    pub fn new(_theme: &UiTheme) -> Self {
        Self {
            left: Vec::new(),
            right: Vec::new(),
        }
    }

    pub fn left(mut self, children: impl IntoIterator<Item = impl IntoElement>) -> Self {
        self.left
            .extend(children.into_iter().map(IntoElement::into_any_element));
        self
    }

    pub fn right(mut self, children: impl IntoIterator<Item = impl IntoElement>) -> Self {
        self.right
            .extend(children.into_iter().map(IntoElement::into_any_element));
        self
    }
}

impl RenderOnce for StatusBarFrame {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = &cx.theme().colors;
        let side = || div().flex().items_center().h_full();
        div()
            .debug_selector(|| "status-bar".into())
            .flex()
            .flex_none()
            .items_center()
            .justify_between()
            .w_full()
            .h(height::STATUS_BAR)
            .overflow_hidden()
            .bg(colors.surface.chrome)
            .border_t_1()
            .border_color(colors.border.default)
            .text_color(colors.text.primary)
            .text_size(UI_LABEL.size)
            .line_height(UI_LABEL.line_height)
            .font_weight(FontWeight(f32::from(UI_LABEL.weight)))
            .child(side().min_w_0().overflow_hidden().children(self.left))
            .child(side().flex_none().children(self.right))
    }
}

/// One status bar item: its label after its icon if it has one, 8 px of
/// padding either side; filled, the two in a pill inside that padding
/// ([`StatusEntry::fill`]). With a click handler, it's highlighted on hover
/// under a pointer cursor, unless filled. With a tooltip, hovering shows
/// it.
#[derive(IntoElement)]
pub struct StatusEntry {
    id: SharedString,
    label: SharedString,
    icon: Option<Icon>,
    tone: StatusTone,
    fill: Option<StatusFill>,
    tooltip: Option<SharedString>,
    on_click: Option<ClickHandler>,
}

impl StatusEntry {
    /// `id` names the element; in tests its bounds are those of the debug
    /// selector `status-item:{id}`. Its colours come from `cx.theme()` as
    /// it renders; `_theme` is ignored: deprecated, kept so callers that
    /// pass a [`UiTheme`] build.
    pub fn new(
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        _theme: &UiTheme,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon: None,
            tone: StatusTone::Default,
            fill: None,
            tooltip: None,
            on_click: None,
        }
    }

    /// Draws `icon` before the label, in the tone's colour or the fill's
    /// foreground. In tests its bounds are those of the debug selector
    /// `status-icon:{id}`.
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Colours the icon: see [`status_entry_colors`].
    pub fn tone(mut self, tone: StatusTone) -> Self {
        self.tone = tone;
        self
    }

    /// Draws the entry's icon and label in a pill of `fill`'s background,
    /// 16 px tall with 6 px inside either end and 4 px corners,
    /// the label at semibold weight. In tests its bounds are those of the
    /// debug selector `status-pill:{id}`.
    pub fn fill(mut self, fill: StatusFill) -> Self {
        self.fill = Some(fill);
        self
    }

    /// Shows `text` in a tooltip on hover. In tests, while shown, its
    /// bounds are those of the debug selector `status-tooltip:{id}:{text}`.
    pub fn tooltip(mut self, text: impl Into<SharedString>) -> Self {
        self.tooltip = Some(text.into());
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

impl RenderOnce for StatusEntry {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let hover = theme.colors.list.row_hover;
        let colors = status_entry_colors(self.tone, self.fill, &theme.colors);
        let id = self.id;
        // One entity per entry, updated to the current text, so a pending
        // show or a visible tooltip GPUI holds across frames shows it.
        let tooltip = self.tooltip.map(|text| {
            let key =
                ElementId::NamedChild(Arc::new(ElementId::Name(id.clone())), "tooltip".into());
            let tooltip =
                window.use_keyed_state(key, cx, |_, cx| StatusTooltip::new(&id, &text, cx));
            tooltip.update(cx, |tooltip, cx| tooltip.set_text(&text, cx));
            tooltip
        });
        let icon = self.icon.map(|icon| {
            let id = id.clone();
            svg()
                .path(icon.path())
                .size(px(ICON_SIZE))
                .flex_none()
                .text_color(colors.icon)
                .debug_selector(move || format!("status-icon:{id}"))
        });
        // The icon and label, in a pill if filled.
        let content = div()
            .flex()
            .flex_none()
            .items_center()
            .when(icon.is_some(), |content| content.gap_1())
            .children(icon)
            .child(self.label)
            .when_some(colors.background, |pill, background| {
                let id = id.clone();
                pill.debug_selector(move || format!("status-pill:{id}"))
                    .h(PILL_HEIGHT)
                    .px(space::S6)
                    .rounded(radius::CONTROL)
                    .bg(background)
                    .font_weight(FontWeight::SEMIBOLD)
            });
        let entry = div()
            .id(ElementId::Name(id.clone()))
            .debug_selector(|| format!("status-item:{id}"))
            .flex()
            .flex_none()
            .items_center()
            .h_full()
            .px(space::S8)
            .whitespace_nowrap()
            .text_color(colors.text)
            .child(content);
        entry
            .when_some(self.on_click, |entry, on_click| {
                entry
                    .cursor_pointer()
                    // A fill keeps its colour on hover.
                    .when(colors.background.is_none(), |entry| {
                        entry.hover(move |style| style.bg(hover))
                    })
                    .on_click(on_click)
            })
            .when_some(tooltip, |entry, tooltip| {
                entry.tooltip(move |_, _| tooltip.clone().into())
            })
    }
}

/// A [`Tooltip`] under a debug selector naming its entry and text, so
/// tests can see it shown.
struct StatusTooltip {
    id: SharedString,
    text: SharedString,
    tooltip: Entity<Tooltip>,
}

impl StatusTooltip {
    fn new(id: &SharedString, text: &SharedString, cx: &mut Context<Self>) -> Self {
        Self {
            id: id.clone(),
            text: text.clone(),
            tooltip: Self::tooltip(text, cx),
        }
    }

    fn tooltip(text: &SharedString, cx: &mut Context<Self>) -> Entity<Tooltip> {
        let text = text.clone();
        cx.new(|cx| Tooltip::new(text, None::<SharedString>, &ui_theme(cx)))
    }

    /// Shows `text`; an unchanged text notifies nothing.
    fn set_text(&mut self, text: &SharedString, cx: &mut Context<Self>) {
        if self.text != *text {
            self.text = text.clone();
            self.tooltip = Self::tooltip(text, cx);
            cx.notify();
        }
    }
}

impl Render for StatusTooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let selector = format!("status-tooltip:{}:{}", self.id, self.text);
        div()
            .debug_selector(move || selector)
            .child(self.tooltip.clone())
    }
}
