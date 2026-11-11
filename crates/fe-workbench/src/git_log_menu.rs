//! The Graph's right-click menu and its commands, as VS Code's Source
//! Control Graph and GitLens offer them. A right click on a commit row
//! selects it and offers Open Changes (its change to all its files in one
//! tab, [`commit_diff_view`](super::commit_diff_view)), Copy Commit Hash
//! (its full id) and Copy Subject (its message's first line), each put on
//! the clipboard; nothing that rewrites history. A right click on a file row selects it and offers Open
//! Changes (its change at that commit, as a click on it opens it, kept),
//! Open File at This Revision (the file's text at that commit,
//! [`Repo::text_at`], in a read-only editor tab titled `a.rs @ abc1234`,
//! highlighted in the file's language, a split of it read-only too;
//! asking again shows that tab, and of two opens whose reads land out of
//! order only the later one asked for is shown) and
//! Compare with Working Tree (a diff view of that text, read-only on the
//! left, against the file's working side on the right:
//! [`Workbench::compare_revision`]). A file the commit deleted compares
//! nothing with the working tree, and has no text to open; a file gone from
//! the working tree compares with an empty right side.
//!
//! The menu is drawn where it was asked for, over the window; a click
//! outside it or Escape closes it, as does the list being read again. Its
//! items are the panel's typed commands too (`git_log.*`, in the
//! `git_log_panel` context), so the palette lists them while the panel has
//! focus; from there they act on the selected row: the commit's for a row
//! under one, the file's for a file row (else they do nothing).

use std::{collections::HashMap, path::PathBuf, sync::Arc};

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use fe_git::{CommitInfo, Repo};
use fe_gpui::EditorView;
use fe_ui::{MenuEntry, MenuPanel};
use gpui::{
    AnyElement, App, AppContext as _, ClipboardItem, Context, Entity, EntityId, Focusable as _,
    Global, InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Point, SharedString,
    WeakEntity, Window, anchored, deferred, div,
};
use kiln::Cx;
use kiln_l10n::tr;
use kiln_shell::PaneView as _;

use super::{GitCommand, GitLogPanel, GitOp, Line, Listing};
use crate::{Pane, Workbench};

/// An item of a row's menu, and a command of the panel's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MenuItem {
    CopyCommitHash,
    CopySubject,
    OpenChanges,
    OpenFileAtRevision,
    CompareWithWorkingTree,
}

/// What a commit row's menu offers.
const COMMIT_ITEMS: &[MenuItem] = &[
    MenuItem::OpenChanges,
    MenuItem::CopyCommitHash,
    MenuItem::CopySubject,
];

/// What a file row's menu offers.
const FILE_ITEMS: &[MenuItem] = &[
    MenuItem::OpenChanges,
    MenuItem::OpenFileAtRevision,
    MenuItem::CompareWithWorkingTree,
];

impl MenuItem {
    pub(crate) fn title(self) -> SharedString {
        match self {
            MenuItem::CopyCommitHash => tr!("workbench-git-log-copy-hash"),
            MenuItem::CopySubject => tr!("workbench-git-log-menu-copy-subject"),
            MenuItem::OpenChanges => tr!("workbench-git-log-menu-open-changes"),
            MenuItem::OpenFileAtRevision => tr!("workbench-git-log-menu-open-file"),
            MenuItem::CompareWithWorkingTree => tr!("workbench-git-log-menu-compare"),
        }
    }
}

/// A row's open menu.
pub(super) struct RowMenu {
    /// The row it's for, wherever that is by now.
    line: Line,
    items: &'static [MenuItem],
    /// Where it was asked for, in the window.
    position: Point<Pixels>,
}

#[kiln::command(context = "git_log_panel")]
impl GitLogPanel {
    /// Git: Copy Commit Hash
    ///
    /// Puts the selected commit's full id on the clipboard.
    #[command(id = "git_log.copy_commit_hash")]
    fn copy_commit_hash(&mut self, cx: &mut Cx<Self>) {
        self.run_menu_item(MenuItem::CopyCommitHash, cx.window, cx.cx);
    }

    /// Git: Copy Commit Subject
    ///
    /// Puts the first line of the selected commit's message on the
    /// clipboard.
    #[command(id = "git_log.copy_subject")]
    fn copy_subject(&mut self, cx: &mut Cx<Self>) {
        self.run_menu_item(MenuItem::CopySubject, cx.window, cx.cx);
    }

    /// Git: Open Changes
    ///
    /// Opens the selected commit's change to all its files, or the
    /// selected file's change at its commit.
    #[command(id = "git_log.open_changes")]
    fn open_changes(&mut self, cx: &mut Cx<Self>) {
        self.run_menu_item(MenuItem::OpenChanges, cx.window, cx.cx);
    }

    /// Git: Open File at This Revision
    ///
    /// Opens the selected file as its commit has it, read-only.
    #[command(id = "git_log.open_file_at_revision")]
    fn open_file_at_revision(&mut self, cx: &mut Cx<Self>) {
        self.run_menu_item(MenuItem::OpenFileAtRevision, cx.window, cx.cx);
    }

    /// Git: Compare with Working Tree
    ///
    /// Compares the selected file as its commit has it with the file in
    /// the working tree.
    #[command(id = "git_log.compare_with_working_tree")]
    fn compare_with_working_tree(&mut self, cx: &mut Cx<Self>) {
        self.run_menu_item(MenuItem::CompareWithWorkingTree, cx.window, cx.cx);
    }
}

/// The op the menu's answers are given as: in a toast.
fn clicked() -> GitOp {
    GitOp {
        command: GitCommand::Log,
        args: String::new(),
        range: None,
        editor: None,
        typed: false,
    }
}

/// The name a tab gives `path`.
fn file_name(path: &std::path::Path) -> String {
    (path.file_name()).map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

impl GitLogPanel {
    /// A right click at `position` on `line`, wherever its row is by now:
    /// selects it and opens its menu, if it's a commit's or a file's.
    pub(super) fn open_menu(
        &mut self,
        line: Line,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ix) = self.lines.iter().position(|other| *other == line) else {
            return;
        };
        let items = match line {
            Line::Commit(_) => COMMIT_ITEMS,
            Line::File(..) => FILE_ITEMS,
            Line::Note(_) => return,
        };
        self.select(ix, cx);
        self.menu = Some(RowMenu {
            line,
            items,
            position,
        });
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// Closes the open menu, if there's one.
    pub(super) fn close_menu(&mut self, cx: &mut Context<Self>) {
        if self.menu.take().is_some() {
            cx.notify();
        }
    }

    /// The open menu's items.
    #[cfg(test)]
    pub(crate) fn menu_items(&self) -> Option<&'static [MenuItem]> {
        self.menu.as_ref().map(|menu| menu.items)
    }

    /// Runs `item` for the open menu's row, else the selected one, closing
    /// the menu: see the module docs.
    pub(crate) fn run_menu_item(
        &mut self,
        item: MenuItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let line = match self.menu.take() {
            Some(menu) => Some(menu.line),
            None => self.selected.and_then(|ix| self.lines.get(ix).copied()),
        };
        cx.notify();
        let Some(line) = line else {
            return;
        };
        let Some(entry) = self.commits.get(line.commit()) else {
            return;
        };
        let commit = entry.commit.clone();
        match item {
            MenuItem::CopyCommitHash => {
                cx.write_to_clipboard(ClipboardItem::new_string(commit.id.clone()));
            }
            MenuItem::CopySubject => {
                cx.write_to_clipboard(ClipboardItem::new_string(commit.summary.clone()));
            }
            MenuItem::OpenChanges => match line {
                Line::Commit(ix) => self.open_commit_diff(ix, window, cx),
                Line::File(ix, file) => self.open_file(ix, file, false, window, cx),
                Line::Note(_) => {}
            },
            MenuItem::OpenFileAtRevision | MenuItem::CompareWithWorkingTree => {
                let Line::File(ix, file) = line else {
                    return;
                };
                let Some(Listing::Files(files)) = self.expanded.get(&ix) else {
                    return;
                };
                let (Some(file), Some(repo)) = (files.get(file), self.repo.clone()) else {
                    return;
                };
                let path = file.path.clone();
                let workbench = self.workbench.clone();
                // Once this update is over: the workbench moves focus.
                window.defer(cx, move |window, cx| {
                    (workbench.update(cx, |workbench, cx| match item {
                        MenuItem::OpenFileAtRevision => {
                            workbench.open_revision(repo, commit, path, window, cx)
                        }
                        _ => {
                            let commit = (commit.id.clone(), commit.short.clone());
                            workbench.compare_revision(repo, commit, path, clicked(), window, cx)
                        }
                    }))
                    .ok();
                });
            }
        }
    }

    /// The open menu, drawn over the window where it was asked for.
    pub(super) fn render_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.menu.as_ref()?;
        let items = menu.items;
        let entries = (items.iter())
            .map(|item| MenuEntry::Item {
                title: item.title(),
                shortcut: None,
            })
            .collect();
        let panel = cx.entity().downgrade();
        let ui = fe_ui::ui_theme(cx);
        let menu_panel = MenuPanel::new(entries, &ui).on_click(move |ix, window, cx| {
            let Some(&item) = items.get(ix) else {
                return;
            };
            (panel.update(cx, |panel, cx| panel.run_menu_item(item, window, cx))).ok();
        });
        let wrapper = div()
            .debug_selector(|| "git-log-menu".into())
            .on_mouse_down_out(cx.listener(|panel, _, _, cx| panel.close_menu(cx)))
            .child(menu_panel);
        Some(
            deferred(
                anchored()
                    .position(menu.position)
                    .snap_to_window()
                    .child(wrapper),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }
}

/// The editors opened on a file at a commit, in any window, and each
/// workbench's last open of one asked for.
#[derive(Default)]
struct Revisions {
    /// Each one's full commit id and file.
    opened: Vec<(String, PathBuf, WeakEntity<EditorView>)>,
    /// The number of the last open asked for, by workbench: one window's
    /// don't hold another's back.
    asks: HashMap<EntityId, u64>,
}

impl Global for Revisions {}

/// An open of an editor at a revision asked for: its number, its pane, and
/// the pane's navigations and file opens then. Unless it's still the last
/// asked for in its workbench and its pane, still the active one, has
/// neither moved on as it lands (any tab shown counts as a navigation; a
/// tab added without being shown, as a superseded open's is, doesn't), it
/// waits in its tab rather than be shown.
struct Asked {
    ask: u64,
    pane: WeakEntity<Pane>,
    navigations: u64,
    opens: u64,
}

/// Asks for an open in workbench `me`, superseding any asked for before:
/// its number.
fn ask(me: EntityId, cx: &mut App) -> u64 {
    let asks = cx.default_global::<Revisions>().asks.entry(me).or_default();
    *asks += 1;
    *asks
}

impl Workbench {
    /// The tab open on the file at `path` as commit `id` has it, and its
    /// pane.
    fn revision_tab(
        &self,
        id: &str,
        path: &std::path::Path,
        cx: &mut App,
    ) -> Option<(Entity<Pane>, usize)> {
        let opened = &mut cx.default_global::<Revisions>().opened;
        opened.retain(|(_, _, editor)| editor.upgrade().is_some());
        let editors: Vec<_> = (opened.iter())
            .filter(|(commit, file, _)| commit == id && file == path)
            .filter_map(|(_, _, editor)| editor.upgrade())
            .collect();
        editors.into_iter().find_map(|editor| {
            self.panes(cx).into_iter().find_map(|pane| {
                let tabs = pane.read(cx).tabs();
                let ix =
                    (tabs.iter()).position(|tab| tab.item().downcast_ref() == Some(&editor))?;
                Some((pane.clone(), ix))
            })
        })
    }

    /// Shows tab `ix` of `pane`, focused.
    fn show_revision_tab(
        &mut self,
        pane: Entity<Pane>,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.activate(pane.clone(), window, cx);
        pane.update(cx, |pane, cx| pane.activate(ix, window, cx));
    }

    /// Opens the file at `path` as `commit` has it, read from `repo`, in a
    /// read-only editor tab of the active pane, `a.rs @ abc1234`, or shows
    /// the one open: see the module docs.
    pub(crate) fn open_revision(
        &mut self,
        repo: Arc<Repo>,
        commit: Arc<CommitInfo>,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ask = ask(cx.entity_id(), cx);
        if let Some((pane, ix)) = self.revision_tab(&commit.id, &path, cx) {
            return self.show_revision_tab(pane, ix, window, cx);
        }
        let pane = self.active.clone();
        let asked = {
            let state = pane.read(cx);
            Asked {
                ask,
                pane: pane.downgrade(),
                navigations: state.navigations(),
                opens: state.open_serial(),
            }
        };
        let (id, at) = (commit.id.clone(), path.clone());
        #[cfg(test)]
        let gate = crate::workbench::git_stage::test_gate(cx);
        let read = cx.background_spawn(async move {
            #[cfg(test)]
            gate.await;
            repo.text_at(&id, &at)
        });
        cx.spawn_in(window, async move |workbench, cx| {
            let read = read.await;
            (workbench.update_in(cx, |workbench, window, cx| {
                let text = match read {
                    Ok(Some(text)) => text,
                    Ok(None) => {
                        let answer = tr!(
                            "workbench-git-log-not-in-commit",
                            name = file_name(&path),
                            short = commit.short.as_str()
                        )
                        .to_string();
                        return workbench.answer_git_op(&clicked(), answer, cx);
                    }
                    Err(error) => {
                        return workbench.answer_git_op(&clicked(), error.to_string(), cx);
                    }
                };
                workbench.land_revision(commit, path, text, asked, window, cx);
            }))
            .ok();
        })
        .detach();
    }

    /// Opens the editor [`Workbench::open_revision`] read `text` for, in
    /// the pane it was asked in (else the active one), shown and focused
    /// only if `asked` still stands (see [`Asked`]); one opened meanwhile
    /// is shown instead, if it does.
    fn land_revision(
        &mut self,
        commit: Arc<CommitInfo>,
        path: PathBuf,
        text: String,
        asked: Asked,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pane = (asked.pane.upgrade()).filter(|pane| self.panes(cx).contains(pane));
        let me = cx.entity_id();
        let last =
            (cx.default_global::<Revisions>().asks.get(&me)).is_some_and(|ask| *ask == asked.ask);
        let show = last
            && pane.as_ref().is_some_and(|pane| {
                let state = pane.read(cx);
                *pane == self.active
                    && state.navigations() == asked.navigations
                    && state.open_serial() == asked.opens
            });
        if let Some((pane, ix)) = self.revision_tab(&commit.id, &path, cx) {
            if show {
                self.show_revision_tab(pane, ix, window, cx);
            }
            return;
        }
        let pane = pane.unwrap_or_else(|| self.active.clone());
        let name = file_name(&path);
        let title = tr!(
            "workbench-git-log-revision-title",
            name = name.as_str(),
            short = commit.short.as_str()
        )
        .to_string();
        let buffer = Buffer::from_file_contents(&text);
        let model = EditorModel::new(buffer, None);
        let (editor, ix) = pane.update(cx, |pane, cx| {
            let editor = pane.new_editor(model, cx);
            editor.update(cx, |editor, cx| {
                editor.set_read_only(true, cx);
                editor.set_tab_title(title, name, cx);
                editor.detect_language_from(path.clone(), cx);
            });
            let ix = pane.pane_mut().push(Box::new(editor.clone()), cx);
            pane.items_changed(cx);
            cx.notify();
            (editor, ix)
        });
        let opened = &mut cx.default_global::<Revisions>().opened;
        opened.push((commit.id.clone(), path, editor.downgrade()));
        // Shown as a tab is shown, which counts as a navigation, so an
        // open landing after it doesn't take its place.
        if show {
            self.show_revision_tab(pane, ix, window, cx);
            window.focus(&editor.focus_handle(cx), cx);
        }
    }
}

#[cfg(test)]
#[path = "git_log_menu_tests.rs"]
mod git_log_menu_tests;
