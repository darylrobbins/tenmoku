// SPDX-License-Identifier: Apache-2.0

//! Plugin grants: the load, prompt or escalation decision, program
//! identities, and the store, kept in temporary directories. The programs
//! are scripts, `#!/bin/sh` on Unix and `.cmd` on Windows.

#[path = "support/script.rs"]
mod support;

use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};

use kiln_command::capability::{Capability, Grants};
use kiln_plugin::grants::{
    Decision, GrantRecord, GrantStore, GrantStoreError, Prompt, SuspendReason, check_program,
    evaluate, needs_wildcard_acceptance, resolve_program,
};
use sha2::{Digest as _, Sha256};

const ID: &str = "acme.tool";

/// A `PATH` for decisions that resolve no program.
fn no_path() -> &'static OsStr {
    OsStr::new("")
}

fn grants(capabilities: &[&str]) -> Grants {
    capabilities
        .iter()
        .map(|capability| Capability::parse(capability).unwrap())
        .collect()
}

fn installed(version: (u32, u32, u32), granted: &[&str]) -> GrantRecord {
    GrantRecord {
        version: Some(version),
        granted: grants(granted),
        ..GrantRecord::default()
    }
}

/// A directory of its own under cargo's temporary directory for tests.
fn temp_dir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("kiln-plugin-grants-{}-{name}", std::process::id()));
    fs::remove_dir_all(&dir).ok();
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Writes an executable script `name` into `dir`, echoing `said`, and returns its path.
fn program(dir: &Path, name: &str, said: &str) -> PathBuf {
    support::script(dir, name, &format!("echo {said}"), &format!("echo {said}"))
}

fn path_var(dirs: &[&Path]) -> OsString {
    std::env::join_paths(dirs).unwrap()
}

#[test]
fn an_unknown_plugin_needs_a_prompt() {
    let requested = grants(&["buffer.read", "net:api.example.com"]);
    assert_eq!(
        evaluate(None, &requested),
        Decision::Prompt(Prompt::NewPlugin)
    );

    let store = GrantStore::new(temp_dir("unknown").join("plugin-grants.toml"));
    let decision = store.decide(ID, (1, 0, 0), &requested, no_path()).unwrap();
    assert_eq!(decision, Decision::Prompt(Prompt::NewPlugin));
    assert_eq!(store.get(ID).unwrap(), None, "nothing is granted or loaded");
}

#[test]
fn the_same_request_loads_from_the_record() {
    let requested = grants(&["buffer.read", "commands", "net:api.example.com"]);
    let record = installed(
        (1, 0, 0),
        &["buffer.read", "commands", "net:api.example.com"],
    );
    assert_eq!(
        evaluate(Some(&record), &requested),
        Decision::Load(requested.clone())
    );
}

#[test]
fn new_or_wider_capabilities_are_an_escalation() {
    let record = installed((1, 0, 0), &["buffer.read", "net:a.example.com"]);
    let escalation = |requested: &[&str], added: &[&str]| {
        assert_eq!(
            evaluate(Some(&record), &grants(requested)),
            Decision::Escalation {
                added: grants(added),
                keep_running: Some((1, 0, 0)),
            },
            "{requested:?}"
        );
    };
    escalation(
        &["buffer.read", "buffer.write", "net:a.example.com"],
        &["buffer.write"],
    );
    escalation(
        &["buffer.read", "net:a.example.com", "net:b.example.com"],
        &["net:b.example.com"],
    );
    escalation(&["buffer.read", "net:*"], &["net:*"]);
    escalation(
        &["buffer.read", "net:a.example.com", "process.spawn:*"],
        &["process.spawn:*"],
    );
}

#[test]
fn requesting_less_applies_without_a_prompt() {
    let store = GrantStore::new(temp_dir("less").join("plugin-grants.toml"));
    store
        .set(
            ID,
            installed((1, 0, 0), &["buffer.read", "buffer.write", "net:*"]),
        )
        .unwrap();

    let narrower = grants(&["buffer.read", "net:api.example.com"]);
    let decision = store.decide(ID, (1, 1, 0), &narrower, no_path()).unwrap();
    assert_eq!(decision, Decision::Load(narrower.clone()));
    let record = store.get(ID).unwrap().unwrap();
    assert_eq!(record.granted, narrower, "the record shrank to the request");
    assert_eq!(record.version, Some((1, 1, 0)));

    // `process.spawn:*` narrowed to one program binds it, unasked.
    let dir = temp_dir("less-spawn");
    program(&dir, "tool", "tool");
    let mut any = installed((1, 0, 0), &["buffer.read", "process.spawn:*"]);
    any.wildcards_accepted = true;
    store.set(ID, any).unwrap();
    let narrower = grants(&["buffer.read", "process.spawn:tool"]);
    let decision = store
        .decide(ID, (1, 1, 0), &narrower, &path_var(&[&dir]))
        .unwrap();
    assert_eq!(decision, Decision::Load(narrower.clone()));
    let record = store.get(ID).unwrap().unwrap();
    assert_eq!(record.granted, narrower);
    assert!(!record.wildcards_accepted);
    assert_eq!(
        record.programs.get("tool"),
        Some(&resolve_program("tool", &path_var(&[&dir])).unwrap())
    );

    // A program that can't be bound, alone or beside one that can, fails
    // the narrowing: nothing loads and the record, version and all, stays.
    let mut wide = installed((2, 0, 0), &["net:*", "process.spawn:*"]);
    wide.wildcards_accepted = true;
    store.set(ID, wide.clone()).unwrap();
    for requested in [
        &["process.spawn:missing"][..],
        &["process.spawn:tool", "process.spawn:missing"],
    ] {
        let failed = store.decide(ID, (2, 1, 0), &grants(requested), &path_var(&[&dir]));
        assert!(
            matches!(&failed, Err(GrantStoreError::Unresolved { program, .. }) if program == "missing"),
            "{requested:?}: {failed:?}"
        );
        assert_eq!(store.get(ID).unwrap(), Some(wide.clone()), "{requested:?}");
    }

    // Once a narrowing does load, what it dropped is asked for again.
    let narrower = grants(&["process.spawn:tool"]);
    let decision = store
        .decide(ID, (2, 1, 0), &narrower, &path_var(&[&dir]))
        .unwrap();
    assert_eq!(decision, Decision::Load(narrower));
    assert_eq!(
        store
            .decide(
                ID,
                (2, 2, 0),
                &grants(&["net:*", "process.spawn:tool"]),
                no_path()
            )
            .unwrap(),
        Decision::Escalation {
            added: grants(&["net:*"]),
            keep_running: Some((2, 1, 0)),
        }
    );
}

#[test]
fn the_old_version_keeps_running_while_an_escalation_is_pending() {
    let store = GrantStore::new(temp_dir("pending").join("plugin-grants.toml"));
    store
        .set(ID, installed((1, 2, 3), &["buffer.read"]))
        .unwrap();

    let wider = grants(&["buffer.read", "status"]);
    let decision = store.decide(ID, (2, 0, 0), &wider, no_path()).unwrap();
    assert_eq!(
        decision,
        Decision::Escalation {
            added: grants(&["status"]),
            keep_running: Some((1, 2, 3)),
        }
    );
    let record = store.get(ID).unwrap().unwrap();
    assert_eq!(record.version, Some((1, 2, 3)), "the update isn't recorded");
    assert_eq!(record.granted, grants(&["buffer.read"]));
}

#[test]
fn a_declined_request_is_not_asked_again_until_it_changes() {
    let store = GrantStore::new(temp_dir("declined").join("plugin-grants.toml"));
    let asked = grants(&["buffer.read", "net:api.example.com"]);
    let other = grants(&["buffer.read"]);

    // A new plugin's first request.
    store.decline(ID, &asked).unwrap();
    assert_eq!(
        store.decide(ID, (1, 0, 0), &asked, no_path()).unwrap(),
        Decision::Declined
    );
    assert_eq!(
        store.decide(ID, (1, 0, 1), &other, no_path()).unwrap(),
        Decision::Prompt(Prompt::NewPlugin)
    );

    // An installed plugin's escalation.
    store
        .set(ID, installed((1, 0, 0), &["buffer.read"]))
        .unwrap();
    let wider = grants(&["buffer.read", "status"]);
    store.decline(ID, &wider).unwrap();
    assert_eq!(
        store.decide(ID, (2, 0, 0), &wider, no_path()).unwrap(),
        Decision::Declined
    );
    assert_eq!(
        store
            .decide(
                ID,
                (2, 0, 1),
                &grants(&["buffer.read", "decorations"]),
                no_path()
            )
            .unwrap(),
        Decision::Escalation {
            added: grants(&["decorations"]),
            keep_running: Some((1, 0, 0)),
        }
    );
    assert_eq!(
        store.get(ID).unwrap().unwrap().granted,
        grants(&["buffer.read"]),
        "declining keeps what was granted"
    );
}

#[test]
fn wildcards_need_their_own_acceptance() {
    let store = GrantStore::new(temp_dir("wildcards").join("plugin-grants.toml"));
    for wildcard in ["net:*", "process.spawn:*"] {
        let requested = grants(&["buffer.read", wildcard]);
        assert!(needs_wildcard_acceptance(&requested));
        let refused = store.grant(ID, (1, 0, 0), &requested, &[], false);
        assert!(
            matches!(&refused, Err(GrantStoreError::WildcardsNotAccepted(added)) if *added == grants(&[wildcard])),
            "{refused:?}"
        );
        assert_eq!(store.get(ID).unwrap(), None, "nothing was granted");

        let record = store.grant(ID, (1, 0, 0), &requested, &[], true).unwrap();
        assert!(record.wildcards_accepted);
        assert_eq!(store.get(ID).unwrap(), Some(record));
        store.forget(ID).unwrap();
    }
    assert!(!needs_wildcard_acceptance(&grants(&[
        "net:api.example.com",
        "process.spawn:rg"
    ])));
}

#[test]
fn a_spawn_grant_records_the_absolute_path_and_identity() {
    let dir = temp_dir("resolve");
    let (first, second) = (dir.join("first"), dir.join("second"));
    fs::create_dir_all(&first).unwrap();
    fs::create_dir_all(&second).unwrap();
    // Not executable, so passed over.
    fs::write(first.join("tool"), "data\n").unwrap();
    let tool = program(&second, "tool", "tool");
    let content = fs::read(&tool).unwrap();

    let path = path_var(&[Path::new("relative"), &first, &second]);
    let identity = resolve_program("tool", &path).unwrap();
    assert_eq!(identity.name, "tool");
    assert_eq!(identity.path, tool);
    assert!(identity.path.is_absolute());
    assert_eq!(identity.size, content.len() as u64);
    assert_eq!(identity.sha256, format!("{:x}", Sha256::digest(&content)));
    assert_eq!(check_program(&identity), Ok(()));

    assert!(resolve_program("missing", &path).is_err());
    assert!(resolve_program("second/tool", &path_var(&[&dir])).is_err());

    let store = GrantStore::new(dir.join("plugin-grants.toml"));
    let requested = grants(&["process.spawn:tool"]);
    assert!(matches!(
        store.grant(ID, (1, 0, 0), &requested, &[], false),
        Err(GrantStoreError::ProgramNotResolved(program)) if program == "tool"
    ));
    let record = store
        .grant(
            ID,
            (1, 0, 0),
            &requested,
            std::slice::from_ref(&identity),
            false,
        )
        .unwrap();
    assert_eq!(record.programs.get("tool"), Some(&identity));
    assert_eq!(store.get(ID).unwrap().unwrap().programs["tool"], identity);
}

#[test]
fn a_changed_or_missing_program_suspends_the_grant() {
    let dir = temp_dir("suspend");
    let tool = program(&dir, "tool", "one");
    let identity = resolve_program("tool", &path_var(&[&dir])).unwrap();
    let store = GrantStore::new(dir.join("state").join("plugin-grants.toml"));
    let requested = grants(&["buffer.read", "process.spawn:tool"]);
    store
        .grant(
            ID,
            (1, 0, 0),
            &requested,
            std::slice::from_ref(&identity),
            false,
        )
        .unwrap();
    assert_eq!(store.check_programs(ID).unwrap(), vec![]);
    assert_eq!(
        store.decide(ID, (1, 0, 0), &requested, no_path()).unwrap(),
        Decision::Load(requested.clone())
    );

    assert_eq!(program(&dir, "tool", "two"), tool);
    assert_eq!(
        check_program(&identity).map_err(|suspended| suspended.reason),
        Err(SuspendReason::Changed)
    );
    let suspended = store.check_programs(ID).unwrap();
    assert_eq!(suspended.len(), 1);
    assert_eq!(
        (suspended[0].0.as_str(), suspended[0].1.reason),
        ("tool", SuspendReason::Changed)
    );
    let record = store.get(ID).unwrap().unwrap();
    assert_eq!(record.suspended.get("tool"), Some(&SuspendReason::Changed));
    assert_eq!(
        store.decide(ID, (1, 0, 0), &requested, no_path()).unwrap(),
        Decision::Escalation {
            added: grants(&["process.spawn:tool"]),
            keep_running: Some((1, 0, 0)),
        }
    );

    // Granted again, then gone.
    let identity = resolve_program("tool", &path_var(&[&dir])).unwrap();
    store
        .grant(
            ID,
            (1, 0, 0),
            &requested,
            std::slice::from_ref(&identity),
            false,
        )
        .unwrap();
    fs::remove_file(&tool).unwrap();
    assert_eq!(
        check_program(&identity).map_err(|suspended| suspended.reason),
        Err(SuspendReason::Missing)
    );
    store.check_programs(ID).unwrap();
    assert!(matches!(
        store.decide(ID, (1, 0, 0), &requested, no_path()).unwrap(),
        Decision::Escalation { .. }
    ));
}

#[test]
fn the_store_round_trips_under_a_lock_and_forgets() {
    let dir = temp_dir("store");
    let path = dir.join("state").join("plugin-grants.toml");
    program(&dir, "tool", "tool");
    let identity = resolve_program("tool", &path_var(&[&dir])).unwrap();
    let mut record = installed((1, 2, 3), &["buffer.read", "net:*", "process.spawn:tool"]);
    record.programs.insert("tool".into(), identity);
    record.wildcards_accepted = true;
    record.declined = Some(grants(&["buffer.write"]));
    record
        .suspended
        .insert("old".into(), SuspendReason::Missing);

    GrantStore::new(&path).set(ID, record.clone()).unwrap();
    let reopened = GrantStore::new(&path);
    assert_eq!(reopened.get(ID).unwrap(), Some(record));
    assert!(reopened.forget(ID).unwrap());
    assert_eq!(reopened.get(ID).unwrap(), None);
    assert!(!reopened.forget(ID).unwrap());

    // Two writers, each its own store, don't lose each other's records.
    let writers: Vec<_> = (0..2)
        .map(|writer| {
            let store = GrantStore::new(&path);
            std::thread::spawn(move || {
                for n in 0..8 {
                    let id = format!("acme.w{writer}-{n}");
                    store.set(&id, installed((1, 0, n), &["status"])).unwrap();
                }
            })
        })
        .collect();
    for writer in writers {
        writer.join().unwrap();
    }
    let store = GrantStore::new(&path);
    for writer in 0..2 {
        for n in 0..8 {
            let id = format!("acme.w{writer}-{n}");
            assert_eq!(
                store.get(&id).unwrap(),
                Some(installed((1, 0, n), &["status"]))
            );
        }
    }
    let leftovers: Vec<_> = fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .filter(|name| name.to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

// A FIFO is a Unix file type, made with its mode by `mkfifo`. Windows' named pipes live under
// `\\.\pipe\`, which no `PATH` reaches; its twin puts a folder in the program's place.
#[cfg(unix)]
#[test]
fn a_fifo_in_place_of_the_program_is_passed_over_without_blocking() {
    let dir = temp_dir("fifo");
    let (first, second) = (dir.join("first"), dir.join("second"));
    fs::create_dir_all(&first).unwrap();
    fs::create_dir_all(&second).unwrap();
    program(&second, "tool", "tool");
    let tool = program(&first, "tool", "tool");
    let path = path_var(&[&first, &second]);
    let identity = resolve_program("tool", &path).unwrap();
    assert_eq!(identity.path, tool);

    fs::remove_file(&tool).unwrap();
    let made = std::process::Command::new("mkfifo")
        .args(["-m", "755"])
        .arg(&tool)
        .status()
        .unwrap();
    assert!(made.success());

    // Opening a FIFO for reading waits for a writer unless it doesn't block.
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let checked = check_program(&identity).map_err(|suspended| suspended.reason);
        let resolved = resolve_program("tool", &path).map(|identity| identity.path);
        sender.send((checked, resolved)).unwrap();
    });
    let (checked, resolved) = receiver
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("a FIFO blocked the check");
    assert_eq!(checked, Err(SuspendReason::Changed));
    assert_eq!(resolved.unwrap(), second.join("tool"));
}

#[cfg(windows)]
#[test]
fn a_folder_in_place_of_the_program_is_passed_over() {
    let dir = temp_dir("folder");
    let (first, second) = (dir.join("first"), dir.join("second"));
    fs::create_dir_all(&first).unwrap();
    fs::create_dir_all(&second).unwrap();
    program(&second, "tool", "tool");
    let tool = program(&first, "tool", "tool");
    let path = path_var(&[&first, &second]);
    let identity = resolve_program("tool", &path).unwrap();
    assert_eq!(identity.path, tool);

    // `tool.cmd` is a folder now: its name still ends in an extension of `PATHEXT`'s.
    fs::remove_file(&tool).unwrap();
    fs::create_dir(&tool).unwrap();
    assert_eq!(
        check_program(&identity).map_err(|suspended| suspended.reason),
        Err(SuspendReason::Changed)
    );
    let resolved = resolve_program("tool", &path).unwrap();
    assert_eq!(resolved.path, second.join("tool.cmd"));
}

#[test]
fn get_waits_for_the_lock_a_change_holds() {
    use std::sync::mpsc;
    use std::time::Duration;
    let dir = temp_dir("get-under-lock");
    let path = dir.join("grants.toml");
    let store = GrantStore::new(&path);
    store.set(ID, installed((1, 0, 0), &[])).unwrap();
    let lock = fs::OpenOptions::new()
        .write(true)
        .open(dir.join("grants.toml.lock"))
        .unwrap();
    lock.lock().unwrap();
    let (sender, receiver) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        sender.send(store.get(ID)).unwrap();
    });
    assert!(
        receiver.recv_timeout(Duration::from_millis(300)).is_err(),
        "get returned while the lock was held"
    );
    lock.unlock().unwrap();
    let got = receiver.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(got.unwrap(), Some(installed((1, 0, 0), &[])));
    reader.join().unwrap();
}
