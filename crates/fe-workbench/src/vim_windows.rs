//! Vim's window commands over the window's panes: `:sp`, `:vs`, `:new`,
//! `:vne`, `:on` and `:clo` ([`WindowCommand`]), run as the pane commands
//! run, with no pane model of their own. A window is a pane; the current
//! one is the pane of the editor the line was typed in, or for an editor
//! shown outside the tabs (a diff view's side, the commit box) the active
//! pane.
//!
//! - `:sp` and `:vs` split the pane as `pane.split_down` and
//!   `pane.split_right` do (`<C-w>s` and `<C-w>v`): the new pane, active
//!   and focused, shows the same buffer as a second tab of its text, not a
//!   copy of it. nvim puts `:sp`'s new window above ('splitbelow' off) and
//!   `:vs`'s at the left ('splitright' off); fastedit keeps its panes'
//!   geometry, below and to the right, as `<C-w>s` and `<C-w>v` split here.
//! - `:sp {file}` and `:vs {file}` (and `:new {file}`, `:vne {file}`) show
//!   the file in the new pane, alone, read as `:e` reads its path. A file
//!   that isn't there, nor open, is `E484` and nothing splits: unlike
//!   nvim, which edits a new buffer of that name, as `:e` here doesn't.
//! - `:new` and `:vne` split with an untitled empty tab, focused, in place
//!   of the second view.
//! - `:on` closes every other pane. Their tabs with changes aren't lost, as
//!   nvim's 'hidden' (on by default) keeps their buffers: each moves into
//!   the current pane, after its tabs, unless that shows its text already.
//!   The current pane goes on showing what it showed. The other tabs close
//!   with their panes, unasked. `!` changes nothing, as nothing is
//!   discarded. With one pane, it says "Already only one window".
//! - `:clo` closes the current pane as `pane.close` does, asking of each
//!   tab with changes that no other pane shows whether to discard them;
//!   the last pane is `E444`. Its `!` is the same: nvim's keeps the
//!   buffer, and so does the question's Cancel.
//!
//! Neither `:on` nor `:clo` closes the window or quits the app.

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use fe_gpui::EditorView;
use fe_vim::WindowCommand;
use gpui::{Context, Entity, Window};
use kiln_shell::{ItemId, PaneView as _};

use super::{Said, message};
use crate::pane_group::{Destination, Direction};
use crate::{Pane, Workbench};
use kiln_l10n::tr;

const E444: Said = Said(|| tr!("workbench-workbench-vim-e444"));
const E445: Said = Said(|| tr!("workbench-workbench-vim-e445-window"));
const ONLY_ONE: Said = Said(|| tr!("workbench-workbench-vim-only-window"));

impl Workbench {
    /// Runs `command`, typed in `editor`: see the module docs. Every
    /// answer, such as `E444`, is `editor`'s vim message.
    pub(super) fn run_vim_window_command(
        &mut self,
        editor: &Entity<EditorView>,
        command: WindowCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let direction = |vertical| match vertical {
            true => Direction::Right,
            false => Direction::Down,
        };
        let done = match command {
            WindowCommand::Split { vertical, file } => {
                self.vim_split(editor, direction(vertical), file, window, cx)
            }
            WindowCommand::New { vertical } => {
                self.vim_new(editor, direction(vertical), window, cx);
                Ok(())
            }
            WindowCommand::Only { .. } => self.vim_only(editor, window, cx),
            WindowCommand::Close { .. } => self.vim_close(editor, window, cx),
        };
        if let Err(error) = done {
            message(editor, error, cx);
        }
    }

    /// The current window, `editor`'s pane, made the active one if it
    /// isn't.
    fn vim_window(
        &mut self,
        editor: &Entity<EditorView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<Pane> {
        let pane = match self.tab_of(editor, cx) {
            Some((pane, _)) => pane,
            None => self.active.clone(),
        };
        if pane != self.active {
            self.activate(pane.clone(), window, cx);
        }
        pane
    }

    /// `:sp` and `:vs`: the current pane split in `direction`, the new one
    /// showing `file`, else the same buffer.
    fn vim_split(
        &mut self,
        editor: &Entity<EditorView>,
        direction: Direction,
        file: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let Some(file) = file else {
            self.vim_window(editor, window, cx);
            self.split(direction, window, cx);
            return Ok(());
        };
        let (path, open) = self.vim_path(editor, &file, cx)?;
        let is_file = std::fs::metadata(&path).is_ok_and(|meta| !meta.is_dir());
        if !open && !is_file {
            return Err(tr!("workbench-workbench-vim-e484", file = file.as_str()).into());
        }
        self.vim_window(editor, window, cx);
        let (new, second) = self.split_alone(direction, window, cx);
        new.update(cx, |pane, cx| pane.open(path, window, cx));
        self.drop_second_view(&new, second, window, cx);
        Ok(())
    }

    /// `:new` and `:vne`: the current pane split in `direction`, the new
    /// one showing an untitled empty tab, focused.
    fn vim_new(
        &mut self,
        editor: &Entity<EditorView>,
        direction: Direction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.vim_window(editor, window, cx);
        let (new, second) = self.split_alone(direction, window, cx);
        new.update(cx, |pane, cx| {
            pane.add(EditorModel::new(Buffer::new(""), None), cx);
            pane.activate(pane.tabs().len() - 1, window, cx);
        });
        self.drop_second_view(&new, second, window, cx);
    }

    /// Splits the active pane in `direction` as `pane.split_down` does:
    /// the new pane, and the second view it shows, if the pane split showed
    /// a tab, for [`Workbench::drop_second_view`].
    fn split_alone(
        &mut self,
        direction: Direction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Entity<Pane>, Option<ItemId>) {
        self.split(direction, window, cx);
        let new = self.active.clone();
        let second = new.read(cx).pane().items().first().map(|item| item.id());
        (new, second)
    }

    /// Closes `second`, the second view a split gave `pane`, now that it
    /// shows another tab, or is reading the file it will: it shares its
    /// text with the pane split, so nothing is lost. Kept while the pane
    /// shows it still (`:sp` of the file it shows).
    fn drop_second_view(
        &mut self,
        pane: &Entity<Pane>,
        second: Option<ItemId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = second else {
            return;
        };
        let items = pane.read(cx).pane().items();
        let Some(ix) = items.iter().position(|item| item.id() == id) else {
            return;
        };
        let shown = pane.read(cx).active_item().map(|item| item.id());
        if shown == Some(id) && pane.read(cx).reading().next().is_none() {
            return;
        }
        pane.update(cx, |pane, cx| {
            pane.take_item(ix, window, cx);
            cx.notify();
        });
        self.track_editors(cx);
    }

    /// `:on`: every pane but the current closed, their tabs with changes
    /// moved into it first: see the module docs. `E445` if one couldn't
    /// move, its pane kept.
    fn vim_only(
        &mut self,
        editor: &Entity<EditorView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let keep = self.vim_window(editor, window, cx);
        let others: Vec<_> = (self.panes(cx).into_iter())
            .filter(|pane| *pane != keep)
            .collect();
        if others.is_empty() {
            editor.update(cx, |editor, cx| editor.report_vim_message(ONLY_ONE, cx));
            return Ok(());
        }
        let shown = keep.read(cx).active_item().map(|item| item.id());
        // A tab with changes whose text the current pane shows already
        // closes with its pane: the changes live on there.
        let unkept = |pane: &Entity<Pane>, cx: &Context<Self>| {
            let kept = keep.read(cx).pane().items();
            (pane.read(cx).pane().items().iter())
                .filter(|item| item.is_dirty(cx))
                .filter(|item| !kept.iter().any(|kept| kept.shares_document(&***item, cx)))
                .map(|item| item.id())
                .collect::<Vec<_>>()
        };
        let mut refused = false;
        for pane in others {
            for id in unkept(&pane, cx) {
                let into = Destination::Pane {
                    pane: keep.clone(),
                    index: None,
                };
                (self.workspace).update(cx, |workspace, cx| {
                    workspace.move_tab(&pane, id, into, window, cx)
                });
            }
            if !self.panes(cx).contains(&pane) {
                // The move emptied it.
                continue;
            }
            // What the moves take never fails to move into a pane of the
            // group, but a pane left with changes isn't closed unasked.
            if !unkept(&pane, cx).is_empty() {
                refused = true;
                continue;
            }
            (self.workspace).update(cx, |workspace, cx| workspace.close_pane(&pane, window, cx));
        }
        let ix = (keep.read(cx).pane().items().iter()).position(|item| Some(item.id()) == shown);
        self.activate(keep.clone(), window, cx);
        if let Some(ix) = ix {
            keep.update(cx, |pane, cx| pane.activate(ix, window, cx));
        }
        self.track_editors(cx);
        match refused {
            true => Err(String::from(E445)),
            false => Ok(()),
        }
    }

    /// `:clo`: the current pane closed as `pane.close` closes it, unless
    /// it's the last (`E444`).
    fn vim_close(
        &mut self,
        editor: &Entity<EditorView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if self.panes(cx).len() < 2 {
            return Err(String::from(E444));
        }
        self.vim_window(editor, window, cx);
        self.close_active_pane(window, cx);
        Ok(())
    }
}

#[cfg(test)]
#[path = "vim_windows_tests.rs"]
mod tests;
