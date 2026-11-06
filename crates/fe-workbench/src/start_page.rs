//! The Start page: what a window's empty pane shows while no tab is open
//! anywhere in it, with `workbench.start_page` on. Not a tab: the pane draws
//! it in its empty state ([`crate::Pane::set_start_page`]) and it goes as a
//! tab opens, coming back when the last closes. Its rows are
//! [`crate::start_page_model`]'s, read again each time it comes back: New
//! File, Open File, Open Folder, the recent folders and files, and the
//! settings.
//!
//! It takes the pane's focus handle as its own, so whatever focuses the
//! empty pane (closing its last tab, a click, the workbench's focus after a
//! dock or the setup wizard closes) focuses the page. While it has focus,
//! up and down move the selection and Enter runs the selected row; a click
//! runs a row too. With vim on, LazyVim's dashboard keys run their rows,
//! the key's letter underlined in the row's label (`n`, `f`, `p`, `g`, `r`,
//! `s`, `q`), and `j` and `k` move as up and down do. With vim off a letter
//! goes on as if the page weren't there.
//!
//! While the setup wizard's sheet is over the window the page is inert: it
//! runs nothing and takes no keys ([`StartPageView::set_inert`]).
//!
//! A recent row runs `start_page.open_recent_folder` or
//! `start_page.open_recent_file` with its path, typed commands of the page's
//! own, which the palette doesn't list.

#[cfg(test)]
use fe_command::Invocation;
use fe_command::{CommandId, Source};
use fe_ui::{Icon, IconRef, ROW_HEIGHT, file_icon_ref, folder_icon_ref};
use gpui::{
    AnyElement, App, AppContext as _, Context, FocusHandle, FontWeight, HighlightStyle,
    InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, StyledText, Task, UnderlineStyle, WeakEntity,
    Window, div, prelude::FluentBuilder as _, px,
};
use kiln::{
    Cx, ViewCommands as _,
    platform::command_actions::{dispatch_invocation, typed_command},
    platform::keys,
};

use crate::{
    Workbench,
    start_page_model::{CLONE, Options, Row, RowIcon, StartPage, invocation_path},
    workbench::recent_picker,
};

/// The page's column, centred in the pane.
const COLUMN_WIDTH: gpui::Pixels = px(480.);

pub(crate) struct StartPageView {
    workbench: WeakEntity<Workbench>,
    /// The pane's that shows it, tracked by the page in its place.
    focus: FocusHandle,
    page: StartPage,
    /// Whether the rows of the latest read are in.
    loaded: bool,
    /// The selected row, among all the sections' rows in order.
    selected: usize,
    /// While the setup wizard shows: no keys, no runs.
    inert: bool,
    _load: Option<Task<()>>,
    /// Every invocation a row ran, oldest first.
    #[cfg(test)]
    pub(crate) ran: Vec<Invocation>,
}

#[kiln::command(context = "start_page")]
impl StartPageView {
    /// Open Recent Folder
    #[command(id = "start_page.open_recent_folder", palette = false)]
    fn open_recent_folder(&mut self, cx: &mut Cx<Self>) {
        let (Some(path), Some(workbench)) = (invoked_path(cx.cx), self.workbench.upgrade()) else {
            return;
        };
        let window = &mut *cx.window;
        workbench.update(cx.cx, |workbench, cx| workbench.set_root(path, window, cx));
    }

    /// Open Recent File
    #[command(id = "start_page.open_recent_file", palette = false)]
    fn open_recent_file(&mut self, cx: &mut Cx<Self>) {
        let (Some(path), Some(workbench)) = (invoked_path(cx.cx), self.workbench.upgrade()) else {
            return;
        };
        let pane = workbench.read(cx.cx).pane().clone();
        let window = &mut *cx.window;
        pane.update(cx.cx, |pane, cx| pane.open(path, window, cx));
    }
}

/// The path the running command was invoked with.
fn invoked_path(cx: &App) -> Option<std::path::PathBuf> {
    invocation_path(&kiln::current_invocation(cx)?)
}

impl StartPageView {
    pub(crate) fn new(workbench: WeakEntity<Workbench>, cx: &mut Context<Self>) -> Self {
        Self {
            workbench,
            focus: cx.focus_handle(),
            page: StartPage::default(),
            loaded: false,
            selected: 0,
            inert: false,
            _load: None,
            #[cfg(test)]
            ran: Vec::new(),
        }
    }

    /// Takes `focus`, the focus handle of the pane it shows in, as its own.
    pub(crate) fn set_focus(&mut self, focus: FocusHandle, cx: &mut Context<Self>) {
        if self.focus != focus {
            self.focus = focus;
            cx.notify();
        }
    }

    pub(crate) fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    /// While `inert`, under the setup wizard's sheet, the page takes no keys
    /// and runs nothing.
    pub(crate) fn set_inert(&mut self, inert: bool) {
        self.inert = inert;
    }

    /// Reads the recent list again, in the background, and lists it, the
    /// first row selected: as the page comes back.
    pub(crate) fn reload(&mut self, cx: &mut Context<Self>) {
        let list = recent_picker::load(cx);
        let vim = fe_gpui::vim_enabled(cx);
        let clone =
            (CommandId::parse(CLONE).ok()).is_some_and(|id| typed_command(&id, cx).is_some());
        let home = std::env::home_dir();
        self._load = Some(cx.spawn(async move |view, cx| {
            let list = list.await;
            let made = cx.background_spawn(async move {
                let home = home.as_deref();
                StartPage::from(&list, Options { vim, clone, home })
            });
            let page = made.await;
            view.update(cx, |view, cx| {
                view.page = page;
                view.loaded = true;
                view.selected = 0;
                cx.notify();
            })
            .ok();
        }));
    }

    #[cfg(test)]
    pub(crate) fn page(&self) -> &StartPage {
        &self.page
    }

    #[cfg(test)]
    pub(crate) fn selected(&self) -> usize {
        self.selected
    }

    #[cfg(test)]
    pub(crate) fn is_loaded(&self) -> bool {
        self.loaded
    }

    fn select(&mut self, by: isize, cx: &mut Context<Self>) {
        let len = self.page.len();
        if len == 0 {
            return;
        }
        let at = self.selected.min(len - 1) as isize + by;
        self.selected = at.clamp(0, len as isize - 1) as usize;
        cx.notify();
    }

    /// Runs row `ix`'s command from the page's focus, once this event is
    /// over: the page's own commands update it.
    fn run(&mut self, ix: usize, source: Source, window: &mut Window, cx: &mut Context<Self>) {
        if self.inert {
            return;
        }
        let Some(row) = self.page.row(ix) else {
            return;
        };
        self.selected = ix;
        let invocation = row.invocation.clone().source(source);
        #[cfg(test)]
        self.ran.push(invocation.clone());
        cx.notify();
        window.defer(cx, move |window, cx| {
            let stack = keys::context_stack(&window.context_stack(), cx);
            if let Err(error) = dispatch_invocation(invocation.context(stack), window, cx) {
                log::warn!("{error}");
            }
        });
    }

    /// Its keys: see the module's docs. Any other key goes on.
    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if self.inert || !self.loaded || keystroke.modifiers.modified() {
            return;
        }
        // Vim turned off since the rows were read takes the letters with it.
        let vim = self.page.vim && fe_gpui::vim_enabled(cx);
        match keystroke.key.as_str() {
            "up" => self.select(-1, cx),
            "down" => self.select(1, cx),
            "k" if vim => self.select(-1, cx),
            "j" if vim => self.select(1, cx),
            "enter" if !event.is_held => self.run(self.selected, Source::Keyboard, window, cx),
            key if vim && !event.is_held => {
                let mut chars = key.chars();
                let (Some(key), None) = (chars.next(), chars.next()) else {
                    return;
                };
                let Some(ix) = self.page.row_for_key(key) else {
                    return;
                };
                self.run(ix, Source::Keyboard, window, cx);
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    fn render_row(
        &self,
        ix: usize,
        row: &Row,
        focused: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let ui = fe_ui::ui_theme(cx);
        let (icon, tint) = match &row.icon {
            RowIcon::Folder => (folder_icon_ref(false), ui.folder_icon),
            RowIcon::File(name) => (file_icon_ref(name), ui.icon),
            icon => (IconRef::Bundled(bundled(icon)), ui.icon),
        };
        let label: SharedString = row.label.clone().into();
        let underline = HighlightStyle {
            underline: Some(UnderlineStyle {
                thickness: px(1.),
                ..UnderlineStyle::default()
            }),
            ..HighlightStyle::default()
        };
        let highlights = row.key_in_label().map(|range| (range, underline));
        let label = StyledText::new(label).with_highlights(highlights);
        let selected = ix == self.selected;
        let selected_bg = if focused {
            ui.row_selected
        } else {
            ui.row_selected_inactive
        };
        let key = row.key.map(|key| {
            div()
                .ml_auto()
                .flex_none()
                .pl_3()
                .text_color(ui.accent)
                .font_weight(FontWeight::SEMIBOLD)
                .child(SharedString::from(key.to_string()))
        });
        div()
            .id(("start-page-row", ix))
            .debug_selector(|| format!("start-page-row:{}", row.label))
            .h(ROW_HEIGHT)
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .rounded_sm()
            .overflow_hidden()
            .whitespace_nowrap()
            .when(selected, |row| row.bg(selected_bg))
            .hover(move |row| row.bg(ui.row_hover))
            .on_click(
                cx.listener(move |view, _, window, cx| view.run(ix, Source::Mouse, window, cx)),
            )
            .child(icon.element(px(16.), tint))
            .child(div().flex_none().text_color(ui.text).child(label))
            .children(row.detail.clone().map(|detail| {
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .text_color(ui.muted_text)
                    .child(detail)
            }))
            .children(key)
            .into_any_element()
    }
}

/// The bundled icon of a command row.
fn bundled(icon: &RowIcon) -> Icon {
    match icon {
        RowIcon::NewFile => Icon::File,
        RowIcon::OpenFile => Icon::FileText,
        RowIcon::OpenFolder | RowIcon::Folder => Icon::FolderOpen,
        RowIcon::Clone => Icon::GitBranch,
        RowIcon::Search => Icon::Search,
        RowIcon::Recent => Icon::Files,
        RowIcon::Quit => Icon::X,
        RowIcon::Settings => Icon::FileCog,
        RowIcon::Shortcuts => Icon::Braces,
        RowIcon::File(_) => Icon::File,
    }
}

impl Render for StartPageView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Vim turned on or off since the rows were read: the dashboard's
        // keys and rows come or go.
        if self.loaded && self.page.vim != fe_gpui::vim_enabled(cx) {
            self.loaded = false;
            cx.defer_in(window, |view, _, cx| view.reload(cx));
        }
        let ui = fe_ui::ui_theme(cx);
        let focused = self.focus.is_focused(window);
        let mut ix = 0;
        let mut sections = Vec::new();
        for section in &self.page.sections {
            let mut rows = Vec::new();
            for row in &section.rows {
                rows.push(self.render_row(ix, row, focused, cx));
                ix += 1;
            }
            sections.push(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .px_2()
                            .text_size(px(11.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(ui.muted_text)
                            .child(section.kind.title()),
                    )
                    .children(rows),
            );
        }
        let page = Self::attach(div().id("start-page"), cx)
            .debug_selector(|| "start-page".into())
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .size_full()
            .overflow_y_scroll()
            .flex()
            .justify_center()
            .text_size(px(12.))
            .bg(ui.background);
        page.child(
            div()
                .w(COLUMN_WIDTH)
                .max_w_full()
                .px_4()
                .py(px(48.))
                .flex()
                .flex_col()
                .gap_4()
                .child(
                    div()
                        .px_2()
                        .text_size(px(15.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(ui.text)
                        .child(fe_config::APP_NAME),
                )
                .children(sections),
        )
    }
}
