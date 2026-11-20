// SPDX-License-Identifier: Apache-2.0

//! A plugin's data store over a temporary state directory: the quota is charged before anything
//! is written, one counter serves every store of a plugin, and the counter is rebuilt from disk.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use kiln_plugin::bindings::kiln::extension::data::Host;
use kiln_plugin::bindings::{Error, HostState};
use kiln_plugin::data::{DATA_DIR, MAX_FILES, OVER_QUOTA, PluginData};

fn state_dir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("kiln-plugin-data-{}-{name}", std::process::id()));
    fs::remove_dir_all(&dir).ok();
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn files(state: &Path, id: &str) -> PathBuf {
    state.join(DATA_DIR).join(id)
}

fn host(data: &PluginData) -> HostState {
    HostState {
        data: kiln_plugin::data::State::new(data.clone()),
        ..HostState::default()
    }
}

fn on_disk(dir: &Path) -> u64 {
    fs::read_dir(dir)
        .map(|entries| {
            entries
                .map(|entry| entry.unwrap().metadata().unwrap().len())
                .sum()
        })
        .unwrap_or(0)
}

fn over_quota() -> Error {
    Error::Failed(OVER_QUOTA.to_owned())
}

#[test]
fn data_round_trips_through_the_imports() {
    let state = state_dir("round-trip");
    let data = PluginData::open(&state, "acme.index").unwrap();
    let mut host = host(&data);
    host.write("a.bin".into(), 0, b"hello".to_vec()).unwrap();
    host.write("a.bin".into(), 3, b"p!!".to_vec()).unwrap();
    assert_eq!(host.read("a.bin".into(), 0, 100), Ok(b"help!!".to_vec()));
    assert_eq!(host.read("a.bin".into(), 4, 100), Ok(b"!!".to_vec()));
    host.replace("b".into(), b"xyz".to_vec()).unwrap();
    host.truncate("a.bin".into(), 2).unwrap();
    assert_eq!(
        host.list(),
        Ok(vec![("a.bin".to_owned(), 2), ("b".to_owned(), 3)])
    );
    assert_eq!(host.usage().0, 5);
    host.delete("a.bin".into()).unwrap();
    host.delete("a.bin".into()).unwrap();
    assert_eq!(host.usage().0, 3);
    assert!(matches!(
        host.read("a.bin".into(), 0, 1),
        Err(Error::NotFound(_))
    ));
    fs::remove_dir_all(&state).ok();
}

#[test]
fn data_write_past_quota_is_refused_whole_and_leaves_counter() {
    let state = state_dir("past-quota");
    let data = PluginData::open(&state, "p").unwrap();
    data.set_quota(100);
    data.write("f", 0, &[1; 60]).unwrap();
    assert_eq!(data.write("f", 50, &[2; 60]), Err(over_quota()));
    assert_eq!(data.write("g", 0, &[3; 41]), Err(over_quota()));
    assert_eq!(data.truncate("f", 101), Err(over_quota()));
    assert_eq!(data.usage(), (60, 100));
    assert_eq!(data.read("f", 0, 1000).unwrap(), vec![1; 60]);
    assert!(!files(&state, "p").join("g").exists(), "no file was made");
    // What fits still goes, to the byte.
    data.write("f", 50, &[2; 50]).unwrap();
    assert_eq!(data.usage(), (100, 100));
    assert_eq!(on_disk(&files(&state, "p")), 100);
    fs::remove_dir_all(&state).ok();
}

#[test]
fn data_job_writing_in_a_loop_stops_at_quota() {
    let state = state_dir("loop");
    let data = PluginData::open(&state, "p").unwrap();
    data.set_quota(1000);
    let mut job = host(&data);
    let mut written = 0;
    let stopped = loop {
        let name = format!("chunk-{written}");
        match job.write(name, 0, vec![7; 64]) {
            Ok(()) => written += 1,
            Err(error) => break error,
        }
        assert!(written < 100, "never stopped");
    };
    assert_eq!(stopped, over_quota());
    assert_eq!(written, 15);
    assert_eq!(job.usage(), (15 * 64, 1000));
    assert_eq!(on_disk(&files(&state, "p")), 15 * 64);
    fs::remove_dir_all(&state).ok();
}

#[test]
fn data_two_workers_and_interactive_never_pass_quota_together() {
    let state = state_dir("concurrent");
    let data = PluginData::open(&state, "p").unwrap();
    data.set_quota(10_000);
    let dir = files(&state, "p");
    let stores: Vec<HostState> = (0..3).map(|_| host(&data)).collect();
    let threads: Vec<_> = stores
        .into_iter()
        .enumerate()
        .map(|(n, mut store)| {
            let dir = dir.clone();
            thread::spawn(move || {
                for i in 0..200 {
                    // Each store's own files, so only the shared counter can hold them back.
                    let _ = store.write(format!("s{n}-{i}"), 0, vec![0; 100]);
                    let used = on_disk(&dir);
                    assert!(used <= 10_000, "{used} bytes on disk");
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    assert_eq!(data.usage(), (10_000, 10_000));
    assert_eq!(on_disk(&dir), 10_000);
    fs::remove_dir_all(&state).ok();
}

#[test]
fn data_replace_is_charged_before_the_rename() {
    let state = state_dir("replace");
    let data = PluginData::open(&state, "p").unwrap();
    data.set_quota(100);
    data.replace("f", &[1; 60]).unwrap();
    // The old file still exists while the new one is written, so both count.
    assert_eq!(data.replace("f", &[2; 50]), Err(over_quota()));
    assert_eq!(data.read("f", 0, 100).unwrap(), vec![1; 60]);
    assert_eq!(data.usage(), (60, 100));
    data.replace("f", &[3; 40]).unwrap();
    assert_eq!(data.usage(), (40, 100), "the old size is given back");
    assert_eq!(data.read("f", 0, 100).unwrap(), vec![3; 40]);
    // No temporary file stays behind, and a refusal leaves none.
    let names: Vec<_> = fs::read_dir(files(&state, "p"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names.len(), 1);
    fs::remove_dir_all(&state).ok();
}

#[test]
fn data_crash_left_file_counted_at_load() {
    let state = state_dir("crash");
    let dir = files(&state, "p");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("left"), [0; 70]).unwrap();
    fs::write(dir.join(".left.1.2.tmp"), [0; 500]).unwrap();
    let data = PluginData::open(&state, "p").unwrap();
    data.set_quota(100);
    assert_eq!(data.usage(), (70, 100));
    assert!(!dir.join(".left.1.2.tmp").exists(), "a temporary is swept");
    assert_eq!(data.write("more", 0, &[0; 31]), Err(over_quota()));
    data.write("more", 0, &[0; 30]).unwrap();
    // Opened again, even while the first is held, it counts what is on disk.
    fs::write(dir.join("late"), [0; 5]).unwrap();
    let again = PluginData::open(&state, "p").unwrap();
    assert_eq!(again.usage().0, 105);
    fs::remove_dir_all(&state).ok();
}

#[test]
fn data_lowered_quota_allows_reads_and_deletes_but_not_growth() {
    let state = state_dir("lowered");
    let data = PluginData::open(&state, "p").unwrap();
    data.set_quota(1000);
    data.write("a", 0, &[1; 400]).unwrap();
    data.write("b", 0, &[2; 400]).unwrap();
    data.set_quota(500);
    assert_eq!(data.write("a", 400, &[1]), Err(over_quota()));
    assert_eq!(data.write("c", 0, &[1]), Err(over_quota()));
    assert_eq!(data.replace("a", &[9; 10]), Err(over_quota()));
    assert_eq!(data.truncate("a", 401), Err(over_quota()));
    assert_eq!(data.read("a", 0, 1000).unwrap(), vec![1; 400]);
    data.write("a", 0, &[5; 10]).unwrap();
    data.truncate("a", 100).unwrap();
    data.delete("b").unwrap();
    assert_eq!(data.usage(), (100, 500));
    data.write("c", 0, &[1; 400]).unwrap();
    assert_eq!(data.usage(), (500, 500));
    fs::remove_dir_all(&state).ok();
}

#[test]
fn data_names_outside_pattern_refused() {
    let state = state_dir("names");
    let data = PluginData::open(&state, "p").unwrap();
    let long = "a".repeat(129);
    for name in [
        "", ".hidden", "..", "a/b", "a\\b", "UPPER", "sp ace", "tab\t", "é", "nul\0", &long,
    ] {
        assert!(
            matches!(data.write(name, 0, b"x"), Err(Error::InvalidArgument(_))),
            "{name:?}"
        );
        assert!(matches!(
            data.read(name, 0, 1),
            Err(Error::InvalidArgument(_))
        ));
        assert!(matches!(data.delete(name), Err(Error::InvalidArgument(_))));
        assert!(matches!(
            data.replace(name, b"x"),
            Err(Error::InvalidArgument(_))
        ));
    }
    assert!(!files(&state, "p").exists(), "nothing was made");
    data.write(&"a".repeat(128), 0, b"x").unwrap();
    data.write("a.b_c-9", 0, b"x").unwrap();
    // The 10,000 name limit.
    let dir = files(&state, "p");
    for n in 0..MAX_FILES - 2 {
        fs::write(dir.join(format!("f{n}")), b"").unwrap();
    }
    let data = PluginData::open(&state, "p").unwrap();
    assert!(matches!(
        data.write("one-more", 0, b""),
        Err(Error::Failed(_))
    ));
    // Over a large oversize request.
    assert!(matches!(
        data.read("a.b_c-9", 0, (16 << 20) + 1),
        Err(Error::InvalidArgument(_))
    ));
    fs::remove_dir_all(&state).ok();
}

#[test]
fn data_other_plugins_files_unreachable() {
    let state = state_dir("isolation");
    let a = PluginData::open(&state, "acme.a").unwrap();
    let b = PluginData::open(&state, "acme.b").unwrap();
    a.write("secret", 0, b"mine").unwrap();
    assert!(matches!(b.read("secret", 0, 10), Err(Error::NotFound(_))));
    assert_eq!(b.list().unwrap(), vec![]);
    assert_eq!(b.usage().0, 0);
    // A name can't climb out, and an id can't name another directory.
    assert!(matches!(
        b.read("../acme.a/secret", 0, 10),
        Err(Error::InvalidArgument(_))
    ));
    assert!(PluginData::open(&state, "../acme.a").is_err());
    assert!(PluginData::open(&state, "a/b").is_err());
    b.clear().unwrap();
    assert_eq!(a.read("secret", 0, 10).unwrap(), b"mine");
    fs::remove_dir_all(&state).ok();
}

#[test]
fn data_clear_during_a_write() {
    let state = state_dir("clear");
    let data = PluginData::open(&state, "p").unwrap();
    data.set_quota(1 << 20);
    let dir = files(&state, "p");
    let stop = Arc::new(AtomicBool::new(false));
    let writers: Vec<_> = (0..2)
        .map(|n| {
            let (data, stop) = (data.clone(), stop.clone());
            thread::spawn(move || {
                let mut i = 0;
                while !stop.load(Ordering::Relaxed) {
                    let _ = data.write(&format!("w{n}-{}", i % 20), 0, &[1; 500]);
                    let _ = data.replace(&format!("r{n}-{}", i % 20), &[2; 300]);
                    i += 1;
                }
            })
        })
        .collect();
    // The shared instance of the directory, as Clear Data finds it.
    let clearing = PluginData::open(&state, "p").unwrap();
    for _ in 0..50 {
        clearing.clear().unwrap();
    }
    stop.store(true, Ordering::Relaxed);
    for writer in writers {
        writer.join().unwrap();
    }
    // Whatever the last writes left, the counter is what is on disk.
    assert_eq!(data.usage().0, on_disk(&dir));
    clearing.clear().unwrap();
    assert_eq!(data.usage().0, 0);
    assert_eq!(on_disk(&dir), 0);
    fs::remove_dir_all(&state).ok();
}

#[test]
fn data_deleted_on_uninstall() {
    let state = state_dir("uninstall");
    let data = PluginData::open(&state, "p").unwrap();
    data.write("a", 0, b"1").unwrap();
    data.replace("b", b"22").unwrap();
    let other = PluginData::open(&state, "q").unwrap();
    other.write("a", 0, b"3").unwrap();
    data.remove().unwrap();
    assert!(!files(&state, "p").exists());
    assert_eq!(data.usage().0, 0);
    // A later write starts the directory again.
    data.write("a", 0, b"1").unwrap();
    assert_eq!(data.usage().0, 1);
    assert_eq!(other.read("a", 0, 10).unwrap(), b"3");
    PluginData::open(&state, "never-written")
        .unwrap()
        .remove()
        .unwrap();
    fs::remove_dir_all(&state).ok();
}

#[test]
fn data_empty_write_beyond_end_charges_nothing() {
    let state = state_dir("empty-write");
    let data = PluginData::open(&state, "p").unwrap();
    data.set_quota(100);
    for _ in 0..10 {
        data.write("f", 100, &[]).unwrap();
    }
    assert_eq!(data.usage(), (0, 100));
    assert_eq!(on_disk(&files(&state, "p")), 0);
    data.delete("f").unwrap();
    assert_eq!(data.usage(), (0, 100));
    data.write("g", 0, &[1; 100]).unwrap();
    fs::remove_dir_all(&state).ok();
}

/// A temporary file that can't be deleted still takes its bytes, so it counts.
#[cfg(unix)]
#[test]
fn data_undeletable_temporary_file_is_counted_at_load() {
    use std::os::unix::fs::PermissionsExt as _;
    let state = state_dir("stuck-temp");
    let dir = files(&state, "p");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(".stuck.1.2.tmp"), [0; 40]).unwrap();
    fs::write(dir.join("kept"), [0; 10]).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).unwrap();
    // A directory that denies deletion to everyone, as Windows does to a held file.
    let writable = fs::File::create(dir.join("probe")).is_ok();
    let data = PluginData::open(&state, "p").unwrap();
    let used = data.usage().0;
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
    if writable {
        // Running as a user the mode doesn't bind: nothing to check.
        fs::remove_dir_all(&state).ok();
        return;
    }
    assert_eq!(used, 50, "the stuck file counts");
    fs::remove_dir_all(&state).ok();
}

#[test]
fn a_host_without_a_store_answers_unimplemented() {
    let mut host = HostState::default();
    assert!(matches!(
        host.read("a".into(), 0, 1),
        Err(Error::Unimplemented(_))
    ));
    assert_eq!(host.usage(), (0, 0));
}

mod component {
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};

    use kiln_command::{Capability, Grants};
    use kiln_plugin::bindings::HostState;
    use kiln_plugin::data::State;
    use kiln_plugin::engine::PluginEngine;
    use kiln_plugin::jobs::{self, JobHost, JobStatus};
    use kiln_plugin::limits::{BackgroundLimits, Ceilings, Limits};
    use kiln_plugin::runner::{Call, Plugin, Reply};

    use super::*;

    const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/data_jobs.wat");

    fn run(plugin: &mut Plugin, id: &str, args: &str) -> Result<(), Error> {
        let call = Call::RunCommand {
            id: id.to_owned(),
            args: args.to_owned(),
        };
        match plugin.call(call).unwrap() {
            Reply::RunCommand(result) => result,
            reply => panic!("{reply:?}"),
        }
    }

    /// A real job, run by the runner in a worker store it makes itself, and the interactive
    /// instance write to one counter: the second write past the quota is refused whichever
    /// of the two makes it.
    #[test]
    fn data_job_and_interactive_instance_share_one_quota_through_the_runner() {
        let state = state_dir("component");
        let data = PluginData::open(&state, "acme.jobs").unwrap();
        data.set_quota(100);
        let (sender, _logged) = mpsc::channel();
        let mut host = HostState::new(Box::new(move |_, message| drop(sender.send(message))));
        host.data = State::new(data.clone());
        host.jobs = jobs::State::new(
            JobHost::new(Ceilings::for_ram(64 << 30)),
            BackgroundLimits::default(),
        );
        let grants: Grants = [Capability::parse("jobs").unwrap()].into_iter().collect();
        let limits = Limits {
            call_deadline: Duration::from_secs(2),
            ..Limits::default()
        };
        let component = wat::parse_file(FIXTURE).unwrap();
        let mut plugin = Plugin::load_hosted(
            PluginEngine::shared().unwrap(),
            None,
            &component,
            "acme.jobs",
            &grants,
            limits,
            host,
        )
        .unwrap();
        let jobs = plugin.jobs().unwrap().clone();
        let wait = |job| {
            let until = Instant::now() + Duration::from_secs(10);
            while matches!(
                jobs.state(job),
                JobStatus::Queued(_) | JobStatus::Running(_)
            ) {
                assert!(Instant::now() < until, "the job never ended");
                thread::sleep(Duration::from_millis(10));
            }
        };
        let started = |plugin: &mut Plugin, args: &str| {
            let before = jobs.ids();
            run(plugin, "zput", args).unwrap();
            *jobs.ids().iter().find(|id| !before.contains(id)).unwrap()
        };
        // The job's worker store writes 60 bytes: the interactive instance's counter has them.
        let first = started(&mut plugin, &"j".repeat(60));
        wait(first);
        assert_eq!(jobs.state(first), JobStatus::Succeeded);
        assert_eq!(data.usage(), (60, 100));
        assert_eq!(data.read("job-file", 0, 100).unwrap(), vec![b'j'; 60]);
        // The interactive instance's 60 bytes more are past the quota.
        assert_eq!(run(&mut plugin, "dput", &"i".repeat(60)), Err(over_quota()));
        assert_eq!(data.usage(), (60, 100));
        run(&mut plugin, "dput", &"i".repeat(40)).unwrap();
        assert_eq!(data.usage(), (100, 100));
        // And a job past it fails, with nothing written.
        let second = {
            let before = jobs.ids();
            run(&mut plugin, "zput", &"j".repeat(80)).unwrap();
            *jobs.ids().iter().find(|id| !before.contains(id)).unwrap()
        };
        wait(second);
        assert_eq!(jobs.state(second), JobStatus::Failed);
        assert_eq!(data.usage(), (100, 100));
        assert_eq!(on_disk(&files(&state, "acme.jobs")), 100);
        drop(plugin);
        fs::remove_dir_all(&state).ok();
    }
}
