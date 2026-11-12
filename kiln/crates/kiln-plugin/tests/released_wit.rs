// SPDX-License-Identifier: Apache-2.0

//! The frozen WIT of each released minor, against `wit/`: a minor only adds.

use std::path::{Path, PathBuf};

use kiln_plugin::compat::{
    HOST_API, World,
    released::{self, Change, Files},
};

/// `wit/released/1.0/`'s digest, as 1.0 was released. It never changes.
const FROZEN_1_0: &str = "38b7ac3a1ec74343722842266023f606abbd652344dd822a7bf5803cf0b47ae1";

fn dir(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
}

fn read(path: &str) -> Files {
    let files = released::read(&dir(path)).unwrap_or_else(|e| panic!("{path}: {e}"));
    assert!(!files.is_empty(), "{path} has no WIT");
    files
}

/// The version every `package` line of `files` names.
fn package_versions(files: &Files) -> Vec<String> {
    let mut versions: Vec<String> = files
        .values()
        .filter_map(|text| text.lines().find_map(|line| line.strip_prefix("package ")))
        .map(|line| {
            line.trim_end_matches(';')
                .rsplit('@')
                .next()
                .unwrap()
                .to_owned()
        })
        .collect();
    versions.dedup();
    versions
}

#[test]
fn released_wit_1_0_matches_frozen_copy() {
    let frozen = read("wit/released/1.0");
    let current = read("wit");
    assert_eq!(
        released::digest(&frozen),
        FROZEN_1_0,
        "wit/released/1.0 changed: it is 1.0 as released"
    );
    assert_eq!(package_versions(&frozen), [World::V1_0.package_version()]);
    assert_eq!(
        package_versions(&current),
        [World::CURRENT.package_version()]
    );
    assert_eq!(World::CURRENT.api(), HOST_API);
    let changes = released::changes(&frozen, &current);
    assert!(
        changes.is_empty(),
        "wit/ changed what 1.0 released; a minor only adds: {changes:#?}"
    );
}

/// Whether `change` is of `container`'s `item`.
fn names(change: &Change, container: &str, item: &str) -> bool {
    match change {
        Change::Removed {
            container: c,
            item: i,
        }
        | Change::Changed {
            container: c,
            item: i,
            ..
        } => c == container && i == item,
        Change::Unparsed { .. } => false,
    }
}

/// The current files with `file`'s `from` replaced by `to`.
fn edited(file: &str, from: &str, to: &str) -> Files {
    let mut files = read("wit");
    let text = files.get_mut(file).unwrap();
    assert!(text.contains(from), "{file} lacks {from:?}");
    *text = text.replacen(from, to, 1);
    files
}

#[test]
fn a_change_to_a_released_definition_fails_the_check() {
    let current = read("wit");
    let refused = [
        // A new case in an existing variant.
        (
            "events.wit",
            "variant event {",
            "variant event {\n        renamed(buffer-id),",
            ("interface events", "variant event"),
        ),
        // A new field in an existing record.
        (
            "types.wit",
            "record range {",
            "record range {\n        line: u32,",
            ("interface types", "record range"),
        ),
        // A changed function.
        (
            "log.wit",
            "message: string",
            "message: list<u8>",
            ("interface log", "log"),
        ),
        // A dropped name of a `use`.
        (
            "events.wit",
            "use types.{buffer-id, version, range, error};",
            "use types.{buffer-id, version, error};",
            ("interface events", "use types . { range }"),
        ),
        // A dropped export of the world.
        (
            "world.wit",
            "export event-handler;",
            "",
            ("world extension", "export event-handler"),
        ),
    ];
    for (file, from, to, (container, item)) in refused {
        let changes = released::changes(&current, &edited(file, from, to));
        assert!(
            changes.iter().any(|change| names(change, container, item)),
            "{file}: {to:?} not reported as {container}: {item}: {changes:#?}"
        );
    }
    let mut renamed = current.clone();
    let types = renamed.remove("types.wit").unwrap();
    renamed.insert(
        "types.wit".into(),
        types.replacen("interface types", "interface kinds", 1),
    );
    let changes = released::changes(&current, &renamed);
    assert!(
        changes.contains(&Change::Removed {
            container: "interface types".into(),
            item: String::new(),
        }),
        "{changes:#?}"
    );
    let mut broken = current.clone();
    broken.insert("broken.wit".into(), "interface broken {".into());
    assert_eq!(
        released::changes(&current, &broken),
        [Change::Unparsed {
            released: false,
            file: "broken.wit".into()
        }]
    );
}

#[test]
fn additions_and_layout_pass_the_check() {
    let current = read("wit");
    let allowed = [
        // A new function, and one more name for a `use`.
        (
            "log.wit",
            "interface log {",
            "interface log {\n    flush: func();",
        ),
        (
            "events.wit",
            "use types.{buffer-id, version, range, error};",
            "use types.{buffer-id, version, range, error, command-id};",
        ),
        // A new type.
        (
            "types.wit",
            "record range {",
            "record span {\n        start: u64,\n    }\n\n    record range {",
        ),
        // A new import and export.
        (
            "world.wit",
            "export event-handler;",
            "export event-handler;\n    import later;\n    export later-handler;",
        ),
        // Comments and layout.
        (
            "log.wit",
            "log: func(",
            "/// Said again.\n    // and again\n    log:   func(\n",
        ),
        // Another version.
        ("log.wit", "@1.1.0", "@1.2.0"),
    ];
    for (file, from, to) in allowed {
        let changes = released::changes(&current, &edited(file, from, to));
        assert_eq!(changes, [], "{file}: {to:?}");
    }
    // A new interface, in a file of its own.
    let mut added = current.clone();
    added.insert(
        "jobs.wit".into(),
        "package kiln:extension@1.1.0;\n\ninterface jobs {\n    run: func();\n}\n".into(),
    );
    assert_eq!(released::changes(&current, &added), []);
}
