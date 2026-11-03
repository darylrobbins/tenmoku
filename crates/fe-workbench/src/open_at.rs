//! Opening a file at a line and column, as the command line asks for one.

use std::path::{Path, PathBuf};

use fe_command::Value;
use fe_text::Text;
use gpui::{Context, Task, Window};
use kiln_shell::WorkspaceEvent;

use crate::{Pane, Workbench, pane::Reveal};

/// A 0-based place in a file, which may be past its end.
#[derive(Clone, Copy)]
struct Place {
    row: u32,
    /// In chars, as Go to Line counts them.
    column: u32,
}

impl Place {
    /// The place 1-based `line` and `col` name; no line names none.
    fn new(line: Option<u32>, col: Option<u32>) -> Option<Self> {
        Some(Self {
            row: line?.saturating_sub(1),
            column: col.unwrap_or(1).saturating_sub(1),
        })
    }

    /// The place's offset in `text`, clamped to the last row and to the
    /// row's end.
    fn offset(self, text: &Text) -> usize {
        let row = self.row.min(text.row_count() - 1);
        let start = text.row_start(row);
        let row_text = text.slice_to_string(start..start + text.row_len(row) as usize);
        let at = (row_text.char_indices().nth(self.column as usize)).map(|(at, _)| at);
        start + at.unwrap_or(row_text.len())
    }

    /// Puts tab `ix` at the place, whether or not it's shown. A viewer takes
    /// the column in bytes, and clamps the rows as its index finds them.
    fn apply(self, pane: &mut Pane, ix: usize, window: &mut Window, cx: &mut Context<Pane>) {
        let tab = pane.tabs()[ix].clone();
        if let Some(editor) = tab.editor() {
            let offset = self.offset(editor.read(cx).model().text());
            if tab.preview().is_some() {
                // The pane knows a place in the text shows the text.
                let path = tab.path.clone().expect("an editor in preview has a file");
                let reveal = Some(Reveal::bytes(offset..offset));
                pane.open_at(path, reveal, window, cx);
            } else {
                editor.update(cx, |editor, cx| {
                    editor.select_and_reveal(offset..offset, cx);
                });
            }
        } else if tab.viewer().is_some() {
            let state = Value::from_iter([
                ("row", Value::from(self.row)),
                ("column", Value::from(self.column as usize)),
                ("within_row", Value::from(true)),
            ]);
            tab.item().apply_state(&state, window, cx);
        }
    }
}

/// Finishes opening `path` if its read is over, the host's launch read
/// (`launch`) being one the pane can't tell has ended once it has shown
/// something else: at `place` if it's in a tab. Whether it is over. The tab
/// may have landed behind another, which stays.
fn land(
    pane: &mut Pane,
    path: &Path,
    place: Option<Place>,
    launch: bool,
    window: &mut Window,
    cx: &mut Context<Pane>,
) -> bool {
    match pane.position(path) {
        Some(ix) => {
            if let Some(place) = place {
                place.apply(pane, ix, window, cx);
            }
            true
        }
        None => !launch && !pane.reading().any(|reading| reading == path),
    }
}

impl Workbench {
    /// Opens `path` in the active pane as [`Workbench::open_document`] does,
    /// then puts a single caret on 1-based `line` and `col` (its start with
    /// no `col`, untouched with no `line`), clamped to the text and scrolled
    /// into view. The launch file the host is still reading isn't read again.
    ///
    /// The task ends once the file is in its tab or has failed to open, so
    /// that opening files one after another keeps their tabs in order. It
    /// does its work whether or not it's awaited, as long as it isn't
    /// dropped.
    pub fn open_document_at(
        &mut self,
        path: PathBuf,
        line: Option<u32>,
        col: Option<u32>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        let place = Place::new(line, col);
        if place.is_some() {
            // As a jump does: a running go-to-definition mustn't land over
            // this one.
            self.nav_state.goto.next();
            self.push_history(cx);
        }
        // The pane showing the host's read of the launch file, if this is it.
        let launch_pane = (self.launch_reading(cx) == Some(path.as_path()))
            .then(|| {
                let panes = self.pane_group(cx).panes();
                let pane = panes
                    .into_iter()
                    .find(|pane| pane.read(cx).is_opening(&path));
                pane.cloned()
            })
            .flatten();
        let launch = launch_pane.is_some();
        let pane = launch_pane.unwrap_or_else(|| self.pane().clone());
        let landed = pane.update(cx, |pane, cx| {
            if !launch {
                pane.open(path.clone(), window, cx);
            }
            land(pane, &path, place, launch, window, cx)
        });
        if landed {
            return Task::ready(());
        }
        let (wake, wakes) = async_channel::unbounded();
        let watch_pane = cx.observe(&pane, {
            let wake = wake.clone();
            move |_, _, _| _ = wake.try_send(())
        });
        // A pane that leaves the workbench is kept alive here, and has
        // dropped its reads: nothing more would come of it.
        let id = pane.entity_id();
        let watch_removal = cx.subscribe(self.workspace(), move |_, _, event, _| {
            if matches!(event, WorkspaceEvent::PaneRemoved(removed) if *removed == id) {
                _ = wake.try_send(());
            }
        });
        cx.spawn_in(window, async move |workbench, cx| {
            let _watches = (watch_pane, watch_removal);
            loop {
                if wakes.recv().await.is_err() {
                    return;
                }
                let state = workbench.read_with(cx, |workbench, cx| {
                    (
                        workbench.pane_group(cx).panes().contains(&&pane),
                        workbench.launch_reading(cx) == Some(path.as_path()),
                    )
                });
                let Ok((true, launch)) = state else { return };
                let landed = pane.update_in(cx, |pane, window, cx| {
                    land(pane, &path, place, launch, window, cx)
                });
                if landed.unwrap_or(true) {
                    return;
                }
            }
        })
    }
}

#[cfg(test)]
#[path = "open_at_tests.rs"]
mod tests;
