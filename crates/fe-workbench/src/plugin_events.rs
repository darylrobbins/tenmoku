//! The editor's events, sent to the plugins that subscribed to them
//! (kiln-plugin's [`events`](kiln_plugin::events)).
//!
//! - **What.** A buffer opened in a workbench, each change to its text (the
//!   edits as replaced ranges and inserted lengths, and the version after),
//!   its saves and its close, all named by the bridge's buffer ids
//!   ([`crate::plugin_bridge`]), a save naming the version it wrote; the
//!   focused buffer, that of the editor with keyboard focus in the active
//!   window, as it changes, none when focus leaves every buffer or the
//!   window; that editor's selection, as it changes; and each settings or
//!   extensions reload. A plugin loaded while
//!   buffers are open hears each as opened, and which has focus.
//! - **Where.** Each plugin has an [`Inbox`]: its [`EventQueue`], which
//!   takes only the kinds the plugin subscribed to, and a wake for its
//!   thread, which delivers what waits one event per call, between calls
//!   into it ([`crate::plugin_host`]).
//! - **Never waiting.** A push takes the queue's lock to queue the event
//!   and wakes the thread without waiting for it. A full queue drops into
//!   `lagged`, and a buffer's changes, the focus, each buffer's selection
//!   and reloads coalesce as they wait, as the queue does. Nothing here
//!   runs while no plugin is loaded: the buffers are followed only while
//!   one is.

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, MutexGuard, PoisonError, mpsc::SyncSender},
};

use fe_buffer::BufferEvent;
use fe_gpui::EditorView;
use gpui::{App, Entity, EntityId, Focusable as _, Global, Subscription, WeakEntity};
use kiln_plugin::{
    bindings::kiln::extension::{
        events::{BufferOpened, BufferSaved, Event, EventKinds, SelectionChanged},
        types::Range,
    },
    events::{Change, Edit, EventQueue, HostEvent},
    limits::Limits,
    runner::Diagnostic,
};

use crate::{Workbench, plugin_bridge, plugin_host::Work};

/// One plugin's end of the events. Clones share it.
#[derive(Clone)]
pub(crate) struct Inbox(Arc<Shared>);

struct Shared {
    queue: EventQueue,
    /// Wakes the plugin's thread to deliver what waits: none before it
    /// starts and once it's unloaded.
    wake: Mutex<Option<SyncSender<Work>>>,
    /// Tells the host the plugin stopped in an event's call; dropped once
    /// it's unloaded.
    stop: Mutex<Option<async_channel::Sender<Diagnostic>>>,
    stopped: async_channel::Receiver<Diagnostic>,
    /// Under test, while set, the plugin's thread delivers nothing.
    #[cfg(test)]
    held: std::sync::atomic::AtomicBool,
    /// Under test, each event delivered, with the thread it was delivered on.
    #[cfg(test)]
    delivered: Mutex<Vec<(String, Event)>>,
    /// Under test, each event the host pushed, whatever its kind.
    #[cfg(test)]
    pushed: Mutex<Vec<HostEvent>>,
}

impl Inbox {
    /// A plugin's, with a queue holding up to `limits.event_queue` events,
    /// subscribed to none.
    pub(crate) fn new(limits: &Limits) -> Self {
        let (stop, stopped) = async_channel::bounded(1);
        Self(Arc::new(Shared {
            queue: EventQueue::new(limits),
            wake: Mutex::new(None),
            stop: Mutex::new(Some(stop)),
            stopped,
            #[cfg(test)]
            held: Default::default(),
            #[cfg(test)]
            delivered: Default::default(),
            #[cfg(test)]
            pushed: Default::default(),
        }))
    }

    /// Has `wake` wake the plugin's thread from now on.
    pub(crate) fn set_wake(&self, wake: SyncSender<Work>) {
        *lock(&self.0.wake) = Some(wake);
    }

    /// Replaces the kinds of event the plugin receives.
    pub(crate) fn subscribe(&self, kinds: EventKinds) {
        self.0.queue.subscribe(kinds);
    }

    /// Queues `event` if the plugin subscribed to its kind, and wakes its
    /// thread if anything waits. It never waits on the plugin.
    fn push(&self, event: HostEvent) {
        #[cfg(test)]
        lock(&self.0.pushed).push(event.clone());
        self.0.queue.push(event);
        if self.0.queue.is_empty() {
            return;
        }
        if let Some(wake) = lock(&self.0.wake).as_ref() {
            // Full, a wake waits already, or the thread is in a call: it
            // delivers what waits once that's done.
            drop(wake.try_send(Work::Events));
        }
    }

    /// The next event to deliver, `lagged` first if any were dropped.
    pub(crate) fn pop(&self) -> Option<Event> {
        #[cfg(test)]
        while self.0.held.load(std::sync::atomic::Ordering::Acquire) && !self.is_closed() {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        self.0.queue.pop()
    }

    /// The plugin stopped in an event's call, as `diagnostic` says: it's
    /// unloaded, and the host is told.
    pub(crate) fn stop(&self, diagnostic: Diagnostic) {
        let stop = lock(&self.0.stop).take();
        self.close();
        if let Some(stop) = stop {
            drop(stop.try_send(diagnostic));
        }
    }

    /// What the plugin stopped with, if it stops in an event's call: ends
    /// with nothing once it's unloaded otherwise.
    pub(crate) fn stopped(&self) -> async_channel::Receiver<Diagnostic> {
        self.0.stopped.clone()
    }

    /// The plugin is unloaded: its queue empties for good, and nothing
    /// wakes its thread.
    pub(crate) fn close(&self) {
        lock(&self.0.wake).take();
        lock(&self.0.stop).take();
        self.0.queue.close();
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.0.queue.is_closed()
    }

    /// How many events wait, not counting a `lagged` to come.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.0.queue.len()
    }

    /// Holds or lets go of the plugin's deliveries.
    #[cfg(test)]
    pub(crate) fn hold(&self, held: bool) {
        (self.0.held).store(held, std::sync::atomic::Ordering::Release);
    }

    /// Notes `event` delivered on this thread.
    #[cfg(test)]
    pub(crate) fn delivered(&self, event: Event) {
        let thread = std::thread::current().name().unwrap_or_default().to_owned();
        lock(&self.0.delivered).push((thread, event));
    }

    /// The events delivered so far, with the threads they were delivered on.
    #[cfg(test)]
    pub(crate) fn deliveries(&self) -> Vec<(String, Event)> {
        lock(&self.0.delivered).clone()
    }

    /// The events pushed so far, whatever their kind.
    #[cfg(test)]
    pub(crate) fn pushed(&self) -> Vec<HostEvent> {
        lock(&self.0.pushed).clone()
    }
}

/// The plugins' inboxes and what's followed for them.
#[derive(Default)]
struct PluginEvents {
    inboxes: Vec<Inbox>,
    /// The open buffers, while a plugin is loaded.
    buffers: HashMap<fe_buffer::BufferId, Watched>,
    /// The open editors, each observed for its changes and for focus
    /// coming and going: a tab shown takes focus.
    editors: HashMap<EntityId, [Subscription; 3]>,
    /// The focused buffer last sent: none while none was.
    focus: Option<u64>,
    /// The selection last sent: its buffer's id and its ranges.
    selection: Option<(u64, Vec<Range>)>,
}

impl Global for PluginEvents {}

/// An open buffer, followed.
struct Watched {
    id: u64,
    /// Its changes and saves not yet sent.
    events: fe_buffer::Subscription,
    /// An editor of it, for its path.
    editor: WeakEntity<EditorView>,
}

/// Sends `inbox`'s plugin the editor's events from now on: first, each open
/// buffer as opened, and the focused one.
pub(crate) fn add(inbox: Inbox, cx: &mut App) {
    if inbox.is_closed() {
        return;
    }
    // What was followed for plugins all unloaded since is let go first.
    live(cx);
    follow(cx);
    let open = plugin_bridge::open_editors(cx);
    let mut seen = HashSet::new();
    for (id, version, editor) in open {
        if seen.insert(id) {
            inbox.push(HostEvent::BufferOpened(opened(id, version, &editor, cx)));
        }
    }
    let events = cx.default_global::<PluginEvents>();
    if let Some(focus) = events.focus {
        inbox.push(HostEvent::FocusChanged(Some(focus)));
    }
    events.inboxes.push(inbox);
}

/// The settings or the extensions reloaded.
pub(crate) fn config_reloaded(cx: &mut App) {
    if live(cx) {
        push(HostEvent::ConfigReloaded, cx);
    }
}

/// The open editors or the focus may have changed: buffers opened and
/// closed since, and the focus if it moved, are sent.
pub(crate) fn sync(cx: &mut App) {
    if live(cx) {
        follow(cx);
    }
}

/// Sends what changed in the buffers followed and wasn't sent yet: before
/// the bridge reads their versions, so that it counts each the events name.
pub(crate) fn catch_up(cx: &mut App) {
    if !live(cx) {
        return;
    }
    let followed: Vec<_> = cx
        .global::<PluginEvents>()
        .buffers
        .keys()
        .copied()
        .collect();
    for buffer in followed {
        drain(buffer, None, cx);
    }
}

/// Whether a plugin takes events. Once none does, nothing is followed.
fn live(cx: &mut App) -> bool {
    let Some(events) = cx.try_global::<PluginEvents>() else {
        return false;
    };
    if !events.inboxes.iter().any(Inbox::is_closed) {
        return !events.inboxes.is_empty();
    }
    let events = cx.global_mut::<PluginEvents>();
    events.inboxes.retain(|inbox| !inbox.is_closed());
    if events.inboxes.is_empty() {
        *events = PluginEvents::default();
    }
    !events.inboxes.is_empty()
}

fn push(event: HostEvent, cx: &App) {
    let Some(events) = cx.try_global::<PluginEvents>() else {
        return;
    };
    for inbox in &events.inboxes {
        inbox.push(event.clone());
    }
}

/// Catches up with the open editors: what changed in the buffers followed
/// is sent first, then each buffer opened as opened and each closed as
/// closed, the focus if it moved and the focused editor's selection.
fn follow(cx: &mut App) {
    cx.default_global::<PluginEvents>();
    catch_up(cx);
    observe(cx);
    let open = plugin_bridge::open_editors(cx);
    let mut buffers = HashSet::new();
    for (id, version, editor) in &open {
        let buffer = editor.read(cx).model().buffer().id();
        buffers.insert(buffer);
        if let Some(watched) = cx.global_mut::<PluginEvents>().buffers.get_mut(&buffer) {
            // Its editor closed, another still shows it.
            if watched.editor.upgrade().is_none() {
                watched.editor = editor.downgrade();
            }
            continue;
        }
        let subscription = editor.update(cx, |view, _| view.model_mut().buffer_mut().subscribe());
        let watched = Watched {
            id: *id,
            events: subscription,
            editor: editor.downgrade(),
        };
        cx.global_mut::<PluginEvents>()
            .buffers
            .insert(buffer, watched);
        push(
            HostEvent::BufferOpened(opened(*id, *version, editor, cx)),
            cx,
        );
    }
    let events = cx.global_mut::<PluginEvents>();
    let closed: Vec<u64> = (events.buffers)
        .extract_if(|buffer, _| !buffers.contains(buffer))
        .map(|(_, watched)| watched.id)
        .collect();
    for id in closed {
        let events = cx.global_mut::<PluginEvents>();
        if events.selection.as_ref().is_some_and(|(of, _)| *of == id) {
            events.selection = None;
        }
        push(HostEvent::BufferClosed(id), cx);
    }
    if let Some(focus) = focused(cx) {
        let focus = focus.and_then(|editor| {
            let buffer = editor.read(cx).model().buffer().id();
            Some(cx.global::<PluginEvents>().buffers.get(&buffer)?.id)
        });
        let events = cx.global_mut::<PluginEvents>();
        if events.focus != focus {
            events.focus = focus;
            push(HostEvent::FocusChanged(focus), cx);
        }
    }
    selection(cx);
}

/// Observes each workbench window's open editors not yet observed, and
/// stops observing those closed.
fn observe(cx: &mut App) {
    let windows: Vec<_> = (cx.windows().into_iter())
        .filter_map(|window| window.downcast::<Workbench>())
        .collect();
    let mut open = HashSet::new();
    for window in windows {
        let Ok(editors) = window.read(cx).map(|workbench| workbench.file_editors(cx)) else {
            continue;
        };
        for editor in editors {
            let observed = cx
                .global::<PluginEvents>()
                .editors
                .contains_key(&editor.entity_id());
            if observed {
                open.insert(editor.entity_id());
                continue;
            }
            // Off the frame that moved the focus.
            let focus = window.update(cx, |_, window, cx| {
                let handle = editor.focus_handle(cx);
                let gained = window.on_focus_in(&handle, cx, |_, cx| cx.defer(sync));
                let lost = window.on_focus_out(&handle, cx, |_, _, cx| cx.defer(sync));
                [gained, lost]
            });
            let Ok([gained, lost]) = focus else {
                continue;
            };
            let notified = cx.observe(&editor, |editor, cx| changed(&editor, cx));
            open.insert(editor.entity_id());
            (cx.global_mut::<PluginEvents>().editors)
                .insert(editor.entity_id(), [notified, gained, lost]);
        }
    }
    let events = cx.global_mut::<PluginEvents>();
    events.editors.retain(|editor, _| open.contains(editor));
}

/// `editor` notified: what changed in its buffer is sent, and its selection
/// if it has focus.
fn changed(editor: &Entity<EditorView>, cx: &mut App) {
    if !live(cx) {
        return;
    }
    let (buffer, path) = {
        let model = editor.read(cx).model();
        (model.buffer().id(), path_of(model))
    };
    drain(buffer, path, cx);
    selection(cx);
}

/// Sends what changed in followed buffer `buffer`, saved to `path` if it
/// was, or else to its editor's path.
fn drain(buffer: fe_buffer::BufferId, path: Option<String>, cx: &mut App) {
    let Some(watched) = cx.global::<PluginEvents>().buffers.get(&buffer) else {
        return;
    };
    let happened = watched.events.drain();
    if happened.is_empty() {
        return;
    }
    let editor = watched.editor.clone();
    for event in happened {
        match event {
            BufferEvent::Edited { to, edits, .. } => {
                let Some((id, version)) = plugin_bridge::advance(buffer, to, cx) else {
                    continue;
                };
                let edits = (edits.iter())
                    .map(|edit| Edit {
                        replaced: range(&edit.old),
                        inserted: edit.new.len() as u64,
                    })
                    .collect();
                let change = Change {
                    buffer: id,
                    version,
                    edits,
                };
                push(HostEvent::BufferChanged(change), cx);
            }
            BufferEvent::Saved { version } => {
                // Saved once more typing had moved it on: the version the
                // text it wrote had then.
                let Some((id, version)) = plugin_bridge::advance(buffer, version, cx) else {
                    log::debug!("a save of buffer {buffer:?} names a version plugins never had");
                    continue;
                };
                let path = path.clone().or_else(|| {
                    let editor = editor.upgrade()?;
                    path_of(editor.read(cx).model())
                });
                let Some(path) = path else {
                    continue;
                };
                let saved = BufferSaved {
                    buffer: id,
                    version,
                    path,
                };
                push(HostEvent::BufferSaved(saved), cx);
            }
        }
    }
}

/// Sends the focused editor's selection, if it moved since it was last
/// sent. Not while the editor is behind its buffer: it notifies again once
/// it catches up.
fn selection(cx: &mut App) {
    let Some(Some(editor)) = focused(cx) else {
        return;
    };
    let (buffer, version, ranges) = {
        let model = editor.read(cx).model();
        let version = model.buffer().version();
        if model.snapshot().version() != version {
            return;
        }
        // One per cursor, the newest last.
        let primary = model.primary_selection();
        let mut ranges: Vec<Range> = (model.selections().into_iter())
            .filter(|selection| *selection != primary)
            .map(|selection| range(&selection.range))
            .collect();
        ranges.push(range(&primary.range));
        (model.buffer().id(), version, ranges)
    };
    if !cx.global::<PluginEvents>().buffers.contains_key(&buffer) {
        return;
    }
    // Its changes not yet sent go first, each counted at its own version:
    // another editor's notification can bring it here.
    drain(buffer, None, cx);
    let Some((id, version)) = plugin_bridge::advance(buffer, version, cx) else {
        return;
    };
    let events = cx.global_mut::<PluginEvents>();
    if events.selection.as_ref() == Some(&(id, ranges.clone())) {
        return;
    }
    events.selection = Some((id, ranges.clone()));
    let selection = SelectionChanged {
        buffer: id,
        version,
        ranges,
    };
    push(HostEvent::SelectionChanged(selection), cx);
}

/// The editor with keyboard focus: one of the active workbench window's
/// open editors, or none, focus being elsewhere or no workbench window
/// active. None at all while the active window can't be asked: it's being
/// updated.
fn focused(cx: &mut App) -> Option<Option<Entity<EditorView>>> {
    let Some(window) = (cx.active_window()).and_then(|window| window.downcast::<Workbench>())
    else {
        return Some(None);
    };
    let editors = window.read(cx).ok()?.file_editors(cx);
    let found = window.update(cx, |_, window, cx| {
        (editors.into_iter()).find(|editor| editor.focus_handle(cx).contains_focused(window, cx))
    });
    found.ok()
}

/// Buffer `id`, at plugin version `version`, as `editor` shows it.
fn opened(id: u64, version: u64, editor: &Entity<EditorView>, cx: &App) -> BufferOpened {
    let view = editor.read(cx);
    BufferOpened {
        buffer: id,
        version,
        path: path_of(view.model()),
        language: (view.syntax()).map(|syntax| syntax.language().name().to_owned()),
    }
}

fn path_of(model: &fe_editor::EditorModel) -> Option<String> {
    model.path().map(|path| path.display().to_string())
}

fn range(range: &std::ops::Range<usize>) -> Range {
    Range {
        start: range.start as u64,
        end: range.end as u64,
    }
}

fn lock<T: ?Sized>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
#[path = "plugin_events_tests.rs"]
mod plugin_events_tests;
