//! Open Editor at Index 1 to 9 and Last Editor in Group: VS Code's
//! `workbench.action.openEditorAtIndex1` to `9` and `lastEditorInGroup`,
//! over the active pane's tabs, a preview tab counting as one and staying
//! a preview. A tab that doesn't exist is nothing to do.
//!
//! The commands are declared in `workbench_commands.rs`'s one
//! `#[kiln::command]` block on [`Workbench`] (a type has only one). A typed
//! command takes no args, so rather than one `workbench.open_editor_at_index`
//! with an `index` arg there are nine ids, `workbench.open_editor_at_index1`
//! to `9` (kiln's snake case wants no underscore before the digit), as VS
//! Code has them.

use kiln::Cx;

use crate::Workbench;

impl Workbench {
    /// Activates the active pane's tab at `ix`, or its last with `None`.
    pub(crate) fn open_editor_at(&mut self, ix: Option<usize>, cx: &mut Cx<Self>) {
        self.on_active_pane(cx, |pane, window, cx| {
            let Some(last) = pane.tabs().len().checked_sub(1) else {
                return;
            };
            pane.activate_at(ix.unwrap_or(last), window, cx);
        });
    }
}
