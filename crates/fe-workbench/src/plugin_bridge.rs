//! What a loaded plugin reaches of the editor: the open buffers, through
//! fe-plugin's [`BufferAccess`], and the editor's commands, through
//! kiln-plugin's [`Dispatcher`].
//!
//! - **Buffer ids.** Each open buffer has one `u64` id, the same in every
//!   split and window showing it, kept while any of them is open and never
//!   given to another buffer in the session ([`buffer_id`]). A plugin reads,
//!   edits and proposes by it; events and decorations name buffers by it
//!   too. Its version, as plugins see it, goes up by one with each change
//!   to its text seen since the last.
//! - **Buffers.** The buffers live on the main thread, where their views
//!   are, and a plugin calls on its own thread, so each call is handed off
//!   to the main thread with a bounded wait: the plugin waits for it until
//!   its call's deadline ([`CallDeadline`]), which doesn't count the time
//!   the call's program runs took, and no longer, the import
//!   failing then. A call the main thread hasn't started by then never
//!   runs ([`Handoff`]). A read gives the text, version, path
//!   and language; an edit goes in through [`fe_buffer::Buffer::edit_from`]
//!   under `Origin::Host("<id>")`, one undo step, refused as a conflict
//!   once the buffer has moved past the version the plugin named; a
//!   proposal is shown in an editor of the buffer, the active window's if
//!   one, for the user to accept or reject.
//! - **Commands.** A plugin's invocation (actor `Host("<id>")`, source
//!   `Script`) runs on the active editor of the active window as a
//!   [`RunCommand`]. The editor's registry authorizes it against the
//!   [`CommandHost`] as its `PluginPolicy`
//!   ([`fe_gpui::commands::set_plugin_policy`]); the refusal a plugin hears
//!   is the same registry's, checked first, naming what it lacks.

use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet, VecDeque},
    ops::Range,
    rc::Rc,
    sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError},
    time::{Duration, Instant},
};

use fe_buffer::{BufferId, Origin, Version};
use fe_command::{Actor, CommandError, DispatchError, Invocation, Registry};
use fe_editor::commands::Editor;
use fe_gpui::{EditorView, commands::RunCommand};
use fe_plugin::editor::{BufferAccess, BufferText, Proposal};
use gpui::{App, Entity, Focusable as _, Global};
use kiln_plugin::{
    bindings::Error,
    commands::{CommandHost, Dispatcher, Request},
    process::ProgramBudget,
};

use crate::Workbench;

/// Work a plugin's thread hands the main thread.
type Job = Box<dyn FnOnce(&Served, &mut App) + Send>;

/// The plugins' way to the main thread. Clones share it.
#[derive(Clone)]
pub(crate) struct Bridge {
    jobs: async_channel::Sender<Job>,
}

/// The main thread's end of a [`Bridge`], until [`serve`] takes it.
pub(crate) struct Jobs(async_channel::Receiver<Job>);

impl Bridge {
    pub(crate) fn new() -> (Self, Jobs) {
        let (jobs, received) = async_channel::unbounded();
        (Self { jobs }, Jobs(received))
    }

    /// What the [`CommandHost`] hands plugins' invocations to.
    pub(crate) fn dispatcher(&self) -> Arc<dyn Dispatcher> {
        Arc::new(EditorCommands(self.clone()))
    }

    /// The open buffers, for a plugin whose calls end at `deadline`, or
    /// `fallback` from the wait when no call has set one.
    pub(crate) fn buffers(
        &self,
        deadline: CallDeadline,
        fallback: Duration,
    ) -> Box<dyn BufferAccess> {
        Box::new(Buffers {
            bridge: self.clone(),
            deadline,
            fallback,
        })
    }

    /// Runs `job` on the main thread and waits for what it returns, until
    /// `deadline` ([`Handoff`]).
    fn on_main<R: Send + 'static>(
        &self,
        deadline: Instant,
        job: impl FnOnce(&Served, &mut App) -> R + Send + 'static,
    ) -> Result<R, Error> {
        let handoff = Handoff::new(deadline);
        let theirs = Arc::clone(&handoff);
        let sent = self.jobs.try_send(Box::new(move |served, cx| {
            if theirs.start() {
                theirs.finish(job(served, cx));
            }
        }));
        if sent.is_err() {
            return Err(Error::Failed("the editor has closed".into()));
        }
        handoff.wait()
    }
}

/// A job handed to the main thread, and what came of it. The plugin waits
/// for it until the deadline and no longer, whether or not it has started.
/// The main thread doesn't start it once the deadline has passed or the
/// plugin has stopped waiting; one it started before then runs to its end,
/// as nothing on the main thread stops halfway, and what it returns is
/// dropped if the plugin has stopped waiting.
struct Handoff<R> {
    deadline: Instant,
    slot: Mutex<Slot<R>>,
    done: Condvar,
}

enum Slot<R> {
    Waiting,
    Running,
    Done(R),
    /// The plugin stopped waiting, or the deadline passed before it started.
    Abandoned,
}

impl<R> Handoff<R> {
    fn new(deadline: Instant) -> Arc<Self> {
        Arc::new(Self {
            deadline,
            slot: Mutex::new(Slot::Waiting),
            done: Condvar::new(),
        })
    }

    /// The main thread's side: whether to run the job, then to [`finish`]
    /// it. Not once the plugin has stopped waiting or the deadline passed.
    ///
    /// [`finish`]: Handoff::finish
    fn start(&self) -> bool {
        let mut slot = lock(&self.slot);
        if !matches!(*slot, Slot::Waiting) || Instant::now() >= self.deadline {
            *slot = Slot::Abandoned;
            return false;
        }
        *slot = Slot::Running;
        true
    }

    /// Hands the plugin what the job returned, if it still waits.
    fn finish(&self, returned: R) {
        let mut slot = lock(&self.slot);
        if matches!(*slot, Slot::Running) {
            *slot = Slot::Done(returned);
            self.done.notify_all();
        }
    }

    /// The plugin's side: what the job returned, if it returns by the
    /// deadline.
    fn wait(&self) -> Result<R, Error> {
        let mut slot = lock(&self.slot);
        loop {
            match std::mem::replace(&mut *slot, Slot::Abandoned) {
                Slot::Done(returned) => return Ok(returned),
                waiting => {
                    let left = self.deadline.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        let started = matches!(waiting, Slot::Running);
                        return Err(Error::Failed(format!(
                            "the editor didn't {} before the call's deadline",
                            if started { "finish" } else { "answer" }
                        )));
                    }
                    *slot = waiting;
                    slot = (self.done)
                        .wait_timeout(slot, left)
                        .unwrap_or_else(PoisonError::into_inner)
                        .0;
                }
            }
        }
    }
}

fn lock<T: ?Sized>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// When a plugin's running call must end, which its runner sets and clears
/// around each call, moved back by the time the call's program runs took,
/// as the runner's is. Clones share it.
#[derive(Clone, Default)]
pub(crate) struct CallDeadline {
    deadline: Arc<Mutex<Option<Instant>>>,
    program_runs: ProgramBudget,
}

impl CallDeadline {
    /// One the call's program runs, charged to `program_runs`, move back.
    pub(crate) fn new(program_runs: ProgramBudget) -> Self {
        Self {
            deadline: Arc::default(),
            program_runs,
        }
    }

    pub(crate) fn set(&self, deadline: Option<Instant>) {
        *lock(&self.deadline) = deadline;
    }

    fn get(&self) -> Option<Instant> {
        self.program_runs.extend(*lock(&self.deadline))
    }
}

/// Takes the plugins' jobs from `jobs` on the main thread, checking their
/// invocations against `policy`, which the editor's registry checks them
/// against too from now on.
pub(crate) fn serve(jobs: Jobs, policy: &CommandHost, cx: &mut App) {
    fe_gpui::commands::set_plugin_policy(Arc::new(policy.clone()));
    let mut registry = fe_editor::commands::registry();
    registry.set_plugin_policy(Arc::new(policy.clone()));
    let ran = Rc::new(Cell::new(false));
    let heard = Rc::clone(&ran);
    fe_gpui::commands::observe_invocations(cx, move |invocation| {
        if matches!(invocation.actor, Actor::Host(_)) {
            heard.set(true);
        }
    });
    let served = Served { registry, ran };
    cx.spawn(async move |cx| {
        while let Ok(job) = jobs.0.recv().await {
            cx.update(|cx| job(&served, cx));
        }
    })
    .detach();
}

/// What the main thread runs plugins' jobs with.
pub(crate) struct Served {
    /// The editor's commands, to check a plugin's invocation against before
    /// it runs.
    registry: Registry<Editor>,
    /// Whether an invocation of a plugin's ran since it was last cleared.
    ran: Rc<Cell<bool>>,
}

impl Served {
    /// Runs `invocation` on the active editor of the active window, if the
    /// editor's registry lets the plugin run it there.
    fn run(&self, invocation: &Invocation, cx: &mut App) -> Result<(), DispatchError> {
        let invocation = invocation.clone().context(fe_editor::commands::context());
        self.registry.validate(&invocation)?;
        let failed = |message: &str| DispatchError::Failed {
            command: invocation.command.clone(),
            error: CommandError(message.to_owned()),
        };
        let window = (cx.active_window())
            .and_then(|window| window.downcast::<Workbench>())
            .ok_or_else(|| failed("no window is active"))?;
        let editor = (window.read(cx).ok())
            .and_then(|workbench| workbench.active_editor(cx).cloned())
            .ok_or_else(|| failed("no editor is active"))?;
        self.ran.set(false);
        let run = RunCommand(invocation.clone());
        window
            .update(cx, |_, window, cx| {
                editor.focus_handle(cx).dispatch_action(&run, window, cx);
            })
            .map_err(|_| failed("the window closed"))?;
        match self.ran.take() {
            true => Ok(()),
            false => Err(failed("the active editor didn't run it")),
        }
    }
}

/// Runs plugins' invocations on the main thread.
struct EditorCommands(Bridge);

impl Dispatcher for EditorCommands {
    fn dispatch(&self, request: Request) {
        // Dropped unsent, it's answered as not run.
        let job: Job = Box::new(move |served, cx| {
            request.run(|invocation| served.run(invocation, cx));
        });
        drop(self.0.jobs.try_send(job));
    }
}

/// One plugin's way to the open buffers.
struct Buffers {
    bridge: Bridge,
    deadline: CallDeadline,
    fallback: Duration,
}

impl Buffers {
    fn on_main<R: Send + 'static>(
        &self,
        job: impl FnOnce(&mut App) -> R + Send + 'static,
    ) -> Result<R, Error> {
        let deadline = (self.deadline.get()).unwrap_or_else(|| Instant::now() + self.fallback);
        self.bridge.on_main(deadline, move |_, cx| job(cx))
    }
}

impl BufferAccess for Buffers {
    fn snapshot(&mut self, buffer: u64) -> Option<BufferText> {
        // A read the editor doesn't answer in time finds nothing; the call
        // has run out of time then anyway.
        self.on_main(move |cx| snapshot(buffer, cx)).ok().flatten()
    }

    fn edit(
        &mut self,
        buffer: u64,
        version: u64,
        edits: Vec<(Range<usize>, String)>,
        origin: Origin,
    ) -> Result<u64, Error> {
        self.on_main(move |cx| edit(buffer, version, edits, origin, cx))?
    }

    fn propose(&mut self, buffer: u64, proposal: Proposal) -> Result<(), Error> {
        self.on_main(move |cx| propose(buffer, proposal, cx))?
    }
}

/// The open buffers' ids and versions, by buffer.
#[derive(Default)]
struct BufferIds {
    /// The last id given.
    last: u64,
    open: HashMap<BufferId, Tracked>,
}

impl Global for BufferIds {}

/// An open buffer, as plugins know it.
struct Tracked {
    id: u64,
    /// The buffer's version when its plugin version last went up.
    seen: Version,
    version: u64,
    /// The buffer's last versions plugins were told of, with their plugin
    /// versions, oldest first: the latest and up to [`HISTORY`] before it.
    history: VecDeque<(Version, u64)>,
}

/// How many of a buffer's versions before its latest [`advance`] can name
/// again: a save finishing up to this many changes after the version it
/// wrote.
const HISTORY: usize = 256;

impl Tracked {
    /// Takes the buffer at `version`, newer than any seen, as its next
    /// plugin version.
    fn bump(&mut self, version: Version) {
        self.seen = version;
        self.version += 1;
        self.remember();
    }

    fn remember(&mut self) {
        if self.history.len() > HISTORY {
            self.history.pop_front();
        }
        self.history.push_back((self.seen, self.version));
    }
}

/// Every window's open editors, the tabs' and the diff views' working
/// sides ([`Workbench::file_editors`]), the active window's first, each
/// with its buffer's id and its plugin version. A buffer no editor shows
/// any more loses its id. Plugins' events of the changes not sent yet go
/// first, so that each version they name is one counted here.
pub(crate) fn open_editors(cx: &mut App) -> Vec<(u64, u64, Entity<EditorView>)> {
    crate::plugin_events::catch_up(cx);
    let active = cx.active_window().map(|window| window.window_id());
    let mut windows: Vec<_> = (cx.windows().into_iter())
        .filter_map(|window| window.downcast::<Workbench>())
        .collect();
    windows.sort_by_key(|window| Some(window.window_id()) != active);
    let editors: Vec<_> = (windows.iter())
        .filter_map(|window| window.read(cx).ok())
        .flat_map(|workbench| workbench.file_editors(cx))
        .map(|editor| {
            let buffer = editor.read(cx).model().buffer();
            (buffer.id(), buffer.version(), editor)
        })
        .collect();
    let open: HashSet<BufferId> = editors.iter().map(|(buffer, ..)| *buffer).collect();
    let ids = cx.default_global::<BufferIds>();
    ids.open.retain(|buffer, _| open.contains(buffer));
    (editors.into_iter())
        .map(|(buffer, version, editor)| {
            let tracked = ids.open.entry(buffer).or_insert_with(|| {
                ids.last += 1;
                let mut tracked = Tracked {
                    id: ids.last,
                    seen: version,
                    version: 1,
                    history: VecDeque::new(),
                };
                tracked.remember();
                tracked
            });
            if tracked.seen != version {
                tracked.bump(version);
            }
            (tracked.id, tracked.version, editor)
        })
        .collect()
}

/// Buffer `buffer`'s id and its plugin version at `version`, if it has an
/// id: an event tells of each change as it comes, so a version newer than
/// any seen is the next plugin version. An older one is the plugin version
/// it was given, such as a save's that finished after more typing; none if
/// it was never given one, or is past [`HISTORY`].
pub(crate) fn advance(buffer: BufferId, version: Version, cx: &mut App) -> Option<(u64, u64)> {
    let tracked = cx.default_global::<BufferIds>().open.get_mut(&buffer)?;
    if version > tracked.seen {
        tracked.bump(version);
        return Some((tracked.id, tracked.version));
    }
    let (_, given) = (tracked.history.iter()).rfind(|(seen, _)| *seen == version)?;
    Some((tracked.id, *given))
}

/// The id plugins know `editor`'s buffer by, if it's open in a workbench.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the tests find an editor's buffer by it")
)]
pub(crate) fn buffer_id(editor: &Entity<EditorView>, cx: &mut App) -> Option<u64> {
    (open_editors(cx).into_iter())
        .find(|(_, _, open)| open == editor)
        .map(|(id, ..)| id)
}

/// An editor of buffer `buffer`, the active window's if one, and the
/// buffer's plugin version.
fn editor_of(buffer: u64, cx: &mut App) -> Result<(Entity<EditorView>, u64), Error> {
    (open_editors(cx).into_iter())
        .find(|(id, ..)| *id == buffer)
        .map(|(_, version, editor)| (editor, version))
        .ok_or_else(|| Error::NotFound(format!("no open buffer has id {buffer}")))
}

/// Buffer `buffer` as it is now, if it's open.
pub(crate) fn snapshot(buffer: u64, cx: &mut App) -> Option<BufferText> {
    let (editor, version) = editor_of(buffer, cx).ok()?;
    let view = editor.read(cx);
    let model = view.model();
    Some(BufferText {
        snapshot: model.buffer().snapshot(),
        version,
        path: model.path().map(|path| path.display().to_string()),
        language: (view.syntax()).map(|syntax| syntax.language().name().to_owned()),
    })
}

/// Applies `edits` to buffer `buffer` as one transaction under `origin`,
/// if it's still at plugin version `version`, and returns the version
/// after.
fn edit(
    buffer: u64,
    version: u64,
    edits: Vec<(Range<usize>, String)>,
    origin: Origin,
    cx: &mut App,
) -> Result<u64, Error> {
    let (editor, now) = editor_of(buffer, cx)?;
    if now != version {
        return Err(Error::Conflict(format!(
            "buffer {buffer} is at version {now}, not {version}"
        )));
    }
    editor.update(cx, |view, cx| {
        let model = view.model_mut();
        model.sync_display();
        let mut buffer = model.buffer_mut();
        let base = buffer.snapshot();
        buffer
            .edit_from(&base, edits, origin)
            .map_err(|error| Error::Conflict(error.to_string()))?;
        drop(buffer);
        cx.notify();
        Ok(())
    })?;
    Ok(editor_of(buffer, cx)?.1)
}

/// Shows `proposal` in an editor of buffer `buffer`, beside any shown.
fn propose(buffer: u64, proposal: Proposal, cx: &mut App) -> Result<(), Error> {
    let (editor, _) = editor_of(buffer, cx)?;
    editor.update(cx, |view, cx| {
        let mut proposals = view.proposals().to_vec();
        proposals.push(Rc::new(RefCell::new(proposal.change)));
        view.set_proposals(proposals, cx);
    });
    Ok(())
}

#[cfg(test)]
#[path = "plugin_bridge_tests.rs"]
mod plugin_bridge_tests;
