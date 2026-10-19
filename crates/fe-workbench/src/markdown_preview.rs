//! The Markdown preview, as VS Code's built-in one: a read-only tab that
//! draws its source editor's Markdown with fe-markdown-view, re-rendered
//! once edits have been quiet for [`DELAY`].
//!
//! - `markdown.show_preview` opens it in a tab after the source's, focused;
//!   `markdown.show_preview_to_side` in the pane to the right, split off if
//!   there's none, with focus left in the source. A preview of the same
//!   source already there is shown instead of opening a second.
//! - `markdown.toggle_preview`, and the tab bar's button for it, shows it
//!   in the source's own tab, in the editor's place, and the editor again,
//!   as VS Code's `markdown.showPreview` and `markdown.showSource` do in
//!   one editor group. The tab stays the editor's ([`crate::pane::Tab`]):
//!   its title, saves and closes, and the editor, alive and as it was, for
//!   when the source shows again. A saved layout restores it as the
//!   editor.
//! - It follows only its own source, like VS Code's locked preview, and
//!   keeps its last document once the source closes. It has no path, so
//!   opening the file finds its editor, and isn't kept in saved layouts.
//! - It's drawn in the source's font, in the active theme's colours: a
//!   theme reload restyles it, though open editors keep theirs.
//! - A link click opens a file in the preview's pane, at the `#L12` line it
//!   names; a web or mail link in the browser; and an `#anchor` scrolls to
//!   its heading ([`fe_markdown::link_target`]).
//! - It scrolls with its source, both ways, to the same fractional source
//!   line, as VS Code's `scrollPreviewWithEditor` and
//!   `scrollEditorWithPreview` do. The scroll one side makes of the other
//!   emits nothing, so there's no echo, and a re-render puts the preview
//!   back at the source's line once it has laid out. With the source
//!   closed, nothing syncs.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use fe_buffer::Version;
use fe_command::{CommandMeta, ContextPredicate};
use fe_gpui::{ActivePalette, EditorTheme, EditorView, commands::EditorViewEvent};
use fe_markdown::{Document, LinkTarget};
use fe_markdown_view::{MarkdownStyle, MarkdownView, MarkdownViewEvent};
use fe_ui::{ActiveUiTheme, ui_theme};
use gpui::{
    App, AppContext as _, Context, Entity, EntityId, FocusHandle, Focusable, Global, IntoElement,
    Render, SharedString, Subscription, Task, WeakEntity, Window,
};
use kiln_shell::{PaneItem, PaneView as _};

use crate::{
    Direction, Pane, RootChanged, Workbench,
    pane::{Reveal, file_name},
    pane_group::Destination,
};

/// How long edits must be quiet before the preview re-renders: VS Code's
/// `markdown.preview` delay.
pub(crate) const DELAY: Duration = Duration::from_millis(300);

/// The key context of a preview shown in its source's tab, where
/// [`TOGGLE_PREVIEW`] applies as it does in the editor.
pub(crate) const MARKDOWN_PREVIEW_CONTEXT: &str = "markdown_preview";

/// Shows a Markdown tab's preview in its editor's place, or its editor.
pub(crate) const TOGGLE_PREVIEW: &str = "markdown.toggle_preview";

/// The editor's Save, listed by the palette in a preview shown in its
/// editor's place too, where the preview saves the editor's file: no other
/// editor command applies there.
pub(crate) fn palette_commands() -> Vec<CommandMeta> {
    let in_place = ContextPredicate::identifier(MARKDOWN_PREVIEW_CONTEXT);
    (fe_gpui::commands::command_list().into_iter())
        .filter(|meta| meta.id.as_str() == "editor.save")
        .map(|meta| CommandMeta {
            context: Some(in_place.clone()),
            ..meta
        })
        .collect()
}

/// The previews shown in their editors' places, by editor: kept as the
/// editor's tab moves to another pane, and dropped with the editor.
#[derive(Default)]
struct InPlace(HashMap<EntityId, (Entity<MarkdownPreview>, Subscription)>);

impl Global for InPlace {}

/// The preview shown in `editor`'s place, if one is.
pub(crate) fn in_place<'a>(
    editor: &Entity<EditorView>,
    cx: &'a App,
) -> Option<&'a Entity<MarkdownPreview>> {
    let shown = cx.try_global::<InPlace>()?;
    shown.0.get(&editor.entity_id()).map(|(preview, _)| preview)
}

/// Shows `preview` in `editor`'s place, or with `None` the editor.
pub(crate) fn set_in_place(
    editor: &Entity<EditorView>,
    preview: Option<Entity<MarkdownPreview>>,
    cx: &mut App,
) {
    let id = editor.entity_id();
    let entry = preview.map(|preview| {
        let released = cx.observe_release(editor, move |_, cx| {
            if cx.has_global::<InPlace>() {
                cx.global_mut::<InPlace>().0.remove(&id);
            }
        });
        (preview, released)
    });
    let shown = &mut cx.default_global::<InPlace>().0;
    match entry {
        Some(entry) => shown.insert(id, entry),
        None => shown.remove(&id),
    };
}

/// A read-only preview of a Markdown editor's text, as a pane item.
pub(crate) struct MarkdownPreview {
    view: Entity<MarkdownView>,
    source: WeakEntity<EditorView>,
    /// For the links that open files, in the pane the preview is in.
    workbench: WeakEntity<Workbench>,
    /// The workbench's root folder as of its last [`RootChanged`], which
    /// links starting with `/` resolve against.
    root: Option<PathBuf>,
    /// The source's file as of its last change; `None` while untitled.
    path: Option<PathBuf>,
    /// The source's font, metrics and colours as of its last change.
    theme: EditorTheme,
    /// The source buffer's version the document is being parsed for, or
    /// shows once parsed.
    version: Option<Version>,
    /// The source buffer's version the document shows.
    shown: Option<Version>,
    /// The wait for edits to settle and the parse after it.
    render: Task<()>,
    _subscriptions: Vec<Subscription>,
}

/// What a preview starts out showing: its source's text, parsed at once,
/// or a split's copy of another preview.
struct Start {
    source: WeakEntity<EditorView>,
    root: Option<PathBuf>,
    path: Option<PathBuf>,
    theme: EditorTheme,
    document: Arc<Document>,
    /// The source buffer's version `document` is of; `None` to parse.
    shown: Option<Version>,
}

impl MarkdownPreview {
    /// A preview from `start`, following its source while it's open.
    fn new(
        start: Start,
        workbench: &Entity<Workbench>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let Start {
            source,
            root,
            path,
            theme,
            document,
            shown,
        } = start;
        let style = style(&theme, cx);
        let doc_path = doc_path(path.as_deref(), root.as_deref());
        let view = cx.new(|cx| MarkdownView::new(document, doc_path, root.clone(), style, cx));
        let mut subscriptions = vec![
            cx.subscribe_in(&view, window, Self::view_event),
            cx.subscribe(workbench, |this, _, RootChanged(root), cx| {
                this.root = Some(root.clone());
                this.relocate(cx);
            }),
            cx.observe_global::<ActivePalette>(Self::restyle),
            cx.observe_global::<ActiveUiTheme>(Self::restyle),
        ];
        let open = source.upgrade();
        if let Some(open) = &open {
            subscriptions.push(cx.observe(open, Self::source_changed));
            subscriptions.push(cx.subscribe(open, Self::source_event));
        }
        let mut this = Self {
            view,
            source,
            workbench: workbench.downgrade(),
            root,
            path,
            theme,
            version: shown,
            shown,
            render: Task::ready(()),
            _subscriptions: subscriptions,
        };
        if let Some(open) = open {
            this.source_changed(open, cx);
        }
        this
    }

    #[cfg(test)]
    pub(crate) fn view(&self) -> &Entity<MarkdownView> {
        &self.view
    }

    #[cfg(test)]
    pub(crate) fn source(&self) -> &WeakEntity<EditorView> {
        &self.source
    }

    /// Restyles the preview from the source's theme, and re-renders it
    /// [`DELAY`] after the source's buffer last changed, at once the first
    /// time.
    fn source_changed(&mut self, source: Entity<EditorView>, cx: &mut Context<Self>) {
        let editor = source.read(cx);
        let path = editor.model().path().map(Path::to_owned);
        let (version, text) = {
            let buffer = editor.model().buffer();
            (buffer.version(), buffer.text().clone())
        };
        if *editor.theme() != self.theme {
            self.theme = editor.theme().clone();
            self.restyle(cx);
        }
        if path != self.path {
            self.path = path;
            self.relocate(cx);
            cx.notify();
        }
        if self.version == Some(version) {
            return;
        }
        let delay = if self.version.is_some() {
            DELAY
        } else {
            Duration::ZERO
        };
        self.version = Some(version);
        self.render = cx.spawn(async move |this, cx| {
            if !delay.is_zero() {
                cx.background_executor().timer(delay).await;
            }
            let parse = cx.background_spawn(async move { fe_markdown::parse(&text.to_string()) });
            let document = Arc::new(parse.await);
            this.update(cx, |this, cx| {
                if this.version == Some(version) {
                    this.shown = Some(version);
                    (this.view).update(cx, |view, cx| view.set_document(document, cx));
                    this.follow_source(cx);
                }
            })
            .ok();
        });
    }

    /// Styles the preview from [`MarkdownPreview::theme`] and the active
    /// theme ([`style`]).
    fn restyle(&mut self, cx: &mut Context<Self>) {
        let style = style(&self.theme, cx);
        self.view.update(cx, |view, cx| view.set_style(style, cx));
    }

    /// Points the view's links and images at the source's file and the
    /// root folder as they are now.
    fn relocate(&mut self, cx: &mut Context<Self>) {
        let (doc_path, root) = (
            doc_path(self.path.as_deref(), self.root.as_deref()),
            self.root.clone(),
        );
        self.view
            .update(cx, |view, cx| view.set_paths(doc_path, root, cx));
    }

    /// Puts the preview at the source's top line, as of its last frame.
    fn follow_source(&mut self, cx: &mut Context<Self>) {
        let Some(source) = self.source.upgrade() else {
            return;
        };
        if let Some(line) = source.read(cx).top_buffer_line() {
            (self.view).update(cx, |view, cx| view.scroll_to_source_line(line, cx));
        }
    }

    /// Follows the source's scrolling.
    fn source_event(
        &mut self,
        _: Entity<EditorView>,
        event: &EditorViewEvent,
        cx: &mut Context<Self>,
    ) {
        if matches!(event, EditorViewEvent::Scrolled { .. }) {
            self.follow_source(cx);
        }
    }

    /// Follows a clicked link, or the preview's scrolling: see the
    /// module's docs.
    fn view_event(
        &mut self,
        _: &Entity<MarkdownView>,
        event: &MarkdownViewEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let dest = match event {
            MarkdownViewEvent::LinkClicked { dest } => dest,
            MarkdownViewEvent::Scrolled => {
                let line = self.view.read(cx).source_line_at_top();
                if let (Some(line), Some(source)) = (line, self.source.upgrade()) {
                    source.update(cx, |source, cx| source.scroll_to_buffer_line(line, cx));
                }
                return;
            }
        };
        let doc_path = doc_path(self.path.as_deref(), self.root.as_deref());
        match fe_markdown::link_target(dest, &doc_path, self.root.as_deref()) {
            LinkTarget::Anchor(slug) => {
                let found = (self.view).update(cx, |view, cx| view.scroll_to_heading(&slug, cx));
                if !found {
                    log::info!("the preview has no heading #{slug}");
                }
            }
            LinkTarget::Web(url) => cx.open_url(&url),
            LinkTarget::File { path, line } => {
                // Once this update is over: opening reads the panes' items,
                // this among them.
                let (workbench, preview) = (self.workbench.clone(), cx.entity());
                window.defer(cx, move |window, cx| {
                    workbench
                        .update(cx, |workbench, cx| {
                            workbench.open_preview_link(&preview, path, line, window, cx)
                        })
                        .ok();
                });
            }
            LinkTarget::Unsupported => log::info!("the preview doesn't open {dest}"),
        }
    }
}

/// The preview's style: the font and metrics of `editor`, the source's
/// theme, in the editor colours of the active theme where the host has set
/// them ([`fe_gpui::set_editor_palette`]), and the chrome's.
fn style(editor: &EditorTheme, cx: &App) -> MarkdownStyle {
    let ui = ui_theme(cx);
    match cx.try_global::<ActivePalette>() {
        Some(ActivePalette(palette)) => {
            let mut editor = editor.clone();
            editor.set_colors(palette);
            MarkdownStyle::new(&editor, &ui)
        }
        None => MarkdownStyle::new(editor, &ui),
    }
}

/// The file links resolve against: the source's, or an untitled one in
/// the root folder.
fn doc_path(path: Option<&Path>, root: Option<&Path>) -> PathBuf {
    match (path, root) {
        (Some(path), _) => path.to_owned(),
        (None, Some(root)) => root.join("Untitled"),
        (None, None) => PathBuf::from("Untitled"),
    }
}

impl Focusable for MarkdownPreview {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.view.focus_handle(cx)
    }
}

impl Render for MarkdownPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.view.clone()
    }
}

impl PaneItem for MarkdownPreview {
    const KIND: &'static str = "fastedit.markdown_preview";

    /// `Preview README.md`, or `Preview Untitled`.
    fn tab_title(&self, _: &App) -> SharedString {
        match &self.path {
            Some(path) => format!("Preview {}", file_name(Some(path))).into(),
            None => "Preview Untitled".into(),
        }
    }

    fn window_title(&self, cx: &App) -> SharedString {
        format!("{} — fastedit", self.tab_title(cx)).into()
    }

    /// The source's file, to tell previews with the same title apart. The
    /// preview keeps the default `None` [`PaneItem::path`].
    fn tab_path<'a>(&'a self, _: &'a App) -> Option<&'a Path> {
        self.path.as_deref()
    }

    /// A second preview of the same source, or of the same last document
    /// once the source has closed.
    fn split(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<Entity<Self>> {
        let workbench = self.workbench.upgrade()?;
        let start = Start {
            source: self.source.clone(),
            root: self.root.clone(),
            path: self.path.clone(),
            theme: self.theme.clone(),
            document: self.view.read(cx).document().clone(),
            shown: self.shown,
        };
        Some(cx.new(|cx| Self::new(start, &workbench, window, cx)))
    }
}

impl Workbench {
    /// Opens a preview of the active editor's Markdown: after its tab and
    /// focused, or with `to_side` in the pane to its right, split off if
    /// there's none, leaving focus in the editor. A preview of the editor
    /// already in that pane is shown instead. Any other editor is left
    /// alone, with a note in the log.
    pub(crate) fn open_markdown_preview(
        &mut self,
        to_side: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(source) = self.active_editor(cx).cloned() else {
            log::info!("no editor to preview");
            return;
        };
        if !is_markdown(source.read(cx)) {
            log::info!("the active editor isn't Markdown, so has no preview");
            return;
        }
        let pane = self.pane().clone();
        if !to_side {
            if let Some(ix) = preview_of(pane.read(cx), &source, cx) {
                pane.update(cx, |pane, cx| pane.activate(ix, window, cx));
                return;
            }
            let after = (pane.read(cx).tabs().iter())
                .position(|tab| tab.editor() == Some(&source))
                .or(pane.read(cx).active())
                .map(|ix| ix + 1);
            let preview = self.new_preview(&source, window, cx);
            pane.update(cx, |pane, cx| {
                pane.insert_item(Box::new(preview), after, window, cx);
                cx.notify();
            });
            return;
        }
        match self.pane_group(cx).neighbour(&pane, Direction::Right, cx) {
            Some(side) => match preview_of(side.read(cx), &source, cx) {
                Some(ix) => side.update(cx, |side, cx| side.activate(ix, window, cx)),
                None => {
                    let preview = self.new_preview(&source, window, cx);
                    side.update(cx, |side, cx| {
                        side.insert_item(Box::new(preview), None, window, cx);
                        cx.notify();
                    });
                }
            },
            None => {
                // Into the source's pane unshown, then split off with it, as
                // a tab moved right is.
                let preview = self.new_preview(&source, window, cx);
                let id = kiln_shell::ItemId(preview.entity_id());
                pane.update(cx, |pane, cx| {
                    pane.pane_mut().push(Box::new(preview), cx);
                    pane.items_changed(cx);
                });
                let destination = Destination::Split {
                    pane: pane.clone(),
                    direction: Direction::Right,
                };
                self.workspace().clone().update(cx, |workspace, cx| {
                    workspace.move_tab(&pane, id, destination, window, cx)
                });
            }
        }
        self.activate(pane, window, cx);
        window.focus(&source.focus_handle(cx), cx);
    }

    /// Shows the active tab's Markdown preview in its editor's place, or,
    /// if it shows, its editor again: focused either way. Any other tab is
    /// left alone, with a note in the log.
    pub(crate) fn toggle_markdown_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pane = self.pane().clone();
        let Some(ix) = pane.read(cx).active() else {
            log::info!("no tab to preview");
            return;
        };
        if pane.read(cx).tabs()[ix].preview().is_some() {
            pane.update(cx, |pane, cx| pane.show_in_place(ix, None, window, cx));
            return;
        }
        let Some(source) = pane.read(cx).active_editor().cloned() else {
            log::info!("no editor to preview");
            return;
        };
        if !is_markdown(source.read(cx)) {
            log::info!("the active editor isn't Markdown, so has no preview");
            return;
        }
        let preview = self.new_preview(&source, window, cx);
        pane.update(cx, |pane, cx| {
            pane.show_in_place(ix, Some(preview), window, cx)
        });
    }

    fn new_preview(
        &self,
        source: &Entity<EditorView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<MarkdownPreview> {
        let editor = source.read(cx);
        let start = Start {
            source: source.downgrade(),
            root: self.root().map(Path::to_owned),
            path: editor.model().path().map(Path::to_owned),
            theme: editor.theme().clone(),
            document: Arc::new(Document::default()),
            shown: None,
        };
        let workbench = cx.entity();
        cx.new(|cx| MarkdownPreview::new(start, &workbench, window, cx))
    }

    /// Opens `path` in the pane `preview` is in, with the caret at the
    /// start of `line` if there's one, as VS Code's
    /// `markdown.links.openLocation` default does.
    fn open_preview_link(
        &mut self,
        preview: &Entity<MarkdownPreview>,
        path: PathBuf,
        line: Option<u32>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let shows = |pane: &Entity<Pane>, cx: &App| {
            (pane.read(cx).tabs().iter()).any(|tab| {
                tab.item().downcast_ref::<MarkdownPreview>() == Some(preview)
                    || tab.preview() == Some(preview)
            })
        };
        let pane = (self.panes(cx).into_iter())
            .find(|pane| shows(pane, cx))
            .unwrap_or_else(|| self.pane().clone());
        if pane != *self.pane() {
            self.activate(pane.clone(), window, cx);
        }
        let reveal = line.map(|row| Reveal {
            row,
            columns: 0..0,
            within_row: true,
        });
        pane.update(cx, |pane, cx| pane.open_at(path, reveal, window, cx));
    }
}

/// Whether `editor` shows Markdown: by its syntax, or, while its language
/// loads, by its path.
pub(crate) fn is_markdown(editor: &EditorView) -> bool {
    match editor.syntax() {
        Some(syntax) => syntax.language().name() == "markdown",
        None => {
            editor.is_loading_language()
                && (editor.model().path())
                    .and_then(fe_syntax::language_name_for_path)
                    .is_some_and(|name| name == "markdown")
        }
    }
}

/// The index of `pane`'s preview of `source`, if it has one.
fn preview_of(pane: &Pane, source: &Entity<EditorView>, cx: &App) -> Option<usize> {
    pane.tabs().iter().position(|tab| {
        (tab.item().downcast_ref::<MarkdownPreview>())
            .is_some_and(|preview| preview.read(cx).source == source.downgrade())
    })
}
