//! A granted extension server runs from an immutable, content-addressed
//! snapshot of its package, kept while anything needs it (extensions record
//! §1, Q5).
#![cfg(unix)]

use std::{
    fs,
    os::unix::fs::{MetadataExt as _, PermissionsExt as _},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use fe_lsp::{LaunchCancel, LocalLauncher, ServerIo, ServerLauncher, ServerSpec};
use futures::io::AsyncWriteExt as _;
use gpui::TestAppContext;

use super::lsp_consent_snapshot::{
    Snapshots, collect_unkept, link_escapes, pause_after_open, private_dir_in, resolve,
    staging_slot,
};
use crate::lsp_consent::{ConsentGate, Invocation, package_hash};
use crate::{
    CONSENT_FILE, ExtensionServer, PendingConsent, grant_extension_server,
    pending_extension_consents, set_extension_consent_file, set_extension_servers,
};

const STORE: &str = "extensions-lsp-packages";

/// An extension whose `server.sh` waits for a line, sources `lib/util.sh`
/// beside it and writes what that set to `$OUT`.
fn extension(dir: &Path, value: &str) -> PathBuf {
    let extension = dir.join("ext");
    fs::create_dir_all(extension.join("lib")).unwrap();
    fs::write(
        extension.join("server.sh"),
        "read line\n. \"$(dirname \"$0\")/lib/util.sh\"\nprintf '%s' \"$VALUE\" > \"$OUT\"\n",
    )
    .unwrap();
    fs::write(extension.join("lib/util.sh"), format!("VALUE={value}\n")).unwrap();
    dunce::canonicalize(&extension).unwrap()
}

fn server(extension: &Path, out: &Path) -> ExtensionServer {
    ExtensionServer {
        extension_id: "acme.sh".into(),
        extension_name: "Sh".into(),
        extension_version: "1.0.0".into(),
        extension_license: "MIT".into(),
        extension_dir: extension.to_owned(),
        spec: ServerSpec {
            args: vec!["server.sh".into()],
            env: vec![("OUT".into(), out.to_string_lossy().into_owned())],
            languages: vec!["sh".into()],
            ..ServerSpec::new("shls", "/bin/sh")
        },
    }
}

fn consenting(cx: &mut TestAppContext, dir: &Path) {
    let file = dir.join(CONSENT_FILE);
    cx.update(|cx| set_extension_consent_file(cx, file));
    cx.run_until_parked();
}

fn set(cx: &mut TestAppContext, server: &ExtensionServer) {
    cx.update(|cx| set_extension_servers(cx, vec![server.clone()]));
    cx.run_until_parked();
}

fn pending(cx: &mut TestAppContext) -> Vec<PendingConsent> {
    cx.update(|cx| pending_extension_consents(cx))
}

/// Grants what `server` shows now; its package hash.
fn grant(cx: &mut TestAppContext, server: &ExtensionServer) -> String {
    set(cx, server);
    let asked = pending(cx).remove(0);
    cx.update(|cx| grant_extension_server(cx, &asked)).unwrap();
    cx.run_until_parked();
    asked.invocation.package_hash
}

fn launch(gate: &ConsentGate, spec: &ServerSpec, root: &Path) -> std::io::Result<ServerIo> {
    gate.launch(spec, root, &LaunchCancel::new())
}

/// Tells the server `io` to go on, and waits for it to end.
fn finish(mut io: ServerIo) {
    futures::executor::block_on(async {
        io.input.write_all(b"go\n").await.unwrap();
        io.input.flush().await.unwrap();
        io.process.exited().await;
    });
}

/// What the server wrote to `out`, which is then removed.
fn written(out: &Path) -> String {
    let text = fs::read_to_string(out).unwrap();
    fs::remove_file(out).unwrap();
    text
}

fn snapshots(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir.join(STORE))
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|name| !name.ends_with(".lock"))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// Waits for `condition`, which a thread of its own makes true.
fn eventually(mut condition: impl FnMut() -> bool) {
    let start = Instant::now();
    while !condition() {
        assert!(start.elapsed() < Duration::from_secs(20), "timed out");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[derive(Default)]
struct Recorder(Mutex<Vec<ServerSpec>>);

impl ServerLauncher for Recorder {
    fn launch(
        &self,
        spec: &ServerSpec,
        _root: &Path,
        _cancel: &LaunchCancel,
    ) -> std::io::Result<ServerIo> {
        self.0.lock().unwrap().push(spec.clone());
        Err(std::io::Error::other("recorded"))
    }
}

#[gpui::test]
fn a_running_server_and_a_restart_read_the_granted_bytes_until_the_change_is_granted(
    cx: &mut TestAppContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let extension = extension(dir.path(), "old");
    let out = dir.path().join("out");
    let server = server(&extension, &out);
    consenting(cx, dir.path());
    grant(cx, &server);
    let gate = cx.update(|cx| ConsentGate::new(cx, Arc::new(LocalLauncher)));

    let running = launch(&gate, &server.spec, dir.path()).unwrap();
    // The installed module is rewritten in place, as an update might.
    fs::write(extension.join("lib/util.sh"), "VALUE=new\n").unwrap();
    // The running server's late source reads what was granted.
    finish(running);
    assert_eq!(written(&out), "old");

    // So does a restart, before the change is accepted.
    set(cx, &server);
    assert_eq!(pending(cx).len(), 1);
    finish(launch(&gate, &server.spec, dir.path()).unwrap());
    assert_eq!(written(&out), "old");

    // Granted, a new launch reads the new bytes.
    let asked = pending(cx).remove(0);
    cx.update(|cx| grant_extension_server(cx, &asked)).unwrap();
    cx.run_until_parked();
    finish(launch(&gate, &server.spec, dir.path()).unwrap());
    assert_eq!(written(&out), "new");
}

#[gpui::test]
fn the_snapshot_is_a_read_only_copy_that_is_checked_at_launch(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let extension = extension(dir.path(), "old");
    let out = dir.path().join("out");
    let server = server(&extension, &out);
    consenting(cx, dir.path());
    let hash = grant(cx, &server);
    let snapshot = dir.path().join(STORE).join(&hash);

    // A copy, not a link: rewriting the installed file can't change it.
    let (live, copy) = (
        fs::metadata(extension.join("server.sh")).unwrap(),
        fs::metadata(snapshot.join("server.sh")).unwrap(),
    );
    assert_ne!(live.ino(), copy.ino());
    assert_eq!(copy.nlink(), 1);
    assert_eq!(live.nlink(), 1);
    // Read-only, the execute bits kept.
    assert_eq!(copy.permissions().mode() & 0o222, 0);
    assert_eq!(
        copy.permissions().mode() & 0o111,
        live.permissions().mode() & 0o111
    );
    assert_eq!(
        fs::metadata(&snapshot).unwrap().permissions().mode() & 0o222,
        0
    );

    // The command and in-package arguments are rewritten in the bound spec only.
    let recorder = Arc::new(Recorder::default());
    let gate = cx.update(|cx| ConsentGate::new(cx, recorder.clone()));
    let mut spec = server.spec.clone();
    spec.args = vec!["server.sh".into()];
    assert!(launch(&gate, &spec, dir.path()).is_err());
    let bound = recorder.0.lock().unwrap().remove(0);
    let canonical = dunce::canonicalize(&snapshot).unwrap();
    assert_eq!(bound.args, [canonical.join("server.sh").to_string_lossy()]);
    assert_eq!(spec.args, ["server.sh"]);

    // A tampered snapshot is refused.
    let file = snapshot.join("lib/util.sh");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
    fs::write(&file, "VALUE=evil\n").unwrap();
    let error = launch(&gate, &spec, dir.path()).err().unwrap();
    assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
    assert!(recorder.0.lock().unwrap().is_empty());
    // A missing one too.
    fs::set_permissions(&file, fs::Permissions::from_mode(0o444)).unwrap();
    for entry in ["lib", ""] {
        fs::set_permissions(snapshot.join(entry), fs::Permissions::from_mode(0o755)).unwrap();
    }
    fs::remove_dir_all(&snapshot).unwrap();
    assert!(launch(&gate, &spec, dir.path()).is_err());
    assert!(recorder.0.lock().unwrap().is_empty());
}

#[gpui::test]
fn the_command_and_flag_values_inside_the_package_run_from_the_snapshot(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let extension = extension(dir.path(), "old");
    fs::create_dir_all(extension.join("bin")).unwrap();
    fs::write(extension.join("bin/run"), "#!/bin/sh\n").unwrap();
    fs::set_permissions(extension.join("bin/run"), fs::Permissions::from_mode(0o755)).unwrap();
    let mut server = server(&extension, &dir.path().join("out"));
    server.spec.command = extension.join("bin/run");
    server.spec.args = vec!["--config=lib/util.sh".into(), "--stdio".into()];
    consenting(cx, dir.path());
    let hash = grant(cx, &server);
    let snapshot = dunce::canonicalize(dir.path().join(STORE).join(hash)).unwrap();

    let recorder = Arc::new(Recorder::default());
    let gate = cx.update(|cx| ConsentGate::new(cx, recorder.clone()));
    assert!(launch(&gate, &server.spec, dir.path()).is_err());
    let bound = recorder.0.lock().unwrap().remove(0);
    assert_eq!(bound.command, snapshot.join("bin/run"));
    assert_eq!(
        bound.args,
        [
            format!("--config={}", snapshot.join("lib/util.sh").display()),
            "--stdio".to_owned()
        ]
    );
}

#[gpui::test]
fn a_symlink_reaching_the_live_directory_is_not_grantable(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out");
    consenting(cx, dir.path());
    type Target = Box<dyn Fn(&Path) -> PathBuf>;
    let cases: [(&str, Target); 3] = [
        // Absolute, into the live directory.
        ("absolute", Box::new(|ext| ext.join("server.sh"))),
        // Relative, out of the package root.
        ("relative", Box::new(|_| PathBuf::from("../../elsewhere"))),
        (
            "through",
            Box::new(|_| PathBuf::from("lib/../../ext/server.sh")),
        ),
    ];
    for (name, target) in cases {
        let extension = extension(&dir.path().join(name), "old");
        std::os::unix::fs::symlink(target(&extension), extension.join("link")).unwrap();
        let server = server(&extension, &out);
        set(cx, &server);
        let asked = pending(cx).remove(0);
        assert_eq!(asked.invocation.package_hash, "incomplete", "{name}");
        assert!(
            cx.update(|cx| grant_extension_server(cx, &asked)).is_err(),
            "{name}"
        );
    }

    // Inside the package, or an absolute path elsewhere, is fine.
    let extension = extension(&dir.path().join("fine"), "old");
    std::os::unix::fs::symlink("lib/util.sh", extension.join("inside")).unwrap();
    std::os::unix::fs::symlink("/bin/sh", extension.join("elsewhere")).unwrap();
    let hash = grant(cx, &server(&extension, &out));
    assert_ne!(hash, "incomplete");
    let snapshot = dir.path().join(STORE).join(hash);
    assert_eq!(
        fs::read_link(snapshot.join("inside")).unwrap(),
        Path::new("lib/util.sh")
    );
}

#[gpui::test]
fn a_snapshot_is_kept_while_granted_pending_or_running_and_then_removed(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let extension = extension(dir.path(), "v1");
    let out = dir.path().join("out");
    let server = server(&extension, &out);
    consenting(cx, dir.path());
    let first = grant(cx, &server);
    let store = dir.path().join(STORE);
    let gate = cx.update(|cx| ConsentGate::new(cx, Arc::new(LocalLauncher)));
    let running = launch(&gate, &server.spec, dir.path()).unwrap();

    // A change pending in this session: both are kept.
    fs::write(extension.join("lib/util.sh"), "VALUE=v2\n").unwrap();
    set(cx, &server);
    let second = pending(cx)[0].invocation.package_hash.clone();
    assert_ne!(first, second);
    assert_eq!(snapshots(dir.path()), {
        let mut both = vec![first.clone(), second.clone()];
        both.sort();
        both
    });

    // Granted, `first` is named by no grant any more, but a process runs
    // from it, and another store user holds its lock too.
    let other = fs::File::open(store.join(format!("{first}.lock"))).unwrap();
    other.lock_shared().unwrap();
    let asked = pending(cx).remove(0);
    cx.update(|cx| grant_extension_server(cx, &asked)).unwrap();
    cx.run_until_parked();
    assert!(snapshots(dir.path()).contains(&first));

    // The process ends: still held by the other user.
    drop(running);
    std::thread::sleep(Duration::from_millis(300));
    assert!(snapshots(dir.path()).contains(&first));
    set(cx, &server);
    assert!(snapshots(dir.path()).contains(&first));

    // Nothing holds it: removed at the next pass, `second` stays.
    drop(other);
    set(cx, &server);
    assert_eq!(snapshots(dir.path()), std::slice::from_ref(&second));

    // A running process alone keeps one too, and its end collects it.
    let running = launch(&gate, &server.spec, dir.path()).unwrap();
    fs::write(extension.join("lib/util.sh"), "VALUE=v3\n").unwrap();
    grant(cx, &server);
    assert!(snapshots(dir.path()).contains(&second));
    drop(running);
    eventually(|| !snapshots(dir.path()).contains(&second));
    assert_eq!(snapshots(dir.path()).len(), 1);
}

#[gpui::test]
fn an_unreferenced_snapshot_and_abandoned_staging_are_removed(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let extension = extension(dir.path(), "v1");
    let out = dir.path().join("out");
    let server = server(&extension, &out);
    consenting(cx, dir.path());
    let hash = grant(cx, &server);
    let store = dir.path().join(STORE);

    // Left by an earlier run: an unreferenced snapshot, and staging dirs.
    let stray = "0".repeat(64);
    fs::create_dir_all(store.join(&stray).join("lib")).unwrap();
    fs::write(store.join(&stray).join("lib/x"), "x").unwrap();
    fs::set_permissions(
        store.join(&stray).join("lib"),
        fs::Permissions::from_mode(0o555),
    )
    .unwrap();
    fs::create_dir_all(store.join(".staging-1-1/sub")).unwrap();
    fs::write(store.join(".staging-1-1/sub/f"), "x").unwrap();
    fs::create_dir_all(store.join(".staging-2-2")).unwrap();
    // One still being made holds its lock.
    let busy = fs::File::create(store.join(".staging-2-2.lock")).unwrap();
    busy.lock().unwrap();

    set(cx, &server);
    assert_eq!(
        snapshots(dir.path()),
        [".staging-2-2".to_owned(), hash.clone()]
    );
    drop(busy);
    set(cx, &server);
    assert_eq!(snapshots(dir.path()), [hash]);
}

#[gpui::test]
fn a_collected_snapshot_keeps_its_lock_file(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let extension = extension(dir.path(), "v1");
    let server = server(&extension, &dir.path().join("out"));
    consenting(cx, dir.path());
    grant(cx, &server);
    let store = dir.path().join(STORE);
    let stray = "1".repeat(64);
    fs::create_dir_all(store.join(&stray)).unwrap();
    let lock = store.join(format!("{stray}.lock"));
    fs::write(&lock, "").unwrap();
    let before = fs::metadata(&lock).unwrap().ino();
    set(cx, &server);
    assert!(!store.join(&stray).exists());
    // The lock file others may be waiting on is the same one.
    assert_eq!(fs::metadata(&lock).unwrap().ino(), before);
}

#[gpui::test]
fn a_store_that_cannot_be_made_fails_closed(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let extension = extension(dir.path(), "v1");
    let server = server(&extension, &dir.path().join("out"));
    // The consent file's directory is a file.
    let blocked = dir.path().join("blocked");
    fs::write(&blocked, "").unwrap();
    cx.update(|cx| set_extension_consent_file(cx, blocked.join(CONSENT_FILE)));
    set(cx, &server);
    let asked = pending(cx).remove(0);
    assert_eq!(asked.invocation.package_hash, "incomplete");
    assert!(cx.update(|cx| grant_extension_server(cx, &asked)).is_err());
}

#[gpui::test]
fn identical_packages_in_other_roots_are_checked_for_symlinks_again(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out");
    consenting(cx, dir.path());
    let (a, b) = (
        extension(&dir.path().join("a"), "v1"),
        extension(&dir.path().join("b"), "v1"),
    );
    // The same link in both: into `b`, which for `a` is elsewhere.
    for extension in [&a, &b] {
        std::os::unix::fs::symlink(b.join("server.sh"), extension.join("link")).unwrap();
    }
    set(cx, &server(&a, &out));
    let hash = pending(cx)[0].invocation.package_hash.clone();
    assert_ne!(hash, "incomplete");
    // Its snapshot exists, and `b` can't reuse it.
    assert!(dir.path().join(STORE).join(&hash).is_dir());
    set(cx, &server(&b, &out));
    assert_eq!(pending(cx)[0].invocation.package_hash, "incomplete");
}

#[gpui::test]
fn a_writable_snapshot_left_by_a_crash_is_replaced_not_reused(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let extension = extension(dir.path(), "v1");
    let server = server(&extension, &dir.path().join("out"));
    consenting(cx, dir.path());
    let hash = package_hash(&extension);
    // A copy under the hash's name, left unfinished: still writable.
    let left = dir.path().join(STORE).join(&hash);
    fs::create_dir_all(left.join("lib")).unwrap();
    for file in ["server.sh", "lib/util.sh"] {
        fs::copy(extension.join(file), left.join(file)).unwrap();
    }
    assert_eq!(package_hash(&left), hash);
    set(cx, &server);
    assert_eq!(pending(cx)[0].invocation.package_hash, hash);
    for file in ["server.sh", "lib/util.sh"] {
        let mode = fs::metadata(left.join(file)).unwrap().permissions().mode();
        assert_eq!(mode & 0o222, 0, "{file}");
    }
}

#[gpui::test]
fn two_instances_never_collect_each_others_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let extension = extension(dir.path(), "v1");
    let consent = dir.path().join(CONSENT_FILE);
    let (first, second) = (Snapshots::default(), Snapshots::default());
    let hash = {
        let mut store = first.begin(Some(&consent)).unwrap();
        let (hash, _) = store.snapshot(&extension).unwrap();
        store.keep([hash.clone()].into());
        hash
    };
    // The other, which keeps nothing, collects: this one's stays.
    {
        let mut store = second.begin(Some(&consent)).unwrap();
        store.keep(Default::default());
        store.collect();
    }
    assert!(dir.path().join(STORE).join(&hash).is_dir());
    // Once this one lets go, it's collected.
    first
        .begin(Some(&consent))
        .unwrap()
        .keep(Default::default());
    second.begin(Some(&consent)).unwrap().collect();
    assert!(!dir.path().join(STORE).join(&hash).exists());
}

#[gpui::test]
fn the_invocation_binds_what_the_snapshot_holds_not_the_live_directory() {
    let dir = tempfile::tempdir().unwrap();
    let extension = extension(dir.path(), "v1");
    let out = dir.path().join("out");
    let spec = server(&extension, &out).spec;
    let snapshots = Snapshots::default();
    let (hash, snapshot) = snapshots
        .begin(Some(&dir.path().join(CONSENT_FILE)))
        .unwrap()
        .snapshot(&extension)
        .unwrap();
    // The live entry goes after the copy, before the invocation is made.
    fs::remove_file(extension.join("server.sh")).unwrap();
    let invocation =
        Invocation::read_from(&spec, PathBuf::from("/bin/sh"), &extension, &snapshot, hash);
    assert_eq!(
        invocation.script_hashes.keys().collect::<Vec<_>>(),
        ["server.sh"]
    );
}

/// A process that notes whether its snapshot was still locked when it was
/// torn down.
struct Double {
    lock: PathBuf,
    locked: Arc<std::sync::atomic::AtomicBool>,
}

impl fe_lsp::ServerProcess for Double {
    fn exited(
        &mut self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = fe_lsp::ExitInfo> + Send + '_>> {
        Box::pin(std::future::pending())
    }

    fn kill(&mut self) {}
}

impl Drop for Double {
    fn drop(&mut self) {
        let other = fs::File::open(&self.lock).unwrap();
        let held = other.try_lock().is_err();
        self.locked.store(held, std::sync::atomic::Ordering::SeqCst);
    }
}

struct DoubleLauncher {
    lock: PathBuf,
    locked: Arc<std::sync::atomic::AtomicBool>,
}

impl ServerLauncher for DoubleLauncher {
    fn launch(
        &self,
        _spec: &ServerSpec,
        _root: &Path,
        _cancel: &LaunchCancel,
    ) -> std::io::Result<ServerIo> {
        Ok(ServerIo {
            input: Box::pin(futures::io::sink()),
            output: Box::pin(futures::io::empty()),
            stderr: None,
            process: Box::new(Double {
                lock: self.lock.clone(),
                locked: self.locked.clone(),
            }),
        })
    }
}

#[gpui::test]
fn a_snapshot_stays_locked_until_its_process_is_torn_down(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let extension = extension(dir.path(), "v1");
    let server = server(&extension, &dir.path().join("out"));
    consenting(cx, dir.path());
    let hash = grant(cx, &server);
    let locked = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let launcher = DoubleLauncher {
        lock: dir.path().join(STORE).join(format!("{hash}.lock")),
        locked: locked.clone(),
    };
    let gate = cx.update(|cx| ConsentGate::new(cx, Arc::new(launcher)));
    let io = launch(&gate, &server.spec, dir.path()).unwrap();
    // A new version is granted: the app keeps the old no more, so only
    // the process's lock is left on it.
    fs::write(extension.join("lib/util.sh"), "VALUE=v2\n").unwrap();
    grant(cx, &server);
    assert!(dir.path().join(STORE).join(&hash).is_dir());
    drop(io);
    // Torn down while that lock was held.
    assert!(locked.load(std::sync::atomic::Ordering::SeqCst));
}

#[gpui::test]
fn an_executable_the_package_holds_runs_from_the_snapshot_though_the_live_one_is_gone(
    cx: &mut TestAppContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let extension = extension(dir.path(), "v1");
    fs::create_dir_all(extension.join("bin")).unwrap();
    fs::write(extension.join("bin/run"), "#!/bin/sh\n").unwrap();
    fs::set_permissions(extension.join("bin/run"), fs::Permissions::from_mode(0o755)).unwrap();
    let mut server = server(&extension, &dir.path().join("out"));
    // A bare name, found on a PATH inside the package.
    server.spec.command = "run".into();
    server.spec.args = vec!["--stdio".into()];
    server.spec.env.push((
        "PATH".into(),
        extension.join("bin").to_string_lossy().into_owned(),
    ));
    consenting(cx, dir.path());
    let hash = grant(cx, &server);
    let snapshot = dunce::canonicalize(dir.path().join(STORE).join(hash)).unwrap();
    // An update removes it.
    fs::remove_file(extension.join("bin/run")).unwrap();

    let recorder = Arc::new(Recorder::default());
    let gate = cx.update(|cx| ConsentGate::new(cx, recorder.clone()));
    assert_eq!(
        launch(&gate, &server.spec, dir.path())
            .err()
            .unwrap()
            .to_string(),
        "recorded"
    );
    let bound = recorder.0.lock().unwrap().remove(0);
    assert_eq!(bound.command, snapshot.join("bin/run"));
}

#[gpui::test]
fn a_private_directory_is_new_and_an_allocation_failure_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let made = private_dir_in(dir.path()).unwrap();
    assert!(made.starts_with(dir.path()));
    assert_eq!(fs::metadata(&made).unwrap().permissions().mode() & 0o077, 0);
    assert_ne!(private_dir_in(dir.path()).unwrap(), made);
    // Nothing to make it in: an error, never a path.
    assert!(private_dir_in(&dir.path().join("missing/deeper")).is_err());
}

#[gpui::test]
fn staging_locks_are_reused_never_unlinked(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let extension = extension(dir.path(), "v1");
    let server = server(&extension, &dir.path().join("out"));
    consenting(cx, dir.path());
    grant(cx, &server);
    let store = dir.path().join(STORE);
    let locks = |store: &Path| -> Vec<(String, u64)> {
        let mut found: Vec<_> = fs::read_dir(store)
            .unwrap()
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(".staging-"))
            .map(|entry| {
                (
                    entry.file_name().to_string_lossy().into_owned(),
                    entry.metadata().unwrap().ino(),
                )
            })
            .collect();
        found.sort();
        found
    };
    let before = locks(&store);
    assert_eq!(before.len(), 1, "{before:?}");
    // Another version is made and a collection runs: the same lock file.
    fs::write(extension.join("lib/util.sh"), "VALUE=v2\n").unwrap();
    grant(cx, &server);
    assert_eq!(locks(&store), before);
}

#[gpui::test]
fn permission_only_damage_is_refused_at_launch(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let extension = extension(dir.path(), "v1");
    let server = server(&extension, &dir.path().join("out"));
    consenting(cx, dir.path());
    let hash = grant(cx, &server);
    let file = dir.path().join(STORE).join(&hash).join("lib/util.sh");
    // The bytes are the same; only the write bit comes back.
    fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
    let gate = cx.update(|cx| ConsentGate::new(cx, Arc::new(Recorder::default())));
    let error = launch(&gate, &server.spec, dir.path()).err().unwrap();
    assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
}

#[gpui::test]
fn an_unfinished_older_grant_is_not_run_by_a_later_session(
    cx: &mut TestAppContext,
    later: &mut TestAppContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let extension = extension(dir.path(), "v1");
    let server = server(&extension, &dir.path().join("out"));
    consenting(cx, dir.path());
    let hash = grant(cx, &server);
    // Interrupted finalisation: the old snapshot's write bit is back, and
    // the installed package has moved on.
    let file = dir.path().join(STORE).join(&hash).join("lib/util.sh");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
    fs::write(extension.join("lib/util.sh"), "VALUE=v2\n").unwrap();
    consenting(later, dir.path());
    set(later, &server);
    assert_eq!(later.update(|cx| crate::extension_servers(cx)), []);
}

#[gpui::test]
fn an_older_grant_this_session_runs_is_kept_from_another_instance() {
    let dir = tempfile::tempdir().unwrap();
    let extension = extension(dir.path(), "v1");
    let consent = dir.path().join(CONSENT_FILE);
    let hash = {
        let snapshots = Snapshots::default();
        let mut made = snapshots.begin(Some(&consent)).unwrap();
        made.snapshot(&extension).unwrap().0
    };
    let (first, second) = (Snapshots::default(), Snapshots::default());
    {
        // Startup: the stored grant is the older version, verified.
        let mut store = first.begin(Some(&consent)).unwrap();
        assert!(store.verified(&hash).is_some());
        store.keep([hash.clone()].into());
    }
    {
        let mut store = second.begin(Some(&consent)).unwrap();
        store.keep(Default::default());
        store.collect();
    }
    assert!(dir.path().join(STORE).join(&hash).is_dir());
}

#[gpui::test]
fn a_name_bound_in_the_snapshot_stays_bound_when_the_live_alias_moves(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let extension = extension(dir.path(), "v1");
    let elsewhere = dir.path().join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    fs::write(elsewhere.join("server.sh"), "evil\n").unwrap();
    // An absolute alias of the package's directory.
    let alias = dir.path().join("alias");
    std::os::unix::fs::symlink(&extension, &alias).unwrap();
    let mut server = server(&extension, &dir.path().join("out"));
    server.spec.args = vec![
        alias.join("server.sh").to_string_lossy().into_owned(),
        format!("{}/lib/../server.sh", extension.display()),
    ];
    consenting(cx, dir.path());
    let hash = grant(cx, &server);
    let snapshot = dunce::canonicalize(dir.path().join(STORE).join(hash)).unwrap();
    // The alias is retargeted after the grant.
    fs::remove_file(&alias).unwrap();
    std::os::unix::fs::symlink(&elsewhere, &alias).unwrap();

    let recorder = Arc::new(Recorder::default());
    let gate = cx.update(|cx| ConsentGate::new(cx, recorder.clone()));
    assert!(launch(&gate, &server.spec, dir.path()).is_err());
    let bound = recorder.0.lock().unwrap().remove(0);
    assert_eq!(
        bound.args,
        [
            snapshot.join("server.sh").to_string_lossy(),
            snapshot.join("server.sh").to_string_lossy()
        ]
    );
}

#[gpui::test]
fn a_collection_between_open_and_lock_cannot_unlink_or_replace_a_staging_lock() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    };
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join(STORE);
    fs::create_dir_all(&store).unwrap();
    let (opened_tx, opened_rx) = mpsc::channel::<()>();
    let (go_tx, go_rx) = mpsc::channel::<()>();
    let channels = Mutex::new((opened_tx, go_rx));
    let first = AtomicBool::new(true);
    // Only the first creator pauses, after opening its lock, before taking it.
    pause_after_open(
        store.clone(),
        Arc::new(move || {
            if first.swap(false, Ordering::SeqCst) {
                let (opened, go) = &*channels.lock().unwrap();
                opened.send(()).unwrap();
                go.recv().unwrap();
            }
        }),
    );
    let creator = {
        let store = store.clone();
        std::thread::spawn(move || staging_slot(&store).unwrap())
    };
    opened_rx.recv().unwrap();

    // Paused holding the file open but not locked. The lock file is there.
    let lock = store.join(format!(".staging-{}-0.lock", std::process::id()));
    let ino = fs::metadata(&lock).unwrap().ino();
    // Two collection passes, and a second creator, run meanwhile.
    collect_unkept(&store);
    collect_unkept(&store);
    let (second, second_lock) = staging_slot(&store).unwrap();
    collect_unkept(&store);
    // Nothing unlinked or replaced the file the first creator opened.
    assert_eq!(fs::metadata(&lock).unwrap().ino(), ino);

    go_tx.send(()).unwrap();
    let (first, first_lock) = creator.join().unwrap();
    // The two hold different slots: the second had slot 0, so the first,
    // whose open was of the same file, found it locked and took the next.
    assert!(second.ends_with("-0"), "{second}");
    assert!(first.ends_with("-1"), "{first}");
    assert_eq!(fs::metadata(&lock).unwrap().ino(), ino);
    // A staging directory being made under a lock isn't collected.
    fs::create_dir(store.join(&first)).unwrap();
    fs::create_dir(store.join(&second)).unwrap();
    collect_unkept(&store);
    assert!(store.join(&first).is_dir() && store.join(&second).is_dir());
    drop((first_lock, second_lock));
    collect_unkept(&store);
    assert!(!store.join(&first).exists() && !store.join(&second).exists());
    // And the lock files remain, for the next creators.
    assert_eq!(fs::metadata(&lock).unwrap().ino(), ino);
}

#[gpui::test]
fn a_dangling_chain_of_links_out_of_the_package_is_not_grantable(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let extension = extension(&dir.path().join("a"), "v1");
    consenting(cx, dir.path());
    // `alias` is the package's own directory, so `escape` climbs out of it
    // through a destination that doesn't exist (yet).
    std::os::unix::fs::symlink(".", extension.join("alias")).unwrap();
    std::os::unix::fs::symlink("alias/../outside/missing", extension.join("escape")).unwrap();
    assert!(!extension.parent().unwrap().join("outside").exists());
    set(cx, &server(&extension, &dir.path().join("out")));
    let asked = pending(cx).remove(0);
    assert_eq!(asked.invocation.package_hash, "incomplete");
    assert!(cx.update(|cx| grant_extension_server(cx, &asked)).is_err());

    // A chain that stays inside is fine, though its end is missing too.
    fs::remove_file(extension.join("escape")).unwrap();
    std::os::unix::fs::symlink("alias/lib/missing", extension.join("inside")).unwrap();
    set(cx, &server(&extension, &dir.path().join("out")));
    assert_ne!(pending(cx)[0].invocation.package_hash, "incomplete");
}

#[gpui::test]
fn a_permission_only_update_makes_a_new_executable_snapshot(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let extension = extension(dir.path(), "v1");
    let server = server(&extension, &dir.path().join("out"));
    consenting(cx, dir.path());
    let first = grant(cx, &server);
    let copy = |hash: &str| dir.path().join(STORE).join(hash).join("lib/util.sh");
    assert_eq!(
        fs::metadata(copy(&first)).unwrap().permissions().mode() & 0o111,
        0
    );
    // The helper becomes executable, its bytes the same.
    fs::set_permissions(
        extension.join("lib/util.sh"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    set(cx, &server);
    let asked = pending(cx).remove(0);
    assert_ne!(asked.invocation.package_hash, first);
    assert_eq!(
        fs::metadata(copy(&asked.invocation.package_hash))
            .unwrap()
            .permissions()
            .mode()
            & 0o111,
        0o111
    );
    // The old one is the same bytes without the bit.
    assert_eq!(
        fs::metadata(copy(&first)).unwrap().permissions().mode() & 0o111,
        0
    );
}

#[gpui::test]
fn a_chain_of_links_that_resolves_out_of_the_package_is_not_grantable(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let extension = extension(&dir.path().join("a"), "v1");
    consenting(cx, dir.path());
    // `first -> second -> ../../elsewhere`, which exists.
    fs::create_dir_all(dir.path().join("elsewhere")).unwrap();
    std::os::unix::fs::symlink("../../elsewhere", extension.join("second")).unwrap();
    std::os::unix::fs::symlink("second", extension.join("first")).unwrap();
    set(cx, &server(&extension, &dir.path().join("out")));
    let asked = pending(cx).remove(0);
    assert_eq!(asked.invocation.package_hash, "incomplete");
    assert!(cx.update(|cx| grant_extension_server(cx, &asked)).is_err());
}

#[gpui::test]
fn a_dangling_chain_through_an_absolute_link_to_the_live_package_is_not_grantable(
    cx: &mut TestAppContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let extension = extension(&dir.path().join("a"), "v1");
    consenting(cx, dir.path());
    let parent = extension.parent().unwrap();
    let name = extension
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    // `alias` holds the live package; `escape` reaches a file it will have.
    std::os::unix::fs::symlink(parent, extension.join("alias")).unwrap();
    std::os::unix::fs::symlink(format!("alias/{name}/future.js"), extension.join("escape"))
        .unwrap();
    assert!(!extension.join("future.js").exists());
    set(cx, &server(&extension, &dir.path().join("out")));
    let asked = pending(cx).remove(0);
    assert_eq!(asked.invocation.package_hash, "incomplete");
    assert!(cx.update(|cx| grant_extension_server(cx, &asked)).is_err());

    // An absolute link elsewhere, itself leading back to a missing file in
    // the live package: refused along the chain.
    fs::remove_file(extension.join("alias")).unwrap();
    fs::remove_file(extension.join("escape")).unwrap();
    let elsewhere = dir.path().join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    std::os::unix::fs::symlink(extension.join("future.js"), elsewhere.join("back")).unwrap();
    std::os::unix::fs::symlink(&elsewhere, extension.join("hop")).unwrap();
    std::os::unix::fs::symlink("hop/back", extension.join("via")).unwrap();
    set(cx, &server(&extension, &dir.path().join("out")));
    assert_eq!(pending(cx)[0].invocation.package_hash, "incomplete");

    // A standalone absolute link to somewhere unrelated is fine, even with
    // nothing at its end; a relative path through it, whose end is missing,
    // leaves the package and is not.
    fs::remove_file(extension.join("via")).unwrap();
    set(cx, &server(&extension, &dir.path().join("out")));
    assert_ne!(pending(cx)[0].invocation.package_hash, "incomplete");
    std::os::unix::fs::symlink("hop/missing", extension.join("via")).unwrap();
    set(cx, &server(&extension, &dir.path().join("out")));
    assert_eq!(pending(cx)[0].invocation.package_hash, "incomplete");
}

#[gpui::test]
fn an_absolute_link_reaching_the_live_package_through_dotdot_or_a_link_is_not_grantable(
    cx: &mut TestAppContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let extension = extension(&dir.path().join("a"), "v1");
    consenting(cx, dir.path());
    let parent = extension.parent().unwrap().to_owned();
    let name = extension
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    fs::create_dir_all(parent.join("other")).unwrap();
    let ungrantable = |cx: &mut TestAppContext, link: &Path| {
        set(cx, &server(&extension, &dir.path().join("out")));
        let hash = pending(cx)[0].invocation.package_hash.clone();
        fs::remove_file(link).unwrap();
        hash == "incomplete"
    };
    // `..` after an existing directory, to a file the live package will have.
    let dotdot = extension.join("dotdot");
    std::os::unix::fs::symlink(
        format!("{}/other/../{name}/future.js", parent.display()),
        &dotdot,
    )
    .unwrap();
    assert!(ungrantable(cx, &dotdot), "dotdot");
    // An intermediate link that leads to the live package.
    std::os::unix::fs::symlink(&extension, parent.join("other/inner")).unwrap();
    let through = extension.join("through");
    std::os::unix::fs::symlink(
        format!("{}/other/inner/future.js", parent.display()),
        &through,
    )
    .unwrap();
    assert!(ungrantable(cx, &through), "through");
    // And a link to somewhere else entirely is still fine.
    std::os::unix::fs::symlink(parent.join("other/missing"), extension.join("elsewhere")).unwrap();
    set(cx, &server(&extension, &dir.path().join("out")));
    assert_ne!(pending(cx)[0].invocation.package_hash, "incomplete");
}

#[gpui::test]
fn a_link_cycle_or_an_overlong_chain_fails_closed(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let extension = extension(&dir.path().join("a"), "v1");
    consenting(cx, dir.path());
    let grantable = |cx: &mut TestAppContext| {
        set(cx, &server(&extension, &dir.path().join("out")));
        pending(cx)[0].invocation.package_hash != "incomplete"
    };
    assert!(grantable(cx));
    // A link to itself, then two to each other.
    std::os::unix::fs::symlink("self", extension.join("self")).unwrap();
    assert!(!grantable(cx));
    fs::remove_file(extension.join("self")).unwrap();
    std::os::unix::fs::symlink("two", extension.join("one")).unwrap();
    std::os::unix::fs::symlink("one", extension.join("two")).unwrap();
    assert!(!grantable(cx));
    fs::remove_file(extension.join("one")).unwrap();
    fs::remove_file(extension.join("two")).unwrap();
    assert!(grantable(cx));
    // A chain of links within the bound is fine; one past it is refused,
    // though every link stays inside the package.
    let chain = |length: usize| {
        for n in 0..length {
            let target = if n + 1 == length {
                "server.sh".to_owned()
            } else {
                format!("link{}", n + 1)
            };
            std::os::unix::fs::symlink(target, extension.join(format!("link{n}"))).unwrap();
        }
    };
    let clear = |length: usize| {
        for n in 0..length {
            fs::remove_file(extension.join(format!("link{n}"))).unwrap();
        }
    };
    chain(30);
    assert!(grantable(cx));
    clear(30);
    chain(60);
    assert!(!grantable(cx));
    clear(60);
}

#[test]
fn resolution_is_by_the_filesystem_hop_by_hop_and_keeps_an_absolute_start() {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    fs::create_dir_all(root.join("real/sub")).unwrap();
    std::os::unix::fs::symlink(root.join("real/sub"), root.join("alias")).unwrap();
    // `..` after a link applies to where the link leads.
    std::os::unix::fs::symlink("real/sub", root.join("rel")).unwrap();
    let (end, absolute) = resolve(&root, Path::new("rel/../file")).unwrap();
    assert_eq!((end, absolute), (root.join("real/file"), false));
    // Through an absolute link it says so.
    let (end, absolute) = resolve(&root, Path::new("alias/../file")).unwrap();
    assert_eq!((end, absolute), (root.join("real/file"), true));
    // An absolute target starts over, and says so.
    let (end, absolute) = resolve(&root, &root.join("real/sub/../x")).unwrap();
    assert_eq!((end, absolute), (root.join("real/x"), true));
    // A relative link out of a package is refused, one inside is not.
    assert!(link_escapes(
        &root.join("real"),
        &root,
        &root.join("real"),
        Path::new("../x")
    ));
    assert!(!link_escapes(
        &root.join("real"),
        &root,
        &root.join("real"),
        Path::new("sub/x")
    ));
}
