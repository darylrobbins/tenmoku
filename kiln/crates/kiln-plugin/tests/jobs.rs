// SPDX-License-Identifier: Apache-2.0

//! Background jobs: worker stores without `init`, the slice, the wall time, the queue and spec
//! limits, cancellation, results held until delivered and capped, the root check, unloading,
//! and the `jobs` capability. Each test loads `fixtures/jobs.wat` on a [`JobHost`] of its own,
//! so the limits across plugins are its alone.

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Receiver},
    },
    thread,
    time::{Duration, Instant},
};

use kiln_command::{Capability, Grants};
use kiln_plugin::{
    bindings::{Error, HostState},
    commands::{self, CommandHost, Dispatcher, Request},
    engine::PluginEngine,
    grants::{GrantRecord, resolve_program},
    jobs::{self, CANCEL_GRACE, JobHost, JobId, JobKind, JobSpec, JobStatus, Jobs, SLICE, Waiting},
    limits::{BackgroundLimits, Ceilings, Limits},
    net::{self, NetBackend, Outgoing, Received, SendCancel},
    process,
    runner::{Call, LoadError, Plugin, PluginState, Reply},
};

#[path = "support/script.rs"]
mod support;

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/jobs.wat");

fn grants(capabilities: &[&str]) -> Grants {
    capabilities
        .iter()
        .map(|capability| Capability::parse(capability).unwrap())
        .collect()
}

/// A queue of its own, on a computer with plenty of RAM.
fn job_host() -> JobHost {
    JobHost::new(Ceilings::for_ram(64 << 30))
}

/// The fixture loaded as `acme.jobs` with `capabilities` on `host`, its jobs under `background`,
/// and what its interactive instance logs.
fn load_with(
    host: &JobHost,
    capabilities: &[&str],
    background: BackgroundLimits,
) -> Result<(Plugin, Receiver<String>), LoadError> {
    load_jobs(jobs::State::new(host.clone(), background), capabilities)
}

/// The fixture loaded as [`load_with`] does, its jobs slice `jobs`.
fn load_jobs(
    jobs: jobs::State,
    capabilities: &[&str],
) -> Result<(Plugin, Receiver<String>), LoadError> {
    let component = wat::parse_file(FIXTURE).unwrap();
    load_component(jobs, capabilities, &component, None)
}

/// [`load_jobs`], attached to `commands`, as its jobs are too.
fn load_jobs_on(
    jobs: jobs::State,
    capabilities: &[&str],
    commands: &CommandHost,
) -> Result<(Plugin, Receiver<String>), LoadError> {
    let component = wat::parse_file(FIXTURE).unwrap();
    load_component(jobs, capabilities, &component, Some(commands))
}

/// [`load_jobs`] of `component`, attached to `commands` if any.
fn load_component(
    jobs: jobs::State,
    capabilities: &[&str],
    component: &[u8],
    commands: Option<&CommandHost>,
) -> Result<(Plugin, Receiver<String>), LoadError> {
    let (sender, logged) = mpsc::channel();
    let mut state = HostState::new(Box::new(move |_, message| drop(sender.send(message))));
    state.jobs = jobs;
    if let Some(commands) = commands {
        let granted = grants(capabilities);
        state.commands = commands::State::new("acme.jobs", &granted, commands).unwrap();
    }
    let limits = Limits {
        call_deadline: Duration::from_secs(2),
        ..Limits::default()
    };
    let plugin = Plugin::load_hosted(
        PluginEngine::shared().unwrap(),
        None,
        component,
        "acme.jobs",
        &grants(capabilities),
        limits,
        state,
    )?;
    Ok((plugin, logged))
}

/// [`load_with`] with only `jobs`, under the default background limits.
fn load(host: &JobHost) -> (Plugin, Receiver<String>) {
    load_with(host, &["jobs"], BackgroundLimits::default()).unwrap()
}

/// What the fixture's `run-command(id, args)` returned.
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

fn jobs_of(plugin: &Plugin) -> Jobs {
    plugin.jobs().expect("loaded with jobs").clone()
}

/// Starts job `name` with `args` through the fixture's `jobs.start`, and returns its id.
fn start(plugin: &mut Plugin, name: &str, args: &str) -> JobId {
    let before = jobs_of(plugin).ids();
    run(plugin, name, args).unwrap();
    let after = jobs_of(plugin).ids();
    *after.iter().find(|id| !before.contains(id)).unwrap()
}

/// `jobs.state` of `job`, asked by the fixture, as the index of its case.
fn state_via_import(plugin: &mut Plugin, logged: &Receiver<String>, job: JobId) -> String {
    run(plugin, "state", &job.to_string()).unwrap();
    logged.try_recv().unwrap()
}

/// Waits up to `timeout` for `done`, and returns whether it came.
fn wait_for(timeout: Duration, mut done: impl FnMut() -> bool) -> bool {
    let until = Instant::now() + timeout;
    while Instant::now() < until {
        if done() {
            return true;
        }
        thread::sleep(Duration::from_millis(10));
    }
    done()
}

fn ended(status: &JobStatus) -> bool {
    !matches!(
        status,
        JobStatus::Queued(_) | JobStatus::Running(_) | JobStatus::Unknown
    )
}

/// What `job-finished` logged since, each delivery's lines joined.
fn finished_logs(logged: &Receiver<String>) -> Vec<String> {
    logged.try_iter().collect()
}

#[test]
fn jobs_run_in_a_worker_store_without_init() {
    let host = job_host();
    let (mut plugin, logged) = load(&host);
    let job = start(&mut plugin, "who", "");
    let jobs = jobs_of(&plugin);
    assert!(wait_for(Duration::from_secs(10), || ended(
        &jobs.state(job)
    )));
    assert_eq!(jobs.state(job), JobStatus::Succeeded);
    assert_eq!(plugin.deliver_jobs(), Ok(1));
    // The interactive instance ran `init`; the worker didn't.
    assert_eq!(finished_logs(&logged), ["o", "no init"]);
}

#[test]
fn jobs_slice_overrun_kills_the_job_not_the_plugin() {
    let host = job_host();
    let (mut plugin, logged) = load(&host);
    let started = Instant::now();
    let job = start(&mut plugin, "loop", "");
    let jobs = jobs_of(&plugin);
    assert!(wait_for(SLICE * 3, || ended(&jobs.state(job))));
    let took = started.elapsed();
    assert!(took >= SLICE - Duration::from_millis(100), "{took:?}");
    assert_eq!(jobs.state(job), JobStatus::Failed);
    assert_eq!(host.counts(), jobs::Counts::default());

    assert_eq!(plugin.state(), &PluginState::Running);
    assert_eq!(plugin.deliver_jobs(), Ok(1));
    assert_eq!(finished_logs(&logged), ["e", jobs::SLICE_OVERRUN]);
    // The plugin runs on, and so do its next jobs.
    let next = start(&mut plugin, "echo", "again");
    assert!(wait_for(Duration::from_secs(10), || ended(
        &jobs.state(next)
    )));
    assert_eq!(plugin.deliver_jobs(), Ok(1));
    assert_eq!(finished_logs(&logged), ["o", "again"]);
    assert_eq!(plugin.state(), &PluginState::Running);
}

#[test]
fn jobs_wall_time_ends_the_job() {
    let host = job_host();
    let wall = Duration::from_secs(1);
    let background = BackgroundLimits {
        wall,
        ..BackgroundLimits::default()
    };
    let (mut plugin, logged) = load_with(&host, &["jobs"], background).unwrap();
    let started = Instant::now();
    // It never stops for `cancelled`, so it runs on for the grace.
    let job = start(&mut plugin, "spin", "");
    let jobs = jobs_of(&plugin);
    assert!(wait_for(Duration::from_secs(15), || ended(
        &jobs.state(job)
    )));
    let took = started.elapsed();
    assert!(
        took >= wall + CANCEL_GRACE - Duration::from_millis(100),
        "{took:?}"
    );
    assert_eq!(jobs.state(job), JobStatus::TimedOut);
    assert_eq!(state_via_import(&mut plugin, &logged, job), "5");
    assert_eq!(plugin.deliver_jobs(), Ok(1));
    assert_eq!(finished_logs(&logged), ["t"]);
    assert_eq!(plugin.state(), &PluginState::Running);
}

#[test]
fn jobs_ninth_job_is_refused() {
    let host = job_host();
    let (mut plugin, _logged) = load(&host);
    let started: Vec<JobId> = (0..jobs::JOBS_PER_PLUGIN)
        .map(|_| start(&mut plugin, "spin", ""))
        .collect();
    let jobs = jobs_of(&plugin);
    let states: Vec<JobStatus> = started.iter().map(|&job| jobs.state(job)).collect();
    let running = states
        .iter()
        .filter(|state| matches!(state, JobStatus::Running(_)))
        .count();
    let queued = states
        .iter()
        .filter(|state| **state == JobStatus::Queued(Waiting::Concurrency))
        .count();
    assert_eq!(
        (running, queued),
        (jobs::RUNNING_PER_PLUGIN, 6),
        "{states:?}"
    );
    // The first two run, in order.
    assert!(matches!(states[0], JobStatus::Running(_)));
    assert!(matches!(states[1], JobStatus::Running(_)));

    assert_eq!(
        run(&mut plugin, "spin", ""),
        Err(Error::Failed(jobs::TOO_MANY_JOBS.to_owned()))
    );
    assert_eq!(jobs.ids(), started);
    assert_eq!(
        host.counts(),
        jobs::Counts {
            queued: 6,
            running: 2
        }
    );
}

#[test]
fn jobs_oversize_spec_is_refused() {
    let host = job_host();
    let (mut plugin, _logged) = load(&host);
    let name = "e".repeat(jobs::MAX_NAME_BYTES + 1);
    assert_eq!(
        run(&mut plugin, &name, ""),
        Err(Error::InvalidArgument(jobs::NAME_TOO_LONG.to_owned()))
    );
    let args = "a".repeat(jobs::MAX_ARGS_BYTES + 1);
    assert_eq!(
        run(&mut plugin, "echo", &args),
        Err(Error::InvalidArgument(jobs::ARGS_TOO_LARGE.to_owned()))
    );
    let jobs = jobs_of(&plugin);
    let title = JobSpec {
        name: "echo".to_owned(),
        args: String::new(),
        title: "t".repeat(jobs::MAX_TITLE_CHARS + 1),
        kind: JobKind::Other,
        cancellable: true,
        root: None,
    };
    assert_eq!(
        jobs.start(title),
        Err(Error::InvalidArgument(jobs::TITLE_TOO_LONG.to_owned()))
    );
    // Nothing was queued.
    assert!(jobs.ids().is_empty());
    assert_eq!(host.counts(), jobs::Counts::default());

    // At the limits, it is taken.
    let name = "e".repeat(jobs::MAX_NAME_BYTES);
    let args = "a".repeat(jobs::MAX_ARGS_BYTES);
    let job = start(&mut plugin, &name, &args);
    assert!(wait_for(Duration::from_secs(10), || ended(
        &jobs.state(job)
    )));
    assert_eq!(jobs.state(job), JobStatus::Succeeded);
}

#[test]
fn jobs_cancel_before_start() {
    let host = job_host();
    let (mut plugin, logged) = load(&host);
    let spinning = [
        start(&mut plugin, "spin", ""),
        start(&mut plugin, "spin", ""),
    ];
    let job = start(&mut plugin, "echo", "never");
    assert_eq!(state_via_import(&mut plugin, &logged, job), "0");

    run(&mut plugin, "cancel", &job.to_string()).unwrap();
    assert_eq!(state_via_import(&mut plugin, &logged, job), "4");
    let jobs = jobs_of(&plugin);
    assert_eq!(jobs.state(job), JobStatus::Cancelled);
    assert_eq!(
        host.counts(),
        jobs::Counts {
            queued: 0,
            running: 2
        }
    );
    // It never ran: its end is the cancel, not the echo.
    assert_eq!(plugin.deliver_jobs(), Ok(1));
    assert_eq!(finished_logs(&logged), ["c"]);
    assert_eq!(state_via_import(&mut plugin, &logged, job), "6");
    assert_eq!(
        run(&mut plugin, "cancel", &job.to_string()),
        Err(Error::NotFound(jobs::NO_SUCH_JOB.to_owned()))
    );
    for job in spinning {
        jobs.cancel(job).unwrap();
    }
}

#[test]
fn jobs_cancel_while_running_with_2s_grace() {
    let host = job_host();
    let (mut plugin, logged) = load(&host);
    let job = start(&mut plugin, "spin", "");
    let jobs = jobs_of(&plugin);
    assert!(matches!(jobs.state(job), JobStatus::Running(_)));
    // Past a slice: it enters the host on each turn, so the call hook renews its slice.
    thread::sleep(SLICE + Duration::from_millis(500));
    assert!(
        matches!(jobs.state(job), JobStatus::Running(_)),
        "{:?}",
        jobs.state(job)
    );

    let cancelled = Instant::now();
    run(&mut plugin, "cancel", &job.to_string()).unwrap();
    // It ignores `cancelled`, so it runs for the grace, then is stopped.
    thread::sleep(CANCEL_GRACE - Duration::from_millis(500));
    assert!(
        matches!(jobs.state(job), JobStatus::Running(_)),
        "{:?}",
        jobs.state(job)
    );
    assert!(wait_for(Duration::from_secs(10), || ended(
        &jobs.state(job)
    )));
    let took = cancelled.elapsed();
    assert!(took >= CANCEL_GRACE, "{took:?}");
    assert!(took < CANCEL_GRACE + Duration::from_secs(3), "{took:?}");
    assert_eq!(jobs.state(job), JobStatus::Cancelled);
    assert_eq!(host.counts(), jobs::Counts::default());
    assert_eq!(plugin.deliver_jobs(), Ok(1));
    assert_eq!(finished_logs(&logged), ["c"]);
    assert_eq!(plugin.state(), &PluginState::Running);
}

#[test]
fn jobs_result_reaches_job_finished_once_then_state_unknown() {
    let host = job_host();
    let (mut plugin, logged) = load(&host);
    let job = start(&mut plugin, "echo", "hello");
    let jobs = jobs_of(&plugin);
    assert!(wait_for(Duration::from_secs(10), || ended(
        &jobs.state(job)
    )));
    assert_eq!(state_via_import(&mut plugin, &logged, job), "2");
    assert_eq!(jobs.undelivered_bytes(), "hello".len());

    assert_eq!(plugin.deliver_jobs(), Ok(1));
    assert_eq!(finished_logs(&logged), ["o", "hello"]);
    // Delivered, it is dropped: once only, and then unknown.
    assert_eq!(jobs.undelivered_bytes(), 0);
    assert_eq!(plugin.deliver_jobs(), Ok(0));
    assert_eq!(finished_logs(&logged), Vec::<String>::new());
    assert_eq!(state_via_import(&mut plugin, &logged, job), "6");
    assert_eq!(jobs.state(job), JobStatus::Unknown);
    assert!(jobs.ids().is_empty());
}

#[test]
fn jobs_undelivered_results_over_32mib_refused() {
    let host = job_host();
    let (mut plugin, logged) = load(&host);
    // Three 12 MiB results: the third to end would pass 32 MiB.
    let started: Vec<JobId> = (0..3).map(|_| start(&mut plugin, "big", "")).collect();
    let jobs = jobs_of(&plugin);
    assert!(wait_for(Duration::from_secs(20), || {
        started.iter().all(|&job| ended(&jobs.state(job)))
    }));
    let states: Vec<JobStatus> = started.iter().map(|&job| jobs.state(job)).collect();
    let succeeded = states
        .iter()
        .filter(|state| **state == JobStatus::Succeeded)
        .count();
    assert_eq!(succeeded, 2, "{states:?}");
    assert!(states.contains(&JobStatus::Failed), "{states:?}");
    // The dropped one's result was never copied out: only its reason is kept.
    assert_eq!(
        jobs.undelivered_bytes(),
        2 * (12 << 20) + jobs::RESULT_DROPPED.len()
    );
    assert!(jobs.undelivered_bytes() <= jobs::UNDELIVERED_BYTES);

    assert_eq!(plugin.deliver_jobs(), Ok(3));
    let logs = finished_logs(&logged);
    let codes: Vec<&str> = logs.iter().step_by(2).map(String::as_str).collect();
    assert_eq!(
        codes.iter().filter(|code| **code == "o").count(),
        2,
        "{codes:?}"
    );
    let dropped = logs.iter().position(|line| line == "e").unwrap();
    assert_eq!(logs[dropped + 1], jobs::RESULT_DROPPED);
    assert_eq!(jobs.undelivered_bytes(), 0);

    // Delivered, they no longer count.
    let next = start(&mut plugin, "big", "");
    assert!(wait_for(Duration::from_secs(10), || ended(
        &jobs.state(next)
    )));
    assert_eq!(jobs.state(next), JobStatus::Succeeded);
}

#[test]
fn jobs_root_not_open_is_refused() {
    let host = job_host();
    let root: PathBuf =
        std::env::temp_dir().join(format!("kiln-plugin-jobs-root-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let guest = kiln_plugin::paths::root(&root, kiln_plugin::paths::Rules::HOST).unwrap();
    host.set_roots(vec![root.clone()]);

    let (mut plugin, logged) = load_with(
        &host,
        &["jobs", "fs.read:workspace"],
        BackgroundLimits::default(),
    )
    .unwrap();
    let refused = Err(Error::InvalidArgument(jobs::ROOT_NOT_OPEN.to_owned()));
    assert_eq!(run(&mut plugin, "echo", "/not/an/open/root"), refused);
    // Inside an open root isn't its root.
    assert_eq!(run(&mut plugin, "echo", &format!("{guest}/src")), refused);
    let jobs = jobs_of(&plugin);
    assert!(jobs.ids().is_empty());

    // `run-job` is told the open root.
    let job = start(&mut plugin, "echo", &guest);
    assert!(wait_for(Duration::from_secs(10), || ended(
        &jobs.state(job)
    )));
    assert_eq!(plugin.deliver_jobs(), Ok(1));
    assert_eq!(finished_logs(&logged), ["o", guest.as_str()]);

    // Without an fs grant reaching it, an open root is refused too.
    let (mut ungranted, _logged) = load(&host);
    assert_eq!(run(&mut ungranted, "echo", &guest), refused);
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn jobs_unload_drops_queued_jobs_and_results() {
    let host = job_host();
    let (mut plugin, logged) = load(&host);
    let delivered = start(&mut plugin, "echo", "kept");
    let jobs = jobs_of(&plugin);
    assert!(wait_for(Duration::from_secs(10), || ended(
        &jobs.state(delivered)
    )));
    let spinning = [
        start(&mut plugin, "spin", ""),
        start(&mut plugin, "spin", ""),
    ];
    let queued = start(&mut plugin, "echo", "queued");
    assert_eq!(jobs.state(queued), JobStatus::Queued(Waiting::Concurrency));
    assert_eq!(jobs.undelivered_bytes(), "kept".len());

    drop(plugin);
    // Queued ones and results are gone at once; every id is unknown.
    assert!(jobs.ids().is_empty());
    assert_eq!(jobs.undelivered_bytes(), 0);
    for job in [delivered, queued].into_iter().chain(spinning) {
        assert_eq!(jobs.state(job), JobStatus::Unknown);
    }
    assert_eq!(host.counts().queued, 0);
    assert_eq!(jobs.take_ended(), None);
    // The running ones are cancelled, and stop within the grace.
    assert!(wait_for(CANCEL_GRACE + Duration::from_secs(3), || {
        host.counts() == jobs::Counts::default()
    }));
    // Nothing reached `job-finished`.
    assert_eq!(finished_logs(&logged), Vec::<String>::new());
}

#[test]
fn jobs_without_the_capability_cannot_link() {
    let host = job_host();
    let refused = load_with(&host, &["fs.read:workspace"], BackgroundLimits::default());
    assert!(
        matches!(refused, Err(LoadError::Invalid { .. })),
        "{:?}",
        refused.map(|_| ())
    );
}

#[test]
fn jobs_memory_grow_does_not_renew_the_slice() {
    let host = job_host();
    let (mut plugin, logged) = load(&host);
    let started = Instant::now();
    // `memory.grow` enters wasmtime's code, but no host import.
    let job = start(&mut plugin, "grow", "");
    let jobs = jobs_of(&plugin);
    assert!(wait_for(SLICE * 3, || ended(&jobs.state(job))));
    let took = started.elapsed();
    assert!(took >= SLICE - Duration::from_millis(100), "{took:?}");
    assert_eq!(plugin.deliver_jobs(), Ok(1));
    assert_eq!(finished_logs(&logged), ["e", jobs::SLICE_OVERRUN]);
}

#[test]
fn jobs_results_and_errors_over_16mib_are_refused() {
    let host = job_host();
    let (mut plugin, logged) = load(&host);
    let jobs = jobs_of(&plugin);
    for name in ["huge", "fat"] {
        let job = start(&mut plugin, name, "");
        assert!(wait_for(Duration::from_secs(10), || ended(
            &jobs.state(job)
        )));
        assert_eq!(jobs.state(job), JobStatus::Failed, "{name}");
        // Only the reason is kept.
        assert_eq!(
            jobs.undelivered_bytes(),
            jobs::RESULT_TOO_LARGE.len(),
            "{name}"
        );
        assert_eq!(plugin.deliver_jobs(), Ok(1));
        assert_eq!(
            finished_logs(&logged),
            ["e", jobs::RESULT_TOO_LARGE],
            "{name}"
        );
    }
}

/// Keeps every invocation unanswered, so a plugin's `commands.invoke` waits until its
/// deadline.
#[derive(Default)]
struct Withheld(Mutex<Vec<Request>>);

impl Dispatcher for Withheld {
    fn dispatch(&self, request: Request) {
        self.0.lock().unwrap().push(request);
    }
}

/// The fixture with `jobs` and `commands`, under `background`, its workers' `commands.invoke`
/// never answered.
fn load_invoking(host: &JobHost, background: BackgroundLimits) -> (Plugin, Receiver<String>) {
    let capabilities = ["jobs", "commands"];
    let commands = CommandHost::new([], Arc::new(Withheld::default()));
    let jobs = jobs::State::new(host.clone(), background);
    load_jobs_on(jobs, &capabilities, &commands).unwrap()
}

#[test]
fn jobs_wall_time_ends_a_job_waiting_in_an_import() {
    let host = job_host();
    let wall = Duration::from_secs(1);
    let background = BackgroundLimits {
        wall,
        ..BackgroundLimits::default()
    };
    let (mut plugin, logged) = load_invoking(&host, background);
    let started = Instant::now();
    // Its first `invoke` would wait forever: it waits only until the wall time.
    let job = start(&mut plugin, "invoke", "");
    let jobs = jobs_of(&plugin);
    assert!(wait_for(Duration::from_secs(15), || ended(
        &jobs.state(job)
    )));
    let took = started.elapsed();
    assert!(took >= wall, "{took:?}");
    assert!(
        took < wall + CANCEL_GRACE + Duration::from_secs(3),
        "{took:?}"
    );
    assert_eq!(jobs.state(job), JobStatus::TimedOut);
    assert_eq!(plugin.deliver_jobs(), Ok(1));
    assert_eq!(finished_logs(&logged), ["t"]);
}

#[test]
fn jobs_cancel_fails_an_import_begun_after_it() {
    let host = job_host();
    let (mut plugin, logged) = load_invoking(&host, BackgroundLimits::default());
    // It invokes once cancelled, which would wait past its 10-minute wall time.
    let job = start(&mut plugin, "tardy", "");
    let jobs = jobs_of(&plugin);
    assert!(matches!(jobs.state(job), JobStatus::Running(_)));
    let cancelled = Instant::now();
    jobs.cancel(job).unwrap();
    assert!(wait_for(Duration::from_secs(15), || ended(
        &jobs.state(job)
    )));
    // The invoke fails as cancelled at once, and the job returns.
    let took = cancelled.elapsed();
    assert!(took < CANCEL_GRACE, "{took:?}");
    assert_eq!(jobs.state(job), JobStatus::Cancelled);
    assert_eq!(plugin.deliver_jobs(), Ok(1));
    assert_eq!(finished_logs(&logged), ["c"]);
}

#[test]
fn jobs_delivery_hands_over_only_the_jobs_ended_before_it() {
    let host = job_host();
    let (mut plugin, logged) = load(&host);
    let job = start(&mut plugin, "chain", "chain");
    let jobs = jobs_of(&plugin);
    assert!(wait_for(Duration::from_secs(10), || ended(
        &jobs.state(job)
    )));
    // Its `job-finished` starts another and waits for it to end: that one waits for the next
    // delivery.
    assert_eq!(plugin.deliver_jobs(), Ok(1));
    assert_eq!(finished_logs(&logged), ["o", "chain"]);
    assert_eq!(jobs.undelivered(), 1);
    assert_eq!(plugin.state(), &PluginState::Running);
}

#[test]
fn jobs_a_plugin_lifting_utf16_strings_is_refused() {
    let host = job_host();
    let text = std::fs::read_to_string(FIXTURE).unwrap();
    let lift = r#"(canon lift (core func $main "run-job") (memory $memory)"#;
    assert!(text.contains(lift));
    let utf16 = text.replace(
        lift,
        r#"(canon lift (core func $main "run-job") string-encoding=utf16 (memory $memory)"#,
    );
    let component = wat::parse_str(&utf16).unwrap();
    let jobs = jobs::State::new(host.clone(), BackgroundLimits::default());
    let refused = load_component(jobs, &["jobs"], &component, None).map(|_| ());
    assert!(
        matches!(&refused, Err(LoadError::Invalid { message, .. }) if message == jobs::NOT_UTF8),
        "{refused:?}"
    );
    // As text, a component isn't checked: it is refused too.
    let jobs = jobs::State::new(host, BackgroundLimits::default());
    let text = load_component(jobs, &["jobs"], text.as_bytes(), None).map(|_| ());
    assert!(matches!(text, Err(LoadError::Invalid { .. })), "{text:?}");
}

/// A backend whose every request waits, unanswered, until released or two minutes pass.
/// It says it can give a send up; watching, it also does, once its send is called off.
struct Hanging {
    entered: Arc<AtomicUsize>,
    released: Arc<AtomicBool>,
    watching: bool,
}

impl Hanging {
    fn hang(&self, cancel: Option<&SendCancel>) -> Result<Received, Error> {
        self.entered.fetch_add(1, Ordering::SeqCst);
        let until = Instant::now() + Duration::from_secs(120);
        while !self.released.load(Ordering::SeqCst)
            && !cancel.is_some_and(SendCancel::is_cancelled)
            && Instant::now() < until
        {
            thread::sleep(Duration::from_millis(10));
        }
        Err(Error::Failed("hung".to_owned()))
    }
}

impl NetBackend for Hanging {
    fn cancellable(&self) -> bool {
        true
    }

    fn send(&mut self, _request: &Outgoing) -> Result<Received, Error> {
        self.hang(None)
    }

    fn send_cancellable(
        &mut self,
        _request: &Outgoing,
        cancel: &SendCancel,
    ) -> Result<Received, Error> {
        self.hang(self.watching.then_some(cancel))
    }
}

/// Imports a job waits in, and whether each has begun waiting.
struct Waits {
    invoked: Arc<Withheld>,
    fetching: Arc<AtomicUsize>,
    /// Ends every fetch waiting in [`Hanging`].
    released: Arc<AtomicBool>,
    /// Whether [`Hanging`] stops once its send is called off.
    watching: bool,
    /// Made by the `sleep` program as it starts.
    marker: PathBuf,
    /// Where `sleep` is.
    dir: PathBuf,
}

impl Waits {
    fn new(name: &str) -> Self {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("kiln-plugin-jobs-{}-{name}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        let marker = dir.join("started");
        let shown = marker.display();
        support::script(
            &dir,
            "sleep",
            &format!("touch '{shown}'\nexec sleep 600"),
            &format!("type nul > \"{shown}\"\nping -n 601 127.0.0.1 >nul"),
        );
        Self {
            invoked: Arc::default(),
            fetching: Arc::default(),
            released: Arc::default(),
            watching: false,
            marker,
            dir,
        }
    }

    /// Whether job `name` waits in its import now.
    fn waiting(&self, name: &str) -> bool {
        match name {
            "invoke" => !self.invoked.0.lock().unwrap().is_empty(),
            "net" => self.fetching.load(Ordering::SeqCst) > 0,
            "process" => self.marker.exists(),
            _ => unreachable!("{name}"),
        }
    }
}

const WAITING: [&str; 3] = ["invoke", "net", "process"];

/// The fixture with what `invoke`, `net` and `process` need, their imports waiting on `waits`.
fn load_waiting(host: &JobHost, waits: &Waits) -> (Plugin, Receiver<String>) {
    load_waiting_under(host, waits, BackgroundLimits::default())
}

/// [`load_waiting`], its jobs under `background`.
fn load_waiting_under(
    host: &JobHost,
    waits: &Waits,
    background: BackgroundLimits,
) -> (Plugin, Receiver<String>) {
    let capabilities = ["jobs", "commands", "net:example.com", "process.spawn:sleep"];
    let granted = grants(&capabilities);
    let sleep = resolve_program("sleep", waits.dir.as_os_str()).unwrap();
    let record = GrantRecord {
        version: Some((1, 0, 0)),
        granted: granted.clone(),
        programs: [("sleep".to_owned(), sleep)].into_iter().collect(),
        ..GrantRecord::default()
    };
    // The plugin's own attachment, which its workers' commands slices share.
    let commands = CommandHost::new([], Arc::clone(&waits.invoked) as Arc<dyn Dispatcher>);
    let fetching = Arc::clone(&waits.fetching);
    let released = Arc::clone(&waits.released);
    let watching = waits.watching;
    let workers: jobs::WorkerHost = Arc::new(move || {
        let mut state = HostState::new(Box::new(|_, _| {}));
        let hanging = Hanging {
            entered: Arc::clone(&fetching),
            released: Arc::clone(&released),
            watching,
        };
        state.net = net::State::new(&granted, &Limits::default(), Box::new(hanging));
        state.process = process::State::new(record.clone(), &Limits::default());
        state
    });
    let jobs = jobs::State::new(host.clone(), background).with_workers(workers);
    load_jobs_on(jobs, &capabilities, &commands).unwrap()
}

#[test]
fn jobs_cancel_ends_imports_already_waiting() {
    for name in WAITING {
        let host = job_host();
        let waits = Waits::new(name);
        let (mut plugin, logged) = load_waiting(&host, &waits);
        let job = start(&mut plugin, name, "");
        let jobs = jobs_of(&plugin);
        assert!(
            wait_for(Duration::from_secs(10), || waits.waiting(name)),
            "{name}"
        );

        let cancelled = Instant::now();
        jobs.cancel(job).unwrap();
        assert!(
            wait_for(Duration::from_secs(10), || ended(&jobs.state(job))),
            "{name}"
        );
        // The import fails as cancelled at once, and the job returns: a wait the cancel didn't
        // reach would last until the job's 10-minute wall time.
        let took = cancelled.elapsed();
        assert!(
            took < CANCEL_GRACE + Duration::from_secs(3),
            "{name}: {took:?}"
        );
        assert_eq!(jobs.state(job), JobStatus::Cancelled, "{name}");
        assert_eq!(host.counts(), jobs::Counts::default(), "{name}");
        assert_eq!(plugin.deliver_jobs(), Ok(1));
        assert_eq!(finished_logs(&logged), ["c"], "{name}");
    }
}

#[test]
fn jobs_unload_ends_imports_already_waiting() {
    for name in WAITING {
        let host = job_host();
        let waits = Waits::new(name);
        let (mut plugin, _logged) = load_waiting(&host, &waits);
        start(&mut plugin, name, "");
        assert!(
            wait_for(Duration::from_secs(10), || waits.waiting(name)),
            "{name}"
        );

        let unloaded = Instant::now();
        drop(plugin);
        assert!(
            wait_for(Duration::from_secs(10), || host.counts()
                == jobs::Counts::default()),
            "{name}"
        );
        let took = unloaded.elapsed();
        assert!(
            took < CANCEL_GRACE + Duration::from_secs(3),
            "{name}: {took:?}"
        );
    }
}

#[test]
fn jobs_results_are_kept_before_post_return_reuses_their_memory() {
    let host = job_host();
    let (mut plugin, logged) = load(&host);
    let jobs = jobs_of(&plugin);
    // The fixture's post-return writes `X` over the string `run-job` returned.
    for (name, args, code) in [("echo", "hello", "o"), ("oops", "broken", "e")] {
        let job = start(&mut plugin, name, args);
        assert!(wait_for(Duration::from_secs(10), || ended(
            &jobs.state(job)
        )));
        assert_eq!(plugin.deliver_jobs(), Ok(1));
        assert_eq!(finished_logs(&logged), [code, args], "{name}");
    }
}

/// The fixture with `deep`'s function named past [`jobs::MAX_DIAGNOSTIC_BYTES`], so a trap's
/// backtrace names it many times over.
fn load_deep(host: &JobHost) -> (Plugin, Receiver<String>) {
    let text = std::fs::read_to_string(FIXTURE).unwrap();
    let long = format!("$deep{}", "x".repeat(jobs::MAX_DIAGNOSTIC_BYTES));
    let component = wat::parse_str(text.replace("$deep", &long)).unwrap();
    let jobs = jobs::State::new(host.clone(), BackgroundLimits::default());
    load_component(jobs, &["jobs"], &component, None).unwrap()
}

#[test]
fn jobs_a_trap_diagnostic_is_bounded() {
    let host = job_host();
    let (mut plugin, _logged) = load_deep(&host);
    let jobs = jobs_of(&plugin);
    let job = start(&mut plugin, "deep", "");
    assert!(wait_for(Duration::from_secs(10), || ended(
        &jobs.state(job)
    )));
    // Its room was reserved at the bound, and is charged as what was kept.
    let charged = jobs.undelivered_bytes();
    let Some((_, jobs::Ended::Finished(Err(Error::Failed(message))))) = jobs.take_ended() else {
        panic!("{:?}", jobs.state(job));
    };
    // Cut at the bound: the whole would name the function many times over.
    assert!(
        message.len() <= jobs::MAX_DIAGNOSTIC_BYTES,
        "{}",
        message.len()
    );
    assert!(message.len() > jobs::MAX_DIAGNOSTIC_BYTES - 8, "{message}");
    assert_eq!(charged, message.len());
}

#[test]
fn jobs_a_trap_diagnostic_past_a_full_quota_is_dropped() {
    let host = job_host();
    let (mut plugin, _logged) = load_deep(&host);
    let jobs = jobs_of(&plugin);
    let half = (jobs::UNDELIVERED_BYTES / 2).to_string();
    let full = [
        start(&mut plugin, "big", &half),
        start(&mut plugin, "big", &half),
    ];
    assert!(wait_for(Duration::from_secs(10), || {
        full.iter().all(|&job| ended(&jobs.state(job)))
    }));
    assert_eq!(jobs.undelivered_bytes(), jobs::UNDELIVERED_BYTES);
    let job = start(&mut plugin, "deep", "");
    assert!(wait_for(Duration::from_secs(10), || ended(
        &jobs.state(job)
    )));
    assert_eq!(jobs.undelivered_bytes(), jobs::UNDELIVERED_BYTES);
    let ended: Vec<_> = std::iter::from_fn(|| jobs.take_ended()).collect();
    assert_eq!(
        ended.last(),
        Some(&(job, jobs::Ended::failed(jobs::RESULT_DROPPED)))
    );
}

#[test]
fn jobs_wall_time_ends_a_fetch_already_waiting() {
    let host = job_host();
    let waits = Waits::new("net-wall");
    let wall = Duration::from_secs(1);
    let background = BackgroundLimits {
        wall,
        ..BackgroundLimits::default()
    };
    let (mut plugin, _logged) = load_waiting_under(&host, &waits, background);
    let started = Instant::now();
    let job = start(&mut plugin, "net", "");
    let jobs = jobs_of(&plugin);
    assert!(wait_for(Duration::from_secs(10), || waits.waiting("net")));
    // The backend never answers: the fetch stops waiting at the wall time.
    assert!(wait_for(Duration::from_secs(10), || ended(
        &jobs.state(job)
    )));
    let took = started.elapsed();
    assert!(took >= wall, "{took:?}");
    // Not the backend's two minutes.
    assert!(took < wall + Duration::from_secs(5), "{took:?}");
    assert_eq!(jobs.state(job), JobStatus::TimedOut);
    assert_eq!(host.counts(), jobs::Counts::default());
    waits.released.store(true, Ordering::SeqCst);
}

#[test]
fn jobs_sends_left_running_by_cancelled_jobs_are_capped() {
    let host = job_host();
    // A backend slow to give its sends up: the cap holds whatever the backend does.
    let waits = Waits::new("net-cap");
    let (mut plugin, _logged) = load_waiting(&host, &waits);
    let jobs = jobs_of(&plugin);
    // Each cancelled job leaves its send waiting on the backend; past the cap, a job's fetch
    // fails at once without reaching the backend.
    for _ in 0..net::MAX_JOB_SENDS + 4 {
        // Read before it starts: its send may reach the backend before `start` returns.
        let entered = waits.fetching.load(Ordering::SeqCst);
        let job = start(&mut plugin, "net", "");
        assert!(wait_for(Duration::from_secs(10), || {
            waits.fetching.load(Ordering::SeqCst) > entered || ended(&jobs.state(job))
        }));
        jobs.cancel(job).ok();
        assert!(wait_for(Duration::from_secs(10), || ended(
            &jobs.state(job)
        )));
        plugin.deliver_jobs().unwrap();
    }
    assert_eq!(waits.fetching.load(Ordering::SeqCst), net::MAX_JOB_SENDS);
    assert_eq!(net::job_sends().0, net::MAX_JOB_SENDS);
    assert_eq!(host.counts(), jobs::Counts::default());

    // Once the backend answers, they end, and their requests go with them.
    waits.released.store(true, Ordering::SeqCst);
    assert!(wait_for(Duration::from_secs(10), || net::job_sends() == (0, 0)));
    let job = start(&mut plugin, "net", "");
    assert!(wait_for(Duration::from_secs(10), || ended(
        &jobs.state(job)
    )));
    assert_eq!(
        waits.fetching.load(Ordering::SeqCst),
        net::MAX_JOB_SENDS + 1
    );
}

#[test]
fn jobs_cancel_calls_off_a_send_the_backend_lets_go_of() {
    let host = job_host();
    let mut waits = Waits::new("net-watching");
    waits.watching = true;
    let (mut plugin, _logged) = load_waiting(&host, &waits);
    let jobs = jobs_of(&plugin);
    // More cancelled sends than may be in flight: each is called off and gives its thread and
    // request back, so every one reaches the backend, and none is released by hand.
    for round in 1..=net::MAX_JOB_SENDS + 4 {
        let job = start(&mut plugin, "net", "");
        assert!(wait_for(Duration::from_secs(10), || {
            waits.fetching.load(Ordering::SeqCst) == round
        }));
        jobs.cancel(job).unwrap();
        assert!(wait_for(Duration::from_secs(10), || ended(
            &jobs.state(job)
        )));
        assert_eq!(jobs.state(job), JobStatus::Cancelled);
        assert!(wait_for(Duration::from_secs(10), || net::job_sends() == (0, 0)));
        plugin.deliver_jobs().unwrap();
    }
    // Unloading calls its send off too.
    start(&mut plugin, "net", "");
    assert!(wait_for(Duration::from_secs(10), || waits.waiting("net")
        && net::job_sends().0 == 1));
    drop(plugin);
    assert!(wait_for(Duration::from_secs(10), || net::job_sends() == (0, 0)));
    assert!(!waits.released.load(Ordering::SeqCst));
}

#[test]
fn jobs_invocations_cancelled_jobs_leave_with_the_app_are_capped() {
    let host = job_host();
    let waits = Waits::new("invoke-cap");
    let (mut plugin, _logged) = load_waiting(&host, &waits);
    let jobs = jobs_of(&plugin);
    let held = || waits.invoked.0.lock().unwrap().len();
    // Each job's worker store invokes on the plugin's own attachment, so what the app still
    // holds of cancelled jobs counts against one cap, job after job.
    for _ in 0..commands::REQUESTS_OUT_PER_PLUGIN + 4 {
        let before = held();
        let job = start(&mut plugin, "invoke", "");
        assert!(wait_for(Duration::from_secs(10), || {
            held() > before || ended(&jobs.state(job))
        }));
        jobs.cancel(job).ok();
        assert!(wait_for(Duration::from_secs(10), || ended(
            &jobs.state(job)
        )));
        plugin.deliver_jobs().unwrap();
    }
    assert_eq!(held(), commands::REQUESTS_OUT_PER_PLUGIN);
    // Once the app lets go of them, the next job's invocation reaches it again.
    waits.invoked.0.lock().unwrap().clear();
    let job = start(&mut plugin, "invoke", "");
    assert!(wait_for(Duration::from_secs(10), || held() == 1));
    jobs.cancel(job).unwrap();
}
