use serde::{Deserialize, Serialize};
use std::fmt;

/// A path or file name on the wire: the unix `OsStr` bytes, so a name that
/// isn't UTF-8 round-trips (§ 2.1). Paths are host paths, absolute unless a
/// body says otherwise.
#[derive(Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PathBytes(#[serde(with = "serde_bytes")] pub Vec<u8>);

impl PathBytes {
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// The path on this machine. v1 hosts and clients are both unix (§ 6).
    #[cfg(unix)]
    pub fn to_path_buf(&self) -> std::path::PathBuf {
        use std::os::unix::ffi::OsStrExt;
        std::ffi::OsStr::from_bytes(&self.0).into()
    }
}

#[cfg(unix)]
impl From<&std::path::Path> for PathBytes {
    fn from(path: &std::path::Path) -> PathBytes {
        use std::os::unix::ffi::OsStrExt;
        PathBytes(path.as_os_str().as_bytes().to_vec())
    }
}

#[cfg(unix)]
impl From<&std::ffi::OsStr> for PathBytes {
    fn from(name: &std::ffi::OsStr) -> PathBytes {
        use std::os::unix::ffi::OsStrExt;
        PathBytes(name.as_bytes().to_vec())
    }
}

impl From<&str> for PathBytes {
    fn from(path: &str) -> PathBytes {
        PathBytes(path.as_bytes().to_vec())
    }
}

impl From<Vec<u8>> for PathBytes {
    fn from(bytes: Vec<u8>) -> PathBytes {
        PathBytes(bytes)
    }
}

impl fmt::Debug for PathBytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.0.escape_ascii().to_string())
    }
}
