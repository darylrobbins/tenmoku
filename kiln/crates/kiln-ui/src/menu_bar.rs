// SPDX-License-Identifier: LicenseRef-Proprietary

use std::rc::Rc;

use gpui::{
    AnyElement, App, InteractiveElement as _, IntoElement, MouseButton, ParentElement as _, Pixels,
    RenderOnce, SharedString, StatefulInteractiveElement as _, Styled as _, Window, deferred, div,
    prelude::FluentBuilder as _, px,
};

use crate::{ActiveTheme as _, UiTheme};

/// The menu bar's height.
pub const MENU_BAR_HEIGHT: Pixels = px(28.);

/// The narrowest a menu's panel is.
const PANEL_MIN_WIDTH: Pixels = px(220.);

type IndexHandler = Rc<dyn Fn(usize, &mut Window, &mut App)>;

/// A row of menu names, for a menu bar drawn in the window. The open
/// menu's name is highlighted, with its panel below it, drawn above the
/// rest of the window.
#[derive(IntoElement)]
pub struct MenuStrip {
    names: Vec<SharedString>,
    open: Option<(usize, AnyElement)>,
    on_click: Option<IndexHandler>,
    on_hover: Option<IndexHandler>,
}

impl MenuStrip {
    /// Its colours come from `cx.theme()` as it renders. `_theme` is
    /// ignored: deprecated, kept so callers that pass a [`UiTheme`] build.
    pub fn new(names: impl IntoIterator<Item = SharedString>, _theme: &UiTheme) -> Self {
        Self {
            names: names.into_iter().collect(),
            open: None,
            on_click: None,
            on_hover: None,
        }
    }

    /// Shows `panel` open below the menu at `ix`.
    pub fn open(mut self, ix: usize, panel: impl IntoElement) -> Self {
        self.open = Some((ix, panel.into_any_element()));
        self
    }

    /// Called with a menu's index when its name is pressed.
    pub fn on_click(mut self, handler: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }

    /// Called with a menu's index when the mouse moves over its name.
    pub fn on_hover(mut self, handler: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_hover = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for MenuStrip {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = &cx.theme().colors;
        let hover = colors.list.row_hover;
        let (open, mut panel) = match self.open {
            Some((ix, panel)) => (Some(ix), Some(panel)),
            None => (None, None),
        };
        let names = self.names.into_iter().enumerate().map(|(ix, name)| {
            let is_open = open == Some(ix);
            let on_click = self.on_click.clone();
            let on_hover = self.on_hover.clone();
            let selector = format!("menu:{name}");
            div()
                .id(("menu", ix))
                .debug_selector(move || selector)
                .relative()
                .flex()
                .items_center()
                .h_full()
                .px_2()
                .rounded_sm()
                .map(|name| {
                    if is_open {
                        name.bg(colors.list.row_selected)
                            .text_color(colors.tab.active_text)
                    } else {
                        name.hover(move |style| style.bg(hover))
                    }
                })
                .child(name)
                .when_some(on_click, |name, on_click| {
                    name.on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        cx.stop_propagation();
                        on_click(ix, window, cx);
                    })
                })
                .when_some(on_hover, |name, on_hover| {
                    name.on_mouse_move(move |_, window, cx| on_hover(ix, window, cx))
                })
                .when(is_open, |name| {
                    let panel = panel.take().expect("one open menu");
                    name.child(
                        deferred(div().absolute().top(MENU_BAR_HEIGHT).left_0().child(panel))
                            .with_priority(1),
                    )
                })
        });
        div()
            .flex()
            .flex_row()
            .items_center()
            .h(MENU_BAR_HEIGHT)
            .px_1()
            .gap_px()
            .text_color(colors.text.primary)
            .children(names)
    }
}

/// One line of a menu's panel.
#[derive(Clone, Debug, PartialEq)]
pub enum MenuEntry {
    Item {
        title: SharedString,
        /// The key that runs it, as the platform prints it.
        shortcut: Option<SharedString>,
    },
    Separator,
}

/// A menu's items, below its name in a [`MenuStrip`]: each with its
/// shortcut, right-aligned, the highlighted one on the selection colour.
#[derive(IntoElement)]
pub struct MenuPanel {
    entries: Vec<MenuEntry>,
    highlighted: Option<usize>,
    on_click: Option<IndexHandler>,
    on_hover: Option<IndexHandler>,
}

impl MenuPanel {
    /// Its colours come from `cx.theme()` as it renders. `_theme` is
    /// ignored: deprecated, kept so callers that pass a [`UiTheme`] build.
    pub fn new(entries: Vec<MenuEntry>, _theme: &UiTheme) -> Self {
        Self {
            entries,
            highlighted: None,
            on_click: None,
            on_hover: None,
        }
    }

    pub fn highlighted(mut self, ix: Option<usize>) -> Self {
        self.highlighted = ix;
        self
    }

    /// Called with an item's index in the entries when it's clicked.
    pub fn on_click(mut self, handler: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }

    /// Called with an item's index when the mouse moves over it.
    pub fn on_hover(mut self, handler: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_hover = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for MenuPanel {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = &cx.theme().colors;
        let border = colors.border.default;
        let rows = self
            .entries
            .into_iter()
            .enumerate()
            .map(|(ix, entry)| match entry {
                MenuEntry::Separator => div().my_1().h_px().bg(border).into_any_element(),
                MenuEntry::Item { title, shortcut } => {
                    let on_click = self.on_click.clone();
                    let on_hover = self.on_hover.clone();
                    let selector = format!("menu-item:{title}");
                    div()
                        .id(("menu-item", ix))
                        .debug_selector(move || selector)
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap_6()
                        .h(px(24.))
                        .px_3()
                        .rounded_sm()
                        .when(self.highlighted == Some(ix), |row| {
                            row.bg(colors.list.row_selected)
                                .text_color(colors.tab.active_text)
                        })
                        .child(title)
                        .children(
                            shortcut.map(|keys| div().text_color(colors.text.muted).child(keys)),
                        )
                        .when_some(on_click, |row, on_click| {
                            row.on_click(move |_, window, cx| {
                                cx.stop_propagation();
                                on_click(ix, window, cx);
                            })
                        })
                        .when_some(on_hover, |row, on_hover| {
                            row.on_mouse_move(move |_, window, cx| on_hover(ix, window, cx))
                        })
                        .into_any_element()
                }
            });
        div()
            .occlude()
            // A press in the panel is the panel's: the bar closes the menu on
            // any press that reaches it.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .min_w(PANEL_MIN_WIDTH)
            .flex()
            .flex_col()
            .p_1()
            .bg(colors.surface.panel)
            .border_1()
            .border_color(border)
            .rounded_md()
            .shadow_lg()
            .text_color(colors.text.primary)
            .children(rows)
    }
}
