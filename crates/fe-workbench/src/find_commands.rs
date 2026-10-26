//! The pane's typed commands: the find bar's, one `#[kiln::command]` block
//! on [`Pane`], whose handlers [`Pane`]'s root element attaches. Each
//! method keeps its command's ID, title and context from the `fe-command`
//! registry it moved from, so keymaps and menus still name it, and runs
//! [`Pane::run_find`] with its op.
//!
//! The block's context is `pane`, not the find bar's: attaching it sets it
//! on the pane's root, where `find_bar` would put the whole pane, editor and
//! all, in the find bar's keys (`escape` would close it, not collapse the
//! editor's selections). The bar's element sets `find_bar`, as before, and
//! each command gives its own context.
//!
//! The terminal panel's find row runs the same commands
//! ([`handle_in`]).

use gpui::{App, Context, InteractiveElement, Window};
use kiln::{
    Cx,
    platform::command_actions::{dispatch_window, typed_command},
};

use crate::{Pane, find_bar::FindOp};

#[kiln::command(context = "pane")]
impl Pane {
    /// Find
    #[command(id = "find.show", context = "editor")]
    pub(crate) fn show_find(&mut self, cx: &mut Cx<Self>) {
        self.find_command(FindOp::Show, cx);
    }

    // Anywhere in the workbench: from outside a pane, [`init`]'s listener
    // runs it in the active one.
    /// Replace…
    #[command(id = "find.show_replace", context = "workbench")]
    pub(crate) fn show_replace(&mut self, cx: &mut Cx<Self>) {
        self.find_command(FindOp::ShowReplace, cx);
    }

    /// Find Next
    #[command(id = "find.next", context = "editor || find_bar")]
    pub(crate) fn find_next(&mut self, cx: &mut Cx<Self>) {
        self.find_command(FindOp::Next, cx);
    }

    /// Find Previous
    #[command(id = "find.previous", context = "editor || find_bar")]
    pub(crate) fn find_previous(&mut self, cx: &mut Cx<Self>) {
        self.find_command(FindOp::Previous, cx);
    }

    /// Close Find
    #[command(id = "find.close", context = "find_bar")]
    pub(crate) fn close_find(&mut self, cx: &mut Cx<Self>) {
        self.find_command(FindOp::Close, cx);
    }

    /// Match Case
    #[command(id = "find.toggle_case", context = "find_bar")]
    pub(crate) fn toggle_case(&mut self, cx: &mut Cx<Self>) {
        self.find_command(FindOp::ToggleCase, cx);
    }

    /// Match Whole Word
    #[command(id = "find.toggle_whole_word", context = "find_bar")]
    pub(crate) fn toggle_whole_word(&mut self, cx: &mut Cx<Self>) {
        self.find_command(FindOp::ToggleWholeWord, cx);
    }

    /// Use Regular Expression
    #[command(id = "find.toggle_regex", context = "find_bar")]
    pub(crate) fn toggle_regex(&mut self, cx: &mut Cx<Self>) {
        self.find_command(FindOp::ToggleRegex, cx);
    }

    /// Toggle Replace
    #[command(id = "find.toggle_replace", context = "find_bar")]
    pub(crate) fn toggle_replace(&mut self, cx: &mut Cx<Self>) {
        self.find_command(FindOp::ToggleReplace, cx);
    }

    // Not a [`FindOp`]: the terminal's find row, which runs those, has no
    // replace.
    /// Toggle Preserve Case
    #[command(id = "find.toggle_preserve_case", context = "find_bar")]
    pub(crate) fn toggle_preserve_case(&mut self, cx: &mut Cx<Self>) {
        if applies(cx.cx) {
            self.update_find(|bar, _, _| bar.toggle_preserve_case(), cx.cx);
            cx.cx.notify();
        }
    }

    /// Replace
    #[command(id = "find.replace_next", context = "find_bar")]
    pub(crate) fn replace_next(&mut self, cx: &mut Cx<Self>) {
        self.find_command(FindOp::ReplaceNext, cx);
    }

    /// Replace All
    #[command(id = "find.replace_all", context = "find_bar")]
    pub(crate) fn replace_all(&mut self, cx: &mut Cx<Self>) {
        self.find_command(FindOp::ReplaceAll, cx);
    }
}

impl Pane {
    /// Does `op` in the find bar, if the command was invoked where its
    /// context holds.
    fn find_command(&mut self, op: FindOp, cx: &mut Cx<Self>) {
        if applies(cx.cx) {
            self.run_find(op, cx.window, cx.cx);
        }
    }
}

/// Runs `find.show_replace` in the active pane when nothing on focus's
/// path handled it: from a panel, such as the tree or a list, the pane's
/// handler isn't on the way. After the dispatch, which has the window out.
pub(crate) fn init(cx: &mut App) {
    cx.on_action(|_: &ShowReplace, cx| {
        let Some(handle) = dispatch_window(cx).or_else(|| cx.active_window()) else {
            return;
        };
        cx.defer(move |cx| {
            handle
                .update(cx, |_, window, cx| {
                    let Some(Some(workbench)) = window.root::<crate::Workbench>() else {
                        return;
                    };
                    let pane = workbench.read(cx).pane().clone();
                    pane.update(cx, |pane, cx| {
                        pane.run_find(FindOp::ShowReplace, window, cx);
                    });
                })
                .ok();
        });
    });
}

/// Whether the find command running applies where it was invoked, as its
/// `fe-command` registry checked: kiln runs a typed command's handler
/// wherever it's dispatched, so a menu pick of Find with the find bar
/// focused, or of Close Find with the editor, would run it. One run with
/// no invocation, as a raw action, whose key binding checked its context,
/// runs.
fn applies(cx: &App) -> bool {
    let Some(invocation) = kiln::current_invocation(cx) else {
        return true;
    };
    let context = typed_command(&invocation.command, cx).and_then(|command| command.context);
    match context {
        Some(context) if !context.eval(&invocation.context) => {
            log::warn!(
                "{} doesn't apply here: not in {context}",
                invocation.command
            );
            false
        }
        _ => true,
    }
}

/// `element`, handling each find command by running `run` with its op on
/// `cx`'s view where the command applies: for a find row other than the
/// pane's (the terminal panel's), which isn't inside a pane.
pub(crate) fn handle_in<E: InteractiveElement, V: 'static>(
    element: E,
    cx: &Context<V>,
    run: fn(&mut V, FindOp, &mut Window, &mut Context<V>),
) -> E {
    fn handler<A: gpui::Action, V: 'static>(
        op: FindOp,
        cx: &Context<V>,
        run: fn(&mut V, FindOp, &mut Window, &mut Context<V>),
    ) -> impl Fn(&A, &mut Window, &mut App) + 'static {
        let view = cx.weak_entity();
        move |_, window, cx| {
            if applies(cx) {
                view.update(cx, |view, cx| run(view, op, window, cx)).ok();
            }
        }
    }
    element
        .on_action(handler::<ShowFind, V>(FindOp::Show, cx, run))
        .on_action(handler::<ShowReplace, V>(FindOp::ShowReplace, cx, run))
        .on_action(handler::<FindNext, V>(FindOp::Next, cx, run))
        .on_action(handler::<FindPrevious, V>(FindOp::Previous, cx, run))
        .on_action(handler::<CloseFind, V>(FindOp::Close, cx, run))
        .on_action(handler::<ToggleCase, V>(FindOp::ToggleCase, cx, run))
        .on_action(handler::<ToggleWholeWord, V>(
            FindOp::ToggleWholeWord,
            cx,
            run,
        ))
        .on_action(handler::<ToggleRegex, V>(FindOp::ToggleRegex, cx, run))
        .on_action(handler::<ToggleReplace, V>(FindOp::ToggleReplace, cx, run))
        .on_action(handler::<ReplaceNext, V>(FindOp::ReplaceNext, cx, run))
        .on_action(handler::<ReplaceAll, V>(FindOp::ReplaceAll, cx, run))
}
