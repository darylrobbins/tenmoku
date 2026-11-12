//! Vim's tab commands over the active pane's editor tabs, as VSCodeVim
//! maps them: `:tabnew`, `:tabn`, `:tabp`, `:tabc`, `:tabo`, `:tabfirst`
//! and `:tablast` ([`TabCommand`]). A fastedit tab stands in for a buffer
//! in a tab page; real tab pages, a window layout each, aren't modelled,
//! so these run the existing tab commands, with no tab model of their own.
//! The pane is the one of the editor the line was typed in, or for an
//! editor shown outside the tabs (a diff view's side, the commit box) the
//! active pane; other panes' tabs are untouched.
//!
//! - `:tabn` and `:tabp` (with counts) activate over the pane's tabs in
//!   strip order, wrapping, as `workbench.next_tab` and `previous_tab` do;
//!   `:tabn N` goes to tab `N`, from 1 (the last if there are fewer), and
//!   `:tabfirst` and `:tablast` to the ends.
//! - `:tabnew` opens an untitled tab after the active one and activates
//!   it. `:tabnew {file}` reads the file as `:e` does, a tab shown if
//!   it's open already; a file not open yet gets its tab after the active
//!   one, once it's read.
//! - `:tabc` closes the active tab as `workbench.close_tab` does: one with
//!   changes stays open, unless another tab shows its text. `:tabc!`
//!   discards it.
//! - `:tabo` closes the pane's other tabs, leaving those with changes in
//!   place (unless another tab shows their text) with a message; `:tabo!`
//!   closes them all.
//!
//! With one tab, `:tabo` says "Already only one tab page"; the others have
//! nothing to move to and change nothing.

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use fe_gpui::EditorView;
use fe_vim::TabCommand;
use gpui::{Context, Entity, Window};
use kiln_shell::PaneView as _;

use super::{Said, message};
use crate::{Pane, Workbench};
use kiln_l10n::tr;

const E445: Said = Said(|| tr!("workbench-workbench-vim-e445-tab"));
const ONLY_ONE: Said = Said(|| tr!("workbench-workbench-vim-only-tab"));

impl Workbench {
    /// Runs `command`, typed in `editor`: see the module docs. Every
    /// answer is `editor`'s vim message.
    pub(super) fn run_vim_tab_command(
        &mut self,
        editor: &Entity<EditorView>,
        command: TabCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pane = match self.tab_of(editor, cx) {
            Some((pane, _)) => pane,
            None => self.active.clone(),
        };
        if pane != self.active {
            self.activate(pane.clone(), window, cx);
        }
        let done = match command {
            TabCommand::New { file: None } => {
                pane.update(cx, |pane, cx| new_tab(pane, window, cx));
                Ok(())
            }
            TabCommand::New { file: Some(file) } => match self.vim_path(editor, &file, cx) {
                Ok((path, _)) => {
                    open_after_active(&pane, path, window, cx);
                    Ok(())
                }
                Err(error) => Err(error),
            },
            TabCommand::Next(None) => cycle(&pane, false, 1, window, cx),
            TabCommand::Previous(count) => cycle(&pane, true, count, window, cx),
            TabCommand::Next(Some(number)) => {
                go(&pane, window, cx, |len| (number as usize).min(len) - 1)
            }
            TabCommand::First => go(&pane, window, cx, |_| 0),
            TabCommand::Last => go(&pane, window, cx, |len| len - 1),
            TabCommand::Close { force: false } => {
                pane.update(cx, |pane, cx| {
                    pane.close_active(window, cx);
                    cx.notify();
                });
                Ok(())
            }
            TabCommand::Close { force: true } => {
                if let Some(ix) = pane.read(cx).active() {
                    pane.update(cx, |pane, cx| {
                        pane.take_item(ix, window, cx);
                        cx.notify();
                    });
                    self.track_editors(cx);
                }
                Ok(())
            }
            TabCommand::Only { force } => self.vim_tab_only(&pane, force, window, cx),
        };
        if let Err(error) = done {
            message(editor, error, cx);
        }
    }

    /// `:tabo`: the tabs of `pane` but the active one closed, those with
    /// changes kept (`E445`) unless `force`, or another tab shows their
    /// text: [`Pane::close_others`], as Close Other Tabs.
    fn vim_tab_only(
        &mut self,
        pane: &Entity<Pane>,
        force: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if pane.read(cx).pane().items().len() < 2 {
            return Err(String::from(ONLY_ONE));
        }
        let kept = pane.update(cx, |pane, cx| {
            let kept = pane.close_others(force, window, cx);
            cx.notify();
            kept
        });
        self.track_editors(cx);
        match kept {
            0 => Ok(()),
            _ => Err(String::from(E445)),
        }
    }
}

/// An untitled empty tab after the active one, shown and focused.
fn new_tab(pane: &mut Pane, window: &mut Window, cx: &mut Context<Pane>) {
    let after = pane.active().map_or(0, |ix| ix + 1);
    pane.add(EditorModel::new(Buffer::new(""), None), cx);
    let added = pane.tabs().len() - 1;
    pane.reorder_item(added, Some(after), window, cx);
    cx.notify();
}

/// Opens `path` in `pane` as `:e` does, its tab, if it needs a new one,
/// placed after the tab active now once the file is read.
fn open_after_active(
    pane: &Entity<Pane>,
    path: std::path::PathBuf,
    window: &mut Window,
    cx: &mut Context<Workbench>,
) {
    let after = pane.read(cx).active().map_or(0, |ix| ix + 1);
    pane.update(cx, |pane, cx| pane.open_after(path, after, window, cx));
}

/// Activates `count` tabs from the active one, wrapping, as the tab
/// commands do one at a time.
fn cycle(
    pane: &Entity<Pane>,
    back: bool,
    count: u32,
    window: &mut Window,
    cx: &mut Context<Workbench>,
) -> Result<(), String> {
    let len = pane.read(cx).tabs().len();
    if len == 0 {
        return Ok(());
    }
    pane.update(cx, |pane, cx| {
        match count as usize % len {
            // A whole turn: back where it began, activating the tab again.
            0 => {
                if let Some(ix) = pane.active() {
                    pane.activate(ix, window, cx);
                }
            }
            turns => (0..turns).for_each(|_| pane.cycle(back, window, cx)),
        }
        cx.notify();
    });
    Ok(())
}

/// Activates the tab `pick` chooses of the pane's `len` tabs.
fn go(
    pane: &Entity<Pane>,
    window: &mut Window,
    cx: &mut Context<Workbench>,
    pick: impl FnOnce(usize) -> usize,
) -> Result<(), String> {
    let len = pane.read(cx).tabs().len();
    if len > 0 {
        let ix = pick(len);
        pane.update(cx, |pane, cx| {
            pane.activate(ix, window, cx);
            cx.notify();
        });
    }
    Ok(())
}

#[cfg(test)]
#[path = "vim_tabs_tests.rs"]
mod tests;
