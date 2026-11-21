//! Every body type round-trips through a frame, and `decode_all` accepts
//! the frame (D § 2.1, § 2.3).

use fe_remote_proto::{
    ControlOp, Credit, DirEntry, DiskStamp, EntryId, EntryKind, FileMatches, FsChange, FsData,
    FsOp, Hello, IoError, IoKind, Kind, LineMatch, Location, OpenTarget, Opened, PathBytes,
    PathFilter, Refused, Resume, SearchData, SearchOp, SearchOptions, SearchQuery, SearchSummary,
    SessionToken, Stat, Status, StreamOffset, StreamResume, Time, WatchOp, Welcome, WriteExpect,
    body, chan, decode_all, decode_frame, encode, read_frame,
};
use proptest::collection::vec;
use proptest::option;
use proptest::prelude::*;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fmt::Debug;
use std::time::{Duration, SystemTime};

/// Encodes `value` as a `kind` frame on `chan`, then decodes it back both
/// from a slice and from a reader.
fn round_trip<T>(kind: Kind, chan: u16, value: &T)
where
    T: Serialize + DeserializeOwned + PartialEq + Debug,
{
    let id = 0x8000_0007;
    let frame = encode(kind, chan, id, value).unwrap();
    let headers = decode_all(&frame).unwrap();
    assert_eq!(headers.len(), 1);
    assert_eq!(
        (headers[0].kind, headers[0].chan, headers[0].id),
        (kind, chan, id)
    );

    let (header, bytes, rest) = decode_frame(&frame).unwrap();
    assert!(rest.is_empty());
    assert_eq!(&body::<T>(&header, bytes).unwrap(), value);

    let (header, bytes) = read_frame(&mut frame.as_slice()).unwrap().unwrap();
    assert_eq!(&body::<T>(&header, &bytes).unwrap(), value);
}

fn path() -> impl Strategy<Value = PathBytes> {
    vec(any::<u8>(), 0..48).prop_map(PathBytes)
}

fn text() -> impl Strategy<Value = String> {
    "\\PC{0,24}"
}

fn time() -> impl Strategy<Value = Time> {
    (any::<i64>(), 0u32..1_000_000_000).prop_map(|(secs, nanos)| Time { secs, nanos })
}

fn stamp() -> impl Strategy<Value = DiskStamp> {
    (any::<u64>(), time(), any::<u64>(), any::<u64>()).prop_map(|(len, modified, dev, ino)| {
        DiskStamp {
            len,
            modified,
            dev,
            ino,
        }
    })
}

fn token() -> impl Strategy<Value = SessionToken> {
    any::<[u8; 16]>().prop_map(SessionToken)
}

fn offset() -> impl Strategy<Value = StreamOffset> {
    (any::<u16>(), any::<u64>()).prop_map(|(chan, recv_next)| StreamOffset { chan, recv_next })
}

fn hello() -> impl Strategy<Value = Hello> {
    let resume =
        (token(), vec(offset(), 0..4)).prop_map(|(session, streams)| Resume { session, streams });
    let location = (text(), path()).prop_map(|(destination, path)| Location { destination, path });
    (
        any::<u32>(),
        text(),
        any::<u32>(),
        option::of(resume),
        location,
    )
        .prop_map(|(protocol, build, window, resume, location)| Hello {
            protocol,
            build,
            window,
            resume,
            location,
        })
}

fn welcome() -> impl Strategy<Value = Welcome> {
    let stream = prop_oneof![
        offset().prop_map(StreamResume::From),
        (any::<u16>(), any::<u64>()).prop_map(|(chan, lost)| StreamResume::Gap { chan, lost }),
    ];
    (
        any::<u32>(),
        text(),
        any::<u32>(),
        token(),
        vec(stream, 0..4),
    )
        .prop_map(|(protocol, build, window, session, streams)| Welcome {
            protocol,
            build,
            window,
            session,
            streams,
        })
}

fn control_op() -> impl Strategy<Value = ControlOp> {
    (text(), any::<u16>()).prop_map(|(host, port)| ControlOp::Open(OpenTarget::Tcp { host, port }))
}

fn io_kind() -> impl Strategy<Value = IoKind> {
    prop::sample::select(vec![
        IoKind::NotFound,
        IoKind::PermissionDenied,
        IoKind::AlreadyExists,
        IoKind::NotADirectory,
        IoKind::IsADirectory,
        IoKind::DirectoryNotEmpty,
        IoKind::InvalidInput,
        IoKind::InvalidData,
        IoKind::Unsupported,
        IoKind::Other,
    ])
}

fn status() -> impl Strategy<Value = Status> {
    prop_oneof![
        Just(Status::Ok),
        Just(Status::Cancelled),
        Just(Status::LinkLost),
        (io_kind(), text()).prop_map(|(kind, message)| Status::Error(IoError { kind, message })),
    ]
}

fn expect() -> impl Strategy<Value = WriteExpect> {
    prop_oneof![
        Just(WriteExpect::Overwrite),
        Just(WriteExpect::AsFound),
        Just(WriteExpect::Absent),
        stamp().prop_map(WriteExpect::Matches),
    ]
}

fn entry_kind() -> impl Strategy<Value = EntryKind> {
    prop::sample::select(vec![EntryKind::File, EntryKind::Dir, EntryKind::Symlink])
}

fn fs_op() -> impl Strategy<Value = FsOp> {
    prop_oneof![
        path().prop_map(|path| FsOp::Read { path }),
        (path(), any::<u64>(), expect()).prop_map(|(path, len, expect)| FsOp::WriteAtomic {
            path,
            len,
            expect
        }),
        (path(), any::<bool>()).prop_map(|(path, follow)| FsOp::Stat { path, follow }),
        (path(), any::<bool>()).prop_map(|(path, ignored)| FsOp::ListDir { path, ignored }),
        (path(), path(), any::<bool>()).prop_map(|(parent, name, folder)| FsOp::Create {
            parent,
            name,
            folder
        }),
        (path(), path()).prop_map(|(from, new_name)| FsOp::Rename { from, new_name }),
        path().prop_map(|path| FsOp::Remove { path }),
        vec(path(), 0..4).prop_map(|paths| FsOp::Trash { paths }),
        path().prop_map(|path| FsOp::Canonicalize { path }),
    ]
}

fn fs_data() -> impl Strategy<Value = FsData> {
    let entry = (
        path(),
        entry_kind(),
        any::<bool>(),
        option::of((any::<u64>(), any::<u64>()).prop_map(|(dev, ino)| EntryId { dev, ino })),
    )
        .prop_map(|(name, kind, ignored, id)| DirEntry {
            name,
            kind,
            ignored,
            id,
        });
    let stat = (entry_kind(), stamp(), any::<bool>()).prop_map(|(kind, stamp, readonly)| Stat {
        kind,
        stamp,
        readonly,
    });
    prop_oneof![
        vec(any::<u8>(), 0..256).prop_map(FsData::Bytes),
        (stamp(), any::<u64>()).prop_map(|(stamp, len)| FsData::File { stamp, len }),
        stamp().prop_map(FsData::Written),
        option::of(stamp()).prop_map(|current| FsData::Conflict { current }),
        stat.prop_map(FsData::Stat),
        vec(entry, 0..8).prop_map(FsData::Entries),
        path().prop_map(FsData::Path),
    ]
}

fn fs_change() -> impl Strategy<Value = FsChange> {
    prop_oneof![path().prop_map(FsChange::Path), Just(FsChange::Rescan)]
}

fn filter() -> impl Strategy<Value = PathFilter> {
    (vec(text(), 0..3), vec(text(), 0..3))
        .prop_map(|(include, exclude)| PathFilter { include, exclude })
}

fn search_op() -> impl Strategy<Value = SearchOp> {
    let query = (text(), any::<bool>(), any::<bool>(), any::<bool>()).prop_map(
        |(pattern, regex, case_sensitive, whole_word)| SearchQuery {
            pattern,
            regex,
            case_sensitive,
            whole_word,
        },
    );
    let options = (any::<u64>(), any::<u32>()).prop_map(|(max_matches, threads)| SearchOptions {
        max_matches,
        threads,
    });
    prop_oneof![
        (path(), query, options, filter(), option::of(path())).prop_map(
            |(root, query, options, filter, excluded)| SearchOp::Search {
                root,
                query,
                options,
                filter,
                excluded,
            }
        ),
        (path(), filter()).prop_map(|(root, filter)| SearchOp::Files { root, filter }),
    ]
}

fn search_data() -> impl Strategy<Value = SearchData> {
    let line = (
        any::<u64>(),
        any::<u64>(),
        vec((any::<u64>(), any::<u64>()).prop_map(|(a, b)| a..b), 0..4),
        text(),
        any::<u64>(),
    )
        .prop_map(
            |(line_number, line_offset, ranges, preview, preview_start)| LineMatch {
                line_number,
                line_offset,
                ranges,
                preview,
                preview_start,
            },
        );
    let file = (path(), text(), vec(line, 0..4)).prop_map(|(path, rel, lines)| FileMatches {
        path,
        rel,
        lines,
    });
    let summary = (
        any::<[u64; 4]>(),
        vec((path(), text()), 0..3),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(
            |(
                [files_searched, files_matched, matches, binary_skipped],
                errors,
                truncated,
                cancelled,
            )| {
                SearchSummary {
                    files_searched,
                    files_matched,
                    matches,
                    binary_skipped,
                    errors,
                    truncated,
                    cancelled,
                }
            },
        );
    prop_oneof![
        vec(file, 0..4).prop_map(SearchData::Matches),
        summary.prop_map(SearchData::Summary),
        vec(path(), 0..8).prop_map(SearchData::Files),
    ]
}

proptest! {
    #[test]
    fn hello_round_trips(v in hello()) {
        round_trip(Kind::Hello, chan::CONTROL, &v);
    }

    #[test]
    fn welcome_round_trips(v in welcome()) {
        round_trip(Kind::Welcome, chan::CONTROL, &v);
    }

    #[test]
    fn refused_round_trips(reason in text()) {
        round_trip(Kind::Refused, chan::CONTROL, &Refused { reason });
    }

    #[test]
    fn a_control_request_round_trips(v in control_op()) {
        round_trip(Kind::Request, chan::CONTROL, &v);
    }

    #[test]
    fn opened_round_trips(chan in any::<u16>()) {
        round_trip(Kind::Data, chan::CONTROL, &Opened { chan });
    }

    #[test]
    fn credit_round_trips(bytes in any::<u32>(), on in any::<u16>()) {
        round_trip(Kind::Credit, on, &Credit { bytes });
    }

    #[test]
    fn end_status_round_trips(v in status(), on in any::<u16>()) {
        round_trip(Kind::End, on, &v);
    }

    #[test]
    fn an_fs_request_round_trips(v in fs_op()) {
        round_trip(Kind::Request, chan::FS, &v);
    }

    #[test]
    fn fs_data_round_trips(v in fs_data()) {
        round_trip(Kind::Data, chan::FS, &v);
    }

    #[test]
    fn a_watch_request_round_trips(root in path()) {
        round_trip(Kind::Request, chan::WATCH, &WatchOp::Watch { root });
    }

    #[test]
    fn a_watch_batch_round_trips(v in vec(fs_change(), 0..8)) {
        round_trip(Kind::Data, chan::WATCH, &v);
    }

    #[test]
    fn a_search_request_round_trips(v in search_op()) {
        round_trip(Kind::Request, chan::SEARCH, &v);
    }

    #[test]
    fn search_data_round_trips(v in search_data()) {
        round_trip(Kind::Data, chan::SEARCH, &v);
    }

    #[test]
    fn a_time_round_trips_through_system_time(secs in -(1i64 << 40)..(1i64 << 40), nanos in 0u32..1_000_000_000) {
        let time = Time { secs, nanos };
        let system = time.to_system_time().unwrap();
        prop_assert_eq!(Time::from(system), time);
    }
}

#[test]
fn ping_pong_and_cancel_have_empty_bodies() {
    for kind in [Kind::Ping, Kind::Pong] {
        round_trip(kind, chan::CONTROL, &());
    }
    round_trip(Kind::Cancel, chan::FS, &());
    assert_eq!(encode(Kind::Ping, chan::CONTROL, 1, &()).unwrap().len(), 12);
}

#[test]
fn a_path_that_is_not_utf8_round_trips() {
    let path = PathBytes(b"/home/me/caf\xe9/\xff\xfe.rs".to_vec());
    assert!(std::str::from_utf8(path.as_bytes()).is_err());
    round_trip(Kind::Request, chan::FS, &FsOp::Read { path: path.clone() });
    round_trip(Kind::Data, chan::WATCH, &vec![FsChange::Path(path)]);
}

#[cfg(unix)]
#[test]
fn a_path_that_is_not_utf8_converts_to_and_from_a_path_buf() {
    use std::os::unix::ffi::OsStrExt;
    let os = std::ffi::OsStr::from_bytes(b"/tmp/caf\xe9");
    let path = PathBytes::from(std::path::Path::new(os));
    assert_eq!(path.as_bytes(), b"/tmp/caf\xe9");
    assert_eq!(path.to_path_buf().as_os_str(), os);
}

#[test]
fn a_time_before_the_epoch_keeps_its_nanoseconds_positive() {
    let t = SystemTime::UNIX_EPOCH - Duration::from_millis(1_500);
    assert_eq!(
        Time::from(t),
        Time {
            secs: -2,
            nanos: 500_000_000
        }
    );
}
