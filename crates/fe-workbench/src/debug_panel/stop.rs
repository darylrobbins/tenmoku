//! What the Debug tool window shows of one stop: its threads and frames,
//! the selected frame's scopes, the children fetched and the watches'
//! values, and the requests that fetch them. Everything here is of one
//! stop epoch: a reply that comes after its stop is over changes nothing,
//! and the next stop starts afresh.

use std::collections::HashMap;

use fe_dap::{
    Epoch, FRAME_PAGE, RequestError, StopRef,
    protocol::{
        EvaluateArguments, EvaluateBody, Scope, StackFrame, Variable, VariablesArguments, request,
    },
};
use gpui::{Context, SharedString, Task, Window};
use kiln_l10n::{tr, verbatim};

use super::{DebugPanel, NodePath, PAGE, ValueRow, frame_path};
use crate::debug_session::StopLocation;

/// A request's answer, while it's awaited, and once it came.
#[derive(Clone, Debug, Default)]
pub(crate) enum Fetch<T> {
    #[default]
    Pending,
    Ready(T),
    Failed(SharedString),
}

impl<T> Fetch<T> {
    pub(crate) fn ready(&self) -> Option<&T> {
        match self {
            Fetch::Ready(value) => Some(value),
            _ => None,
        }
    }
}

/// One thread of the stop.
pub(crate) struct ThreadEntry {
    pub(crate) id: i64,
    pub(crate) name: String,
    /// Its frames: the stopped thread's from the stop, another's once it
    /// was expanded.
    pub(crate) frames: Option<Fetch<Vec<StackFrame>>>,
    /// How many frames the adapter says it has, when it says: optional,
    /// and maybe more than it has.
    pub(crate) total: Option<usize>,
    /// A page came back short: there are no more, whatever `total` says.
    pub(crate) ended: bool,
    /// A page of frames past those fetched is on its way.
    pub(crate) loading_more: bool,
    pub(crate) expanded: bool,
}

impl ThreadEntry {
    fn fetched(&self) -> Option<usize> {
        (self.frames.as_ref()).and_then(Fetch::ready).map(Vec::len)
    }

    /// Whether it may have frames past those fetched: no page came back
    /// short, and the total, if the adapter gave one, is past them. With no
    /// total, until a short page says it's all.
    pub(crate) fn has_more(&self) -> bool {
        let Some(fetched) = self.fetched() else {
            return false;
        };
        !self.ended && self.total.is_none_or(|total| fetched < total)
    }

    /// How many more frames the adapter says it has, if it said.
    pub(crate) fn remaining(&self) -> Option<usize> {
        let fetched = self.fetched()?;
        self.total.map(|total| total.saturating_sub(fetched))
    }

    /// Takes in a page of `received` frames, asked for [`FRAME_PAGE`] of
    /// them: a short one is the stack's end.
    fn page_came(&mut self, received: usize, total: Option<i64>) {
        if let Some(total) = total.and_then(|total| usize::try_from(total).ok()) {
            self.total = Some(total);
        }
        if received < FRAME_PAGE as usize {
            self.ended = true;
        }
    }
}

/// How many children the adapter says a node has, of each kind, when it
/// says (`indexedVariables`, `namedVariables`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Counts {
    pub(crate) indexed: Option<i64>,
    pub(crate) named: Option<i64>,
}

/// What one `variables` request asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ask {
    /// Every child: a node that isn't paged.
    All,
    /// A paged node's named children, all at once.
    Named,
    /// A paged node's indexed children from this one, a page of them.
    Indexed(usize),
}

/// A node's children, fetched in this stop.
#[derive(Default)]
pub(crate) struct Children {
    /// How many indexed children the adapter counts, when it's past a page
    /// and so asked for a page at a time.
    pub(crate) paged: Option<usize>,
    /// A paged node's named children, shown before its indexed ones.
    pub(crate) named: Vec<Variable>,
    /// Its children, or a paged node's indexed ones as far as fetched.
    pub(crate) variables: Vec<Variable>,
    /// The first requests not yet answered.
    waiting: usize,
    /// How many of them show.
    pub(crate) shown: usize,
    pub(crate) state: Fetch<()>,
    /// A page beyond the first is on its way.
    pub(crate) loading: bool,
}

impl Children {
    /// How many there are in all, as far as known.
    pub(crate) fn total(&self) -> usize {
        self.paged.unwrap_or(self.variables.len())
    }
}

/// Holds each page of frames, once answered, until the test sends.
#[cfg(test)]
pub(crate) struct FramePageGate(pub(crate) async_channel::Receiver<()>);

#[cfg(test)]
impl gpui::Global for FramePageGate {}

/// The stop shown.
pub(crate) struct Stop {
    pub(crate) epoch: Epoch,
    /// The thread that stopped.
    pub(crate) thread_id: i64,
    /// Why, as the adapter said.
    pub(crate) reason: Option<String>,
    pub(crate) threads: Vec<ThreadEntry>,
    /// The selected frame: its thread and its index in that thread's
    /// frames, the top one 0.
    pub(crate) selected: (i64, usize),
    /// The selected frame's scopes.
    pub(crate) scopes: Fetch<Vec<Scope>>,
    /// By `variablesReference`.
    pub(crate) children: HashMap<i64, Children>,
    /// The watches' values in the selected frame, by expression.
    pub(crate) watches: HashMap<String, Fetch<EvaluateBody>>,
    /// The requests under way: dropped with the stop, or with the frame
    /// they're of.
    pub(crate) frame_tasks: Vec<Task<()>>,
    pub(crate) tasks: Vec<Task<()>>,
}

impl Stop {
    pub(crate) fn thread(&self, id: i64) -> Option<&ThreadEntry> {
        self.threads.iter().find(|thread| thread.id == id)
    }

    /// The frames of thread `id`, as far as they came.
    pub(crate) fn frames(&self, id: i64) -> &[StackFrame] {
        (self.thread(id))
            .and_then(|thread| thread.frames.as_ref()?.ready())
            .map_or(&[], Vec::as_slice)
    }

    pub(crate) fn selected_frame(&self) -> Option<&StackFrame> {
        let (thread, index) = self.selected;
        self.frames(thread).get(index)
    }
}

/// What a failed request shows: the adapter's message if it gave one.
pub(super) fn failure(error: &RequestError) -> SharedString {
    match error.message() {
        Some(message) if !message.is_empty() => message.to_owned().into(),
        _ => error.to_string().into(),
    }
}

impl DebugPanel {
    /// The program stopped there: the stop's threads, its thread's frames
    /// (the first page the session fetched), its top frame selected, and
    /// that frame's scopes and watches asked for.
    pub(super) fn stopped(&mut self, location: &StopLocation, cx: &mut Context<Self>) {
        let Some(debugger) = self.debugger() else {
            return;
        };
        let debugger = debugger.read(cx);
        let Some(session) = debugger.session() else {
            return;
        };
        let data = session
            .stop_data()
            .filter(|data| data.epoch == location.epoch);
        let frames = (data.as_ref())
            .map(|data| data.frames.clone())
            .filter(|frames| !frames.is_empty())
            .unwrap_or_else(|| vec![location.frame.clone()]);
        let total = (data.as_ref()).and_then(|data| data.total_frames);
        let reason = (data.as_ref())
            .and_then(|data| Some(data.stopped.as_ref()?.reason.clone()))
            .filter(|reason| !reason.is_empty());
        let mut threads: Vec<ThreadEntry> = (session.threads().into_iter())
            .map(|thread| ThreadEntry {
                id: thread.id,
                name: thread.name,
                frames: None,
                total: None,
                ended: false,
                loading_more: false,
                expanded: false,
            })
            .collect();
        if !threads.iter().any(|thread| thread.id == location.thread_id) {
            threads.insert(
                0,
                ThreadEntry {
                    id: location.thread_id,
                    name: tr!("workbench-debug-panel-thread", id = location.thread_id).into(),
                    frames: None,
                    total: None,
                    ended: false,
                    loading_more: false,
                    expanded: false,
                },
            );
        }
        for thread in &mut threads {
            if thread.id == location.thread_id {
                thread.frames = Some(Fetch::Ready(frames.clone()));
                thread.page_came(frames.len(), total);
                thread.expanded = true;
            }
        }
        self.stop = Some(Stop {
            epoch: location.epoch,
            thread_id: location.thread_id,
            reason,
            threads,
            selected: (location.thread_id, 0),
            scopes: Fetch::Pending,
            children: HashMap::new(),
            watches: HashMap::new(),
            frame_tasks: Vec::new(),
            tasks: Vec::new(),
        });
        self.rescope(cx);
    }

    /// The session, while the stop shown is still its current one.
    pub(super) fn live_session(
        &self,
        cx: &Context<Self>,
    ) -> Option<(Epoch, gpui::Entity<crate::debug_session::Debugger>)> {
        let stop = self.stop.as_ref()?;
        let debugger = self.debugger()?;
        let current = debugger.read(cx).stop_location()?.epoch;
        (current == stop.epoch).then_some((stop.epoch, debugger))
    }

    /// Asks for the selected frame's scopes and its watches' values, the
    /// last frame's requests given up.
    pub(super) fn rescope(&mut self, cx: &mut Context<Self>) {
        let Some((epoch, debugger)) = self.live_session(cx) else {
            return;
        };
        let Some(stop) = self.stop.as_mut() else {
            return;
        };
        stop.scopes = Fetch::Pending;
        stop.watches.clear();
        stop.frame_tasks.clear();
        let Some(frame) = stop.selected_frame().map(|frame| frame.id) else {
            stop.scopes = Fetch::Ready(Vec::new());
            return;
        };
        let request =
            (debugger.read(cx).session()).map(|session| session.scopes(StopRef::new(epoch, frame)));
        let Some(request) = request else {
            return;
        };
        let task = cx.spawn(async move |panel, cx| {
            let reply = request.await;
            panel
                .update(cx, |panel, cx| {
                    let Some(stop) = panel.stop.as_mut().filter(|stop| stop.epoch == epoch) else {
                        return;
                    };
                    if stop.selected_frame().map(|frame| frame.id) != Some(frame) {
                        return;
                    }
                    stop.scopes = match reply {
                        Ok(body) => Fetch::Ready(body.scopes),
                        Err(error) => Fetch::Failed(failure(&error)),
                    };
                    panel.fetch_expanded(cx);
                    cx.notify();
                })
                .ok();
        });
        stop.frame_tasks.push(task);
        self.evaluate_watches(cx);
    }

    /// Evaluates each watch not yet evaluated in the selected frame, with
    /// context `watch`.
    pub(super) fn evaluate_watches(&mut self, cx: &mut Context<Self>) {
        let Some((epoch, debugger)) = self.live_session(cx) else {
            return;
        };
        let watches = self.watches.clone();
        let Some(stop) = self.stop.as_mut() else {
            return;
        };
        let Some(frame) = stop.selected_frame().map(|frame| frame.id) else {
            return;
        };
        for expression in watches {
            if stop.watches.contains_key(&expression) {
                continue;
            }
            let arguments = EvaluateArguments {
                expression: expression.clone(),
                context: Some(verbatim!("watch").into()),
                ..Default::default()
            };
            let request = (debugger.read(cx).session())
                .map(|session| session.evaluate(&arguments, Some(StopRef::new(epoch, frame))));
            let Some(request) = request else {
                return;
            };
            stop.watches.insert(expression.clone(), Fetch::Pending);
            let task = cx.spawn(async move |panel, cx| {
                let reply = request.await;
                panel
                    .update(cx, |panel, cx| {
                        let Some(stop) = panel.stop.as_mut().filter(|stop| stop.epoch == epoch)
                        else {
                            return;
                        };
                        if stop.selected_frame().map(|frame| frame.id) != Some(frame) {
                            return;
                        }
                        let value = match reply {
                            Ok(body) => Fetch::Ready(body),
                            Err(error) => Fetch::Failed(failure(&error)),
                        };
                        stop.watches.insert(expression, value);
                        panel.fetch_expanded(cx);
                        cx.notify();
                    })
                    .ok();
            });
            stop.frame_tasks.push(task);
        }
    }

    /// Fetches the children of every node shown expanded whose children
    /// this stop hasn't fetched.
    pub(super) fn fetch_expanded(&mut self, cx: &mut Context<Self>) {
        let (_, mut missing) = self.watch_rows();
        missing.extend(self.variable_rows().1);
        missing.extend(self.console_rows().1);
        for (reference, counts) in missing {
            self.fetch_children(reference, counts, cx);
        }
    }

    /// Fetches `reference`'s children if this stop hasn't: if the adapter
    /// counts more indexed children than a page, its named children and the
    /// first page of the indexed ones, else all of them.
    pub(super) fn fetch_children(
        &mut self,
        reference: i64,
        counts: Counts,
        cx: &mut Context<Self>,
    ) {
        let Some(stop) = self.stop.as_mut() else {
            return;
        };
        if reference <= 0 || stop.children.contains_key(&reference) {
            return;
        }
        let count = |count: Option<i64>| count.and_then(|count| usize::try_from(count).ok());
        let paged = count(counts.indexed).filter(|&n| n > PAGE);
        let asks = match paged {
            // Named children too, unless the adapter says there are none: the
            // count is optional.
            Some(_) if counts.named != Some(0) => {
                vec![Ask::Named, Ask::Indexed(0)]
            }
            Some(_) => vec![Ask::Indexed(0)],
            None => vec![Ask::All],
        };
        stop.children.insert(
            reference,
            Children {
                paged,
                shown: PAGE,
                waiting: asks.len(),
                ..Children::default()
            },
        );
        for ask in asks {
            self.fetch_page(reference, ask, cx);
        }
    }

    /// Asks for `reference`'s children as `ask` says.
    fn fetch_page(&mut self, reference: i64, ask: Ask, cx: &mut Context<Self>) {
        let Some((epoch, debugger)) = self.live_session(cx) else {
            return;
        };
        let Some(stop) = self.stop.as_mut() else {
            return;
        };
        let Some(paged) = stop.children.get(&reference).map(|children| children.paged) else {
            return;
        };
        let debugger = debugger.read(cx);
        let Some(session) = debugger.session() else {
            return;
        };
        let filtered = |filter: &str, start: Option<usize>, count: Option<usize>| {
            let arguments = VariablesArguments {
                variables_reference: reference,
                filter: Some(filter.to_owned()),
                start: start.map(|start| start as i64),
                count: count.map(|count| count as i64),
                ..Default::default()
            };
            // A stop's data, bound to the epoch it's asked in: the stop's,
            // checked just above.
            session.request::<request::Variables>(&arguments)
        };
        let request = match ask {
            Ask::All => session.variables(StopRef::new(epoch, reference)),
            Ask::Named => filtered(verbatim!("named"), None, None),
            Ask::Indexed(start) => {
                let count = PAGE.min(paged.unwrap_or(0).saturating_sub(start));
                filtered(verbatim!("indexed"), Some(start), Some(count))
            }
        };
        let task = cx.spawn(async move |panel, cx| {
            let reply = request.await;
            panel
                .update(cx, |panel, cx| {
                    let Some(stop) = panel.stop.as_mut().filter(|stop| stop.epoch == epoch) else {
                        return;
                    };
                    let Some(children) = stop.children.get_mut(&reference) else {
                        return;
                    };
                    children.waiting = children.waiting.saturating_sub(1);
                    if !matches!(ask, Ask::Named) {
                        children.loading = false;
                    }
                    match reply {
                        Ok(body) if ask == Ask::Named => children.named.extend(body.variables),
                        Ok(body) => children.variables.extend(body.variables),
                        Err(error) => children.state = Fetch::Failed(failure(&error)),
                    }
                    if children.waiting == 0 && matches!(children.state, Fetch::Pending) {
                        children.state = Fetch::Ready(());
                    }
                    panel.fetch_expanded(cx);
                    cx.notify();
                })
                .ok();
        });
        stop.tasks.push(task);
    }

    /// Expands or collapses the node of `row`, fetching its children the
    /// first time this stop.
    pub(crate) fn toggle_node(&mut self, row: &ValueRow, cx: &mut Context<Self>) {
        let Some(path) = row.path() else {
            return;
        };
        let expanded = !row.expanded();
        self.expanded.insert(path.clone(), expanded);
        if expanded && let Some((reference, counts)) = row.reference() {
            self.fetch_children(reference, counts, cx);
        }
        cx.notify();
    }

    /// A "Show 100 more" row was clicked: the next page of its parent's
    /// children shows, asked for if they're paged.
    pub(crate) fn show_more(&mut self, row: &ValueRow, cx: &mut Context<Self>) {
        let ValueRow::More { reference, .. } = row else {
            return;
        };
        let reference = *reference;
        let Some(stop) = self.stop.as_mut() else {
            return;
        };
        let Some(children) = stop.children.get_mut(&reference) else {
            return;
        };
        children.shown += PAGE;
        let start = children.variables.len();
        let fetch = children.paged.is_some_and(|total| start < total) && !children.loading;
        if fetch {
            children.loading = true;
            self.fetch_page(reference, Ask::Indexed(start), cx);
        }
        cx.notify();
    }

    /// Expands or collapses thread `id`, fetching its frames the first
    /// time.
    pub(crate) fn toggle_thread(&mut self, id: i64, cx: &mut Context<Self>) {
        let live = self.live_session(cx);
        let Some(stop) = self.stop.as_mut() else {
            return;
        };
        let epoch = stop.epoch;
        let Some(thread) = stop.threads.iter_mut().find(|thread| thread.id == id) else {
            return;
        };
        thread.expanded = !thread.expanded;
        if !thread.expanded || thread.frames.is_some() {
            return cx.notify();
        }
        let Some((_, debugger)) = live else {
            return;
        };
        let request = (debugger.read(cx).session()).map(|session| session.frames(epoch, id, 0));
        let Some(request) = request else {
            return;
        };
        thread.frames = Some(Fetch::Pending);
        let task = cx.spawn(async move |panel, cx| {
            let reply = request.await;
            panel
                .update(cx, |panel, cx| {
                    let Some(stop) = panel.stop.as_mut().filter(|stop| stop.epoch == epoch) else {
                        return;
                    };
                    let Some(thread) = stop.threads.iter_mut().find(|thread| thread.id == id)
                    else {
                        return;
                    };
                    thread.frames = Some(match reply {
                        Ok(body) => {
                            thread.page_came(body.stack_frames.len(), body.total_frames);
                            Fetch::Ready(body.stack_frames)
                        }
                        Err(error) => Fetch::Failed(failure(&error)),
                    });
                    cx.notify();
                })
                .ok();
        });
        stop.tasks.push(task);
        cx.notify();
    }

    /// Selects frame `index` of thread `thread`, or reveals it again if
    /// it's selected: opens its file at its line, bands a caller's row and
    /// says which frame it is; a new selection re-scopes Variables and
    /// Watch. The opening and the band wait for the workbench's update to
    /// end, and are dropped if another selection, or another stop, comes
    /// first; a selection of a frame with no file gives up the last one's
    /// opening still under way.
    pub(crate) fn select_frame(
        &mut self,
        thread: i64,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(stop) = self.stop.as_mut() else {
            return;
        };
        let Some(frame) = stop.frames(thread).get(index).cloned() else {
            return;
        };
        let count = (stop.thread(thread))
            .and_then(|thread| thread.total)
            .unwrap_or(stop.frames(thread).len());
        let changed = stop.selected != (thread, index);
        stop.selected = (thread, index);
        let (epoch, top) = (stop.epoch, thread == stop.thread_id && index == 0);
        self.frame_selection += 1;
        let selection = self.frame_selection;
        let line = u32::try_from(frame.line.max(1)).unwrap_or(1);
        let path = frame_path(&frame);
        let band = path.clone().filter(|_| !top);
        let panel = cx.weak_entity();
        // Selected from the tool window, the keys stay in it.
        let keep = self
            .focus
            .contains_focused(window, cx)
            .then(|| self.focus.clone());
        self.with_workbench(window, cx, move |workbench, window, cx| {
            let current = panel.upgrade().is_some_and(|panel| {
                let panel = panel.read(cx);
                panel.frame_selection == selection
                    && panel.stop.as_ref().is_some_and(|stop| stop.epoch == epoch)
            });
            if !current {
                return;
            }
            workbench.band_debug_frame(band.as_deref().map(|path| (path, line - 1)), cx);
            workbench.show_debug_frame(epoch, path, line, keep, window, cx);
        });
        let said = tr!(
            "workbench-debug-panel-frame-selected",
            index = index + 1,
            count = count,
            function = frame.name.clone(),
            line = line
        );
        fe_ui::announce(cx, said, fe_ui::Politeness::Polite);
        if changed {
            self.rescope(cx);
        }
        cx.notify();
    }

    /// Moves the selected frame `by` within its thread: 1 to the caller, -1
    /// to the callee. Past the frames fetched, it asks for the next page
    /// and selects the caller once it comes.
    pub(crate) fn move_frame(&mut self, by: isize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(stop) = self.stop.as_ref() else {
            return;
        };
        let (thread, index) = stop.selected;
        let Some(next) = index.checked_add_signed(by) else {
            return;
        };
        if next < stop.frames(thread).len() {
            return self.select_frame(thread, next, window, cx);
        }
        let more = stop.thread(thread).is_some_and(ThreadEntry::has_more);
        if more {
            self.fetch_more_frames(thread, Some(next), window, cx);
        }
    }

    /// Asks for thread `thread`'s next page of frames, if it has more and
    /// isn't asking already; once they come, selects frame `then`, if it's
    /// still the stop and nothing else was selected meanwhile.
    pub(crate) fn fetch_more_frames(
        &mut self,
        thread: i64,
        then: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((epoch, debugger)) = self.live_session(cx) else {
            return;
        };
        let Some(stop) = self.stop.as_mut() else {
            return;
        };
        // The selection when asked: a selection since, even of the same
        // frame, outranks the frame asked for.
        let (selected, selection) = (stop.selected, self.frame_selection);
        let Some(entry) = stop.threads.iter_mut().find(|entry| entry.id == thread) else {
            return;
        };
        if entry.loading_more || !entry.has_more() {
            return;
        }
        let start = (entry.frames.as_ref())
            .and_then(Fetch::ready)
            .map_or(0, Vec::len);
        let request = (debugger.read(cx).session())
            .map(|session| session.frames(epoch, thread, start as i64));
        let Some(request) = request else {
            return;
        };
        entry.loading_more = true;
        #[cfg(test)]
        let gate = cx.try_global::<FramePageGate>().map(|gate| gate.0.clone());
        let task = cx.spawn_in(window, async move |panel, cx| {
            let reply = request.await;
            #[cfg(test)]
            if let Some(gate) = gate {
                gate.recv().await.ok();
            }
            panel
                .update_in(cx, |panel, window, cx| {
                    let Some(stop) = panel.stop.as_mut().filter(|stop| stop.epoch == epoch) else {
                        return;
                    };
                    let Some(entry) = stop.threads.iter_mut().find(|entry| entry.id == thread)
                    else {
                        return;
                    };
                    entry.loading_more = false;
                    let Ok(body) = reply else {
                        // Asked for no more: the row offers them again.
                        return cx.notify();
                    };
                    entry.page_came(body.stack_frames.len(), body.total_frames);
                    if let Some(Fetch::Ready(frames)) = entry.frames.as_mut() {
                        frames.extend(body.stack_frames);
                    }
                    let unchanged = stop.selected == selected && panel.frame_selection == selection;
                    if let Some(then) = then.filter(|_| unchanged) {
                        panel.select_frame(thread, then, window, cx);
                    }
                    cx.notify();
                })
                .ok();
        });
        stop.tasks.push(task);
        cx.notify();
    }

    /// The selected frame, its thread and index, while stopped.
    #[cfg(test)]
    pub(crate) fn selected_frame(&self) -> Option<(i64, usize)> {
        self.stop.as_ref().map(|stop| stop.selected)
    }

    /// A watch row's node path.
    pub(super) fn watch_path(expression: &str) -> NodePath {
        vec![verbatim!("watch").into(), expression.to_owned().into()]
    }

    /// A scope's node path.
    pub(super) fn scope_path(name: &str) -> NodePath {
        vec![verbatim!("scope").into(), name.to_owned().into()]
    }
}
