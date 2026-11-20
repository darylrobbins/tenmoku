//! Consent binds to the whole installed extension package, not only the
//! scripts the arguments name (extensions record §1, Q5).

use std::{fs, path::Path};

use fe_lsp::ServerSpec;
use gpui::TestAppContext;

use crate::lsp_consent::{ConsentStore, Decision, Entry, PACKAGE_INCOMPLETE, package_hash_limited};
use crate::{
    CONSENT_FILE, ExtensionServer, Invocation, PendingConsent, extension_servers,
    grant_extension_server, pending_extension_consents, set_extension_consent_file,
    set_extension_servers, tests::rooted,
};

/// An extension directory whose `server.js` imports `lib/util.js`.
fn extension(dir: &Path) -> std::path::PathBuf {
    let extension = dir.join("zig");
    fs::create_dir_all(extension.join("lib")).unwrap();
    fs::write(extension.join("server.js"), "require('./lib/util')\n").unwrap();
    fs::write(extension.join("lib/util.js"), "exports.a = 1\n").unwrap();
    dunce::canonicalize(&extension).unwrap()
}

fn server(extension: &Path) -> ExtensionServer {
    ExtensionServer {
        extension_id: "acme.zig".into(),
        extension_name: "Zig".into(),
        extension_version: "1.2.3".into(),
        extension_license: "MIT".into(),
        extension_dir: extension.to_owned(),
        spec: ServerSpec {
            args: vec!["server.js".into(), "--stdio".into()],
            languages: vec!["zig".into()],
            ..ServerSpec::new("zls", "/usr/bin/node")
        },
    }
}

fn of(extension: &Path) -> Invocation {
    Invocation::of(&server(extension).spec, extension, None)
}

fn set(cx: &mut TestAppContext, server: &ExtensionServer) {
    cx.update(|cx| set_extension_servers(cx, vec![server.clone()]));
    cx.run_until_parked();
}

fn pending(cx: &mut TestAppContext) -> Vec<PendingConsent> {
    cx.update(|cx| pending_extension_consents(cx))
}

/// Grants `server` and checks nothing is left to ask.
fn granted(cx: &mut TestAppContext, dir: &Path, server: &ExtensionServer) {
    let path = dir.join(CONSENT_FILE);
    cx.update(|cx| set_extension_consent_file(cx, path));
    set(cx, server);
    let asked = pending(cx).remove(0);
    cx.update(|cx| grant_extension_server(cx, &asked)).unwrap();
    cx.run_until_parked();
    assert_eq!(pending(cx), []);
}

#[test]
fn the_package_hash_covers_every_file_and_path() {
    let dir = tempfile::tempdir().unwrap();
    let extension = extension(dir.path());
    let base = of(&extension);
    assert!(!base.package_hash.is_empty());
    assert_eq!(of(&extension), base);

    // A changed module the script imports; script and arguments unchanged.
    fs::write(extension.join("lib/util.js"), "exports.a = 2\n").unwrap();
    let changed = of(&extension);
    assert_ne!(changed.package_hash, base.package_hash);
    assert_eq!(changed.script_hashes, base.script_hashes);
    assert_eq!(changed.args, base.args);

    // An added file, then a removed one, then a renamed one.
    let before = of(&extension);
    fs::write(extension.join("lib/extra.js"), "").unwrap();
    let added = of(&extension);
    assert_ne!(added.package_hash, before.package_hash);
    fs::remove_file(extension.join("lib/extra.js")).unwrap();
    assert_eq!(of(&extension).package_hash, before.package_hash);
    fs::remove_file(extension.join("lib/util.js")).unwrap();
    assert_ne!(of(&extension).package_hash, before.package_hash);
    fs::write(extension.join("lib/util2.js"), "exports.a = 2\n").unwrap();
    let renamed = of(&extension);
    assert_ne!(renamed.package_hash, before.package_hash);

    // node_modules is part of the package.
    fs::create_dir_all(extension.join("node_modules/dep")).unwrap();
    fs::write(extension.join("node_modules/dep/index.js"), "1").unwrap();
    let with_dep = of(&extension);
    assert_ne!(with_dep.package_hash, renamed.package_hash);
    fs::write(extension.join("node_modules/dep/index.js"), "2").unwrap();
    assert_ne!(of(&extension).package_hash, with_dep.package_hash);
}

#[cfg(unix)]
#[test]
fn a_symlink_out_of_the_directory_is_not_followed() {
    let dir = tempfile::tempdir().unwrap();
    let extension = extension(dir.path());
    let outside = dir.path().join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("a.js"), "1").unwrap();
    std::os::unix::fs::symlink(&outside, extension.join("escape")).unwrap();
    std::os::unix::fs::symlink(&extension, extension.join("loop")).unwrap();
    let base = of(&extension);
    // Neither the outside content nor the loop's is read.
    fs::write(outside.join("a.js"), "2").unwrap();
    assert_eq!(of(&extension).package_hash, base.package_hash);
    // Retargeting the link is a change.
    fs::remove_file(extension.join("escape")).unwrap();
    std::os::unix::fs::symlink(dir.path(), extension.join("escape")).unwrap();
    assert_ne!(of(&extension).package_hash, base.package_hash);
}

#[test]
fn a_stored_grant_without_a_package_hash_never_matches() {
    let dir = tempfile::tempdir().unwrap();
    let extension = extension(dir.path());
    let now = of(&extension);
    let old = Invocation {
        package_hash: String::new(),
        ..now.clone()
    };
    assert_ne!(old, now);

    // Written by a version that had no such field.
    let path = dir.path().join(CONSENT_FILE);
    let store = ConsentStore::new(&path);
    store
        .record(Entry {
            extension_dir: extension.clone(),
            server: "zls".into(),
            decision: Decision::Granted,
            invocation: old,
        })
        .unwrap();
    let text: String = fs::read_to_string(&path)
        .unwrap()
        .lines()
        .filter(|line| !line.trim_start().starts_with("package_hash"))
        .map(|line| format!("{line}\n"))
        .collect();
    fs::write(&path, text).unwrap();
    let loaded = ConsentStore::new(&path).load();
    assert_eq!(loaded.len(), 1);
    let entry = loaded.values().next().unwrap();
    assert_eq!(entry.invocation.package_hash, "");
    assert_ne!(entry.invocation, now);
}

#[gpui::test]
fn a_changed_module_asks_again_and_the_old_spec_keeps_running(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let extension = extension(dir.path());
    let server = server(&extension);
    granted(cx, dir.path(), &server);

    // Unchanged package: no question, and the server is handed.
    set(cx, &server);
    assert_eq!(pending(cx), []);
    assert_eq!(cx.update(|cx| extension_servers(cx)).len(), 1);

    // A module changes; script and arguments don't.
    fs::write(extension.join("lib/util.js"), "exports.a = 2\n").unwrap();
    set(cx, &server);
    let asked = pending(cx);
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].invocation.args, server.spec.args);
    assert_eq!(
        asked[0].previous.as_ref().map(|p| &p.script_hashes),
        Some(&asked[0].invocation.script_hashes)
    );
    // What is handed is still the granted spec.
    let handed = cx.update(|cx| extension_servers(cx));
    assert_eq!(handed.len(), 1);
    assert_eq!(handed[0].command, rooted(&server.spec.command));

    // Accepting it clears the question.
    cx.update(|cx| grant_extension_server(cx, &asked[0]))
        .unwrap();
    cx.run_until_parked();
    assert_eq!(pending(cx), []);

    // A new file asks, and so does a removed one.
    fs::write(extension.join("lib/new.js"), "").unwrap();
    set(cx, &server);
    assert_eq!(pending(cx).len(), 1);
    let asked = pending(cx).remove(0);
    cx.update(|cx| grant_extension_server(cx, &asked)).unwrap();
    cx.run_until_parked();
    fs::remove_file(extension.join("lib/new.js")).unwrap();
    set(cx, &server);
    assert_eq!(pending(cx).len(), 1);
}

#[test]
fn a_same_size_rewrite_with_the_mtime_restored_is_a_change() {
    let dir = tempfile::tempdir().unwrap();
    let extension = extension(dir.path());
    let util = extension.join("lib/util.js");
    let base = of(&extension);
    let modified = fs::metadata(&util).unwrap().modified().unwrap();
    fs::write(&util, "exports.a = 2\n").unwrap();
    fs::File::options()
        .write(true)
        .open(&util)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    assert_eq!(fs::metadata(&util).unwrap().len(), 14);
    assert_ne!(of(&extension).package_hash, base.package_hash);
}

#[test]
fn a_package_that_cant_be_found_is_incomplete() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("gone");
    assert_eq!(of(&missing).package_hash, PACKAGE_INCOMPLETE);
}

/// Restores a directory's permissions when dropped.
#[cfg(unix)]
struct Restore(std::path::PathBuf);

#[cfg(unix)]
impl Drop for Restore {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt as _;
        let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o755));
    }
}

/// Makes `lib` execute-only, so its files read by name but it can't be
/// listed, as a package would be that this can't see whole.
#[cfg(unix)]
fn execute_only(lib: &Path) -> Restore {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(lib, fs::Permissions::from_mode(0o100)).unwrap();
    Restore(lib.to_owned())
}

#[cfg(unix)]
#[test]
fn an_unlistable_directory_makes_the_package_incomplete() {
    let dir = tempfile::tempdir().unwrap();
    let extension = extension(dir.path());
    let _restore = execute_only(&extension.join("lib"));
    // Root reads it anyway; only check where the listing really fails.
    if fs::read_dir(extension.join("lib")).is_ok() {
        return;
    }
    // The module still reads by its path, yet the hash can't cover it.
    assert!(fs::read(extension.join("lib/util.js")).is_ok());
    assert_eq!(of(&extension).package_hash, PACKAGE_INCOMPLETE);
}

#[cfg(unix)]
#[gpui::test]
fn an_incomplete_package_cannot_be_granted_or_match_a_grant(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let extension = extension(dir.path());
    let server = server(&extension);
    let _restore = execute_only(&extension.join("lib"));
    if fs::read_dir(extension.join("lib")).is_ok() {
        return;
    }
    let path = dir.path().join(CONSENT_FILE);
    cx.update(|cx| set_extension_consent_file(cx, path));
    set(cx, &server);
    let asked = pending(cx).remove(0);
    assert_eq!(asked.invocation.package_hash, PACKAGE_INCOMPLETE);
    assert!(cx.update(|cx| grant_extension_server(cx, &asked)).is_err());
    cx.run_until_parked();
    assert_eq!(cx.update(|cx| extension_servers(cx)), []);
    assert_eq!(pending(cx).len(), 1);
}

#[cfg(unix)]
#[test]
fn names_differing_only_in_invalid_utf8_hash_differently() {
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt as _};
    let dir = tempfile::tempdir().unwrap();
    let extension = extension(dir.path());
    let (a, b) = (
        extension.join(OsStr::from_bytes(b"m\xff.js")),
        extension.join(OsStr::from_bytes(b"m\xfe.js")),
    );
    // Some file systems (APFS) refuse names that aren't UTF-8.
    if fs::write(&a, "1").is_err() {
        return;
    }
    let first = of(&extension);
    fs::rename(&a, &b).unwrap();
    let renamed = of(&extension);
    assert_ne!(first.package_hash, renamed.package_hash);
    // And a link's target, likewise.
    let link = extension.join("link");
    std::os::unix::fs::symlink(OsStr::from_bytes(b"t\xff"), &link).unwrap();
    let before = of(&extension);
    fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink(OsStr::from_bytes(b"t\xfe"), &link).unwrap();
    assert_ne!(of(&extension).package_hash, before.package_hash);
}

#[test]
fn a_package_past_any_limit_is_incomplete() {
    let dir = tempfile::tempdir().unwrap();
    let extension = extension(dir.path());
    // The fixture has three entries (`lib` too) and 36 bytes of content in
    // all, the biggest file 22.
    let (entries, bytes, file) = (3, 36, 22);
    let whole = package_hash_limited(&extension, entries, bytes, file);
    assert_ne!(whole, PACKAGE_INCOMPLETE);
    assert_eq!(whole, of(&extension).package_hash);
    // One entry too many (the directory counts), one byte too many in all,
    // one byte too many in a file.
    for (e, b, f) in [
        (entries - 1, bytes, file),
        (entries, bytes - 1, file),
        (entries, bytes, file - 1),
    ] {
        assert_eq!(
            package_hash_limited(&extension, e, b, f),
            PACKAGE_INCOMPLETE
        );
    }
}

#[test]
fn directories_and_links_count_toward_the_entry_limit() {
    let dir = tempfile::tempdir().unwrap();
    let extension = extension(dir.path());
    for name in ["a", "b", "c", "d"] {
        fs::create_dir(extension.join(name)).unwrap();
    }
    // Two files, five directories: seven entries, only two regular files.
    let limit = |entries| package_hash_limited(&extension, entries, u64::MAX, u64::MAX);
    assert_ne!(limit(7), PACKAGE_INCOMPLETE);
    assert_eq!(limit(6), PACKAGE_INCOMPLETE);
}
