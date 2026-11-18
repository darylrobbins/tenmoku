//! The What's New tab: the curated release notes built into the app, drawn
//! read-only with fe-markdown-view, newest first.
//!
//! The notes are one file per release at the repository root,
//! `release-notes/<X.Y.Z>.md`, the version exactly as
//! `[workspace.package] version` writes it; `build.rs` embeds them all
//! (`RELEASE_NOTES`) and fails the build on any other name. The same file
//! feeds the GitHub release's body and the Sparkle appcast, so it keeps to
//! these rules (`tests::whats_new` checks them):
//!
//! - No H1: the tab adds the `<app> <version>` heading.
//! - Only `## New`, `## Improved` and `## Fixed` sections, in that order,
//!   each optional but at least one, each a bullet list of short lines a
//!   user would notice: no internal work (flakes, test seams, tooling).
//! - No images, no raw HTML, and no `]]>` (the appcast wraps the text in
//!   CDATA).
//! - English only: the notes aren't localized; the tab's own strings are.
//!
//! The tab shows the versions up to the running one: a file above it, a
//! draft of the next release, is left out, and a running version with no
//! file (a dev build after a bump) gets a one-line note above the earlier
//! ones. `app.show_whats_new` opens it ([`Workbench::open_whats_new`]); the
//! launch's first window opens it once per upgrade
//! ([`crate::offer_whats_new`]), and records the version once it has drawn.
//! A web link opens in the browser; other links do nothing. It isn't kept
//! in saved layouts.

use std::{path::PathBuf, sync::Arc};

use fe_config::whats_new::parse_version;
use fe_gpui::{ActivePalette, EditorTheme};
use fe_markdown::LinkTarget;
use fe_markdown_view::{MarkdownStyle, MarkdownView, MarkdownViewEvent};
use fe_ui::{ActiveUiTheme, Icon, ui_theme};
use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, IntoElement, Render,
    SharedString, Subscription, WeakEntity, Window,
};
use kiln_l10n::tr;
use kiln_shell::{PaneItem, PaneView as _};

use crate::Workbench;

include!(concat!(env!("OUT_DIR"), "/release_notes.rs"));

/// The notes and the running version tests see, in place of
/// [`RELEASE_NOTES`] and the crate's version.
#[cfg(test)]
pub(crate) struct NotesSeam {
    /// Each version and its notes, in any order.
    pub(crate) notes: Vec<(String, String)>,
    pub(crate) running: String,
}

#[cfg(test)]
impl gpui::Global for NotesSeam {}

/// The version running: the crate's, or in tests the seam's.
pub(crate) fn running_version(cx: &App) -> String {
    #[cfg(test)]
    if let Some(seam) = cx.try_global::<NotesSeam>() {
        return seam.running.clone();
    }
    let _ = cx;
    env!("CARGO_PKG_VERSION").to_owned()
}

/// Every release's notes, by version: the embedded ones, or the seam's.
fn release_notes(cx: &App) -> Vec<(String, String)> {
    #[cfg(test)]
    if let Some(seam) = cx.try_global::<NotesSeam>() {
        return seam.notes.clone();
    }
    let _ = cx;
    (RELEASE_NOTES.iter())
        .map(|(version, text)| ((*version).to_owned(), (*text).to_owned()))
        .collect()
}

/// The tab's Markdown: each release up to `running`, newest first, under its
/// `<app> <version>` heading; `running` with no notes gets its heading and
/// the no-notes line first.
pub(crate) fn notes_markdown(notes: &[(String, String)], running: &str) -> String {
    let limit = parse_version(running);
    let mut shown: Vec<_> = (notes.iter())
        .filter_map(|(version, text)| Some((parse_version(version)?, version, text)))
        .filter(|(numbers, _, _)| limit.is_none_or(|limit| *numbers <= limit))
        .collect();
    shown.sort_by_key(|shown| std::cmp::Reverse(shown.0));
    let heading = |version: &str| {
        let text = tr!(
            "workbench-workbench-whats-new-heading",
            app = fe_config::APP_NAME,
            version = version
        );
        format!("# {}\n\n", escape(&text))
    };
    let mut markdown = String::new();
    if !shown.iter().any(|(_, version, _)| *version == running) {
        markdown.push_str(&heading(running));
        let none = tr!("workbench-workbench-whats-new-none");
        markdown.push_str(&format!("{}\n\n", escape(&none)));
    }
    for (_, version, text) in shown {
        markdown.push_str(&heading(version));
        markdown.push_str(text.trim());
        markdown.push_str("\n\n");
    }
    markdown
}

/// `text` as Markdown that reads as itself: every ASCII punctuation mark
/// backslash-escaped, as CommonMark allows.
fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch.is_ascii_punctuation() {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped
}

/// The What's New tab, as a pane item.
pub(crate) struct WhatsNew {
    view: Entity<MarkdownView>,
    /// For the editor's font, which the notes are drawn in.
    workbench: WeakEntity<Workbench>,
    /// The state directory and the version to record there once the tab
    /// has drawn: set by the launch's offer.
    record: Option<(PathBuf, String)>,
    _subscriptions: Vec<Subscription>,
}

impl WhatsNew {
    /// The tab, drawn in `editor`'s font: the workbench's, which is being
    /// updated, so can't be read here.
    fn new(
        workbench: &Entity<Workbench>,
        editor: EditorTheme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let markdown = notes_markdown(&release_notes(cx), &running_version(cx));
        let document = Arc::new(fe_markdown::parse(&markdown));
        let style = style(editor, cx);
        let view = cx.new(|cx| MarkdownView::new(document, PathBuf::new(), None, style, cx));
        let subscriptions = vec![
            cx.subscribe_in(&view, window, Self::view_event),
            cx.observe_global::<ActivePalette>(Self::restyle),
            cx.observe_global::<ActiveUiTheme>(Self::restyle),
        ];
        Self {
            view,
            workbench: workbench.downgrade(),
            record: None,
            _subscriptions: subscriptions,
        }
    }

    #[cfg(test)]
    pub(crate) fn document(&self, cx: &App) -> Arc<fe_markdown::Document> {
        self.view.read(cx).document().clone()
    }

    fn restyle(&mut self, cx: &mut Context<Self>) {
        let Some(workbench) = self.workbench.upgrade() else {
            return;
        };
        let editor = workbench.read(cx).pane().read(cx).theme().clone();
        let style = style(editor, cx);
        self.view.update(cx, |view, cx| view.set_style(style, cx));
    }

    /// A web link opens in the browser; any other does nothing.
    fn view_event(
        &mut self,
        _: &Entity<MarkdownView>,
        event: &MarkdownViewEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let MarkdownViewEvent::LinkClicked { dest } = event else {
            return;
        };
        match fe_markdown::link_target(dest, &PathBuf::new(), None) {
            LinkTarget::Web(url) => cx.open_url(&url),
            _ => log::info!("What's New doesn't open {dest}"),
        }
    }
}

/// The notes' style, as the Markdown preview's: the editor's font and
/// metrics, in the active theme's editor colours where the host has set
/// them, and the chrome's.
fn style(mut editor: EditorTheme, cx: &App) -> MarkdownStyle {
    let ui = ui_theme(cx);
    if let Some(ActivePalette(palette)) = cx.try_global::<ActivePalette>() {
        editor.set_colors(palette);
    }
    MarkdownStyle::new(&editor, &ui)
}

impl Focusable for WhatsNew {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.view.focus_handle(cx)
    }
}

impl Render for WhatsNew {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Drawn: the launch's offer has been seen.
        if let Some((state, version)) = self.record.take() {
            cx.background_spawn(async move {
                if let Err(error) = fe_config::whats_new::record(&state, &version) {
                    log::warn!("can't record What's New as seen: {error}");
                }
            })
            .detach();
        }
        self.view.clone()
    }
}

impl PaneItem for WhatsNew {
    const KIND: &'static str = "fastedit.whats_new";

    fn tab_title(&self, _: &App) -> SharedString {
        tr!("workbench-workbench-whats-new-title")
    }

    fn tab_icon(&self, _: &App) -> Option<Icon> {
        Some(Icon::ScrollText)
    }

    fn window_title(&self, cx: &App) -> SharedString {
        format!("{} — {}", self.tab_title(cx), fe_config::APP_NAME).into()
    }
}

impl Workbench {
    /// Shows the What's New tab: the one open in any pane, activating that
    /// pane, or else a new one in the active pane, focused either way.
    pub(crate) fn open_whats_new(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<WhatsNew> {
        for pane in self.panes(cx) {
            let open = (pane.read(cx).tabs().iter())
                .enumerate()
                .find_map(|(ix, tab)| {
                    let item = tab.item().downcast_ref::<WhatsNew>()?;
                    Some((ix, item.clone()))
                });
            if let Some((ix, item)) = open {
                pane.update(cx, |pane, cx| pane.activate(ix, window, cx));
                self.activate(pane, window, cx);
                return item;
            }
        }
        let workbench = cx.entity();
        let editor = self.pane().read(cx).theme().clone();
        let item = cx.new(|cx| WhatsNew::new(&workbench, editor, window, cx));
        self.pane().clone().update(cx, |pane, cx| {
            pane.insert_item(Box::new(item.clone()), None, window, cx);
            cx.notify();
        });
        item
    }

    /// The launch's offer: opens the tab, and records `version` as seen in
    /// `state` once it has drawn. A window that closes first records
    /// nothing, so the next launch offers it again.
    pub(crate) fn show_whats_new_once(
        &mut self,
        state: PathBuf,
        version: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let item = self.open_whats_new(window, cx);
        item.update(cx, |item, cx| {
            item.record = Some((state, version));
            cx.notify();
        });
    }
}
