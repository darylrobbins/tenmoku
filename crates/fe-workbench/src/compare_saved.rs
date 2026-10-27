//! Compare Active File with Saved: `diff.compare_saved` (`cmd-k d`), as
//! VS Code's `workbench.files.action.compareWithSaved`. The active file's
//! text on disk, read on the background executor, is the read-only left
//! side `Saved` of a diff view against the open buffer, `Working Tree`; see
//! [`Workbench::finish_saved_diff`]. The left side is a snapshot of the
//! moment it was asked for, taking the buffer's text as that is saved. No repository is needed. An untitled buffer
//! answers `E32: No file name`, a file gone from disk that it's missing,
//! and neither opens a tab.

use std::path::Path;

use crate::workbench::toast::{self, Toast};
use fe_buffer::Buffer;
use gpui::{AppContext as _, Context, Window};

use super::Workbench;
use crate::loader;

/// What a command for the active file answers when it has none.
const NO_FILE: &str = "E32: No file name";

/// What a file that's not on disk answers, before its path.
const MISSING: &str = "File missing";

/// The file at `path` as it is on disk: none if it's missing, an answer if
/// it can't be read or is too large to compare.
fn read_saved(path: &Path) -> Result<Option<Buffer>, String> {
    if loader::is_large(path) {
        return Err(format!("Too large to compare: {}", path.display()));
    }
    loader::read(path.to_owned()).map_err(|error| format!("Can't open {}: {error}", path.display()))
}

impl Workbench {
    /// Opens the active file's saved text against its buffer.
    pub(super) fn compare_saved(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ask = match self.begin_saved_diff(window, cx) {
            None => return toast::show(cx, Toast::info(NO_FILE)),
            Some(None) => return,
            Some(Some(ask)) => ask,
        };
        let at = ask.path().to_owned();
        let read = cx.background_spawn(async move { read_saved(&at) });
        cx.spawn_in(window, async move |workbench, cx| {
            let read = read.await;
            workbench
                .update_in(cx, |workbench, window, cx| match read {
                    Ok(Some(saved)) => workbench.finish_saved_diff(ask, saved, window, cx),
                    Ok(None) => {
                        let missing = format!("{MISSING}: {}", ask.path().display());
                        toast::show(cx, Toast::info(missing))
                    }
                    Err(answer) => toast::show(cx, Toast::info(answer)),
                })
                .ok();
        })
        .detach();
    }
}
