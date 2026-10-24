// SPDX-License-Identifier: LicenseRef-Proprietary

//! An extension's keymap as kept for every platform with the keymap store
//! (`SharedKeymap::platform_defaults`), through `kiln::init` and the
//! reloads of its file, headless: each platform keeps its own last good
//! layer, as a file can parse on one platform and not on another. An app
//! that edits the user's keymap reads these layers to tell what the
//! user's entries for another platform do to the extension's keys there.

use std::{
    fs,
    path::{Path, PathBuf},
};

use gpui::TestAppContext;
use kiln::{
    Config,
    core::{ApiVersion, Diagnostic, Os, extension::MANIFEST_FILE},
    platform::config::{BatchSender, batch_channel, keymap_store},
};

mod notes {
    kiln::commands!(
        notes,
        [
            /// Tidy
            Tidy,
        ]
    );
}

const MANIFEST: &str = "\
id = \"acme.tidy\"
name = \"Tidy\"
version = \"1.0.0\"
license = \"MIT\"

[engine]
notes = \"1.0\"

[contributes]
keymaps = [\"keymap.toml\"]
";

/// Binds Tidy to `keys`.
fn tidy_on(keys: &str) -> String {
    format!("[[bindings]]\nkeys = \"{keys}\"\ncommand = \"notes.tidy\"\n")
}

/// The extension's directory and what sends the batch its tree watcher
/// would.
struct Setup {
    extensions: PathBuf,
    extension: PathBuf,
    sender: BatchSender,
}

impl Setup {
    /// Writes the extension's keymap and resolves the keymap again.
    fn save_keymap(&self, text: &str, cx: &mut TestAppContext) {
        fs::write(self.extension.join("keymap.toml"), text).unwrap();
        self.sender.send(vec![self.extensions.clone()]);
        cx.run_until_parked();
    }
}

/// Each platform's keys for Tidy in the default layers kept with the
/// keymap store, in `Os::ALL`'s order.
fn tidy_keys(cx: &mut TestAppContext) -> Vec<(Os, Vec<String>)> {
    cx.update(|cx| {
        let store = keymap_store(cx).expect("the app's keymap store");
        let platforms = store.platform_defaults().expect("kept with its defaults");
        (platforms.into_iter())
            .map(|(os, layers)| {
                let keys = (layers.iter())
                    .flat_map(|layer| &layer.entries)
                    .filter(|entry| {
                        entry.command.as_ref().map(|id| id.as_str()) == Some("notes.tidy")
                    })
                    .map(|entry| entry.keys.to_string())
                    .collect();
                (os, keys)
            })
            .collect()
    })
}

fn on_each(macos: &str, linux: &str, windows: &str) -> Vec<(Os, Vec<String>)> {
    vec![
        (Os::MacOs, vec![macos.to_owned()]),
        (Os::Linux, vec![linux.to_owned()]),
        (Os::Windows, vec![windows.to_owned()]),
    ]
}

/// Sets the app up with the extension's keymap `keymap`, under `state`,
/// returning what `init` found.
fn init(state: &Path, keymap: &str, cx: &mut TestAppContext) -> (Setup, Vec<Diagnostic>) {
    let extensions = state.join("extensions");
    let extension = extensions.join("tidy");
    fs::create_dir_all(&extension).unwrap();
    fs::write(extension.join(MANIFEST_FILE), MANIFEST).unwrap();
    fs::write(extension.join("keymap.toml"), keymap).unwrap();
    let (sender, batches) = batch_channel();
    let config = Config::new("com.example.Notes", "Notes")
        .commands(notes::register)
        .extensions(&extensions, "notes", ApiVersion { major: 1, minor: 0 })
        .watch_with(batches);
    let diagnostics = cx.update(|cx| kiln::init(cx, config));
    cx.run_until_parked();
    let setup = Setup {
        extensions,
        extension,
        sender,
    };
    (setup, diagnostics)
}

#[gpui::test]
fn each_platform_keeps_its_own_last_good_extension_keymap(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let (setup, diagnostics) = init(state.path(), &tidy_on("f9"), cx);
    assert_eq!(diagnostics, []);
    assert_eq!(tidy_keys(cx), on_each("f9", "f9", "f9"));

    // `secondary` is ctrl off macOS, so this holds ctrl twice there: an
    // error on Linux and Windows, which keep f9, while macOS takes it.
    let ctrl_twice = tidy_on("secondary-ctrl-k");
    setup.save_keymap(&ctrl_twice, cx);
    assert_eq!(tidy_keys(cx), on_each("cmd-ctrl-k", "f9", "f9"));

    // Resolved again: each platform still has its own.
    setup.save_keymap(&format!("# Tidy's keys\n{ctrl_twice}"), cx);
    assert_eq!(tidy_keys(cx), on_each("cmd-ctrl-k", "f9", "f9"));

    // A file good everywhere is every platform's again.
    setup.save_keymap(&tidy_on("secondary-alt-t"), cx);
    assert_eq!(
        tidy_keys(cx),
        on_each("cmd-alt-t", "ctrl-alt-t", "ctrl-alt-t")
    );
}

#[gpui::test]
fn a_platform_where_the_extension_keymap_never_loaded_has_none(cx: &mut TestAppContext) {
    // An error from the start off macOS, with no last good version there:
    // macOS alone has the layer.
    let state = tempfile::tempdir().unwrap();
    let (setup, _) = init(state.path(), &tidy_on("secondary-ctrl-k"), cx);
    let only_macos = vec![
        (Os::MacOs, vec!["cmd-ctrl-k".to_owned()]),
        (Os::Linux, Vec::new()),
        (Os::Windows, Vec::new()),
    ];
    assert_eq!(tidy_keys(cx), only_macos);
    setup.save_keymap(
        &format!("# Tidy's keys\n{}", tidy_on("secondary-ctrl-k")),
        cx,
    );
    assert_eq!(tidy_keys(cx), only_macos);
}
