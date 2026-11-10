// SPDX-License-Identifier: Apache-2.0

//! Saved layouts: what a window shows, saved as it changes and restored
//! when the app opens again, through kiln-core's [`LayoutStore`] (a file
//! per root folder, an entry per window, each window's id claimed by one
//! live window at a time).
//!
//! A window's entry is a JSON object the host builds from kiln's parts and
//! its own:
//!
//! - `bounds`, the window's ([`save_bounds`], [`restore_bounds`]);
//! - `panes`, the pane tree ([`save_panes`]): each axis's way and its
//!   members' ratios, each pane's items and its active one, and the
//!   active pane by its path in the tree. An item is `{kind, document,
//!   state}`: its [`PaneItem::KIND`], a number shared by the items of the
//!   window that show one document, and what [`PaneItem::serialize`] gave,
//!   and `pinned: true` if it's pinned ([`PaneView::pin_tab`]): an entry
//!   saved without it restores its items unpinned. An item that gives
//!   nothing is left out;
//! - `dock`, a dock's state ([`DockState::save`](crate::DockState::save));
//! - whatever else the host keeps, such as whether its breadcrumbs show.
//!
//! Restoring ([`restore_panes`]) rebuilds the tree, then the items: for
//! each document, its first item through its kind's restore
//! ([`register_item`]), and the others as that item's
//! [`split`](PaneItem::split), each given its own state with
//! [`apply_state`](PaneItem::apply_state), so they share the document
//! again. An unregistered kind, or a restore that gives nothing (a file
//! that's gone), drops that document's items.
//!
//! A host's [`WindowLayout`] saves the window's entry through one
//! background [`Writer`]: when its shape (the entry without the items'
//! states and the bounds, [`shape`]) changes, and in full when the app quits. When the
//! window closes while others stay open, its entry is deleted; closing the
//! last window, or quitting, keeps it. Its claim is released after its
//! last write.
//!
//! A launch on the folder restores only the last session's windows
//! ([`LayoutStore::restore_last_session`]), at most [`MAX_RESTORED`]; older
//! sessions' windows are deleted. A file launch's window marks its entries
//! `"restore": false`, so no later launch restores it.
//!
//! A launch on no folder in particular can reopen the windows the app last
//! quit with, across roots ([`launch_last_quit`]), each on the folder it was
//! on, at most [`MAX_RESTORED`] in all.

use std::{
    collections::{HashMap, HashSet},
    future::Future,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

use gpui::{App, Bounds, Entity, Global, Task, Window, WindowBounds, point, px, size};
use kiln_command::Value;
pub use kiln_core::layout::{Claim, LayoutStore};

use crate::{
    ItemHandle, PaneItem, PaneView,
    pane_group::{Axis, Member, PaneGroup},
    persist::Writer,
};

/// The most windows one folder launch reopens: a safety net.
pub const MAX_RESTORED: usize = 8;

/// Rebuilds an item of a kind from its saved state.
pub type Restore = Rc<dyn Fn(Value, &mut Window, &mut App) -> Restoring>;

/// The kinds of item that restore, and the layout writer.
#[derive(Default)]
struct Layouts {
    kinds: HashMap<&'static str, Restore>,
    writer: Option<Writer<(PathBuf, u64), Job>>,
    quitting: bool,
}

impl Global for Layouts {}

/// Has saved items of `T`'s kind restore through [`PaneItem::restore`].
pub fn register_item<T: PaneItem>(cx: &mut App) {
    register_kind(
        T::KIND,
        Rc::new(|state, window, cx| {
            let restored = T::restore(state, window, cx);
            cx.spawn(async move |_| {
                let item = restored.await?;
                Some(Box::new(item) as Box<dyn ItemHandle>)
            })
        }),
        cx,
    );
}

/// Has saved items of `kind` restore through `restore`.
pub fn register_kind(kind: &'static str, restore: Restore, cx: &mut App) {
    cx.default_global::<Layouts>().kinds.insert(kind, restore);
}

/// How saved items of `kind` restore, if it's registered.
pub fn restorer(kind: &str, cx: &App) -> Option<Restore> {
    cx.try_global::<Layouts>()?.kinds.get(kind).cloned()
}

/// `bounds`, saved: `{state, x, y, width, height}`.
pub fn save_bounds(bounds: WindowBounds) -> Value {
    let (state, bounds) = match bounds {
        WindowBounds::Windowed(bounds) => ("windowed", bounds),
        WindowBounds::Maximized(bounds) => ("maximized", bounds),
        WindowBounds::Fullscreen(bounds) => ("fullscreen", bounds),
    };
    Value::from_iter([
        ("state", Value::from(state)),
        ("x", Value::from(f64::from(f32::from(bounds.origin.x)))),
        ("y", Value::from(f64::from(f32::from(bounds.origin.y)))),
        (
            "width",
            Value::from(f64::from(f32::from(bounds.size.width))),
        ),
        (
            "height",
            Value::from(f64::from(f32::from(bounds.size.height))),
        ),
    ])
}

/// The bounds [`save_bounds`] saved, if they're whole and of a usable
/// size.
pub fn restore_bounds(saved: &Value) -> Option<WindowBounds> {
    let number = |name: &str| saved.get(name)?.as_f64().map(|value| value as f32);
    let (width, height) = (number("width")?, number("height")?);
    if !(width >= 100. && height >= 100.) {
        return None;
    }
    let bounds = Bounds {
        origin: point(px(number("x")?), px(number("y")?)),
        size: size(px(width), px(height)),
    };
    Some(match saved.get("state")?.as_str()? {
        "maximized" => WindowBounds::Maximized(bounds),
        "fullscreen" => WindowBounds::Fullscreen(bounds),
        _ => WindowBounds::Windowed(bounds),
    })
}

/// The pane tree of `group`, `active` its active pane, saved: see the
/// module docs.
pub fn save_panes<V: PaneView>(group: &PaneGroup<V>, active: &Entity<V>, cx: &App) -> Value {
    let mut documents: Vec<Box<dyn ItemHandle>> = Vec::new();
    let tree = save_member(group.root(), &mut documents, cx);
    let path = group.path_of(active).unwrap_or_default();
    Value::from_iter([("tree", tree), ("active", Value::from(path))])
}

fn save_member<V: PaneView>(
    member: &Member<V>,
    documents: &mut Vec<Box<dyn ItemHandle>>,
    cx: &App,
) -> Value {
    match member {
        Member::Pane(pane) => {
            let state = pane.read(cx).pane();
            let mut active = 0;
            let mut items = Vec::new();
            for (ix, item) in state.items().iter().enumerate() {
                let Some(saved) = item.serialize(cx) else {
                    continue;
                };
                if state.active() == Some(ix) {
                    active = items.len();
                }
                let document = (documents.iter())
                    .position(|first| first.id() == item.id() || first.shares_document(&**item, cx))
                    .unwrap_or_else(|| {
                        documents.push(item.boxed_clone());
                        documents.len() - 1
                    });
                let mut saved = Value::from_iter([
                    ("kind", Value::from(item.kind())),
                    ("document", Value::from(document)),
                    ("state", saved),
                ]);
                if state.is_pinned(ix)
                    && let Some(object) = saved.as_object_mut()
                {
                    object.insert("pinned".into(), Value::Bool(true));
                }
                items.push(saved);
            }
            let pane = Value::from_iter([
                ("items", Value::Array(items)),
                ("active", Value::from(active)),
            ]);
            Value::from_iter([("pane", pane)])
        }
        Member::Axis { axis, members } => {
            let axis = match axis {
                Axis::Horizontal => "horizontal",
                Axis::Vertical => "vertical",
            };
            let saved = members
                .iter()
                .map(|member| save_member(member, documents, cx));
            let ratios = members.ratios().iter().map(|ratio| f64::from(*ratio));
            Value::from_iter([
                ("axis", Value::from(axis)),
                ("ratios", Value::from_iter(ratios)),
                ("members", Value::from_iter(saved)),
            ])
        }
    }
}

/// `entry` without what changes without the layout changing: the window's
/// `bounds`, at the top, and each pane item's `state` (an item being an
/// object with a `kind`). A panel's own state, under a saved dock's
/// `panels` ([`DockState::save`](crate::DockState::save)), is shape
/// whatever its keys.
pub fn shape(entry: &Value) -> Value {
    fn strip(value: &mut Value) {
        match value {
            Value::Object(object) => {
                if object.contains_key("kind") {
                    object.remove("state");
                }
                for (key, value) in object.iter_mut() {
                    if key != "panels" {
                        strip(value);
                    }
                }
            }
            Value::Array(values) => values.iter_mut().for_each(strip),
            _ => {}
        }
    }
    let mut shape = entry.clone();
    if let Some(object) = shape.as_object_mut() {
        object.remove("bounds");
    }
    strip(&mut shape);
    shape
}

/// A pane tree rebuilt from [`save_panes`]' output: its panes, made but
/// empty, and a task that restores their items, resolving to the panes it
/// left empty.
pub struct RestoredPanes<V> {
    pub root: Member<V>,
    pub active: Entity<V>,
    pub filled: Task<Vec<Entity<V>>>,
}

/// An item being restored.
type Restoring = Task<Option<Box<dyn ItemHandle>>>;

/// Makes a saved pane, empty, recording its items.
type MakePane<'a, V> = dyn FnMut(&Value, &mut Window, &mut App) -> Option<Entity<V>> + 'a;

/// An item to restore, where it goes.
struct Slot {
    pane: usize,
    kind: String,
    document: u64,
    state: Value,
    pinned: bool,
}

/// Rebuilds the pane tree `saved` describes, making each pane with
/// `new_pane`, and restores the items into them in the background (see the
/// module docs): each pane's, in order, its saved active item shown,
/// without focus. `None` if `saved` holds no tree.
pub fn restore_panes<V: PaneView>(
    saved: &Value,
    new_pane: &mut dyn FnMut(&mut Window, &mut App) -> Entity<V>,
    window: &mut Window,
    cx: &mut App,
) -> Option<RestoredPanes<V>> {
    restore_panes_with(saved, new_pane, &mut |_, _| {}, window, cx)
}

/// [`restore_panes`], calling `before_items` once the tree is rebuilt and
/// before any item is restored: not at all if `saved` holds no tree. A host
/// puts back state of its own there that its items need.
pub fn restore_panes_with<V: PaneView>(
    saved: &Value,
    new_pane: &mut dyn FnMut(&mut Window, &mut App) -> Entity<V>,
    before_items: &mut dyn FnMut(&mut Window, &mut App),
    window: &mut Window,
    cx: &mut App,
) -> Option<RestoredPanes<V>> {
    let mut panes = Vec::new();
    let mut slots = Vec::new();
    let mut actives = Vec::new();
    let root = restore_member(
        saved.get("tree")?,
        &mut |saved, window, cx| {
            let pane = new_pane(window, cx);
            let ix = panes.len();
            panes.push(pane.clone());
            let items = saved.get("items").and_then(Value::as_array);
            for item in items.into_iter().flatten() {
                let (Some(kind), Some(document)) = (
                    item.get("kind").and_then(Value::as_str),
                    item.get("document").and_then(Value::as_u64),
                ) else {
                    continue;
                };
                slots.push(Slot {
                    pane: ix,
                    kind: kind.to_owned(),
                    document,
                    state: item.get("state").cloned().unwrap_or_default(),
                    pinned: item.get("pinned").and_then(Value::as_bool) == Some(true),
                });
            }
            let active = saved.get("active").and_then(Value::as_u64);
            actives.push(active.unwrap_or(0) as usize);
            Some(pane)
        },
        window,
        cx,
    )?;
    let path: Vec<usize> = (saved.get("active").and_then(Value::as_array))
        .into_iter()
        .flatten()
        .filter_map(|ix| Some(ix.as_u64()? as usize))
        .collect();
    let active = pane_at(&root, &path).unwrap_or_else(|| panes[0].clone());
    before_items(window, cx);

    // Each document's first item, restored through its kind.
    let mut firsts: Vec<(u64, Option<Restoring>)> = Vec::new();
    for slot in &slots {
        if firsts
            .iter()
            .any(|(document, _)| *document == slot.document)
        {
            continue;
        }
        let restore = restorer(&slot.kind, cx);
        let task = restore.map(|restore| restore(slot.state.clone(), window, cx));
        firsts.push((slot.document, task));
    }
    let filled_panes = panes.clone();
    let filled = window.spawn(cx, async move |cx| {
        let mut restored: HashMap<u64, Box<dyn ItemHandle>> = HashMap::new();
        for (document, task) in firsts {
            if let Some(task) = task
                && let Some(item) = task.await
            {
                restored.insert(document, item);
            }
        }
        cx.update(|window, cx| fill(&filled_panes, slots, &actives, restored, window, cx))
            .unwrap_or_default()
    });
    Some(RestoredPanes {
        root,
        active,
        filled,
    })
}

/// An item restored into a pane: its saved index there, and whether it's
/// pinned.
type Placed = (usize, Box<dyn ItemHandle>, bool);

/// Puts each document's restored item in its first slot, and a split of
/// it in each other, into the panes.
fn fill<V: PaneView>(
    panes: &[Entity<V>],
    slots: Vec<Slot>,
    actives: &[usize],
    mut restored: HashMap<u64, Box<dyn ItemHandle>>,
    window: &mut Window,
    cx: &mut App,
) -> Vec<Entity<V>> {
    let mut placed: HashSet<u64> = HashSet::new();
    let mut items: Vec<Vec<Placed>> = panes.iter().map(|_| Vec::new()).collect();
    let mut counts = vec![0; panes.len()];
    for slot in slots {
        let ix = counts[slot.pane];
        counts[slot.pane] += 1;
        let Some(first) = restored.get(&slot.document) else {
            continue;
        };
        let item = if placed.insert(slot.document) {
            first.boxed_clone()
        } else {
            let Some(split) = first.split(window, cx) else {
                continue;
            };
            split.apply_state(&slot.state, window, cx);
            split
        };
        items[slot.pane].push((ix, item, slot.pinned));
    }
    restored.clear();
    let mut empty = Vec::new();
    for ((pane, items), active) in panes.iter().zip(items).zip(actives) {
        if items.is_empty() {
            empty.push(pane.clone());
            continue;
        }
        let shown = items
            .iter()
            .position(|(ix, _, _)| ix >= active)
            .unwrap_or(0);
        // Pinned in their saved order, which led the others.
        let pinned: Vec<_> = (items.iter())
            .filter(|(_, _, pinned)| *pinned)
            .map(|(_, item, _)| item.id())
            .collect();
        let items = items.into_iter().map(|(_, item, _)| item).collect();
        pane.update(cx, |pane, cx| {
            pane.add_items(items, shown, cx);
            for id in pinned {
                if let Some(ix) = pane.pane().position(id) {
                    pane.pin_tab(ix, cx);
                }
            }
        });
    }
    empty
}

fn restore_member<V>(
    saved: &Value,
    pane: &mut MakePane<'_, V>,
    window: &mut Window,
    cx: &mut App,
) -> Option<Member<V>> {
    if let Some(saved) = saved.get("pane") {
        return pane(saved, window, cx).map(Member::Pane);
    }
    let axis = match saved.get("axis")?.as_str()? {
        "horizontal" => Axis::Horizontal,
        "vertical" => Axis::Vertical,
        _ => return None,
    };
    let members: Vec<Member<V>> = (saved.get("members")?.as_array()?.iter())
        .filter_map(|member| restore_member(member, pane, window, cx))
        .collect();
    if members.is_empty() {
        return None;
    }
    let ratios = (saved.get("ratios").and_then(Value::as_array))
        .map(|ratios| {
            (ratios.iter())
                .map(|ratio| ratio.as_f64().unwrap_or(0.) as f32)
                .collect()
        })
        .unwrap_or_default();
    Some(Member::axis(axis, members, ratios))
}

fn pane_at<V>(member: &Member<V>, path: &[usize]) -> Option<Entity<V>> {
    match (member, path.split_first()) {
        (Member::Pane(pane), None) => Some(pane.clone()),
        (Member::Axis { members, .. }, Some((ix, rest))) => pane_at(members.get(*ix)?, rest),
        _ => None,
    }
}

/// A write of a window's entry, or its deletion, and whether its claim
/// goes once it's done.
struct Job {
    store: LayoutStore,
    claim: Arc<Claim>,
    /// `None` deletes the entry.
    entry: Option<Value>,
}

/// Saves or deletes the entry, then drops the claim if this held it last,
/// removing its lock file if the entry went.
fn run(job: Job) {
    let written = match &job.entry {
        Some(entry) => job.store.write(&job.claim, entry.clone()),
        None => job.store.delete(&job.claim),
    };
    if let Err(error) = written {
        log::error!(
            "can't save the window's layout in {}: {error}",
            job.store.file().display()
        );
    }
    if let Ok(claim) = Arc::try_unwrap(job.claim) {
        claim.release(job.entry.is_none());
    }
}

fn writer(cx: &mut App) -> Writer<(PathBuf, u64), Job> {
    if let Some(writer) = cx
        .try_global::<Layouts>()
        .and_then(|layouts| layouts.writer.clone())
    {
        return writer;
    }
    let writer = Writer::new(cx, |_: &(PathBuf, u64), job: Job| run(job));
    cx.default_global::<Layouts>().writer = Some(writer.clone());
    writer
}

/// The windows a launch on `store`'s root opens, each with its layout and
/// the entry it restores, if any. With `restore` (a launch on the folder
/// itself), the last session's windows (see the module docs), newest first;
/// failing that, or without `restore` (a file launch, whose window is never
/// restored later), one window with a fresh id. Empty only if no id
/// can be claimed (the state directory can't be written, say).
pub fn launch(store: &LayoutStore, restore: bool) -> Vec<(WindowLayout, Option<Value>)> {
    let mut windows: Vec<_> = match restore {
        true => {
            let last = store.restore_last_session(MAX_RESTORED);
            if last.dropped > 0 {
                log::info!(
                    "dropped {} saved windows of {}: older sessions, file launches, or beyond {MAX_RESTORED}",
                    last.dropped,
                    store.file().display()
                );
            }
            (last.windows.into_iter())
                .map(|(claim, entry)| (WindowLayout::new(store.clone(), claim), Some(entry)))
                .collect()
        }
        false => Vec::new(),
    };
    if windows.is_empty() {
        match store.claim_new() {
            Ok(claim) => {
                let mut layout = WindowLayout::new(store.clone(), claim);
                layout.restores = restore;
                windows.push((layout, None));
            }
            Err(error) => log::error!("can't keep this window's layout: {error}"),
        }
    }
    windows
}

/// The windows open when the app last quit, in the layouts directory
/// `dir`, whatever their roots, newest first, at most [`MAX_RESTORED`]:
/// each with the folder it was on (the empty path for none), its layout,
/// in the store it was saved in, and the entry it restores. A window whose
/// folder isn't `there` is skipped. See kiln-core's
/// [`restore_last_quit`](kiln_core::layout::restore_last_quit).
pub fn launch_last_quit(
    dir: &Path,
    there: impl Fn(&Path) -> bool,
) -> Vec<(PathBuf, WindowLayout, Value)> {
    (kiln_core::layout::restore_last_quit(dir, MAX_RESTORED, there).into_iter())
        .map(|window| {
            let layout = WindowLayout::new(window.store, window.claim);
            (window.root, layout, window.entry)
        })
        .collect()
}

/// A new window on `store`'s root (not the launch's): the newest entry no
/// live window holds, claimed, or else a fresh id.
pub fn new_window(store: &LayoutStore) -> Option<(WindowLayout, Option<Value>)> {
    if let Some((claim, entry)) = store.newest_restorable() {
        return Some((WindowLayout::new(store.clone(), claim), Some(entry)));
    }
    match store.claim_new() {
        Ok(claim) => Some((WindowLayout::new(store.clone(), claim), None)),
        Err(error) => {
            log::error!("can't keep this window's layout: {error}");
            None
        }
    }
}

/// Marks the app as quitting, so the windows closing as it does keep their
/// entries: for a host that quits without GPUI's quit observers running
/// first, and for tests. [`WindowLayout::save_at_quit`] marks it too.
pub fn mark_quitting(cx: &mut App) {
    cx.default_global::<Layouts>().quitting = true;
}

/// Whether the app is quitting: windows closing as it does keep their
/// entries.
pub fn quitting(cx: &App) -> bool {
    cx.try_global::<Layouts>()
        .is_some_and(|layouts| layouts.quitting)
}

/// A window's saved layout: its store, and its claim on its id.
pub struct WindowLayout {
    store: LayoutStore,
    claim: Option<Arc<Claim>>,
    /// The shape last saved.
    saved: Option<Value>,
    /// False for a file launch's window: its entries say `"restore": false`.
    restores: bool,
}

impl WindowLayout {
    /// The layout of a window on `store`'s root, its id held by `claim`.
    pub fn new(store: LayoutStore, claim: Claim) -> Self {
        Self {
            store,
            claim: Some(Arc::new(claim)),
            saved: None,
            restores: true,
        }
    }

    fn mark(&self, entry: &mut Value) {
        if !self.restores
            && let Some(object) = entry.as_object_mut()
        {
            object.insert("restore".into(), Value::Bool(false));
        }
    }

    pub fn id(&self) -> Option<u64> {
        self.claim.as_ref().map(|claim| claim.id())
    }

    pub fn store(&self) -> &LayoutStore {
        &self.store
    }

    /// Saves `entry` in the background if its [`shape`] changed since the
    /// last save.
    pub fn save_if_changed(&mut self, entry: Value, cx: &mut App) {
        let shape = shape(&entry);
        if self.saved.as_ref() != Some(&shape) {
            self.saved = Some(shape);
            self.save(entry, cx);
        }
    }

    /// Saves `entry` in the background.
    pub fn save(&mut self, mut entry: Value, cx: &mut App) {
        self.mark(&mut entry);
        let Some(claim) = self.claim.clone() else {
            return;
        };
        self.saved = Some(shape(&entry));
        let key = (self.store.file(), claim.id());
        let store = self.store.clone();
        writer(cx).save(
            key,
            Job {
                store,
                claim,
                entry: Some(entry),
            },
        );
    }

    /// Saves `entry` as the app quits, marking it as quitting, and resolves
    /// once every save queued is written.
    pub fn save_at_quit(&mut self, entry: Value, cx: &mut App) -> impl Future<Output = ()> + use<> {
        cx.default_global::<Layouts>().quitting = true;
        self.save(entry, cx);
        writer(cx).finish()
    }

    /// The window is closing: unless the app is quitting, its entry is
    /// saved as `entry` if no other window stays open, or else deleted.
    /// Its claim is released once that's done.
    pub fn close(mut self, mut entry: Value, others_open: bool, cx: &mut App) {
        self.mark(&mut entry);
        let Some(claim) = self.claim.take() else {
            return;
        };
        if quitting(cx) {
            return;
        }
        let key = (self.store.file(), claim.id());
        let store = self.store.clone();
        let entry = (!others_open).then_some(entry);
        writer(cx).save(
            key,
            Job {
                store,
                claim,
                entry,
            },
        );
    }
}
