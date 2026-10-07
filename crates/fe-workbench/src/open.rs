//! Opening a folder or a file from the system's open panel.
//!
//! Its commands are those of [`init`]'s registry, in the workbench's
//! [`crate::KEY_CONTEXT`]. Their handlers only name an op: the workbench
//! then shows the panel, and does what its answer asks with the window at
//! hand: a folder re-roots the workbench in place
//! ([`crate::Workbench::set_root`]), and a file opens in the active pane.

use std::{
    fs,
    io::{self, Write as _},
    path::PathBuf,
    rc::Rc,
    sync::{Arc, Mutex},
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
}

impl OpenOp {
    /// How the open panel is shown for it: one folder, or one file.
    pub(crate) fn prompt(self) -> PathPromptOptions {
        debug_assert_ne!(self, OpenOp::Settings, "settings need no open panel");
        let folder = self == OpenOp::Folder;
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

/// Registers the open commands.
pub(crate) fn init(cx: &mut App) {
    cx.set_global(Commands(Rc::new(registry())));
}

pub(crate) fn registry() -> Registry<OpenCommands> {
    let commands = [
        ("workbench.open_folder", "Open Folder…", OpenOp::Folder),
        ("workbench.open_file", "Open File…", OpenOp::File),
        ("workbench.open_settings", "Settings…", OpenOp::Settings),
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

/// Writes the settings template to `path`, with its directory, unless the
/// file exists, which is never overwritten.
///
/// Requests for the same path are serialized, so one that finds the file
/// there finds it complete: it was written whole, or, if the write failed,
/// removed again for the next request to retry and report its own error.
pub(crate) fn create_settings(path: &std::path::Path) -> io::Result<()> {
    let lock = {
        static LOCKS: Mutex<Vec<(PathBuf, Arc<Mutex<()>>)>> = Mutex::new(Vec::new());
        let mut locks = LOCKS.lock().unwrap_or_else(|e| e.into_inner());
        match locks.iter().find(|(p, _)| p == path) {
            Some((_, lock)) => lock.clone(),
            None => {
                let lock = Arc::new(Mutex::new(()));
                locks.push((path.to_owned(), lock.clone()));
                lock
            }
        }
    };
    let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut file) => {
            #[cfg(test)]
            after_create_hook(path);
            let written = file.write_all(fe_config::USER_SETTINGS_TEMPLATE.as_bytes());
            if written.is_err() {
                drop(file);
                let _ = fs::remove_file(path);
            }
            written
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
pub(crate) type Hook = Arc<dyn Fn() + Send + Sync>;

/// Runs between `create_settings` creating the file and writing it, for
/// the one path a test names.
#[cfg(test)]
pub(crate) static AFTER_CREATE: Mutex<Option<(PathBuf, Hook)>> = Mutex::new(None);

#[cfg(test)]
fn after_create_hook(path: &std::path::Path) {
    let hook = match AFTER_CREATE.lock().unwrap().as_ref() {
        Some((hooked, hook)) if hooked == path => Some(hook.clone()),
        _ => None,
    };
    // Called unlocked: it blocks.
    if let Some(hook) = hook {
        hook();
    }
}
