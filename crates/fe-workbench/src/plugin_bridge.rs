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
//!   `Script`) of an editor command runs on the active editor of the
//!   active window as a [`RunCommand`]. The editor's registry authorizes it
//!   against the [`CommandHost`] as its `PluginPolicy`
//!   ([`fe_gpui::commands::set_plugin_policy`]); the refusal a plugin hears
//!   is the same registry's, checked first, naming what it lacks.
//!   `editor.save`, though granted, is refused for an untitled buffer, for
//!   a file outside the workspace roots (where it leads, symlinks
//!   followed, dangling ones included) and while a save of the buffer is
//!   under way; it runs without the save hook (format on save), so that
//!   the file it writes is the one checked. One of the workbench
//!   commands open to plugins ([`crate::plugin_access`]) is authorized by
//!   that module's registry and runs as its typed command in the active
//!   window. Any other workbench command, and any plugin's own command, is
//!   refused.

use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet, VecDeque},
    ops::Range,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError},
    time::{Duration, Instant},
};

use fe_buffer::{BufferId, BufferSnapshot, Origin, Version};
use fe_command::{
    Actor, CommandError, DispatchError, Invocation, PluginDenied, PluginPolicy, Registry,
};
use fe_editor::commands::Editor;
use fe_gpui::{EditorView, commands::RunCommand};
use fe_plugin::editor::{BufferAccess, BufferText, Proposal};
use gpui::{AnyWindowHandle, App, Entity, Focusable as _, Global};
use kiln_l10n::tr;
use kiln_plugin::{
    bindings::Error,
    commands::{CommandHost, Dispatcher, Request},
    process::ProgramBudget,
};

use crate::{
    Workbench,
    plugin_access::{self, Forward, WorkbenchCommands},
};

/// The editor command a plugin runs only on a file inside the workspace.
const SAVE: &str = "editor.save";

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
            return Err(Error::Failed(
                tr!("workbench-extensions-bridge-closed").into(),
            ));
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
                        let message = match started {
                            true => tr!("workbench-extensions-bridge-unfinished"),
                            false => tr!("workbench-extensions-bridge-unanswered"),
                        };
                        return Err(Error::Failed(message.into()));
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
    let served = Served::new(Arc::new(policy.clone()), cx);
    cx.spawn(async move |cx| {
        while let Ok(job) = jobs.0.recv().await {
            cx.update(|cx| job(&served, cx));
        }
    })
    .detach();
}

/// What the main thread runs plugins' jobs with.
pub(crate) struct Served {
    /// What plugins may do: their grants and the workspace.
    policy: Arc<dyn PluginPolicy>,
    /// The editor's commands, to check a plugin's invocation against before
    /// it runs.
    registry: Registry<Editor>,
    /// The workbench's commands open to plugins, which check a plugin's
    /// invocation and run it.
    workbench: Registry<WorkbenchCommands>,
    /// Whether an invocation of a plugin's ran since it was last cleared.
    ran: Rc<Cell<bool>>,
}

impl Served {
    /// Checks plugins' invocations against `policy`, which the editor's
    /// registry checks them against too from now on.
    pub(crate) fn new(policy: Arc<dyn PluginPolicy>, cx: &mut App) -> Self {
        fe_gpui::commands::set_plugin_policy(Arc::clone(&policy));
        let mut registry = fe_editor::commands::registry();
        registry.set_plugin_policy(Arc::clone(&policy));
        let mut workbench = plugin_access::registry();
        workbench.set_plugin_policy(Arc::clone(&policy));
        let ran = Rc::new(Cell::new(false));
        let heard = Rc::clone(&ran);
        fe_gpui::commands::observe_invocations(cx, move |invocation| {
            if matches!(invocation.actor, Actor::Host(_)) {
                heard.set(true);
            }
        });
        Self {
            policy,
            registry,
            workbench,
            ran,
        }
    }

    /// Runs `invocation`: a workbench command open to plugins in the active
    /// window, an editor command on the active editor of the active window,
    /// if the registry it's in lets the plugin run it there.
    pub(crate) fn run(&self, invocation: &Invocation, cx: &mut App) -> Result<(), DispatchError> {
        let command = &invocation.command;
        let failed = |message: &str| DispatchError::Failed {
            command: command.clone(),
            error: CommandError(message.to_owned()),
        };
        if self.workbench.get(command).is_some() {
            let window = (cx.active_window())
                .and_then(|window| window.downcast::<Workbench>())
                .ok_or_else(|| failed(&tr!("workbench-extensions-bridge-no-window")))?;
            return AnyWindowHandle::from(window)
                .update(cx, |_, window, cx| {
                    let mut forward = Forward {
                        invocation: invocation.clone(),
                        window,
                        cx,
                    };
                    self.workbench.dispatch(&mut forward, invocation).map(drop)
                })
                .map_err(|_| failed(&tr!("workbench-extensions-bridge-window-closed")))?;
        }
        if self.registry.get(command).is_none()
            && let Actor::Host(plugin) = &invocation.actor
        {
            // Another workbench command, or a plugin's.
            let typed = kiln::platform::command_actions::typed_command(command, cx).is_some();
            if typed || plugin_access::is_plugin_command(&*self.policy, command) {
                return Err(plugin_access::undeclared(plugin, command).into());
            }
        }
        let invocation = invocation.clone().context(fe_editor::commands::context());
        self.registry.validate(&invocation)?;
        let window = (cx.active_window())
            .and_then(|window| window.downcast::<Workbench>())
            .ok_or_else(|| failed(&tr!("workbench-extensions-bridge-no-window")))?;
        let editor = (window.read(cx).ok())
            .and_then(|workbench| workbench.active_editor(cx).cloned())
            .ok_or_else(|| failed(&tr!("workbench-extensions-bridge-no-editor")))?;
        // A plugin's save is checked against the file it writes: none queued
        // before it, which would write wherever the buffer's path is by
        // then, and no save hook, so that the save takes its path now.
        let saves = command.as_str() == SAVE;
        let mut hook = None;
        if saves {
            let view = editor.read(cx);
            if view.is_saving() {
                return Err(failed(&tr!("workbench-extensions-bridge-saving")));
            }
            let path = view.model().path().map(Path::to_owned);
            self.may_save(&invocation, path.as_deref())?;
            hook = editor.update(cx, |view, _| {
                let hook = view.save_hook().cloned();
                view.set_save_hook(None);
                hook
            });
        }
        self.ran.set(false);
        let run = RunCommand(invocation.clone());
        let dispatched = window.update(cx, |_, window, cx| {
            editor.focus_handle(cx).dispatch_action(&run, window, cx);
        });
        if hook.is_some() {
            // Put back, unless the host set another meanwhile.
            editor.update(cx, |view, _| {
                if view.save_hook().is_none() {
                    view.set_save_hook(hook);
                }
            });
        }
        dispatched.map_err(|_| failed(&tr!("workbench-extensions-bridge-window-closed")))?;
        match self.ran.take() {
            true => Ok(()),
            false => Err(failed(&tr!("workbench-extensions-bridge-not-run"))),
        }
    }

    /// Refuses a plugin's [`SAVE`] of the buffer at `path` unless it's a
    /// file inside a workspace root: not an untitled buffer, which saving
    /// would ask the user where to put, nor a file the plugin's
    /// `fs.write:workspace` doesn't reach.
    fn may_save(&self, invocation: &Invocation, path: Option<&Path>) -> Result<(), PluginDenied> {
        let Actor::Host(plugin) = &invocation.actor else {
            return Ok(());
        };
        let Some(path) = path else {
            return Err(PluginDenied::MissingPath {
                plugin: plugin.clone(),
                command: invocation.command.clone(),
                arg: "the buffer's file",
            });
        };
        let roots: Vec<PathBuf> = (self.policy.workspace_roots().iter())
            .filter_map(|root| resolved(root))
            .collect();
        let inside =
            resolved(path).is_some_and(|path| roots.iter().any(|root| path.starts_with(root)));
        if inside {
            return Ok(());
        }
        Err(PluginDenied::OutsideWorkspace {
            plugin: plugin.clone(),
            command: invocation.command.clone(),
            arg: "the buffer's file",
            path: path.display().to_string(),
        })
    }
}

/// The most symlinks [`resolved`] follows in one path, as Linux's
/// `MAXSYMLINKS`: more is a loop, or as good as one.
const MAX_SYMLINKS: u32 = 40;

/// Where `path` leads, as a write to it would: symlinks followed one
/// component at a time as the OS follows them, so `..` after a symlink
/// leaves where it leads, and a symlink whose target doesn't exist yet,
/// which a save would create, counts as its target. A component that
/// doesn't exist is taken as named. `None` for a relative path, a symlink
/// loop and anything that can't be read, so that a file that can't be
/// checked isn't saved. As kiln's check of plugins' path args resolves
/// them.
fn resolved(path: &Path) -> Option<PathBuf> {
    use std::path::Component;
    if !path.is_absolute() {
        return None;
    }
    let owned = |component: Component<'_>| PathBuf::from(component.as_os_str());
    let mut rest: VecDeque<PathBuf> = path.components().map(owned).collect();
    let mut resolved = PathBuf::new();
    let mut symlinks = 0;
    while let Some(component) = rest.pop_front() {
        match component.components().next() {
            None | Some(Component::CurDir) => {}
            Some(Component::ParentDir) => {
                resolved.pop();
            }
            // An absolute symlink target starts over from its root.
            Some(Component::RootDir | Component::Prefix(_)) => resolved.push(&component),
            Some(Component::Normal(_)) => {
                let next = resolved.join(&component);
                match std::fs::symlink_metadata(&next) {
                    Ok(meta) if meta.file_type().is_symlink() => {
                        symlinks += 1;
                        if symlinks > MAX_SYMLINKS {
                            return None;
                        }
                        // Its target's components come next, from the
                        // symlink's folder.
                        let target = std::fs::read_link(&next).ok()?;
                        for component in target.components().rev() {
                            rest.push_front(owned(component));
                        }
                    }
                    Ok(_) => resolved = next,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        resolved = next;
                    }
                    Err(_) => return None,
                }
            }
        }
    }
    Some(resolved)
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
    /// The buffer's version when plugins first heard of it: no older
    /// version has a plugin version.
    first: Version,
}

impl Tracked {
    /// Takes the buffer at `version`, newer than any seen, as its next
    /// plugin version.
    fn bump(&mut self, version: Version) {
        self.seen = version;
        self.version += 1;
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
                Tracked {
                    id: ids.last,
                    seen: version,
                    version: 1,
                    first: version,
                }
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
/// it was given, such as a save's that finished after more typing: each
/// batch of edits since is one plugin version, counted in `snapshot`, of the
/// buffer at `version` or later. None if it was never given one,
/// or the buffer no longer keeps its edits back that far.
pub(crate) fn advance(
    buffer: BufferId,
    version: Version,
    snapshot: Option<&BufferSnapshot>,
    cx: &mut App,
) -> Option<(u64, u64)> {
    let tracked = cx.default_global::<BufferIds>().open.get_mut(&buffer)?;
    if version > tracked.seen {
        tracked.bump(version);
        return Some((tracked.id, tracked.version));
    }
    if version == tracked.seen {
        return Some((tracked.id, tracked.version));
    }
    let snapshot = snapshot?;
    if version < tracked.first || version < snapshot.oldest_version() {
        return None;
    }
    let behind = snapshot.edits_since(version).count() - snapshot.edits_since(tracked.seen).count();
    let given = tracked
        .version
        .checked_sub(behind as u64)
        .filter(|given| *given >= 1)?;
    Some((tracked.id, given))
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
        .ok_or_else(|| {
            let message = tr!(
                "workbench-extensions-bridge-no-buffer",
                buffer = buffer.to_string()
            );
            Error::NotFound(message.into())
        })
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
        return Err(Error::Conflict(
            tr!(
                "workbench-extensions-bridge-stale-version",
                buffer = buffer.to_string(),
                now = now.to_string(),
                version = version.to_string()
            )
            .into(),
        ));
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
