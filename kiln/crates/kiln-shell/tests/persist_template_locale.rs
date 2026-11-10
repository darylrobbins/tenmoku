// SPDX-License-Identifier: Apache-2.0

//! The settings writer resolves its template when it creates the file, not
//! when it is made, so a live locale change reaches it.

use std::{
    fs,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use gpui::TestAppContext;
use kiln_shell::persist::{SettingKey, SettingsPath, setting_writer};

const VISIBLE: SettingKey = &["panel", "visible"];

#[gpui::test]
fn a_file_created_after_a_locale_change_uses_the_new_template(cx: &mut TestAppContext) {
    let dir = std::env::temp_dir().join(format!("kiln-shell-template-locale-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.toml");

    let locale = Arc::new(Mutex::new("# English.\n\n[panel]\n# visible = true\n"));
    let calls = Arc::new(AtomicUsize::new(0));
    let writer = {
        let (locale, calls) = (locale.clone(), calls.clone());
        cx.update(|cx| {
            setting_writer(
                SettingsPath::new(&path),
                move || {
                    calls.fetch_add(1, Ordering::SeqCst);
                    *locale.lock().unwrap()
                },
                cx,
            )
        })
    };
    assert_eq!(calls.load(Ordering::SeqCst), 0, "called when made");

    *locale.lock().unwrap() = "# Fran\u{e7}ais.\n\n[panel]\n# visible = true\n";
    writer.save(VISIBLE, false);
    cx.run_until_parked();

    let text = fs::read_to_string(&path).unwrap();
    assert!(text.starts_with("# Fran\u{e7}ais.\n"), "{text:?}");
    assert!(text.contains("\nvisible = false\n"), "{text:?}");
    let _ = fs::remove_dir_all(&dir);
}
