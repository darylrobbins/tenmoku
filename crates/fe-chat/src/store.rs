//! A folder's chats on disk (the record's § 11): one `<id>.json` each,
//! written atomically, kept until deleted.

use std::fmt;
use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::{ChatId, Conversation};

/// The file format's version.
const VERSION: u64 = 1;

/// A chat as listed: enough to show it and pick it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatSummary {
    pub id: ChatId,
    pub title: String,
    /// The file's modified time.
    pub updated: SystemTime,
}

/// Why a chat couldn't be saved, loaded or deleted.
#[derive(Debug)]
pub enum StoreError {
    Io {
        path: PathBuf,
        error: io::Error,
    },
    /// The file isn't a chat this version reads.
    Unreadable {
        path: PathBuf,
        reason: String,
    },
    /// A compare-and-swap save found the file at another revision than the
    /// one the saved chat was derived from: it was written meanwhile.
    Diverged {
        path: PathBuf,
        current: u64,
    },
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Io { path, error } => write!(f, "{}: {error}", path.display()),
            StoreError::Diverged { path, current } => write!(
                f,
                "{} was changed elsewhere (revision {current})",
                path.display()
            ),
            StoreError::Unreadable { path, reason } => {
                write!(
                    f,
                    "{} isn't a chat this version reads: {reason}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for StoreError {}

/// Held while a file is written or removed, so two windows of the process
/// saving or deleting one chat take turns.
static WRITING: Mutex<()> = Mutex::new(());

/// Tells the temporary files of concurrent saves apart.
static TEMPORARIES: AtomicU64 = AtomicU64::new(0);

/// What a file holds, to write.
#[derive(Serialize)]
struct Saved<'a> {
    version: u64,
    /// Counts the accepted saves of this chat; a file from before it has 0.
    revision: u64,
    conversation: &'a Conversation,
}

/// What a file holds, as read: the version first, so a newer file's other
/// shape isn't a parse error.
#[derive(Deserialize)]
struct Versioned {
    version: u64,
}

#[derive(Deserialize)]
struct Loaded {
    #[serde(default)]
    revision: u64,
    conversation: Conversation,
}

/// One folder's chats, in a directory.
#[derive(Clone, Debug)]
pub struct ChatStore {
    dir: PathBuf,
}

impl ChatStore {
    pub fn new(dir: impl Into<PathBuf>) -> ChatStore {
        ChatStore { dir: dir.into() }
    }

    fn file(&self, id: ChatId) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }

    /// Writes `conversation` to `<id>.json` through `<id>.json.tmp` and a
    /// rename, making the directory (0700) and the file (0600) on unix.
    pub fn save(&self, conversation: &Conversation) -> Result<(), StoreError> {
        self.write(conversation, None).map(|_| ())
    }

    /// As [`save`](Self::save), but only if the file is at revision `base`
    /// (0 for a chat not written yet): else it is [`StoreError::Diverged`]
    /// and nothing is written. Gives the new revision.
    pub fn save_if(&self, conversation: &Conversation, base: u64) -> Result<u64, StoreError> {
        self.write(conversation, Some(base))
    }

    fn write(&self, conversation: &Conversation, base: Option<u64>) -> Result<u64, StoreError> {
        let file = self.file(conversation.id);
        let unique = TEMPORARIES.fetch_add(1, Ordering::Relaxed);
        let tmp = file.with_extension(format!("json.{}-{unique}.tmp", std::process::id()));
        let io = |path: &Path| {
            let path = path.to_owned();
            move |error| StoreError::Io { path, error }
        };
        let _writing = WRITING
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let current = fs::read(&file)
            .ok()
            .and_then(|bytes| parse(&file, &bytes).ok())
            .map_or(0, |(_, revision)| revision);
        if base.is_some_and(|base| base != current) {
            return Err(StoreError::Diverged {
                path: file,
                current,
            });
        }
        let revision = current + 1;
        let text = serde_json::to_vec(&Saved {
            version: VERSION,
            revision,
            conversation,
        })
        .map_err(|error| StoreError::Unreadable {
            path: file.clone(),
            reason: error.to_string(),
        })?;
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        builder.create(&self.dir).map_err(io(&self.dir))?;
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let written = options.open(&tmp).and_then(|mut out| {
            out.write_all(&text)?;
            out.sync_all()
        });
        if let Err(error) = written {
            fs::remove_file(&tmp).ok();
            return Err(StoreError::Io { path: tmp, error });
        }
        fs::rename(&tmp, &file).map(|()| revision).map_err(|error| {
            fs::remove_file(&tmp).ok();
            StoreError::Io { path: file, error }
        })
    }

    /// The chat `id`, or an error naming its file if that can't be read.
    pub fn load(&self, id: ChatId) -> Result<Conversation, StoreError> {
        self.load_revision(id).map(|(conversation, _)| conversation)
    }

    /// The chat `id` with the revision of the file it was read from.
    pub fn load_revision(&self, id: ChatId) -> Result<(Conversation, u64), StoreError> {
        let file = self.file(id);
        let bytes = fs::read(&file).map_err(|error| StoreError::Io {
            path: file.clone(),
            error,
        })?;
        parse(&file, &bytes)
    }

    /// The readable chats, newest first. A file that doesn't parse or is of
    /// a newer version stays on disk and is logged, once, and skipped.
    pub fn list(&self) -> Vec<ChatSummary> {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut chats = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(id) = path
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.strip_suffix(".json"))
                .and_then(|stem| stem.parse::<ChatId>().ok())
            else {
                continue;
            };
            let read = fs::read(&path)
                .map_err(|error| StoreError::Io {
                    path: path.clone(),
                    error,
                })
                .and_then(|bytes| parse(&path, &bytes));
            match read {
                Ok((conversation, _)) => {
                    let updated = (entry.metadata().and_then(|meta| meta.modified()))
                        .unwrap_or(SystemTime::UNIX_EPOCH);
                    chats.push(ChatSummary {
                        id,
                        title: conversation.title,
                        updated,
                    });
                }
                Err(error) => log_once(&path, &error),
            }
        }
        chats.sort_by(|a, b| b.updated.cmp(&a.updated).then(b.id.cmp(&a.id)));
        chats
    }

    /// Removes the chat's file; one already gone is fine.
    pub fn delete(&self, id: ChatId) -> Result<(), StoreError> {
        let file = self.file(id);
        let _writing = WRITING
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match fs::remove_file(&file) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => {
                Err(StoreError::Io { path: file, error })
            }
            _ => Ok(()),
        }
    }
}

fn parse(path: &Path, bytes: &[u8]) -> Result<(Conversation, u64), StoreError> {
    let unreadable = |reason: String| StoreError::Unreadable {
        path: path.to_owned(),
        reason,
    };
    let Versioned { version } =
        serde_json::from_slice(bytes).map_err(|error| unreadable(error.to_string()))?;
    if version != VERSION {
        return Err(unreadable(format!("version {version}")));
    }
    let loaded: Loaded =
        serde_json::from_slice(bytes).map_err(|error| unreadable(error.to_string()))?;
    Ok((loaded.conversation, loaded.revision))
}

/// Logs that `path` was skipped, the first time only.
fn log_once(path: &Path, error: &StoreError) {
    use std::collections::HashSet;
    use std::sync::{Mutex, OnceLock};
    static LOGGED: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
    let logged = LOGGED.get_or_init(Default::default);
    if logged
        .lock()
        .is_ok_and(|mut logged| logged.insert(path.to_owned()))
    {
        eprintln!("chat: skipping {}: {error}", path.display());
    }
}
