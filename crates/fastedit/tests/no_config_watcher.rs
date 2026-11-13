//! Under gpui's test scheduler `kiln::init` starts no `config-watcher`
//! thread, which on Windows reported activity mid-test and tripped the
//! scheduler's determinism check. Linux-only: it reads `/proc`.

#[cfg(target_os = "linux")]
#[gpui::test]
fn init_without_watch_with_starts_no_config_watcher(cx: &mut gpui::TestAppContext) {
    let home = std::env::temp_dir().join(format!("no-config-watcher-{}", std::process::id()));
    std::fs::create_dir_all(&home).unwrap();
    let file = home.join("settings.toml");
    std::fs::write(&file, "").unwrap();
    cx.update(|cx| {
        let config = kiln::Config::new("dev.fastedit.test", "fastedit")
            .settings_store(fe_config::SettingsStore::load(Some(file.clone()), None).into_inner());
        let _ = kiln::init(cx, config);
    });
    let names: Vec<String> = std::fs::read_dir("/proc/self/task")
        .unwrap()
        .filter_map(|t| std::fs::read_to_string(t.unwrap().path().join("comm")).ok())
        .map(|n| n.trim().to_owned())
        .collect();
    std::fs::remove_dir_all(&home).ok();
    // `comm` is cut to 15 bytes, and a new thread carries its parent's name
    // until it sets its own, so the inotify thread notify starts for the
    // watcher is checked too.
    let watcher = |n: &String| n == "config-watcher" || n.starts_with("notify-rs");
    assert!(!names.iter().any(watcher), "{names:?}");
}
