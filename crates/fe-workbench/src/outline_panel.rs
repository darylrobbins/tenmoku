//! The outline, Structure in the sidebar's Explorer, collapsed at the foot
//! of Project: the active editor's symbols, in pre-order and indented by
//! depth, which jump to their name when clicked. Collapsed, its header
//! still shows the symbol under the caret ([`CaretSymbol`]).
//!
//! The workbench says which editor is active ([`OutlinePanel::set_editor`]);
//! the panel observes it, and refreshes when its [`Syntax`] has a tree
//! reparsed at another version than the outline shown. The editor catches
//! its syntax up with its text as it renders, which notifies no one, so when
//! the syntax trailed the text the panel looks again once the frame is drawn
//! ([`OutlinePanel::frame_drawn`]), shown or not.
//!
//! A refresh builds the [`fe_syntax::OutlineJob`] here, cloning the tree and
//! the text, and runs it on the background executor. At most one runs at a
//! time: refreshes meanwhile only update the status, and the panel refreshes
//! again when it lands, which starts the follow-up if the syntax moved on.
//! Until then the last outline of the tab stays on screen, so the panel
//! never blanks between keystrokes. A job's outline shows only if the editor
//! is still of its language: one set to another language and back meanwhile
//! keeps the outline shown.

use kiln_l10n::tr;
use std::{collections::HashMap, ops::Range};

use fe_config::theme::EditorPalette;
use fe_gpui::EditorView;
use fe_syntax::{Language, MAX_OUTLINE_ITEMS, NoOutline, Outline, OutlineKind, Syntax};
use fe_ui::{ActiveTheme as _, TreeRow};
use gpui::{
    AppContext as _, Context, Entity, EntityId, Focusable as _, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, Styled as _, Subscription, Task,
    UniformListScrollHandle, WeakEntity, Window, div, prelude::FluentBuilder as _, px,
    uniform_list,
};

/// The outline's view in the sidebar's Explorer, Structure, under the
/// project tree; `outline.show` shows the Explorer with it expanded.
pub(crate) const VIEW: kiln_shell::ViewId = kiln_shell::ViewId("outline");

/// What the panel says in place of, or under, the outline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    NoFile,
    PlainText,
    /// The language, which has no outline query.
    Unsupported(&'static str),
    TooLarge,
    /// The outline, or the last one while the next is found: "Parsing…"
    /// before the first.
    Outline,
}

pub struct OutlinePanel {
    editor: Option<WeakEntity<EditorView>>,
    status: Status,
    /// The active editor's last outline, and the language it's of.
    outline: Option<(Outline, &'static Language)>,
    /// Moves on with each outline shown and each editor followed, so a
    /// click on a row of another can't land.
    generation: u64,
    /// The outlines the language server gave, by editor: while an editor has
    /// one it's shown in place of tree-sitter's.
    served: HashMap<EntityId, Outline>,
    /// The job in flight.
    job: Option<Task<()>>,
    /// Whether the last refresh found the syntax trailing the text, or not
    /// reparsed: the editor catches up as it renders.
    behind: bool,
    scroll: UniformListScrollHandle,
    /// Focused by `outline.show`, for its keys (`outline_panel_keys.rs`).
    focus: gpui::FocusHandle,
    selection: keys::Selection,
    /// Whether the panel had focus as it last rendered, which outlines the
    /// selected row.
    focused: bool,
    #[cfg(test)]
    counters: TestCounters,
    _observe: Option<Subscription>,
}

impl gpui::Focusable for OutlinePanel {
    fn focus_handle(&self, _: &gpui::App) -> gpui::FocusHandle {
        self.focus.clone()
    }
}

impl OutlinePanel {
    /// A panel following no editor.
    pub fn new(cx: &mut gpui::App) -> Self {
        Self {
            editor: None,
            status: Status::NoFile,
            outline: None,
            generation: 0,
            served: HashMap::new(),
            job: None,
            behind: false,
            scroll: UniformListScrollHandle::new(),
            focus: cx.focus_handle(),
            selection: keys::Selection::default(),
            focused: false,
            #[cfg(test)]
            counters: TestCounters::default(),
            _observe: None,
        }
    }

    /// Follows `editor`, the active tab's, or none. The hook for "the active
    /// editor changed": the workbench calls it as it renders, for its pane;
    /// with splits, for the active pane. Following another editor drops the
    /// outline shown; a job still running for the last one lands, unshown.
    pub fn set_editor(&mut self, editor: Option<&Entity<EditorView>>, cx: &mut Context<Self>) {
        if editor.map(Entity::entity_id) == self.editor_id() {
            return;
        }
        self.editor = editor.map(Entity::downgrade);
        self._observe = editor.map(|editor| {
            cx.observe(editor, |panel, _, cx| {
                panel.refresh(cx);
                cx.notify();
            })
        });
        self.outline = None;
        self.clear_selection();
        self.generation += 1;
        self.scroll.scroll_to_item(0, gpui::ScrollStrategy::Top);
        self.refresh(cx);
        cx.notify();
    }

    /// Shows `outline`, the language server's symbols for `editor`, in place
    /// of the tree-sitter outline; `None` (no provider, an error, an empty
    /// reply, a stopped server, the editor closed) goes back to it.
    pub(crate) fn set_served(
        &mut self,
        editor: EntityId,
        outline: Option<Outline>,
        cx: &mut Context<Self>,
    ) {
        match outline {
            Some(outline) => {
                self.served.insert(editor, outline);
            }
            None if self.served.remove(&editor).is_none() => return,
            None => {}
        }
        if self.editor_id() == Some(editor) {
            self.generation += 1;
            self.refresh(cx);
            cx.notify();
        }
    }

    /// The outline shown, the server's or tree-sitter's.
    fn current(&self) -> Option<&Outline> {
        match self.editor_id().and_then(|editor| self.served.get(&editor)) {
            Some(served) => Some(served),
            None => self.outline.as_ref().map(|(outline, _)| outline),
        }
    }

    fn editor_id(&self) -> Option<EntityId> {
        self.editor.as_ref().map(|editor| editor.entity_id())
    }

    /// Updates the status from the editor's syntax, and starts a job if its
    /// tree is parsed at another version than the outline shown.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.behind = false;
        let Some(editor) = self.editor.as_ref().and_then(WeakEntity::upgrade) else {
            self.status = Status::NoFile;
            return;
        };
        if self.served.contains_key(&editor.entity_id()) {
            self.status = Status::Outline;
            return;
        }
        let view = editor.read(cx);
        let Some(syntax) = view.syntax() else {
            self.status = if view.is_loading_language() {
                Status::Outline
            } else {
                Status::PlainText
            };
            return;
        };
        self.behind = syntax.version() != view.model().snapshot().version();
        let job = match fe_syntax::outline_job(syntax) {
            Ok(job) => job,
            Err(NoOutline::NotParsed) => {
                self.status = Status::Outline;
                self.behind = true;
                return;
            }
            Err(NoOutline::Unsupported) => {
                self.status = Status::Unsupported(syntax.language().name());
                return;
            }
            Err(NoOutline::TooLarge) => {
                self.status = Status::TooLarge;
                return;
            }
        };
        self.status = Status::Outline;
        if self.shows(syntax) {
            return;
        }
        if self.job.is_some() {
            return;
        }
        let language = syntax.language();
        let editor = editor.entity_id();
        #[cfg(test)]
        let (ran, gate) = self.counters.started(cx);
        let found = cx.background_spawn(async move {
            #[cfg(test)]
            {
                gate.await;
                ran();
            }
            job.run()
        });
        self.job = Some(cx.spawn(async move |panel, cx| {
            let outline = found.await;
            panel
                .update(cx, |panel, cx| panel.landed(editor, outline, language, cx))
                .ok();
        }));
    }

    /// Refreshes once a frame is drawn if the last refresh found the editor
    /// behind, as its render may have caught up. The workbench calls it after
    /// every frame.
    pub(crate) fn frame_drawn(&mut self, cx: &mut Context<Self>) {
        if !self.behind {
            return;
        }
        let status = self.status;
        self.refresh(cx);
        if self.status != status {
            cx.notify();
        }
    }

    /// Whether the outline shown is of `syntax`'s language and version.
    fn shows(&self, syntax: &Syntax) -> bool {
        self.outline.as_ref().is_some_and(|(outline, language)| {
            outline.version == syntax.version() && std::ptr::eq(*language, syntax.language())
        })
    }

    /// Shows a job's outline if it's of the editor still followed, in the
    /// language it's still of, then refreshes: the syntax may have moved on
    /// while the job ran, or been set to another language and back.
    fn landed(
        &mut self,
        editor: EntityId,
        outline: Outline,
        language: &'static Language,
        cx: &mut Context<Self>,
    ) {
        self.job = None;
        let current = self
            .editor
            .as_ref()
            .and_then(WeakEntity::upgrade)
            .filter(|current| current.entity_id() == editor)
            .and_then(|current| current.read(cx).syntax().map(Syntax::language));
        if current.is_some_and(|current| std::ptr::eq(current, language)) {
            let old = self.outline.replace((outline, language));
            self.generation += 1;
            self.keep_selection(old.as_ref().map(|(outline, _)| outline), cx);
        }
        self.refresh(cx);
        cx.notify();
    }

    /// The outline for `editor`, if it's the editor followed and can have
    /// one: `Some(None)` until the first lands, else the outline shown with
    /// its generation. `None` for plain text, an unsupported language or a
    /// file too large.
    pub(crate) fn shown_for(&self, editor: EntityId) -> Option<Option<(&Outline, u64)>> {
        if self.editor_id() != Some(editor) || self.status != Status::Outline {
            return None;
        }
        Some(self.current().map(|outline| (outline, self.generation)))
    }

    /// The outline's items, if it's shown.
    fn items(&self) -> &[fe_syntax::OutlineItem] {
        match (&self.status, self.current()) {
            (Status::Outline, Some(outline)) => &outline.items,
            _ => &[],
        }
    }

    /// What shows in place of the items, if anything.
    fn message(&self) -> Option<String> {
        let message = match self.status {
            Status::NoFile => tr!("workbench-project-structure-no-file"),
            Status::PlainText => tr!("workbench-project-structure-plain-text"),
            Status::Unsupported(language) => {
                tr!(
                    "workbench-project-structure-unsupported",
                    language = language
                )
            }
            Status::TooLarge => tr!("workbench-project-structure-too-large"),
            Status::Outline => match self.current() {
                None => tr!("workbench-project-structure-parsing"),
                Some(outline) if outline.items.is_empty() => {
                    tr!("workbench-project-structure-no-symbols")
                }
                Some(_) => return None,
            },
        };
        Some(message.into())
    }

    /// The footer under a capped outline.
    fn footer(&self) -> Option<String> {
        let truncated = self.status == Status::Outline
            && self.current().is_some_and(|outline| outline.truncated);
        truncated.then(|| {
            let (count, shown) = (MAX_OUTLINE_ITEMS, thousands(MAX_OUTLINE_ITEMS));
            tr!(
                "workbench-project-structure-capped",
                count = count,
                shown = shown
            )
            .into()
        })
    }

    /// Selects item `ix`'s name start, if it's still `generation`'s, in the
    /// editor followed, and focuses it. `select_and_reveal` clamps an offset
    /// the buffer has moved past since.
    fn jump(&mut self, generation: u64, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if generation != self.generation {
            return;
        }
        let Some(start) = self.items().get(ix).map(|item| item.name_range.start) else {
            return;
        };
        let Some(editor) = self.editor.as_ref().and_then(WeakEntity::upgrade) else {
            return;
        };
        self.selection.row = Some(ix);
        editor.update(cx, |editor, cx| editor.select_and_reveal(start..start, cx));
        window.focus(&editor.focus_handle(cx), cx);
    }

    pub(crate) fn render_rows(
        &mut self,
        range: Range<usize>,
        cx: &mut Context<Self>,
    ) -> Vec<TreeRow> {
        #[cfg(test)]
        self.counters.built.push(range.clone());
        let generation = self.generation;
        // A symbol's signature or type, dimmed after its name.
        let dim = cx.theme().colors.text.muted;
        let ui = fe_ui::ui_theme(cx);
        let (selected, focused) = (self.selection.row, self.focused);
        let palette = fe_gpui::editor_palette(cx);
        let items = self.items();
        range
            .filter_map(|ix| {
                let item = items.get(ix)?;
                Some(
                    TreeRow::new(("outline-row", ix), item.name.clone(), &ui)
                        .depth(item.depth as usize)
                        .icon(symbol_icon(item.kind))
                        .when_some(symbol_tint(item.kind, &palette), TreeRow::icon_color)
                        .detail(item.detail.clone(), dim)
                        .selected(selected == Some(ix))
                        .focused(focused)
                        .on_click(cx.listener(move |panel, _, window, cx| {
                            panel.jump(generation, ix, window, cx)
                        })),
                )
            })
            .collect()
    }
}

/// The symbol under the active editor's caret, as Structure's header shows
/// it while collapsed: its kind's icon, faint, and its name in its syntax
/// colour. Nothing while no outline shows, the outline trails the text
/// (until the next lands) or the caret is in no symbol.
pub(crate) struct CaretSymbol {
    outline: Entity<OutlinePanel>,
    /// The name the last frame drew.
    #[cfg(test)]
    drawn: Option<String>,
    _outline_changed: Subscription,
}

impl CaretSymbol {
    /// Follows `outline`, which notifies as its editor does, the caret
    /// moving included.
    pub(crate) fn new(outline: &Entity<OutlinePanel>, cx: &mut Context<Self>) -> Self {
        Self {
            outline: outline.clone(),
            #[cfg(test)]
            drawn: None,
            _outline_changed: cx.observe(outline, |_, _, cx| cx.notify()),
        }
    }

    /// The name the last frame drew, if any.
    #[cfg(test)]
    pub(crate) fn drawn(&self) -> Option<String> {
        self.drawn.clone()
    }
}

impl OutlinePanel {
    /// The innermost symbol of the outline shown that holds the editor's
    /// caret, and the editor's colour for its kind. None while the outline
    /// shown is of another version of the text than the editor's: its
    /// ranges, kept on screen until the next one lands, may no longer be
    /// where its symbols are.
    fn caret_symbol(&self, cx: &gpui::App) -> Option<(&fe_syntax::OutlineItem, gpui::Hsla)> {
        let editor = self.editor.as_ref()?.upgrade()?;
        let editor = editor.read(cx);
        let model = editor.model();
        let caret = model.primary_selection().head();
        let version = model.snapshot().version();
        let outline = (self.current())
            .filter(|outline| !self.items().is_empty() && outline.version == version)?;
        let item = outline.items.get(outline.item_at(caret)?)?;
        let color = (fe_syntax::Highlight::from_capture(symbol_highlight(item.kind)))
            .and_then(|highlight| editor.theme().syntax_color(highlight))
            .unwrap_or(cx.theme().colors.text.primary);
        Some((item, color))
    }
}

/// The colour a symbol of `kind`'s icon is drawn in: that of the longest
/// listed prefix of its [`symbol_highlight`] in `palette`'s syntax table,
/// as [`fe_syntax::resolve_theme`] picks a capture's, so `function.method`
/// takes `function`'s colour if the theme lists no method colour. `None`
/// if the theme colours nothing for it: the icon keeps the list's colour.
pub(crate) fn symbol_tint(kind: OutlineKind, palette: &EditorPalette) -> Option<gpui::Hsla> {
    let name = symbol_highlight(kind);
    (palette.syntax.iter())
        .filter(|(prefix, _)| {
            (name.strip_prefix(prefix.as_str()))
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
        })
        .max_by_key(|(prefix, _)| prefix.len())
        .map(|(_, color)| gpui::rgba(color.to_rgba_u32()).into())
}

/// The highlight whose colour a symbol of `kind` is drawn in, as its name
/// is in the code.
pub(crate) fn symbol_highlight(kind: OutlineKind) -> &'static str {
    match kind {
        OutlineKind::Module => "module",
        OutlineKind::Class
        | OutlineKind::Struct
        | OutlineKind::Enum
        | OutlineKind::Interface
        | OutlineKind::Trait
        | OutlineKind::Impl
        | OutlineKind::Type => "type",
        OutlineKind::Function => "function",
        OutlineKind::Method => "function.method",
        OutlineKind::Constant => "constant",
        OutlineKind::Macro => "function.macro",
        OutlineKind::Table | OutlineKind::Field => "property",
        OutlineKind::Heading(_) => "markup.heading",
        OutlineKind::Variant => "constant.enum_member",
        // `OutlineKind` is non-exhaustive.
        _ => "variable",
    }
}

impl Render for CaretSymbol {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let outline = self.outline.read(cx);
        let symbol = outline
            .caret_symbol(cx)
            .map(|(item, color)| (item.name.clone(), item.kind, color));
        #[cfg(test)]
        {
            self.drawn = symbol.as_ref().map(|(name, ..)| name.to_string());
        }
        let faint = cx.theme().colors.text.faint;
        div()
            .debug_selector(|| "structure-caret-symbol".into())
            .min_w_0()
            .flex()
            .items_center()
            .gap_1()
            .when_some(symbol, |row, (name, kind, color)| {
                let icon = gpui::svg()
                    .path(symbol_icon(kind).path())
                    .size(px(14.))
                    .flex_none()
                    .text_color(faint);
                row.child(icon)
                    .child(div().min_w_0().truncate().text_color(color).child(name))
            })
    }
}

/// The icon before a symbol of `kind`; every heading level shares one.
pub(crate) fn symbol_icon(kind: OutlineKind) -> fe_ui::Icon {
    use fe_ui::Icon;
    match kind {
        OutlineKind::Module => Icon::Box,
        OutlineKind::Class => Icon::Component,
        OutlineKind::Struct => Icon::Braces,
        OutlineKind::Enum => Icon::List,
        OutlineKind::Interface => Icon::Plug,
        OutlineKind::Trait => Icon::Puzzle,
        OutlineKind::Impl => Icon::Blocks,
        OutlineKind::Type => Icon::Type,
        OutlineKind::Function => Icon::SquareFunction,
        OutlineKind::Method => Icon::MathFunction,
        OutlineKind::Constant => Icon::Hash,
        OutlineKind::Macro => Icon::Wand,
        OutlineKind::Table => Icon::Table,
        OutlineKind::Heading(_) => Icon::Heading,
        OutlineKind::Field => Icon::KeyRound,
        OutlineKind::Variant => Icon::CircleDot,
        OutlineKind::Variable => Icon::Variable,
        // `OutlineKind` is non-exhaustive.
        _ => Icon::Variable,
    }
}

/// `n` with commas between groups of three digits.
fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (ix, digit) in digits.chars().enumerate() {
        if ix > 0 && (digits.len() - ix).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

impl Render for OutlinePanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(test)]
        self.counters.built.clear();
        self.focused = self.focus.contains_focused(window, cx);
        let ui = fe_ui::ui_theme(cx);
        let count = self.items().len();
        // Each outline's rows have their own IDs, so a click pressed on a
        // row of the last one doesn't land on this one's.
        let list = uniform_list(
            ("outline-rows", self.generation as usize),
            count,
            cx.processor(|panel, range, _, cx| panel.render_rows(range, cx)),
        )
        .track_scroll(&self.scroll)
        .flex_1();
        let muted = |text: String| {
            div()
                .flex_none()
                .px_3()
                .py_2()
                .text_color(ui.muted_text)
                .child(text)
        };
        let panel = div().track_focus(&self.focus);
        <Self as kiln::ViewCommands>::attach(panel, cx)
            .key_context(self.key_context(cx))
            .on_key_down(cx.listener(|panel, event, _, cx| panel.count_key(event, cx)))
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
            .when_some(self.message(), |panel, message| panel.child(muted(message)))
            .child(list)
            .when_some(self.footer(), |panel, footer| panel.child(muted(footer)))
    }
}

#[path = "outline_panel_keys.rs"]
mod keys;

#[cfg(test)]
#[path = "outline_panel_keys_tests.rs"]
mod keys_tests;

/// What the tests count.
#[cfg(test)]
#[derive(Default)]
struct TestCounters {
    /// The row ranges the last frame built.
    built: Vec<Range<usize>>,
    /// Jobs started.
    started: usize,
    /// Jobs that ran, and of them those that ran off the main thread.
    ran: std::sync::Arc<std::sync::Mutex<(usize, usize)>>,
    /// While set, jobs started wait for [`OutlinePanel::release_jobs`]
    /// before they run; the senders are the jobs waiting.
    held: Option<Vec<futures::channel::oneshot::Sender<()>>>,
}

#[cfg(test)]
impl TestCounters {
    /// Counts a job started, and returns what the job calls as it runs.
    /// The job also awaits the returned gate before it runs.
    fn started(
        &mut self,
        cx: &gpui::App,
    ) -> (
        impl FnOnce() + Send + 'static,
        impl Future<Output = ()> + Send + 'static,
    ) {
        self.started += 1;
        let (release, gate) = futures::channel::oneshot::channel();
        match &mut self.held {
            Some(held) => held.push(release),
            None => drop(release.send(())),
        }
        let gate = async move {
            gate.await.ok();
        };
        let ran = self.ran.clone();
        let executor = cx.background_executor().clone();
        let ran = move || {
            let mut ran = ran.lock().unwrap();
            ran.0 += 1;
            ran.1 += usize::from(!executor.is_main_thread());
        };
        (ran, gate)
    }
}

#[cfg(test)]
impl OutlinePanel {
    /// Holds the jobs started from now until [`Self::release_jobs`], so they
    /// don't run however long the executor does.
    pub(crate) fn hold_jobs(&mut self) {
        self.counters.held.get_or_insert_default();
    }

    /// Lets the held jobs, and the ones started from now, run.
    pub(crate) fn release_jobs(&mut self) {
        for release in self.counters.held.take().unwrap_or_default() {
            release.send(()).ok();
        }
    }

    /// The rows shown, as `(depth, name)`.
    pub(crate) fn rows(&self) -> Vec<(u32, String)> {
        self.items()
            .iter()
            .map(|item| (item.depth, item.name.clone()))
            .collect()
    }

    /// The outline shown, if any.
    pub(crate) fn outline(&self) -> Option<&Outline> {
        self.current()
    }

    /// What shows in place of the rows, if anything.
    pub(crate) fn message_text(&self) -> Option<String> {
        self.message()
    }

    pub(crate) fn footer_text(&self) -> Option<String> {
        self.footer()
    }

    /// The row ranges the last frame built.
    pub(crate) fn built(&self) -> Vec<Range<usize>> {
        self.counters.built.clone()
    }

    /// Jobs started.
    pub(crate) fn jobs_started(&self) -> usize {
        self.counters.started
    }

    /// Jobs that ran, and of them those that ran off the main thread.
    pub(crate) fn jobs_ran(&self) -> (usize, usize) {
        *self.counters.ran.lock().unwrap()
    }

    /// Where the list was laid out in the last frame.
    pub(crate) fn list_bounds(&self) -> gpui::Bounds<gpui::Pixels> {
        self.scroll.0.borrow().base_handle.bounds()
    }

    /// Where row `ix` was laid out in the last frame, from the list's bounds,
    /// scroll offset and measured row height.
    pub(crate) fn row_bounds(&self, ix: usize) -> Option<gpui::Bounds<gpui::Pixels>> {
        let count = self.items().len();
        let state = self.scroll.0.borrow();
        let list = state.base_handle.bounds();
        let height = state.last_item_size?.contents.height / count as f32;
        let top = list.origin.y + state.base_handle.offset().y + height * ix as f32;
        Some(gpui::Bounds::new(
            gpui::point(list.origin.x, top),
            gpui::size(list.size.width, height),
        ))
    }
}
