//! `tenmoku --wait`: the requests whose clients are waiting for their files to
//! close, for `EDITOR="tenmoku --wait"`.
//!
//! A request's wait ends when none of its files has a tab in any window, so
//! where a tab is doesn't matter, and an entry is a file with no window to it:
//! a tab moved to another pane holds the wait, as does a tab that was open
//! before the request, and a tab whose file is now another (Save As) doesn't.
//! A tab with a Save/Discard prompt up is still a tab: the wait ends only if the
//! prompt lets it close.
//!
//! The tabs are looked at again whenever a pane changes or goes and a window
//! closes, after the effects of the moment are over: a tab moving between panes
//! leaves one before it reaches the other. A quit answers every wait. A client
//! that disconnects is forgotten; its tabs stay.
//!
//! A file that fails to open has no tab, and so holds no wait: the app has shown
//! the error, and the client isn't left blocked on a tab that will never close.
//! A `--wait` launch with no server to be a client of, because it serves or runs
//! alone, is the editor itself: it can only exit by quitting, so that's what the
//! end of its wait does ([`add_own`]), asking as any quit does.

use std::path::{Path, PathBuf};

use async_channel::Sender;
use fe_workbench::{Pane, Workbench, file_identity};
use gpui::{App, Global, Task, WindowHandle};

use crate::ipc::{self, Handoff, Request, Waiter};

/// A file a request waits on: by the path it was asked for, and by any other
/// path to the same file.
struct Entry {
    path: PathBuf,
    identity: Option<PathBuf>,
}

impl Entry {
    fn new(path: PathBuf) -> Self {
        let identity = file_identity(&path);
        Self { path, identity }
    }

    fn has_tab(&self, tabs: &[PathBuf]) -> bool {
        // The paths alone first: this runs whenever a pane changes.
        tabs.contains(&self.path)
            || (self.identity.is_some()
                && (tabs.iter()).any(|tab| file_identity(tab) == self.identity))
    }
}

/// Who a wait's end is for.
enum Waiting {
    /// A client, over its connection.
    Client(Waiter),
    /// This process, with what ends it.
    Itself(Box<dyn FnOnce(&mut App)>),
}

impl Waiting {
    fn over(self, cx: &mut App) {
        match self {
            Waiting::Client(waiter) => waiter.closed(),
            Waiting::Itself(quit) => quit(cx),
        }
    }
}

/// One launch's wait.
struct Wait {
    id: u64,
    entries: Vec<Entry>,
    /// Its files have all landed, or failed to: until then, a file with no tab
    /// just hasn't got one yet.
    landed: bool,
    waiting: Waiting,
}

/// The waits, on the main thread.
struct WaitSet {
    waits: Vec<Wait>,
    next_id: u64,
    /// A look at the tabs is due once the moment's effects are over.
    looking: bool,
    /// Where a client's [`Waiter::watch`] says it has gone.
    gone: Sender<u64>,
}

impl Global for WaitSet {}

/// Starts hearing what can end a wait. Call it before the first window opens,
/// for it's new panes that are heard.
pub(crate) fn init(cx: &mut App) {
    let (gone, went) = async_channel::unbounded();
    cx.set_global(WaitSet {
        waits: Vec::new(),
        next_id: 0,
        looking: false,
        gone,
    });
    cx.observe_new(|_: &mut Pane, _, cx| {
        cx.observe_self(|_, cx| look_soon(cx)).detach();
        // Close Pane says nothing else.
        cx.on_release(|_, cx| look_soon(cx)).detach();
    })
    .detach();
    cx.on_window_closed(|cx, _| look_soon(cx)).detach();
    cx.on_app_quit(|cx| {
        answer_all(cx);
        async {}
    })
    .detach();
    cx.spawn(async move |cx| {
        while let Ok(id) = went.recv().await {
            cx.update(|cx| forget(id, cx));
        }
    })
    .detach();
}

/// Acts on `handoff`, as [`ipc::route`] does, and answers it: at once, or with
/// `wait`, once its files have closed.
pub(crate) fn serve(
    handoff: Handoff,
    cx: &mut App,
    new_window: impl Fn(Option<PathBuf>, &mut App) -> Option<WindowHandle<Workbench>> + 'static,
) {
    let request = handoff.request.clone();
    let files = files_of(&request);
    let landed = ipc::route(request, cx, new_window);
    if handoff.request.wait && !files.is_empty() {
        add(handoff.hold(), files, landed, cx);
    } else {
        handoff.answer();
        landed.detach();
    }
}

/// What `request` waits on: its targets that aren't folders.
fn files_of(request: &Request) -> Vec<PathBuf> {
    (request.targets.iter())
        .map(|target| request.cwd.join(&target.path))
        .filter(|path| !Path::is_dir(path))
        .collect()
}

/// Has `waiter` told once `files` have no tab left, which are looked at once
/// `landed` has finished opening them. With no files, that's now.
pub(crate) fn add(waiter: Waiter, files: Vec<PathBuf>, landed: Task<()>, cx: &mut App) {
    track(Waiting::Client(waiter), files, landed, cx);
}

/// [`add`], for this process's own launch: `quit` runs in place of telling a
/// client.
pub(crate) fn add_own(
    quit: impl FnOnce(&mut App) + 'static,
    files: Vec<PathBuf>,
    landed: Task<()>,
    cx: &mut App,
) {
    track(Waiting::Itself(Box::new(quit)), files, landed, cx);
}

fn track(waiting: Waiting, files: Vec<PathBuf>, landed: Task<()>, cx: &mut App) {
    if files.is_empty() {
        waiting.over(cx);
        landed.detach();
        return;
    }
    let set = cx.global_mut::<WaitSet>();
    let id = set.next_id;
    set.next_id += 1;
    if let Waiting::Client(waiter) = &waiting {
        let gone = set.gone.clone();
        waiter.watch(move || _ = gone.try_send(id));
    }
    set.waits.push(Wait {
        id,
        entries: files.into_iter().map(Entry::new).collect(),
        landed: false,
        waiting,
    });
    cx.spawn(async move |cx| {
        landed.await;
        cx.update(|cx| {
            let waits = &mut cx.global_mut::<WaitSet>().waits;
            if let Some(wait) = waits.iter_mut().find(|wait| wait.id == id) {
                wait.landed = true;
                look_soon(cx);
            }
        });
    })
    .detach();
}

/// Has the tabs looked at once the moment's effects are over.
fn look_soon(cx: &mut App) {
    // Read-only at first: this runs whenever any pane changes, and a change to
    // a global is an effect of its own.
    let set = cx.global::<WaitSet>();
    if set.waits.is_empty() || set.looking {
        return;
    }
    cx.global_mut::<WaitSet>().looking = true;
    cx.defer(|cx| {
        cx.global_mut::<WaitSet>().looking = false;
        look(cx);
    });
}

/// Answers each wait whose files have no tab left.
fn look(cx: &mut App) {
    let tabs = open_tabs(cx);
    let waits = std::mem::take(&mut cx.global_mut::<WaitSet>().waits);
    let (over, waits): (Vec<_>, Vec<_>) = waits
        .into_iter()
        .partition(|wait| wait.landed && !wait.entries.iter().any(|entry| entry.has_tab(&tabs)));
    cx.global_mut::<WaitSet>().waits = waits;
    for wait in over {
        wait.waiting.over(cx);
    }
}

/// The file of every tab of every window.
fn open_tabs(cx: &App) -> Vec<PathBuf> {
    (cx.windows().into_iter())
        .filter_map(|window| window.downcast::<Workbench>())
        .filter_map(|window| {
            let read = window.read_with(cx, |workbench, cx| {
                let panes = workbench.pane_group(cx).panes();
                (panes.into_iter())
                    .flat_map(|pane| pane.read(cx).tabs())
                    .filter_map(|tab| tab.path.clone())
                    .collect::<Vec<_>>()
            });
            read.ok()
        })
        .flatten()
        .collect()
}

/// Forgets the wait of a client that has gone, if it's still waiting.
fn forget(id: u64, cx: &mut App) {
    (cx.global_mut::<WaitSet>().waits).retain(|wait| wait.id != id);
}

/// Answers every client, for the app is quitting.
fn answer_all(cx: &mut App) {
    for wait in std::mem::take(&mut cx.global_mut::<WaitSet>().waits) {
        if let Waiting::Client(waiter) = wait.waiting {
            waiter.closed();
        }
    }
}

/// How many clients are waiting.
#[cfg(all(test, unix))]
pub(crate) fn pending(cx: &App) -> usize {
    cx.global::<WaitSet>().waits.len()
}

#[cfg(test)]
#[path = "wait_tests.rs"]
mod tests;
