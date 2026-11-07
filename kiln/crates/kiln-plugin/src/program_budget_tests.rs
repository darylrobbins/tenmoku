// SPDX-License-Identifier: Apache-2.0

//! A call's program runs share one budget: the second of two, past what the first left, is
//! killed; and a run's time moves the call's deadline back rather than being charged to it.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use kiln_command::capability::{Capability, Grants};

use crate::bindings::HostState;
use crate::bindings::kiln::extension::process::Host as _;
use crate::grants::{GrantRecord, resolve_program};
use crate::limits::Limits;
use crate::process::{SpawnError, State};

fn naps(name: &str) -> (PathBuf, GrantRecord) {
    let dir =
        std::env::temp_dir().join(format!("kiln-plugin-budget-{}-{name}", std::process::id()));
    fs::remove_dir_all(&dir).ok();
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("nap");
    fs::write(&path, "#!/bin/sh\nsleep \"$1\"\n").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    let identity = resolve_program("nap", dir.as_os_str()).unwrap();
    let granted: Grants = [Capability::parse("process.spawn:nap").unwrap()]
        .into_iter()
        .collect();
    let record = GrantRecord {
        version: Some((1, 0, 0)),
        granted,
        programs: BTreeMap::from([("nap".to_owned(), identity)]),
        ..GrantRecord::default()
    };
    (dir, record)
}

fn nap(seconds: &str) -> Vec<String> {
    vec![seconds.to_owned()]
}

#[test]
fn a_calls_second_run_is_killed_at_what_the_first_left() {
    let (dir, record) = naps("share");
    let mut host = HostState::new(Box::new(|_, _| {}));
    host.process = State::new(record, &Limits::default());
    // A sub-second share for the whole call.
    host.program_runs.arm(Duration::from_millis(900));

    // The first run is charged what it measured, as the import does.
    host.spawn("nap".to_owned(), nap("0"), None).unwrap();
    let first = host.program_runs.spent();
    assert!(
        first > Duration::ZERO && first < Duration::from_millis(900),
        "{first:?}"
    );

    // The second would take 5 s: it is killed by what the first left, not left to run.
    let started = Instant::now();
    let second = host
        .process
        .spawn_within("nap", &nap("5"), None, host.program_runs.remaining());
    assert!(
        matches!(second, Err(SpawnError::TimedOut { .. })),
        "{second:?}"
    );
    assert!(started.elapsed() < Duration::from_secs(3));
    fs::remove_dir_all(dir).ok();
}

#[test]
fn checking_the_program_comes_out_of_the_share() {
    let (dir, record) = naps("checking");
    let mut state = State::new(record, &Limits::default());
    let share = Some(Duration::from_millis(500));

    // The share was read 600 ms ago: the check used it up, so nothing launches.
    let entered = Instant::now() - Duration::from_millis(600);
    let late = state.spawn_from("nap", &nap("5"), None, share, entered);
    assert!(matches!(late, Err(SpawnError::TimedOut { .. })), "{late:?}");

    // Read 300 ms ago: the run gets the 200 ms left, not 500 ms or 5 s.
    let entered = Instant::now() - Duration::from_millis(300);
    let started = Instant::now();
    let run = state.spawn_from("nap", &nap("5"), None, share, entered);
    assert!(matches!(run, Err(SpawnError::TimedOut { .. })), "{run:?}");
    assert!(started.elapsed() < Duration::from_secs(3));
    fs::remove_dir_all(dir).ok();
}

#[test]
fn a_run_moves_the_calls_deadline_back_and_a_spent_budget_runs_nothing() {
    let (dir, record) = naps("deadline");
    let mut host = HostState::new(Box::new(|_, _| {}));
    host.process = State::new(record, &Limits::default());
    host.program_runs.arm(Duration::from_secs(5));
    let deadline = Instant::now() + Duration::from_millis(100);
    host.deadline = Some(deadline);

    host.spawn("nap".to_owned(), nap("0.2"), None).unwrap();
    let moved = host.deadline.unwrap().duration_since(deadline);
    assert!(moved >= Duration::from_millis(200), "{moved:?}");
    assert!(host.program_runs.spent() >= Duration::from_millis(200));
    assert_eq!(
        host.program_runs.extend(Some(deadline)).unwrap() - deadline,
        host.program_runs.spent()
    );

    // Once the share is used up, nothing starts.
    host.program_runs.charge(Duration::from_secs(60));
    let started = Instant::now();
    host.spawn("nap".to_owned(), nap("5"), None).unwrap_err();
    assert!(started.elapsed() < Duration::from_millis(500));
    host.program_runs.disarm();
    fs::remove_dir_all(dir).ok();
}

#[test]
fn a_spent_budget_refuses_before_the_program_is_checked() {
    let (dir, record) = naps("spent");
    let mut state = State::new(record, &Limits::default());
    // The program changes after its grant: checking it would suspend the grant.
    fs::write(dir.join("nap"), "#!/bin/sh\n# changed\nsleep \"$1\"\n").unwrap();
    let refused = state.spawn_within("nap", &nap("0"), None, Some(Duration::ZERO));
    assert!(
        matches!(refused, Err(SpawnError::TimedOut { .. })),
        "{refused:?}"
    );
    assert!(
        state.record().suspended.is_empty(),
        "the program was checked"
    );
    // With share left, the same program is checked, and the change suspends it.
    let checked = state.spawn_within("nap", &nap("0"), None, Some(Duration::from_secs(1)));
    assert!(
        matches!(checked, Err(SpawnError::Suspended { .. })),
        "{checked:?}"
    );
    fs::remove_dir_all(dir).ok();
}

#[test]
fn the_time_a_call_excludes_from_its_deadline_is_capped_at_its_budget() {
    let budget = crate::process::ProgramBudget::default();
    budget.arm(Duration::from_millis(500));
    // Runs and refusals alike are charged, but never past the share.
    assert_eq!(
        budget.charge(Duration::from_millis(300)),
        Duration::from_millis(300)
    );
    assert_eq!(
        budget.charge(Duration::from_secs(60)),
        Duration::from_millis(200)
    );
    assert_eq!(budget.charge(Duration::from_secs(60)), Duration::ZERO);
    assert_eq!(budget.spent(), Duration::from_millis(500));
    let deadline = Instant::now();
    assert_eq!(
        budget.extend(Some(deadline)).unwrap() - deadline,
        Duration::from_millis(500)
    );

    // Through the host's import, the deadline moves back by no more than the share.
    let (dir, record) = naps("cap");
    let mut host = HostState::new(Box::new(|_, _| {}));
    host.process = State::new(record, &Limits::default());
    host.program_runs.arm(Duration::from_millis(300));
    host.deadline = Some(deadline);
    for _ in 0..3 {
        host.spawn("nap".to_owned(), nap("5"), None).unwrap_err();
    }
    let moved = host.deadline.unwrap() - deadline;
    assert!(moved <= Duration::from_millis(300), "{moved:?}");
    fs::remove_dir_all(dir).ok();
}

#[test]
fn a_share_that_ends_mid_check_refuses_without_suspending_the_grant() {
    let (dir, mut record) = naps("big");
    // A program of 32 chunks, each of which a check reads slowly.
    const CHUNKS: usize = 32;
    let path = dir.join("big");
    fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
    fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .set_len(u64::try_from(CHUNKS * (64 << 10)).unwrap())
        .unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    let identity = resolve_program("big", dir.as_os_str()).unwrap();
    record.granted = [Capability::parse("process.spawn:big").unwrap()]
        .into_iter()
        .collect();
    record.programs.insert("big".to_owned(), identity.clone());
    let mut state = State::new(record, &Limits::default());

    let (delay, share) = (Duration::from_millis(40), Duration::from_millis(100));
    crate::grants::hash_probe::slow_reads(delay);
    let started = Instant::now();
    let refused = state.spawn_within("big", &[], None, Some(share));
    let took = started.elapsed();
    assert!(
        matches!(refused, Err(SpawnError::TimedOut { .. })),
        "{refused:?}"
    );
    // The hash was interrupted, not completed: fewer chunks were read than the file has, and the
    // check ended within the share plus one chunk's read (and some slack for a loaded machine).
    let read = crate::grants::hash_probe::chunks();
    assert!(read > 0 && read < CHUNKS, "{read} of {CHUNKS} chunks");
    assert!(
        took <= share + delay + Duration::from_millis(600),
        "{took:?}"
    );
    // A slow read is not a changed program: the grant stands, and it still checks out.
    assert!(state.record().suspended.is_empty());
    assert_eq!(crate::grants::check_program(&identity), Ok(()));
    fs::remove_dir_all(dir).ok();
}

#[test]
fn a_share_that_expires_before_launch_refuses_and_a_held_store_lock_does_not_block_past_it() {
    let (dir, record) = naps("launch");
    let mut state = State::new(record, &Limits::default());
    // Read 1 ms of 1 ms ago: whatever the check and setup took, nothing is left to launch with.
    let entered = Instant::now() - Duration::from_millis(1);
    let started = Instant::now();
    let late = state.spawn_from(
        "nap",
        &nap("5"),
        None,
        Some(Duration::from_millis(1)),
        entered,
    );
    assert!(matches!(late, Err(SpawnError::TimedOut { .. })), "{late:?}");
    assert!(started.elapsed() < Duration::from_secs(1));

    // A suspension whose store is locked elsewhere waits for the lock only until the share ends.
    let store = crate::grants::GrantStore::new(dir.join("grants.toml"));
    let lock = fs::File::create(dir.join("grants.toml.lock")).unwrap();
    lock.lock().unwrap();
    let by = Instant::now() + Duration::from_millis(50);
    let started = Instant::now();
    let held = store.check_programs_by("acme.tool", Some(by));
    assert!(
        matches!(&held, Err(crate::grants::GrantStoreError::Io { source, .. })
            if source.kind() == std::io::ErrorKind::TimedOut),
        "{held:?}"
    );
    assert!(started.elapsed() < Duration::from_secs(2));
    fs::remove_dir_all(dir).ok();
}
