//! The app's one chat-store worker: every read and write of chat files goes
//! through it, in the order asked, so a list or load sees every earlier
//! accepted write and nothing is replayed.
//!
//! A request carries a [`Token`] for the folder it was made for. The worker
//! checks the token against the [`Authority`] when it runs the filesystem
//! operation, not when the request was queued, and refuses a stale one:
//! revoking a folder's trust, or trusting it again, bumps its generation.
//! The worker is joined at app quit (exactly once), after it has run what
//! was accepted.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    fmt,
    future::Future,
    path::PathBuf,
    pin::Pin,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    task::{Context, Poll, Waker},
    thread::JoinHandle,
};

use fe_chat::{ChatId, ChatStore, ChatSummary, Conversation, StoreError};
use gpui::{App, AppContext as _, Global};
use kiln_l10n::tr;

/// The folder a request is for: its canonical root and its chats'
/// directory, fixed together when the target was made from the root: it is never worked out again
/// where the filesystem is touched, so a root alias retargeted since can't
/// redirect a request.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Target {
    pub(crate) dir: PathBuf,
    /// The canonical root it was made from, whose trust decision it
    /// follows; none for a window with no folder, which has none to ask.
    pub(crate) root: Option<PathBuf>,
}

impl Target {
    fn store(&self) -> ChatStore {
        ChatStore::new(&self.dir)
    }
}

/// Permission to use a folder's chats, as of one generation of its trust.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Token {
    target: Target,
    generation: u64,
    /// A panel's hold on the folder, which it gives up on leaving it: the
    /// requests made under it go stale with it.
    lease: Option<u64>,
    /// Good for one request: its lease ends when that has run.
    once: bool,
}

/// Why a request wasn't done.
#[derive(Debug)]
pub(crate) enum WorkerError {
    /// The folder's trust changed after the request was made.
    Stale,
    /// The worker was shut down.
    Closed,
    Store(StoreError),
    /// A save found the chat's file written since the revision it was
    /// derived from, and wrote nothing.
    Diverged,
}

/// `title` for the copy of its chat: stored, so without the isolates the
/// catalogue puts round the title.
fn copy_title(title: &str) -> String {
    let named = tr!("workbench-chat-copy-title", title = title);
    kiln_l10n::strip_isolates(&named).into_owned()
}

impl fmt::Display for WorkerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WorkerError::Stale => f.write_str(&tr!("workbench-chat-why-trust-changed")),
            WorkerError::Closed => f.write_str(&tr!("workbench-chat-why-quitting")),
            WorkerError::Store(error) => error.fmt(f),
            WorkerError::Diverged => f.write_str(&tr!("workbench-chat-why-changed-elsewhere")),
        }
    }
}

/// Each folder's trust and its generation: a token is good while both
/// stand.
#[derive(Default)]
struct Authority {
    folders: HashMap<Target, (bool, u64)>,
    leases: HashSet<u64>,
    next_lease: u64,
}

impl Authority {
    fn allows(&self, token: &Token) -> bool {
        self.folders.get(&token.target) == Some(&(true, token.generation))
            && token.lease.is_none_or(|lease| self.leases.contains(&lease))
    }
}

/// What a chat's saves have to go on: the revision of its file they derive
/// from, and, once a save found the file written elsewhere, the copy the
/// chat goes on in. Shared by the panel and the saves it has queued.
pub(crate) struct SaveState {
    revision: AtomicU64,
    copy: Mutex<Option<CopyOf>>,
}

#[derive(Clone)]
struct CopyOf {
    id: ChatId,
    title: String,
    revision: u64,
}

impl SaveState {
    /// For a chat whose file is at `revision` (0 if not written yet).
    pub(crate) fn new(revision: u64) -> Arc<SaveState> {
        Arc::new(SaveState {
            revision: AtomicU64::new(revision),
            copy: Mutex::new(None),
        })
    }

    /// The revision the chat's saves derive from.
    pub(crate) fn revision(&self) -> u64 {
        self.revision.load(Ordering::SeqCst)
    }

    /// Whether the chat's later turns have gone to a copy.
    pub(crate) fn has_copy(&self) -> bool {
        self.lock_copy().is_some()
    }

    fn lock_copy(&self) -> std::sync::MutexGuard<'_, Option<CopyOf>> {
        self.copy
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// What a save did.
#[derive(Debug)]
pub(crate) enum Saved {
    /// Written to chat `id` (the chat itself, or the copy it went on in),
    /// whose file is now at `revision`.
    Written {
        id: ChatId,
        title: String,
        revision: u64,
    },
    /// The file had been written since; the chat went to this new chat.
    Copied {
        id: ChatId,
        title: String,
        revision: u64,
    },
}

/// An answer to come: a future that is ready once the worker has run the
/// request. Dropping it doesn't cancel the request.
pub(crate) struct Reply<T> {
    slot: Arc<Mutex<Slot<T>>>,
}

struct Slot<T> {
    value: Option<Result<T, WorkerError>>,
    waker: Option<Waker>,
}

impl<T> Reply<T> {
    fn pair() -> (Reply<T>, Arc<Mutex<Slot<T>>>) {
        let slot = Arc::new(Mutex::new(Slot {
            value: None,
            waker: None,
        }));
        (Reply { slot: slot.clone() }, slot)
    }
}

fn answer<T>(slot: &Mutex<Slot<T>>, value: Result<T, WorkerError>) {
    let mut slot = slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    slot.value = Some(value);
    if let Some(waker) = slot.waker.take() {
        waker.wake();
    }
}

impl<T> Future for Reply<T> {
    type Output = Result<T, WorkerError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut slot = (self.slot.lock()).unwrap_or_else(|poisoned| poisoned.into_inner());
        match slot.value.take() {
            Some(value) => Poll::Ready(value),
            None => {
                slot.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

type Job = Box<dyn FnOnce(&Shared) + Send>;

#[derive(Default)]
struct Queue {
    jobs: VecDeque<Job>,
    closed: bool,
}

struct Shared {
    queue: Mutex<Queue>,
    ready: Condvar,
    authority: Mutex<Authority>,
    /// How many requests were refused as stale.
    refused: AtomicUsize,
}

impl Shared {
    fn run(&self, job: Job) {
        job(self);
    }

    /// Runs `check` on the authority, holding its lock only for that and
    /// never across filesystem work, so the UI thread's updates of it don't
    /// wait for the disk.
    fn with_authority<R>(&self, check: impl FnOnce(&mut Authority) -> R) -> R {
        let mut authority =
            (self.authority.lock()).unwrap_or_else(|poisoned| poisoned.into_inner());
        check(&mut authority)
    }
}

/// The worker: a thread running the queue in order, or under test a queue
/// the test runs by hand with [`Worker::run_pending`], so a test decides
/// when the filesystem is touched.
pub(crate) struct Worker {
    shared: Arc<Shared>,
    thread: Mutex<Option<JoinHandle<()>>>,
    manual: bool,
}

impl Global for WorkerHandle {}

#[derive(Clone)]
pub(crate) struct WorkerHandle(pub(crate) Arc<Worker>);

impl Worker {
    fn new(manual: bool) -> Arc<Worker> {
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue::default()),
            ready: Condvar::new(),
            authority: Mutex::new(Authority::default()),
            refused: AtomicUsize::new(0),
        });
        let thread = (!manual).then(|| {
            let shared = shared.clone();
            std::thread::Builder::new()
                .name("chat-store".into())
                .spawn(move || {
                    loop {
                        let job = {
                            let mut queue = (shared.queue.lock())
                                .unwrap_or_else(|poisoned| poisoned.into_inner());
                            loop {
                                if let Some(job) = queue.jobs.pop_front() {
                                    break Some(job);
                                }
                                if queue.closed {
                                    break None;
                                }
                                queue = (shared.ready.wait(queue))
                                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                            }
                        };
                        match job {
                            Some(job) => shared.run(job),
                            None => return,
                        }
                    }
                })
                .expect("the chat store thread starts")
        });
        Arc::new(Worker {
            shared,
            thread: Mutex::new(thread),
            manual,
        })
    }

    /// The app's worker, made on first use and joined at quit. Under test it
    /// is the by-hand kind.
    pub(crate) fn of(cx: &mut App) -> Arc<Worker> {
        if let Some(WorkerHandle(worker)) = cx.try_global::<WorkerHandle>() {
            return worker.clone();
        }
        let worker = Worker::new(cfg!(test));
        cx.set_global(WorkerHandle(worker.clone()));
        let quitting = worker.clone();
        cx.on_app_quit(move |cx| {
            let quitting = quitting.clone();
            cx.background_spawn(async move { quitting.shutdown() })
        })
        .detach();
        worker
    }

    /// Starts tracking `target`, trusted or not as the decisions say now. A
    /// folder already tracked keeps what it has: [`Worker::refresh`] follows
    /// the decisions.
    pub(crate) fn track(&self, target: &Target, trusted: bool) {
        (self.shared.authority.lock())
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .folders
            .entry(target.clone())
            .or_insert((trusted, 0));
    }

    /// Sets every tracked folder's trust from the decisions, `trusted_of`.
    /// A change bumps the folder's generation, so tokens made before it go
    /// stale; a folder no window shows any more is included.
    pub(crate) fn refresh(&self, trusted_of: impl Fn(&Target) -> bool) {
        let mut authority =
            (self.shared.authority.lock()).unwrap_or_else(|poisoned| poisoned.into_inner());
        for (target, entry) in authority.folders.iter_mut() {
            let trusted = trusted_of(target);
            if entry.0 != trusted {
                *entry = (trusted, entry.1 + 1);
            }
        }
    }

    /// A token for `target` as it stands, if its folder is trusted.
    #[cfg(test)]
    pub(crate) fn token(&self, target: &Target) -> Option<Token> {
        self.token_under(target, None, false)
    }

    /// A new lease: the hold a panel has on the folder it shows.
    pub(crate) fn lease(&self) -> u64 {
        let mut authority =
            (self.shared.authority.lock()).unwrap_or_else(|poisoned| poisoned.into_inner());
        authority.next_lease += 1;
        let lease = authority.next_lease;
        authority.leases.insert(lease);
        lease
    }

    /// Ends `lease`: what was asked under it and hasn't run is refused.
    pub(crate) fn release(&self, lease: u64) {
        (self.shared.authority.lock())
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .leases
            .remove(&lease);
    }

    /// Gives back a token not used: a one-shot's lease ends with it.
    pub(crate) fn release_token(&self, token: Token) {
        if let Some(lease) = token.lease.filter(|_| token.once) {
            self.release(lease);
        }
    }

    /// A token for `target` under `lease`, if its folder is trusted.
    pub(crate) fn token_leased(&self, target: &Target, lease: u64) -> Option<Token> {
        self.token_under(target, Some(lease), false)
    }

    /// A token for one request, under a lease of its own that ends when it
    /// has run.
    pub(crate) fn token_once(&self, target: &Target) -> Option<Token> {
        let lease = self.lease();
        let token = self.token_under(target, Some(lease), true);
        if token.is_none() {
            self.release(lease);
        }
        token
    }

    fn token_under(&self, target: &Target, lease: Option<u64>, once: bool) -> Option<Token> {
        let authority =
            (self.shared.authority.lock()).unwrap_or_else(|poisoned| poisoned.into_inner());
        let &(trusted, generation) = authority.folders.get(target)?;
        trusted.then(|| Token {
            target: target.clone(),
            generation,
            lease,
            once,
        })
    }

    fn submit<T: Send + 'static>(
        &self,
        token: Token,
        read: bool,
        work: impl FnOnce(ChatStore) -> Result<T, StoreError> + Send + 'static,
    ) -> Reply<T> {
        let (reply, slot) = Reply::pair();
        let job: Job = Box::new(move |shared| {
            // Checked as the filesystem is about to be touched, and for a
            // read again once it has been: what was read for authority that
            // went meanwhile is not handed back.
            let result = if shared.with_authority(|authority| authority.allows(&token)) {
                let result = work(token.target.store()).map_err(|error| match error {
                    StoreError::Diverged { .. } => WorkerError::Diverged,
                    error => WorkerError::Store(error),
                });
                if read && !shared.with_authority(|authority| authority.allows(&token)) {
                    shared.refused.fetch_add(1, Ordering::SeqCst);
                    Err(WorkerError::Stale)
                } else {
                    result
                }
            } else {
                shared.refused.fetch_add(1, Ordering::SeqCst);
                Err(WorkerError::Stale)
            };
            if let Some(lease) = token.lease.filter(|_| token.once) {
                shared.with_authority(|authority| authority.leases.remove(&lease));
            }
            answer(&slot, result);
        });
        let mut queue = (self.shared.queue.lock()).unwrap_or_else(|poisoned| poisoned.into_inner());
        if queue.closed {
            drop(queue);
            // Dropping the job drops its slot's other half; answer through
            // a fresh look at it.
            let (reply, slot) = Reply::pair();
            answer(&slot, Err(WorkerError::Closed));
            return reply;
        }
        queue.jobs.push_back(job);
        self.shared.ready.notify_one();
        reply
    }

    /// Saves `chat` if its file is at the revision `state` holds, the one
    /// its last accepted save left (the worker runs requests in order, so a
    /// save queued behind another sees its result). If the file was written
    /// since, nothing of it is touched: the same operation writes `chat` as
    /// a new chat, "(copy)", and the chat's later saves go on in that copy.
    /// Nothing depends on the caller after the request is accepted.
    pub(crate) fn save(
        &self,
        token: Token,
        chat: Conversation,
        state: Arc<SaveState>,
    ) -> Reply<Saved> {
        self.submit(token, false, move |store| {
            // The chat as a new chat of its own, written here and now; the
            // chat's later saves go on in it.
            let copy_of = |chat: Conversation| -> Result<Saved, StoreError> {
                let mut copy = chat;
                copy.id = ChatId::new();
                copy.title = copy_title(&copy.title);
                let revision = store.save_if(&copy, 0)?;
                let (id, title) = (copy.id, copy.title.clone());
                *state.lock_copy() = Some(CopyOf {
                    id,
                    title: title.clone(),
                    revision,
                });
                Ok(Saved::Copied {
                    id,
                    title,
                    revision,
                })
            };
            let existing = state.lock_copy().clone();
            if let Some(copy) = existing {
                let mut moved = chat.clone();
                (moved.id, moved.title) = (copy.id, copy.title.clone());
                return match store.save_if(&moved, copy.revision) {
                    Ok(revision) => {
                        if let Some(copy) = state.lock_copy().as_mut() {
                            copy.revision = revision;
                        }
                        Ok(Saved::Written {
                            id: copy.id,
                            title: copy.title,
                            revision,
                        })
                    }
                    // The copy was written elsewhere too: another copy.
                    Err(StoreError::Diverged { .. }) => copy_of(chat),
                    Err(error) => Err(error),
                };
            }
            match store.save_if(&chat, state.revision.load(Ordering::SeqCst)) {
                Ok(revision) => {
                    state.revision.store(revision, Ordering::SeqCst);
                    Ok(Saved::Written {
                        id: chat.id,
                        title: chat.title,
                        revision,
                    })
                }
                Err(StoreError::Diverged { .. }) => copy_of(chat),
                Err(error) => Err(error),
            }
        })
    }

    pub(crate) fn delete(&self, token: Token, id: ChatId) -> Reply<()> {
        self.submit(token, false, move |store| store.delete(id))
    }

    pub(crate) fn list(&self, token: Token) -> Reply<Vec<ChatSummary>> {
        self.submit(token, true, |store| Ok(store.list()))
    }

    pub(crate) fn load(&self, token: Token, id: ChatId) -> Reply<(Conversation, u64)> {
        self.submit(token, true, move |store| store.load_revision(id))
    }

    /// The folder's newest readable chat, if any.
    pub(crate) fn newest(&self, token: Token) -> Reply<Option<(Conversation, u64)>> {
        self.submit(token, true, |store| {
            Ok(store.list().into_iter().find_map(|summary| {
                (store.load_revision(summary.id))
                    .map_err(|error| log::warn!("chat: {error}"))
                    .ok()
            }))
        })
    }

    /// How many requests were refused as stale.
    #[cfg(test)]
    pub(crate) fn refused(&self) -> usize {
        self.shared.refused.load(Ordering::SeqCst)
    }

    /// Runs what is queued, in order. For the by-hand worker.
    pub(crate) fn run_pending(&self) -> usize {
        let mut ran = 0;
        loop {
            let job = (self.shared.queue.lock())
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .jobs
                .pop_front();
            let Some(job) = job else {
                return ran;
            };
            self.shared.run(job);
            ran += 1;
        }
    }

    /// Runs everything accepted, then stops; later requests are refused.
    /// Done once: a second call finds nothing to do.
    pub(crate) fn shutdown(&self) {
        (self.shared.queue.lock())
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .closed = true;
        self.shared.ready.notify_all();
        if self.manual {
            self.run_pending();
        }
        let thread = (self.thread.lock())
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        if let Some(thread) = thread {
            thread.join().ok();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    /// A worker thread blocked in I/O doesn't keep the UI thread from
    /// updating the authority, and a read whose authority went meanwhile
    /// is not handed back.
    #[test]
    fn authority_updates_dont_wait_for_the_disk() {
        let worker = Worker::new(false);
        let target = Target {
            dir: PathBuf::from("/nowhere/chats"),
            root: Some(PathBuf::from("/nowhere")),
        };
        worker.track(&target, true);
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let blocked_read = worker.submit(worker.token(&target).unwrap(), true, move |_| {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            Ok(1)
        });
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();

        // The UI thread's updates, while the worker is in the middle of I/O.
        let (done_tx, done_rx) = mpsc::channel();
        let ui = worker.clone();
        let ui_target = target.clone();
        std::thread::spawn(move || {
            ui.track(&ui_target, true);
            let lease = ui.lease();
            ui.release(lease);
            ui.refresh(|_| false);
            done_tx.send(()).unwrap();
        });
        done_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("the authority isn't held across I/O");

        release_tx.send(()).unwrap();
        let result = futures::executor::block_on(blocked_read);
        assert!(matches!(result, Err(WorkerError::Stale)));
        worker.shutdown();
    }
}

#[cfg(test)]
mod copy_title_tests {
    use super::copy_title;

    #[test]
    fn a_copy_title_is_the_title_and_copy_and_nothing_else() {
        assert_eq!(copy_title("first"), "first (copy)");
        assert_eq!(copy_title("שלום"), "שלום (copy)");
        assert_eq!(copy_title("مرحبا"), "مرحبا (copy)");
    }
}
