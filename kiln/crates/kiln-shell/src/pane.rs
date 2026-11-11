// SPDX-License-Identifier: Apache-2.0

//! A pane: a tab per [`PaneItem`](crate::PaneItem), held through its
//! [`ItemHandle`], the active one's view below the tab strip.
//!
//! [`PaneState`] is the pane's items and layout, for the view that holds
//! and draws it, a [`PaneView`]. Its provided methods are the pane's
//! operations ([`PaneView::activate_item`], `cycle_items`, `take_item`,
//! `insert_item`, `reorder_item`, `reorder_active_item`,
//! `close_active_item`), and its hooks ([`PaneView::activated`],
//! [`PaneView::items_changed`]) let a host keep
//! its own state in step: a placeholder shown while a file is read, say.
//! [`ItemPane`] is kiln's own pane view.
//!
//! A pane can hold one preview item ([`PaneView::add_preview`]), a file
//! opened for a look: its tab is in italics, and the next preview takes its
//! place. It's kept, made an ordinary item, once it's dirty, its tab is
//! double-clicked, or [`PaneView::pin_item`] pins it.
//!
//! A tab can be pinned to the strip's start ([`PaneView::pin_tab`]), as
//! VS Code's sticky tabs are: the pinned items lead, in the order they were
//! pinned, and are drawn compact, at their icon's width
//! ([`kiln_ui::Tab::pinned`]). No reorder or insert crosses the boundary:
//! a pinned item moves only among the pinned, and an item put in the pane
//! goes after them. A layout saves and restores the flag
//! ([`crate::layout`]).
//!
//! A tab can be dragged ([`DraggedTab`]) into another pane's tab strip or
//! onto its content, whose edges split it ([`DropZone`]); the pane it's
//! dropped on emits [`TabDropped`], for the host to move it with
//! [`PaneGroup::move_tab`](crate::PaneGroup::move_tab).

use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    path::{Component, Path, PathBuf},
    rc::Rc,
};

use gpui::{
    AnyElement, App, AppContext as _, Bounds, ClickEvent, Context, DragMoveEvent, Entity, EntityId,
    EventEmitter, FocusHandle, Focusable, InteractiveElement as _, IntoElement, ParentElement as _,
    Pixels, Point, Render, ScrollHandle, SharedString, Styled as _, Subscription, WeakEntity,
    Window, canvas, div, point, px, size,
};
use kiln_ui::{TAB_HEIGHT, TabBar, ui_theme};

use crate::{
    ItemHandle, ItemId,
    close::{self, CloseKind, CloseRequest, Closing, Items, LiveItems},
    pane_group::{Destination, Direction},
};

/// Every pane of a group, each one included, so a pane can see what the
/// others show.
pub type Peers<V> = Rc<RefCell<Vec<WeakEntity<V>>>>;

/// What a pane tells its host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneEvent {
    /// A mouse button went down in the pane, or something it was waiting
    /// for landed and took focus, so it's the active pane now.
    Activated,
}

/// A tab being dragged: its pane, and its item, by ID, as indices can
/// shift while the drag goes on.
pub struct DraggedTab<V> {
    pub pane: WeakEntity<V>,
    pub item: ItemId,
}

impl<V> Clone for DraggedTab<V> {
    fn clone(&self) -> Self {
        Self {
            pane: self.pane.clone(),
            item: self.item,
        }
    }
}

impl<V: 'static> std::fmt::Debug for DraggedTab<V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DraggedTab")
            .field("pane", &self.pane.entity_id())
            .field("item", &self.item)
            .finish()
    }
}

/// A tab dropped on a pane, for the host to move.
pub struct TabDropped<V> {
    pub dragged: DraggedTab<V>,
    pub destination: Destination<V>,
}

/// The dragged tab's title, in tab style, at the pointer.
pub struct TabGhost(pub SharedString);

impl Render for TabGhost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tab = kiln_ui::Tab::new("ghost", self.0.clone(), &ui_theme(cx)).active(true);
        div().h(TAB_HEIGHT).text_size(px(12.)).child(tab)
    }
}

/// Where in a pane's content a drop goes: the outer quarter on each side
/// splits that way, the rest takes the tab in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropZone {
    Left,
    Right,
    Up,
    Down,
    Centre,
}

impl DropZone {
    /// The zone of content laid out at `bounds` that `point` is in. In a
    /// corner, the nearer edge wins, by distance relative to the size.
    pub fn at(bounds: Bounds<Pixels>, point: Point<Pixels>) -> Self {
        let x = f32::from(point.x - bounds.left()) / f32::from(bounds.size.width);
        let y = f32::from(point.y - bounds.top()) / f32::from(bounds.size.height);
        let (dx, dy) = (x.min(1. - x), y.min(1. - y));
        match (dx.min(dy) < 0.25, dx <= dy) {
            (false, _) => DropZone::Centre,
            (true, true) if x < 0.5 => DropZone::Left,
            (true, true) => DropZone::Right,
            (true, false) if y < 0.5 => DropZone::Up,
            (true, false) => DropZone::Down,
        }
    }

    /// The part of a pane laid out at `pane` a drop here fills: the half on
    /// that side, or for the centre all of its content, laid out at
    /// `content`.
    pub fn overlay(self, pane: Bounds<Pixels>, content: Bounds<Pixels>) -> Bounds<Pixels> {
        let (mut overlay, half) = (pane, pane.size / 2.);
        match self {
            DropZone::Left => overlay.size.width = half.width,
            DropZone::Up => overlay.size.height = half.height,
            DropZone::Right => {
                overlay.origin.x += half.width;
                overlay.size.width -= half.width;
            }
            DropZone::Down => {
                overlay.origin.y += half.height;
                overlay.size.height -= half.height;
            }
            DropZone::Centre => overlay = content,
        }
        overlay
    }
}

/// Where in a pane a hovering tab would drop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropTarget {
    /// Into the tab strip, at this index.
    TabBar(usize),
    /// Onto the content area.
    Content(DropZone),
}

impl DropTarget {
    /// Where the tab goes, dropped here in `pane`.
    pub fn destination<V>(self, pane: Entity<V>) -> Destination<V> {
        match self {
            DropTarget::TabBar(index) => Destination::Pane {
                pane,
                index: Some(index),
            },
            DropTarget::Content(zone) => {
                let direction = match zone {
                    DropZone::Left => Direction::Left,
                    DropZone::Right => Direction::Right,
                    DropZone::Up => Direction::Up,
                    DropZone::Down => Direction::Down,
                    DropZone::Centre => return Destination::Pane { pane, index: None },
                };
                Destination::Split { pane, direction }
            }
        }
    }
}

/// The insertion index for a pointer at `x` over tabs laid out at `tabs`:
/// before the first tab whose right half or beyond it's not in, else the
/// end.
pub fn insertion(tabs: &[Bounds<Pixels>], x: Pixels) -> usize {
    tabs.iter()
        .position(|tab| x < tab.center().x)
        .unwrap_or(tabs.len())
}

/// The 2 px insertion marker for `index` among tabs laid out at `tabs`, in
/// a strip laid out at `bar`.
pub fn marker(tabs: &[Bounds<Pixels>], index: usize, bar: Bounds<Pixels>) -> Bounds<Pixels> {
    let x = match (tabs.get(index), tabs.last()) {
        (Some(tab), _) => tab.left(),
        (None, Some(last)) => last.right(),
        (None, None) => bar.left() + px(1.),
    };
    // Tabs can reach past the strip, scrolled or long.
    let x = (x - px(1.)).clamp(bar.left(), (bar.right() - px(2.)).max(bar.left()));
    Bounds {
        origin: point(x, bar.top()),
        size: size(px(2.), TAB_HEIGHT),
    }
}

/// Fills its parent, recording where it's laid out in `bounds`.
pub fn record(bounds: &Rc<Cell<Bounds<Pixels>>>) -> impl IntoElement + use<> {
    let bounds = bounds.clone();
    canvas(move |laid_out, _, _| bounds.set(laid_out), |_, _, _, _| {})
        .absolute()
        .top_0()
        .left_0()
        .size_full()
}

/// A pane's items, the active one, and where the last frame laid it out:
/// the state a [`PaneView`] holds.
pub struct PaneState<V> {
    items: Vec<Box<dyn ItemHandle>>,
    active: usize,
    /// Each item's notifications, which redraw the pane.
    observed: HashMap<ItemId, Subscription>,
    /// Focused when there's no item, so the pane's commands still apply.
    focus: FocusHandle,
    tab_scroll: ScrollHandle,
    /// The active tab the strip last scrolled into view, so a scroll by hand
    /// stays until the active tab changes.
    revealed: Cell<Option<(usize, ItemId)>>,
    bounds: Rc<Cell<Bounds<Pixels>>>,
    content_bounds: Rc<Cell<Bounds<Pixels>>>,
    tab_bar_bounds: Rc<Cell<Bounds<Pixels>>>,
    hint_bounds: Rc<Cell<Bounds<Pixels>>>,
    hover: Option<DropTarget>,
    /// The preview item, which the next preview replaces. The items'
    /// observers clear it once it's dirty.
    preview: Rc<Cell<Option<ItemId>>>,
    /// The pinned items, which lead the others: see [`PaneView::pin_tab`].
    pinned: HashSet<ItemId>,
    peers: Peers<V>,
    /// The view's.
    id: EntityId,
    view: WeakEntity<V>,
    emptied: bool,
    /// Whether the view leaves its tab strip out: see
    /// [`PaneState::set_tab_bar_hidden`].
    tab_bar_hidden: bool,
}

impl<V: PaneView> PaneState<V> {
    /// An empty pane, for the view `cx` makes. It draws in the active
    /// theme's colours ([`ui_theme`]), so it follows a theme change.
    pub fn new(cx: &mut Context<V>) -> Self {
        Self {
            items: Vec::new(),
            active: 0,
            observed: HashMap::new(),
            focus: cx.focus_handle(),
            tab_scroll: ScrollHandle::new(),
            revealed: Cell::new(None),
            bounds: Rc::default(),
            content_bounds: Rc::default(),
            tab_bar_bounds: Rc::default(),
            hint_bounds: Rc::default(),
            hover: None,
            preview: Rc::default(),
            pinned: HashSet::new(),
            peers: Peers::default(),
            id: cx.entity_id(),
            view: cx.weak_entity(),
            emptied: false,
            tab_bar_hidden: false,
        }
    }

    pub fn items(&self) -> &[Box<dyn ItemHandle>] {
        &self.items
    }

    /// The active item's index; `None` with no items.
    pub fn active(&self) -> Option<usize> {
        (self.active < self.items.len()).then_some(self.active)
    }

    pub fn active_item(&self) -> Option<&dyn ItemHandle> {
        self.items.get(self.active).map(|item| &**item)
    }

    /// The index of the item with `id`.
    pub fn position(&self, id: ItemId) -> Option<usize> {
        self.items.iter().position(|item| item.id() == id)
    }

    /// Focused when the pane has no item to focus.
    pub fn focus(&self) -> &FocusHandle {
        &self.focus
    }

    /// Adds `item` after the others, without showing it.
    pub fn push(&mut self, item: Box<dyn ItemHandle>, cx: &mut App) -> usize {
        self.insert(self.items.len(), item, cx)
    }

    /// Puts `item` at `ix`, clamped, and after the pinned items, without
    /// showing it: the active item stays the same one.
    pub fn insert(&mut self, ix: usize, item: Box<dyn ItemHandle>, cx: &mut App) -> usize {
        let ix = ix.clamp(self.pinned_count(), self.items.len());
        self.insert_at(ix, item, cx)
    }

    /// Puts `item` just before item `ix`, which it's to replace in place,
    /// pinned if that one is, without showing it: the active item stays the
    /// same one. The host then takes the old item out by its ID, now at
    /// `ix + 1`. `None`, adding nothing, if there's no item `ix`.
    pub fn insert_replacing(
        &mut self,
        ix: usize,
        item: Box<dyn ItemHandle>,
        cx: &mut App,
    ) -> Option<usize> {
        let old = self.items.get(ix)?.id();
        if self.pinned.contains(&old) {
            self.pinned.insert(item.id());
        }
        Some(self.insert_at(ix, item, cx))
    }

    /// Puts `item` at `ix`, which must keep the pinned items leading.
    fn insert_at(&mut self, ix: usize, item: Box<dyn ItemHandle>, cx: &mut App) -> usize {
        let (pane, preview, watched) = (self.id, self.preview.clone(), item.boxed_clone());
        let observed = item.observe(
            Box::new(move |cx| {
                // An edit keeps a preview.
                if preview.get() == Some(watched.id()) && watched.is_dirty(cx) {
                    preview.set(None);
                }
                cx.notify(pane);
            }),
            cx,
        );
        self.observed.insert(item.id(), observed);
        if ix <= self.active && ix < self.items.len() {
            self.active += 1;
        }
        self.items.insert(ix, item);
        ix
    }

    /// The preview item's index: the item the next preview replaces, drawn
    /// in italics. `None` once it's dirty, though its observer hasn't heard
    /// yet.
    pub fn preview(&self, cx: &App) -> Option<usize> {
        let id = self.preview.get()?;
        let ix = self.position(id)?;
        (!self.items[ix].is_dirty(cx)).then_some(ix)
    }

    /// Makes item `ix` the preview, or with `None` keeps the preview there
    /// is, as an ordinary item.
    pub fn set_preview(&mut self, ix: Option<usize>) {
        let id = ix.and_then(|ix| self.items.get(ix)).map(|item| item.id());
        self.preview.set(id);
    }

    /// Whether item `ix` is pinned.
    pub fn is_pinned(&self, ix: usize) -> bool {
        (self.items.get(ix)).is_some_and(|item| self.pinned.contains(&item.id()))
    }

    /// How many items are pinned: the first that many.
    pub fn pinned_count(&self) -> usize {
        (self.items.iter())
            .take_while(|item| self.pinned.contains(&item.id()))
            .count()
    }

    /// Pins item `ix`, moving it to the end of the pinned items; a preview
    /// is kept as an ordinary item. The active item stays the same one.
    /// Its index after, or `None` if there's no such item.
    pub fn pin(&mut self, ix: usize) -> Option<usize> {
        let id = self.items.get(ix)?.id();
        if self.pinned.contains(&id) {
            return Some(ix);
        }
        if self.preview.get() == Some(id) {
            self.preview.set(None);
        }
        let to = self.pinned_count();
        self.pinned.insert(id);
        Some(self.move_item(ix, to))
    }

    /// Unpins item `ix`, moving it to just after the pinned items. The
    /// active item stays the same one. Its index after, or `None` if there's
    /// no such item.
    pub fn unpin(&mut self, ix: usize) -> Option<usize> {
        let id = self.items.get(ix)?.id();
        if !self.pinned.remove(&id) {
            return Some(ix);
        }
        // The pinned items before and after it, less itself.
        let to = (self.items.iter())
            .filter(|item| self.pinned.contains(&item.id()))
            .count();
        Some(self.move_item(ix, to))
    }

    /// Moves item `from` to `to`, the active item staying the same one.
    fn move_item(&mut self, from: usize, to: usize) -> usize {
        let active = self.items.get(self.active).map(|item| item.id());
        let item = self.items.remove(from);
        let to = to.min(self.items.len());
        self.items.insert(to, item);
        if let Some(ix) = active.and_then(|id| self.position(id)) {
            self.active = ix;
        }
        to
    }

    /// Makes `ix` the active index, without focusing anything.
    pub fn set_active(&mut self, ix: usize) {
        self.active = ix;
    }

    /// Has the view leave its tab strip out, or draw it again: its items
    /// stay, and switching them by command still works, but nothing drops
    /// on the strip while it's out. [`ItemPane`] follows it; a host's own
    /// view reads [`PaneState::tab_bar_hidden`].
    pub fn set_tab_bar_hidden(&mut self, hidden: bool) {
        self.tab_bar_hidden = hidden;
        if hidden {
            self.tab_bar_bounds.set(Bounds::default());
        }
    }

    /// Whether the view leaves its tab strip out.
    pub fn tab_bar_hidden(&self) -> bool {
        self.tab_bar_hidden
    }

    /// Joins a group of panes, `peers`.
    pub fn set_peers(&mut self, peers: Peers<V>) {
        self.peers = peers;
    }

    pub fn peers(&self) -> &Peers<V> {
        &self.peers
    }

    /// Whether the pane's last item went, and nothing has come since.
    /// Asking clears it once it's true.
    pub fn take_emptied(&mut self) -> bool {
        if !self.items.is_empty() {
            self.emptied = false;
        }
        std::mem::take(&mut self.emptied)
    }

    /// Whether the pane's last item went and nothing has come since,
    /// without clearing it.
    pub fn is_emptied(&self) -> bool {
        self.emptied && self.items.is_empty()
    }

    /// Whether an item other than `item`, in this pane or another of the
    /// group, shows its document.
    pub fn shown_elsewhere(&self, item: &dyn ItemHandle, cx: &App) -> bool {
        self.items
            .iter()
            .any(|other| other.id() != item.id() && other.shares_document(item, cx))
            || self.shown_in_peers(item, cx)
    }

    /// Whether an item in another pane of the group shows `item`'s
    /// document.
    pub fn shown_in_peers(&self, item: &dyn ItemHandle, cx: &App) -> bool {
        self.peer_item(cx, |other| other.shares_document(item, cx))
            .is_some()
    }

    /// Every item of the group's panes, this one's first. It reads the
    /// others' views, so it can be called while this one updates.
    pub fn group_items(&self, cx: &App) -> Vec<Box<dyn ItemHandle>> {
        let mut items = self.items.clone();
        items.extend(peer_items(&self.peers, self.id, cx));
        items
    }

    /// The first item in another pane of the group that `pick`s.
    pub fn peer_item(
        &self,
        cx: &App,
        pick: impl Fn(&dyn ItemHandle) -> bool,
    ) -> Option<Box<dyn ItemHandle>> {
        let peers = self.peers.borrow();
        peers
            .iter()
            .filter(|peer| peer.entity_id() != self.id)
            .filter_map(WeakEntity::upgrade)
            .find_map(|peer| {
                let state = peer.read(cx).pane();
                let item = state.items.iter().find(|item| pick(&***item))?;
                Some(item.boxed_clone())
            })
    }

    /// Where the last frame laid the pane out; zero-sized before that.
    pub fn bounds(&self) -> Bounds<Pixels> {
        self.bounds.get()
    }

    /// Where it laid out the content below the tab strip and any rows the
    /// host adds.
    pub fn content_bounds(&self) -> Bounds<Pixels> {
        self.content_bounds.get()
    }

    /// Where it laid out the tab strip, left of anything beside it.
    pub fn tab_bar_bounds(&self) -> Bounds<Pixels> {
        self.tab_bar_bounds.get()
    }

    /// Where tab `ix` was laid out in the last frame.
    pub fn tab_bounds(&self, ix: usize) -> Option<Bounds<Pixels>> {
        self.tab_scroll.bounds_for_item(ix)
    }

    /// Scrolls the tab strip left by `by`.
    pub fn scroll_tabs(&self, by: Pixels) {
        self.tab_scroll.set_offset(point(-by, px(0.)));
    }

    /// Where a hovering tab would drop.
    pub fn hovered_drop(&self) -> Option<DropTarget> {
        self.hover
    }

    /// Where the last frame laid out the drop marker or overlay, if one
    /// showed.
    pub fn drop_hint_bounds(&self) -> Option<Bounds<Pixels>> {
        self.hover.map(|_| self.hint_bounds.get())
    }

    /// Records where the pane is laid out: a child of its root element.
    pub fn record_bounds(&self) -> impl IntoElement + use<V> {
        record(&self.bounds)
    }

    /// Records where its content is laid out: a child of the content's
    /// element.
    pub fn record_content_bounds(&self) -> impl IntoElement + use<V> {
        record(&self.content_bounds)
    }

    /// Records where its tab strip is laid out: a child of the strip's
    /// parent.
    pub fn record_tab_bar_bounds(&self) -> impl IntoElement + use<V> {
        record(&self.tab_bar_bounds)
    }

    /// Where a tab dragged to `at` would drop: in the tab strip, unless
    /// it's out, or on the content area, by the last frame's layout.
    pub fn drop_target(&self, at: Point<Pixels>) -> Option<DropTarget> {
        if !self.bounds().contains(&at) {
            return None;
        }
        let content = self.content_bounds.get();
        if content.contains(&at) {
            return Some(DropTarget::Content(DropZone::at(content, at)));
        }
        if self.tab_bar_hidden {
            return None;
        }
        Some(DropTarget::TabBar(insertion(&self.tab_layout(), at.x)))
    }

    /// Where the last frame laid out each tab, scrolled as it shows.
    fn tab_layout(&self) -> Vec<Bounds<Pixels>> {
        let scrolled = self.tab_scroll.offset();
        (0..self.items.len())
            .map_while(|ix| self.tab_scroll.bounds_for_item(ix))
            .map(|tab| Bounds::new(tab.origin + scrolled, tab.size))
            .collect()
    }

    /// A tab dragged over the pane moved: whether where it would drop
    /// changed, for the pane to redraw its hint.
    pub fn drag_moved(&mut self, event: &DragMoveEvent<DraggedTab<V>>) -> bool {
        let target = self.drop_target(event.event.position);
        let changed = target != self.hover;
        self.hover = target;
        changed
    }

    /// Forgets the hover once no drag is active; call as the pane draws.
    pub fn end_hover(&mut self, cx: &App) {
        if !cx.has_active_drag() {
            self.hover = None;
        }
    }

    /// A tab dropped on the pane at `at`: emits [`TabDropped`] for the
    /// host to move it, unless it's the pane's only tab dropped on it.
    pub fn dropped(&mut self, dragged: &DraggedTab<V>, at: Point<Pixels>, cx: &mut Context<V>) {
        self.hover = None;
        cx.notify();
        let Some(target) = self.drop_target(at) else {
            return;
        };
        let own = dragged.pane.entity_id() == self.id;
        if own && self.items.len() == 1 && self.items[0].id() == dragged.item {
            return;
        }
        cx.emit(TabDropped {
            dragged: dragged.clone(),
            destination: target.destination(cx.entity()),
        });
    }

    /// The listener for a tab dropped on one of the pane's elements.
    pub fn on_drop(
        &self,
        cx: &mut Context<V>,
    ) -> impl Fn(&DraggedTab<V>, &mut Window, &mut App) + 'static {
        cx.listener(|view: &mut V, dragged: &DraggedTab<V>, window, cx| {
            view.pane_mut()
                .dropped(dragged, window.mouse_position(), cx)
        })
    }

    /// Over the content while a tab is dragged, so it takes drops before
    /// the item's own mouse handlers see them.
    pub fn drop_layer(&self, cx: &mut Context<V>) -> Option<impl IntoElement + use<V>> {
        cx.has_active_drag().then(|| {
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .on_drop(self.on_drop(cx))
        })
    }

    /// A tab per item as drawn, but for its click, close, drag and drop
    /// handlers: its icon, title, title colour and dirty dot from the item,
    /// and whether it is the active one, which shows its close button while
    /// the others show theirs only on hovering. Tabs that share a title are described
    /// by their [`PaneItem::tab_path`](crate::PaneItem::tab_path)s'
    /// folders ([`tab_descriptions`]).
    pub fn tabs(&self, cx: &App) -> Vec<kiln_ui::Tab> {
        let ui = ui_theme(cx);
        let titles: Vec<_> = self.items.iter().map(|item| item.tab_title(cx)).collect();
        let paths: Vec<_> = self.items.iter().map(|item| item.tab_path(cx)).collect();
        let descriptions = tab_descriptions(&titles, &paths);
        let preview = self.preview(cx);
        (self.items.iter().zip(titles).zip(descriptions).enumerate())
            .map(|(ix, ((item, title), description))| {
                kiln_ui::Tab::new(("tab", ix), title, &ui)
                    .preview(Some(ix) == preview)
                    .pinned(self.is_pinned(ix))
                    .icon(item.tab_icon_ref(cx))
                    .description(description)
                    .label_color(item.tab_title_color(cx))
                    .dirty(item.is_dirty(cx))
                    .active(ix == self.active)
            })
            .collect()
    }

    /// The tab strip: a tab per item, its title and dirty dot from the
    /// item, which a click activates, a double-click pins, the × closes,
    /// and a drag carries. The active tab has the accent line while the
    /// pane's focus handle ([`PaneState::focus`]) is focused or contains
    /// what is, so the view must track it around its items.
    pub fn tab_bar(&self, cx: &mut Context<V>) -> TabBar {
        let pane = cx.entity().downgrade();
        let ui = ui_theme(cx);
        let active = self
            .items
            .get(self.active)
            .map(|item| (self.active, item.id()));
        if active != self.revealed.replace(active)
            && let Some((ix, _)) = active
        {
            self.tab_scroll.scroll_to_item(ix);
        }
        let tabs: Vec<_> = (self.tabs(cx).into_iter().zip(&self.items))
            .enumerate()
            .map(|(ix, (tab, item))| {
                // The × and a middle click close the same way.
                let close = Rc::new(cx.listener(
                    move |view: &mut V, _: &ClickEvent, window, cx| {
                        view.activate_item(ix, window, cx);
                        view.close_active_item(window, cx);
                        cx.notify();
                    },
                ));
                let label = item.tab_title(cx);
                let dragged = DraggedTab {
                    pane: pane.clone(),
                    item: item.id(),
                };
                let (x, middle) = (close.clone(), close);
                tab.on_drag(dragged, move |_, _, _, cx| {
                    cx.new(|_| TabGhost(label.clone()))
                })
                .on_drop(self.on_drop(cx))
                .on_click(
                    cx.listener(move |view: &mut V, event: &ClickEvent, window, cx| {
                        view.activate_item(ix, window, cx);
                        if event.click_count() >= 2 {
                            view.pin_item(ix, cx);
                        }
                        cx.notify();
                    }),
                )
                .on_close(move |event, window, cx| x(event, window, cx))
                .on_middle_click(move |event, window, cx| middle(event, window, cx))
            })
            .collect();
        TabBar::new("tabs", &ui)
            .track_scroll(&self.tab_scroll)
            .focus_within(&self.focus)
            .on_drop(self.on_drop(cx))
            .children(tabs)
    }

    /// The drop marker in the tab strip, or the overlay on the content,
    /// for where a hovering tab would drop: the last child of the pane's
    /// root element.
    pub fn drop_hint(&self, cx: &App) -> Option<AnyElement> {
        let hover = self.hover?;
        let ui = ui_theme(cx);
        let origin = self.bounds().origin;
        let (hint, fill) = match hover {
            DropTarget::TabBar(index) => {
                let bar = self.tab_bar_bounds.get();
                (marker(&self.tab_layout(), index, bar), ui.drop_target)
            }
            DropTarget::Content(zone) => {
                let overlay = zone.overlay(self.bounds(), self.content_bounds.get());
                (overlay, ui.drop_target.opacity(0.2))
            }
        };
        let element = div()
            .absolute()
            .left(hint.left() - origin.x)
            .top(hint.top() - origin.y)
            .w(hint.size.width)
            .h(hint.size.height)
            .bg(fill)
            .child(record(&self.hint_bounds));
        if !matches!(hover, DropTarget::TabBar(_)) {
            return Some(element.into_any_element());
        }
        // The marker is clipped to the tab strip.
        let clip = div()
            .absolute()
            .top_0()
            .left_0()
            .w(self.tab_bar_bounds.get().size.width)
            .h(TAB_HEIGHT)
            .overflow_hidden()
            .child(element);
        Some(clip.into_any_element())
    }
}

/// The items of the panes in `peers` but `except`.
fn peer_items<V: PaneView>(
    peers: &Peers<V>,
    except: EntityId,
    cx: &App,
) -> Vec<Box<dyn ItemHandle>> {
    let peers = peers.borrow();
    (peers.iter())
        .filter(|peer| peer.entity_id() != except)
        .filter_map(WeakEntity::upgrade)
        .flat_map(|peer| peer.read(cx).pane().items.clone())
        .collect()
}

/// A view that holds a [`PaneState`] and draws it. Its provided methods
/// are the pane's operations; a host overrides the hooks to keep its own
/// state in step.
pub trait PaneView:
    Render + Focusable + EventEmitter<PaneEvent> + EventEmitter<TabDropped<Self>> + Sized + 'static
{
    fn pane(&self) -> &PaneState<Self>;

    fn pane_mut(&mut self) -> &mut PaneState<Self>;

    /// An empty pane like this one, for a split, made in `cx`.
    fn new_peer(&self, cx: &mut Context<Self>) -> Self;

    /// The item the pane shows: its active item, unless the host draws
    /// something else over it (a file still loading, say). A split copies
    /// it, and focus goes to it, or to the pane while there's none.
    fn shown_item(&self) -> Option<&dyn ItemHandle> {
        self.pane().active_item()
    }

    /// Item `ix` was made the active one and focused.
    fn activated(&mut self, _ix: usize, _window: &mut Window, _cx: &mut App) {}

    /// Items came, went or moved.
    fn items_changed(&mut self, _cx: &mut App) {}

    /// Shows item `ix` and focuses it; false if there's none.
    fn activate_item(&mut self, ix: usize, window: &mut Window, cx: &mut App) -> bool {
        let Some(item) = self.pane().items.get(ix) else {
            return false;
        };
        let focus = item.item_focus_handle(cx);
        self.pane_mut().active = ix;
        window.focus(&focus, cx);
        self.activated(ix, window, cx);
        true
    }

    /// Activates the next item, or with `back` the previous one, wrapping.
    /// A sole item activates itself.
    fn cycle_items(&mut self, back: bool, window: &mut Window, cx: &mut App) {
        let state = self.pane();
        let count = state.items.len();
        if count > 0 {
            let ix = if back {
                state.active + count - 1
            } else {
                state.active + 1
            } % count;
            self.activate_item(ix, window, cx);
        }
    }

    /// Removes item `ix`, for a close or a move. If it was active, its
    /// right neighbour is activated, or the left one if it was last. With
    /// none left, the pane is focused and emptied.
    fn take_item(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Box<dyn ItemHandle>> {
        let state = self.pane_mut();
        if ix >= state.items.len() {
            return None;
        }
        let item = state.items.remove(ix);
        state.observed.remove(&item.id());
        state.pinned.remove(&item.id());
        if state.preview.get() == Some(item.id()) {
            state.preview.set(None);
        }
        if state.items.is_empty() {
            state.active = 0;
            state.emptied = true;
            let focus = state.focus.clone();
            window.focus(&focus, cx);
        } else if ix == state.active {
            let to = state.active.min(state.items.len() - 1);
            self.activate_item(to, window, cx);
        } else if ix < state.active {
            state.active -= 1;
        }
        self.items_changed(cx);
        cx.notify(self.pane().id);
        Some(item)
    }

    /// Adds `item`, moved from another pane, at `index` or else at the
    /// end, and shows and focuses it.
    fn insert_item(
        &mut self,
        item: Box<dyn ItemHandle>,
        index: Option<usize>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let state = self.pane_mut();
        let ix = state.insert(index.unwrap_or(usize::MAX), item, cx);
        self.activate_item(ix, window, cx);
        self.items_changed(cx);
        cx.notify(self.pane().id);
    }

    /// Adds `items` after the others and makes the one at `active` among
    /// them the active item, without focusing it: for a restored layout.
    fn add_items(&mut self, items: Vec<Box<dyn ItemHandle>>, active: usize, cx: &mut App) {
        if items.is_empty() {
            return;
        }
        let state = self.pane_mut();
        let first = state.items.len();
        let count = items.len();
        for item in items {
            state.push(item, cx);
        }
        state.active = first + active.min(count - 1);
        self.items_changed(cx);
        cx.notify(self.pane().id);
    }

    /// Adds `item` as the pane's preview, without showing it: in the
    /// preview's place, the preview closing without being asked, or else
    /// after the others. A preview that's dirty is kept, though its
    /// observer hasn't heard yet. Returns its index.
    fn add_preview(&mut self, item: Box<dyn ItemHandle>, cx: &mut App) -> usize {
        let state = self.pane_mut();
        let ix = match state.preview(cx) {
            Some(ix) => {
                let old = state.items.remove(ix);
                state.observed.remove(&old.id());
                // Every other item keeps its index, the active one too.
                let active = state.active;
                state.insert(ix, item, cx);
                state.active = active;
                ix
            }
            None => state.push(item, cx),
        };
        state.set_preview(Some(ix));
        self.items_changed(cx);
        cx.notify(self.pane().id);
        ix
    }

    /// Pins item `ix`: if it's the preview, it's kept as an ordinary item,
    /// which the next preview doesn't replace.
    fn pin_item(&mut self, ix: usize, cx: &mut App) {
        let state = self.pane_mut();
        let pinned =
            (state.items.get(ix)).is_some_and(|item| state.preview.get() == Some(item.id()));
        if pinned {
            state.preview.set(None);
            cx.notify(self.pane().id);
        }
    }

    /// Pins item `ix` to the strip's start, after the items pinned before
    /// it, drawn compact; a preview is kept. The active item stays the same
    /// one. Bulk closes a host makes may leave pinned items be; a close of
    /// the item itself closes it.
    fn pin_tab(&mut self, ix: usize, cx: &mut App) {
        if self.pane_mut().pin(ix).is_some() {
            self.items_changed(cx);
            cx.notify(self.pane().id);
        }
    }

    /// Unpins item `ix`, moving it to just after the pinned items.
    fn unpin_tab(&mut self, ix: usize, cx: &mut App) {
        if self.pane_mut().unpin(ix).is_some() {
            self.items_changed(cx);
            cx.notify(self.pane().id);
        }
    }

    /// Moves item `ix` to before the item now at `index`, or else to the
    /// end, and shows and focuses it. It stays on its side of the pinned
    /// items' boundary: a pinned item among the pinned, another after them.
    fn reorder_item(&mut self, ix: usize, index: Option<usize>, window: &mut Window, cx: &mut App) {
        let state = self.pane_mut();
        if ix >= state.items.len() {
            return;
        }
        let to = match index {
            Some(index) if index > ix => index - 1,
            Some(index) => index,
            None => state.items.len() - 1,
        };
        let pinned = state.pinned_count();
        let to = if state.is_pinned(ix) {
            to.min(pinned - 1)
        } else {
            to.max(pinned)
        };
        let item = state.items.remove(ix);
        let to = to.min(state.items.len());
        state.items.insert(to, item);
        self.activate_item(to, window, cx);
        self.items_changed(cx);
        cx.notify(self.pane().id);
    }

    /// Moves the active item one place along the strip, right or with
    /// `back` left, as dragging its tab past its neighbour does, and keeps
    /// it shown and focused. It stays on its side of the pinned items'
    /// boundary. False, doing nothing, at that end of its side, or with no
    /// item.
    fn reorder_active_item(&mut self, back: bool, window: &mut Window, cx: &mut App) -> bool {
        let state = self.pane();
        let (ix, count) = (state.active, state.items.len());
        if ix >= count {
            return false;
        }
        let pinned = state.pinned_count();
        let (first, last) = if state.is_pinned(ix) {
            (0, pinned - 1)
        } else {
            (pinned, count - 1)
        };
        match back {
            true if ix > first => self.reorder_item(ix, Some(ix - 1), window, cx),
            false if ix < last => self.reorder_item(ix, Some(ix + 2), window, cx),
            _ => return false,
        }
        true
    }

    /// Closes the active item and activates its right neighbour, or the
    /// left one if it was last, once the item agrees
    /// ([`PaneItem::may_close`](crate::PaneItem::may_close), through
    /// [`close::confirm`]). It isn't asked while another item, in this pane
    /// or another, shows its document: its changes live on there. An item
    /// that asks the user closes once they agree, if it's still open.
    fn close_active_item(&mut self, window: &mut Window, cx: &mut App) {
        let state = self.pane();
        let Some(item) = state.active_item().map(ItemHandle::boxed_clone) else {
            return;
        };
        let (id, view, peers) = (item.id(), state.view.clone(), state.peers.clone());
        let now = Items {
            closing: vec![item],
            open: state.group_items(cx),
        };
        let live: LiveItems = Rc::new(move |cx| {
            let pane = view.upgrade()?;
            let mut open = pane.read(cx).pane().items.clone();
            open.extend(peer_items(&peers, pane.entity_id(), cx));
            let closing = open.iter().filter(|item| item.id() == id).cloned();
            Some(Items {
                closing: closing.collect(),
                open,
            })
        });
        let view = state.view.clone();
        let commit = move |window: Option<&mut Window>, cx: &mut App| {
            let Some(window) = window else {
                return;
            };
            view.update(cx, |pane, cx| {
                if let Some(ix) = pane.pane().position(id) {
                    pane.take_item(ix, window, cx);
                    cx.notify();
                }
            })
            .ok();
        };
        let request =
            CloseRequest::new(CloseKind::Items).scope(window.window_handle(), Some(now), live);
        match close::confirm(request, commit, Some(window), cx) {
            Closing::Now => {
                if let Some(ix) = self.pane().position(id) {
                    self.take_item(ix, window, cx);
                }
            }
            Closing::Refused => {}
            Closing::Later(closing) => closing.detach(),
        }
    }
}

/// Kiln's pane view: the tab strip, then the active item, with nothing of
/// the host's around them.
pub struct ItemPane {
    state: PaneState<ItemPane>,
}

impl ItemPane {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            state: PaneState::new(cx),
        }
    }

    /// Adds `item` after the others and shows it, without focusing it.
    pub fn add_item(&mut self, item: Box<dyn ItemHandle>, cx: &mut Context<Self>) {
        let ix = self.state.push(item, cx);
        self.state.set_active(ix);
        self.items_changed(cx);
        cx.notify();
    }
}

impl EventEmitter<PaneEvent> for ItemPane {}

impl EventEmitter<TabDropped<ItemPane>> for ItemPane {}

impl Focusable for ItemPane {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.state.focus.clone()
    }
}

impl PaneView for ItemPane {
    fn pane(&self) -> &PaneState<Self> {
        &self.state
    }

    fn pane_mut(&mut self) -> &mut PaneState<Self> {
        &mut self.state
    }

    fn new_peer(&self, cx: &mut Context<Self>) -> Self {
        let mut pane = Self::new(cx);
        pane.state.set_peers(self.state.peers.clone());
        pane
    }
}

impl Render for ItemPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.state.end_hover(cx);
        let ui = ui_theme(cx);
        let body = match self.state.active_item() {
            Some(item) => item.to_any_view().into_any_element(),
            None => div().into_any_element(),
        };
        let tab_bar = (!self.state.tab_bar_hidden).then(|| {
            div()
                .relative()
                .flex_none()
                .w_full()
                .h(TAB_HEIGHT)
                .child(self.state.tab_bar(cx))
                .child(self.state.record_tab_bar_bounds())
        });
        div()
            .track_focus(&self.state.focus)
            .capture_any_mouse_down(cx.listener(|_, _, _, cx| cx.emit(PaneEvent::Activated)))
            .on_drag_move(
                cx.listener(|pane, event: &DragMoveEvent<DraggedTab<Self>>, _, cx| {
                    if pane.state.drag_moved(event) {
                        cx.notify();
                    }
                }),
            )
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(ui.background)
            .child(self.state.record_bounds())
            .children(tab_bar)
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(self.state.record_content_bounds())
                    .child(body)
                    .children(self.state.drop_layer(cx)),
            )
            .children(self.state.drop_hint(cx))
    }
}

/// What tells each tab apart from the others with its title, as VS Code
/// does: the fewest trailing folders of its path's parent, joined with `/`,
/// that no other path among them ends its folders with, comparing whole
/// components (`src` and `tests` for `x/src/ex.rs` and `x/src/tests/ex.rs`).
/// A path whose folders run out first is described by its whole parent.
/// A tab with a title of its own, no path, or a path another tab with its
/// title shares (two views of one file, say) has none.
fn tab_descriptions(
    titles: &[SharedString],
    paths: &[Option<PathBuf>],
) -> Vec<Option<SharedString>> {
    let folders = |path: &Path| -> Vec<String> {
        let parent = path.parent().unwrap_or(Path::new(""));
        (parent.components())
            .filter_map(|part| match part {
                Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
                _ => None,
            })
            .collect()
    };
    (titles.iter().zip(paths).enumerate())
        .map(|(ix, (title, path))| {
            let path = path.as_deref()?;
            let others: Vec<&Path> = (titles.iter().zip(paths).enumerate())
                .filter(|&(other_ix, (other, _))| other_ix != ix && other == title)
                .filter_map(|(_, (_, other))| other.as_deref())
                .collect();
            if others.is_empty() || others.contains(&path) {
                return None;
            }
            let own = folders(path);
            let others: Vec<_> = others.into_iter().map(folders).collect();
            let unique = (1..=own.len()).find(|&n| {
                let tail = &own[own.len() - n..];
                !others.iter().any(|other| other.ends_with(tail))
            });
            let description = match unique {
                Some(n) => own[own.len() - n..].join("/"),
                None => path.parent()?.to_string_lossy().into_owned(),
            };
            (!description.is_empty()).then(|| description.into())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use gpui::SharedString;

    use super::tab_descriptions;

    /// Each path's file name as its title, and the descriptions for them.
    fn described(paths: &[&str]) -> Vec<Option<String>> {
        let paths: Vec<_> = paths.iter().map(|path| Some(PathBuf::from(path))).collect();
        let titles: Vec<SharedString> = (paths.iter().flatten())
            .map(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
                    .into()
            })
            .collect();
        (tab_descriptions(&titles, &paths).into_iter())
            .map(|description| description.map(String::from))
            .collect()
    }

    #[test]
    fn two_views_of_a_file_beside_a_namesake() {
        assert_eq!(
            described(&["a/src/y.rs", "a/src/y.rs", "b/src/y.rs"]),
            [None, None, Some("b/src".into())],
            "the twins share a path, so none tells them apart"
        );
    }
}
