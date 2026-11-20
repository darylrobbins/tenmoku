//! `remote-spike codec`: the encoding comparison behind the protocol's choice.
//!
//! Encodes and decodes three representative bodies with postcard, bincode 2
//! (serde, standard config), serde_json and protobuf (prost), and prints
//! the size and the time of one encode plus one decode, each from an
//! already-built value of the codec's own type. Converting serde types to
//! prost's is timed on a row of its own.

use crate::frame::{Body, CHUNK, Entry, Op};
use std::hint::black_box;
use std::time::{Duration, Instant};

#[derive(Clone, PartialEq, prost::Message)]
struct PEntry {
    #[prost(string, tag = "1")]
    path: String,
    #[prost(bool, tag = "2")]
    dir: bool,
    #[prost(uint64, tag = "3")]
    size: u64,
    #[prost(sint64, tag = "4")]
    mtime_ns: i64,
}

#[derive(Clone, PartialEq, prost::Message)]
struct PBody {
    #[prost(bytes = "vec", tag = "1")]
    bytes: Vec<u8>,
    #[prost(message, repeated, tag = "2")]
    entries: Vec<PEntry>,
}

#[derive(Clone, PartialEq, prost::Message)]
struct POp {
    #[prost(string, tag = "1")]
    read_file: String,
}

pub fn sample_entries(n: usize) -> Vec<Entry> {
    (0..n)
        .map(|i| Entry {
            path: format!(
                "d{:02}/s{:02}/file_{:03}.rs",
                i / 1000 % 50,
                i / 50 % 20,
                i % 50
            ),
            dir: false,
            size: 1_000 + (i as u64 * 37) % 9_000,
            mtime_ns: 1_763_600_000_000_000_000 + i as i64 * 1_000_003,
        })
        .collect()
}

fn to_prost(b: &Body) -> PBody {
    match b {
        Body::Bytes(v) => PBody {
            bytes: v.clone(),
            entries: vec![],
        },
        Body::Entries(es) => PBody {
            bytes: vec![],
            entries: es
                .iter()
                .map(|e| PEntry {
                    path: e.path.clone(),
                    dir: e.dir,
                    size: e.size,
                    mtime_ns: e.mtime_ns,
                })
                .collect(),
        },
    }
}

/// Median time of one call of `f`, over enough calls to fill ~150 ms.
fn time(mut f: impl FnMut()) -> Duration {
    let t = Instant::now();
    let mut probe = 0u32;
    while t.elapsed() < Duration::from_millis(20) {
        f();
        probe += 1;
    }
    let per_batch = (probe / 4).max(1);
    let mut samples: Vec<Duration> = (0..15)
        .map(|_| {
            let t = Instant::now();
            for _ in 0..per_batch {
                f();
            }
            t.elapsed() / per_batch
        })
        .collect();
    samples.sort();
    samples[samples.len() / 2]
}

struct Row {
    name: &'static str,
    bytes: usize,
    time: Duration,
}

fn measure_body(b: &Body) -> Vec<Row> {
    let bc = bincode::config::standard();
    let p = to_prost(b);
    vec![
        Row {
            name: "postcard",
            bytes: postcard::to_allocvec(b).unwrap().len(),
            time: time(|| {
                let v = postcard::to_allocvec(black_box(b)).unwrap();
                black_box(postcard::from_bytes::<Body>(&v).unwrap());
            }),
        },
        Row {
            name: "bincode 2",
            bytes: bincode::serde::encode_to_vec(b, bc).unwrap().len(),
            time: time(|| {
                let v = bincode::serde::encode_to_vec(black_box(b), bc).unwrap();
                black_box(bincode::serde::decode_from_slice::<Body, _>(&v, bc).unwrap());
            }),
        },
        Row {
            name: "serde_json",
            bytes: serde_json::to_vec(b).unwrap().len(),
            time: time(|| {
                let v = serde_json::to_vec(black_box(b)).unwrap();
                black_box(serde_json::from_slice::<Body>(&v).unwrap());
            }),
        },
        Row {
            name: "protobuf (prost)",
            bytes: prost::Message::encode_to_vec(&p).len(),
            time: time(|| {
                let v = prost::Message::encode_to_vec(black_box(&p));
                black_box(<PBody as prost::Message>::decode(v.as_slice()).unwrap());
            }),
        },
        Row {
            // Only if the protocol kept serde types and converted at the
            // edge; prost types used throughout would not pay this.
            name: "(prost: building its struct from the serde one)",
            bytes: 0,
            time: time(|| {
                black_box(to_prost(black_box(b)));
            }),
        },
    ]
}

fn measure_op(op: &Op) -> Vec<Row> {
    let bc = bincode::config::standard();
    let Op::ReadFile { path } = op else {
        unreachable!()
    };
    let p = POp {
        read_file: path.clone(),
    };
    vec![
        Row {
            name: "postcard",
            bytes: postcard::to_allocvec(op).unwrap().len(),
            time: time(|| {
                let v = postcard::to_allocvec(black_box(op)).unwrap();
                black_box(postcard::from_bytes::<Op>(&v).unwrap());
            }),
        },
        Row {
            name: "bincode 2",
            bytes: bincode::serde::encode_to_vec(op, bc).unwrap().len(),
            time: time(|| {
                let v = bincode::serde::encode_to_vec(black_box(op), bc).unwrap();
                black_box(bincode::serde::decode_from_slice::<Op, _>(&v, bc).unwrap());
            }),
        },
        Row {
            name: "serde_json",
            bytes: serde_json::to_vec(op).unwrap().len(),
            time: time(|| {
                let v = serde_json::to_vec(black_box(op)).unwrap();
                black_box(serde_json::from_slice::<Op>(&v).unwrap());
            }),
        },
        Row {
            name: "protobuf (prost)",
            bytes: prost::Message::encode_to_vec(&p).len(),
            time: time(|| {
                let v = prost::Message::encode_to_vec(black_box(&p));
                black_box(<POp as prost::Message>::decode(v.as_slice()).unwrap());
            }),
        },
    ]
}

pub fn run() {
    let cases: Vec<(String, Vec<Row>)> = vec![
        (
            "walk batch, 512 entries".into(),
            measure_body(&Body::Entries(sample_entries(512))),
        ),
        (
            format!("file chunk, {} KiB", CHUNK / 1024),
            measure_body(&Body::Bytes(
                (0..CHUNK).map(|i| (i * 31 % 251) as u8).collect(),
            )),
        ),
        (
            "request, ReadFile".into(),
            measure_op(&Op::ReadFile {
                path: "/home/dev/src/project/crates/core/src/lib.rs".into(),
            }),
        ),
    ];
    println!("| body | encoding | bytes | encode + decode |");
    println!("|---|---|---:|---:|");
    for (case, rows) in &cases {
        for r in rows {
            println!(
                "| {case} | {} | {} | {:.1} µs |",
                r.name,
                if r.bytes == 0 {
                    "—".into()
                } else {
                    r.bytes.to_string()
                },
                r.time.as_secs_f64() * 1e6
            );
        }
    }
}
