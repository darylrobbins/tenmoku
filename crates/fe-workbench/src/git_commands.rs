//! The git and diff commands, each once: a [`GitCommand`], run as a typed
//! command of the workbench's (a method in `workbench_commands.rs`, with its
//! id and palette title, in [`crate::KEY_CONTEXT`]) through
//! [`Workbench::run_git_command`], and its ex forms in [`EX_FORMS`], typed
//! on the vim line and routed here by fe-vim's host commands. Either way it
//! becomes a [`GitOp`] that [`Workbench::run_git_op`] hands to its
//! feature's module: `git_diff`, `git_stage`, `git_commit`, `git_blame`,
//! `git_branch`, [`git_history`] or [`git_log`], each with a `run` that
//! answers `Not supported yet: <title>` until its card fills it in.
//!
//! The ex forms follow the plugins vim users know them from: fugitive's
//! `:Git` (or `:G`) subcommands, as `:Git commit --amend`, and its `:G…`
//! commands for the file and the repository (`:Gdiffsplit`, `:Gwrite`,
//! `:Gread`, `:Gclog`, which takes a range and any arguments, for
//! [`git_history`] to pick its forms from); gitsigns' `:Gitsigns <action>`
//! for changes and line blame, the only others that take a range
//! (`:'<,'>Gitsigns stage_hunk`); and
//! Vim's own `:diff…` commands, which fe-vim parses itself
//! (`:diffsplit {file}`). A name none of them has is `E492`; a known name
//! with a subcommand, arguments or `!` none of its forms takes is
//! `Not supported: :<line>`.
//!
//! The editors a feature shows outside the tabs (a diff view's sides, the
//! commit box) are registered with [`Workbench::own_editor`]: a host
//! command typed in one (`:w`, `do`) goes to its owner's module through
//! [`owned_host_command`].

use std::ops::Range;

use fe_gpui::{DiffView, EditorView};
use fe_vim::HostCommand;
use gpui::{App, Context, Entity, WeakEntity, Window};

use super::{
    Workbench, git_blame, git_branch, git_commit, git_diff, git_stage,
    toast::{self, Toast},
};

#[path = "commit_rows.rs"]
mod commit_rows;
#[path = "git_history.rs"]
pub(crate) mod git_history;
#[path = "git_log.rs"]
pub(crate) mod git_log;

/// A git or diff command: each is a typed command of the workbench's, a
/// method in `workbench_commands.rs` with its id and title.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GitCommand {
    DiffIndex,
    DiffHead,
    CompareFile,
    NextChange,
    PreviousChange,
    StageFile,
    UnstageFile,
    DiscardFile,
    ResetBuffer,
    StageHunk,
    UnstageHunk,
    RevertHunk,
    Commit,
    CommitAmend,
    ToggleLineBlame,
    BlameLine,
    OpenLineCommit,
    SwitchBranch,
    CreateBranch,
    FileHistory,
    Log,
    LogAll,
}

use GitCommand::*;

/// The module a command runs in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Feature {
    Diff,
    Stage,
    Commit,
    Blame,
    Branch,
    History,
    Log,
}

impl GitCommand {
    /// Its palette title, its method's in `workbench_commands.rs`, which
    /// its `Not supported yet` answer names.
    pub(crate) fn title(self) -> &'static str {
        match self {
            DiffIndex => "Git: Compare with Index",
            DiffHead => "Git: Compare with HEAD",
            CompareFile => "Compare Active File With…",
            NextChange => "Go to Next Change",
            PreviousChange => "Go to Previous Change",
            StageFile => "Git: Stage File",
            UnstageFile => "Git: Unstage File",
            DiscardFile => "Git: Discard File Changes",
            ResetBuffer => "Git: Revert Editor to Index",
            StageHunk => "Git: Stage Change",
            UnstageHunk => "Git: Undo Stage Change",
            RevertHunk => "Git: Revert Change",
            Commit => "Git: Commit Staged",
            CommitAmend => "Git: Commit Staged (Amend)",
            ToggleLineBlame => "Git: Toggle Line Blame",
            BlameLine => "Git: Show Line Blame",
            OpenLineCommit => "Git: Open Line's Commit Change",
            SwitchBranch => "Git: Switch Branch…",
            CreateBranch => "Git: Create Branch…",
            FileHistory => "Git: File History",
            Log => "Git: Log",
            LogAll => "Git: Log (All Branches)",
        }
    }

    pub(crate) fn feature(self) -> Feature {
        match self {
            DiffIndex | DiffHead | CompareFile | NextChange | PreviousChange => Feature::Diff,
            StageFile | UnstageFile | DiscardFile | ResetBuffer | StageHunk | UnstageHunk
            | RevertHunk => Feature::Stage,
            Commit | CommitAmend => Feature::Commit,
            ToggleLineBlame | BlameLine | OpenLineCommit => Feature::Blame,
            SwitchBranch | CreateBranch => Feature::Branch,
            FileHistory => Feature::History,
            Log | LogAll => Feature::Log,
        }
    }
}

/// A run of a [`GitCommand`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GitOp {
    pub(crate) command: GitCommand,
    /// What followed the ex form's words: the branch of `:Git switch -c x`,
    /// the file of `:diffsplit {file}`. Empty from the palette.
    pub(crate) args: String,
    /// The rows the ex line's range named, as fe-vim gives them.
    pub(crate) range: Option<Range<u32>>,
    /// The editor it's for: the one its vim line was typed in, else the
    /// active one, if any.
    pub(crate) editor: Option<Entity<EditorView>>,
    /// Whether it was typed on the vim line, which then shows the answer;
    /// otherwise a toast does.
    pub(crate) typed: bool,
}

/// What a form takes after its words.
#[derive(Clone, Copy)]
enum Takes {
    Nothing,
    /// A name, or nothing.
    MaybeName,
    Name,
    /// Any words, which the feature's module picks its forms from.
    Anything,
}

/// The ex commands' names: fugitive's `:Git` and its short form.
const GIT: &[&str] = &["Git", "G"];
const GDIFFSPLIT: &[&str] = &["Gdiffsplit", "Gvdiffsplit"];
const GITSIGNS: &str = "Gitsigns";
const GCLOG: &str = "Gclog";

/// Each ex form: the command names it's typed with, the words that follow
/// the name, what it takes after them, and the command it runs.
/// `:diffsplit` is fe-vim's own [`HostCommand::DiffSplit`].
const EX_FORMS: &[(&[&str], &[&str], Takes, GitCommand)] = &[
    (GDIFFSPLIT, &[], Takes::Nothing, DiffIndex),
    (GDIFFSPLIT, &["HEAD"], Takes::Nothing, DiffHead),
    (&[GITSIGNS], &["next_hunk"], Takes::Nothing, NextChange),
    (&[GITSIGNS], &["prev_hunk"], Takes::Nothing, PreviousChange),
    (&["Gwrite"], &[], Takes::Nothing, StageFile),
    (&[GITSIGNS], &["stage_buffer"], Takes::Nothing, StageFile),
    (
        &[GITSIGNS],
        &["reset_buffer_index"],
        Takes::Nothing,
        UnstageFile,
    ),
    (&["Gread"], &[], Takes::Nothing, ResetBuffer),
    (&[GITSIGNS], &["reset_buffer"], Takes::Nothing, ResetBuffer),
    (&[GITSIGNS], &["stage_hunk"], Takes::Nothing, StageHunk),
    (
        &[GITSIGNS],
        &["undo_stage_hunk"],
        Takes::Nothing,
        UnstageHunk,
    ),
    (&[GITSIGNS], &["reset_hunk"], Takes::Nothing, RevertHunk),
    (GIT, &["commit"], Takes::Nothing, Commit),
    (GIT, &["commit", "--amend"], Takes::Nothing, CommitAmend),
    (
        &[GITSIGNS],
        &["toggle_current_line_blame"],
        Takes::Nothing,
        ToggleLineBlame,
    ),
    (GIT, &["blame"], Takes::Nothing, ToggleLineBlame),
    (&[GITSIGNS], &["blame_line"], Takes::Nothing, BlameLine),
    (
        &[GITSIGNS],
        &["show_commit"],
        Takes::Nothing,
        OpenLineCommit,
    ),
    (GIT, &["switch"], Takes::MaybeName, SwitchBranch),
    (GIT, &["checkout"], Takes::MaybeName, SwitchBranch),
    (GIT, &["switch", "-c"], Takes::Name, CreateBranch),
    (GIT, &["checkout", "-b"], Takes::Name, CreateBranch),
    (&[GCLOG], &[], Takes::Anything, FileHistory),
];

/// The command `:{name}{bang} {args}` runs, typed with `range`, and what
/// follows its form's words; or the answer: `E492` for a name no form has,
/// `E481` for a range on a form other than `:Gitsigns`' and `:Gclog`, else
/// `Not supported: :<line>`.
pub(crate) fn ex_command(
    name: &str,
    bang: bool,
    range: bool,
    args: &str,
) -> Result<(GitCommand, String), String> {
    let line = format!("{name}{} {args}", if bang { "!" } else { "" });
    let line = line.trim_end();
    let forms: Vec<_> = (EX_FORMS.iter())
        .filter(|(names, ..)| names.contains(&name))
        .collect();
    if forms.is_empty() {
        return Err(format!("E492: Not an editor command: {line}"));
    }
    if range && name != GITSIGNS && name != GCLOG {
        return Err("E481: No range allowed".to_owned());
    }
    let unsupported = || format!("Not supported: :{line}");
    if bang {
        return Err(unsupported());
    }
    let typed: Vec<&str> = args.split_whitespace().collect();
    // The form with the most words, all typed.
    let (_, words, takes, command) = (forms.into_iter())
        .filter(|(_, words, ..)| typed.starts_with(words))
        .max_by_key(|(_, words, ..)| words.len())
        .ok_or_else(unsupported)?;
    let rest = &typed[words.len()..];
    let fits = match takes {
        Takes::Nothing => rest.is_empty(),
        Takes::MaybeName => rest.len() <= 1,
        Takes::Name => rest.len() == 1,
        Takes::Anything => true,
    };
    if !fits {
        return Err(unsupported());
    }
    Ok((*command, rest.join(" ")))
}

/// What shows an editor that isn't a tab's, registered with
/// [`Workbench::own_editor`].
#[derive(Clone, Debug)]
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "the diff view and commit cards register their editors"
    )
)]
pub(crate) enum Owner {
    /// A side of this diff view.
    DiffView(WeakEntity<DiffView>),
    /// The commit message box.
    CommitBox,
}

/// Where a git op or an owned editor's host command went, for the tests.
#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Routed {
    Op(GitOp),
    DiffView(HostCommand),
    CommitBox(HostCommand),
}

impl Workbench {
    /// Runs `command` from the palette or a key, for the active editor.
    pub(crate) fn run_git_command(
        &mut self,
        command: GitCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let op = GitOp {
            command,
            args: String::new(),
            range: None,
            editor: self.active_editor(cx).cloned(),
            typed: false,
        };
        self.run_git_op(op, window, cx);
    }

    /// Hands `op` to its feature's module.
    pub(super) fn run_git_op(&mut self, op: GitOp, window: &mut Window, cx: &mut Context<Self>) {
        #[cfg(test)]
        self.git_routed.push(Routed::Op(op.clone()));
        match op.command.feature() {
            Feature::Diff => git_diff::run(self, op, window, cx),
            Feature::Stage => git_stage::run(self, op, window, cx),
            Feature::Commit => git_commit::run(self, op, window, cx),
            Feature::Blame => git_blame::run(self, op, window, cx),
            Feature::Branch => git_branch::run(self, op, window, cx),
            Feature::History => git_history::run(self, op, window, cx),
            Feature::Log => git_log::run(self, op, window, cx),
        }
    }

    /// Answers `op` with `text`: on the vim line it was typed on, else in a
    /// toast.
    pub(super) fn answer_git_op(&self, op: &GitOp, text: String, cx: &mut App) {
        match &op.editor {
            Some(editor) if op.typed => {
                editor.update(cx, |editor, cx| editor.set_vim_message(text, cx));
            }
            _ => toast::show(cx, Toast::info(text)),
        }
    }

    /// Has `owner` take the host commands typed in `editor`, which isn't a
    /// tab's: see [`owned_host_command`].
    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "the diff view and commit cards register their editors"
        )
    )]
    pub(super) fn own_editor(&mut self, editor: &Entity<EditorView>, owner: Owner) {
        self.owned_editors
            .retain(|(owned, _)| owned.upgrade().is_some_and(|owned| owned != *editor));
        self.owned_editors.push((editor.downgrade(), owner));
    }
}

/// Runs `command`, typed in `editor`, which isn't a tab's, in the module of
/// the owner [`Workbench::own_editor`] registered; an editor no one owns
/// runs nothing.
pub(super) fn owned_host_command(
    workbench: &mut Workbench,
    editor: Entity<EditorView>,
    command: HostCommand,
    window: &mut Window,
    cx: &mut Context<Workbench>,
) {
    workbench
        .owned_editors
        .retain(|(owned, _)| owned.upgrade().is_some());
    let owner = (workbench.owned_editors.iter())
        .find(|(owned, _)| *owned == editor.downgrade())
        .map(|(_, owner)| owner.clone());
    match owner {
        Some(Owner::DiffView(diff)) => {
            #[cfg(test)]
            workbench.git_routed.push(Routed::DiffView(command.clone()));
            git_diff::host_command(workbench, diff, editor, command, window, cx);
        }
        Some(Owner::CommitBox) => {
            #[cfg(test)]
            workbench
                .git_routed
                .push(Routed::CommitBox(command.clone()));
            git_commit::host_command(workbench, editor, command, window, cx);
        }
        None => {}
    }
}

#[cfg(test)]
#[path = "git_commands_tests.rs"]
mod git_commands_tests;
