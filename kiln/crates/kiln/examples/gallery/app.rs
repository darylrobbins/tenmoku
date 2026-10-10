// SPDX-License-Identifier: LicenseRef-Proprietary

//! The gallery's window: a header with the theme switcher, a sidebar of
//! pages beside the page shown, split by a SplitPane, and a status bar.

use gpui::{
    Action as _, AnyElement, AnyView, App, AppContext as _, ClickEvent, Context, Div, Entity,
    FocusHandle, Focusable, InteractiveElement as _, IntoElement, ParentElement as _, PromptLevel,
    Render, Styled as _, Subscription, Window, div, px,
};
use kiln::{
    core::shortcut_label,
    platform::{command_actions::command_id, keys},
    prelude::*,
    ui::{
        Button, GlobalTheme, List, ListDelegate, ListEvent, RowState, SplitPane, StatusBarFrame,
        StatusEntry, Theme, ThemeMode, h_flex, observe_system_appearance, set_themes, ui_theme,
        v_flex,
    },
};

use crate::{
    commands::{NextPage, PreviousPage, ShowPrompt, ToggleTheme},
    pages::{
        ChromePage, ControlsPage, LayoutPage, ListsPage, Page, PromptsPage, TextPage, ThemePage,
        list_row,
    },
};

/// The key context the gallery's root sets.
pub(crate) const KEY_CONTEXT: &str = "gallery";

pub(crate) struct Gallery {
    focus: FocusHandle,
    page: Page,
    /// Each page's view, in [`Page::ALL`] order, kept while another is
    /// shown so its state survives.
    pages: Vec<AnyView>,
    prompts: Entity<PromptsPage>,
    sidebar: Entity<List<PageList>>,
    host: Entity<PageHost>,
    split: Entity<SplitPane>,
    _subscriptions: Vec<Subscription>,
}

impl Gallery {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let prompts = cx.new(|_| PromptsPage::new());
        let pages: Vec<AnyView> = Page::ALL
            .iter()
            .map(|page| match page {
                Page::Controls => cx.new(|_| ControlsPage::new()).into(),
                Page::Text => cx.new(|cx| TextPage::new(window, cx)).into(),
                Page::Lists => cx.new(|cx| ListsPage::new(window, cx)).into(),
                Page::Layout => cx.new(|cx| LayoutPage::new(window, cx)).into(),
                Page::Chrome => cx.new(|_| ChromePage::new()).into(),
                Page::Prompts => prompts.clone().into(),
                Page::Theme => cx.new(|_| ThemePage).into(),
            })
            .collect();
        let page = Page::default();
        let sidebar = cx.new(|cx| {
            let mut list = List::new(PageList, cx);
            list.set_selected(Some(page.index()), cx);
            list
        });
        let host = cx.new(|_| PageHost {
            view: pages[page.index()].clone(),
        });
        let split = cx.new(|_| {
            SplitPane::new(gpui::Axis::Horizontal, sidebar.clone(), host.clone())
                .with_ratio(0.18)
                .with_min_sizes(px(150.), px(400.))
        });
        let subscriptions = vec![
            cx.subscribe_in(
                &sidebar,
                window,
                |gallery, _, event, window, cx| match *event {
                    ListEvent::Selected(ix) | ListEvent::Confirmed(ix) => {
                        gallery.show(Page::ALL[ix], window, cx)
                    }
                },
            ),
            // The System mode follows the window's appearance.
            observe_system_appearance(window),
        ];
        Self {
            focus: cx.focus_handle(),
            page,
            pages,
            prompts,
            sidebar,
            host,
            split,
            _subscriptions: subscriptions,
        }
    }

    #[allow(dead_code, reason = "for tests/gallery.rs")]
    pub(crate) fn page(&self) -> Page {
        self.page
    }

    /// Shows `page`, selecting it in the sidebar.
    pub(crate) fn show(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        self.page = page;
        // Focus left in the page being hidden stays on its element, which
        // is no longer drawn, and keys dispatched there reach nothing: the
        // gallery takes it back, unless the sidebar has it.
        if !self.sidebar.focus_handle(cx).is_focused(window) {
            window.focus(&self.focus, cx);
        }
        self.sidebar
            .update(cx, |list, cx| list.set_selected(Some(page.index()), cx));
        let view = self.pages[page.index()].clone();
        self.host.update(cx, |host, cx| {
            host.view = view;
            cx.notify();
        });
        cx.notify();
    }

    fn next_page(&mut self, _: &NextPage, window: &mut Window, cx: &mut Context<Self>) {
        self.show(self.page.next(), window, cx);
    }

    fn previous_page(&mut self, _: &PreviousPage, window: &mut Window, cx: &mut Context<Self>) {
        self.show(self.page.previous(), window, cx);
    }

    fn toggle_theme(&mut self, _: &ToggleTheme, _: &mut Window, cx: &mut Context<Self>) {
        let dark = cx.theme().appearance == kiln::ui::Appearance::Dark;
        let mode = if dark {
            ThemeMode::Light
        } else {
            ThemeMode::Dark
        };
        set_mode(mode, cx);
    }

    fn show_prompt(&mut self, _: &ShowPrompt, window: &mut Window, cx: &mut Context<Self>) {
        self.prompts
            .update(cx, |page, cx| page.ask(PromptLevel::Info, window, cx));
    }

    fn header(&self, cx: &mut Context<Self>) -> Div {
        let c = cx.theme().colors;
        let mode = cx.global::<GlobalTheme>().mode();
        let modes = [
            (ThemeMode::Dark, "Dark"),
            (ThemeMode::Light, "Light"),
            (ThemeMode::System, "System"),
        ];
        let switcher = h_flex().gap_1().children(modes.map(|(choice, label)| {
            Button::new(label, label)
                .small()
                .variant(if mode == Some(choice) {
                    kiln::ui::ButtonVariant::Primary
                } else {
                    kiln::ui::ButtonVariant::Ghost
                })
                .on_click(move |_: &ClickEvent, _, cx| set_mode(choice, cx))
        }));
        h_flex()
            .flex_none()
            .h(px(40.))
            .px_4()
            .gap_3()
            .items_center()
            .border_b_1()
            .border_color(c.border.default)
            .bg(c.surface.panel)
            .child(div().text_color(c.text.primary).child("Kiln Gallery"))
            .child(div().text_color(c.text.muted).child(self.page.title()))
            .child(div().flex_1())
            .child(div().text_color(c.text.muted).child("Theme"))
            .child(switcher)
    }

    fn status_bar(&self, cx: &mut Context<Self>) -> StatusBarFrame {
        let theme = ui_theme(cx);
        let hints = [
            (NextPage.name(), "next page"),
            (PreviousPage.name(), "previous page"),
            (ToggleTheme.name(), "toggle theme"),
            (ShowPrompt.name(), "prompt"),
        ]
        .iter()
        .filter_map(|(action, what)| Some(format!("{} {what}", shortcut(action, cx)?)))
        .collect::<Vec<_>>()
        .join("   ");
        let position = format!(
            "Page {} of {}: {}",
            self.page.index() + 1,
            Page::ALL.len(),
            self.page.title()
        );
        StatusBarFrame::new(&theme)
            .left([StatusEntry::new("page", position, &theme)])
            .right([StatusEntry::new("keys", hints, &theme)])
    }
}

/// Sets the theme mode, over kiln's bundled dark and light themes.
pub(crate) fn set_mode(mode: ThemeMode, cx: &mut App) {
    set_themes(cx, mode, Theme::dark(), Theme::light());
}

/// The label of the key the keymap binds to the command GPUI calls
/// `action`, such as `Ctrl+]`.
fn shortcut(action: &str, cx: &App) -> Option<String> {
    let id = command_id(action)?;
    let keymap = keys::current_keymap(cx);
    let binding = keymap.bindings_for(&id).into_iter().next()?;
    Some(shortcut_label(&binding.keys))
}

impl Focusable for Gallery {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Gallery {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = cx.theme().colors;
        v_flex()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::next_page))
            .on_action(cx.listener(Self::previous_page))
            .on_action(cx.listener(Self::toggle_theme))
            .on_action(cx.listener(Self::show_prompt))
            .size_full()
            // The sidebar shows the panel colour; the page host paints the
            // page's background over it.
            .bg(c.surface.panel)
            .text_color(c.text.primary)
            .text_size(px(13.))
            .child(self.header(cx))
            .child(div().flex_1().min_h_0().child(self.split.clone()))
            .child(self.status_bar(cx))
    }
}

/// The sidebar's rows: the pages' titles.
pub(crate) struct PageList;

impl ListDelegate for PageList {
    fn count(&self) -> usize {
        Page::ALL.len()
    }

    fn render_row(
        &mut self,
        ix: usize,
        state: RowState,
        _: &mut Window,
        cx: &mut Context<List<Self>>,
    ) -> AnyElement {
        let c = cx.theme().colors;
        list_row(state, &c)
            .px_3()
            .child(Page::ALL[ix].title())
            .into_any_element()
    }
}

/// The page side of the split: whichever page is shown. SplitPane holds
/// its two views for good, so swapping the page means a view in between.
pub(crate) struct PageHost {
    view: AnyView,
}

impl Render for PageHost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(cx.theme().colors.surface.background)
            .child(self.view.clone())
    }
}
