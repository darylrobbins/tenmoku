// SPDX-License-Identifier: Apache-2.0

//! Closing items: every path that removes items asks them first, through
//! [`confirm`]. Kiln's own paths do: a tab's close
//! ([`PaneView::close_active_item`](crate::PaneView::close_active_item)),
//! a tab dropped on a pane that shows its document already
//! ([`PaneGroup::move_tab`](crate::PaneGroup::move_tab)), a window's close
//! (`window.close`, or the platform's close button, which [`Shell`](crate::Shell)
//! hears) and quitting (`app.quit`, every window's items, window by
//! window). A host closing a pane of its own builds a [`CloseRequest`] of
//! the pane's items.
//!
//! Each item is asked [`PaneItem::may_close`](crate::PaneItem::may_close)
//! in order, unless an item that stays open shows its document, or one the
//! request closes has agreed for that document already. When every answer
//! is `Close`, the close happens at once, in the caller's update
//! ([`Closing::Now`]). An `Ask` makes the request a close transaction,
//! which:
//!
//! - **is one of its kind at a time.** While one waits on an answer, a
//!   request for the same close (the same kind, window and items: a second
//!   click on the close button) joins it and resolves as it does; any other
//!   waits for it to resolve, then starts, reading its items afresh. One
//!   that has nothing to ask (each item it closes has its document open in
//!   an item that stays) closes at once all the same.
//! - **remembers each answer with the item's
//!   [`close_revision`](crate::PaneItem::close_revision)** as it was asked.
//! - **revalidates before it commits.** After each answer it reads its
//!   scope afresh (the items of the pane, the window or every window, which
//!   may have gained some since) and asks, in order, every item it would
//!   close that hasn't agreed, or whose revision has changed since it did.
//!   Once a pass asks nothing, the close is made in that same update. Any
//!   `Keep`, or an `Ask` that resolves `false`, stops it, and nothing
//!   closes. So a quit runs only once every window's items have agreed as
//!   they are then.
//!
//! The windows' items come from their hosts: [`set_window_items`].

use std::{
    cell::RefCell,
    collections::HashMap,
    future::Future,
    pin::Pin,
    rc::Rc,
    task::{Context as TaskContext, Poll, Waker},
};

use gpui::{AnyWindowHandle, App, AsyncApp, Global, Task, Window, WindowId};

use crate::{CloseAnswer, ItemHandle, ItemId};

/// What a request closes: of two requests waiting at once, only two of
/// the same kind, in the same window, over the same items, are one close.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseKind {
    /// Some items, such as a tab.
    Items,
    /// The items a rule picks among some others (all of a pane's, say, but
    /// those it keeps), which may pick fewer by the time it closes them:
    /// never one close with [`CloseKind::Items`] over the same items, so a
    /// request for an item itself doesn't resolve as the rule's does.
    Bulk,
    /// A pane, with its items.
    Pane,
    /// A window, with its items.
    Window,
    /// Every window: quitting.
    Quit,
}

/// The items a request closes in a window, and every item open there, so
/// it can tell which of them stay open.
#[derive(Clone, Default)]
pub struct Items {
    pub closing: Vec<Box<dyn ItemHandle>>,
    pub open: Vec<Box<dyn ItemHandle>>,
}

/// Reads a request's [`Items`] in a window afresh, for each pass after the
/// first: `None` once there's nothing of it left (its pane has gone).
pub type LiveItems = Rc<dyn Fn(&App) -> Option<Items>>;

/// Every item open in a window, as its host reads them.
pub type WindowItems = Rc<dyn Fn(&App) -> Vec<Box<dyn ItemHandle>>>;

/// What a confirmed close does, in the request's first window (`None` if
/// that has closed meanwhile), when it goes ahead after waiting.
pub type Commit = Box<dyn FnOnce(Option<&mut Window>, &mut App)>;

/// The items to close, window by window.
pub struct CloseRequest {
    kind: CloseKind,
    scopes: Vec<Scope>,
}

struct Scope {
    window: AnyWindowHandle,
    /// The items as the caller read them, for the first pass, in its
    /// update; `None` reads them live then too.
    now: Option<Items>,
    live: LiveItems,
}

impl CloseRequest {
    pub fn new(kind: CloseKind) -> Self {
        Self {
            kind,
            scopes: Vec::new(),
        }
    }

    /// Closes items in `window`: `now`, as the caller reads them in its
    /// update (or `None` to read them through `live` then as well), and
    /// `live`, read afresh on each later pass.
    pub fn scope(mut self, window: AnyWindowHandle, now: Option<Items>, live: LiveItems) -> Self {
        self.scopes.push(Scope { window, now, live });
        self
    }

    fn home(&self) -> Option<AnyWindowHandle> {
        self.scopes.first().map(|scope| scope.window)
    }

    /// The scope's items for this pass.
    fn items(scope: &mut Scope, cx: &App) -> Items {
        match scope.now.take() {
            Some(items) => items,
            None => (scope.live)(cx).unwrap_or_default(),
        }
    }

    /// What tells it from another request.
    fn key(&self, cx: &App) -> Key {
        let mut items: Vec<u64> = (self.scopes.iter())
            .flat_map(|scope| match &scope.now {
                Some(items) => items.closing.clone(),
                None => (scope.live)(cx).unwrap_or_default().closing,
            })
            .map(|item| item.id().0.as_u64())
            .collect();
        items.sort_unstable();
        Key {
            kind: self.kind,
            home: self.home().map(|home| home.window_id()),
            items,
        }
    }

    /// Whether any item it closes would be asked: one whose document no
    /// item staying open shows.
    fn asks(&self, cx: &App) -> bool {
        self.scopes.iter().any(|scope| {
            let items = match &scope.now {
                Some(items) => items.clone(),
                None => (scope.live)(cx).unwrap_or_default(),
            };
            let survivors = survivors(&items);
            (items.closing.iter()).any(|item| !shown_by(&**item, &survivors, cx))
        })
    }
}

/// The open items a request doesn't close.
fn survivors(items: &Items) -> Vec<Box<dyn ItemHandle>> {
    let closing: Vec<ItemId> = items.closing.iter().map(|item| item.id()).collect();
    (items.open.iter())
        .filter(|item| !closing.contains(&item.id()))
        .cloned()
        .collect()
}

/// Whether one of `others` shows `item`'s document.
fn shown_by(item: &dyn ItemHandle, others: &[Box<dyn ItemHandle>], cx: &App) -> bool {
    others.iter().any(|other| other.shares_document(item, cx))
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Key {
    kind: CloseKind,
    home: Option<WindowId>,
    items: Vec<u64>,
}

/// What [`confirm`] found.
pub enum Closing {
    /// Every item may close: close them now, in this update. The commit
    /// isn't run.
    Now,
    /// An item stays open, and so does everything else: nothing closes.
    Refused,
    /// Waiting on an answer, or on another close: if every item agrees,
    /// the commit runs later, in an update of its own. The task resolves
    /// to whether it did.
    Later(Task<bool>),
}

impl std::fmt::Debug for Closing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Now => "Now",
            Self::Refused => "Refused",
            Self::Later(_) => "Later",
        })
    }
}

/// The close transaction waiting on an answer, if any, and who waits on it.
#[derive(Default)]
struct Closes {
    pending: Option<Pending>,
    windows: HashMap<WindowId, WindowItems>,
    quit: Option<Rc<Quit>>,
}

/// What a confirmed quit runs: see [`set_quit`].
type Quit = dyn Fn(&mut App);

impl Global for Closes {}

struct Pending {
    key: Key,
    /// Requests for the same close, which resolve as it does.
    joined: Vec<Sender<bool>>,
    /// Requests for other closes, which start once it resolves.
    waiting: Vec<Sender<()>>,
}

/// Has `app.quit` and `window.close` ask the windows' items before they
/// quit or close one ([`quit`], [`close_window`]), and forgets a window's
/// items once it closes. [`crate::init`] calls it.
pub fn init(cx: &mut App) {
    if cx.has_global::<Closes>() {
        return;
    }
    cx.set_global(Closes::default());
    kiln_platform::commands::set_close_handlers(cx, quit, |handle, cx| {
        match close_window(handle, None, cx) {
            Closing::Now => {
                handle
                    .update(cx, |_, window, _| window.remove_window())
                    .ok();
            }
            Closing::Refused => {}
            Closing::Later(closing) => closing.detach(),
        }
    });
    cx.on_window_closed(|cx, closed| {
        cx.default_global::<Closes>().windows.remove(&closed);
    })
    .detach();
}

/// Has a window's close and quitting ask `items`, every item open in
/// `window`, as the host reads them. They're read outside any update of
/// the host's, so the host's views can be read.
pub fn set_window_items(window: &Window, items: WindowItems, cx: &mut App) {
    let id = window.window_handle().window_id();
    cx.default_global::<Closes>().windows.insert(id, items);
}

/// What a confirmed quit runs: `cx.quit()`, unless a host has it do more,
/// or a test records it.
pub fn set_quit(cx: &mut App, quit: impl Fn(&mut App) + 'static) {
    cx.default_global::<Closes>().quit = Some(Rc::new(quit));
}

/// Every item open in the window `handle`, as its host reads them: `None`
/// for a window whose host gave none.
pub fn window_items(handle: AnyWindowHandle, cx: &App) -> Option<Vec<Box<dyn ItemHandle>>> {
    let items = cx
        .try_global::<Closes>()?
        .windows
        .get(&handle.window_id())?;
    Some(items(cx))
}

/// Asks the items in `window` (`window` itself, if it's being updated, or
/// else `None`) whether it may close: every item open there. A window whose
/// host gave no items closes at once. On [`Closing::Later`] the window is
/// removed once every item agrees.
pub fn close_window(handle: AnyWindowHandle, window: Option<&mut Window>, cx: &mut App) -> Closing {
    let Some(items) = cx
        .try_global::<Closes>()
        .and_then(|closes| closes.windows.get(&handle.window_id()).cloned())
    else {
        return Closing::Now;
    };
    let live: LiveItems = Rc::new(move |cx| {
        let open = items(cx);
        Some(Items {
            closing: open.clone(),
            open,
        })
    });
    let request = CloseRequest::new(CloseKind::Window).scope(handle, None, live);
    let commit = |window: Option<&mut Window>, _: &mut App| {
        if let Some(window) = window {
            window.remove_window();
        }
    };
    confirm(request, commit, window, cx)
}

/// Quits once every window's items agree, asked window by window: the
/// first item to stay open stops the quit, with every window still open.
/// It runs once the current update is over, so that no window is being
/// updated.
pub fn quit(cx: &mut App) {
    cx.defer(|cx| {
        let windows: Vec<_> = cx
            .windows()
            .into_iter()
            .filter_map(|handle| {
                let items = cx
                    .try_global::<Closes>()?
                    .windows
                    .get(&handle.window_id())?;
                Some((handle, items.clone()))
            })
            .collect();
        let mut request = CloseRequest::new(CloseKind::Quit);
        for (handle, items) in windows {
            let live: LiveItems = Rc::new(move |cx| {
                let open = items(cx);
                Some(Items {
                    closing: open.clone(),
                    open,
                })
            });
            request = request.scope(handle, None, live);
        }
        let commit = |_: Option<&mut Window>, cx: &mut App| run_quit(cx);
        match confirm(request, commit, None, cx) {
            Closing::Now => run_quit(cx),
            Closing::Refused => {}
            Closing::Later(task) => task.detach(),
        }
    });
}

fn run_quit(cx: &mut App) {
    match cx
        .try_global::<Closes>()
        .and_then(|closes| closes.quit.clone())
    {
        Some(quit) => quit(cx),
        None => cx.quit(),
    }
}

/// Asks the items `request` closes whether they may (see the module docs).
/// `window` is the window being updated, if any: the caller's. On
/// [`Closing::Now`] the caller closes them; on [`Closing::Later`],
/// `commit` does, if they all agree.
pub fn confirm(
    request: CloseRequest,
    commit: impl FnOnce(Option<&mut Window>, &mut App) + 'static,
    window: Option<&mut Window>,
    cx: &mut App,
) -> Closing {
    let commit: Commit = Box::new(commit);
    let key = request.key(cx);
    let mut transaction = Transaction {
        request,
        approved: Vec::new(),
    };
    let pending = (cx.try_global::<Closes>())
        .and_then(|closes| closes.pending.as_ref().map(|pending| pending.key.clone()));
    if let Some(pending) = pending {
        let join = pending == key;
        if !join && !transaction.request.asks(cx) {
            return Closing::Now;
        }
        let pending = (cx.default_global::<Closes>().pending.as_mut()).expect("a close is pending");
        if join {
            let (sender, receiver) = oneshot();
            pending.joined.push(sender);
            return Closing::Later(cx.spawn(async move |_| receiver.await.unwrap_or(false)));
        }
        let (sender, receiver) = oneshot();
        pending.waiting.push(sender);
        return Closing::Later(cx.spawn(async move |cx| {
            receiver.await;
            queued(transaction, key, commit, cx).await
        }));
    }
    match transaction.pass(window, cx) {
        Pass::Ready => Closing::Now,
        Pass::Refused => Closing::Refused,
        Pass::Asked(asked) => {
            cx.default_global::<Closes>().pending = Some(Pending::new(key));
            Closing::Later(cx.spawn(async move |cx| transaction.finish(asked, commit, cx).await))
        }
    }
}

impl Pending {
    fn new(key: Key) -> Self {
        Self {
            key,
            joined: Vec::new(),
            waiting: Vec::new(),
        }
    }
}

/// A request that waited for another close to resolve: it joins one for
/// the same close, waits again for any other, or else starts, its items
/// read afresh.
async fn queued(mut transaction: Transaction, key: Key, commit: Commit, cx: &mut AsyncApp) -> bool {
    // Its items as the caller read them are stale after the wait: each
    // pass reads them live.
    for scope in &mut transaction.request.scopes {
        scope.now = None;
    }
    enum Next {
        Join(Receiver<bool>),
        Wait(Receiver<()>),
        Done(bool),
        Ask(Asked),
    }
    let mut commit = Some(commit);
    loop {
        let next = cx.update(|cx| {
            let closes = cx.default_global::<Closes>();
            if let Some(pending) = &mut closes.pending {
                if pending.key == key {
                    let (sender, receiver) = oneshot();
                    pending.joined.push(sender);
                    return Next::Join(receiver);
                }
                let (sender, receiver) = oneshot();
                pending.waiting.push(sender);
                return Next::Wait(receiver);
            }
            match transaction.pass(None, cx) {
                Pass::Ready => {
                    transaction.commit(commit_once(&mut commit), cx);
                    Next::Done(true)
                }
                Pass::Refused => Next::Done(false),
                Pass::Asked(asked) => {
                    cx.default_global::<Closes>().pending = Some(Pending::new(key.clone()));
                    Next::Ask(asked)
                }
            }
        });
        match next {
            Next::Join(receiver) => return receiver.await.unwrap_or(false),
            Next::Wait(receiver) => {
                receiver.await;
            }
            Next::Done(done) => return done,
            Next::Ask(asked) => {
                return transaction
                    .finish(asked, commit_once(&mut commit), cx)
                    .await;
            }
        }
    }
}

/// Takes the commit out of `slot`.
fn commit_once(slot: &mut Option<Commit>) -> Commit {
    slot.take().expect("a close commits once")
}

/// An item asked, and what it answered with.
struct Asked {
    item: Box<dyn ItemHandle>,
    revision: u64,
    answer: Task<bool>,
}

enum Pass {
    Ready,
    Refused,
    Asked(Asked),
}

struct Transaction {
    request: CloseRequest,
    /// Each item that agreed, with its revision when it was asked.
    approved: Vec<(Box<dyn ItemHandle>, u64)>,
}

impl Transaction {
    /// Asks, in order, each item to close that's still to agree as it is
    /// now, up to the first that refuses or asks the user.
    fn pass(&mut self, mut current: Option<&mut Window>, cx: &mut App) -> Pass {
        for ix in 0..self.request.scopes.len() {
            let scope = &mut self.request.scopes[ix];
            let handle = scope.window;
            let items = CloseRequest::items(scope, cx);
            let approved = &mut self.approved;
            let pass = with_window(handle, &mut current, cx, |window, cx| {
                ask(&items, approved, window, cx)
            });
            match pass {
                // Gone, and its items with it.
                None | Some(Pass::Ready) => {}
                Some(pass) => return pass,
            }
        }
        Pass::Ready
    }

    /// Waits for `asked`'s answer, then asks again until every item has
    /// agreed as it is, and commits in that same update; or until one
    /// refuses. Resolves the pending close either way.
    async fn finish(mut self, mut asked: Asked, commit: Commit, cx: &mut AsyncApp) -> bool {
        let mut commit = Some(commit);
        loop {
            let agreed = (&mut asked.answer).await;
            let next = cx.update(|cx| {
                let next = if agreed {
                    self.approved
                        .push((asked.item.boxed_clone(), asked.revision));
                    match self.pass(None, cx) {
                        Pass::Ready => {
                            self.commit(commit_once(&mut commit), cx);
                            Some(true)
                        }
                        Pass::Refused => Some(false),
                        Pass::Asked(next) => {
                            asked = next;
                            None
                        }
                    }
                } else {
                    Some(false)
                };
                if let Some(done) = next {
                    resolve(done, cx);
                }
                next
            });
            if let Some(done) = next {
                return done;
            }
        }
    }

    /// Runs `commit` in the request's first window, or without one if it
    /// has closed.
    fn commit(&self, commit: Commit, cx: &mut App) {
        let mut commit = Some(commit);
        if let Some(home) = self.request.home() {
            with_window(home, &mut None, cx, |window, cx| {
                commit_once(&mut commit)(Some(window), cx)
            });
        }
        if let Some(commit) = commit {
            commit(None, cx);
        }
    }
}

/// Asks each of `items.closing` in turn, but those an item staying open
/// shows the document of, or that agreed (themselves or for their
/// document) as they are now.
fn ask(
    items: &Items,
    approved: &mut Vec<(Box<dyn ItemHandle>, u64)>,
    window: &mut Window,
    cx: &mut App,
) -> Pass {
    let survivors = survivors(items);
    for item in &items.closing {
        if shown_by(&**item, &survivors, cx) {
            continue;
        }
        let revision = item.close_revision(cx);
        let agreed = approved.iter().any(|(other, at)| {
            let same = other.id() == item.id() && *at == revision;
            *at == other.close_revision(cx) && (same || other.shares_document(&**item, cx))
        });
        if agreed {
            continue;
        }
        match item.may_close(window, cx) {
            CloseAnswer::Close => approved.push((item.boxed_clone(), revision)),
            CloseAnswer::Keep => {
                log::warn!("not closing {}: it stays open", item.tab_title(cx));
                return Pass::Refused;
            }
            CloseAnswer::Ask(answer) => {
                return Pass::Asked(Asked {
                    item: item.boxed_clone(),
                    revision,
                    answer,
                });
            }
        }
    }
    Pass::Ready
}

/// Ends the pending close with `done`: those that joined it resolve so,
/// and those waiting on it go on.
fn resolve(done: bool, cx: &mut App) {
    let Some(pending) = cx.default_global::<Closes>().pending.take() else {
        return;
    };
    for joined in pending.joined {
        joined.send(done);
    }
    for waiting in pending.waiting {
        waiting.send(());
    }
}

/// Runs `f` in the window `handle`: `current`, if that's it, or else by
/// updating it. `None` if it has closed.
fn with_window<R>(
    handle: AnyWindowHandle,
    current: &mut Option<&mut Window>,
    cx: &mut App,
    f: impl FnOnce(&mut Window, &mut App) -> R,
) -> Option<R> {
    if let Some(window) = current.as_deref_mut()
        && window.window_handle() == handle
    {
        return Some(f(window, cx));
    }
    handle.update(cx, |_, window, cx| f(window, cx)).ok()
}

/// One value, from a [`Sender`] to a [`Receiver`] on the same thread.
struct Slot<T> {
    value: Option<T>,
    closed: bool,
    waker: Option<Waker>,
}

struct Sender<T>(Rc<RefCell<Slot<T>>>);

struct Receiver<T>(Rc<RefCell<Slot<T>>>);

fn oneshot<T>() -> (Sender<T>, Receiver<T>) {
    let slot = Rc::new(RefCell::new(Slot {
        value: None,
        closed: false,
        waker: None,
    }));
    (Sender(slot.clone()), Receiver(slot))
}

impl<T> Sender<T> {
    fn send(self, value: T) {
        self.0.borrow_mut().value = Some(value);
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        let mut slot = self.0.borrow_mut();
        slot.closed = true;
        if let Some(waker) = slot.waker.take() {
            waker.wake();
        }
    }
}

/// Resolves to the value sent, or `None` if the sender went without one.
impl<T> Future for Receiver<T> {
    type Output = Option<T>;

    fn poll(self: Pin<&mut Self>, task: &mut TaskContext<'_>) -> Poll<Option<T>> {
        let mut slot = self.0.borrow_mut();
        if let Some(value) = slot.value.take() {
            return Poll::Ready(Some(value));
        }
        if slot.closed {
            return Poll::Ready(None);
        }
        slot.waker = Some(task.waker().clone());
        Poll::Pending
    }
}
