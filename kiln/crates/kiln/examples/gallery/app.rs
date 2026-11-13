// SPDX-License-Identifier: Apache-2.0

//! The gallery's window: a header with the theme switcher, a sidebar of
//! pages beside the page shown, split by a SplitPane, and a status bar.

use gpui::{
    AnyElement, AnyView, App, AppContext as _, ClickEvent, Context, Div, Entity, FocusHandle,
    Focusable, InteractiveElement as _, IntoElement, ParentElement as _, PromptLevel, Render,
    Styled as _, Subscription, Window, div, px,
};
use kiln::{
    command::CommandId,
    prelude::*,
    ui::{
        Button, ButtonVariant, GlobalTheme, List, ListDelegate, ListEvent, ListRow, RowState,
        SplitPane, StatusBarFrame, StatusEntry, ThemeMode, h_flex, ui_theme, v_flex,
    },
};
use kiln_l10n::Text;

use crate::{
    commands::{NextPage, PreviousPage, ShowPrompt, ToggleTheme},
    pages::{
        ChromePage, ControlsPage, LayoutPage, ListsPage, Page, PromptsPage, TextPage, ThemePage,
    },
    settings::GallerySettings,
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
    /// The sidebar, then the page shown.
    split: Entity<SplitPane>,
    _subscriptions: Vec<Subscription>,
}

impl Gallery {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let prompts = cx.new(|_| PromptsPage::new());
        let pages: Vec<AnyView> = Page::ALL
            .iter()
            .map(|page| match page {
                Page::Controls => cx.new(ControlsPage::new).into(),
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
        let split = cx.new(|_| {
            SplitPane::new(
                gpui::Axis::Horizontal,
                sidebar.clone(),
                pages[page.index()].clone(),
            )
            .first_size(px(200.))
            .with_min_sizes(px(150.), px(400.))
        });
        let subscriptions = vec![cx.subscribe_in(
            &sidebar,
            window,
            |gallery, _, event, window, cx| match *event {
                ListEvent::Selected(ix) | ListEvent::Confirmed(ix) => {
                    gallery.show(Page::ALL[ix], window, cx)
                }
            },
        )];
        Self {
            focus: cx.focus_handle(),
            page,
            pages,
            prompts,
            sidebar,
            split,
            _subscriptions: subscriptions,
        }
    }

    #[allow(dead_code, reason = "for tests/gallery.rs")]
    pub(crate) fn page(&self) -> Page {
        self.page
    }

    /// `page`'s view.
    #[allow(dead_code, reason = "for tests/gallery.rs")]
    pub(crate) fn page_view(&self, page: Page) -> AnyView {
        self.pages[page.index()].clone()
    }

    /// Shows `page`, selecting it in the sidebar.
    pub(crate) fn show(&mut self, page: Page, _: &mut Window, cx: &mut Context<Self>) {
        self.page = page;
        // Focus left in the page being hidden moves to the gallery once
        // the page isn't drawn: kiln watches the window.
        self.sidebar
            .update(cx, |list, cx| list.set_selected(Some(page.index()), cx));
        let view = self.pages[page.index()].clone();
        self.split
            .update(cx, |split, cx| split.set_second(view, cx));
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
        cx.set_theme_mode(mode);
    }

    fn show_prompt(&mut self, _: &ShowPrompt, window: &mut Window, cx: &mut Context<Self>) {
        self.prompts
            .update(cx, |page, cx| page.ask(PromptLevel::Info, window, cx));
    }

    fn header(&self, cx: &mut Context<Self>) -> Div {
        let c = cx.theme().colors;
        let title = cx.settings::<GallerySettings>().title.clone();
        let mode = cx.global::<GlobalTheme>().mode();
        let modes = [
            (ThemeMode::Dark, "Dark"),
            (ThemeMode::Light, "Light"),
            (ThemeMode::System, "System"),
        ];
        let switcher = h_flex().gap_1().children(modes.map(|(choice, label)| {
            Button::new(label, Text::verbatim(label))
                .small()
                .variant(if mode == Some(choice) {
                    ButtonVariant::Primary
                } else {
                    ButtonVariant::Ghost
                })
                .on_click(move |_: &ClickEvent, _, cx| cx.set_theme_mode(choice))
        }));
        // Buttons that run commands, as their keys and menu items do.
        let paging = h_flex()
            .gap_1()
            .child(
                Button::new("previous-page", Text::verbatim("Previous"))
                    .small()
                    .ghost()
                    .debug_selector(|| "previous-page".into())
                    .command(PreviousPage),
            )
            .child(
                Button::new("next-page", Text::verbatim("Next"))
                    .small()
                    .ghost()
                    .debug_selector(|| "next-page".into())
                    .command(NextPage),
            );
        h_flex()
            .flex_none()
            .h(px(40.))
            .px_4()
            .gap_3()
            .items_center()
            .border_b_1()
            .border_color(c.border.default)
            .bg(c.surface.panel)
            // From the user's settings.toml, reloaded as it's saved.
            .child(div().text_color(c.text.primary).child(title))
            // A fixed width keeps the paging buttons still between pages.
            .child(
                div()
                    .w(px(110.))
                    .text_color(c.text.muted)
                    .child(self.page.title()),
            )
            .child(paging)
            .child(div().flex_1())
            .child(div().text_color(c.text.muted).child("Theme"))
            .child(switcher)
    }

    fn status_bar(&self, cx: &mut Context<Self>) -> StatusBarFrame {
        let theme = ui_theme(cx);
        let hints: [(CommandId, _); 4] = [
            (NextPage::id(), "next page"),
            (PreviousPage::id(), "previous page"),
            (ToggleTheme::id(), "toggle theme"),
            (ShowPrompt::id(), "prompt"),
        ];
        let hints = hints
            .iter()
            .filter_map(|(command, what)| Some(format!("{} {what}", cx.binding_label(command)?)))
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
            // The sidebar shows the panel colour; each page paints the
            // background over it.
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
        _: &mut Context<List<Self>>,
    ) -> AnyElement {
        ListRow::new(state)
            .px_3()
            .child(Page::ALL[ix].title())
            .into_any_element()
    }
}
