//! The quickfix list, or a pane's location list, in the sidebar, as nvim's
//! quickfix window shows it (`:copen`, `:lopen`): one row per item,
//! `path:row:col  text`, the place dimmed and the text tinted by the item's
//! kind, under a header with the list's title (`:grep foo`, `:make`) and how
//! many items it has. The current item is highlighted, and scrolled into
//! view as `:cnext` and the rest move it.
//!
//! The rows are the lists `quickfix_commands.rs` keeps, read as they're
//! drawn: the panel holds no copy, and the workbench tells it when a list
//! changes ([`Workbench::quickfix_changed`]). Up and Down (with vim on,
//! `j`, `k`, `gg` and `G` too) move a cursor through the rows, which starts
//! on the current item whenever it moves; Enter (with vim on, `o`) or a
//! click lands on the row's item as `:cc N` does, making it current, in the
//! pane whose list it is. With vim on, the panel's key context has the
//! `vim` flag, as the references panel's has.
//!
//! The panel shows one list: the quickfix list, or the location list of the
//! pane that ran `:lopen` (or `location.show`), until another is shown.

use std::{ops::Range, path::PathBuf};

use fe_editor::diagnostics::Severity;
use fe_gpui::EditorTheme;
use fe_ui::{Icon, ROW_HEIGHT};
use gpui::{
    AnyElement, App, Context, ElementId, EntityId, FocusHandle, Focusable, Hsla,
    InteractiveElement as _, IntoElement, KeyContext, KeyDownEvent, ParentElement as _, Render,
    ScrollStrategy, SharedString, StatefulInteractiveElement as _, Styled as _,
    UniformListScrollHandle, WeakEntity, Window, div, prelude::FluentBuilder as _, px,
    uniform_list,
};

use crate::{
    Workbench,
    dock::{DockOp, SidebarPanel},
    quickfix::{QuickfixItem, QuickfixKind, QuickfixList},
    quickfix_commands::{shown_list, shown_revision},
};

/// The panel in the sidebar: on the rail once it has shown; focused when
/// shown, as nvim's `:copen` goes to the window.
pub(crate) const SIDEBAR: SidebarPanel = SidebarPanel {
    panel: crate::DockPanel::new("quickfix"),
    name: "Quickfix List",
    icon: Icon::List,
    show_command: "quickfix.show",
    show_op: DockOp::ShowQuickfix,
    order: 90,
    always_on_rail: false,
    focus_on_show: true,
};

/// The key context the panel's element sets.
pub(crate) const KEY_CONTEXT: &str = "quickfix_panel";

/// The flag the panel's key context has while vim is on.
const VIM_FLAG: &str = "vim";

/// The title row's name while the panel shows a location list.
const LOCATION_NAME: &str = "Location List";

pub(crate) struct QuickfixPanel {
    workbench: WeakEntity<Workbench>,
    /// The window's lists are its workbench's.
    workbench_id: EntityId,
    focus: FocusHandle,
    /// The pane whose location list it shows; none for the quickfix list.
    source: Option<EntityId>,
    /// The cursor's row.
    selected: usize,
    /// Vim's `g`, waiting for the second of `gg`.
    pending_g: bool,
    /// Drops `pending_g` when focus leaves, however it does.
    focus_out: Option<gpui::Subscription>,
    scroll: UniformListScrollHandle,
    /// Error, warning, then info and note.
    colors: [Hsla; 3],
    #[cfg(test)]
    pub(crate) test: TestHooks,
}

/// What the tests read of the last frame.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct TestHooks {
    /// The rows built, by row, as `(place, text)`: row 0 too when
    /// uniform_list only measures it.
    pub(crate) rows: std::collections::BTreeMap<usize, (String, String)>,
    /// The header's title and count.
    pub(crate) header: Option<(String, String)>,
    /// The current item's row, as drawn highlighted.
    pub(crate) highlighted: Option<usize>,
}

/// A row as drawn: its place, text and kind.
struct RowText {
    place: String,
    text: String,
    kind: QuickfixKind,
}

impl QuickfixPanel {
    /// A panel showing the quickfix list of `workbench`'s window, its kinds
    /// tinted as `theme`'s diagnostics are.
    pub(crate) fn new(
        workbench: &gpui::Entity<Workbench>,
        theme: &EditorTheme,
        cx: &mut App,
    ) -> Self {
        let colors = [Severity::Error, Severity::Warning, Severity::Information]
            .map(|severity| theme.diagnostic_color(severity));
        Self {
            workbench: workbench.downgrade(),
            workbench_id: workbench.entity_id(),
            focus: cx.focus_handle(),
            source: None,
            selected: 0,
            pending_g: false,
            focus_out: None,
            scroll: UniformListScrollHandle::new(),
            colors,
            #[cfg(test)]
            test: TestHooks::default(),
        }
    }

    /// The pane whose location list it shows; none for the quickfix list.
    pub(crate) fn source(&self) -> Option<EntityId> {
        self.source
    }

    /// Shows the quickfix list, or with `pane` that pane's location list,
    /// the cursor on its current item.
    pub(crate) fn show_list(&mut self, pane: Option<EntityId>, cx: &mut Context<Self>) {
        self.source = pane;
        self.list_changed(cx);
    }

    /// The list it shows has changed, or moved to another item: puts the
    /// cursor on the current item, scrolled into view.
    pub(crate) fn list_changed(&mut self, cx: &mut Context<Self>) {
        let (source, workbench) = (self.source, self.workbench_id);
        let current = shown_list(workbench, source, cx).map_or(0, |list| list.current);
        self.select(current, cx);
    }

    /// The title row's text: which list it shows.
    pub(crate) fn title(&self, _: &App) -> SharedString {
        match self.source {
            Some(_) => LOCATION_NAME.into(),
            None => SIDEBAR.name.into(),
        }
    }

    /// The list it shows, if there is one.
    fn list<'a>(&self, cx: &'a mut App) -> Option<&'a QuickfixList> {
        shown_list(self.workbench_id, self.source, cx)
    }

    /// How many rows the list has.
    fn len(&self, cx: &mut App) -> usize {
        self.list(cx).map_or(0, |list| list.items.len())
    }

    /// Puts the cursor on row `ix` (clamped), scrolled into view.
    fn select(&mut self, ix: usize, cx: &mut Context<Self>) {
        let len = self.len(cx);
        self.selected = ix.min(len.saturating_sub(1));
        self.pending_g = false;
        if len > 0 {
            self.scroll
                .scroll_to_item(self.selected, ScrollStrategy::Nearest);
        }
        cx.notify();
    }

    /// Moves the cursor `by` rows, stopping at either end.
    fn move_selection(&mut self, by: isize, cx: &mut Context<Self>) {
        let ix = self.selected.saturating_add_signed(by);
        self.select(ix, cx);
    }

    /// Lands on row `ix`'s item, as `:cc N` does, in the pane whose list it
    /// is.
    /// Nothing if the list shown is no longer the one `revision` names,
    /// when the row was activated or when the open runs.
    fn open(&mut self, ix: usize, revision: u64, window: &mut Window, cx: &mut Context<Self>) {
        if shown_revision(self.workbench_id, self.source, cx) != Some(revision) {
            return;
        }
        self.selected = ix;
        let source = self.source;
        if let Some(workbench) = self.workbench.upgrade() {
            // The workbench tells the panel of the move, so not inside an
            // update of it.
            window.defer(cx, move |window, cx| {
                workbench.update(cx, |workbench, cx| {
                    workbench.open_quickfix_row(source, revision, ix, window, cx)
                });
            });
        }
    }

    /// Lands on the cursor's row.
    fn open_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(revision) = shown_revision(self.workbench_id, self.source, cx) {
            self.open(self.selected, revision, window, cx);
        }
    }

    /// Its element's key context: [`KEY_CONTEXT`], with the `vim` flag
    /// while vim is on.
    fn key_context(&self, cx: &App) -> KeyContext {
        let mut context = KeyContext::default();
        context.add(KEY_CONTEXT);
        if fe_gpui::vim_enabled(cx) {
            context.add(VIM_FLAG);
        }
        context
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;
        let shift_only = modifiers.shift
            && !(modifiers.control || modifiers.alt)
            && !(modifiers.platform || modifiers.function);
        if modifiers.modified() && !shift_only {
            return;
        }
        let vim = fe_gpui::vim_enabled(cx);
        let pending_g = std::mem::take(&mut self.pending_g);
        let shifted = modifiers.shift;
        match keystroke.key.as_str() {
            "up" if !shifted => self.move_selection(-1, cx),
            "down" if !shifted => self.move_selection(1, cx),
            "enter" if !shifted => self.open_selected(window, cx),
            "k" if vim && !shifted => self.move_selection(-1, cx),
            "j" if vim && !shifted => self.move_selection(1, cx),
            "o" if vim && !shifted => self.open_selected(window, cx),
            "g" if vim && !shifted && pending_g => self.select(0, cx),
            "g" if vim && !shifted => self.pending_g = true,
            "g" if vim && shifted => self.select(usize::MAX, cx),
            _ => return,
        }
        cx.stop_propagation();
    }

    fn color(&self, kind: QuickfixKind) -> Option<Hsla> {
        match kind {
            QuickfixKind::Error => Some(self.colors[0]),
            QuickfixKind::Warning => Some(self.colors[1]),
            QuickfixKind::Info | QuickfixKind::Note => Some(self.colors[2]),
            QuickfixKind::Untyped => None,
        }
    }

    /// The rows `range` of the list, as drawn.
    fn row_texts(&self, range: Range<usize>, root: Option<PathBuf>, cx: &mut App) -> Vec<RowText> {
        let Some(list) = self.list(cx) else {
            return Vec::new();
        };
        let end = range.end.min(list.items.len());
        let start = range.start.min(end);
        (list.items[start..end].iter())
            .map(|item| row_text(item, root.as_deref()))
            .collect()
    }

    fn render_rows(
        &mut self,
        range: Range<usize>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let ui = fe_ui::ui_theme(cx);
        let root = (self.workbench.upgrade())
            .and_then(|workbench| workbench.read(cx).root().map(ToOwned::to_owned));
        let current = self.list(cx).map(|list| list.current);
        let texts = self.row_texts(range.clone(), root, cx);
        let focused = self.focus.contains_focused(window, cx);
        let cursor_bg = if focused {
            ui.row_selected
        } else {
            ui.row_selected_inactive
        };
        // The shown list's identity is in each row's ID, so a press on one
        // list's row and a release on the next's are not a click.
        let revision = shown_revision(self.workbench_id, self.source, cx);
        let name = SharedString::from(format!(
            "quickfix-row-{:?}-{}",
            self.source,
            revision.unwrap_or(0)
        ));
        (range.zip(texts))
            .map(|(ix, row)| {
                #[cfg(test)]
                self.test
                    .rows
                    .insert(ix, (row.place.clone(), row.text.clone()));
                let is_current = current == Some(ix);
                #[cfg(test)]
                if is_current {
                    self.test.highlighted = Some(ix);
                }
                let text_color = self.color(row.kind).unwrap_or(ui.text);
                div()
                    .id(ElementId::NamedInteger(name.clone(), ix as u64))
                    .h(ROW_HEIGHT)
                    .w_full()
                    .px_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .when(is_current, |row| {
                        row.bg(ui.row_selected_inactive)
                            .border_l_2()
                            .border_color(ui.row_focus_border)
                    })
                    .when(self.selected == ix && focused, |row| row.bg(cursor_bg))
                    .hover(move |row| row.bg(ui.row_hover))
                    .on_click(cx.listener(move |panel, _, window, cx| {
                        if let Some(revision) = revision {
                            panel.open(ix, revision, window, cx);
                        }
                    }))
                    .child(div().flex_none().text_color(ui.muted_text).child(row.place))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text_color(text_color)
                            .child(row.text),
                    )
                    .into_any_element()
            })
            .collect()
    }

    /// The header: the list's title and how many items it has, or what
    /// shows in place of a list.
    fn header(&self, cx: &mut App) -> (String, String) {
        match self.list(cx) {
            Some(list) => {
                let n = list.items.len();
                let s = if n == 1 { "" } else { "s" };
                (list.title.clone(), format!("{n} item{s}"))
            }
            None if self.source.is_some() => ("No location list".into(), String::new()),
            None => ("No quickfix list".into(), String::new()),
        }
    }
}

/// `item` as its row shows it: `path:row:col`, the path from `root` if
/// it's under it, and its text on one line.
fn row_text(item: &QuickfixItem, root: Option<&std::path::Path>) -> RowText {
    let path = root
        .and_then(|root| item.path.strip_prefix(root).ok())
        .unwrap_or(&item.path);
    let place = format!(
        "{}:{}:{}",
        path.display(),
        u64::from(item.row) + 1,
        u64::from(item.col) + 1
    );
    let text = one_line(&item.text);
    RowText {
        place,
        text,
        kind: item.kind,
    }
}

/// `text` on one line, as nvim's quickfix window shows an item's: its
/// leading blanks dropped, and each line break, with the blanks after it,
/// one space.
fn one_line(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.trim_start().chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\n' {
            out.push(' ');
            while chars.next_if(|ch| ch.is_whitespace()).is_some() {}
        } else {
            out.push(ch);
        }
    }
    out
}

impl Focusable for QuickfixPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for QuickfixPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.focus_out.is_none() {
            self.focus_out = Some(cx.on_focus_out(&self.focus, window, |panel, _, _, _| {
                panel.pending_g = false;
            }));
        }
        #[cfg(test)]
        {
            self.test.rows.clear();
            self.test.highlighted = None;
        }
        let ui = fe_ui::ui_theme(cx);
        let (title, count) = self.header(cx);
        #[cfg(test)]
        {
            self.test.header = Some((title.clone(), count.clone()));
        }
        let len = self.len(cx);
        let list = uniform_list(
            "quickfix-rows",
            len,
            cx.processor(|panel, range, window, cx| panel.render_rows(range, window, cx)),
        )
        .track_scroll(&self.scroll)
        .flex_1();
        div()
            .track_focus(&self.focus)
            .key_context(self.key_context(cx))
            .on_key_down(cx.listener(Self::on_key_down))
            .debug_selector(|| "sidebar-panel".into())
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .border_l_1()
            .border_color(ui.border)
            .bg(ui.panel_background)
            .text_color(ui.text)
            .text_size(px(12.))
            .child(
                div()
                    .flex_none()
                    .px_3()
                    .py_1()
                    .flex()
                    .gap_2()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(div().flex_1().min_w_0().overflow_hidden().child(title))
                    .child(div().flex_none().text_color(ui.muted_text).child(count)),
            )
            .child(list)
    }
}

#[cfg(test)]
#[path = "quickfix_panel_tests.rs"]
mod quickfix_panel_tests;
