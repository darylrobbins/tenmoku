// SPDX-License-Identifier: Apache-2.0

//! The background writer: each key's last value wins, writes run one at a
//! time off the main thread, quitting waits for them, and boolean settings
//! go into a settings file kept as it was otherwise. Generalised from the
//! editor's toggle-saving tests.

use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use gpui::TestAppContext;
use kiln_shell::persist::{SettingKey, SettingsPath, Writer, setting_writer};

/// An empty directory of the test's own, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("kiln-shell-persist-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[gpui::test]
fn each_keys_last_value_is_written(cx: &mut TestAppContext) {
    let written = Arc::new(Mutex::new(Vec::new()));
    let writer = cx.update(|cx| {
        let written = written.clone();
        Writer::new(cx, move |key: &&str, value: u32| {
            written.lock().unwrap().push((*key, value))
        })
    });
    writer.save("a", 1);
    writer.save("b", 1);
    writer.save("a", 2);
    let pending = writer.queue.lock().unwrap().pending.clone();
    assert_eq!(pending, [("b", 1), ("a", 2)], "a's last value, moved last");
    assert!(writer.writer.borrow().is_some(), "a writer is running");
    cx.run_until_parked();
    assert_eq!(*written.lock().unwrap(), [("b", 1), ("a", 2)]);
    assert!(writer.queue.lock().unwrap().pending.is_empty());

    // Once it's done, another save starts another writer.
    writer.save("a", 3);
    cx.run_until_parked();
    assert_eq!(written.lock().unwrap().last(), Some(&("a", 3)));
}

#[gpui::test]
fn quitting_waits_for_the_writes(cx: &mut TestAppContext) {
    let written = Arc::new(Mutex::new(Vec::new()));
    let writer = cx.update(|cx| {
        let written = written.clone();
        let writer = Writer::new(cx, move |key: &&str, value: bool| {
            written.lock().unwrap().push((*key, value))
        });
        writer.wait_at_quit(cx);
        writer
    });
    writer.save("on", true);
    assert!(written.lock().unwrap().is_empty(), "not written yet");
    cx.quit();
    assert_eq!(*written.lock().unwrap(), [("on", true)]);
}

const TEMPLATE: &str = "# Settings.\n\n[panel]\n# visible = true\n";

const VISIBLE: SettingKey = &["panel", "visible"];

#[gpui::test]
fn settings_are_written_in_place(cx: &mut TestAppContext) {
    let scratch = Scratch::new("in-place");
    let (first, second) = (scratch.0.join("first.toml"), scratch.0.join("second.toml"));
    let path = SettingsPath::new(&first);
    let writer = cx.update(|cx| setting_writer(path.clone(), || TEMPLATE, cx));

    // A missing file starts from the template, the default's comment
    // becoming the setting.
    writer.save(VISIBLE, false);
    cx.run_until_parked();
    let text = fs::read_to_string(&first).unwrap();
    assert!(text.starts_with("# Settings.\n"), "{text:?}");
    assert!(text.contains("\nvisible = false\n"), "{text:?}");
    assert!(!text.contains("# visible = true"), "{text:?}");

    // An existing file keeps the rest of what's in it.
    fs::write(&second, "# Mine.\n[panel]\nwidth = 3\n").unwrap();
    path.set(&second);
    assert_eq!(path.get(), second);
    writer.save(VISIBLE, true);
    cx.run_until_parked();
    let text = fs::read_to_string(&second).unwrap();
    assert!(text.starts_with("# Mine.\n[panel]\n"), "{text:?}");
    assert!(text.contains("width = 3\n"), "{text:?}");
    assert!(text.contains("visible = true\n"), "{text:?}");

    // A file that doesn't parse is left as it is.
    fs::write(&second, "[panel\n").unwrap();
    writer.save(VISIBLE, false);
    cx.run_until_parked();
    assert_eq!(fs::read_to_string(&second).unwrap(), "[panel\n");
}
