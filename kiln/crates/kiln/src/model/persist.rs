// SPDX-License-Identifier: LicenseRef-Proprietary

//! A model kept in the state directory: [`persist`] loads `M` from
//! `<state dir>/<file>.json` when it registers, and saves it after each
//! change, debounced and off the main thread, and at quit.

use std::{
    cell::RefCell,
    fs,
    io::Write as _,
    path::{Path, PathBuf},
    rc::Rc,
    time::Duration,
};

use gpui::{App, Global, Task};
use kiln_shell::persist::Writer;
use serde::{Serialize, de::DeserializeOwned};

use crate::model;

/// How long after a change `M` is written, a burst of changes coalescing.
pub const DEBOUNCE: Duration = Duration::from_millis(500);

/// The directory persisted models are stored in: the app's state
/// directory, which [`AppBuilder::start`](crate::AppBuilder::start) records.
#[derive(Clone, Debug)]
pub struct StateDir(pub PathBuf);

impl Global for StateDir {}

/// Registers `M` as persisted in `<state dir>/<file>.json`. It's loaded
/// now: a missing file starts it from `Default`, and one that doesn't
/// parse is renamed to `<file>.json.bad`, logged, and `M` starts from
/// `Default`. After each change it's written 500 ms later, a burst of
/// changes in one write, off the main thread; and at quit, which waits for
/// the write. Without a [`StateDir`] it's an ordinary model.
pub fn persist<M>(cx: &mut App, file: &str)
where
    M: Serialize + DeserializeOwned + Default + 'static,
{
    persist_with::<M>(cx, file, write);
}

/// [`persist`], writing with `write` (run on the background executor) in
/// place of the atomic file write: for a test to see the writes.
#[doc(hidden)]
pub fn persist_with<M>(
    cx: &mut App,
    file: &str,
    write: impl Fn(&Path, &[u8]) -> std::io::Result<()> + Send + Sync + 'static,
) where
    M: Serialize + DeserializeOwned + Default + 'static,
{
    let Some(dir) = cx.try_global::<StateDir>().map(|dir| dir.0.clone()) else {
        log::warn!("no state directory: `{file}` isn't persisted");
        model::register::<M>(cx);
        return;
    };
    let path = dir.join(format!("{file}.json"));
    let entity = model::insert(cx, load::<M>(&path));

    let writer = Writer::new(cx, move |path: &PathBuf, bytes: Vec<u8>| {
        if let Err(error) = write(path, &bytes) {
            log::error!("can't save {}: {error}", path.display());
        }
    });
    // What's on disk, or would be: loaded, or last queued. A save of what's
    // the same is skipped, so quitting writes only what changed, however
    // the change was noticed.
    let last = Rc::new(RefCell::new(
        serde_json::to_vec_pretty(entity.read(cx)).ok(),
    ));
    let timer: Rc<RefCell<Option<Task<()>>>> = Rc::default();

    // Takes the model's value as it is now and queues it.
    let save = {
        let (writer, last, path) = (writer.clone(), last.clone(), path.clone());
        move |cx: &mut App| {
            let entity = model::entity_if::<M>(cx);
            let Some(Ok(bytes)) = entity.map(|model| serde_json::to_vec_pretty(model.read(cx)))
            else {
                log::error!("can't serialize the model for {}", path.display());
                return;
            };
            if last.borrow().as_ref() == Some(&bytes) {
                return;
            }
            last.replace(Some(bytes.clone()));
            writer.save(path.clone(), bytes);
        }
    };

    let (observed_timer, observed_save) = (timer.clone(), save.clone());
    cx.observe(&entity, move |_, cx| {
        let save = observed_save.clone();
        // Replacing the task drops the last, restarting the wait.
        let task = cx.spawn(async move |cx| {
            cx.background_executor().timer(DEBOUNCE).await;
            cx.update(|cx| save(cx));
        });
        observed_timer.replace(Some(task));
    })
    .detach();

    cx.on_app_quit(move |cx| {
        timer.take();
        save(cx);
        writer.finish()
    })
    .detach();
}

/// The value in `path`; `M::default()` if there's no file, or if it
/// doesn't parse, when it's set aside as `<path>.bad`.
fn load<M: DeserializeOwned + Default>(path: &Path) -> M {
    let text = match fs::read(path) {
        Ok(text) => text,
        Err(error) => {
            if error.kind() != std::io::ErrorKind::NotFound {
                log::warn!("can't read {}: {error}", path.display());
            }
            return M::default();
        }
    };
    match serde_json::from_slice(&text) {
        Ok(model) => model,
        Err(error) => {
            let mut bad = path.as_os_str().to_owned();
            bad.push(".bad");
            let moved = fs::rename(path, &bad);
            log::warn!(
                "{} doesn't parse ({error}); {}, starting from the default",
                path.display(),
                match moved {
                    Ok(()) => format!("renamed to {}", Path::new(&bad).display()),
                    Err(error) => format!("couldn't rename it: {error}"),
                }
            );
            M::default()
        }
    }
}

/// Puts `bytes` in place at `path` whole, through a temporary file.
fn write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".tmp");
    let mut file = fs::File::create(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temporary, path)
}
