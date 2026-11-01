//! Files and folders dragged from the file manager onto the window: files
//! open as tabs in the pane under the drop, a folder becomes the project.

use std::{fs, path::PathBuf};

use gpui::{Context, ExternalPaths, Focusable as _, Pixels, Point, Window};

use kiln_shell::WorkspaceEvent;

use crate::Workbench;

/// What a dropped path is.
enum Dropped {
    File(PathBuf),
    Folder(PathBuf),
}

/// `paths` sorted into files and folders, in the order dropped. Anything
/// else (a missing path, a socket) is skipped and logged.
fn classify(paths: &[PathBuf]) -> Vec<Dropped> {
    paths
        .iter()
        .filter_map(|path| match fs::metadata(path) {
            Ok(meta) if meta.is_dir() => Some(Dropped::Folder(path.clone())),
            Ok(meta) if meta.is_file() => Some(Dropped::File(path.clone())),
            Ok(_) => {
                log::warn!(
                    "not opening dropped {}: not a file or folder",
                    path.display()
                );
                None
            }
            Err(error) => {
                log::warn!("not opening dropped {}: {error}", path.display());
                None
            }
        })
        .collect()
}

impl Workbench {
    /// Opens `path` in the active pane, as a tab.
    pub fn open_document(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let pane = self.active.clone();
        pane.update(cx, |pane, cx| pane.open(path, window, cx));
    }

    /// Opens what was dropped at `position`: the first folder as the
    /// project, as Open Folder does, then the files, in order, as tabs of
    /// the pane under `position` (else the active one), the last active and
    /// focused. A file already open there is activated, not reopened.
    pub(crate) fn drop_external(
        &mut self,
        paths: &ExternalPaths,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let dropped = classify(paths.paths());
        let pane = (self.pane_group(cx).panes().into_iter())
            .find(|pane| pane.read(cx).bounds().contains(&position))
            .unwrap_or(&self.active)
            .clone();
        let mut folders = dropped.iter().filter_map(|d| match d {
            Dropped::Folder(path) => Some(path),
            Dropped::File(_) => None,
        });
        if let Some(folder) = folders.next() {
            self.set_root(folder.clone(), window, cx);
        }
        let files: Vec<PathBuf> = dropped
            .into_iter()
            .filter_map(|d| match d {
                Dropped::File(path) => Some(path),
                Dropped::Folder(_) => None,
            })
            .collect();
        if files.is_empty() {
            return;
        }
        self.activate(pane.clone(), window, cx);
        // One at a time, each once read: reads finish in any order, and the
        // tabs follow the order dropped. The batch stops if the pane closes,
        // or a file doesn't land (it failed to read, or something else was
        // opened in the pane meanwhile and took over).
        let (wake, wakes) = async_channel::unbounded();
        let watch_pane = cx.observe(&pane, {
            let wake = wake.clone();
            move |_, _, _| _ = wake.try_send(())
        });
        let watch_workbench = cx.observe_self({
            let wake = wake.clone();
            move |_, _| _ = wake.try_send(())
        });
        // A pane that leaves the workbench is kept alive by the batch, and
        // reads still in flight would land in it and take the focus: they
        // are dropped as it goes.
        let watch_removal = cx.subscribe(&self.workspace, {
            let pane = pane.clone();
            move |_, _, event, cx| {
                if let WorkspaceEvent::PaneRemoved(id) = event
                    && *id == pane.entity_id()
                {
                    pane.update(cx, |pane, cx| pane.abandon_reads(cx));
                    _ = wake.try_send(());
                }
            }
        });
        cx.spawn_in(window, async move |workbench, cx| {
            let _watches = (watch_pane, watch_workbench, watch_removal);
            let in_workbench = |cx: &mut gpui::AsyncWindowContext| {
                workbench
                    .read_with(cx, |workbench, cx| {
                        workbench.pane_group(cx).panes().contains(&&pane)
                    })
                    .unwrap_or(false)
            };
            for path in files {
                if !in_workbench(cx) {
                    return;
                }
                // Another open in the pane, from a later drop or the user,
                // or the user navigating its tabs, supersedes what's left of
                // this batch. This open's own landing is neither.
                let marks = pane.update_in(cx, |pane, window, cx| {
                    pane.open(path.clone(), window, cx);
                    (pane.open_serial(), pane.navigations())
                });
                let Ok(marks) = marks else { return };
                let superseded = |cx: &mut gpui::AsyncWindowContext| {
                    pane.read_with(cx, |pane, _| {
                        (pane.open_serial(), pane.navigations()) != marks
                    })
                };
                loop {
                    if superseded(cx) {
                        return;
                    }
                    let state = pane.read_with(cx, |pane, _| {
                        (pane.position(&path).is_some(), pane.is_opening(&path))
                    });
                    match state {
                        (true, _) | (false, false) => break,
                        (false, true) => {}
                    }
                    if wakes.recv().await.is_err() || !in_workbench(cx) {
                        return;
                    }
                }
                if superseded(cx) || !in_workbench(cx) {
                    return;
                }
                if pane.read_with(cx, |pane, _| pane.position(&path).is_none()) {
                    return;
                }
                // With vim on, the file shown is in normal mode, whatever
                // mode an editor already open was left in.
                let editor = workbench
                    .read_with(cx, |workbench, cx| {
                        let editor = pane.read(cx).active_editor().cloned()?;
                        let abnormal = editor.read(cx).vim_mode().is_some_and(|m| m != "normal");
                        (abnormal && workbench.active == pane).then_some(editor)
                    })
                    .ok()
                    .flatten();
                if let Some(editor) = editor {
                    // Outside any update: the key runs the editor's own.
                    cx.update(|window, cx| {
                        window.focus(&editor.focus_handle(cx), cx);
                        window.dispatch_keystroke(gpui::Keystroke::parse("escape").unwrap(), cx);
                    })
                    .ok();
                }
            }
        })
        .detach();
    }
}

#[cfg(test)]
#[path = "external_drop_tests.rs"]
mod external_drop_tests;
