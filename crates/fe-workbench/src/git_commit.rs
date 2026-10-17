//! Committing the staged changes from the Changes panel's message box (see
//! `git_panel`): `git.commit` and `git.commit_amend`, routed here by
//! `git_commands`, the box's button and `git.commit_box.commit`, and the
//! host commands typed in the box.
//!
//! The workbench makes the box when the panel's first drawn, or a commit
//! command needs it: an editor over a path-less buffer in the editors'
//! theme, soft-wrapped, registered with [`Workbench::own_editor`] so its
//! host commands come here. `git.commit` (`:Git commit`, `cc` in the panel)
//! shows the panel and focuses the box; `git.commit_amend` (`:Git commit
//! --amend`, `ca`) does so with Amend checked. Where the panel has found
//! no repository they answer so.
//!
//! A commit runs [`Repo::commit`] on the background executor, in the
//! repository containing the root, in its turn after the staging asked
//! before it (`git_stage`'s queue), the button saying "Committing…". When
//! it's made, the box empties, Amend is unchecked and the workbench's git
//! state is read again. With nothing staged it asks, as VS Code's smart
//! commit does, "There are no staged changes to commit.", and "Stage All
//! and Commit" stages every change and commits. A hook that fails is a
//! warning naming it with what it printed, the message staying in the box;
//! git failing otherwise is "Git: commit failed" with its output, and an
//! empty message turns the box's placeholder to a warning.
//!
//! In the box, vim's `:wq`, `:x` and `ZZ` commit; `:w` keeps the message
//! (`:w {path}` and `:sav` aren't supported);
//! `:q!` and `ZQ` empty the box, as fugitive's abort does, and focus the
//! active editor again; `:q` focuses it if the box is empty, and answers
//! `E37` if not. `:Git …` runs as in any editor; `:e`, `:wa` and `:qa` run
//! in the active editor.

use std::path::PathBuf;

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use fe_git::{CommitError, GitError, Repo};
use fe_gpui::EditorView;
use fe_vim::HostCommand;
use gpui::{
    App, AppContext as _, Context, Entity, Focusable as _, PromptButton, PromptLevel, Window,
};

use super::{
    Workbench,
    git_commands::{self, GitCommand, GitOp, Owner},
    git_stage,
};
use crate::{GitPanel, dock::DockOp, git::GitState, git_panel::CommitRequest};

/// What `:w` in the box answers.
const KEPT: &str = "Message kept; :wq commits";

/// What `:q` on a box with a message answers.
const E37: &str = "E37: No write since last change (add ! to override)";

/// What a diff command typed in the box answers.
const E99: &str = "E99: Current buffer is not in diff mode";

/// What a command the box has no use for answers.
const NOT_IN_THE_BOX: &str = "Not supported in the commit box";

/// What committing outside a repository says.
const NO_REPO: &str = "Not in a git repository";

/// The question when nothing is staged, and its detail: VS Code's.
const NOTHING_STAGED: &str = "There are no staged changes to commit.";
const STAGE_ALL_DETAIL: &str = "Would you like to stage all your changes and commit them directly?";

/// The button that stages everything and commits.
pub(crate) const STAGE_ALL_AND_COMMIT: &str = "Stage All and Commit";

/// The most of a failed hook's output its warning shows.
const HOOK_OUTPUT: usize = 2000;

/// Runs `op`, a commit command: shows the Changes panel and focuses the
/// box, with Amend checked for `git.commit_amend`. Outside a repository it
/// answers so.
pub(super) fn run(
    workbench: &mut Workbench,
    op: GitOp,
    window: &mut Window,
    cx: &mut Context<Workbench>,
) {
    let changes = workbench.dock.read(cx).changes().clone();
    if changes.read(cx).not_a_repository() {
        return workbench.answer_git_op(&op, NO_REPO.to_owned(), cx);
    }
    let amend = op.command == GitCommand::CommitAmend;
    workbench.focus_commit_box(amend, window, cx);
}

/// Runs `command`, typed in `editor`, the commit box.
pub(super) fn host_command(
    workbench: &mut Workbench,
    editor: Entity<EditorView>,
    command: HostCommand,
    window: &mut Window,
    cx: &mut Context<Workbench>,
) {
    let message = |editor: &Entity<EditorView>, text: &str, cx: &mut Context<Workbench>| {
        editor.update(cx, |editor, cx| editor.set_vim_message(text, cx))
    };
    match command {
        HostCommand::Write { path: None, .. } => message(&editor, KEPT, cx),
        // The box has no file to save as, nor a copy to write.
        HostCommand::Write { path: Some(_), .. } | HostCommand::SaveAs { .. } => {
            message(&editor, NOT_IN_THE_BOX, cx)
        }
        HostCommand::WriteQuit { .. } => workbench.commit_from_box(window, cx),
        HostCommand::Quit { force } => {
            let blank = editor.read(cx).model().text().to_string().trim().is_empty();
            if !force && !blank {
                return message(&editor, E37, cx);
            }
            let changes = workbench.dock.read(cx).changes().clone();
            changes.update(cx, |changes, cx| {
                if force {
                    changes.clear_box(cx);
                }
                changes.leave_box();
            });
            window.focus(&workbench.focus_handle(cx), cx);
        }
        HostCommand::User {
            name,
            bang,
            range,
            args,
        } => match git_commands::ex_command(&name, bang, range.is_some(), &args) {
            Ok((command, args)) => {
                let op = GitOp {
                    command,
                    args,
                    range,
                    editor: Some(editor),
                    typed: true,
                };
                workbench.run_git_op(op, window, cx);
            }
            Err(answer) => message(&editor, &answer, cx),
        },
        HostCommand::DiffGet { .. } | HostCommand::DiffPut { .. } => message(&editor, E99, cx),
        HostCommand::Edit { .. } | HostCommand::WriteAll | HostCommand::QuitAll { .. } => {
            match workbench.active_editor(cx).cloned() {
                Some(active) => {
                    let changes = workbench.dock.read(cx).changes().clone();
                    changes.update(cx, |changes, _| changes.leave_box());
                    workbench.run_vim_host_command(&active.downgrade(), command, window, cx)
                }
                None => message(&editor, NOT_IN_THE_BOX, cx),
            }
        }
        HostCommand::DiffSplit { .. } => message(&editor, NOT_IN_THE_BOX, cx),
        HostCommand::Explore
        | HostCommand::OldFiles
        | HostCommand::OpenProblems
        | HostCommand::CloseProblems
        | HostCommand::ReplaceInFiles { .. } => {
            workbench.run_window_command(&editor, command, window, cx)
        }
    }
}

impl Workbench {
    /// Gives `changes`, the Changes panel, its commit box, unless it has
    /// one: an editor over a path-less buffer, soft-wrapped, with no
    /// gutter, in the editors' theme, whose host commands come here.
    pub(crate) fn attach_commit_box(
        &mut self,
        changes: &Entity<GitPanel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if changes.read(cx).commit_box().is_some() {
            return;
        }
        let theme = self.active.read(cx).theme().clone();
        let editor = cx.new(|cx| {
            let mut model = EditorModel::new(Buffer::new(""), None);
            model.set_soft_wrap(true);
            let mut view = EditorView::new(model, theme, cx);
            view.set_gutter(false, cx);
            view
        });
        self.own_editor(&editor, Owner::CommitBox);
        changes.update(cx, |changes, cx| changes.set_commit_box(editor, window, cx));
    }

    /// Shows the Changes panel and focuses its commit box, with `amend`
    /// Amend checked.
    pub(crate) fn focus_commit_box(
        &mut self,
        amend: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_dock_op(DockOp::ShowChanges, window, cx);
        let changes = self.dock.read(cx).changes().clone();
        self.attach_commit_box(&changes, window, cx);
        changes.update(cx, |changes, cx| {
            if amend {
                changes.set_amend(true, window, cx);
            }
            changes.focus_commit_box(window, cx);
        });
    }

    /// Runs `command`, which the commit box's vim left, as any editor's
    /// host commands run: here, as the box's owner.
    pub(crate) fn commit_box_host_command(
        &mut self,
        editor: Entity<EditorView>,
        command: HostCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_vim_host_command(&editor.downgrade(), command, window, cx);
    }

    /// Commits the commit box's message in the background, unless a commit
    /// is running; then shows how it went.
    pub(crate) fn commit_from_box(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let changes = self.dock.read(cx).changes().clone();
        let Some(request) = changes.update(cx, |changes, cx| changes.begin_commit(cx)) else {
            return;
        };
        self.run_commit(changes, request, false, window, cx);
    }

    /// Commits `request`, from `changes`' box, in the background, first
    /// staging every change if `stage_all`, in its turn among the work that
    /// changes a repository (`git_stage`), so it commits the index the
    /// staging asked before it leaves; then shows how it went.
    fn run_commit(
        &mut self,
        changes: Entity<GitPanel>,
        request: CommitRequest,
        stage_all: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let workbench = cx.weak_entity();
        let window = window.window_handle();
        git_stage::mutate(cx, move |cx| {
            #[cfg(test)]
            let gate = git_stage::test_gate(cx);
            let committed = cx.background_spawn(async move {
                #[cfg(test)]
                gate.await;
                let committed = commit(&request, stage_all);
                (request, committed)
            });
            cx.spawn(async move |cx| {
                let (request, committed) = committed.await;
                (cx.update_window(window, |_, window, cx| {
                    (workbench.update(cx, |workbench, cx| {
                        workbench.committed(changes, request, committed, window, cx)
                    }))
                    .ok();
                }))
                .ok();
            })
        });
    }

    /// Whether `changes` is still the Changes panel: not once the root has
    /// changed, which makes another.
    fn is_changes(&self, changes: &Entity<GitPanel>, cx: &App) -> bool {
        self.dock.read(cx).changes() == changes
    }

    /// `request`, a commit from `changes`' box, has landed: the box
    /// empties once it's made; otherwise the message stays and the failure
    /// shows. Either way the git state is read again, a hook having maybe
    /// staged files. Once the root has changed, nothing shows.
    fn committed(
        &mut self,
        changes: Entity<GitPanel>,
        request: CommitRequest,
        committed: Result<String, CommitError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let empty = matches!(committed, Err(CommitError::EmptyMessage));
        changes.update(cx, |changes, cx| match committed {
            Ok(_) => changes.committed(cx),
            Err(_) => changes.commit_refused(empty, cx),
        });
        self.git.update(cx, GitState::refresh);
        let Err(error) = committed else {
            return;
        };
        if !self.is_changes(&changes, cx) {
            return;
        }
        let (level, message, detail) = match error {
            CommitError::EmptyMessage => return,
            CommitError::NothingStaged => {
                return self.ask_to_stage_all(changes, request, window, cx);
            }
            CommitError::HookFailed { hook, output } => (
                PromptLevel::Warning,
                format!("Git: {hook} hook failed"),
                truncated(&output, HOOK_OUTPUT),
            ),
            CommitError::Failed { output, .. } => (
                PromptLevel::Critical,
                "Git: commit failed".to_owned(),
                output,
            ),
            CommitError::Git(error) => (
                PromptLevel::Critical,
                "Git: commit failed".to_owned(),
                error.to_string(),
            ),
        };
        let detail = Some(detail.as_str()).filter(|detail| !detail.is_empty());
        drop(window.prompt(level, &message, detail, &[PromptButton::ok("OK")], cx));
    }

    /// Asks whether to stage every change and commit `request`, refused
    /// from `changes`' box as nothing is staged, and does if so: the
    /// message and Amend as submitted, in its repository, unless the root
    /// has changed or another commit is running by then.
    fn ask_to_stage_all(
        &mut self,
        changes: Entity<GitPanel>,
        request: CommitRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let buttons = [
            PromptButton::ok(STAGE_ALL_AND_COMMIT),
            PromptButton::cancel("Cancel"),
        ];
        let answer = window.prompt(
            PromptLevel::Warning,
            NOTHING_STAGED,
            Some(STAGE_ALL_DETAIL),
            &buttons,
            cx,
        );
        cx.spawn_in(window, async move |workbench, cx| {
            if answer.await != Ok(0) {
                return;
            }
            (workbench.update_in(cx, |workbench, window, cx| {
                let resumed = workbench.is_changes(&changes, cx)
                    && changes.update(cx, |changes, cx| changes.resume_commit(&request, cx));
                if resumed {
                    workbench.run_commit(changes, request, true, window, cx);
                }
            }))
            .ok();
        })
        .detach();
    }
}

/// Commits `request`'s message in the repository containing its root,
/// first staging every change if `stage_all`. Runs on the background
/// executor.
fn commit(request: &CommitRequest, stage_all: bool) -> Result<String, CommitError> {
    let CommitRequest {
        root,
        message,
        amend,
        ..
    } = request;
    let repo = Repo::discover(root)?.ok_or_else(|| GitError::Io(NO_REPO.to_owned()))?;
    if stage_all {
        let changed: Vec<PathBuf> = (repo.status_split()?.into_iter())
            .filter(|status| status.unstaged.is_some())
            .map(|status| status.path)
            .collect();
        if !changed.is_empty() {
            repo.stage(&changed)?;
        }
    }
    repo.commit(message, *amend)
}

/// The first `most` characters of `text`, then `…` if there were more.
fn truncated(text: &str, most: usize) -> String {
    match text.char_indices().nth(most) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text.to_owned(),
    }
}

#[cfg(test)]
#[path = "git_commit_tests.rs"]
mod git_commit_tests;
