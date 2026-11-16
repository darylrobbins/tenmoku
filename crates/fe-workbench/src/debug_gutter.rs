//! The debugger in the editors (docs/decisions/r1b9dw3g0g-debugger.md §5,
//! card D6; `docs/design/design-system.md`, Debugger): each file editor's
//! breakpoints over its line numbers, the stopped frame's arrow and band,
//! and a caller frame's.
//!
//! - **Breakpoints** come from the root's [`BreakpointStore`], by the
//!   editor's buffer rows ([`BreakpointStore::breakpoints_in`]), and are
//!   shown again whenever it says they changed or moved. While a session
//!   runs, what its adapter said of each ([`Debugger::verification`], by
//!   the line on disk it was sent on) decides its glyph: unverified is a
//!   ring whose tooltip is the adapter's message, and one the adapter moved
//!   draws on the line it reported, for the session's life only.
//!   Disabled ones are grey whatever the session says.
//! - **Clicks.** A click on a row's number toggles a breakpoint there (on a
//!   moved one's own row, so it goes); the right-click menu offers Add
//!   Breakpoint on a row without one, else Disable or Enable Breakpoint and
//!   Remove Breakpoint. Each goes through the store, which sends the file's
//!   set again while a session runs.
//! - **Frames.** While stopped, the top frame's row gets the current-line
//!   band ([`Tint::DebugCurrent`]) and the filled arrow; a caller frame
//!   selected in the call stack ([`Workbench::band_debug_frame`], D9's
//!   seam) the caller band ([`Tint::DebugFrame`]) and the outlined arrow.
//!   Both go when the session runs on or ends. The bands are decorations,
//!   so they draw without a gutter too.

use std::{
    cell::RefCell,
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
    rc::Rc,
};

use fe_buffer::{Anchor, BufferId, Version};
use fe_dap::Epoch;
use fe_editor::decorations::{Decoration, DecorationKind, DecorationSet, ProviderId, Tint};
use fe_gpui::{
    BreakpointHandlers, BreakpointMark, BreakpointMenuItem, BreakpointState, EditorView, FrameArrow,
};
use gpui::{
    App, AppContext as _, Context, Entity, EntityId, Global, Subscription, WeakEntity, Window,
};
use kiln_l10n::tr;

use crate::{
    Workbench,
    debug_breakpoints::{BreakpointEvent, BreakpointStore},
    debug_session::{Debugger, DebuggerEvent, Verification},
};

/// A window's debugger in its editors: see the [module](self).
pub struct DebugGutter {
    debugger: WeakEntity<Debugger>,
    /// The root whose store it follows.
    root: Option<PathBuf>,
    store: Option<Entity<BreakpointStore>>,
    _store_events: Option<Subscription>,
    editors: HashMap<EntityId, Shown>,
    /// The caller frame D9 selected in the current stop: its canonical
    /// file, row (from 0) and the stop's epoch, which it lasts for.
    caller: Option<(PathBuf, u32, Epoch)>,
    /// By canonical file: the enabled lines on disk as last seen, and those
    /// added or enabled since the session last said what it made of the
    /// file's breakpoints. Until it says again, its verification of those
    /// lines is an older breakpoint's.
    enabled: HashMap<PathBuf, BTreeSet<u32>>,
    pending: HashMap<PathBuf, Pending>,
    _debugger_events: Subscription,
}

/// A file's breakpoints waiting for the session's answer.
#[derive(Default)]
struct Pending {
    lines: BTreeSet<u32>,
    /// The generation of the send they wait on: the file's last when the
    /// store's change was heard by the session, settled once that has
    /// sent. Only the reply to it, or to a later send, answers them; a
    /// `breakpoint` event or an earlier send's reply doesn't, whatever ids
    /// the adapter reuses. `None` until settled.
    waits: Option<u64>,
}

/// An editor's part.
struct Shown {
    editor: WeakEntity<EditorView>,
    /// Its bands, a decoration provider of its model.
    bands: Rc<RefCell<DecorationSet>>,
    provider: ProviderId,
    /// The file and buffer it was last shown for: a Save As, or a buffer
    /// swapped in, shows it again.
    seen: Option<(PathBuf, BufferId)>,
    /// The text version it was last shown at: frame bands follow text
    /// changes even when no breakpoint moves.
    version: Option<Version>,
    /// Where this stop's frames were first placed, as anchors in its
    /// buffer, so a save or a split line leaves them on their text.
    frames: Option<Frames>,
    _editor_changes: Subscription,
}

/// The frames of one stop, anchored in one buffer: each at the start of its
/// row, after text inserted there. They outlive saves, which move the
/// store's baseline, and are rebuilt for a new stop, file or buffer.
#[derive(Clone)]
struct Frames {
    file: PathBuf,
    buffer: BufferId,
    epoch: Epoch,
    stop: Option<Anchor>,
    /// Each caller selected in the stop: its row on disk, and its anchor.
    callers: Vec<(u32, Anchor)>,
}

/// Each window's, by its workbench.
#[derive(Default)]
struct Gutters(HashMap<EntityId, Entity<DebugGutter>>);

impl Global for Gutters {}

/// Shows each window's breakpoints and stops in its editors. Registered
/// after [`crate::debug_session::init`], which makes the debugger, and
/// [`crate::debug_breakpoints::init`], which hands the editors to the
/// store.
pub(crate) fn init(cx: &mut App) {
    cx.observe_new(|workbench: &mut Workbench, window, cx| {
        if window.is_none() {
            return;
        }
        let Some(debugger) = workbench.debugger(cx) else {
            return;
        };
        let gutter = cx.new(|cx| {
            let events = cx.subscribe(&debugger, |this: &mut DebugGutter, _, event, cx| {
                this.heard(event, cx)
            });
            DebugGutter {
                debugger: debugger.downgrade(),
                root: None,
                store: None,
                _store_events: None,
                editors: HashMap::new(),
                caller: None,
                enabled: HashMap::new(),
                pending: HashMap::new(),
                _debugger_events: events,
            }
        });
        cx.observe_self({
            let gutter = gutter.downgrade();
            move |workbench, cx| {
                if let Some(gutter) = gutter.upgrade() {
                    follow(workbench, &gutter, cx);
                }
            }
        })
        .detach();
        follow(workbench, &gutter, cx);
        let id = cx.entity_id();
        cx.default_global::<Gutters>().0.insert(id, gutter);
        cx.on_release(move |_, cx| {
            if cx.has_global::<Gutters>() {
                cx.global_mut::<Gutters>().0.remove(&id);
            }
        })
        .detach();
    })
    .detach();
}

/// The window's root and file editors as `workbench` has them now: a new
/// root's store, editors opened shown and closed ones let go.
fn follow(workbench: &Workbench, gutter: &Entity<DebugGutter>, cx: &mut Context<Workbench>) {
    let root = workbench.root().map(Path::to_owned);
    let editors = workbench.file_editors(cx);
    let moved = gutter.read(cx).root != root;
    let store = match (&root, moved) {
        (Some(root), true) => Some(BreakpointStore::for_root(root, cx)),
        _ => None,
    };
    gutter.update(cx, |gutter, cx| {
        if moved {
            gutter.root = root;
            gutter._store_events = (store.as_ref()).map(|store| {
                cx.subscribe(store, |this: &mut DebugGutter, store, event, cx| {
                    this.store_said(&store, event, cx);
                    this.refresh(cx);
                })
            });
            gutter.store = store;
            gutter.caller = None;
            gutter.refresh(cx);
        }
        gutter.follow_editors(editors, cx);
    });
}

impl DebugGutter {
    /// Shows editors new to it and lets go of those gone.
    fn follow_editors(&mut self, editors: Vec<Entity<EditorView>>, cx: &mut Context<Self>) {
        let gone: Vec<EntityId> = (self.editors.keys())
            .filter(|id| !editors.iter().any(|editor| editor.entity_id() == **id))
            .copied()
            .collect();
        for id in gone {
            let Some(shown) = self.editors.remove(&id) else {
                continue;
            };
            if let Some(editor) = shown.editor.upgrade() {
                editor.update(cx, |view, cx| {
                    view.model_mut().remove_decoration_provider(shown.provider);
                    view.set_breakpoint_handlers(None, cx);
                    let version = view.model().snapshot().version();
                    view.set_breakpoints(version, Vec::new(), cx);
                });
            }
        }
        for editor in editors {
            if self.editors.contains_key(&editor.entity_id()) {
                continue;
            }
            let shown = self.attach(&editor, cx);
            self.editors.insert(editor.entity_id(), shown);
            self.show(editor.entity_id(), cx);
        }
    }

    /// Gives `editor` its bands' provider and the column's handlers.
    fn attach(&mut self, editor: &Entity<EditorView>, cx: &mut Context<Self>) -> Shown {
        let bands = Rc::new(RefCell::new(DecorationSet::new()));
        let me = cx.entity().downgrade();
        let weak = editor.downgrade();
        let on_toggle = {
            let (me, editor) = (me.clone(), weak.clone());
            Rc::new(move |row: u32, _: &mut Window, cx: &mut App| {
                with_store(&me, &editor, cx, |store, path, buffer, cx| {
                    store.toggle_in(path, buffer, row, cx);
                });
            }) as Rc<dyn Fn(u32, &mut Window, &mut App)>
        };
        let menu = {
            let editor = weak.clone();
            Rc::new(move |row: u32, cx: &App| menu_items(&me, &editor, row, cx))
                as Rc<dyn Fn(u32, &App) -> Vec<BreakpointMenuItem>>
        };
        let provider = editor.update(cx, |view, cx| {
            let provider = view.model_mut().add_decoration_provider(bands.clone());
            view.set_breakpoint_handlers(Some(BreakpointHandlers { on_toggle, menu }), cx);
            provider
        });
        // Its file or buffer changing (Save As) shows it again.
        let changes = cx.observe(editor, |this: &mut DebugGutter, editor, cx| {
            let id = editor.entity_id();
            let now = identity(&editor, cx);
            let version = editor.read(cx).model().snapshot().version();
            if (this.editors.get(&id)).is_some_and(|shown| {
                shown.seen != now || (shown.frames.is_some() && shown.version != Some(version))
            }) {
                this.show(id, cx);
            }
        });
        Shown {
            editor: weak,
            bands,
            provider,
            seen: None,
            version: None,
            frames: None,
            _editor_changes: changes,
        }
    }

    fn heard(&mut self, event: &DebuggerEvent, cx: &mut Context<Self>) {
        match event {
            DebuggerEvent::State(_) => {
                // Running on, ended, or a stop retired for another: a caller
                // frame selected in it goes.
                self.caller = None;
                self.refresh(cx);
            }
            DebuggerEvent::Stopped(_) => {
                self.caller = None;
                self.refresh(cx);
            }
            DebuggerEvent::Breakpoints(path) => {
                // Only the reply to the send they wait on (or a later one)
                // answers them: a `breakpoint` event, or an earlier send's
                // reply, leaves them waiting.
                let file = canonical(path);
                let answered = self.debugger.upgrade().is_some_and(|debugger| {
                    let debugger = debugger.read(cx);
                    self.pending.get(&file).is_none_or(|pending| {
                        pending.waits.is_some_and(|waits| {
                            (debugger.answered_generation(&file)).is_some_and(|got| got >= waits)
                        })
                    })
                });
                if answered {
                    self.pending.remove(&file);
                }
                self.refresh(cx);
            }
            // A new session sends every file afresh.
            DebuggerEvent::Starting(_) => self.pending.clear(),
            DebuggerEvent::StartGivenUp | DebuggerEvent::Output(_) => {}
        }
    }

    /// The store changed or saved `event`'s file: lines enabled since last
    /// seen (added, or enabled again) wait for the session's answer, and a
    /// save, which moves the lines on disk, makes them all wait.
    fn store_said(
        &mut self,
        store: &Entity<BreakpointStore>,
        event: &BreakpointEvent,
        cx: &mut Context<Self>,
    ) {
        let (path, saved) = match event {
            BreakpointEvent::Changed(path) => (path, false),
            BreakpointEvent::Saved(path) => (path, true),
            BreakpointEvent::Moved(_) => return,
        };
        let lines: BTreeSet<u32> = (store.read(cx).enabled(path).into_iter())
            .map(|breakpoint| breakpoint.line)
            .collect();
        let file = canonical(path);
        let seen = self.enabled.insert(file.clone(), lines.clone());
        let fresh: Vec<u32> = match (saved, seen) {
            (false, Some(seen)) => lines.difference(&seen).copied().collect(),
            _ => lines.iter().copied().collect(),
        };
        let debugger = self.debugger.upgrade();
        let session = (debugger.as_ref()).is_some_and(|debugger| {
            debugger
                .read(cx)
                .state()
                .is_some_and(|state| !state.is_ended())
        });
        if session && !fresh.is_empty() {
            let pending = self.pending.entry(file.clone()).or_default();
            pending.lines.extend(fresh);
            pending.waits = None;
            // The session sends from its own subscription to the store,
            // before or after this one: settle once both have run.
            let me = cx.entity().downgrade();
            cx.defer(move |cx| {
                me.update(cx, |this, cx| {
                    let last = (this.debugger.upgrade())
                        .and_then(|debugger| debugger.read(cx).awaited_generation(&file));
                    if let Some(pending) = this.pending.get_mut(&file) {
                        pending.waits = Some(last.unwrap_or(0));
                    }
                })
                .ok();
            });
        }
    }

    /// Bands caller frame `frame`, its file and row, or none: see
    /// [`Workbench::band_debug_frame`].
    fn band_caller(&mut self, frame: Option<(&Path, u32)>, cx: &mut Context<Self>) {
        // Only in a stop: its epoch is the selection's life.
        let epoch = (self.debugger.upgrade())
            .and_then(|debugger| Some(debugger.read(cx).stop_location()?.epoch));
        self.caller = frame
            .zip(epoch)
            .map(|((path, row), epoch)| (canonical(path), row, epoch));
        self.refresh(cx);
    }

    /// Shows every editor's marks and bands again.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let ids: Vec<EntityId> = self.editors.keys().copied().collect();
        for id in ids {
            self.show(id, cx);
        }
    }

    /// Shows editor `id`'s breakpoints, frame arrows and bands as the
    /// store and the session have them.
    fn show(&mut self, id: EntityId, cx: &mut Context<Self>) {
        let Some(shown) = self.editors.get_mut(&id) else {
            return;
        };
        let Some(editor) = shown.editor.upgrade() else {
            return;
        };
        shown.seen = identity(&editor, cx);
        shown.version = Some(editor.read(cx).model().snapshot().version());
        let old_frames = shown.frames.clone();
        let Some((path, buffer)) = shown.seen.clone() else {
            return;
        };
        let snapshot = editor.read(cx).model().snapshot().clone();
        let canonical = canonical(&path);
        // Its enabled lines as first seen, which later changes are told
        // from.
        if let Some(store) = &self.store
            && !self.enabled.contains_key(&canonical)
        {
            let lines = (store.read(cx).enabled(&path).into_iter())
                .map(|breakpoint| breakpoint.line)
                .collect();
            self.enabled.insert(canonical.clone(), lines);
        }
        let debugger = self.debugger.upgrade();
        let session = (debugger.as_ref()).and_then(|debugger| {
            let debugger = debugger.read(cx);
            debugger.state().filter(|state| !state.is_ended())?;
            Some(debugger)
        });
        let text = snapshot.text();
        let last = text.row_count().saturating_sub(1);
        let mut marks: Vec<BreakpointMark> = Vec::new();
        // Lines on disk in this buffer's text, as the store places them.
        let in_buffer = |lines: &[u32]| -> Vec<u32> {
            match &self.store {
                Some(store) => store.read(cx).rows_of_lines(&path, buffer, lines),
                None => lines.iter().map(|line| line.saturating_sub(1)).collect(),
            }
        };
        if let Some(store) = &self.store {
            let store = store.read(cx);
            let placements = store.placements_in(&path, buffer);
            let verification =
                session.map_or(&[][..], |debugger| debugger.verification(&canonical));
            // What the adapter said of each enabled one, by the line on disk
            // it was sent on; a disabled one isn't sent, and one added or
            // enabled since the session last said waits for its answer.
            let pending = self.pending.get(&canonical);
            let said: Vec<Option<&Verification>> = (placements.iter())
                .map(|placement| {
                    let waits = pending.is_some_and(|wait| wait.lines.contains(&placement.line));
                    (verification.iter())
                        .find(|said| placement.enabled && !waits && said.line == placement.line)
                })
                .collect();
            // Where the adapter put those it moved, a line on disk, in this
            // buffer's text: as the store places breakpoints.
            let reported: Vec<u32> = (said.iter().flatten())
                .filter_map(|said| said.adapter_line)
                .collect();
            let mut moved = store.rows_of_lines(&path, buffer, &reported).into_iter();
            for (placement, said) in placements.iter().zip(said) {
                let row = placement.row.min(last);
                let state = match (placement.enabled, said) {
                    (false, _) => BreakpointState::Disabled,
                    (true, Some(said)) if !said.verified => BreakpointState::Unverified {
                        message: said.message.clone().map(Into::into),
                    },
                    (true, Some(_)) => BreakpointState::Enabled,
                    // A session that hasn't said yet: not bound yet.
                    (true, None) if session.is_some() => {
                        BreakpointState::Unverified { message: None }
                    }
                    (true, None) => BreakpointState::Enabled,
                };
                // For the session's life, where the adapter put it.
                let drawn = match said.and_then(|said| said.adapter_line) {
                    Some(_) => moved.next().unwrap_or(row).min(last),
                    None => row,
                };
                // A click on one drawn elsewhere acts on its own row; of two
                // drawn on one row, the view shows and acts on the first.
                marks.push(BreakpointMark {
                    row: drawn,
                    breakpoint: Some(state),
                    frame: None,
                    target: (drawn != row).then_some(row),
                });
            }
        }
        // The frames: the stopped thread's top one, and a caller selected
        // in this stop, unless it's on the top one's row.
        let stop =
            (debugger.as_ref()).and_then(|debugger| debugger.read(cx).stop_location().cloned());
        let epoch = stop.as_ref().map(|stop| stop.epoch);
        // Both are lines of the program, the file on disk: placed in this
        // buffer's text as breakpoints are, once for the stop, and held as
        // anchors after that.
        let kept = old_frames.filter(|frames| {
            frames.file == canonical && frames.buffer == buffer && Some(frames.epoch) == epoch
        });
        let anchor_at = |row: u32| -> Anchor {
            let row = in_buffer(&[row + 1])[0].min(last);
            snapshot.anchor_after(text.row_start(row))
        };
        let stop_anchor = (stop.as_ref())
            .filter(|stop| stop.path.as_deref().map(self::canonical) == Some(canonical.clone()))
            .map(|stop| {
                (kept.as_ref())
                    .and_then(|frames| frames.stop)
                    .unwrap_or_else(|| anchor_at(stop.row))
            });
        // Every caller selected in this stop keeps its anchor, so selecting
        // another, or none, and coming back finds it where its text is.
        let mut callers = (kept.as_ref())
            .map(|frames| frames.callers.clone())
            .unwrap_or_default();
        let selected = (self.caller.as_ref())
            .filter(|(file, _, at)| *file == canonical && Some(*at) == epoch)
            .map(|(_, row, _)| *row);
        if let Some(row) = selected
            && !callers.iter().any(|(at, _)| *at == row)
        {
            callers.push((row, anchor_at(row)));
        }
        let row_of =
            |anchor: &Anchor| (text.offset_to_point(snapshot.resolve(anchor)).row).min(last);
        let stop = stop_anchor.as_ref().map(row_of);
        let caller = (selected.and_then(|row| callers.iter().find(|(at, _)| *at == row)))
            .map(|(_, anchor)| row_of(anchor))
            .filter(|row| Some(*row) != stop);
        let frames = epoch.map(|epoch| Frames {
            file: canonical.clone(),
            buffer,
            epoch,
            stop: stop_anchor,
            callers,
        });
        let mut bands = Vec::new();
        for (row, arrow, tint) in [
            (stop, FrameArrow::Current, Tint::DebugCurrent),
            (caller, FrameArrow::Caller, Tint::DebugFrame),
        ] {
            let Some(row) = row else {
                continue;
            };
            marks.push(BreakpointMark {
                row,
                breakpoint: None,
                frame: Some(arrow),
                target: None,
            });
            let start = text.row_start(row);
            let end = start + text.row_len(row) as usize;
            let band = Decoration::new(start..end, DecorationKind::RowBackground, tint);
            bands.push(band);
        }
        let Some(shown) = self.editors.get_mut(&id) else {
            return;
        };
        shown.frames = frames.filter(|frames| frames.stop.is_some() || !frames.callers.is_empty());
        let version = snapshot.version();
        shown.bands.borrow_mut().apply(&snapshot, version, bands);
        editor.update(cx, |view, cx| {
            view.set_breakpoints(version, marks, cx);
            cx.notify();
        });
    }
}

/// `editor`'s file and buffer, if it has a file.
fn identity(editor: &Entity<EditorView>, cx: &App) -> Option<(PathBuf, BufferId)> {
    let model = editor.read(cx).model();
    Some((model.path()?.to_owned(), model.buffer().id()))
}

/// Runs `f` on the store of `gutter`'s root, with `editor`'s file and
/// buffer, if it has both.
fn with_store(
    gutter: &WeakEntity<DebugGutter>,
    editor: &WeakEntity<EditorView>,
    cx: &mut App,
    f: impl FnOnce(&mut BreakpointStore, &Path, BufferId, &mut Context<BreakpointStore>),
) {
    let Some(store) = (gutter.upgrade()).and_then(|gutter| gutter.read(cx).store.clone()) else {
        return;
    };
    let Some(editor) = editor.upgrade() else {
        return;
    };
    let model = editor.read(cx).model();
    let (Some(path), buffer) = (model.path().map(Path::to_owned), model.buffer().id()) else {
        return;
    };
    store.update(cx, |store, cx| f(store, &path, buffer, cx));
}

/// The menu for buffer row `row` of `editor`: Add Breakpoint, or Disable
/// or Enable Breakpoint and Remove Breakpoint.
fn menu_items(
    gutter: &WeakEntity<DebugGutter>,
    editor: &WeakEntity<EditorView>,
    row: u32,
    cx: &App,
) -> Vec<BreakpointMenuItem> {
    let Some(store) = (gutter.upgrade()).and_then(|gutter| gutter.read(cx).store.clone()) else {
        return Vec::new();
    };
    let Some(view) = editor.upgrade() else {
        return Vec::new();
    };
    let model = view.read(cx).model();
    let Some(path) = model.path().map(Path::to_owned) else {
        return Vec::new();
    };
    let buffer = model.buffer().id();
    let here = (store.read(cx).breakpoints_in(&path, buffer).into_iter())
        .find(|breakpoint| breakpoint.line == row + 1);
    let item = |title: gpui::SharedString, act: Act| {
        let (gutter, editor) = (gutter.clone(), editor.clone());
        BreakpointMenuItem {
            title,
            run: Rc::new(move |_, cx| {
                with_store(&gutter, &editor, cx, |store, path, buffer, cx| match act {
                    // Only if there's none there by now: it never removes.
                    Act::Add => {
                        let there = (store.breakpoints_in(path, buffer).iter())
                            .any(|breakpoint| breakpoint.line == row + 1);
                        if !there {
                            store.toggle_in(path, buffer, row, cx);
                        }
                    }
                    Act::Enable(enabled) => {
                        store.set_enabled_in(path, buffer, row, enabled, cx);
                    }
                    Act::Remove => {
                        store.remove_in(path, buffer, row, cx);
                    }
                });
            }),
        }
    };
    match here {
        None => vec![item(tr!("workbench-debug-menu-add-breakpoint"), Act::Add)],
        Some(breakpoint) => {
            let toggle = match breakpoint.enabled {
                true => item(
                    tr!("workbench-debug-menu-disable-breakpoint"),
                    Act::Enable(false),
                ),
                false => item(
                    tr!("workbench-debug-menu-enable-breakpoint"),
                    Act::Enable(true),
                ),
            };
            let remove = item(tr!("workbench-debug-menu-remove-breakpoint"), Act::Remove);
            vec![toggle, remove]
        }
    }
}

/// What a menu item does to its row's breakpoint.
#[derive(Clone, Copy)]
enum Act {
    Add,
    Enable(bool),
    Remove,
}

/// `path` with its symlinks resolved, or as it is if that fails: how the
/// debugger keys its files.
fn canonical(path: &Path) -> PathBuf {
    dunce::canonicalize(path).unwrap_or_else(|_| path.to_owned())
}

impl Workbench {
    /// Bands caller frame `frame`, a file and its row from 0, as the
    /// selected frame of a stopped session's call stack, or takes the band
    /// away (`None`): the caller band and the outlined arrow, until the
    /// session runs on, stops again or ends. The current frame's are the
    /// debugger's own. For the call stack (D9).
    pub fn band_debug_frame(&mut self, frame: Option<(&Path, u32)>, cx: &mut Context<Self>) {
        let Some(gutter) = (cx.try_global::<Gutters>())
            .and_then(|gutters| gutters.0.get(&cx.entity_id()).cloned())
        else {
            return;
        };
        gutter.update(cx, |gutter, cx| gutter.band_caller(frame, cx));
    }
}
