//! The one test that waits on the file system. Waits are bounded by
//! [`TIMEOUT`], and no duration is asserted.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::mpsc,
    time::Duration,
};

use fe_config::ConfigWatcher;

use super::{keystroke, stack, write};
use crate::{KeyOutcome, KeymapStore, Matcher};

/// An upper bound only, sized for a loaded CI host: FSEvents may take a while.
const TIMEOUT: Duration = Duration::from_secs(60);

const DEFAULTS: &str = "\
[[bindings]]
keys = \"cmd-s\"
command = \"workspace.save\"
";

fn binding(command: &str) -> String {
    format!("[[bindings]]\nkeys = \"cmd-j\"\ncommand = \"{command}\"\n")
}

/// What `cmd-j` runs.
fn cmd_j(matcher: &mut Matcher) -> Option<String> {
    match matcher.push(keystroke("cmd-j"), &stack(&["editor"])) {
        KeyOutcome::Dispatch(invocation) => Some(invocation.command.to_string()),
        _ => None,
    }
}

/// Writes `text` to a temporary file beside `path` and renames it over it.
fn replace(path: &Path, text: &str) {
    let temp = path.with_extension("toml.tmp");
    fs::write(&temp, text).unwrap();
    fs::rename(&temp, path).unwrap();
}

/// Reloads each batch for `path`, feeding the matcher, until `done` holds.
/// One save can arrive in several batches, the first maybe before it
/// finishes.
fn reload_until(
    store: &mut KeymapStore,
    matcher: &mut Matcher,
    batches: &mpsc::Receiver<Vec<PathBuf>>,
    path: &Path,
    done: fn(&KeymapStore) -> bool,
) {
    loop {
        let batch = batches.recv_timeout(TIMEOUT).expect("a batch in time");
        assert_eq!(batch, [path]);
        if let Some(event) = store.reload(&batch) {
            matcher.set_keymap(event.keymap);
        }
        if done(store) {
            return;
        }
    }
}

#[test]
fn hot_reload_swaps_the_keymap() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), "keymap.toml", &binding("editor.join_lines"));
    let mut store = KeymapStore::load(DEFAULTS, Some(path.clone()), None);
    let mut matcher = Matcher::new(store.keymap().clone());
    assert_eq!(cmd_j(&mut matcher).as_deref(), Some("editor.join_lines"));

    let (tx, batches) = mpsc::channel();
    let _watcher = ConfigWatcher::new(store.paths(), move |batch| {
        tx.send(batch).ok();
    })
    .unwrap();
    let first = batches.recv_timeout(TIMEOUT).expect("a batch in time");
    assert_eq!(first, [path.as_path()], "the first batch, before any event");

    // A valid edit changes what the matcher resolves.
    replace(&path, &binding("editor.duplicate_line"));
    reload_until(&mut store, &mut matcher, &batches, &path, |store| {
        store
            .keymap()
            .bindings()
            .iter()
            .any(|binding| binding.command.as_str() == "editor.duplicate_line")
    });
    assert_eq!(
        cmd_j(&mut matcher).as_deref(),
        Some("editor.duplicate_line")
    );
    assert!(store.diagnostics().is_empty(), "{:?}", store.diagnostics());

    // An invalid one keeps the previous bindings, with diagnostics.
    replace(
        &path,
        "[[bindings]]\nkeys = \"cmd-bogus\"\ncommand = \"editor.x\"\n",
    );
    reload_until(&mut store, &mut matcher, &batches, &path, |store| {
        store.diagnostics().iter().any(|d| d.is_error())
    });
    assert_eq!(
        cmd_j(&mut matcher).as_deref(),
        Some("editor.duplicate_line")
    );
    let error = &store.diagnostics()[0];
    assert_eq!((error.line, error.column), (2, 13), "{error}");
}
