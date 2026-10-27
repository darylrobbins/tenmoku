//! Save All and Save without Formatting, VS Code's
//! `workbench.action.files.saveAll` and `saveWithoutFormatting`.
//!
//! `Workbench`'s one `#[kiln::command]` block is `workbench_commands.rs`'s,
//! so the commands are declared on [`SaveCommands`], which no element
//! attaches, and [`init`]'s listeners run them on the window's workbench, as
//! `path_commands::init`'s do.

use std::collections::HashSet;

use fe_editor::auto_save::SaveReason;
use fe_gpui::pane_item::{file_name, save_declined};
use gpui::{App, Context, Task};
use kiln::{Cx, platform::command_actions::dispatch_window};

use crate::{
    Workbench,
    workbench::toast::{self, Toast},
};

/// The commands' declaration: see the module's docs. They're handled by
/// [`init`]'s listeners, not here.
#[allow(dead_code)]
struct SaveCommands;

#[kiln::command(context = "workbench")]
#[allow(dead_code)]
impl SaveCommands {
    /// Save All
    #[command(id = "workbench.save_all")]
    fn save_all(&mut self, _: &mut Cx<Self>) {}

    /// Save without Formatting
    #[command(id = "workbench.save_without_formatting")]
    fn save_without_formatting(&mut self, _: &mut Cx<Self>) {}
}

impl Workbench {
    /// Saves every dirty file in the window, each buffer once, as an explicit
    /// save (format on save, save fixes and the changed-on-disk prompt as
    /// for `editor.save`). Untitled buffers and read-only viewers are
    /// skipped. A failed save keeps its editor's state and doesn't stop the
    /// others. The task is over when every save is.
    pub(crate) fn save_all(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        let mut seen = HashSet::new();
        let mut saves = Vec::new();
        for editor in self.file_editors(cx) {
            let saving = editor.update(cx, |view, cx| {
                let model = view.model();
                if model.path().is_none() || view.is_read_only() || !model.is_dirty() {
                    return None;
                }
                if !seen.insert(model.buffer().id()) {
                    return None;
                }
                Some(view.save_with(SaveReason::Explicit, window, cx))
            });
            let name = file_name(editor.read(cx).model().path());
            saves.extend(saving.map(|save| (name, save)));
        }
        cx.spawn(async move |_, cx| {
            for (name, save) in saves {
                if let Err(error) = save.await {
                    report(&name, &*error, cx);
                }
            }
        })
    }

    /// Saves the active editor's text as it is: no format, trim, final
    /// newline or code actions on save.
    pub(crate) fn save_without_formatting(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        let Some(editor) = self.active_editor(cx).cloned() else {
            return Task::ready(());
        };
        let name = file_name(editor.read(cx).model().path());
        let save = editor.update(cx, |view, cx| {
            (view.model().path().is_some() && !view.is_read_only())
                .then(|| view.save_with(SaveReason::ExplicitRaw, window, cx))
        });
        cx.spawn(async move |_, cx| {
            if let Some(Err(error)) = match save {
                Some(save) => Some(save.await),
                None => None,
            } {
                report(&name, &*error, cx);
            }
        })
    }
}

/// Shows why `name` wasn't saved, unless the person declined to overwrite
/// a file that changed on disk ([`save_declined`]), which the editor keeps
/// as it was.
fn report(name: &str, error: &(dyn std::error::Error + 'static), cx: &mut gpui::AsyncApp) {
    if save_declined(error) {
        return;
    }
    log::warn!("can't save {name}: {error}");
    cx.update(|cx| {
        toast::show(
            cx,
            Toast::error(format!("Can't save {name}")).detail(error.to_string()),
        );
    });
}

/// Runs `run` on the workbench of the window the command was dispatched in,
/// after the dispatch, which has the window out.
fn on_workbench(
    cx: &mut App,
    run: impl FnOnce(&mut Workbench, &mut gpui::Window, &mut Context<Workbench>) + 'static,
) {
    let Some(handle) = dispatch_window(cx).or_else(|| cx.active_window()) else {
        return;
    };
    cx.defer(move |cx| {
        handle
            .update(cx, |_, window, cx| {
                if let Some(Some(workbench)) = window.root::<Workbench>() {
                    workbench.update(cx, |workbench, cx| run(workbench, window, cx));
                }
            })
            .ok();
    });
}

/// Handles the commands.
pub(crate) fn init(cx: &mut App) {
    cx.on_action(|_: &SaveAll, cx| {
        on_workbench(cx, |workbench, window, cx| {
            workbench.save_all(window, cx).detach();
        });
    });
    cx.on_action(|_: &SaveWithoutFormatting, cx| {
        on_workbench(cx, |workbench, window, cx| {
            workbench.save_without_formatting(window, cx).detach();
        });
    });
}
