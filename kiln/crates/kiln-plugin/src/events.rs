// SPDX-License-Identifier: Apache-2.0

//! Events the host delivers to plugins.
//!
//! Each plugin has an [`EventQueue`]. The host [`push`](EventQueue::push)es onto it from any
//! thread, the UI thread among them, and a push never waits on the plugin: delivery holds the
//! queue's lock only to pop an event, never across a call into the plugin. A push can wait for
//! another push or a pop to let go of the lock, and each holds it for at most a queue's worth of
//! work. It is not wait-free. The queue coalesces what the plugin has not yet received: consecutive changes to one buffer merge into
//! one, and focus, selection (per buffer) and config reloads keep only the latest. Opens, saves
//! and closes are never merged, and each buffer's events keep their order. When the queue is full
//! the oldest event that coalescing would have absorbed goes first, and the plugin next receives
//! `lagged` with how many went.
//!
//! A [`Recipient`] owns the plugin beside its queue: it hands the plugin one event per call into
//! it, in order, under the call's deadline, and closes the queue as soon as the plugin unloads or
//! drops, so that the queue empties and every later push does nothing.
//!
//! Events carry buffer ids, versions and ranges, never text.
//!
//! The `subscribe` import is still a stub, [`unimplemented`]: its store state has no handle on
//! the plugin's queue yet. Until it does, the host sets what the queue takes with
//! [`EventQueue::subscribe`].

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use crate::bindings::kiln::extension::events::{
    BufferChanged, BufferOpened, BufferSaved, Event, EventKinds, Host, SelectionChanged,
};
use crate::bindings::kiln::extension::types::{BufferId, Range, Version};
use crate::bindings::{Error, HostState, unimplemented};
use crate::limits::Limits;
use crate::runner::{Call, CallError, Plugin, PluginState, Reply};

/// The most ranges a merged change lists. Past it, the change lists none: the plugin reads the
/// buffer again rather than patching it.
pub const MAX_RANGES: usize = 64;

/// The events slice of [`HostState`].
pub struct State;

impl Host for HostState {
    fn subscribe(&mut self, _kinds: EventKinds) -> Result<(), Error> {
        unimplemented("kiln:extension/events.subscribe")
    }
}

/// One edit of a change: the bytes it `replaced`, in the text before the change, and how many
/// bytes it `inserted` in their place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Edit {
    pub replaced: Range,
    pub inserted: u64,
}

/// A buffer's change, as the host pushes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub buffer: BufferId,
    /// The version after the change.
    pub version: Version,
    /// In order and apart: each edit's `replaced` ends at or before the next one's starts. A
    /// change whose edits are not reaches the plugin as one to read again.
    pub edits: Vec<Edit>,
}

/// What the host pushes onto an [`EventQueue`]: an [`Event`] but `lagged`, which only the queue
/// makes, with a change naming its edits so that a later change can map its ranges.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostEvent {
    BufferOpened(BufferOpened),
    BufferChanged(Change),
    BufferSaved(BufferSaved),
    BufferClosed(BufferId),
    FocusChanged(Option<BufferId>),
    SelectionChanged(SelectionChanged),
    ConfigReloaded,
}

impl HostEvent {
    fn kind(&self) -> EventKinds {
        match self {
            Self::BufferOpened(_) => EventKinds::BUFFER_OPENED,
            Self::BufferChanged(_) => EventKinds::BUFFER_CHANGED,
            Self::BufferSaved(_) => EventKinds::BUFFER_SAVED,
            Self::BufferClosed(_) => EventKinds::BUFFER_CLOSED,
            Self::FocusChanged(_) => EventKinds::FOCUS_CHANGED,
            Self::SelectionChanged(_) => EventKinds::SELECTION_CHANGED,
            Self::ConfigReloaded => EventKinds::CONFIG_RELOADED,
        }
    }
}

/// One plugin's events, waiting for it. Clones share the queue.
#[derive(Clone)]
pub struct EventQueue {
    inner: Arc<Mutex<Queue>>,
}

/// Everything that waits for the plugin, under the one lock: no event waits anywhere else, so
/// `capacity` bounds them all.
struct Queue {
    events: VecDeque<Event>,
    capacity: usize,
    kinds: EventKinds,
    /// How many events went since the plugin last received `lagged`.
    dropped: u64,
    /// Whether the plugin is unloaded.
    closed: bool,
}

impl EventQueue {
    /// A queue holding up to `limits.event_queue` events, subscribed to none. At zero it holds
    /// none, and every push does nothing.
    pub fn new(limits: &Limits) -> Self {
        let capacity = limits.event_queue;
        Self {
            inner: Arc::new(Mutex::new(Queue {
                events: VecDeque::with_capacity(capacity),
                capacity,
                kinds: EventKinds::empty(),
                dropped: 0,
                closed: false,
            })),
        }
    }

    /// Replaces the kinds of event the queue takes. Events already waiting stay.
    pub fn subscribe(&self, kinds: EventKinds) {
        self.lock().kinds = kinds;
    }

    /// Queues `event` for the plugin, if it subscribed to its kind and is not unloaded. It never
    /// waits on the plugin, and never grows the queue past its capacity.
    ///
    /// A change the plugin did not subscribe to leaves a gap in its buffer's history, which no
    /// later change merges across: see [`Queue::gap`].
    pub fn push(&self, event: HostEvent) {
        let mut queue = self.lock();
        if queue.closed || queue.capacity == 0 {
            return;
        }
        if queue.kinds.contains(event.kind()) {
            queue.push(event);
        } else if let HostEvent::BufferChanged(change) = event {
            queue.gap(&change);
        }
    }

    /// Takes the next event: `lagged` first if any went since the last one.
    pub fn pop(&self) -> Option<Event> {
        let mut queue = self.lock();
        if queue.dropped > 0 {
            return Some(Event::Lagged(std::mem::take(&mut queue.dropped)));
        }
        queue.events.pop_front()
    }

    /// How many events wait, not counting a `lagged` to come.
    pub fn len(&self) -> usize {
        self.lock().events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whether the plugin is unloaded, so the queue takes nothing.
    pub fn is_closed(&self) -> bool {
        self.lock().closed
    }

    /// Empties the queue for good: the plugin is unloaded.
    pub fn close(&self) {
        let mut queue = self.lock();
        queue.closed = true;
        queue.dropped = 0;
        queue.events = VecDeque::new();
    }

    fn lock(&self) -> MutexGuard<'_, Queue> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A plugin and its [`EventQueue`]. Every call into the plugin goes through here, so the queue
/// closes as soon as a call unloads the plugin, or the plugin drops.
pub struct Recipient {
    plugin: Plugin,
    queue: EventQueue,
}

impl Recipient {
    /// `plugin`, loaded under `limits`, with a queue subscribed to none.
    pub fn new(plugin: Plugin, limits: &Limits) -> Self {
        let queue = EventQueue::new(limits);
        if matches!(plugin.state(), PluginState::Unloaded(_)) {
            queue.close();
        }
        Self { plugin, queue }
    }

    /// The plugin's queue, for the host to push onto.
    pub fn events(&self) -> EventQueue {
        self.queue.clone()
    }

    pub fn plugin(&self) -> &Plugin {
        &self.plugin
    }

    /// [`Plugin::call`], closing the queue if the call unloads the plugin.
    pub fn call(&mut self, call: Call) -> Result<Reply, CallError> {
        let returned = self.plugin.call(call);
        if matches!(self.plugin.state(), PluginState::Unloaded(_)) {
            self.queue.close();
        }
        returned
    }

    /// Delivers the next event to the plugin in a call of its own, returning the event, or none
    /// if none waits. An unloaded plugin gets nothing.
    pub fn deliver(&mut self) -> Result<Option<Event>, CallError> {
        if self.queue.is_closed() {
            return Err(CallError::Unloaded);
        }
        let Some(event) = self.queue.pop() else {
            return Ok(None);
        };
        self.call(Call::HandleEvents(vec![event.clone()]))?;
        Ok(Some(event))
    }
}

impl Drop for Recipient {
    fn drop(&mut self) {
        self.queue.close();
    }
}

impl Queue {
    fn push(&mut self, event: HostEvent) {
        let event = match event {
            HostEvent::BufferChanged(change) => match self.merge(change) {
                Ok(()) => return,
                Err(change) => Event::BufferChanged(BufferChanged {
                    buffer: change.buffer,
                    version: change.version,
                    ranges: merge(None, &change.edits),
                }),
            },
            HostEvent::FocusChanged(focus) => {
                self.events
                    .retain(|waiting| !matches!(waiting, Event::FocusChanged(_)));
                Event::FocusChanged(focus)
            }
            HostEvent::SelectionChanged(selection) => {
                let buffer = selection.buffer;
                self.events
                    .retain_mut(|waiting| selection_of(waiting, buffer).is_none());
                Event::SelectionChanged(selection)
            }
            HostEvent::ConfigReloaded => {
                self.events
                    .retain(|waiting| !matches!(waiting, Event::ConfigReloaded));
                Event::ConfigReloaded
            }
            HostEvent::BufferOpened(opened) => Event::BufferOpened(opened),
            HostEvent::BufferSaved(saved) => Event::BufferSaved(saved),
            HostEvent::BufferClosed(buffer) => Event::BufferClosed(buffer),
        };
        if self.events.len() >= self.capacity {
            let oldest = self.events.iter().position(coalescible).unwrap_or_default();
            self.events.remove(oldest);
            self.dropped = self.dropped.saturating_add(1);
        }
        self.events.push_back(event);
    }

    /// The waiting change of `buffer`: the last open, change, save or close of it waiting, if
    /// that is a change, and where it waits.
    fn waiting_change(&self, buffer: BufferId) -> Option<usize> {
        let at = self.events.iter().rposition(|waiting| match waiting {
            Event::BufferOpened(BufferOpened { buffer: of, .. })
            | Event::BufferChanged(BufferChanged { buffer: of, .. })
            | Event::BufferSaved(BufferSaved { buffer: of, .. })
            | Event::BufferClosed(of) => *of == buffer,
            _ => false,
        })?;
        matches!(self.events[at], Event::BufferChanged(_)).then_some(at)
    }

    /// Merges `change` into its buffer's [waiting change](Self::waiting_change), if any. A
    /// selection of the buffer waiting after that change moves with it: its ranges are mapped
    /// through `change` to its version. If `change`'s edits cannot map them, or there are more
    /// than [`MAX_RANGES`] of them to map, the change does not merge, so that it still follows
    /// the selection.
    fn merge(&mut self, change: Change) -> Result<(), Change> {
        let Some(at) = self.waiting_change(change.buffer) else {
            return Err(change);
        };
        let mappable = mappable(&change.edits);
        let mut events = self.events.range_mut(at..);
        let Some(Event::BufferChanged(waiting)) = events.next() else {
            return Err(change);
        };
        let selection = events.find_map(|waiting| selection_of(waiting, change.buffer));
        if selection
            .as_ref()
            .is_some_and(|selection| !mappable || selection.ranges.len() > MAX_RANGES)
        {
            return Err(change);
        }
        if let Some(selection) = selection {
            selection.version = change.version;
            for range in &mut selection.ranges {
                *range = map_range(*range, &change.edits);
            }
        }
        waiting.version = change.version;
        waiting.ranges = merge(Some(&waiting.ranges), &change.edits);
        Ok(())
    }

    /// `change` happened while the plugin was not subscribed to changes. A later change must not
    /// merge across it, mapping ranges through only its own edits: the buffer's waiting change
    /// becomes one to read again, at `change`'s version, and a selection waiting after it,
    /// which nothing can carry across the gap, goes and counts toward the next `lagged`.
    fn gap(&mut self, change: &Change) {
        let Some(at) = self.waiting_change(change.buffer) else {
            return;
        };
        let mut events = self.events.range_mut(at..);
        if let Some(Event::BufferChanged(waiting)) = events.next() {
            waiting.version = change.version;
            waiting.ranges = Vec::new();
        }
        if let Some(selected) = (at + 1..self.events.len())
            .find(|&at| selection_of(&mut self.events[at], change.buffer).is_some())
        {
            self.events.remove(selected);
            self.dropped = self.dropped.saturating_add(1);
        }
    }
}

/// `event`, if it is a selection of `buffer`.
fn selection_of(event: &mut Event, buffer: BufferId) -> Option<&mut SelectionChanged> {
    match event {
        Event::SelectionChanged(selection) if selection.buffer == buffer => Some(selection),
        _ => None,
    }
}

/// Whether a later event could have absorbed `event`, so that it is the first to go when the
/// queue is full.
fn coalescible(event: &Event) -> bool {
    matches!(
        event,
        Event::BufferChanged(_)
            | Event::FocusChanged(_)
            | Event::SelectionChanged(_)
            | Event::ConfigReloaded
    )
}

/// Whether `edits` are few enough to map ranges through, and in order and apart.
fn mappable(edits: &[Edit]) -> bool {
    !edits.is_empty()
        && edits.len() <= MAX_RANGES
        && edits
            .iter()
            .all(|edit| edit.replaced.start <= edit.replaced.end)
        && edits
            .windows(2)
            .all(|pair| pair[0].replaced.end <= pair[1].replaced.start)
}

/// What changed across a change and any `earlier` ones it merges into, in the text after it:
/// the ranges `earlier` ones left mapped through `edits`, joined with what `edits` inserted, in
/// order, any that touch merged. Empty, for the plugin to read the buffer again, if `earlier`
/// already is, if `edits` are not [`mappable`], or past [`MAX_RANGES`]. It allocates for at most
/// twice that many ranges, however many edits there are.
fn merge(earlier: Option<&[Range]>, edits: &[Edit]) -> Vec<Range> {
    if !mappable(edits) || earlier.is_some_and(<[Range]>::is_empty) {
        return Vec::new();
    }
    let earlier = earlier.unwrap_or_default();
    let mut ranges = Vec::with_capacity(edits.len() + earlier.len());
    let mut delta = 0i128;
    for edit in edits {
        let start = shift(edit.replaced.start, delta);
        ranges.push(Range {
            start,
            end: start.saturating_add(edit.inserted),
        });
        delta += growth(edit);
    }
    ranges.extend(earlier.iter().map(|range| map_range(*range, edits)));
    ranges.sort_unstable_by_key(|range| range.start);
    let mut joined: Vec<Range> = Vec::with_capacity(ranges.len().min(MAX_RANGES));
    for range in ranges {
        if let Some(last) = joined.last_mut()
            && range.start <= last.end
        {
            last.end = last.end.max(range.end);
        } else if joined.len() == MAX_RANGES {
            return Vec::new();
        } else {
            joined.push(range);
        }
    }
    joined
}

/// `range` in the text before `edits`, in the text after them. Within a replaced span, a start
/// moves to where its insertion starts and an end to where it ends; an empty range, a bare
/// cursor, moves as a start does.
fn map_range(range: Range, edits: &[Edit]) -> Range {
    let start = map_start(range.start, edits);
    if range.start == range.end {
        return Range { start, end: start };
    }
    Range {
        start,
        end: map_end(range.end, edits).max(start),
    }
}

/// How many bytes longer `edit` left the text.
fn growth(edit: &Edit) -> i128 {
    i128::from(edit.inserted) - i128::from(edit.replaced.end - edit.replaced.start)
}

fn shift(at: u64, delta: i128) -> u64 {
    u64::try_from((i128::from(at) + delta).max(0)).unwrap_or(u64::MAX)
}

/// Where a range starting at `at` in the text before `edits` starts after them: within a
/// replaced span, where its insertion starts.
fn map_start(at: u64, edits: &[Edit]) -> u64 {
    let mut delta = 0i128;
    for edit in edits {
        if at < edit.replaced.start {
            break;
        }
        if at < edit.replaced.end {
            return shift(edit.replaced.start, delta);
        }
        delta += growth(edit);
    }
    shift(at, delta)
}

/// Where a range ending at `at` in the text before `edits` ends after them: within a replaced
/// span, where its insertion ends.
fn map_end(at: u64, edits: &[Edit]) -> u64 {
    let mut delta = 0i128;
    for edit in edits {
        if at <= edit.replaced.start {
            break;
        }
        if at <= edit.replaced.end {
            return shift(edit.replaced.start, delta).saturating_add(edit.inserted);
        }
        delta += growth(edit);
    }
    shift(at, delta)
}
