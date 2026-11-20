//! `remote-spike bench`: the measured numbers for the decision file.
//!
//! Spawns `remote-spike server` as a child, talks to it over its stdio
//! through `Delayed` (injected one-way latency) and prints Markdown tables.

use crate::client::{Conn, Event};
use crate::frame::{Body, Op, Status};
use crate::transport::{ChildStdio, Delayed};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub const BUILD: &str = concat!(env!("CARGO_PKG_VERSION"), "+spike");
pub const TREE_DIRS: (usize, usize) = (50, 20);
pub const TREE_FILES_PER_DIR: usize = 50;
const MIB: u64 = 1 << 20;

pub struct Opts {
    pub latencies_ms: Vec<u64>,
    pub window: u32,
    pub dir: PathBuf,
    /// Run the server through `ssh -T <host> <bin> server` instead of locally.
    pub ssh: Option<(String, String)>,
    pub quick: bool,
    /// Link for the window table: one-way latency and cap in Mbit/s.
    pub capped: (u64, f64),
}

fn ms(d: Duration) -> String {
    format!("{:.2} ms", d.as_secs_f64() * 1e3)
}

fn pct(v: &mut [Duration], p: f64) -> Duration {
    v.sort();
    v[((v.len() - 1) as f64 * p).round() as usize]
}

pub fn transport(o: &Opts, one_way: Duration) -> Delayed<ChildStdio> {
    capped_transport(o, one_way, None)
}

pub fn capped_transport(o: &Opts, one_way: Duration, mbps: Option<f64>) -> Delayed<ChildStdio> {
    let inner = match &o.ssh {
        Some((host, bin)) => ChildStdio {
            program: "ssh".into(),
            args: vec!["-T".into(), host.clone(), bin.clone(), "server".into()],
        },
        None => ChildStdio {
            program: std::env::current_exe()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            args: vec!["server".into()],
        },
    };
    Delayed {
        inner,
        one_way,
        bytes_per_sec: mbps.map(|m| m * 1e6 / 8.0),
    }
}

/// Marks a directory this module made, with this exact content.
const OWNER_FILE: &str = ".remote-spike-fixture";
const OWNER_TEXT: &str = "made by remote-spike fixture\n";

fn remove_if_present(r: io::Result<()>) -> io::Result<()> {
    match r {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// The 50k-file tree (50 × 20 directories of 50 empty files) and a 1 MiB file,
/// in `dir`. `dir` must be new, empty, or one this function made: anything
/// else is refused, and nothing is ever deleted outside the `tree` and
/// `one-mib.bin` it makes inside its own marked directory.
pub fn fixture(dir: &Path) -> io::Result<(PathBuf, PathBuf)> {
    let tree = dir.join("tree");
    let big = dir.join("one-mib.bin");
    let done = dir.join(".complete");
    let owner = dir.join(OWNER_FILE);
    let owned = fs::read_to_string(&owner).is_ok_and(|s| s == OWNER_TEXT);
    if !owned {
        let empty = match fs::read_dir(dir) {
            Ok(mut entries) => entries.next().is_none(),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                fs::create_dir_all(dir)?;
                true
            }
            Err(e) => return Err(e),
        };
        if !empty {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!(
                    "{} is not empty and wasn't made by `remote-spike fixture`; give a new or empty directory",
                    dir.display()
                ),
            ));
        }
        fs::write(&owner, OWNER_TEXT)?;
    }
    if !done.exists() {
        // An interrupted earlier run: redo only what this function makes.
        remove_if_present(fs::remove_dir_all(&tree))?;
        remove_if_present(fs::remove_file(&big))?;
        for a in 0..TREE_DIRS.0 {
            for b in 0..TREE_DIRS.1 {
                let d = tree.join(format!("d{a:02}")).join(format!("s{b:02}"));
                fs::create_dir_all(&d)?;
                for f in 0..TREE_FILES_PER_DIR {
                    fs::File::create(d.join(format!("file_{f:03}.rs")))?;
                }
            }
        }
        fs::write(
            &big,
            (0..MIB).map(|i| (i * 31 % 251) as u8).collect::<Vec<_>>(),
        )?;
        fs::write(&done, b"")?;
    }
    Ok((tree, big))
}

fn drain(s: &mut crate::client::Stream) -> io::Result<(usize, usize, Status)> {
    let (mut bytes, mut items) = (0, 0);
    while let Some(ev) = s.recv() {
        match ev {
            Event::Data(Body::Bytes(b), _) => {
                bytes += b.len();
                items += 1;
            }
            Event::Data(Body::Entries(e), n) => {
                bytes += n;
                items += e.len();
            }
            Event::End(st) => return Ok((bytes, items, st)),
        }
    }
    Err(io::Error::other("link dropped"))
}

pub fn run(o: &Opts) -> io::Result<()> {
    let (tree, big) = fixture(&o.dir)?;
    // Over ssh the fixture must already exist at the same path on the host.
    let (tree_s, big_s) = (
        tree.to_string_lossy().into_owned(),
        big.to_string_lossy().into_owned(),
    );
    let reps = if o.quick { 5 } else { 20 };

    // Local baseline: the same walk with no transport at all.
    let t = Instant::now();
    let mut n = 0;
    crate::server::walk(&tree, &mut |b| {
        n += b.len();
        true
    })?;
    println!(
        "Local walk, no transport: {n} entries in {}\n",
        ms(t.elapsed())
    );

    println!("Handshake with a different build:");
    match Conn::connect(&transport(o, Duration::ZERO), "0.0.0-other", o.window) {
        Err(e) if e.kind() == io::ErrorKind::ConnectionRefused => println!("  refused: {e}\n"),
        Err(e) => println!("  failed differently: {e}\n"),
        Ok(_) => println!("  ACCEPTED (bug)\n"),
    }

    println!(
        "| one-way latency | connect | ping p50 / p99 | echo 1 KiB p50 | echo under bulk p50 / p99 | bulk meanwhile: MiB/s, longest gap | cancel bulk: to End, bytes after | 1 MiB open p50 | walk: first batch, total, entries, wire MiB | cancel walk: entries after |"
    );
    println!("|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|");
    for &lat in &o.latencies_ms {
        let one_way = Duration::from_millis(lat);
        let t = Instant::now();
        let conn = Conn::connect(&transport(o, one_way), BUILD, o.window)?;
        let connect = t.elapsed();

        let mut pings: Vec<_> = (0..reps * 5)
            .map(|_| conn.ping())
            .collect::<io::Result<_>>()?;
        let ping50 = pct(&mut pings, 0.5);
        let ping99 = pct(&mut pings, 0.99);

        let mut echoes = Vec::new();
        for _ in 0..reps * 2 {
            let t = Instant::now();
            drain(&mut conn.request(1, &Op::Echo(vec![7; 1024]))?)?;
            echoes.push(t.elapsed());
        }
        let echo50 = pct(&mut echoes, 0.5);

        // Channel 2 streams without end for as long as channel 1 echoes.
        let mut ub = echo_under_bulk(&conn, reps * 2, Duration::from_millis(300))?;
        let t = Instant::now();
        let (st, after, _) = ub.bulk.cancel()?;
        let cancel_bulk = t.elapsed();
        assert_eq!(st, Some(Status::Cancelled));

        let mut opens = Vec::new();
        for _ in 0..reps {
            let t = Instant::now();
            let (bytes, _, st) = drain(&mut conn.request(
                1,
                &Op::ReadFile {
                    path: big_s.clone(),
                },
            )?)?;
            opens.push(t.elapsed());
            assert_eq!((bytes as u64, st), (MIB, Status::Ok));
        }

        let t = Instant::now();
        let mut walk = conn.request(
            1,
            &Op::Walk {
                root: tree_s.clone(),
            },
        )?;
        let (mut first, mut entries, mut wire) = (None, 0, 0);
        let st = loop {
            match walk.recv() {
                Some(Event::Data(Body::Entries(e), n)) => {
                    first.get_or_insert(t.elapsed());
                    entries += e.len();
                    wire += n;
                }
                Some(Event::End(st)) => break st,
                _ => return Err(io::Error::other("walk broke")),
            }
        };
        let walk_total = t.elapsed();
        assert_eq!(st, Status::Ok);

        let mut walk = conn.request(
            1,
            &Op::Walk {
                root: tree_s.clone(),
            },
        )?;
        let _ = walk.recv();
        let (st, _, walk_after) = walk.cancel()?;
        assert_eq!(st, Some(Status::Cancelled));

        println!(
            "| {lat} ms | {} | {} / {} | {} | {} / {} | {:.1}, {} | {}, {:.2} MiB | {} | {}, {}, {entries}, {:.2} | {walk_after} |",
            ms(connect),
            ms(ping50),
            ms(ping99),
            ms(echo50),
            ms(pct(&mut ub.echoes, 0.5)),
            ms(pct(&mut ub.echoes, 0.99)),
            ub.rate,
            ms(ub.max_gap),
            ms(cancel_bulk),
            after as f64 / MIB as f64,
            ms(pct(&mut opens, 0.5)),
            ms(first.unwrap()),
            ms(walk_total),
            wire as f64 / MIB as f64,
        );
    }

    println!("\nThroughput, one bulk stream on one channel (MiB/s):\n");
    let windows: [u32; 4] = [256 * 1024, 1 << 20, 1280 * 1024, 4 << 20];
    println!(
        "| one-way latency | {} |",
        windows
            .iter()
            .map(|w| format!("window {} KiB", w / 1024))
            .collect::<Vec<_>>()
            .join(" | ")
    );
    println!("|---:|{}", "---:|".repeat(windows.len()));
    for &lat in &o.latencies_ms {
        let mut cells = Vec::new();
        for &w in &windows {
            let conn = Conn::connect(&transport(o, Duration::from_millis(lat)), BUILD, w)?;
            let rate = stream_rate(&conn, if o.quick { 0.7 } else { 1.5 })?;
            cells.push(format!("{rate:.0}"));
        }
        println!("| {lat} ms | {} |", cells.join(" | "));
    }

    if o.ssh.is_some() {
        // A cap on top of a real link measures nothing useful.
        return Ok(());
    }
    let (lat, mbps) = o.capped;
    println!(
        "\nBulk beside interactive on a capped link ({lat} ms one-way, {mbps} Mbit/s): one channel streams while another echoes 1 KiB.\n"
    );
    println!(
        "| window | bulk MiB/s while echoing | longest bulk gap | echo p50 / p99 under bulk | idle echo p50 |"
    );
    println!("|---:|---:|---:|---:|---:|");
    for &w in &windows {
        let conn = Conn::connect(
            &capped_transport(o, Duration::from_millis(lat), Some(mbps)),
            BUILD,
            w,
        )?;
        let mut idle = Vec::new();
        for _ in 0..reps {
            let t = Instant::now();
            drain(&mut conn.request(1, &Op::Echo(vec![7; 1024]))?)?;
            idle.push(t.elapsed());
        }
        // Past the slow start, so the samples see the stream's steady state.
        let mut ub = echo_under_bulk(&conn, reps, Duration::from_millis(700))?;
        ub.bulk.cancel()?;
        println!(
            "| {} KiB | {:.1} | {} | {} / {} | {} |",
            w / 1024,
            ub.rate,
            ms(ub.max_gap),
            ms(pct(&mut ub.echoes, 0.5)),
            ms(pct(&mut ub.echoes, 0.99)),
            ms(pct(&mut idle, 0.5)),
        );
    }
    Ok(())
}

/// Streams one unbounded bulk request on channel 2 for `secs` after a warm-up,
/// then cancels it. Returns its rate in MiB/s.
fn stream_rate(conn: &Conn, secs: f64) -> io::Result<f64> {
    let mut bulk = conn.request(2, &Op::Bulk { bytes: u64::MAX })?;
    let warm = Instant::now() + Duration::from_millis(300);
    while Instant::now() < warm {
        bulk.recv();
    }
    let (mut got, t) = (0usize, Instant::now());
    while t.elapsed().as_secs_f64() < secs {
        if let Some(Event::Data(_, n)) = bulk.recv() {
            got += n;
        }
    }
    let rate = got as f64 / MIB as f64 / t.elapsed().as_secs_f64();
    let (st, _, _) = bulk.cancel()?;
    assert_eq!(st, Some(Status::Cancelled));
    Ok(rate)
}

/// Echo samples taken while a bulk stream ran for the whole sampling window.
struct UnderBulk {
    echoes: Vec<Duration>,
    /// The bulk stream's rate while the echoes ran, MiB/s.
    rate: f64,
    /// The longest time in that window with no bulk data arriving.
    max_gap: Duration,
    /// Still streaming: the caller cancels it.
    bulk: crate::client::Stream,
}

/// Starts an unbounded bulk stream on channel 2, lets it run for `warm`, then
/// times `n` 1 KiB echoes on channel 1. A pump thread consumes the stream
/// until the last echo is done, so every sample sees live contention, and
/// it records arrivals so the stream's progress during the samples shows.
fn echo_under_bulk(conn: &Conn, n: usize, warm: Duration) -> io::Result<UnderBulk> {
    let mut bulk = conn.request(2, &Op::Bulk { bytes: u64::MAX })?;
    let stop = Arc::new(AtomicBool::new(false));
    let pump = {
        let stop = stop.clone();
        std::thread::spawn(move || {
            let mut arrivals = Vec::new();
            while !stop.load(Ordering::Acquire) {
                match bulk.recv() {
                    Some(Event::Data(_, n)) => arrivals.push((Instant::now(), n)),
                    _ => break,
                }
            }
            (bulk, arrivals)
        })
    };
    std::thread::sleep(warm);
    let t0 = Instant::now();
    let mut echoes = Vec::new();
    for _ in 0..n {
        let t = Instant::now();
        drain(&mut conn.request(1, &Op::Echo(vec![7; 1024]))?)?;
        echoes.push(t.elapsed());
    }
    let t1 = Instant::now();
    stop.store(true, Ordering::Release);
    let (bulk, arrivals) = pump.join().unwrap();
    let (mut bytes, mut prev, mut max_gap) = (0usize, t0, Duration::ZERO);
    for &(t, n) in arrivals.iter().filter(|(t, _)| (t0..=t1).contains(t)) {
        bytes += n;
        max_gap = max_gap.max(t - prev);
        prev = t;
    }
    max_gap = max_gap.max(t1 - prev);
    Ok(UnderBulk {
        echoes,
        rate: bytes as f64 / MIB as f64 / (t1 - t0).as_secs_f64(),
        max_gap,
        bulk,
    })
}
