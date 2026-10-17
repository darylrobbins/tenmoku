// SPDX-License-Identifier: LicenseRef-Proprietary

//! Discovering the extensions in a directory, rescanning it, and the
//! licence notices, on temporary directories.

use std::{fs, path::Path};

use kiln_core::{
    ApiVersion, ExtensionApi, ExtensionSet, LicenseClass,
    extension::{ACKNOWLEDGED_FILE, MANIFEST_FILE},
};

fn api() -> ExtensionApi {
    ExtensionApi {
        app: "notes".into(),
        version: ApiVersion { major: 1, minor: 0 },
        reserved: vec!["app".into()],
        kinds: Vec::new(),
        allowed_licenses: None,
    }
}

fn manifest(id: &str, version: &str, license: &str) -> String {
    format!(
        "id = \"{id}\"\nname = \"Ext {id}\"\nversion = \"{version}\"\nlicense = \"{license}\"\n\n\
         [engine]\nnotes = \"1.0\"\n"
    )
}

/// Writes `<root>/<dir>/extension.toml`.
fn install(root: &Path, dir: &str, text: &str) {
    let dir = root.join(dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(MANIFEST_FILE), text).unwrap();
}

fn ids(set: &ExtensionSet) -> Vec<&str> {
    set.extensions().iter().map(|ext| ext.id.as_str()).collect()
}

#[test]
fn discovery_loads_in_id_order_and_rejects_duplicates() {
    let state = tempfile::tempdir().unwrap();
    let root = state.path().join("extensions");
    let missing = ExtensionSet::discover(&root, &api());
    assert_eq!(missing.extensions(), []);
    assert_eq!(missing.diagnostics(), []);

    // Directory order isn't ID order.
    install(&root, "a-zebra", &manifest("acme.zebra", "1.0.0", "MIT"));
    install(&root, "b-apple", &manifest("acme.apple", "1.0.0", "MIT"));
    // Not an extension: no manifest, or not a directory.
    fs::create_dir_all(root.join("c-empty")).unwrap();
    fs::write(root.join("README"), "").unwrap();
    let set = ExtensionSet::discover(&root, &api());
    assert_eq!(set.diagnostics(), []);
    assert_eq!(ids(&set), ["acme.apple", "acme.zebra"]);
    assert_eq!(set.extensions()[0].dir, root.join("b-apple"));

    install(&root, "z-again", &manifest("acme.apple", "2.0.0", "MIT"));
    let set = ExtensionSet::discover(&root, &api());
    assert_eq!(ids(&set), ["acme.apple", "acme.zebra"]);
    assert_eq!(set.extensions()[0].version, (1, 0, 0));
    let [error] = set.diagnostics() else {
        panic!("{:#?}", set.diagnostics());
    };
    assert!(error.is_error());
    assert_eq!(error.file, root.join("z-again").join(MANIFEST_FILE));
    assert_eq!(error.key.as_deref(), Some("id"));
    assert_eq!((error.line, error.column), (1, 6));
    assert!(error.message.contains("acme.apple"), "{error}");
    assert!(
        error
            .message
            .contains(&root.join("b-apple").display().to_string()),
        "{error}"
    );
}

#[test]
fn a_rescan_keeps_the_last_good_manifest() {
    let state = tempfile::tempdir().unwrap();
    let root = state.path().join("extensions");
    install(&root, "one", &manifest("acme.one", "1.0.0", "MIT"));
    install(&root, "two", &manifest("acme.two", "1.0.0", "MIT"));
    let mut set = ExtensionSet::discover(&root, &api());
    assert_eq!(ids(&set), ["acme.one", "acme.two"]);

    let before = set.clone();
    assert!(!set.rescan());
    assert_eq!(set, before);

    install(&root, "one", "id = \"acme.one\"\nname = \n");
    assert!(set.rescan());
    assert_eq!(ids(&set), ["acme.one", "acme.two"]);
    assert_eq!(set.extensions()[0], before.extensions()[0]);
    let errors: Vec<_> = set.diagnostics().iter().filter(|d| d.is_error()).collect();
    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert_eq!(errors[0].file, root.join("one").join(MANIFEST_FILE));
    let broken = set.clone();
    assert!(!set.rescan());
    assert_eq!(set, broken);

    install(&root, "one", &manifest("acme.one", "1.1.0", "MIT"));
    assert!(set.rescan());
    assert_eq!(set.diagnostics(), []);
    assert_eq!(set.extensions()[0].version, (1, 1, 0));

    fs::remove_dir_all(root.join("two")).unwrap();
    assert!(set.rescan());
    assert_eq!(ids(&set), ["acme.one"]);

    // A new directory whose manifest has errors has no last good.
    install(&root, "three", "id = \"acme.three\"\n");
    assert!(set.rescan());
    assert_eq!(ids(&set), ["acme.one"]);
    assert!(set.diagnostics().iter().any(|d| d.is_error()));
}

#[test]
fn licence_notices_are_given_once_per_version() {
    let state = tempfile::tempdir().unwrap();
    let root = state.path().join("extensions");
    install(&root, "gpl", &manifest("acme.gpl", "1.0.0", "GPL-3.0-only"));
    install(&root, "mit", &manifest("acme.mit", "1.0.0", "MIT"));
    let set = ExtensionSet::discover(&root, &api());
    let notices = set.license_notices(state.path());
    let [notice] = notices.as_slice() else {
        panic!("{notices:#?}");
    };
    assert_eq!(notice.id, "acme.gpl");
    assert_eq!(notice.license.class, LicenseClass::Copyleft);
    assert_eq!(
        notice.message("Notes"),
        "Ext acme.gpl (acme.gpl) is licensed GPL-3.0-only. Notes doesn't ship it; you installed it."
    );
    let acknowledged = fs::read_to_string(state.path().join(ACKNOWLEDGED_FILE)).unwrap();
    assert!(acknowledged.contains("acme.gpl@1.0.0"), "{acknowledged}");
    assert!(!acknowledged.contains("acme.mit"), "{acknowledged}");

    // As after a restart: the acknowledgement is read back.
    let set = ExtensionSet::discover(&root, &api());
    assert_eq!(set.license_notices(state.path()), []);

    install(&root, "gpl", &manifest("acme.gpl", "1.1.0", "GPL-3.0-only"));
    let set = ExtensionSet::discover(&root, &api());
    let notices = set.license_notices(state.path());
    assert_eq!(notices.len(), 1, "{notices:#?}");
    assert_eq!(notices[0].version, (1, 1, 0));
    assert_eq!(set.license_notices(state.path()), []);

    // An unknown licence is told of too.
    install(
        &root,
        "own",
        &manifest("acme.own", "1.0.0", "LicenseRef-acme"),
    );
    let set = ExtensionSet::discover(&root, &api());
    let notices = set.license_notices(state.path());
    assert_eq!(notices.len(), 1, "{notices:#?}");
    assert_eq!(notices[0].id, "acme.own");
}

#[test]
fn a_missing_or_unreadable_manifest_keeps_the_last_good() {
    use std::os::unix::fs::PermissionsExt;

    let state = tempfile::tempdir().unwrap();
    let root = state.path().join("extensions");
    install(&root, "one", &manifest("acme.one", "1.0.0", "MIT"));
    install(&root, "two", &manifest("acme.two", "1.0.0", "MIT"));
    let mut set = ExtensionSet::discover(&root, &api());
    assert_eq!(ids(&set), ["acme.one", "acme.two"]);

    fs::remove_file(root.join("one").join(MANIFEST_FILE)).unwrap();
    assert!(set.rescan());
    assert_eq!(ids(&set), ["acme.one", "acme.two"]);
    let [error] = set.diagnostics() else {
        panic!("{:#?}", set.diagnostics());
    };
    assert!(error.is_error());
    assert_eq!(error.file, root.join("one").join(MANIFEST_FILE));

    let two = root.join("two");
    fs::set_permissions(&two, fs::Permissions::from_mode(0o000)).unwrap();
    // Root reads anything: there's nothing to check then.
    let unreadable = fs::read_dir(&two).is_err();
    let rescanned = set.rescan();
    fs::set_permissions(&two, fs::Permissions::from_mode(0o755)).unwrap();
    if unreadable {
        assert!(rescanned);
        assert_eq!(ids(&set), ["acme.one", "acme.two"]);
        assert!(
            set.diagnostics()
                .iter()
                .any(|d| d.is_error() && d.file == two.join(MANIFEST_FILE)),
            "{:#?}",
            set.diagnostics()
        );
    }

    // Gone with its directory.
    fs::remove_dir_all(root.join("one")).unwrap();
    set.rescan();
    assert_eq!(ids(&set), ["acme.two"]);
    assert_eq!(set.diagnostics(), []);

    // Or with a file in its place.
    fs::remove_dir_all(&two).unwrap();
    fs::write(&two, "").unwrap();
    assert!(set.rescan());
    assert_eq!(ids(&set), [] as [&str; 0]);
    assert_eq!(set.diagnostics(), []);
}

#[test]
fn concurrent_discoveries_give_each_notice_once() {
    let state = tempfile::tempdir().unwrap();
    let sets: Vec<ExtensionSet> = (0..8)
        .map(|index| {
            let root = state.path().join(format!("extensions-{index}"));
            install(
                &root,
                "shared",
                &manifest("acme.shared", "1.0.0", "GPL-3.0-only"),
            );
            let own = format!("acme.own{index}");
            install(&root, "own", &manifest(&own, "1.0.0", "GPL-3.0-only"));
            ExtensionSet::discover(&root, &api())
        })
        .collect();
    let notices: Vec<_> = std::thread::scope(|scope| {
        let threads: Vec<_> = sets
            .iter()
            .map(|set| scope.spawn(|| set.license_notices(state.path())))
            .collect();
        threads
            .into_iter()
            .flat_map(|thread| thread.join().unwrap())
            .collect()
    });
    let shared = notices.iter().filter(|n| n.id == "acme.shared").count();
    assert_eq!(shared, 1, "{notices:#?}");
    assert_eq!(notices.len(), 9, "{notices:#?}");
    // No process's entries were written over.
    for set in &sets {
        assert_eq!(set.pending_license_notices(state.path()), []);
    }
}
