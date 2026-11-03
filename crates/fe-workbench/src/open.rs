//! Opening a folder or a file from the system's open panel, and a new
//! untitled file, saved where the save panel says.
//!
//! Its commands are typed: `#[kiln::command]` methods of the workbench
//! (`workbench_commands.rs`), in its [`crate::KEY_CONTEXT`], that each name
//! an [`OpenOp`]. The workbench then shows the panel, and does what its answer asks with the window at
//! hand: a folder re-roots the workbench in place
//! ([`crate::Workbench::set_root`]), and a file opens in the active pane.
//! New File opens an untitled tab in the active pane; Save As… asks where
//! to save the active editor, which then saves there.
//! `window.new` opens a window on the same folder, and Open Folder in New
//! Window one on the folder picked, through the host's opener
//! ([`set_window_opener`]).

use std::{
    cell::{Cell, RefCell},
    fs,
    io::{self, Write as _},
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use fe_buffer::BufferId;
use gpui::{App, Global, PathPromptOptions};

/// What an open command asks for: its `#[kiln::command]` method runs it
/// with `Workbench::prompt_open`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OpenOp {
    /// Pick a folder to re-root the workbench at.
    Folder,
    /// Pick a file to open in the active pane.
    File,
    /// Open the user's `settings.toml`, creating it from the template.
    Settings,
    /// Open a window on the same folder (`window.new`).
    NewWindow,
    /// Pick a folder to open in a new window.
    FolderInNewWindow,
    /// Open an untitled tab in the active pane (`workbench.new_file`).
    NewFile,
    /// Pick where to save the active editor, and save it there
    /// (`workbench.save_as`): the save panel, not the open panel.
    SaveAs,
}

impl OpenOp {
    /// How the open panel is shown for it: one folder, or one file.
    pub(crate) fn prompt(self) -> PathPromptOptions {
        debug_assert!(
            matches!(
                self,
                OpenOp::Folder | OpenOp::File | OpenOp::FolderInNewWindow
            ),
            "only folders and files are picked in the open panel"
        );
        let folder = matches!(self, OpenOp::Folder | OpenOp::FolderInNewWindow);
        PathPromptOptions {
            files: !folder,
            directories: folder,
            multiple: false,
            prompt: Some("Open".into()),
        }
    }
}

/// Opens a window on a folder, or with none: the host's.
type Opener = Rc<dyn Fn(Option<PathBuf>, &mut App)>;

struct WindowOpener(Opener);

impl Global for WindowOpener {}

/// Has `window.new` and Open Folder in New Window open a window through
/// `open`, given the folder it's on: the window's own, or the one picked.
/// Until it's set, they do nothing but log.
pub fn set_window_opener(cx: &mut App, open: impl Fn(Option<PathBuf>, &mut App) + 'static) {
    cx.set_global(WindowOpener(Rc::new(open)));
}

/// Opens a window on `root` through the host's opener, once the current
/// update is over.
pub(crate) fn open_window(root: Option<PathBuf>, cx: &mut App) {
    let Some(WindowOpener(open)) = cx.try_global::<WindowOpener>() else {
        log::warn!("no window opener: the host opens one window");
        return;
    };
    let open = open.clone();
    cx.defer(move |cx| open(root, cx));
}

/// Where a path the system asked the app to open goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DocumentRoute {
    /// A live window (its index in the roots given) shows it: a folder is
    /// that window's root, a file is opened in it.
    Window {
        window: usize,
        file: Option<PathBuf>,
    },
    /// No live window does: a new one, on the folder itself or on the
    /// file's parent, with the file opened in it.
    NewWindow {
        root: PathBuf,
        file: Option<PathBuf>,
    },
}

/// `path` with symlinks resolved as far as it exists, so a missing file
/// compares by its folder.
fn resolved(path: &Path) -> PathBuf {
    if let Ok(real) = fs::canonicalize(path) {
        return real;
    }
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) if !parent.as_os_str().is_empty() => resolved(parent).join(name),
        _ => path.to_owned(),
    }
}

/// Where `path` goes given the `roots` of the live windows: a folder to a
/// window on it, else a new one; a file to the window whose root contains
/// it (the deepest, when several do), else a new window on its parent.
pub fn route_document(path: &Path, roots: &[Option<PathBuf>]) -> DocumentRoute {
    let path = std::path::absolute(path).unwrap_or_else(|_| path.to_owned());
    let real = resolved(&path);
    let roots: Vec<_> = roots.iter().map(|r| r.as_deref().map(resolved)).collect();
    if path.is_dir() {
        return match roots.iter().position(|root| root.as_ref() == Some(&real)) {
            Some(window) => DocumentRoute::Window { window, file: None },
            None => DocumentRoute::NewWindow {
                root: path,
                file: None,
            },
        };
    }
    let deepest = (roots.iter().enumerate())
        .filter_map(|(i, root)| Some((i, root.as_ref()?)))
        .filter(|(_, root)| real.starts_with(root))
        .max_by_key(|(_, root)| root.components().count());
    match deepest {
        Some((window, _)) => DocumentRoute::Window {
            window,
            file: Some(path),
        },
        None => DocumentRoute::NewWindow {
            root: path.parent().map(Path::to_owned).unwrap_or_default(),
            file: Some(path),
        },
    }
}

/// The path a `file://` URL names, percent-decoded; `None` for any other
/// URL, or a path that isn't UTF-8.
pub fn path_of_url(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("file://")?;
    // `file://host/path`: only the local machine's.
    let path = &rest[rest.find('/')?..];
    let path = path.split(['?', '#']).next().unwrap_or(path);
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = path.get(i + 1..i + 3)
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok().map(PathBuf::from)
}

/// Opens what the system handed the app (Finder's Open With, `open -a`, a
/// drop on the Dock icon), each of `paths` by [`route_document`] over the
/// live workbench windows. `new_window` opens a window on a folder, with a
/// file to open in it.
pub fn open_documents(
    paths: &[PathBuf],
    cx: &mut App,
    new_window: impl Fn(PathBuf, Option<PathBuf>, &mut App),
) {
    for path in paths {
        let windows: Vec<_> = cx
            .windows()
            .into_iter()
            .filter_map(|w| w.downcast::<crate::Workbench>())
            .collect();
        let roots: Vec<_> = windows
            .iter()
            .map(|w| {
                w.read_with(cx, |wb, _| wb.root().map(Path::to_owned))
                    .ok()
                    .flatten()
            })
            .collect();
        match route_document(path, &roots) {
            DocumentRoute::Window { window, file } => {
                windows[window]
                    .update(cx, |wb, window, cx| {
                        window.activate_window();
                        if let Some(file) = file {
                            wb.open_document(file, window, cx);
                        }
                    })
                    .ok();
            }
            DocumentRoute::NewWindow { root, file } => new_window(root, file, cx),
        }
    }
}

#[cfg(test)]
#[path = "save_as_tests.rs"]
mod save_as_tests;

/// Which file a save to `path` writes, to tell two paths to one file apart
/// from two files: the target a save follows symlinks to
/// ([`fe_editor::save_target`]), dangling ones too, resolved through `..`
/// and its folder's symlinks. `None` if that can't be found.
pub fn file_identity(path: &Path) -> Option<PathBuf> {
    crate::lsp::resolve(&fe_editor::save_target(path).ok()?)
}

/// A Save As destination, held from the moment it's chosen until the
/// workbench has taken in how its save went (the buffer at the new path,
/// the tabs following it), whatever happens to the tab meanwhile: no other
/// buffer saves there, and an open of it waits ([`reserved`]). Dropping it
/// lets go.
pub(crate) struct Reservation {
    id: u64,
    _over: async_channel::Sender<()>,
}

struct Held {
    id: u64,
    buffer: BufferId,
    path: PathBuf,
    identity: Option<PathBuf>,
    /// Closes once let go.
    over: async_channel::Receiver<()>,
}

thread_local! {
    /// The destinations held. GPUI runs on one thread.
    static RESERVED: RefCell<Vec<Held>> = const { RefCell::new(Vec::new()) };
    static NEXT_RESERVATION: Cell<u64> = const { Cell::new(0) };
}

/// Holds `path`, which is `identity` ([`file_identity`]), for a Save As of
/// `buffer`.
pub(crate) fn reserve(buffer: BufferId, path: PathBuf, identity: Option<PathBuf>) -> Reservation {
    let id = NEXT_RESERVATION.replace(NEXT_RESERVATION.get() + 1);
    let (over, done) = async_channel::bounded(1);
    RESERVED.with_borrow_mut(|held| {
        held.push(Held {
            id,
            buffer,
            path,
            identity,
            over: done,
        })
    });
    Reservation { id, _over: over }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        let id = self.id;
        RESERVED
            .try_with(|held| held.borrow_mut().retain(|held| held.id != id))
            .ok();
    }
}

/// The buffer holding a Save As destination that's `path` or `identity`,
/// and a channel that closes once it's let go; `None` if none is held
/// there.
pub(crate) fn reserved(
    path: &Path,
    identity: Option<&Path>,
) -> Option<(BufferId, async_channel::Receiver<()>)> {
    RESERVED.with_borrow(|held| {
        held.iter()
            .find(|held| {
                held.path == path || identity.is_some() && held.identity.as_deref() == identity
            })
            .map(|held| (held.buffer, held.over.clone()))
    })
}

/// Every Save As destination held, by the path it was chosen as and the
/// file that is ([`file_identity`]).
pub(crate) fn reserved_paths() -> Vec<PathBuf> {
    RESERVED.with_borrow(|held| {
        (held.iter())
            .flat_map(|held| [Some(held.path.clone()), held.identity.clone()])
            .flatten()
            .collect()
    })
}

/// Whether any Save As destination is held: until one is, an open needn't
/// resolve its path to look.
pub(crate) fn any_reserved() -> bool {
    RESERVED.with_borrow(|held| !held.is_empty())
}

/// Where the tests say the user's config directory is, instead of the
/// environment's: `None` is an app with no config directory.
#[cfg(test)]
pub(crate) struct ConfigDirOverride(pub Option<PathBuf>);

#[cfg(test)]
impl Global for ConfigDirOverride {}

/// The user's `settings.toml`, if there's a config directory.
pub(crate) fn settings_path(cx: &App) -> Option<PathBuf> {
    #[cfg(test)]
    if let Some(over) = cx.try_global::<ConfigDirOverride>() {
        return over
            .0
            .as_ref()
            .map(|dir| dir.join(fe_config::paths::SETTINGS_FILE));
    }
    let _ = cx;
    fe_config::user_settings_path()
}

/// What overlapping requests for one path share: the outcome of the
/// request that initializes it, once it has one.
struct Flight {
    outcome: Mutex<Option<Result<(), (io::ErrorKind, String)>>>,
    done: Condvar,
}

/// Writes the settings template to `path`, with its directory, unless the
/// file exists, which is never overwritten.
///
/// The template is written to a temporary file and linked into place, so
/// the file is there whole or not at all, even if cleanup after a failed
/// write fails too. Requests overlapping an initialization wait for it and
/// get its outcome, an error included, rather than retrying; the next
/// request after it starts afresh.
pub(crate) fn create_settings(path: &Path) -> io::Result<()> {
    static FLIGHTS: Mutex<Vec<(PathBuf, Arc<Flight>)>> = Mutex::new(Vec::new());
    let (flight, leader) = {
        let mut flights = FLIGHTS.lock().unwrap_or_else(|e| e.into_inner());
        match flights.iter().find(|(p, _)| p == path) {
            Some((_, flight)) => (flight.clone(), false),
            None => {
                let flight = Arc::new(Flight {
                    outcome: Mutex::new(None),
                    done: Condvar::new(),
                });
                flights.push((path.to_owned(), flight.clone()));
                (flight, true)
            }
        }
    };
    if !leader {
        #[cfg(test)]
        hook(path, Stage::Wait).ok();
        let mut outcome = flight.outcome.lock().unwrap_or_else(|e| e.into_inner());
        while outcome.is_none() {
            outcome = flight.done.wait(outcome).unwrap_or_else(|e| e.into_inner());
        }
        return match outcome.clone().expect("the loop ends with an outcome") {
            Ok(()) => Ok(()),
            Err((kind, message)) => Err(io::Error::new(kind, message)),
        };
    }
    let result = initialize(path);
    // Out of the list first, so a request after this one starts afresh.
    FLIGHTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .retain(|(_, f)| !Arc::ptr_eq(f, &flight));
    *flight.outcome.lock().unwrap_or_else(|e| e.into_inner()) = Some(
        result
            .as_ref()
            .map(|_| ())
            .map_err(|e| (e.kind(), e.to_string())),
    );
    flight.done.notify_all();
    result
}

fn initialize(path: &Path) -> io::Result<()> {
    let dir = path.parent();
    if let Some(dir) = dir {
        fs::create_dir_all(dir)?;
    }
    if path.exists() {
        return Ok(());
    }
    // A name no other file has, made exclusively, so nothing existing (or
    // a symlink) is written through, and only what this attempt made is
    // ever removed.
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let name = path.file_name().unwrap_or_default();
    let mut attempts = 0;
    let (temp, file) = loop {
        let mut temp_name = name.to_owned();
        temp_name.push(format!(
            ".{}-{}.tmp",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let temp = dir.unwrap_or(Path::new("")).join(temp_name);
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
        {
            Ok(file) => break (temp, file),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists && attempts < 100 => attempts += 1,
            Err(e) => return Err(e),
        }
    };
    let written = (|| {
        let mut file = file;
        #[cfg(test)]
        hook(path, Stage::Write)?;
        file.write_all(fe_config::USER_SETTINGS_TEMPLATE.as_bytes())?;
        file.sync_all()
    })();
    // Linking fails if `path` has appeared meanwhile, which stays as is.
    let linked = written.and_then(|()| match fs::hard_link(&temp, path) {
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        other => other,
    });
    #[cfg(test)]
    let removed = hook(path, Stage::Cleanup).and_then(|()| fs::remove_file(&temp));
    #[cfg(not(test))]
    let removed = fs::remove_file(&temp);
    match (linked, removed) {
        (Ok(()), _) => Ok(()),
        (Err(error), Ok(())) => Err(error),
        (Err(error), Err(cleanup)) => Err(io::Error::new(
            error.kind(),
            format!(
                "{error} (and removing {} failed: {cleanup})",
                temp.display()
            ),
        )),
    }
}

/// The steps of initializing a test can intercept.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stage {
    /// The temporary file is created, the template not yet written; an
    /// error fails the write.
    Write,
    /// The temporary file is about to be removed; an error fails that.
    Cleanup,
    /// A request found an initialization under way and is about to wait.
    Wait,
}

#[cfg(test)]
pub(crate) type Hook = Arc<dyn Fn(Stage) -> io::Result<()> + Send + Sync>;

/// Intercepts the one path a test names.
#[cfg(test)]
pub(crate) static HOOK: Mutex<Option<(PathBuf, Hook)>> = Mutex::new(None);

#[cfg(test)]
fn hook(path: &Path, stage: Stage) -> io::Result<()> {
    let hook = match HOOK.lock().unwrap().as_ref() {
        Some((hooked, hook)) if hooked == path => Some(hook.clone()),
        _ => None,
    };
    // Called unlocked: it blocks.
    hook.map_or(Ok(()), |hook| hook(stage))
}
