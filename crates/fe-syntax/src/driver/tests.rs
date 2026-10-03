use std::{
    cell::Cell,
    ops::Range,
    rc::{Rc, Weak},
    time::{Duration, Instant},
};

use fe_buffer::{Buffer, Origin};

use crate::*;

/// Runs parse jobs only when a test says so. Each task is an `Rc` the driver
/// holds; a job whose task was dropped is cancelled.
#[derive(Default)]
struct ManualSpawner {
    jobs: Vec<(ParseJob, Weak<()>)>,
    spawned: usize,
}

impl Spawner for ManualSpawner {
    type Task = Rc<()>;

    fn spawn(&mut self, job: ParseJob) -> Rc<()> {
        let task = Rc::new(());
        self.jobs.push((job, Rc::downgrade(&task)));
        self.spawned += 1;
        task
    }
}

impl ManualSpawner {
    /// Jobs spawned and neither run nor cancelled.
    fn pending(&mut self) -> usize {
        self.jobs.retain(|(_, task)| task.strong_count() > 0);
        self.jobs.len()
    }

    /// Runs the oldest pending job.
    fn run(&mut self) -> Parsed {
        assert!(self.pending() > 0, "no job to run");
        self.jobs.remove(0).0.run()
    }
}

const BUDGET: Duration = Duration::from_millis(1);
const TICK: Duration = Duration::from_micros(100);

fn rust() -> &'static Language {
    language_by_name("rust").unwrap()
}

/// A driver whose clock moves on by [`TICK`] at each read, and how far that
/// clock has moved.
fn driver(buffer: &Buffer) -> (SyntaxDriver<Rc<()>>, Rc<Cell<Duration>>) {
    let start = Instant::now();
    let elapsed = Rc::new(Cell::new(Duration::ZERO));
    let clock = elapsed.clone();
    let driver =
        SyntaxDriver::new(Syntax::new(rust(), buffer.current()), BUDGET).with_clock(move || {
            clock.set(clock.get() + TICK);
            start + clock.get()
        });
    (driver, elapsed)
}

/// Too much Rust to parse within [`BUDGET`] of [`TICK`]s.
fn big_rust() -> Buffer {
    Buffer::new(&"fn f() {}\n".repeat(20_000))
}

fn first_keyword(driver: &SyntaxDriver<Rc<()>>) -> Option<Range<usize>> {
    driver
        .syntax()
        .highlights(0..40)
        .into_iter()
        .find(|span| span.highlight.name() == "keyword")
        .map(|span| span.range)
}

#[test]
fn quick_parses_stay_on_the_calling_thread() {
    let buffer = Buffer::new("fn f() {}\n");
    let (mut driver, _) = driver(&buffer);
    let mut spawner = ManualSpawner::default();
    driver.sync(buffer.current(), &mut spawner);
    assert!(!driver.syntax().needs_parse());
    assert_eq!(spawner.spawned, 0);
}

#[test]
fn sync_never_parses_past_its_budget() {
    let buffer = big_rust();
    let (mut driver, elapsed) = driver(&buffer);
    let mut spawner = ManualSpawner::default();
    driver.sync(buffer.current(), &mut spawner);
    // One read to start, then a read per check until one finds the budget
    // spent.
    assert!(elapsed.get() > BUDGET);
    assert!(elapsed.get() <= TICK + BUDGET + TICK, "{:?}", elapsed.get());
    assert!(driver.syntax().needs_parse());
    assert_eq!(spawner.pending(), 1);
}

#[test]
fn one_background_parse_at_a_time() {
    let mut buffer = big_rust();
    let (mut driver, _) = driver(&buffer);
    let mut spawner = ManualSpawner::default();
    for _ in 0..3 {
        driver.sync(buffer.current(), &mut spawner);
        buffer.edit([(0..0, "x")], Origin::User);
    }
    assert_eq!(spawner.spawned, 1);
    assert!(driver.is_parsing());
}

#[test]
fn a_late_result_is_edited_forward_and_used() {
    let mut buffer = big_rust();
    let (mut driver, _) = driver(&buffer);
    let mut spawner = ManualSpawner::default();
    driver.sync(buffer.current(), &mut spawner);
    assert_eq!(first_keyword(&driver), None);
    buffer.edit([(0..0, "// ")], Origin::User);
    driver.sync(buffer.current(), &mut spawner);
    driver.finish(spawner.run());
    // The tree from before "// " was typed, moved past it.
    assert_eq!(first_keyword(&driver), Some(3..5));
    assert!(driver.syntax().needs_parse());
    assert!(!driver.is_parsing());
}

#[test]
fn typing_during_a_parse_starts_one_follow_up() {
    let mut buffer = big_rust();
    let (mut driver, _) = driver(&buffer);
    let mut spawner = ManualSpawner::default();
    driver.sync(buffer.current(), &mut spawner);
    // Each line becomes the body of the function before it, so little of the
    // old tree is reusable and the reparse needs the background too.
    for _ in 0..3 {
        buffer.edit([(0..0, "fn g() {")], Origin::User);
        driver.sync(buffer.current(), &mut spawner);
    }
    driver.finish(spawner.run());
    driver.sync(buffer.current(), &mut spawner);
    driver.sync(buffer.current(), &mut spawner);
    assert_eq!(spawner.spawned, 2);
    driver.finish(spawner.run());
    assert!(!driver.syntax().needs_parse());
    driver.sync(buffer.current(), &mut spawner);
    assert_eq!(spawner.spawned, 2);
}

#[test]
fn dropping_the_driver_or_switching_buffers_cancels_its_parse() {
    let buffer = big_rust();
    let (mut driver, _) = driver(&buffer);
    let mut spawner = ManualSpawner::default();
    driver.sync(buffer.current(), &mut spawner);
    let other = big_rust();
    driver.sync(other.current(), &mut spawner);
    // The first buffer's parse is cancelled and the second's started.
    assert_eq!((spawner.spawned, spawner.pending()), (2, 1));
    drop(driver);
    assert_eq!(spawner.pending(), 0);
}
