// SPDX-License-Identifier: Apache-2.0

//! Command titles as keys: the resolver's order (`command-<id>`, then a
//! derived label, then the registered title) and its silent fallback, in
//! en-US and a fake locale; the registry check that a crate's commands and
//! its `command-*` keys match; and kiln-command's own catalogue. The process
//! catalogue is global, so only `labels_follow_the_locale` switches it.

use std::sync::{Mutex, PoisonError};

use kiln_command::l10n::{RESOURCES, check_command_keys};
use kiln_command::{
    Actor, CommandMeta, CommandTarget, Dispatched, Effect, Registry, command_detail, command_key,
    command_label, derive_label, set_label_arg,
};
use kiln_l10n::{LanguageIdentifier, ResourceFile, Resources, set_locales, text};

mod probe {
    kiln_l10n::resources! {
        "en-US" => ["fixtures/l10n/en-US/probe.ftl"],
        "qps-ploc" => ["fixtures/l10n/qps-ploc/probe.ftl"],
    }
}

struct Probe;

impl CommandTarget for Probe {
    type Tx = ();
    type Target<'a> = ();
}

fn registry(ids: &[(&str, &str)]) -> Registry<Probe> {
    let mut registry = Registry::new();
    for (id, title) in ids {
        registry
            .register(id, title, |_: &mut (), _: &Actor| {
                Ok(Dispatched::new(Effect::Nothing))
            })
            .unwrap();
    }
    registry
}

fn meta<'a>(registry: &'a Registry<Probe>, id: &str) -> &'a CommandMeta {
    registry
        .commands()
        .find(|meta| meta.id.as_str() == id)
        .unwrap()
}

/// Records warnings, to show a keyless command's fallback logs nothing.
struct Recorder(Mutex<Vec<String>>);

impl log::Log for Recorder {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }
    fn log(&self, record: &log::Record) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(record.args().to_string());
    }
    fn flush(&self) {}
}

static RECORDER: Recorder = Recorder(Mutex::new(Vec::new()));

#[test]
fn the_key_is_the_id_with_dashes() {
    assert_eq!(command_key("git.unstage_file"), "command-git-unstage_file");
    assert_eq!(command_key("a.b.c"), "command-a-b-c");
}

#[test]
fn labels_follow_the_locale() {
    log::set_logger(&RECORDER).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    let mut commands = registry(&[
        ("probe.keyed", "Registered Title"),
        ("probe.plain", "Plain Title"),
        ("probe.toggle", "Toggle Registered"),
        ("probe.named", "Hide Registered"),
    ]);
    commands
        .register("probe.described", "Described", |_: &mut (), _: &Actor| {
            Ok(Dispatched::new(Effect::Nothing))
        })
        .unwrap()
        .description("Its registered description");
    derive_label(
        "probe.toggle",
        text!("kiln-command-toggle", title = "Probe"),
    );

    // en-US: the key's text, the derived label, else the registered title.
    let keyed = meta(&commands, "probe.keyed");
    assert_eq!(keyed.label(), "Keyed Title");
    assert_eq!(keyed.detail(), "What the keyed command does");
    assert_eq!(meta(&commands, "probe.plain").label(), "Plain Title");
    assert_eq!(meta(&commands, "probe.toggle").label(), "Toggle Probe");
    let described = meta(&commands, "probe.described");
    assert_eq!(described.detail(), "Its registered description");
    assert_eq!(command_label("probe.unknown", "Fallback"), "Fallback");
    assert_eq!(command_detail("probe.unknown", ""), "");

    // A title that takes the app's name: the registered title until the
    // app sets it, then the message with it.
    let named = meta(&commands, "probe.named");
    assert_eq!(named.label(), "Hide Registered");
    set_label_arg("app", "Probe App");
    assert_eq!(named.label(), "Hide Probe App");

    // A fake locale: each resolves again, the derived label too.
    let fake: Vec<LanguageIdentifier> = vec!["qps-ploc".parse().unwrap()];
    set_locales(&fake);
    assert_eq!(keyed.label(), "⟦Kéyéd Títlé⟧");
    assert_eq!(keyed.detail(), "⟦Whát thé kéyéd cómmánd dóés⟧");
    assert_eq!(meta(&commands, "probe.plain").label(), "Plain Title");
    assert_eq!(meta(&commands, "probe.toggle").label(), "⟦Tógglé Probe⟧");
    assert_eq!(named.label(), "⟦Hídé Probe App⟧");

    // Back to en-US: nothing kept a stale title.
    set_locales(&[]);
    assert_eq!(keyed.label(), "Keyed Title");
    assert_eq!(meta(&commands, "probe.toggle").label(), "Toggle Probe");

    // A command with no key, as a plugin's is, falls back silently.
    let logged = RECORDER.0.lock().unwrap_or_else(PoisonError::into_inner);
    assert!(
        !logged
            .iter()
            .any(|line| line.contains("probe-plain") || line.contains("probe-unknown")),
        "{logged:?}"
    );
}

/// A crate's resources, by hand: `command-*` keys in en-US and one in `de`.
static FIXTURE: Resources = Resources {
    crate_name: "notes",
    manifest_dir: "",
    declared_in: "",
    files: &[
        ResourceFile {
            locale: "en-US",
            path: "en-US/notes.ftl",
            source: "command-notes-archive = Archive Note\n\
                     command-notes-stale = Stale Note\n\
                     notes-empty = No notes\n",
        },
        ResourceFile {
            locale: "de",
            path: "de/notes.ftl",
            source: "command-notes-archive = Notiz archivieren\n\
                     command-notes-rename = Notiz umbenennen\n",
        },
    ],
    dynamic_prefixes: &["command-"],
};

#[test]
fn the_registry_check_matches_commands_and_keys() {
    let commands = registry(&[
        ("notes.archive", "Archive Note"),
        ("notes.rename", "Rename"),
    ]);
    let ids = commands.commands().map(|meta| meta.id.to_string());
    assert_eq!(
        check_command_keys(&FIXTURE, ids),
        [
            // A translation's key doesn't count: en-US must have it.
            "the command notes.rename has no `command-notes-rename` in notes's en-US resources",
            "`command-notes-stale` in notes's en-US resources names no command of notes",
        ]
    );
    let matching = ["notes.archive", "notes.stale"];
    assert_eq!(check_command_keys(&FIXTURE, matching), Vec::<String>::new());
}

#[test]
fn keys_used_here_exist() {
    kiln_l10n::lint::keys(env!("CARGO_MANIFEST_DIR"), &RESOURCES).check();
}
