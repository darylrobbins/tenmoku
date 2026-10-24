// SPDX-License-Identifier: LicenseRef-Proprietary

//! Spawning programs for a plugin: only by a grant, only the file recorded for it, with
//! arguments as given, a minimal environment, and output and time capped. The programs are real
//! ones from the system (`printf`, `env`, `sleep`), copied into temporary directories where
//! their identity can be made to change.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use kiln_command::capability::{Capability, Grants};
use kiln_plugin::grants::{
    Decision, GrantRecord, GrantStore, GrantStoreError, ProgramIdentity, SuspendReason, evaluate,
    resolve_program,
};
use kiln_plugin::limits::Limits;
use kiln_plugin::process::{SYSTEM_PATH, SpawnError, State, minimal_environment};

const ID: &str = "acme.tool";

fn grants(capabilities: &[&str]) -> Grants {
    capabilities
        .iter()
        .map(|capability| Capability::parse(capability).unwrap())
        .collect()
}

/// A directory of its own under cargo's temporary directory for tests.
fn temp_dir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("kiln-plugin-process-{}-{name}", std::process::id()));
    fs::remove_dir_all(&dir).ok();
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// The system's `name`, copied into `dir` under its own name (a multi-call binary picks what it
/// runs as by that), and bound as a grant would bind it.
fn copied(dir: &Path, name: &str) -> ProgramIdentity {
    let system = std::env::split_paths(SYSTEM_PATH)
        .map(|system| system.join(name))
        .find(|path| path.is_file())
        .unwrap_or_else(|| panic!("no {name} in {SYSTEM_PATH}"));
    let copy = dir.join(name);
    fs::copy(system, &copy).unwrap();
    resign(&copy);
    resolve_program(name, dir.as_os_str()).unwrap()
}

/// macOS kills (SIGKILL at exec) a copied system binary: its signature is the platform's, which
/// binds it to `/usr/bin` and `/bin`. An ad-hoc signature makes the copy an ordinary program.
#[cfg(target_os = "macos")]
fn resign(path: &Path) {
    let status = std::process::Command::new("codesign")
        .args(["--force", "--sign", "-"])
        .arg(path)
        .output()
        .unwrap();
    assert!(status.status.success(), "{status:?}");
}

#[cfg(not(target_os = "macos"))]
fn resign(_: &Path) {}

/// An executable script `name` in `dir`, bound as a grant would bind it.
fn script(dir: &Path, name: &str, body: &str) -> ProgramIdentity {
    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    resolve_program(name, dir.as_os_str()).unwrap()
}

/// A record granting `granted`, with `programs` bound.
fn record(granted: &[&str], programs: &[&ProgramIdentity]) -> GrantRecord {
    GrantRecord {
        version: Some((1, 0, 0)),
        granted: grants(granted),
        programs: programs
            .iter()
            .map(|identity| (identity.name.clone(), (*identity).clone()))
            .collect::<BTreeMap<_, _>>(),
        ..GrantRecord::default()
    }
}

fn args(args: &[&str]) -> Vec<String> {
    args.iter().map(|&arg| arg.to_owned()).collect()
}

fn stdout(state: &mut State, program: &str, arguments: &[&str]) -> String {
    let output = state.spawn(program, &args(arguments), None).unwrap();
    assert_eq!(output.status, Some(0), "{output:?}");
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn a_granted_program_runs_by_its_recorded_path() {
    let dir = temp_dir("recorded");
    let printf = copied(&dir, "printf");
    assert_eq!(printf.path, dir.join("printf"));
    let mut state = State::new(
        record(&["process.spawn:printf"], &[&printf]),
        &Limits::default(),
    );
    assert_eq!(stdout(&mut state, "printf", &["%s-%s", "a", "b"]), "a-b");

    // A `printf` of its own that no PATH holds: a name looked up again would find the system's.
    let shadow = temp_dir("recorded-shadow");
    let recorded = script(&shadow, "printf", r#"echo "recorded: $0""#);
    let mut state = State::new(
        record(&["process.spawn:printf"], &[&recorded]),
        &Limits::default(),
    );
    assert_eq!(
        stdout(&mut state, "printf", &["%s", "x"]),
        format!("recorded: {}\n", shadow.join("printf").display())
    );

    // `process.spawn:*` runs a name bound to a program the same way.
    let mut state = State::new(
        record(&["process.spawn:*"], &[&recorded]),
        &Limits::default(),
    );
    assert!(stdout(&mut state, "printf", &[]).starts_with("recorded: "));
}

#[test]
fn an_ungranted_program_is_refused() {
    let dir = temp_dir("ungranted");
    let printf = copied(&dir, "printf");
    let marker = dir.join("ran");
    let tool = script(&dir, "tool", &format!("touch '{}'", marker.display()));
    let refused = |state: &mut State, program: &str| match state.spawn(program, &[], None) {
        Err(SpawnError::NotGranted { program: named }) => assert_eq!(named, program),
        other => panic!("{program} was not refused: {other:?}"),
    };

    // Bound but not granted: the grant decides, not the binding.
    let mut state = State::new(
        record(&["process.spawn:printf"], &[&printf, &tool]),
        &Limits::default(),
    );
    refused(&mut state, "tool");
    refused(&mut state, "sleep");
    refused(&mut state, "");
    refused(&mut state, &tool.path.display().to_string());
    // `process.spawn:*` with nothing bound to the name: it is never looked up.
    let mut state = State::new(record(&["process.spawn:*"], &[]), &Limits::default());
    refused(&mut state, "tool");
    refused(&mut state, "printf");
    // Nothing granted at all.
    refused(&mut State::default(), "printf");
    assert!(!marker.exists(), "an ungranted program ran");
}

#[test]
fn a_changed_binary_suspends_the_grant_before_spawn() {
    changed_binary_suspends("named", &["process.spawn:printf"]);
    // `process.spawn:*` still covers the name, but a suspended program asks again all the same.
    changed_binary_suspends("wildcard", &["process.spawn:*", "process.spawn:printf"]);
}

/// Grants `granted` through a store, binding `printf`, then overwrites the program: the next
/// spawn is refused, and the store's next decision on the same request asks about `printf`.
fn changed_binary_suspends(case: &str, granted: &[&str]) {
    let dir = temp_dir(&format!("changed-{case}"));
    let printf = copied(&dir, "printf");
    let marker = dir.join("ran");
    let requested = grants(granted);
    let store = GrantStore::new(dir.join("state/plugin-grants.toml"));
    store
        .grant(
            ID,
            (1, 0, 0),
            &requested,
            std::slice::from_ref(&printf),
            true,
        )
        .unwrap();
    let mut state = State::new(store.get(ID).unwrap().unwrap(), &Limits::default())
        .recorded_in(store.clone(), ID);
    assert_eq!(stdout(&mut state, "printf", &["ok"]), "ok", "{case}");

    // Overwritten in place: same path, same inode, different content.
    fs::write(
        &printf.path,
        format!("#!/bin/sh\ntouch '{}'\n", marker.display()),
    )
    .unwrap();
    for _ in 0..2 {
        match state.spawn("printf", &args(&["again"]), None) {
            Err(SpawnError::Suspended { program, suspended }) => {
                assert_eq!(program, "printf");
                assert_eq!(suspended.reason, SuspendReason::Changed);
            }
            other => panic!("{case}: not suspended: {other:?}"),
        }
    }
    assert!(!marker.exists(), "{case}: the changed program ran");

    // The grant is withdrawn, here and in the store: loading again asks about the program.
    let escalation = Decision::Escalation {
        added: grants(&["process.spawn:printf"]),
        keep_running: Some((1, 0, 0)),
    };
    assert_eq!(state.record().suspended["printf"], SuspendReason::Changed);
    assert_eq!(
        evaluate(Some(state.record()), &requested),
        escalation,
        "{case}"
    );
    let stored = store.get(ID).unwrap().unwrap();
    assert_eq!(stored.suspended["printf"], SuspendReason::Changed);
    assert_eq!(evaluate(Some(&stored), &requested), escalation, "{case}");
    let decided = store.decide(ID, (1, 0, 0), &requested, dir.as_os_str());
    assert_eq!(decided.unwrap(), escalation, "{case}");
}

#[test]
fn a_removed_binary_is_suspended_as_missing() {
    let dir = temp_dir("removed");
    let printf = copied(&dir, "printf");
    let mut state = State::new(
        record(&["process.spawn:printf"], &[&printf]),
        &Limits::default(),
    );
    fs::remove_file(&printf.path).unwrap();
    match state.spawn("printf", &[], None) {
        Err(SpawnError::Suspended { suspended, .. }) => {
            assert_eq!(suspended.reason, SuspendReason::Missing);
        }
        other => panic!("not suspended: {other:?}"),
    }
}

#[test]
fn arguments_are_not_shell_parsed() {
    let dir = temp_dir("arguments");
    let printf = copied(&dir, "printf");
    let mut state = State::new(
        record(&["process.spawn:printf"], &[&printf]),
        &Limits::default(),
    );
    let literal = [
        "; echo pwned",
        "$(touch pwned)",
        "`id`",
        "a b",
        "*",
        "'\"",
        "",
    ];
    let mut arguments = vec!["%s|"];
    arguments.extend(literal);
    assert_eq!(
        stdout(&mut state, "printf", &arguments),
        literal.map(|arg| format!("{arg}|")).concat()
    );
    assert!(!Path::new("pwned").exists());

    // Stdin is the plugin's bytes, as they are.
    let cat = script(&dir, "cat", "exec /bin/cat");
    let mut state = State::new(record(&["process.spawn:cat"], &[&cat]), &Limits::default());
    let output = state
        .spawn("cat", &[], Some(b"; echo pwned\n\0bytes"))
        .unwrap();
    assert_eq!(output.stdout, b"; echo pwned\n\0bytes");
}

#[test]
fn the_environment_is_minimal() {
    let dir = temp_dir("environment");
    let env = copied(&dir, "env");
    let mut state = State::new(record(&["process.spawn:env"], &[&env]), &Limits::default());
    let printed = stdout(&mut state, "env", &[]);
    let variables: BTreeMap<&str, &str> = printed
        .lines()
        .map(|line| line.split_once('=').unwrap())
        .collect();
    for name in variables.keys() {
        assert!(
            matches!(*name, "PATH" | "HOME" | "LANG" | "TMPDIR" | "TERM")
                || name.starts_with("LC_"),
            "{name} reached the program:\n{printed}"
        );
    }
    assert_eq!(variables["PATH"], SYSTEM_PATH);
    // What does pass is the app's own value.
    for name in ["HOME", "LANG", "TERM"] {
        assert_eq!(
            variables.get(name).copied(),
            std::env::var(name).ok().as_deref(),
            "{name}"
        );
    }
    // Cargo set these for this test: none of them reaches the program.
    assert!(std::env::var_os("CARGO_MANIFEST_DIR").is_some());
    assert!(!variables.contains_key("CARGO_MANIFEST_DIR"));
}

#[test]
fn the_minimal_environment_keeps_only_its_variables() {
    let app = [
        ("PATH", "/home/me/bin:/usr/bin"),
        ("HOME", "/home/me"),
        ("LANG", "en_CA.UTF-8"),
        ("LC_ALL", "C"),
        ("LC_CTYPE", "C.UTF-8"),
        ("TMPDIR", "/tmp/me"),
        ("TERM", "xterm"),
        ("SSH_AUTH_SOCK", "/tmp/agent"),
        ("LD_PRELOAD", "/tmp/evil.so"),
        ("DYLD_INSERT_LIBRARIES", "/tmp/evil.dylib"),
        ("CLASS", "LC_X"),
    ];
    let minimal = minimal_environment(app.map(|(name, value)| (name.into(), value.into())));
    let minimal: Vec<_> = minimal
        .iter()
        .map(|(name, value)| (name.to_str().unwrap(), value.to_str().unwrap()))
        .collect();
    assert_eq!(
        minimal,
        [
            ("PATH", SYSTEM_PATH),
            ("HOME", "/home/me"),
            ("LANG", "en_CA.UTF-8"),
            ("LC_ALL", "C"),
            ("LC_CTYPE", "C.UTF-8"),
            ("TMPDIR", "/tmp/me"),
            ("TERM", "xterm"),
        ]
    );
}

#[test]
fn output_over_the_cap_and_runs_over_the_deadline_are_killed() {
    let dir = temp_dir("limits");
    let sleep = copied(&dir, "sleep");
    let printf = copied(&dir, "printf");
    let limits = Limits {
        max_read_bytes: 16,
        ..Limits::default()
    };
    let record = record(
        &["process.spawn:sleep", "process.spawn:printf"],
        &[&sleep, &printf],
    );
    let mut state = State::new(record.clone(), &limits).with_deadline(Duration::from_millis(300));

    let started = Instant::now();
    match state.spawn("sleep", &args(&["5"]), None) {
        Err(SpawnError::TimedOut { program, deadline }) => {
            assert_eq!(program, "sleep");
            assert_eq!(deadline, Duration::from_millis(300));
        }
        other => panic!("not killed: {other:?}"),
    }
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );

    // Output is cut at the cap. A megabyte fills the pipe and blocks: the program is killed at
    // the cap, not left to the deadline, which would fail the call.
    let mut state = State::new(record, &limits).with_deadline(Duration::from_millis(1500));
    // The format is reused for each argument: a megabyte from a few hundred short ones, well
    // inside any system's argument limit.
    let mut arguments = vec!["%04096d"];
    arguments.extend(["0"; 256]);
    let output = state.spawn("printf", &args(&arguments), None).unwrap();
    assert_eq!(output.stdout, b"0000000000000000");
    assert_eq!(output.status, None);
}

#[test]
fn nothing_outlives_a_call_that_ends() {
    let dir = temp_dir("outlives");
    let sleep = copied(&dir, "sleep");
    let printf = copied(&dir, "printf");
    // Leaves a program of its own holding stdout and stderr open after it exits.
    let holder = script(&dir, "holder", "sleep 3 &\necho started");
    let record = record(
        &[
            "process.spawn:sleep",
            "process.spawn:printf",
            "process.spawn:holder",
        ],
        &[&sleep, &printf, &holder],
    );
    let mut state =
        State::new(record.clone(), &Limits::default()).with_deadline(Duration::from_millis(300));
    let before = threads();
    let unread = vec![b'x'; 1 << 20];

    let started = Instant::now();
    let held = state.spawn("holder", &[], None);
    assert!(matches!(held, Err(SpawnError::TimedOut { .. })), "{held:?}");
    let ignored = state.spawn("sleep", &args(&["5"]), Some(&unread));
    assert!(
        matches!(ignored, Err(SpawnError::TimedOut { .. })),
        "{ignored:?}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );

    // A program that ends without reading its stdin ends the call. This is the success path, so
    // it gets a deadline a loaded machine cannot miss.
    let mut state = State::new(record, &Limits::default()).with_deadline(Duration::from_secs(30));
    let output = state
        .spawn("printf", &args(&["ok"]), Some(&unread))
        .unwrap();
    assert_eq!(output.stdout, b"ok");
    assert_eq!(threads(), before, "a call left a thread behind");
}

/// How many threads this process has, where the system says.
fn threads() -> Option<usize> {
    fs::read_dir("/proc/self/task").ok().map(Iterator::count)
}

#[test]
fn a_suspension_holds_until_the_program_is_granted_again() {
    let dir = temp_dir("suspension");
    let printf = copied(&dir, "printf");
    let store = GrantStore::new(dir.join("state/plugin-grants.toml"));
    let path_var = dir.as_os_str();
    let both = grants(&["process.spawn:*", "process.spawn:printf"]);
    let asks_about_printf = |version| Decision::Escalation {
        added: grants(&["process.spawn:printf"]),
        keep_running: Some(version),
    };
    store
        .grant(ID, (1, 0, 0), &both, std::slice::from_ref(&printf), true)
        .unwrap();
    fs::write(&printf.path, "#!/bin/sh\n").unwrap();
    assert_eq!(store.check_programs(ID).unwrap().len(), 1);

    // Dropping the name while `process.spawn:*` still covers it keeps the suspension, so naming
    // it again asks.
    let wildcard = grants(&["process.spawn:*"]);
    let decided = store.decide(ID, (1, 0, 0), &wildcard, path_var).unwrap();
    assert_eq!(decided, Decision::Load(wildcard.clone()));
    assert_eq!(
        store.get(ID).unwrap().unwrap().suspended["printf"],
        SuspendReason::Changed
    );
    let mut state = State::new(store.get(ID).unwrap().unwrap(), &Limits::default());
    let spawned = state.spawn("printf", &[], None);
    assert!(
        matches!(spawned, Err(SpawnError::Suspended { .. })),
        "{spawned:?}"
    );
    let decided = store.decide(ID, (1, 0, 0), &both, path_var).unwrap();
    assert_eq!(decided, asks_about_printf((1, 0, 0)));

    // Granting something unrelated keeps it too.
    let unrelated = grants(&["process.spawn:*", "buffer.read"]);
    store.grant(ID, (2, 0, 0), &unrelated, &[], true).unwrap();
    assert!(
        store
            .get(ID)
            .unwrap()
            .unwrap()
            .suspended
            .contains_key("printf")
    );
    let with_printf = grants(&["process.spawn:*", "process.spawn:printf", "buffer.read"]);
    let decided = store.decide(ID, (2, 0, 0), &with_printf, path_var).unwrap();
    assert_eq!(decided, asks_about_printf((2, 0, 0)));
    // Naming it at a prompt needs the program bound anew; nothing else lifts it.
    let refused = store.grant(ID, (2, 0, 0), &with_printf, &[], true);
    assert!(
        matches!(&refused, Err(GrantStoreError::Suspended(program)) if program == "printf"),
        "{refused:?}"
    );
    assert!(
        store
            .get(ID)
            .unwrap()
            .unwrap()
            .suspended
            .contains_key("printf")
    );

    // Binding it anew at a prompt lifts the suspension.
    let rebound = resolve_program("printf", path_var).unwrap();
    let record = store
        .grant(ID, (2, 0, 0), &with_printf, &[rebound], true)
        .unwrap();
    assert!(record.suspended.is_empty());
    let decided = store.decide(ID, (2, 0, 0), &with_printf, path_var).unwrap();
    assert_eq!(decided, Decision::Load(with_printf));

    // With nothing left to cover it, a suspension has nothing to hold.
    fs::write(&printf.path, "#!/bin/sh\nexit 1\n").unwrap();
    assert_eq!(store.check_programs(ID).unwrap().len(), 1);
    let unrelated = grants(&["buffer.read"]);
    let decided = store.decide(ID, (2, 0, 0), &unrelated, path_var).unwrap();
    assert_eq!(decided, Decision::Load(unrelated));
    assert!(store.get(ID).unwrap().unwrap().suspended.is_empty());
}
