// SPDX-License-Identifier: Apache-2.0

//! Everything a program run does before its launch stops at the end of the call's program share:
//! the setup, the checks of the program and the taking of the store's lock. A suspension is held
//! in memory at once and reaches the store later, however slow the store is, and is never lost.
//!
//! The seams the code under test calls live in [`super::seams`].

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::RwLockReadGuard;
use std::time::{Duration, Instant};

use kiln_command::capability::{Capability, Grants};

use crate::grants::{
    CheckTimedOut, GrantRecord, GrantStore, GrantStoreError, SuspendReason, check_program_by,
    resolve_program,
};
use crate::limits::Limits;
use crate::process::{SpawnError, State};
use crate::tests::{hash_probe, seams, support};

const ID: &str = "acme.tool";

/// How much later than the share's end a call may return: a loaded machine's scheduling, not work.
const MARGIN: Duration = Duration::from_secs(2);

/// Long enough never to be waited out by a test that passes, and short of hanging one that fails.
const HOLD: Duration = Duration::from_secs(30);

/// How long a test waits for what a writer's thread does.
const PATIENCE: Duration = Duration::from_secs(60);

struct Fixture {
    dir: PathBuf,
    /// The program, a script: `mark` on Unix, `mark.cmd` on Windows.
    program: PathBuf,
    /// Created by the program, if it ever runs, from its only argument.
    marker: PathBuf,
    store: GrantStore,
    _programs: Programs,
}

/// How a fixture holds [`crate::tests::programs`]: until it's dropped, never read.
#[allow(dead_code)]
enum Programs {
    Beside(RwLockReadGuard<'static, ()>),
    #[cfg(windows)]
    Alone(std::sync::RwLockWriteGuard<'static, ()>),
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.dir).ok();
    }
}

impl Fixture {
    fn new(name: &str) -> Self {
        Self::holding(name, Programs::Beside(crate::tests::programs()))
    }

    /// A fixture for a test that changes what makes a file a program.
    #[cfg(windows)]
    fn alone(name: &str) -> Self {
        Self::holding(name, Programs::Alone(crate::tests::programs_alone()))
    }

    fn holding(name: &str, programs: Programs) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "kiln-plugin-prelaunch-{}-{name}",
            std::process::id()
        ));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).unwrap();
        Self {
            program: support::script(&dir, "mark", r#": > "$1""#, r#"type nul > "%~1""#),
            marker: dir.join("ran"),
            store: GrantStore::new(dir.join("state/plugin-grants.toml")),
            dir,
            _programs: programs,
        }
    }

    fn program(&self) -> &Path {
        &self.program
    }

    /// Grants the program as it is now, through the store.
    fn grant(&self) -> GrantRecord {
        let identity = resolve_program("mark", self.dir.as_os_str()).unwrap();
        let requested: Grants = [Capability::parse("process.spawn:mark").unwrap()]
            .into_iter()
            .collect();
        (self.store)
            .grant(ID, (1, 0, 0), &requested, &[identity], false)
            .unwrap()
    }

    /// A state recording suspensions in the store, for a grant of a program that has since changed.
    fn state_with_a_changed_program(&self) -> State {
        let record = self.grant();
        support::script(
            &self.dir,
            "mark",
            "# changed\n: > \"$1\"",
            "rem changed\ntype nul > \"%~1\"",
        );
        State::new(record, &Limits::default()).recorded_in(self.store.clone(), ID)
    }

    fn args(&self) -> Vec<String> {
        vec![self.marker.to_str().unwrap().to_owned()]
    }

    fn store_suspends(&self) -> bool {
        let record = self.store.get(ID).unwrap().unwrap();
        record.suspended.get("mark") == Some(&SuspendReason::Changed)
    }

    fn seam(&self, change: impl FnOnce(&mut seams::Store)) -> seams::Store {
        seams::store(self.store.path(), change)
    }
}

/// Waits until `done`, which a thread elsewhere brings about.
fn eventually(mut done: impl FnMut() -> bool) -> bool {
    let until = Instant::now() + PATIENCE;
    while !done() {
        if Instant::now() >= until {
            return false;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    true
}

/// The regression test. With nothing but the setup in the way, the program would run: the share
/// is far longer than it takes, and only the setup's delay takes the share to its end.
#[test]
fn delayed_setup_past_the_share_end_is_refused() {
    let fixture = Fixture::new("setup");
    let mut state = State::new(fixture.grant(), &Limits::default());
    let share = Duration::from_secs(1);
    let entered = Instant::now();
    seams::delay_setup(entered + share + Duration::from_millis(1));

    let ran = state.spawn_from("mark", &fixture.args(), None, Some(share), entered);

    assert!(matches!(ran, Err(SpawnError::TimedOut { .. })));
    // A child started past the end would be killed at once, likely before it wrote the marker.
    assert_eq!(seams::launches(), 0);
    assert!(!fixture.marker.exists());
    assert!(state.record().suspended.is_empty());
}

/// Setup that leaves some of the share still launches.
#[test]
fn setup_within_the_share_launches() {
    let fixture = Fixture::new("in-time");
    let mut state = State::new(fixture.grant(), &Limits::default());
    let entered = Instant::now();
    let share = Duration::from_secs(60);
    seams::delay_setup(entered + Duration::from_millis(5));

    let ran = state.spawn_from("mark", &fixture.args(), None, Some(share), entered);

    assert_eq!(ran.unwrap().status, Some(0));
    assert!(fixture.marker.exists());
}

#[test]
fn a_slow_store_does_not_hold_the_call_and_the_suspension_arrives_later() {
    let fixture = Fixture::new("slow-store");
    let mut state = fixture.state_with_a_changed_program();
    fixture.seam(|store| store.held_until = Some(Instant::now() + HOLD));
    let share = Duration::from_millis(100);

    let started = Instant::now();
    let refused = state.spawn_within("mark", &fixture.args(), None, Some(share));
    let took = started.elapsed();

    assert!(took <= share + MARGIN, "{took:?}");
    assert!(
        matches!(refused, Err(SpawnError::Suspended { .. })),
        "{refused:?}"
    );
    assert!(!fixture.marker.exists());
    // In memory at once; in the store not yet.
    assert_eq!(
        state.record().suspended.get("mark"),
        Some(&SuspendReason::Changed)
    );
    assert!(!fixture.store_suspends());

    fixture.seam(|store| store.held_until = None);
    assert!(eventually(|| fixture.store_suspends()));
}

#[test]
fn a_suspension_that_fails_to_persist_is_retried() {
    let fixture = Fixture::new("retried");
    let mut state = fixture.state_with_a_changed_program();
    fixture.seam(|store| store.failing = 2);

    let refused = state.spawn_within(
        "mark",
        &fixture.args(),
        None,
        Some(Duration::from_millis(5)),
    );

    assert!(
        matches!(refused, Err(SpawnError::Suspended { .. })),
        "{refused:?}"
    );
    assert!(eventually(|| fixture.store_suspends()));
    assert_eq!(fixture.seam(|_| {}).tries, 3);
}

/// The writer's thread starts when the plugin loads, so a suspension found when no thread can
/// start is still written.
#[test]
fn a_writer_started_at_load_needs_no_new_thread_later() {
    let fixture = Fixture::new("started-at-load");
    let mut state = fixture.state_with_a_changed_program();
    fixture.seam(|store| store.no_threads = true);

    let refused = state.spawn_within("mark", &fixture.args(), None, Some(Duration::from_secs(5)));

    assert!(
        matches!(refused, Err(SpawnError::Suspended { .. })),
        "{refused:?}"
    );
    assert!(eventually(|| fixture.store_suspends()));
}

/// A state whose writer can't start, with a suspension found under it.
fn state_with_a_write_and_no_writer(fixture: &Fixture) -> State {
    fixture.seam(|store| store.no_threads = true);
    let mut state = fixture.state_with_a_changed_program();
    // With nothing to wait for, the call doesn't use up its share waiting.
    let started = Instant::now();
    let refused = state.spawn_within("mark", &fixture.args(), None, Some(HOLD));
    assert!(started.elapsed() <= MARGIN, "{:?}", started.elapsed());
    assert!(
        matches!(refused, Err(SpawnError::Suspended { .. })),
        "{refused:?}"
    );
    assert!(!fixture.store_suspends());
    state
}

#[test]
fn a_writer_that_cannot_start_is_started_again_at_the_next_spawn() {
    let fixture = Fixture::new("no-thread");
    let mut state = state_with_a_write_and_no_writer(&fixture);
    fixture.seam(|store| store.no_threads = false);

    // The grant is withdrawn, so this one stops at that; the suspension goes on its way.
    let refused = state.spawn_within("mark", &fixture.args(), None, Some(HOLD));

    assert!(
        matches!(refused, Err(SpawnError::Suspended { .. })),
        "{refused:?}"
    );
    assert!(eventually(|| fixture.store_suspends()));
}

/// Unloading the plugin doesn't take the suspension with it, and it needs no further spawn.
#[test]
fn a_write_with_no_writer_outlives_its_state() {
    let fixture = Fixture::new("unload");
    let state = state_with_a_write_and_no_writer(&fixture);
    fixture.seam(|store| store.no_threads = false);

    drop(state);

    assert!(eventually(|| fixture.store_suspends()));
}

/// Nor does an unload when the thread still can't start: the next state on the store gets it.
#[test]
fn a_write_left_by_an_unloaded_state_goes_with_the_next_state_on_the_store() {
    let fixture = Fixture::new("next-state");
    drop(state_with_a_write_and_no_writer(&fixture));
    assert!(!fixture.store_suspends());
    fixture.seam(|store| store.no_threads = false);

    let _next = State::new(GrantRecord::default(), &Limits::default())
        .recorded_in(fixture.store.clone(), "another.plugin");

    assert!(eventually(|| fixture.store_suspends()));
}

/// With no share, as when there's no runner, the store is waited for: what the call found is on
/// disk when it returns.
#[test]
fn with_no_share_the_store_is_written_before_the_call_returns() {
    let fixture = Fixture::new("no-share");
    let mut state = fixture.state_with_a_changed_program();
    let refused = state.spawn("mark", &fixture.args(), None);
    assert!(
        matches!(refused, Err(SpawnError::Suspended { .. })),
        "{refused:?}"
    );
    assert!(fixture.store_suspends());
}

/// With no share, the wait for the store is still bounded, by the run's own deadline.
#[test]
fn with_no_share_a_slow_store_is_waited_for_no_longer_than_the_runs_deadline() {
    let fixture = Fixture::new("no-share-slow");
    let deadline = Duration::from_millis(100);
    let mut state = fixture
        .state_with_a_changed_program()
        .with_deadline(deadline);
    fixture.seam(|store| store.held_until = Some(Instant::now() + HOLD));

    let started = Instant::now();
    let refused = state.spawn("mark", &fixture.args(), None);
    let took = started.elapsed();

    assert!(took <= deadline + MARGIN, "{took:?}");
    assert!(
        matches!(refused, Err(SpawnError::Suspended { .. })),
        "{refused:?}"
    );
    fixture.seam(|store| store.held_until = None);
    assert!(eventually(|| fixture.store_suspends()));
}

/// A suspension still on its way must not withdraw what the user has granted since.
#[test]
fn a_late_suspension_leaves_a_program_granted_anew_alone() {
    let fixture = Fixture::new("granted-anew");
    let mut state = fixture.state_with_a_changed_program();
    fixture.seam(|store| store.held_until = Some(Instant::now() + HOLD));

    let refused = state.spawn_within(
        "mark",
        &fixture.args(),
        None,
        Some(Duration::from_millis(5)),
    );
    assert!(
        matches!(refused, Err(SpawnError::Suspended { .. })),
        "{refused:?}"
    );

    let granted = fixture.grant();
    fixture.seam(|store| store.held_until = None);
    assert!(eventually(|| fixture.seam(|_| {}).written == 1));
    assert_eq!(fixture.store.get(ID).unwrap().unwrap(), granted);
}

/// A file found wanting and then restored has the identity it was granted with, so a grant of it
/// after that binds the same identity: the finding is about the earlier grant all the same.
#[cfg(unix)]
#[test]
fn a_late_suspension_leaves_the_same_file_granted_again_alone() {
    let fixture = Fixture::new("granted-again");
    late_suspension_of_a_restored_file(&fixture, false, |runnable| mode(&fixture, runnable));
}

/// Forgetting a plugin takes its record with it, so nothing kept in the record can tell a grant
/// made after that from the one the finding was about.
#[cfg(unix)]
#[test]
fn a_late_suspension_leaves_the_same_file_granted_after_a_forget_alone() {
    let fixture = Fixture::new("forgotten");
    late_suspension_of_a_restored_file(&fixture, true, |runnable| mode(&fixture, runnable));
}

/// The program made no program by its mode, 0644, or a program again, 0755.
#[cfg(unix)]
fn mode(fixture: &Fixture, runnable: bool) {
    use std::os::unix::fs::PermissionsExt as _;
    let mode = if runnable { 0o755 } else { 0o644 };
    fs::set_permissions(fixture.program(), fs::Permissions::from_mode(mode)).unwrap();
}

/// [`a_late_suspension_leaves_the_same_file_granted_again_alone`] where a file's mode doesn't
/// make it a program: its extension drops out of `PATHEXT`, then comes back.
#[cfg(windows)]
#[test]
fn a_late_suspension_leaves_the_same_file_granted_again_alone_by_pathext() {
    let fixture = Fixture::alone("granted-again-pathext");
    let mut pathext = None;
    late_suspension_of_a_restored_file(&fixture, false, |runnable| {
        pathext = (!runnable).then(Pathext::without_cmd);
    });
    drop(pathext);
}

/// [`a_late_suspension_leaves_the_same_file_granted_after_a_forget_alone`] where a file's mode
/// doesn't make it a program: its extension drops out of `PATHEXT`, then comes back.
#[cfg(windows)]
#[test]
fn a_late_suspension_leaves_the_same_file_granted_after_a_forget_alone_by_pathext() {
    let fixture = Fixture::alone("forgotten-pathext");
    let mut pathext = None;
    late_suspension_of_a_restored_file(&fixture, true, |runnable| {
        pathext = (!runnable).then(Pathext::without_cmd);
    });
    drop(pathext);
}

/// `PATHEXT` without `.CMD`, so the fixture's script is no program, until dropped: then it is
/// as it was.
#[cfg(windows)]
struct Pathext(Option<std::ffi::OsString>);

#[cfg(windows)]
impl Pathext {
    fn without_cmd() -> Self {
        let was = std::env::var_os("PATHEXT");
        // SAFETY: Windows keeps the environment behind a lock of its own, so nothing reading it
        // meanwhile reads it torn; and the fixture holds `programs_alone`, so no other test
        // resolves or checks a program while it's changed.
        unsafe { std::env::set_var("PATHEXT", ".COM;.EXE;.BAT") };
        Self(was)
    }
}

#[cfg(windows)]
impl Drop for Pathext {
    fn drop(&mut self) {
        // SAFETY: as in `without_cmd`.
        unsafe {
            match &self.0 {
                Some(was) => std::env::set_var("PATHEXT", was),
                None => std::env::remove_var("PATHEXT"),
            }
        }
    }
}

/// The program is granted, then made no program by `runnable(false)`, which a call finds while
/// the store is held; then made a program again by `runnable(true)` and granted again (after the
/// plugin is forgotten, if `forget`). Its content never changes, so the grant binds the same
/// identity, and the late suspension, about the earlier grant, leaves it alone.
fn late_suspension_of_a_restored_file(
    fixture: &Fixture,
    forget: bool,
    mut runnable: impl FnMut(bool),
) {
    let record = fixture.grant();
    runnable(false);
    let mut state =
        State::new(record.clone(), &Limits::default()).recorded_in(fixture.store.clone(), ID);
    fixture.seam(|store| store.held_until = Some(Instant::now() + HOLD));

    let refused = state.spawn_within(
        "mark",
        &fixture.args(),
        None,
        Some(Duration::from_millis(5)),
    );
    assert!(
        matches!(refused, Err(SpawnError::Suspended { .. })),
        "{refused:?}"
    );

    runnable(true);
    if forget {
        assert!(fixture.store.forget(ID).unwrap());
    }
    let granted = fixture.grant();
    assert_eq!(granted.programs, record.programs);
    fixture.seam(|store| store.held_until = None);
    assert!(eventually(|| fixture.seam(|_| {}).written == 1));
    assert_eq!(fixture.store.get(ID).unwrap().unwrap(), granted);
}

/// A program of eight chunks, granted.
fn big_program(fixture: &Fixture) -> crate::grants::ProgramIdentity {
    const CHUNKS: u64 = 8;
    let file = fs::OpenOptions::new()
        .append(true)
        .open(fixture.program())
        .unwrap();
    file.set_len(CHUNKS * (64 << 10)).unwrap();
    resolve_program("mark", fixture.dir.as_os_str()).unwrap()
}

#[test]
fn a_check_stops_within_one_chunk_of_the_share_end() {
    let fixture = Fixture::new("one-chunk");
    let identity = big_program(&fixture);
    // The first chunk's read alone takes the check past the end.
    hash_probe::slow_reads(Duration::from_millis(50));
    let by = Instant::now() + Duration::from_millis(10);

    assert_eq!(check_program_by(&identity, by), Err(CheckTimedOut));

    assert!(hash_probe::chunks() <= 1, "{}", hash_probe::chunks());
}

#[test]
fn past_the_end_a_program_is_not_checked_even_if_it_would_be_an_error() {
    let fixture = Fixture::new("metadata");
    let identity = resolve_program("mark", fixture.dir.as_os_str()).unwrap();
    fs::remove_file(fixture.program()).unwrap();

    // Past the end on entry: that the file is gone isn't looked at.
    let by = Instant::now();
    assert_eq!(check_program_by(&identity, by), Err(CheckTimedOut));

    // Past the end by the time the failed open returned: it doesn't count.
    let by = Instant::now() + Duration::from_millis(50);
    seams::delay_open(by + Duration::from_millis(1));
    assert_eq!(check_program_by(&identity, by), Err(CheckTimedOut));

    // In time, it's a finding.
    let by = Instant::now() + PATIENCE;
    assert!(check_program_by(&identity, by).unwrap().is_err());
}

/// A free lock isn't taken past the end, nor anything read or written under it: the file here
/// would make any read an error of its own, and no lock file is made.
#[test]
fn past_the_end_the_store_lock_is_not_accepted() {
    let fixture = Fixture::new("free-lock");
    let path = fixture.store.path().to_owned();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "not toml [").unwrap();

    let swept = fixture.store.check_programs_by(ID, Some(Instant::now()));

    assert_timed_out(&swept);
    assert!(!lock_of(&path).exists());
    // With time left the lock is taken, and the read goes as it does.
    let swept = (fixture.store).check_programs_by(ID, Some(Instant::now() + PATIENCE));
    assert!(
        matches!(swept, Err(GrantStoreError::Unreadable { .. })),
        "{swept:?}"
    );
}

/// A sweep over many programs stops at the first it reaches past the end, and writes nothing.
#[test]
fn a_sweep_past_the_end_checks_no_more_programs_and_writes_nothing() {
    let fixture = Fixture::new("sweep");
    let record = fixture.grant();
    let identity = record.programs["mark"].clone();
    let programs = BTreeMap::from([
        ("a".to_owned(), identity.clone()),
        ("mark".to_owned(), identity),
    ]);
    let record = GrantRecord { programs, ..record };
    fixture.store.set(ID, record.clone()).unwrap();
    // The file is gone, which would suspend both if they were looked at.
    fs::remove_file(fixture.program()).unwrap();

    let by = Instant::now() + Duration::from_millis(100);
    seams::delay_open(by + Duration::from_millis(1));
    let swept = fixture.store.check_programs_by(ID, Some(by));

    assert_timed_out(&swept);
    assert_eq!(fixture.store.get(ID).unwrap().unwrap(), record);
}

fn assert_timed_out<T: std::fmt::Debug>(result: &Result<T, GrantStoreError>) {
    assert!(
        matches!(result, Err(GrantStoreError::Io { source, .. })
            if source.kind() == std::io::ErrorKind::TimedOut),
        "{result:?}"
    );
}

fn lock_of(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap().to_owned();
    name.push(".lock");
    path.with_file_name(name)
}
