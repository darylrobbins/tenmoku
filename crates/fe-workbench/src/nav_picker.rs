//! The nav commands' palettes: the definitions an ambiguous go-to-definition
//! found, and workspace symbols matching the input.

use std::{
    fmt::Write as _,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use async_channel::Receiver;
use fe_nav::{Definition, Location, Navigation, OutlineKind, Precision, WorkspaceSymbol};
use fe_ui::{HighlightedLabel, UiTheme, char_ranges};
use gpui::{
    AnyElement, App, AppContext as _, Context, FocusHandle, IntoElement as _, ParentElement as _,
    SharedString, Styled as _, Task, WeakEntity, Window, div,
};

use kiln_l10n::{Text, tr};

use crate::{
    Picker, PickerDelegate, Workbench,
    nav::{show_source, source},
};

/// The most symbols a query shows.
pub(crate) const LIMIT: usize = 50;

/// The definitions of an ambiguous go-to-definition, in the answer's order,
/// or none, its status saying why. The input doesn't filter them.
pub(crate) struct DefinitionsPicker {
    workbench: WeakEntity<Workbench>,
    items: Vec<Definition>,
    /// Resolved as it's drawn.
    status: Text,
    /// Focused before the picker opened, and again once it's dismissed.
    previous_focus: Option<FocusHandle>,
}

impl DefinitionsPicker {
    pub(crate) fn new(workbench: WeakEntity<Workbench>) -> Self {
        Self {
            workbench,
            items: Vec::new(),
            status: Text::verbatim(""),
            previous_focus: None,
        }
    }

    pub(crate) fn open(
        &mut self,
        items: Vec<Definition>,
        status: Text,
        previous_focus: Option<FocusHandle>,
    ) {
        self.items = items;
        self.status = status;
        // Shown again while it shows, it keeps what it restores.
        self.previous_focus = previous_focus.or(self.previous_focus.take());
    }

    #[cfg(test)]
    pub(crate) fn items(&self) -> &[Definition] {
        &self.items
    }

    #[cfg(test)]
    pub(crate) fn status_text(&self) -> String {
        self.status().unwrap_or_default().to_string()
    }
}

impl PickerDelegate for DefinitionsPicker {
    fn update_matches(&mut self, _: String, cx: &mut Context<Picker<Self>>) {
        cx.notify();
    }

    fn match_count(&self) -> usize {
        self.items.len()
    }

    fn match_id(&self, ix: usize) -> SharedString {
        row_id(&self.items[ix].location)
    }

    fn render_match(&self, ix: usize, _: bool, cx: &App) -> AnyElement {
        let ui = fe_ui::ui_theme(cx);
        let item = &self.items[ix];
        let label = HighlightedLabel::new(item.name.clone(), Vec::new(), &ui);
        let container = item.container.clone().unwrap_or_default();
        row(label, place(&item.location), container, &ui)
    }

    fn confirm(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Picker<Self>>) -> bool {
        let location = self.items[ix].location.clone();
        self.previous_focus = None;
        jump(&self.workbench, &location, window, cx)
    }

    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        if let Some(focus) = self.previous_focus.take() {
            window.focus(&focus, cx);
        }
    }

    fn status(&self) -> Option<SharedString> {
        Some(self.status.resolve())
    }
}

/// Workspace symbols matching the input. One query runs at a time: one
/// typed meanwhile waits in its place, replacing any already waiting, and
/// results for text the input no longer holds are dropped. Until the
/// input's text has results, picking a row is refused.
pub(crate) struct SymbolsPicker {
    workbench: WeakEntity<Workbench>,
    navigation: Arc<Navigation>,
    /// Closed once the search provider is in; `None` with no folder open.
    ready: Option<Receiver<()>>,
    /// Files indexed so far, while the index builds.
    indexing: Option<usize>,
    /// The input's text, as last given to `update_matches`.
    query: String,
    /// What `matches` are for.
    shown: Option<String>,
    matches: Vec<WorkspaceSymbol>,
    /// How the shown matches were found, or why they weren't: the
    /// search's error, or none before a query lands.
    answer: Result<(String, Precision), Option<String>>,
    /// The query to run once the running one is done.
    pending: Option<String>,
    querying: Option<Task<()>>,
    /// Bumped on each opening and root change, so a query from before
    /// (maybe for another root) never lands.
    opening: u64,
    /// The running query's.
    cancel: Arc<AtomicBool>,
    previous_focus: Option<FocusHandle>,
    #[cfg(test)]
    pub(crate) queries: usize,
    /// Awaited before the next query's answer lands.
    #[cfg(test)]
    pub(crate) hold: Option<Receiver<()>>,
}

impl SymbolsPicker {
    pub(crate) fn new(workbench: WeakEntity<Workbench>, navigation: Arc<Navigation>) -> Self {
        Self {
            workbench,
            navigation,
            ready: None,
            indexing: None,
            query: String::new(),
            shown: None,
            matches: Vec::new(),
            answer: Err(None),
            pending: None,
            querying: None,
            opening: 0,
            cancel: Arc::default(),
            previous_focus: None,
            #[cfg(test)]
            queries: 0,
            #[cfg(test)]
            hold: None,
        }
    }

    /// The picker is opening: `ready` is `Nav::start`'s.
    pub(crate) fn opened(&mut self, ready: Option<Receiver<()>>, previous: Option<FocusHandle>) {
        self.ready = ready;
        self.opening += 1;
        self.previous_focus = previous;
        self.matches.clear();
        self.shown = None;
    }

    /// The root changed: the running query is cancelled and its answer, and
    /// any waiting, dropped. It's for the old root.
    pub(crate) fn reset(&mut self) {
        self.opening += 1;
        self.cancel.store(true, Ordering::Relaxed);
        self.ready = None;
        self.indexing = None;
        self.pending = None;
        self.shown = None;
        self.matches.clear();
        self.answer = Err(None);
    }

    pub(crate) fn indexing(&mut self, indexed: Option<usize>) {
        self.indexing = indexed;
    }

    /// Matches the input's text again, as the index changed.
    pub(crate) fn requery(&mut self, cx: &mut Context<Picker<Self>>) {
        if self.ready.is_some() {
            self.update_matches(self.query.clone(), cx);
        }
        cx.notify();
    }

    #[cfg(test)]
    pub(crate) fn matches(&self) -> &[WorkspaceSymbol] {
        &self.matches
    }

    #[cfg(test)]
    pub(crate) fn status_text(&self) -> String {
        self.status().unwrap_or_default().to_string()
    }

    /// The row for match `ix`'s name, the matched chars highlighted.
    pub(crate) fn label(&self, ix: usize) -> Option<HighlightedLabel> {
        let found = self.matches.get(ix)?;
        let highlights = char_ranges(&found.name, &found.indices);
        // Coloured by the active theme as it draws.
        Some(HighlightedLabel {
            text: found.name.clone().into(),
            highlights,
        })
    }

    /// Runs the waiting query in the background, once the provider is in.
    fn run_query(&mut self, cx: &mut Context<Picker<Self>>) {
        let (Some(query), Some(ready)) = (self.pending.take(), self.ready.clone()) else {
            return;
        };
        #[cfg(test)]
        {
            self.queries += 1;
        }
        let navigation = self.navigation.clone();
        let opening = self.opening;
        self.cancel = Arc::default();
        let cancel = self.cancel.clone();
        #[cfg(test)]
        let hold = self.hold.take();
        self.querying = Some(cx.spawn(async move |picker, cx| {
            ready.recv().await.ok();
            let answer = cx
                .background_spawn({
                    let query = query.clone();
                    async move { navigation.workspace_symbols(&query, LIMIT, &cancel) }
                })
                .await;
            #[cfg(test)]
            if let Some(hold) = hold {
                hold.recv().await.ok();
            }
            picker
                .update(cx, |picker, cx| {
                    let this = &mut picker.delegate;
                    this.querying = None;
                    if this.query == query && this.opening == opening {
                        let new_query = this.shown.as_ref() != Some(&query);
                        this.shown = Some(query);
                        match answer {
                            Ok(answer) => {
                                if let Some(workbench) = this.workbench.upgrade() {
                                    let bar = workbench.read(cx).status_bar().clone();
                                    show_source(&bar, &answer.provider, cx);
                                }
                                this.matches = answer.value;
                                this.answer = Ok((answer.provider, answer.precision));
                            }
                            Err(error) => {
                                this.matches.clear();
                                this.answer = Err(Some(error.to_string()));
                            }
                        }
                        picker.matches_updated(new_query, cx);
                    }
                    picker.delegate.run_query(cx);
                })
                .ok();
        }));
    }
}

impl PickerDelegate for SymbolsPicker {
    fn update_matches(&mut self, query: String, cx: &mut Context<Picker<Self>>) {
        self.query = query.clone();
        if query.is_empty() {
            self.pending = None;
            self.shown = Some(query);
            self.matches.clear();
            return cx.notify();
        }
        self.pending = Some(query);
        if self.querying.is_none() {
            self.run_query(cx);
        }
    }

    fn match_count(&self) -> usize {
        self.matches.len()
    }

    fn match_id(&self, ix: usize) -> SharedString {
        row_id(&self.matches[ix].location)
    }

    fn render_match(&self, ix: usize, _: bool, cx: &App) -> AnyElement {
        let (Some(label), Some(found)) = (self.label(ix), self.matches.get(ix)) else {
            return gpui::Empty.into_any_element();
        };
        row(
            label,
            kind_word(found.kind).to_string(),
            place(&found.location),
            &fe_ui::ui_theme(cx),
        )
    }

    fn confirm(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Picker<Self>>) -> bool {
        if self.shown.as_ref() != Some(&self.query) {
            return false;
        }
        let location = self.matches[ix].location.clone();
        self.previous_focus = None;
        jump(&self.workbench, &location, window, cx)
    }

    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        if let Some(focus) = self.previous_focus.take() {
            window.focus(&focus, cx);
        }
    }

    fn status(&self) -> Option<SharedString> {
        Some(match (&self.ready, self.indexing, &self.answer) {
            (None, ..) => tr!("workbench-navigation-no-folder"),
            (_, Some(indexed), _) => tr!("workbench-navigation-indexing", count = indexed),
            (_, None, Err(None)) => SharedString::default(),
            (_, None, Err(Some(error))) => {
                tr!(
                    "workbench-navigation-symbols-failed",
                    error = error.as_str()
                )
            }
            (_, None, Ok((provider, precision))) => tr!(
                "workbench-navigation-results",
                count = self.matches.len(),
                source = source(*precision),
                provider = provider.as_str()
            ),
        })
    }
}

/// Jumps to `location` from a picker: the workbench isn't being updated
/// then.
fn jump(
    workbench: &WeakEntity<Workbench>,
    location: &Location,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    workbench
        .update(cx, |workbench, cx| workbench.nav_jump(location, window, cx))
        .is_ok()
}

/// A row: the name, then two muted columns.
fn row(name: HighlightedLabel, second: String, third: String, ui: &UiTheme) -> AnyElement {
    div()
        .flex()
        .gap_3()
        .child(name)
        .child(div().text_color(ui.muted_text).child(second))
        .child(div().text_color(ui.muted_text).child(third))
        .into_any_element()
}

/// `rel:row`, the row 1-based.
pub(crate) fn place(location: &Location) -> String {
    format!("{}:{}", location.rel.display(), location.row + 1)
}

/// A row's identity: its file, losslessly (as is if it's UTF-8, else a
/// NUL, which no path holds, then its bytes in hex), and where the name
/// starts.
fn row_id(location: &Location) -> SharedString {
    let path: &Path = &location.rel;
    let mut id = match path.to_str() {
        Some(path) => path.to_owned(),
        None => {
            let mut id = String::from("\0");
            for byte in path.as_os_str().as_encoded_bytes() {
                write!(id, "{byte:02x}").expect("writing to a String");
            }
            id
        }
    };
    write!(id, ":{}", location.range.start).expect("writing to a String");
    id.into()
}

/// A symbol's kind, as a row shows it.
fn kind_word(kind: OutlineKind) -> SharedString {
    // The message's `kind` argument.
    let kind = match kind {
        OutlineKind::Module => "module",
        OutlineKind::Class => "class",
        OutlineKind::Struct => "struct",
        OutlineKind::Enum => "enum",
        OutlineKind::Interface => "interface",
        OutlineKind::Trait => "trait",
        OutlineKind::Impl => "impl",
        OutlineKind::Type => "type",
        OutlineKind::Function => "function",
        OutlineKind::Method => "method",
        OutlineKind::Constant => "constant",
        OutlineKind::Macro => "macro",
        OutlineKind::Table => "table",
        OutlineKind::Heading(_) => "heading",
        OutlineKind::Field => "field",
        OutlineKind::Variant => "variant",
        OutlineKind::Variable => "variable",
        _ => "symbol",
    };
    tr!("workbench-navigation-symbol-kind", kind = kind)
}
