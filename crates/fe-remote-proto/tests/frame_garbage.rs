//! `decode_all` on mutated and truncated frames returns, an error at worst,
//! and never panics (D § 2.1). fuzz/fuzz_targets/remote_frames.rs drives the
//! same function with libFuzzer.

use fe_remote_proto::{
    DirEntry, DiskStamp, EntryKind, FileMatches, FsData, FsOp, Hello, Kind, LineMatch, Location,
    PathBytes, SearchData, Time, WriteExpect, chan, decode_all, encode,
};

/// A frame of each shape with variable-length fields to corrupt.
fn seeds() -> Vec<Vec<u8>> {
    let stamp = DiskStamp {
        len: 10,
        modified: Time { secs: 1, nanos: 2 },
        dev: 3,
        ino: 4,
    };
    let hello = Hello {
        protocol: 1,
        build: "0.2.5+0123456789ab".into(),
        window: 1_310_720,
        resume: None,
        location: Location {
            destination: "neon".into(),
            path: "/home/me/src/app".into(),
        },
    };
    let entries = FsData::Entries(vec![DirEntry {
        name: PathBytes(b"main\xff.rs".to_vec()),
        kind: EntryKind::File,
        ignored: false,
        id: None,
    }]);
    let write = FsOp::WriteAtomic {
        path: "/src/main.rs".into(),
        len: 3,
        expect: WriteExpect::Matches(stamp),
    };
    let matches = SearchData::Matches(vec![FileMatches {
        path: "/src/lib.rs".into(),
        rel: "src/lib.rs".into(),
        lines: vec![LineMatch {
            line_number: 3,
            line_offset: 40,
            ranges: vec![2..5, 7..8],
            preview: "fn foo()".into(),
            preview_start: 0,
        }],
    }]);
    vec![
        encode(Kind::Hello, chan::CONTROL, 0, &hello).unwrap(),
        encode(Kind::Data, chan::FS, 1, &entries).unwrap(),
        encode(Kind::Request, chan::FS, 2, &write).unwrap(),
        encode(Kind::Data, chan::SEARCH, 3, &matches).unwrap(),
        [
            encode(Kind::Data, chan::FS, 1, &entries).unwrap(),
            encode(Kind::Ping, chan::CONTROL, 9, &()).unwrap(),
        ]
        .concat(),
    ]
}

#[test]
fn mutated_and_truncated_frames_never_panic() {
    let seeds = seeds();
    let single: Vec<bool> = seeds
        .iter()
        .map(|s| decode_all(s).unwrap().len() == 1)
        .collect();
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let mut errors = 0;
    for i in 0..200_000 {
        let which = i % seeds.len();
        let mut frame = seeds[which].clone();
        let mutations = next() % 4;
        for _ in 0..mutations {
            let at = (next() as usize) % frame.len();
            frame[at] = next() as u8;
        }
        let cut = (next() as usize) % (frame.len() + 1);
        let decoded = decode_all(&frame[..cut]);
        // A frame cut short, unmutated, is always refused.
        if single[which] && mutations == 0 && cut > 0 && cut < frame.len() {
            assert!(decoded.is_err(), "seed {which} cut at {cut}");
        }
        errors += usize::from(decoded.is_err());
    }
    assert!(errors > 100_000, "only {errors} of 200,000 were refused");
}

#[test]
fn the_seeds_decode_whole() {
    for frame in seeds() {
        assert!(!decode_all(&frame).unwrap().is_empty());
    }
}
