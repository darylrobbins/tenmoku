//! The queue the tab's keymap edits are written through: in the order they
//! were made, per file, by a worker that outlives the tab. A test holds
//! the first edit up in the worker on a gate, which leaves other tasks
//! free to run, as a slow disk does.

use std::{fs, path::PathBuf, sync::Arc};

use fe_keymap::{KEYMAP_FILE, KEYMAP_HEADER, KeymapEdit};
use gpui::{AppContext as _, Entity, TestAppContext, VisualTestContext};

use super::{BeforeWrite, ShortcutsEditor, queue_write};
use crate::{Launch, open::ConfigDirOverride};

const COMMAND: &str = "workbench.open_settings_ui";
const BOUND: &str = "[[bindings]]\nkeys = \"alt-9\"\ncommand = \"workbench.open_settings_ui\"\n";

fn add() -> KeymapEdit {
    KeymapEdit::Add {
        keys: "alt-9".into(),
        context: None,
        command: COMMAND.into(),
        args: None,
    }
}

fn remove() -> KeymapEdit {
    KeymapEdit::Remove {
        keys: "alt-9".into(),
        context: None,
    }
}

fn reset() -> KeymapEdit {
    KeymapEdit::ResetCommand {
        command: COMMAND.into(),
    }
}

/// A keymap holding the `alt-9` binding, and a gate each Remove waits on
/// in the worker until the returned sender is dropped or sent to.
fn keymap(cx: &mut TestAppContext) -> (tempfile::TempDir, PathBuf, async_channel::Sender<()>) {
    let dir = tempfile::tempdir().unwrap();
    let config = dunce::canonicalize(dir.path()).unwrap();
    let path = config.join(KEYMAP_FILE);
    fs::write(&path, format!("{KEYMAP_HEADER}\n{BOUND}")).unwrap();
    let (release, gate) = async_channel::bounded(1);
    cx.update(|cx| {
        cx.set_global(ConfigDirOverride(Some(config)));
        cx.set_global(BeforeWrite(Arc::new(move |edit| {
            matches!(edit, KeymapEdit::Remove { .. }).then(|| gate.clone())
        })));
    });
    (dir, path, release)
}

#[gpui::test]
fn delayed_remove_then_reset_lands_in_order(cx: &mut TestAppContext) {
    let (_dir, path, release) = keymap(cx);
    let before = fs::read_to_string(&path).unwrap();
    let (first, second) = cx.update(|cx| {
        (
            queue_write(path.clone(), Vec::new(), remove(), None, cx),
            queue_write(path.clone(), Vec::new(), reset(), None, cx),
        )
    });
    // The Remove is held up; the Reset made after it waits behind it.
    cx.run_until_parked();
    assert_eq!(fs::read_to_string(&path).unwrap(), before);
    assert!(first.try_recv().is_err() && second.try_recv().is_err());

    release.try_send(()).unwrap();
    cx.run_until_parked();
    // A Reset first would leave nothing for the Remove to unbind.
    assert!(first.try_recv().unwrap().is_ok());
    assert!(second.try_recv().unwrap().is_ok());
    let text = fs::read_to_string(&path).unwrap();
    assert!(!text.contains("alt-9"), "{text}");
}

/// A Keyboard Shortcuts tab in a workbench whose config is `path`'s.
fn tab<'a>(
    cx: &'a mut TestAppContext,
    path: &std::path::Path,
) -> (Entity<ShortcutsEditor>, &'a mut VisualTestContext) {
    let (_workbench, cx, _) = crate::tests::open(
        cx,
        Launch {
            root: None,
            file: None,
        },
    );
    let config = path.parent().unwrap().to_owned();
    let editor = cx.update(|_, cx| {
        cx.set_global(ConfigDirOverride(Some(config)));
        cx.new(ShortcutsEditor::new)
    });
    (editor, cx)
}

#[gpui::test]
fn write_survives_tab_close_and_orders_with_next_tab(cx: &mut TestAppContext) {
    let (dir, path, release) = keymap(cx);
    let (first, cx) = tab(cx, &path);
    // The first tab's Remove is held up in the worker, and its tab closes.
    first.update(cx, |tab, cx| tab.edit(remove(), cx));
    cx.run_until_parked();
    drop(first);
    cx.run_until_parked();
    assert!(fs::read_to_string(&path).unwrap().contains("alt-9"));

    // A later tab's Add waits behind it, so the binding is back at the end.
    let second = cx.update(|_, cx| cx.new(ShortcutsEditor::new));
    second.update(cx, |tab, cx| tab.edit(add(), cx));
    cx.run_until_parked();
    assert!(fs::read_to_string(&path).unwrap().contains("alt-9"));

    release.try_send(()).unwrap();
    cx.run_until_parked();
    let text = fs::read_to_string(&path).unwrap();
    assert_eq!(text.matches("alt-9").count(), 1, "{text}");
    // Out of order, the Add would have landed first and the Remove then
    // deleted it.
    let user = &text[KEYMAP_HEADER.len()..];
    assert!(user.contains(COMMAND), "{text}");
    drop(dir);
}
