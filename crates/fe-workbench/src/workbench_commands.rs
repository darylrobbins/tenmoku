//! The workbench's typed commands: one `#[kiln::command]` block on
//! [`Workbench`], in its key context, whose handlers [`Workbench`]'s root
//! element attaches. Each method keeps its command's ID, title and context
//! from the `fe-command` registry it moved from, so keymaps and menus still
//! name it.

use fe_command::ContextPredicate;
use fe_lsp::code_action::CodeActionQuery;
use fe_nav::Related;
use gpui::{App, Context, Window};
use kiln::Cx;

use kiln_shell::DockPosition;

use crate::{
    Axis, Direction, Pane, ReferencesPanel, Workbench,
    ai_changes_panel::{self, AiChangesOp},
    dock::{DockOp, ProblemsOp, problems_panel},
    dock_layout,
    encoding_picker::EncodingAction,
    git::git_conflicts::{Side, Which},
    nav::NavOp,
    open::OpenOp,
    persist, references_panel,
    workbench::git_commands::GitCommand,
    workbench::recent_picker::RecentOp,
};

#[path = "close_many.rs"]
mod close_many;

#[path = "markdown_preview.rs"]
pub(crate) mod markdown_preview;

#[cfg(test)]
#[path = "markdown_preview_buttons_tests.rs"]
mod markdown_preview_buttons_tests;

#[cfg(test)]
#[path = "close_many_tests.rs"]
mod close_many_tests;

#[path = "compare_saved.rs"]
mod compare_saved;

#[cfg(test)]
#[path = "compare_saved_tests.rs"]
mod compare_saved_tests;

#[path = "local_history_view.rs"]
mod local_history_view;

#[cfg(test)]
#[path = "local_history_view_tests.rs"]
mod local_history_view_tests;

#[path = "provenance_column.rs"]
pub(crate) mod provenance_column;

#[cfg(test)]
#[path = "provenance_column_tests.rs"]
mod provenance_column_tests;

#[path = "focus_group.rs"]
mod focus_group;

#[cfg(test)]
#[path = "focus_group_tests.rs"]
mod focus_group_tests;

#[kiln::command(context = "workbench")]
impl Workbench {
    /// Close Tab
    #[command(id = "workbench.close_tab")]
    fn close_tab(&mut self, cx: &mut Cx<Self>) {
        self.on_active_pane(cx, |pane, window, cx| pane.close_active(window, cx));
    }

    /// Close Other Tabs
    #[command(id = "workbench.close_other_tabs")]
    fn close_other_tabs(&mut self, cx: &mut Cx<Self>) {
        self.on_active_pane(cx, |pane, window, cx| {
            pane.close_others(false, window, cx);
        });
    }

    /// Close Saved Editors
    #[command(id = "workbench.close_saved")]
    fn close_saved_editors(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.close_saved(cx.window, cx.cx);
    }

    /// Close Editors in Group
    #[command(id = "workbench.close_group_editors")]
    fn close_group_editors(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.close_editors(false, cx.window, cx.cx);
    }

    /// Close All Editors
    #[command(id = "workbench.close_all_editors")]
    fn close_all_editors(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.close_editors(true, cx.window, cx.cx);
    }

    /// Reopen Closed Editor
    #[command(id = "workbench.reopen_closed_editor")]
    fn reopen_closed_editor(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.reopen_closed(cx.window, cx.cx);
    }

    /// Toggle Zen Mode
    #[command(id = "workbench.toggle_zen_mode")]
    fn toggle_zen_mode_command(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.toggle_zen_mode(cx.window, cx.cx);
    }

    /// Next Tab
    #[command(id = "workbench.next_tab")]
    fn next_tab(&mut self, cx: &mut Cx<Self>) {
        self.on_active_pane(cx, |pane, window, cx| pane.cycle(false, window, cx));
    }

    /// Previous Tab
    #[command(id = "workbench.previous_tab")]
    fn previous_tab(&mut self, cx: &mut Cx<Self>) {
        self.on_active_pane(cx, |pane, window, cx| pane.cycle(true, window, cx));
    }

    /// Open Editor at Index 1
    #[command(id = "workbench.open_editor_at_index1")]
    fn open_editor_at_index1(&mut self, cx: &mut Cx<Self>) {
        self.open_editor_at(Some(0), cx);
    }

    /// Open Editor at Index 2
    #[command(id = "workbench.open_editor_at_index2")]
    fn open_editor_at_index2(&mut self, cx: &mut Cx<Self>) {
        self.open_editor_at(Some(1), cx);
    }

    /// Open Editor at Index 3
    #[command(id = "workbench.open_editor_at_index3")]
    fn open_editor_at_index3(&mut self, cx: &mut Cx<Self>) {
        self.open_editor_at(Some(2), cx);
    }

    /// Open Editor at Index 4
    #[command(id = "workbench.open_editor_at_index4")]
    fn open_editor_at_index4(&mut self, cx: &mut Cx<Self>) {
        self.open_editor_at(Some(3), cx);
    }

    /// Open Editor at Index 5
    #[command(id = "workbench.open_editor_at_index5")]
    fn open_editor_at_index5(&mut self, cx: &mut Cx<Self>) {
        self.open_editor_at(Some(4), cx);
    }

    /// Open Editor at Index 6
    #[command(id = "workbench.open_editor_at_index6")]
    fn open_editor_at_index6(&mut self, cx: &mut Cx<Self>) {
        self.open_editor_at(Some(5), cx);
    }

    /// Open Editor at Index 7
    #[command(id = "workbench.open_editor_at_index7")]
    fn open_editor_at_index7(&mut self, cx: &mut Cx<Self>) {
        self.open_editor_at(Some(6), cx);
    }

    /// Open Editor at Index 8
    #[command(id = "workbench.open_editor_at_index8")]
    fn open_editor_at_index8(&mut self, cx: &mut Cx<Self>) {
        self.open_editor_at(Some(7), cx);
    }

    /// Open Editor at Index 9
    #[command(id = "workbench.open_editor_at_index9")]
    fn open_editor_at_index9(&mut self, cx: &mut Cx<Self>) {
        self.open_editor_at(Some(8), cx);
    }

    /// Last Editor in Group
    #[command(id = "workbench.last_editor_in_group")]
    fn last_editor_in_group(&mut self, cx: &mut Cx<Self>) {
        self.open_editor_at(None, cx);
    }

    /// Keep Editor
    #[command(id = "workbench.keep_editor")]
    fn keep_editor(&mut self, cx: &mut Cx<Self>) {
        self.on_active_pane(cx, |pane, _, cx| pane.keep_active(cx));
    }

    /// Split Right
    #[command(id = "pane.split_right")]
    fn split_right(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.split(Direction::Right, cx.window, cx.cx);
    }

    /// Split Down
    #[command(id = "pane.split_down")]
    fn split_down(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.split(Direction::Down, cx.window, cx.cx);
    }

    /// Focus Left Pane
    #[command(id = "pane.focus_left")]
    fn focus_left(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.focus_pane(Direction::Left, cx.window, cx.cx);
    }

    /// Focus Right Pane
    #[command(id = "pane.focus_right")]
    fn focus_right(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.focus_pane(Direction::Right, cx.window, cx.cx);
    }

    /// Focus Pane Above
    #[command(id = "pane.focus_up")]
    fn focus_up(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.focus_pane(Direction::Up, cx.window, cx.cx);
    }

    /// Focus Pane Below
    #[command(id = "pane.focus_down")]
    fn focus_down(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.focus_pane(Direction::Down, cx.window, cx.cx);
    }

    /// Move Tab Left
    #[command(id = "pane.move_tab_left")]
    fn move_tab_left(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.move_tab(Direction::Left, cx.window, cx.cx);
    }

    /// Move Tab Right
    #[command(id = "pane.move_tab_right")]
    fn move_tab_right(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.move_tab(Direction::Right, cx.window, cx.cx);
    }

    /// Move Tab Up
    #[command(id = "pane.move_tab_up")]
    fn move_tab_up(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.move_tab(Direction::Up, cx.window, cx.cx);
    }

    /// Move Tab Down
    #[command(id = "pane.move_tab_down")]
    fn move_tab_down(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.move_tab(Direction::Down, cx.window, cx.cx);
    }

    /// Close Pane
    #[command(id = "pane.close")]
    fn close_pane(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.close_active_pane(cx.window, cx.cx);
    }

    /// Git: Compare with Index
    #[command(id = "git.diff_index")]
    fn git_diff_index(&mut self, cx: &mut Cx<Self>) {
        self.run_typed_git(GitCommand::DiffIndex, cx);
    }

    /// Git: Compare with HEAD
    #[command(id = "git.diff_head")]
    fn git_diff_head(&mut self, cx: &mut Cx<Self>) {
        self.run_typed_git(GitCommand::DiffHead, cx);
    }

    /// Compare Active File With…
    #[command(id = "diff.compare_file")]
    fn diff_compare_file(&mut self, cx: &mut Cx<Self>) {
        self.run_typed_git(GitCommand::CompareFile, cx);
    }

    /// Compare Active File with Saved
    #[command(id = "diff.compare_saved", context = "editor")]
    fn diff_compare_saved(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.compare_saved(cx.window, cx.cx);
    }

    /// Local History: Show History
    #[command(id = "history.show_local", context = "editor")]
    fn history_show_local(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.show_local_history(cx.window, cx.cx);
    }

    /// Toggle Edit Provenance
    #[command(id = "editor.toggle_provenance", context = "editor")]
    fn editor_toggle_provenance(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.toggle_provenance(cx.cx);
    }

    /// Local History: Restore This Version
    #[command(id = "history.restore_local", context = "DiffView")]
    fn history_restore_local(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.restore_local_history(cx.window, cx.cx);
    }

    /// Go to Next Change
    #[command(id = "git.next_change")]
    fn git_next_change(&mut self, cx: &mut Cx<Self>) {
        self.run_typed_git(GitCommand::NextChange, cx);
    }

    /// Git: Next Conflict
    #[command(id = "git.next_conflict")]
    fn git_next_conflict(&mut self, cx: &mut Cx<Self>) {
        self.in_editor(cx, |this, window, cx| this.go_to_conflict(true, window, cx));
    }

    /// Git: Previous Conflict
    #[command(id = "git.previous_conflict")]
    fn git_previous_conflict(&mut self, cx: &mut Cx<Self>) {
        self.in_editor(cx, |this, window, cx| {
            this.go_to_conflict(false, window, cx)
        });
    }

    /// Git: Accept Current Change
    #[command(id = "git.accept_current")]
    fn git_accept_current(&mut self, cx: &mut Cx<Self>) {
        let row = invocation_row(cx);
        self.in_editor(cx, |this, window, cx| {
            this.accept(Side::Current, Which::At(row), window, cx)
        });
    }

    /// Git: Accept Incoming Change
    #[command(id = "git.accept_incoming")]
    fn git_accept_incoming(&mut self, cx: &mut Cx<Self>) {
        let row = invocation_row(cx);
        self.in_editor(cx, |this, window, cx| {
            this.accept(Side::Incoming, Which::At(row), window, cx)
        });
    }

    /// Git: Accept Both Changes
    #[command(id = "git.accept_both")]
    fn git_accept_both(&mut self, cx: &mut Cx<Self>) {
        let row = invocation_row(cx);
        self.in_editor(cx, |this, window, cx| {
            this.accept(Side::Both, Which::At(row), window, cx)
        });
    }

    /// Git: Compare Conflict Changes
    #[command(id = "git.compare_conflict")]
    fn git_compare_conflict(&mut self, cx: &mut Cx<Self>) {
        let row = invocation_row(cx);
        self.in_editor(cx, |this, window, cx| {
            this.compare_conflict(row, window, cx)
        });
    }

    /// Git: Accept All Current
    #[command(id = "git.accept_all_current")]
    fn git_accept_all_current(&mut self, cx: &mut Cx<Self>) {
        self.in_editor(cx, |this, window, cx| {
            this.accept(Side::Current, Which::All, window, cx)
        });
    }

    /// Git: Accept All Incoming
    #[command(id = "git.accept_all_incoming")]
    fn git_accept_all_incoming(&mut self, cx: &mut Cx<Self>) {
        self.in_editor(cx, |this, window, cx| {
            this.accept(Side::Incoming, Which::All, window, cx)
        });
    }

    /// Git: Accept All Both
    #[command(id = "git.accept_all_both")]
    fn git_accept_all_both(&mut self, cx: &mut Cx<Self>) {
        self.in_editor(cx, |this, window, cx| {
            this.accept(Side::Both, Which::All, window, cx)
        });
    }

    /// Go to Previous Change
    #[command(id = "git.previous_change")]
    fn git_previous_change(&mut self, cx: &mut Cx<Self>) {
        self.run_typed_git(GitCommand::PreviousChange, cx);
    }

    /// Git: Stage File
    #[command(id = "git.stage_file")]
    fn git_stage_file(&mut self, cx: &mut Cx<Self>) {
        self.run_typed_git(GitCommand::StageFile, cx);
    }

    /// Git: Unstage File
    #[command(id = "git.unstage_file")]
    fn git_unstage_file(&mut self, cx: &mut Cx<Self>) {
        self.run_typed_git(GitCommand::UnstageFile, cx);
    }

    /// Git: Discard File Changes
    #[command(id = "git.discard_file")]
    fn git_discard_file(&mut self, cx: &mut Cx<Self>) {
        self.run_typed_git(GitCommand::DiscardFile, cx);
    }

    /// Git: Revert Editor to Index
    #[command(id = "git.reset_buffer")]
    fn git_reset_buffer(&mut self, cx: &mut Cx<Self>) {
        self.run_typed_git(GitCommand::ResetBuffer, cx);
    }

    /// Git: Stage Change
    #[command(id = "git.stage_hunk")]
    fn git_stage_hunk(&mut self, cx: &mut Cx<Self>) {
        self.run_typed_git(GitCommand::StageHunk, cx);
    }

    /// Git: Undo Stage Change
    #[command(id = "git.unstage_hunk")]
    fn git_unstage_hunk(&mut self, cx: &mut Cx<Self>) {
        self.run_typed_git(GitCommand::UnstageHunk, cx);
    }

    /// Git: Revert Change
    #[command(id = "git.revert_hunk")]
    fn git_revert_hunk(&mut self, cx: &mut Cx<Self>) {
        self.run_typed_git(GitCommand::RevertHunk, cx);
    }

    /// Git: Commit Staged
    #[command(id = "git.commit")]
    fn git_commit(&mut self, cx: &mut Cx<Self>) {
        self.run_typed_git(GitCommand::Commit, cx);
    }

    /// Git: Commit Staged (Amend)
    #[command(id = "git.commit_amend")]
    fn git_commit_amend(&mut self, cx: &mut Cx<Self>) {
        self.run_typed_git(GitCommand::CommitAmend, cx);
    }

    /// Git: Toggle Line Blame
    #[command(id = "git.toggle_line_blame")]
    fn git_toggle_line_blame(&mut self, cx: &mut Cx<Self>) {
        self.run_typed_git(GitCommand::ToggleLineBlame, cx);
    }

    /// Git: Toggle Blame Annotations
    #[command(id = "git.toggle_blame_annotations")]
    fn git_toggle_blame_annotations(&mut self, cx: &mut Cx<Self>) {
        self.run_typed_git(GitCommand::ToggleBlameAnnotations, cx);
    }

    /// Git: Show Line Blame
    #[command(id = "git.blame_line")]
    fn git_blame_line(&mut self, cx: &mut Cx<Self>) {
        self.run_typed_git(GitCommand::BlameLine, cx);
    }

    /// Git: Open Line's Commit Change
    #[command(id = "git.open_line_commit")]
    fn git_open_line_commit(&mut self, cx: &mut Cx<Self>) {
        self.run_typed_git(GitCommand::OpenLineCommit, cx);
    }

    /// Git: Switch Branch…
    #[command(id = "git.switch_branch")]
    fn git_switch_branch(&mut self, cx: &mut Cx<Self>) {
        self.run_typed_git(GitCommand::SwitchBranch, cx);
    }

    /// Git: Create Branch…
    #[command(id = "git.create_branch")]
    fn git_create_branch(&mut self, cx: &mut Cx<Self>) {
        self.run_typed_git(GitCommand::CreateBranch, cx);
    }

    /// Git: File History
    #[command(id = "git.file_history")]
    fn git_file_history(&mut self, cx: &mut Cx<Self>) {
        self.run_typed_git(GitCommand::FileHistory, cx);
    }

    /// Git: Log
    #[command(id = "git.log")]
    fn git_log(&mut self, cx: &mut Cx<Self>) {
        self.run_typed_git(GitCommand::Log, cx);
    }

    /// Git: Log (All Branches)
    #[command(id = "git.log_all")]
    fn git_log_all(&mut self, cx: &mut Cx<Self>) {
        self.run_typed_git(GitCommand::LogAll, cx);
    }

    /// Git: Stash
    #[command(id = "git.stash")]
    fn git_stash(&mut self, cx: &mut Cx<Self>) {
        self.run_typed_git(GitCommand::Stash, cx);
    }

    /// Git: Stash (Include Untracked)
    #[command(id = "git.stash_include_untracked")]
    fn git_stash_include_untracked(&mut self, cx: &mut Cx<Self>) {
        self.run_typed_git(GitCommand::StashIncludeUntracked, cx);
    }

    /// Git: Pop Stash…
    #[command(id = "git.stash_pop")]
    fn git_stash_pop(&mut self, cx: &mut Cx<Self>) {
        self.run_typed_git(GitCommand::StashPop, cx);
    }

    /// Git: Apply Stash…
    #[command(id = "git.stash_apply")]
    fn git_stash_apply(&mut self, cx: &mut Cx<Self>) {
        self.run_typed_git(GitCommand::StashApply, cx);
    }

    /// Grow Pane Width
    #[command(id = "pane.grow_width")]
    fn grow_pane_width(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.resize_active_pane(Axis::Horizontal, true, cx);
    }

    /// Shrink Pane Width
    #[command(id = "pane.shrink_width")]
    fn shrink_pane_width(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.resize_active_pane(Axis::Horizontal, false, cx);
    }

    /// Grow Pane Height
    #[command(id = "pane.grow_height")]
    fn grow_pane_height(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.resize_active_pane(Axis::Vertical, true, cx);
    }

    /// Shrink Pane Height
    #[command(id = "pane.shrink_height")]
    fn shrink_pane_height(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.resize_active_pane(Axis::Vertical, false, cx);
    }

    /// Equalize Panes
    #[command(id = "pane.equalize")]
    fn equalize(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.equalize_panes(cx);
    }

    /// Focus Next Pane
    #[command(id = "pane.focus_next")]
    fn focus_next(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.focus_next_pane(true, cx.window, cx.cx);
    }

    /// Focus Previous Pane
    #[command(id = "pane.focus_previous")]
    fn focus_previous(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.focus_next_pane(false, cx.window, cx.cx);
    }

    /// Focus First Editor Group
    #[command(id = "pane.focus_group1")]
    fn focus_group1(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.focus_group(1, cx.window, cx.cx);
    }

    /// Focus Second Editor Group
    #[command(id = "pane.focus_group2")]
    fn focus_group2(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.focus_group(2, cx.window, cx.cx);
    }

    /// Focus Third Editor Group
    #[command(id = "pane.focus_group3")]
    fn focus_group3(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.focus_group(3, cx.window, cx.cx);
    }

    /// Focus Fourth Editor Group
    #[command(id = "pane.focus_group4")]
    fn focus_group4(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.focus_group(4, cx.window, cx.cx);
    }

    /// Focus Left of Dock
    #[command(id = "dock.focus_left")]
    fn focus_left_of_dock(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.focus_from_dock(Direction::Left, cx.window, cx.cx);
    }

    /// Focus Right of Dock
    #[command(id = "dock.focus_right")]
    fn focus_right_of_dock(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.focus_from_dock(Direction::Right, cx.window, cx.cx);
    }

    /// Focus Above Dock
    #[command(id = "dock.focus_up")]
    fn focus_above_dock(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.focus_from_dock(Direction::Up, cx.window, cx.cx);
    }

    /// Focus Below Dock
    #[command(id = "dock.focus_down")]
    fn focus_below_dock(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.focus_from_dock(Direction::Down, cx.window, cx.cx);
    }

    /// Focus Active Editor
    #[command(id = "dock.focus_editor")]
    fn focus_editor_from_dock_command(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.focus_editor_from_dock(cx.window, cx.cx);
    }

    /// Grow Dock
    #[command(id = "dock.grow")]
    fn grow_dock(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.resize_focused_dock(dock_layout::STEP, cx.window, cx.cx);
    }

    /// Shrink Dock
    #[command(id = "dock.shrink")]
    fn shrink_dock(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.resize_focused_dock(-dock_layout::STEP, cx.window, cx.cx);
    }

    /// Move Terminal to Bottom
    #[command(id = "terminal.move_to_bottom")]
    fn move_terminal_to_bottom(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.move_terminal(DockPosition::Bottom, cx.cx);
    }

    /// Move Terminal to Left
    #[command(id = "terminal.move_to_left")]
    fn move_terminal_to_left(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.move_terminal(DockPosition::Left, cx.cx);
    }

    /// Move Terminal to Right
    #[command(id = "terminal.move_to_right")]
    fn move_terminal_to_right(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.move_terminal(DockPosition::Right, cx.cx);
    }

    /// Move Sidebar to Left
    #[command(id = "dock.move_sidebar_left")]
    fn move_sidebar_left(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.move_sidebar(DockPosition::Left, cx.cx);
    }

    /// Move Sidebar to Right
    #[command(id = "dock.move_sidebar_right")]
    fn move_sidebar_right(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.move_sidebar(DockPosition::Right, cx.cx);
    }

    /// Show Project
    #[command(id = "dock.show_project")]
    fn show_project(&mut self, cx: &mut Cx<Self>) {
        self.dock_command(DockOp::ShowProject, cx);
    }

    /// Toggle Dock
    #[command(id = "dock.toggle")]
    fn toggle_dock(&mut self, cx: &mut Cx<Self>) {
        self.dock_command(DockOp::Toggle, cx);
    }

    /// Find in Files
    #[command(id = "search.show")]
    fn find_in_files(&mut self, cx: &mut Cx<Self>) {
        self.dock_command(DockOp::ShowSearch, cx);
    }

    /// Replace in Files…
    ///
    /// The search panel does it itself while focused.
    #[command(id = "search.show_replace")]
    pub(crate) fn replace_in_files(&mut self, cx: &mut Cx<Self>) {
        self.dock_command(DockOp::ShowReplace, cx);
    }

    /// Replace All in Files
    ///
    /// The search panel does it itself while focused.
    #[command(id = "search.replace_all")]
    pub(crate) fn replace_all_in_files(&mut self, cx: &mut Cx<Self>) {
        self.dock_command(DockOp::ReplaceAll, cx);
    }

    /// Show Changes
    #[command(id = "git.show_changes")]
    fn show_changes(&mut self, cx: &mut Cx<Self>) {
        self.dock_command(DockOp::ShowChanges, cx);
    }

    /// Refresh Changes
    #[command(id = "git.refresh")]
    fn refresh_changes(&mut self, cx: &mut Cx<Self>) {
        self.dock_command(DockOp::RefreshChanges, cx);
    }

    /// Show Outline
    #[command(id = "outline.show")]
    fn show_outline(&mut self, cx: &mut Cx<Self>) {
        self.dock_command(DockOp::ShowOutline, cx);
    }

    /// Reveal Active File in Project
    #[command(id = "project_panel.reveal_active")]
    fn reveal_active_file(&mut self, cx: &mut Cx<Self>) {
        self.in_context(crate::KEY_CONTEXT, cx, |this, _, cx| this.reveal_active(cx));
    }

    /// Find All References
    #[command(id = "nav.find_references")]
    fn find_all_references(&mut self, cx: &mut Cx<Self>) {
        self.in_context(crate::KEY_CONTEXT, cx, Self::find_references);
    }

    /// Select Previous Reference
    ///
    /// The References panel does it itself while focused.
    #[command(id = "references_panel.select_prev", context = "ReferencesPanel")]
    pub(crate) fn select_previous_reference(&mut self, cx: &mut Cx<Self>) {
        self.references_command(cx, |panel, _, cx| panel.move_selection(-1, cx));
    }

    /// Select Next Reference
    ///
    /// The References panel does it itself while focused.
    #[command(id = "references_panel.select_next", context = "ReferencesPanel")]
    pub(crate) fn select_next_reference(&mut self, cx: &mut Cx<Self>) {
        self.references_command(cx, |panel, _, cx| panel.move_selection(1, cx));
    }

    /// Select First Reference
    ///
    /// The References panel does it itself while focused.
    #[command(id = "references_panel.select_first", context = "ReferencesPanel")]
    pub(crate) fn select_first_reference(&mut self, cx: &mut Cx<Self>) {
        self.references_command(cx, |panel, _, cx| panel.select_end(false, cx));
    }

    /// Select Last Reference
    ///
    /// The References panel does it itself while focused.
    #[command(id = "references_panel.select_last", context = "ReferencesPanel")]
    pub(crate) fn select_last_reference(&mut self, cx: &mut Cx<Self>) {
        self.references_command(cx, |panel, _, cx| panel.select_end(true, cx));
    }

    /// Open Reference
    ///
    /// The References panel does it itself while focused.
    #[command(id = "references_panel.open", context = "ReferencesPanel")]
    pub(crate) fn open_reference(&mut self, cx: &mut Cx<Self>) {
        self.references_command(cx, ReferencesPanel::open_selected);
    }

    /// Show Problems
    #[command(id = "problems.show")]
    fn show_problems(&mut self, cx: &mut Cx<Self>) {
        self.dock_command(DockOp::Problems(ProblemsOp::Show), cx);
    }

    /// Select Next Problem
    #[command(id = "problems.select_next", context = "problems_panel")]
    fn select_next_problem(&mut self, cx: &mut Cx<Self>) {
        self.problems_command(ProblemsOp::SelectNext, cx);
    }

    /// Select Previous Problem
    #[command(id = "problems.select_previous", context = "problems_panel")]
    fn select_previous_problem(&mut self, cx: &mut Cx<Self>) {
        self.problems_command(ProblemsOp::SelectPrevious, cx);
    }

    /// Select First Problem
    #[command(id = "problems.select_first", context = "problems_panel")]
    fn select_first_problem(&mut self, cx: &mut Cx<Self>) {
        self.problems_command(ProblemsOp::SelectFirst, cx);
    }

    /// Select Last Problem
    #[command(id = "problems.select_last", context = "problems_panel")]
    fn select_last_problem(&mut self, cx: &mut Cx<Self>) {
        self.problems_command(ProblemsOp::SelectLast, cx);
    }

    /// Open Problem
    #[command(id = "problems.open", context = "problems_panel")]
    fn open_problem_command(&mut self, cx: &mut Cx<Self>) {
        self.problems_command(ProblemsOp::Open, cx);
    }

    /// Show AI Changes
    #[command(id = "ai.show_changes")]
    fn show_ai_changes(&mut self, cx: &mut Cx<Self>) {
        self.in_context(crate::KEY_CONTEXT, cx, |this, window, cx| {
            this.ai_changes_panel(cx);
            this.run_dock_op(DockOp::ShowAiChanges, window, cx)
        });
    }

    /// Review Latest Chat Changes
    ///
    /// Loads every file edit the open chat's latest reply proposes into the
    /// AI Changes panel, as the reply's Review Changes button does.
    #[command(id = "chat.review_latest_changes")]
    fn review_latest_chat_changes(&mut self, cx: &mut Cx<Self>) {
        self.in_context(crate::KEY_CONTEXT, cx, |this, window, cx| {
            if let Some(chat) = this.dock().read(cx).chat().cloned() {
                // The panel reads the workbench, which is being updated.
                window.defer(cx, move |window, cx| {
                    chat.update(cx, |chat, cx| chat.review_latest(window, cx))
                });
            }
        });
    }

    /// Accept All AI Changes
    #[command(id = "ai.changes_accept_all")]
    fn ai_changes_accept_all(&mut self, cx: &mut Cx<Self>) {
        self.ai_changes_command(AiChangesOp::AcceptAll, crate::KEY_CONTEXT, cx);
    }

    /// Reject All AI Changes
    #[command(id = "ai.changes_reject_all")]
    fn ai_changes_reject_all(&mut self, cx: &mut Cx<Self>) {
        self.ai_changes_command(AiChangesOp::RejectAll, crate::KEY_CONTEXT, cx);
    }

    /// Revert AI Turn
    #[command(id = "ai.changes_revert")]
    fn ai_changes_revert(&mut self, cx: &mut Cx<Self>) {
        self.ai_changes_command(AiChangesOp::Revert, crate::KEY_CONTEXT, cx);
    }

    /// Select Next AI Changed File
    #[command(id = "ai.changes_next_file", context = "ai_changes")]
    fn ai_changes_next_file(&mut self, cx: &mut Cx<Self>) {
        self.ai_changes_command(AiChangesOp::NextFile, ai_changes_panel::KEY_CONTEXT, cx);
    }

    /// Select Previous AI Changed File
    #[command(id = "ai.changes_previous_file", context = "ai_changes")]
    fn ai_changes_previous_file(&mut self, cx: &mut Cx<Self>) {
        self.ai_changes_command(AiChangesOp::PreviousFile, ai_changes_panel::KEY_CONTEXT, cx);
    }

    /// Go to Next Problem
    #[command(id = "diagnostics.next")]
    fn go_to_next_problem(&mut self, cx: &mut Cx<Self>) {
        self.problems_command(ProblemsOp::Next, cx);
    }

    /// Go to Previous Problem
    #[command(id = "diagnostics.previous")]
    fn go_to_previous_problem(&mut self, cx: &mut Cx<Self>) {
        self.problems_command(ProblemsOp::Previous, cx);
    }

    /// Toggle Status Bar
    #[command(id = "view.toggle_status_bar")]
    fn toggle_status_bar(&mut self, cx: &mut Cx<Self>) {
        self.set_status_bar_visible(!self.status_bar_visible(), cx.cx);
        persist::save_status_bar(self.status_bar_visible(), cx.cx);
    }

    /// Go to File
    #[command(id = "quick_open.toggle")]
    fn toggle_quick_open_command(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.toggle_quick_open(cx.window, cx.cx);
    }

    /// Go to Line/Column…
    #[command(id = "go_to_line.toggle")]
    fn toggle_go_to_line_command(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.toggle_go_to_line(cx.window, cx.cx);
    }

    /// Dismiss Notifications
    #[command(id = "toast.dismiss_all")]
    fn dismiss_notifications(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.toasts
            .update(cx.cx, |toasts, cx| toasts.dismiss_all(cx));
    }

    /// Show Config Problems
    #[command(id = "workbench.show_config_problems")]
    fn show_config_problems_command(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.show_config_problems(cx.window, cx.cx);
    }

    /// Toggle Breadcrumbs
    #[command(id = "view.toggle_breadcrumbs")]
    fn toggle_breadcrumbs_command(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.toggle_breadcrumbs(cx.cx);
    }

    /// Change Language Mode…
    #[command(id = "language.select")]
    fn select_language_mode(&mut self, cx: &mut Cx<Self>) {
        self.select_language(cx.window, cx.cx);
    }

    /// Change File Encoding
    #[command(id = "workbench.change_encoding")]
    fn change_encoding_command(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.change_encoding(cx.window, cx.cx);
    }

    /// Reopen Editor with Encoding…
    #[command(id = "workbench.reopen_with_encoding")]
    fn reopen_with_encoding_command(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.pick_encoding_for_active(EncodingAction::Reopen, cx.window, cx.cx);
    }

    /// Save with Encoding…
    #[command(id = "workbench.save_with_encoding")]
    fn save_with_encoding_command(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.pick_encoding_for_active(EncodingAction::Save, cx.window, cx.cx);
    }

    /// Change Color Theme…
    #[command(id = "theme.select")]
    fn select_color_theme(&mut self, cx: &mut Cx<Self>) {
        self.select_theme(cx.window, cx.cx);
    }

    /// Extensions: Show Installed
    #[command(id = "workbench.show_extensions")]
    fn show_extensions_command(&mut self, cx: &mut Cx<Self>) {
        self.show_extensions(cx.window, cx.cx);
    }

    /// Go to Definition
    #[command(id = "nav.goto_definition")]
    fn go_to_definition(&mut self, cx: &mut Cx<Self>) {
        // Over a language server's link, as a cmd-click there is, the link
        // opens instead.
        let source = kiln::current_invocation(cx.cx).map(|invocation| invocation.source);
        if self.open_link_for_definition(source, cx.window, cx.cx) {
            return self.ran(cx);
        }
        self.nav_command(NavOp::GotoDefinition, cx);
    }

    /// Peek Definition
    #[command(id = "editor.peek_definition", context = "editor")]
    fn peek_definition(&mut self, cx: &mut Cx<Self>) {
        self.in_editor(cx, |this, window, cx| {
            this.run_nav_op(NavOp::PeekDefinition, window, cx)
        });
    }

    /// Peek References
    #[command(id = "editor.peek_references", context = "editor")]
    fn peek_references_command(&mut self, cx: &mut Cx<Self>) {
        self.in_editor(cx, Self::peek_references);
    }

    /// Close Peek
    #[command(id = "peek.close", context = "peek || peek_shown")]
    fn close_peek_command(&mut self, cx: &mut Cx<Self>) {
        self.in_context("peek || peek_shown", cx, Self::close_peek);
    }

    /// Open Peeked Definition
    #[command(id = "peek.open", context = "peek")]
    fn open_from_peek_command(&mut self, cx: &mut Cx<Self>) {
        self.in_context(crate::peek::KEY_CONTEXT, cx, Self::open_from_peek);
    }

    /// Open Link
    #[command(id = "editor.open_link", context = "editor")]
    fn open_link(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.in_editor(cx, |this, window, cx| this.open_link_at_caret(window, cx));
    }

    /// Go to Declaration
    #[command(id = "nav.goto_declaration")]
    fn go_to_declaration(&mut self, cx: &mut Cx<Self>) {
        self.nav_command(NavOp::Related(Related::Declaration), cx);
    }

    /// Go to Type Definition
    #[command(id = "nav.goto_type_definition")]
    fn go_to_type_definition(&mut self, cx: &mut Cx<Self>) {
        self.nav_command(NavOp::Related(Related::TypeDefinition), cx);
    }

    /// Go to Implementations
    #[command(id = "nav.goto_implementation")]
    fn go_to_implementation(&mut self, cx: &mut Cx<Self>) {
        self.nav_command(NavOp::Related(Related::Implementation), cx);
    }

    /// Show Call Hierarchy
    #[command(id = "nav.show_call_hierarchy")]
    fn show_call_hierarchy_command(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.show_call_hierarchy(cx.window, cx.cx);
    }

    /// Show Type Hierarchy
    #[command(id = "nav.show_type_hierarchy")]
    fn show_type_hierarchy_command(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.show_type_hierarchy(cx.window, cx.cx);
    }

    /// Toggle Incoming and Outgoing Calls
    #[command(id = "hierarchy.toggle_direction")]
    fn toggle_hierarchy_direction_command(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.toggle_hierarchy_direction(cx.window, cx.cx);
    }

    /// Go to Symbol in Workspace…
    #[command(id = "nav.workspace_symbols")]
    fn workspace_symbols(&mut self, cx: &mut Cx<Self>) {
        self.nav_command(NavOp::WorkspaceSymbols, cx);
    }

    /// Go Back
    #[command(id = "nav.back")]
    fn go_back(&mut self, cx: &mut Cx<Self>) {
        self.nav_command(NavOp::Back, cx);
    }

    /// Go Forward
    #[command(id = "nav.forward")]
    fn go_forward(&mut self, cx: &mut Cx<Self>) {
        self.nav_command(NavOp::Forward, cx);
    }

    /// Open Recent…
    #[command(id = "workbench.open_recent")]
    fn open_recent(&mut self, cx: &mut Cx<Self>) {
        self.recent_command(RecentOp::Open, cx);
    }

    /// Clear Recently Opened
    #[command(id = "workbench.clear_recent")]
    fn clear_recent(&mut self, cx: &mut Cx<Self>) {
        self.recent_command(RecentOp::Clear, cx);
    }

    /// Open Recent in New Window
    #[command(id = "recent.open_in_new_window", context = "picker", palette = false)]
    fn open_recent_in_new_window(&mut self, cx: &mut Cx<Self>) {
        self.recent_command(RecentOp::OpenInNewWindow, cx);
    }

    /// Restart Language Server
    #[command(id = "lsp.restart")]
    fn restart_language_server(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.run_server_op(false, cx.window, cx.cx);
    }

    /// Language Server Log
    #[command(id = "lsp.show_log")]
    fn show_language_server_log(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.run_server_op(true, cx.window, cx.cx);
    }

    /// Rename Symbol…
    #[command(id = "lsp.rename", context = "editor")]
    fn rename_symbol_command(&mut self, cx: &mut Cx<Self>) {
        self.in_editor(cx, |this, window, cx| this.rename_symbol(window, cx));
    }

    /// Inline Edit with AI
    #[command(id = "ai.inline_edit", context = "editor")]
    fn inline_edit_command(&mut self, cx: &mut Cx<Self>) {
        self.in_editor(cx, |this, window, cx| this.inline_edit(window, cx));
    }

    /// Send Inline Edit
    #[command(id = "inline_edit.submit", context = "inline_edit", palette = false)]
    fn inline_edit_submit_command(&mut self, cx: &mut Cx<Self>) {
        self.in_context(crate::inline_edit::KEY_CONTEXT, cx, |this, window, cx| {
            this.inline_edit_submit(window, cx)
        });
    }

    /// Cancel Inline Edit
    #[command(id = "inline_edit.cancel", context = "inline_edit", palette = false)]
    fn inline_edit_cancel_command(&mut self, cx: &mut Cx<Self>) {
        self.in_context(crate::inline_edit::KEY_CONTEXT, cx, |this, window, cx| {
            this.inline_edit_cancel(window, cx)
        });
    }

    /// Code Actions…
    #[command(id = "lsp.code_actions", context = "editor")]
    fn code_actions(&mut self, cx: &mut Cx<Self>) {
        self.in_editor(cx, |this, window, cx| {
            this.show_code_actions(CodeActionQuery::default(), false, window, cx)
        });
    }

    /// Organize Imports
    #[command(id = "lsp.organize_imports", context = "editor")]
    fn organize_imports(&mut self, cx: &mut Cx<Self>) {
        self.code_actions_of("source.organizeImports", true, cx);
    }

    /// Fix All
    #[command(id = "lsp.fix_all", context = "editor")]
    fn fix_all(&mut self, cx: &mut Cx<Self>) {
        self.code_actions_of("source.fixAll", true, cx);
    }

    /// Refactor…
    #[command(id = "lsp.refactor", context = "editor")]
    fn refactor(&mut self, cx: &mut Cx<Self>) {
        self.code_actions_of("refactor", false, cx);
    }

    /// Source Action…
    #[command(id = "lsp.source_action", context = "editor")]
    fn source_action(&mut self, cx: &mut Cx<Self>) {
        self.code_actions_of("source", false, cx);
    }

    /// Run Code Lens
    #[command(id = "lsp.run_lens", context = "editor", palette = false)]
    fn run_code_lens(&mut self, cx: &mut Cx<Self>) {
        let id = kiln::current_invocation(cx.cx)
            .and_then(|invocation| invocation.args.get("lens")?.as_u64());
        self.in_editor(cx, |this, _, cx| {
            if let Some(id) = id {
                this.run_lens(id, cx);
            }
        });
    }

    /// Format Document
    #[command(id = "lsp.format_document", context = "editor")]
    fn format_document(&mut self, cx: &mut Cx<Self>) {
        self.in_editor(cx, |this, window, cx| this.format_active(false, window, cx));
    }

    /// Format Selection
    #[command(id = "lsp.format_selection", context = "editor")]
    fn format_selection(&mut self, cx: &mut Cx<Self>) {
        self.in_editor(cx, |this, window, cx| this.format_active(true, window, cx));
    }

    /// Markdown: Open Preview
    #[command(id = "markdown.show_preview", context = "editor")]
    fn show_markdown_preview(&mut self, cx: &mut Cx<Self>) {
        self.in_editor(cx, |this, window, cx| {
            this.open_markdown_preview(false, window, cx)
        });
    }

    /// Markdown: Open Preview to the Side
    #[command(id = "markdown.show_preview_to_side", context = "editor")]
    fn show_markdown_preview_to_side(&mut self, cx: &mut Cx<Self>) {
        self.in_editor(cx, |this, window, cx| {
            this.open_markdown_preview(true, window, cx)
        });
    }

    /// Markdown: Toggle Preview
    #[command(id = "markdown.toggle_preview", context = "editor || markdown_preview")]
    fn toggle_markdown_preview_command(&mut self, cx: &mut Cx<Self>) {
        self.in_context("editor || markdown_preview", cx, |this, window, cx| {
            this.toggle_markdown_preview(window, cx)
        });
    }

    /// Open as Text
    #[command(id = "image.open_as_text", context = "ImageViewer && svg")]
    fn open_as_text_command(&mut self, cx: &mut Cx<Self>) {
        self.in_context("ImageViewer && svg", cx, |this, window, cx| {
            let pane = this.pane().clone();
            let path = svg_tab(&pane, true, cx);
            if let Some(path) = path {
                pane.update(cx, |pane, cx| pane.open_as_text(path, window, cx));
            }
        });
    }

    /// Open as Image
    #[command(id = "image.open_as_image", context = "editor")]
    fn open_as_image_command(&mut self, cx: &mut Cx<Self>) {
        self.in_context("editor", cx, |this, window, cx| {
            let pane = this.pane().clone();
            let path = svg_tab(&pane, false, cx);
            if let Some(path) = path {
                pane.update(cx, |pane, cx| pane.open_as_image(&path, window, cx));
            }
        });
    }

    /// Open Folder…
    #[command(id = "workbench.open_folder")]
    fn open_folder(&mut self, cx: &mut Cx<Self>) {
        self.open_command(OpenOp::Folder, cx);
    }

    /// Open File…
    #[command(id = "workbench.open_file")]
    fn open_file(&mut self, cx: &mut Cx<Self>) {
        self.open_command(OpenOp::File, cx);
    }

    /// Open Settings (JSON)
    #[command(id = "workbench.open_settings")]
    fn open_settings_command(&mut self, cx: &mut Cx<Self>) {
        self.open_command(OpenOp::Settings, cx);
    }

    /// Open Settings (UI)
    #[command(id = "workbench.open_settings_ui")]
    fn open_settings_ui_command(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.open_settings_ui(cx.window, cx.cx);
    }

    /// Open Keyboard Shortcuts
    #[command(id = "workbench.open_keyboard_shortcuts")]
    fn open_keyboard_shortcuts_command(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.open_keyboard_shortcuts(cx.window, cx.cx);
    }

    /// Open Keyboard Shortcuts (JSON)
    #[command(id = "workbench.open_keyboard_shortcuts_json")]
    fn open_keyboard_shortcuts_json_command(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.open_keymap_file(cx.window, cx.cx);
    }

    /// New Window
    #[command(id = "window.new")]
    fn new_window(&mut self, cx: &mut Cx<Self>) {
        self.open_command(OpenOp::NewWindow, cx);
    }

    /// Open Folder in New Window…
    #[command(id = "workbench.open_folder_in_new_window")]
    fn open_folder_in_new_window(&mut self, cx: &mut Cx<Self>) {
        self.open_command(OpenOp::FolderInNewWindow, cx);
    }

    /// New File
    #[command(id = "workbench.new_file")]
    fn new_file_command(&mut self, cx: &mut Cx<Self>) {
        self.open_command(OpenOp::NewFile, cx);
    }

    /// Save As…
    #[command(id = "workbench.save_as")]
    fn save_as(&mut self, cx: &mut Cx<Self>) {
        self.open_command(OpenOp::SaveAs, cx);
    }

    /// Trust This Folder
    #[command(id = "workbench.trust_folder")]
    fn trust_folder(&mut self, cx: &mut Cx<Self>) {
        self.trust_command(true, cx);
    }

    /// Untrust This Folder
    #[command(id = "workbench.untrust_folder")]
    fn untrust_folder(&mut self, cx: &mut Cx<Self>) {
        self.trust_command(false, cx);
    }

    /// Vim: Cancel Filter
    #[command(id = "workbench.cancel_vim_filter")]
    fn cancel_vim_filter_command(&mut self, cx: &mut Cx<Self>) {
        self.cancel_vim_filter(cx.window, cx.cx);
    }

    /// Toggle Terminal
    #[command(id = "terminal.toggle")]
    fn toggle_terminal_command(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.toggle_terminal(cx.window, cx.cx);
    }

    /// New Terminal
    #[command(id = "terminal.new")]
    fn new_terminal_command(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.new_terminal(cx.window, cx.cx);
    }

    /// Kill Terminal
    #[command(id = "terminal.close")]
    fn close_terminal_command(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.close_terminal(cx.window, cx.cx);
    }

    /// Rename Terminal
    #[command(id = "terminal.rename")]
    fn rename_terminal_command(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.rename_terminal(cx.window, cx.cx);
    }

    /// Next Terminal
    #[command(id = "terminal.next")]
    fn next_terminal(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.cycle_terminal(1, cx.window, cx.cx);
    }

    /// Previous Terminal
    #[command(id = "terminal.previous")]
    fn previous_terminal(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.cycle_terminal(-1, cx.window, cx.cx);
    }

    /// Terminal: Find
    #[command(id = "terminal.find")]
    fn find_in_terminal_command(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.find_in_terminal(cx.window, cx.cx);
    }

    /// Terminal: Previous Prompt
    #[command(id = "terminal.previous_prompt", context = "terminal")]
    fn previous_prompt(&mut self, cx: &mut Cx<Self>) {
        self.in_context(fe_terminal_view::KEY_CONTEXT, cx, |this, _, cx| {
            this.scroll_terminal_to_prompt(false, cx)
        });
    }

    /// Terminal: Next Prompt
    #[command(id = "terminal.next_prompt", context = "terminal")]
    fn next_prompt(&mut self, cx: &mut Cx<Self>) {
        self.in_context(fe_terminal_view::KEY_CONTEXT, cx, |this, _, cx| {
            this.scroll_terminal_to_prompt(true, cx)
        });
    }

    /// Terminal: Vi Copy Mode
    #[command(id = "terminal.vi_mode", context = "terminal")]
    fn terminal_vi_mode(&mut self, cx: &mut Cx<Self>) {
        self.in_context(fe_terminal_view::KEY_CONTEXT, cx, |this, window, cx| {
            this.toggle_terminal_vi_mode(window, cx)
        });
    }
}

impl Workbench {
    /// Runs `run` on the active pane, then redraws it.
    pub(crate) fn on_active_pane(
        &mut self,
        cx: &mut Cx<Self>,
        run: impl FnOnce(&mut Pane, &mut Window, &mut App),
    ) {
        self.ran(cx);
        let Cx { window, cx } = cx;
        self.pane().clone().update(*cx, |pane, cx| {
            run(pane, window, cx);
            cx.notify();
        });
    }

    /// Does the dock op `op`, as a rail click does.
    fn dock_command(&mut self, op: DockOp, cx: &mut Cx<Self>) {
        self.in_context(crate::KEY_CONTEXT, cx, |this, window, cx| {
            this.run_dock_op(op, window, cx)
        });
    }

    /// Runs `run` on the References panel, a panel command's, if the
    /// command was invoked where the panel's context holds: see
    /// [`Workbench::in_context`].
    fn references_command(
        &mut self,
        cx: &mut Cx<Self>,
        run: impl FnOnce(&mut ReferencesPanel, &mut Window, &mut Context<ReferencesPanel>) + 'static,
    ) {
        self.in_context(references_panel::KEY_CONTEXT, cx, |this, window, cx| {
            this.on_references_panel(run, window, cx)
        });
    }

    /// Does the AI changes op `op`, if the command was invoked where
    /// `context` holds: see [`Workbench::in_context`].
    fn ai_changes_command(&mut self, op: AiChangesOp, context: &str, cx: &mut Cx<Self>) {
        self.in_context(context, cx, |this, window, cx| {
            this.run_ai_changes_op(op, window, cx)
        });
    }

    /// Does the problems op `op`, if the command was invoked where its
    /// context holds: the panel's for selecting and opening, the
    /// workbench's for going to a diagnostic. See [`Workbench::in_context`].
    fn problems_command(&mut self, op: ProblemsOp, cx: &mut Cx<Self>) {
        let context = match op {
            ProblemsOp::SelectNext
            | ProblemsOp::SelectPrevious
            | ProblemsOp::SelectFirst
            | ProblemsOp::SelectLast
            | ProblemsOp::Open => problems_panel::KEY_CONTEXT,
            ProblemsOp::Show | ProblemsOp::Next | ProblemsOp::Previous => crate::KEY_CONTEXT,
        };
        self.in_context(context, cx, |this, window, cx| {
            this.run_problems_op(op, window, cx)
        });
    }

    /// Runs the nav command `op`.
    fn nav_command(&mut self, op: NavOp, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.run_nav_op(op, cx.window, cx.cx);
    }

    /// Runs the recent command `op`.
    fn recent_command(&mut self, op: RecentOp, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.run_recent_op(op, cx.window, cx.cx);
    }

    /// Runs `run`, an editor command's, if the command was invoked where
    /// the editor's context holds ([`Self::in_context`]).
    fn in_editor(
        &mut self,
        cx: &mut Cx<Self>,
        run: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>),
    ) {
        self.in_context(fe_editor::commands::CONTEXT, cx, run);
    }

    /// Lists the code actions of `kind` at the editor's selection: applies
    /// the one there is, when `direct`.
    fn code_actions_of(&mut self, kind: &str, direct: bool, cx: &mut Cx<Self>) {
        let query = CodeActionQuery {
            only: Some(vec![kind.to_owned()]),
            automatic: false,
        };
        self.in_editor(cx, |this, window, cx| {
            this.show_code_actions(query, direct, window, cx)
        });
    }

    /// Runs `run`, a command's in `context`, if the command was invoked
    /// where `context` holds: see [`invoked_in`].
    fn in_context(
        &mut self,
        context: &str,
        cx: &mut Cx<Self>,
        run: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>),
    ) {
        self.ran(cx);
        if invoked_in(context, cx.cx) {
            run(self, cx.window, cx.cx);
        }
    }

    /// Runs the open command `op`.
    fn open_command(&mut self, op: OpenOp, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.prompt_open(op, cx.window, cx.cx);
    }

    /// Trusts the root, if `trusted`, or untrusts it.
    fn trust_command(&mut self, trusted: bool, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.set_root_trust(trusted, cx.cx);
    }

    /// Runs the git or diff `command` for the active editor, as
    /// [`crate::workbench::git_commands`] says.
    fn run_typed_git(&mut self, command: GitCommand, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.run_git_command(command, cx.window, cx.cx);
    }

    /// Notes the command running in `commands_run`, for tests.
    fn ran(&mut self, cx: &Cx<Self>) {
        #[cfg(test)]
        self.commands_run.extend(kiln::current_invocation(cx.cx));
        #[cfg(not(test))]
        let _ = (self, cx);
    }
}

/// The `row` of the running command's args table, if it was given one.
fn invocation_row(cx: &Cx<Workbench>) -> Option<u32> {
    let invocation = kiln::current_invocation(cx.cx)?;
    let row = invocation.args.get("row")?.as_u64()?;
    u32::try_from(row).ok()
}

/// Whether the typed command running was invoked where `context`, a
/// [`ContextPredicate`], holds, as its `fe-command` registry checked,
/// warning if not: kiln runs a typed command's handler wherever it's
/// dispatched, so a menu pick or a `RunCommand` with the project panel or
/// terminal focused would act on the active editor, or on a panel. A run
/// with no invocation, as a raw action, whose key binding checked its
/// context, applies. For the workbench's handlers and a panel's of the
/// commands it does itself.
pub(crate) fn invoked_in(context: &str, cx: &App) -> bool {
    let context = ContextPredicate::parse(context).expect("a command's context parses");
    match kiln::current_invocation(cx) {
        Some(invocation) if !context.eval(&invocation.context) => {
            log::warn!(
                "{} doesn't apply here: not in {context}",
                invocation.command
            );
            false
        }
        _ => true,
    }
}

/// The path of `pane`'s active tab if it's an SVG's viewer (`viewer`) or
/// an SVG's editor: `image.open_as_text` and `image.open_as_image` do
/// nothing on any other tab.
fn svg_tab(
    pane: &gpui::Entity<crate::Pane>,
    viewer: bool,
    cx: &gpui::App,
) -> Option<std::path::PathBuf> {
    let pane = pane.read(cx);
    let tab = pane.tabs().get(pane.active()?)?;
    let shown = if viewer {
        tab.image_viewer().is_some()
    } else {
        tab.editor().is_some()
    };
    let path = tab.path.clone()?;
    (shown && crate::loader::is_svg(&path)).then_some(path)
}
