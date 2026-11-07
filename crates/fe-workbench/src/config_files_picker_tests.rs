//! Open Config File…: the bounded walk of a temp config folder, and the
//! picker over it.

use std::fs;
use std::path::Path;

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::commands::RunCommand;
use gpui::{Focusable as _, TestAppContext, VisualTestContext};

use super::{LIMIT, list_config_files};
use crate::{open::ConfigDirOverride, tests::open_root, workbench::Palette};

/// Writes each of `files`, folders and all, under `dir`.
fn write(dir: &Path, files: &[&str]) {
    for file in files {
        let path = dir.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, format!("{file}\n")).unwrap();
    }
}

/// `list_config_files`' paths, from `dir`, with `/`s.
fn listed(dir: &Path, limit: usize) -> Vec<String> {
    (list_config_files(dir, limit).into_iter())
        .map(|path| {
            let rest = path.strip_prefix(dir).unwrap();
            rest.to_string_lossy().replace('\\', "/")
        })
        .collect()
}

#[test]
fn settings_toml_first_then_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        &[
            "vimrc",
            "keymap.toml",
            "settings.toml",
            "themes/dusk.toml",
            "snippets/rust.json",
            "a.txt",
            // Only the top's is the settings file.
            "themes/settings.toml",
        ],
    );
    assert_eq!(
        listed(dir.path(), LIMIT),
        [
            "settings.toml",
            "a.txt",
            "keymap.toml",
            "snippets/rust.json",
            "themes/dusk.toml",
            "themes/settings.toml",
            "vimrc",
        ]
    );
}

#[test]
fn skips_dotfiles_and_cache_folders() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        &[
            "settings.toml",
            ".hidden",
            ".git/config",
            "cache/grammars.bin",
            "state/recent.toml",
            "target/debug/out",
            "plugins/.local",
            "plugins/one/plugin.toml",
            // Three folders deep is the deepest.
            "a/b/c/deep.toml",
            "a/b/c/d/deeper.toml",
        ],
    );
    assert_eq!(
        listed(dir.path(), LIMIT),
        [
            "settings.toml",
            "a/b/c/deep.toml",
            "plugins/one/plugin.toml"
        ]
    );
    // A folder that isn't there has nothing in it.
    assert_eq!(
        listed(&dir.path().join("missing"), LIMIT),
        Vec::<String>::new()
    );
}

#[test]
fn stops_at_the_limit() {
    let dir = tempfile::tempdir().unwrap();
    let names: Vec<String> = (0..20).map(|ix| format!("f{ix:02}.toml")).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    write(dir.path(), &names);
    write(dir.path(), &["settings.toml"]);
    // `settings.toml` is taken first, then as many entries as are left.
    let first = listed(dir.path(), 5);
    assert_eq!(first.len(), 5, "{first:?}");
    assert_eq!(first[0], "settings.toml");
    assert!(
        first[1..].iter().all(|name| names.contains(&name.as_str())),
        "{first:?}"
    );
    assert_eq!(listed(dir.path(), 21).len(), 21);
    // Nothing is read with no entries to take.
    assert_eq!(listed(dir.path(), 0), Vec::<String>::new());

    // A folder, gone into or not, takes an entry: with the top read whole,
    // none is left for `sub`'s file.
    write(dir.path(), &["sub/inner.toml"]);
    assert_eq!(listed(dir.path(), 22).len(), 21);
    assert_eq!(listed(dir.path(), 23).len(), 22);

    // So do the entries left out: a folder of dotfiles uses up the limit.
    let hidden = tempfile::tempdir().unwrap();
    let dots: Vec<String> = (0..50).map(|ix| format!(".h{ix:02}")).collect();
    let dots: Vec<&str> = dots.iter().map(String::as_str).collect();
    write(hidden.path(), &dots);
    write(hidden.path(), &["visible.toml"]);
    assert!(listed(hidden.path(), 10).len() <= 1);
    assert_eq!(listed(hidden.path(), 51), ["visible.toml"]);
}

#[gpui::test]
fn the_picker_lists_and_opens_them(cx: &mut TestAppContext) {
    let config = tempfile::tempdir().unwrap();
    write(config.path(), &["settings.toml", "vimrc"]);
    let root = tempfile::tempdir().unwrap();
    let (workbench, cx) = open_root(cx, root.path());
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&workbench.focus_handle(cx), cx);
        cx.set_global(ConfigDirOverride(Some(config.path().to_owned())));
    });
    let context = ContextStack::new().with(ContextFrame::new("workbench"));
    let run = RunCommand((Invocation::of("workbench.show_config_files").unwrap()).context(context));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
    let picker = workbench.read_with(cx, |workbench, cx| {
        assert_eq!(workbench.palette, Some(Palette::ConfigFiles));
        workbench.config_files_picker(cx).expect("the picker shows")
    });
    let labels = picker.read_with(cx, |picker, _| {
        assert!(picker.delegate.listed());
        picker.delegate.labels()
    });
    assert_eq!(labels, ["settings.toml", "vimrc"]);

    cx.update(|window, cx| picker.update(cx, |picker, cx| picker.confirm(1, window, cx)));
    draw(cx);
    let shown = workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        pane.active().and_then(|ix| pane.tabs()[ix].path.clone())
    });
    assert_eq!(shown, Some(config.path().join("vimrc")));
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}
