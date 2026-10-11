//! Opening a folder or a file from the system's open panel.
//!
//! Its commands are those of [`init`]'s registry, in the workbench's
//! [`crate::KEY_CONTEXT`]. Their handlers only name an op: the workbench
//! then shows the panel, and does what its answer asks with the window at
//! hand: a folder re-roots the workbench in place
//! ([`crate::Workbench::set_root`]), and a file opens in the active pane.
//! `window.new` opens a window on the same folder, and Open Folder in New
//! Window one on the folder picked, through the host's opener
//! ([`set_window_opener`]).

use std::{
    fs,
    io::{self, Write as _},
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use fe_command::{Actor, CommandTarget, ContextPredicate, Dispatched, Effect, Registry};
use fe_gpui::commands::RunCommand;
use gpui::{App, Global, PathPromptOptions};

/// What an open command asks for.
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
}

impl OpenOp {
    /// How the open panel is shown for it: one folder, or one file.
    pub(crate) fn prompt(self) -> PathPromptOptions {
        debug_assert!(
            !matches!(self, OpenOp::Settings | OpenOp::NewWindow),
            "settings and a new window need no open panel"
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

/// The [`CommandTarget`] of open commands: the op to do.
pub(crate) struct OpenCommands;

impl CommandTarget for OpenCommands {
    type Tx = ();
    type Target<'a> = Option<OpenOp>;
}

struct Commands(Rc<Registry<OpenCommands>>);

impl Global for Commands {}

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

/// Registers the open commands.
pub(crate) fn init(cx: &mut App) {
    cx.set_global(Commands(Rc::new(registry())));
}

pub(crate) fn registry() -> Registry<OpenCommands> {
    let commands = [
        ("workbench.open_folder", "Open Folder…", OpenOp::Folder),
        ("workbench.open_file", "Open File…", OpenOp::File),
        ("workbench.open_settings", "Settings…", OpenOp::Settings),
        ("window.new", "New Window", OpenOp::NewWindow),
        (
            "workbench.open_folder_in_new_window",
            "Open Folder in New Window…",
            OpenOp::FolderInNewWindow,
        ),
    ];
    let mut registry = Registry::new();
    for (id, title, op) in commands {
        registry
            .register(id, title, move |target: &mut Option<OpenOp>, _: &Actor| {
                *target = Some(op);
                Ok(Dispatched::new(Effect::Notify))
            })
            .expect("open command IDs are valid and distinct")
            .context(ContextPredicate::identifier(crate::KEY_CONTEXT));
    }
    registry
}

/// The op of `run`, if it's an open command that applies in its context.
pub(crate) fn op(run: &RunCommand, cx: &App) -> Option<OpenOp> {
    let registry = cx.try_global::<Commands>()?.0.clone();
    registry.get(&run.0.command)?;
    let mut op = None;
    if let Err(error) = registry.dispatch(&mut op, &run.0) {
        log::warn!("{error}");
    }
    op
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
