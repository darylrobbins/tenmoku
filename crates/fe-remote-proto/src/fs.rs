//! Channel 1's bodies: `FsHost` (§ 5) and write preconditions (§ 7).

use crate::PathBytes;
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime};

/// A `Request` body on channel 1. Paths are absolute on the host.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FsOp {
    /// Replies [`FsData::File`], then the contents as [`FsData::Bytes`]
    /// chunks of at most [`crate::CHUNK`].
    Read { path: PathBytes },
    /// Replaces the file at `path` if `expect` holds. The client follows
    /// the request with exactly `len` bytes as [`FsData::Bytes`] chunks on
    /// the same channel and id, counted against the server's credit. Once
    /// they are all in, the server commits and replies [`FsData::Written`]
    /// or [`FsData::Conflict`].
    WriteAtomic {
        path: PathBytes,
        len: u64,
        expect: WriteExpect,
    },
    /// Replies [`FsData::Stat`]. `follow` stats a symlink's target rather
    /// than the link.
    Stat { path: PathBytes, follow: bool },
    /// Replies [`FsData::Entries`] in batches. `ignored` lists entries an
    /// ignore file matches too, marked as such.
    ListDir { path: PathBytes, ignored: bool },
    /// Creates `name` in `parent`, a file or a `folder`, refusing a name
    /// that is taken. Replies [`FsData::Path`] with the new path.
    Create {
        parent: PathBytes,
        name: PathBytes,
        folder: bool,
    },
    /// Renames `from` within its directory. Replies [`FsData::Path`].
    Rename {
        from: PathBytes,
        new_name: PathBytes,
    },
    /// Deletes a file, or a directory and everything in it.
    Remove { path: PathBytes },
    /// Moves the paths to the host's trash.
    Trash { paths: Vec<PathBytes> },
    /// Replies [`FsData::Path`] with the canonical path.
    Canonicalize { path: PathBytes },
}

/// A `Data` body on channel 1, either way.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FsData {
    /// A chunk of a file's contents, at most [`crate::CHUNK`].
    Bytes(#[serde(with = "serde_bytes")] Vec<u8>),
    /// The file a `Read` opened: its stamp, and how many bytes follow.
    File {
        stamp: DiskStamp,
        len: u64,
    },
    /// A `WriteAtomic` committed; the file's new stamp.
    Written(DiskStamp),
    /// A `WriteAtomic`'s `expect` didn't hold, so nothing was written.
    /// `current` is what is at the path now, if anything.
    Conflict {
        current: Option<DiskStamp>,
    },
    Stat(Stat),
    Entries(Vec<DirEntry>),
    Path(PathBytes),
}

/// What a write expects to replace (§ 7). fe-editor's `SaveJob` maps onto
/// it one to one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WriteExpect {
    /// Overwrite was chosen: replace whatever is there.
    Overwrite,
    /// The buffer doesn't know the file's state. What the server finds when
    /// the write starts must still be there at the commit, and a path found
    /// empty is committed without clobbering.
    AsFound,
    /// Nothing, not even a symlink, may be at the path.
    Absent,
    /// The file must still be the one with this stamp.
    Matches(DiskStamp),
}

/// fe-buffer's `DiskStamp`, computed on the host, so `dev` and `ino` are
/// the host's. The client only compares stamps for equality.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DiskStamp {
    pub len: u64,
    pub modified: Time,
    pub dev: u64,
    pub ino: u64,
}

/// A `SystemTime` as seconds and nanoseconds from the unix epoch; `secs` is
/// negative before it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Time {
    pub secs: i64,
    /// Below 1,000,000,000.
    pub nanos: u32,
}

impl From<SystemTime> for Time {
    fn from(t: SystemTime) -> Time {
        match t.duration_since(SystemTime::UNIX_EPOCH) {
            Ok(d) => Time {
                secs: d.as_secs() as i64,
                nanos: d.subsec_nanos(),
            },
            Err(e) => {
                let d = e.duration();
                let (secs, nanos) = (d.as_secs() as i64, d.subsec_nanos());
                if nanos == 0 {
                    Time { secs: -secs, nanos }
                } else {
                    Time {
                        secs: -secs - 1,
                        nanos: 1_000_000_000 - nanos,
                    }
                }
            }
        }
    }
}

impl Time {
    /// The `SystemTime`, or `None` where this machine can't hold it.
    pub fn to_system_time(self) -> Option<SystemTime> {
        let nanos = Duration::from_nanos(u64::from(self.nanos));
        if self.secs >= 0 {
            SystemTime::UNIX_EPOCH
                .checked_add(Duration::from_secs(self.secs as u64))?
                .checked_add(nanos)
        } else {
            SystemTime::UNIX_EPOCH
                .checked_sub(Duration::from_secs(self.secs.unsigned_abs()))?
                .checked_add(nanos)
        }
    }
}

/// What `Stat` found.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stat {
    pub kind: EntryKind,
    pub stamp: DiskStamp,
    pub readonly: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EntryKind {
    File,
    Dir,
    Symlink,
}

/// One entry of a `ListDir`, as fe-project's listing has it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirEntry {
    pub name: PathBytes,
    pub kind: EntryKind,
    /// An ignore file matches it. Only ever set when `ListDir.ignored` is.
    pub ignored: bool,
    /// A directory's device and inode, from `stat`; `None` for anything
    /// else.
    pub id: Option<EntryId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EntryId {
    pub dev: u64,
    pub ino: u64,
}
