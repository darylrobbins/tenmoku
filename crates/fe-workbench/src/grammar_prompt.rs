//! Offers to install syntax highlighting when a file opens in a language with
//! a first-party grammar that isn't installed: a toast with Install (size),
//! Not now and Never for .ext. The decision record is
//! `docs/decisions/pp118n3cn5-grammar-prompt.md`.
//!
//! The app supplies the [`GrammarOffers`] ([`set_grammar_offers`]): what can be
//! installed and how, so this crate has no network code and no installer.
//! Without them nothing is ever offered.
//!
//! - **Consent.** Install is the only thing that calls [`GrammarOffers::fetch`]
//!   and then [`GrammarOffers::install`]. Closing the toast, or losing it any
//!   other way, counts as Not now.
//! - **Not now** silences a language in every window until the next launch.
//!   **Never** is per extension, in [`PROMPT_FILE`] in the state directory,
//!   which is read again before each offer so that another window or process's
//!   answer counts.
//! - **One at a time.** A language being asked about or installed isn't offered
//!   again, in any window. An extension that opened meanwhile waits, for when
//!   Never for another extension of the language ends the prompt. Each prompt
//!   belongs to the window that showed it and stops with it, a download in
//!   progress too, until the download is done.
//! - **Nothing waits for this.** Tabs open as usual; the file is read, and the
//!   toast shown, off the back of that.
//!
//! An installed extension is registered by the extension loader on a thread of
//! its own, with no signal, so an install then watches
//! [`fe_syntax::language_names`] for the language, and detects the language of
//! every tab of it, in every window, once it's there. Writing it and all of that
//! belong to no window, as they can't be called back, and the watching goes on past
//! the warning that it's taking too long.

use std::{
    cell::RefCell,
    collections::{BTreeSet, HashMap, HashSet},
    fs, io,
    io::Write as _,
    path::{Path, PathBuf},
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use fe_gpui::{DiffView, EditorView};
use futures::channel::oneshot;
use gpui::{
    App, AppContext as _, AsyncApp, Context, Entity, EntityId, Global, Task, WeakEntity, Window,
    WindowId,
};
use serde::{Deserialize, Serialize};

use crate::{
    Workbench,
    workbench::toast::{self, Toast, ToastId, ToastLayer},
};

/// The file in the state directory that holds the extensions never to offer for.
pub const PROMPT_FILE: &str = "grammar_prompt.json";

/// The indexes of the offer's answers, Not now being the one between.
const INSTALL: usize = 0;
const NEVER: usize = 2;

/// How often an install looks for the language to have registered.
const REGISTERED_POLL: Duration = Duration::from_millis(250);

/// How long before an install warns that it hasn't: the loader's watcher looks
/// every few seconds at a directory that didn't exist before, and compiling a
/// module of a few MB can take seconds more, longer on a busy machine.
const REGISTERED_DEADLINE: Duration = Duration::from_secs(60);

/// How often it looks after that, in case it still does.
const REGISTERED_SLOW_POLL: Duration = Duration::from_secs(2);

/// A first-party grammar that can be installed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrammarOffer {
    /// The name fe-syntax will know it by.
    pub language: String,
    /// What to call it: "Kotlin".
    pub name: String,
    /// The extensions of its files, without the dot.
    pub extensions: Vec<String>,
    /// Its download's size in bytes.
    pub size: u64,
    /// Its download's SHA-256: empty until it's released, and never offered
    /// until then.
    pub sha256: String,
}

/// Why an install failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrammarInstallError {
    /// One line, to follow "Couldn't install Kotlin syntax: ".
    pub reason: String,
    /// Whether it looks like there's no connection, so that the language pack
    /// is worth a mention.
    pub offline: bool,
}

/// Whether a language is there already, registered, built in or on its way:
/// asked of a catalog entry's own language, even when fe-syntax knows that name.
pub type GrammarInstalled = Rc<dyn Fn(&str) -> bool>;

/// Downloads a catalog entry's grammar. Dropping the task stops it, with nothing
/// written.
pub type FetchGrammar =
    Rc<dyn Fn(&GrammarOffer, &mut App) -> Task<Result<Vec<u8>, GrammarInstallError>>>;

/// Writes a catalog entry's downloaded grammar where the extension loader will
/// find it. Once this starts, its task is always seen through.
pub type InstallGrammar =
    Rc<dyn Fn(&GrammarOffer, Vec<u8>, &mut App) -> Task<Result<(), GrammarInstallError>>>;

/// What the app can install, and how.
#[derive(Clone)]
pub struct GrammarOffers {
    pub catalog: Vec<GrammarOffer>,
    pub installed: GrammarInstalled,
    pub fetch: FetchGrammar,
    pub install: InstallGrammar,
    /// `<state dir>/`[`PROMPT_FILE`]. Without one, Never lasts for the session.
    pub never_file: Option<PathBuf>,
}

/// Has every workbench, from now on, offer from `offers`.
pub fn set_grammar_offers(cx: &mut App, offers: GrammarOffers) {
    cx.default_global::<Prompts>().offers = Some(Rc::new(offers));
}

/// The app's offers and what's been answered, across its windows.
#[derive(Default)]
struct Prompts {
    offers: Option<Rc<GrammarOffers>>,
    /// The languages being asked about or installed.
    busy: HashMap<String, Busy>,
    /// The languages answered Not now, until the next launch.
    silenced: HashSet<String>,
    /// The extensions answered Never in this session, whether or not the file
    /// could be written.
    never: HashSet<String>,
    /// Every workbench by its window, for the tabs to highlight after an
    /// install.
    workbenches: Vec<(WindowId, WeakEntity<Workbench>)>,
}

impl Global for Prompts {}

/// A prompt under way.
struct Busy {
    /// The window it stops with: none once it has started writing.
    window: Option<WindowId>,
    /// The language's other extensions that opened meanwhile, to ask about if
    /// the one asked about gets Never.
    pending: BTreeSet<String>,
    _task: Task<()>,
}

pub(crate) fn init(cx: &mut App) {
    cx.observe_new(|workbench: &mut Workbench, window, cx| {
        if let Some(window) = window {
            follow(workbench, window, cx);
        }
    })
    .detach();
    // A prompt's task, and with it a download, stops with its window. Not when
    // the workbench is released, which can be later than that.
    cx.on_window_closed(|cx, closed| {
        let prompts = cx.default_global::<Prompts>();
        (prompts.workbenches).retain(|(window, _)| *window != closed);
        prompts.busy.retain(|_, busy| busy.window != Some(closed));
    })
    .detach();
}

/// Ends `language`'s prompt. Last in its task, as this drops the task running it.
fn end_prompt(language: &str, cx: &mut AsyncApp) {
    cx.update(|cx| {
        cx.default_global::<Prompts>().busy.remove(language);
    });
}

/// Has a new workbench offer for what opens in it, what's open already first.
fn follow(workbench: &Workbench, window: &Window, cx: &mut Context<Workbench>) {
    let this = cx.weak_entity();
    let id = window.window_handle().window_id();
    (cx.default_global::<Prompts>().workbenches).push((id, this.clone()));
    let seen = Rc::new(Seen {
        window: id,
        paths: RefCell::default(),
    });
    // A tab opening, or a pane's path changing with a Save As or a rename.
    cx.observe(workbench.workspace(), {
        let seen = seen.clone();
        move |workbench, _, cx| sweep(workbench, &seen, cx)
    })
    .detach();
    cx.observe_self({
        let seen = seen.clone();
        move |workbench, cx| sweep(workbench, &seen, cx)
    })
    .detach();
    cx.defer(move |cx| {
        this.update(cx, |workbench, cx| sweep(workbench, &seen, cx))
            .ok();
    });
}

/// A workbench's window, and the path each of its editors last had: one that's
/// new here has just opened, or moved.
struct Seen {
    window: WindowId,
    paths: RefCell<HashMap<EntityId, PathBuf>>,
}

/// Offers for each editor whose path it hasn't seen.
fn sweep(workbench: &Workbench, seen: &Seen, cx: &mut Context<Workbench>) {
    let offers = cx.try_global::<Prompts>().and_then(|p| p.offers.clone());
    let Some(offers) = offers.filter(|offers| !offers.catalog.is_empty()) else {
        return;
    };
    let editors = workbench.file_editors(cx);
    let mut paths = seen.paths.borrow_mut();
    let open: HashSet<EntityId> = editors.iter().map(Entity::entity_id).collect();
    paths.retain(|id, _| open.contains(id));
    for editor in editors {
        let view = editor.read(cx);
        let Some(path) = view.model().path().map(Path::to_owned) else {
            continue;
        };
        if paths.get(&editor.entity_id()) == Some(&path) {
            continue;
        }
        let chosen = view.is_language_chosen();
        paths.insert(editor.entity_id(), path.clone());
        if chosen {
            continue;
        }
        if let Some((offer, extension)) = offer_for(&path, &offers) {
            let offer = offer.clone();
            ask(
                offer,
                extension,
                offers.clone(),
                workbench.toasts.clone(),
                seen.window,
                cx,
            );
        }
    }
}

/// The catalog entry to offer for the file at `path`, and its extension in
/// lower case, going by what the app knows of the language. What's been
/// answered is [`ask`]'s to check.
///
/// The entry is the one with the file's longest extension, as fe-syntax picks
/// a language: `blade.php`'s over `php`'s for `x.blade.php`
/// ([`fe_syntax::Language::file_has_extension`]).
///
/// `installed` alone speaks for the entry's own language. fe-syntax says only
/// whether another language would keep the file once it's installed, as an
/// extension's might: not a plain `php` when the entry's is `blade.php`
/// ([`fe_syntax::Language::extension_would_take`]).
fn offer_for<'a>(path: &Path, offers: &'a GrammarOffers) -> Option<(&'a GrammarOffer, String)> {
    let file_name = path.file_name()?.to_str()?;
    let (offer, extension) = (offers.catalog.iter())
        .filter(|offer| !offer.sha256.is_empty())
        .flat_map(|offer| (offer.extensions.iter()).map(move |extension| (offer, extension)))
        .filter(|(_, extension)| fe_syntax::Language::file_has_extension(file_name, extension))
        // The first of the longest.
        .min_by_key(|(_, extension)| std::cmp::Reverse(extension.len()))?;
    let extension = extension.to_ascii_lowercase();
    if (offers.installed)(&offer.language) {
        return None;
    }
    let claimed = fe_syntax::language_name_for_path(path);
    if claimed.is_some_and(|name| name != offer.language)
        && !fe_syntax::Language::extension_would_take(path, &extension)
    {
        return None;
    }
    Some((offer, extension))
}

/// Starts the prompt for `offer`, as the file with `extension` opened, unless
/// it's answered already. One under way keeps the extension for later.
fn ask(
    offer: GrammarOffer,
    extension: String,
    offers: Rc<GrammarOffers>,
    layer: Entity<ToastLayer>,
    window: WindowId,
    cx: &mut Context<Workbench>,
) {
    let prompts = cx.default_global::<Prompts>();
    if prompts.silenced.contains(&offer.language) || prompts.never.contains(&extension) {
        return;
    }
    if let Some(busy) = prompts.busy.get_mut(&offer.language) {
        busy.pending.insert(extension);
        return;
    }
    let language = offer.language.clone();
    let task = cx.spawn(async move |_, cx: &mut AsyncApp| {
        prompt(&offer, extension, &offers, &layer.downgrade(), window, cx).await;
        end_prompt(&offer.language, cx);
    });
    cx.default_global::<Prompts>().busy.insert(
        language,
        Busy {
            window: Some(window),
            pending: BTreeSet::new(),
            _task: task,
        },
    );
}

/// Asks about `offer` for `extension`, and does what's answered. Never for it
/// leaves the language's other extensions to ask about, if any have opened.
async fn prompt(
    offer: &GrammarOffer,
    mut extension: String,
    offers: &Rc<GrammarOffers>,
    layer: &WeakEntity<ToastLayer>,
    window: WindowId,
    cx: &mut AsyncApp,
) {
    loop {
        if !remembered_never(&extension, offers, cx).await {
            let Some(choice) = ask_toast(offer, &extension, offers, layer, cx).await else {
                return;
            };
            match choice {
                Some(INSTALL) => return install(offer, offers, layer, window, cx).await,
                // The session's copy, for if the file can't be written.
                Some(NEVER) => cx.update(|cx| {
                    (cx.default_global::<Prompts>().never).insert(extension.clone());
                }),
                // Not now, or closing the toast or losing it any other way.
                _ => {
                    return cx.update(|cx| {
                        (cx.default_global::<Prompts>().silenced).insert(offer.language.clone());
                    });
                }
            }
        }
        match cx.update(|cx| next_pending(&offer.language, cx)) {
            Some(next) => extension = next,
            None => return,
        }
    }
}

/// The next extension waiting on `language`'s prompt, not answered Never here.
fn next_pending(language: &str, cx: &mut App) -> Option<String> {
    let prompts = cx.default_global::<Prompts>();
    let never = prompts.never.clone();
    let busy = prompts.busy.get_mut(language)?;
    while let Some(next) = busy.pending.pop_first() {
        if !never.contains(&next) {
            return Some(next);
        }
    }
    None
}

/// Whether the file says never to offer for `extension`.
async fn remembered_never(extension: &str, offers: &GrammarOffers, cx: &mut AsyncApp) -> bool {
    let Some(file) = offers.never_file.clone() else {
        return false;
    };
    let never = cx.background_spawn(async move { read_never(&file) }).await;
    never.contains(extension)
}

/// Shows the offer and waits for the answer, which is none if the toast went
/// without one. `None` if there's no layer left to show it in.
///
/// Never is written from the answer itself, on no window's task, so that closing
/// the window straight after can't lose it.
async fn ask_toast(
    offer: &GrammarOffer,
    extension: &str,
    offers: &GrammarOffers,
    layer: &WeakEntity<ToastLayer>,
    cx: &mut AsyncApp,
) -> Option<Option<usize>> {
    let (answer, answered) = oneshot::channel();
    let remember = (offers.never_file.clone()).map(|file| {
        let executor = cx.background_executor().clone();
        (file, extension.to_owned(), executor)
    });
    let toast = Toast::info(format!(
        "Syntax highlighting for {} isn't installed",
        offer.name
    ))
    .choices(
        vec![
            format!("Install ({})", size_label(offer.size)),
            "Not now".to_owned(),
            format!("Never for .{extension}"),
        ],
        move |choice| {
            if choice == Some(NEVER)
                && let Some((file, extension, executor)) = remember
            {
                executor
                    .spawn(async move {
                        if let Err(error) = write_never(&file, &extension) {
                            log::warn!("couldn't write {}: {error}", file.display());
                        }
                    })
                    .detach();
            }
            answer.send(choice).ok();
        },
    );
    layer.update(cx, |layer, cx| layer.push(toast, cx)).ok()?;
    Some(answered.await.ok().flatten())
}

/// Downloads `offer`, writes it, then follows it until it registers. The window
/// closing stops the download. From the moment that's done it no longer can, so
/// that whatever gets written is seen through to its tabs.
async fn install(
    offer: &GrammarOffer,
    offers: &Rc<GrammarOffers>,
    layer: &WeakEntity<ToastLayer>,
    window: WindowId,
    cx: &mut AsyncApp,
) {
    let name = &offer.name;
    let toast = Toast::info(format!("Installing {name} syntax…"));
    let going = layer.update(cx, |layer, cx| layer.push(toast, cx)).ok();
    let fetching = cx.update(|cx| (offers.fetch)(offer, cx));
    let fetched = fetching.await;
    let owner = Owner {
        window,
        layer: layer.clone(),
        going,
    };
    let written = match fetched {
        Ok(wasm) => {
            let writing = cx.update(|cx| {
                let busy = cx.default_global::<Prompts>().busy.get_mut(&offer.language);
                if let Some(busy) = busy {
                    busy.window = None;
                }
                (offers.install)(offer, wasm, cx)
            });
            writing.await
        }
        Err(error) => Err(error),
    };
    if let Err(error) = written {
        owner.dismiss_going(cx);
        owner.tell(Toast::error(failure_line(name, &error)), cx);
        return;
    }
    report_registered(offer, owner, cx).await;
}

/// The window an install was asked for in, and its toast saying so.
struct Owner {
    window: WindowId,
    layer: WeakEntity<ToastLayer>,
    going: Option<ToastId>,
}

impl Owner {
    /// Stops saying the install's going, if the window's still there to.
    fn dismiss_going(&self, cx: &mut AsyncApp) {
        if let Some(going) = self.going {
            let dismissed = self.layer.update(cx, |layer, cx| layer.dismiss(going, cx));
            dismissed.ok();
        }
    }

    /// Shows `toast` in the window, or if it has closed, wherever there's one.
    fn tell(&self, toast: Toast, cx: &mut AsyncApp) {
        let open =
            cx.update(|cx| (cx.windows().iter()).any(|window| window.window_id() == self.window));
        if open {
            let pushed = self.layer.update(cx, {
                let toast = toast.clone();
                move |layer, cx| {
                    layer.push(toast, cx);
                }
            });
            if pushed.is_ok() {
                return;
            }
        }
        cx.update(|cx| toast::show(cx, toast));
    }
}

/// Waits for fe-syntax to know `offer`'s language, then highlights its tabs and
/// says so. Past [`REGISTERED_DEADLINE`] it warns, as when the loader refused it
/// or it's disabled, and goes on looking while a workbench is open.
async fn report_registered(offer: &GrammarOffer, owner: Owner, cx: &mut AsyncApp) {
    let name = &offer.name;
    let tell = |toast: Toast, cx: &mut AsyncApp| {
        owner.dismiss_going(cx);
        owner.tell(toast, cx);
    };
    let executor = cx.background_executor().clone();
    let deadline = executor.now() + REGISTERED_DEADLINE;
    let mut warned = false;
    while !fe_syntax::language_names().any(|known| known == offer.language) {
        if !warned && executor.now() >= deadline {
            warned = true;
            log::warn!("{} is installed but hasn't registered", offer.language);
            let toast = Toast::warning(format!(
                "Installed {name} syntax, but it hasn't loaded: it may after a restart"
            ));
            tell(toast, cx);
        }
        let any_open = cx.update(|cx| !cx.default_global::<Prompts>().workbenches.is_empty());
        if !any_open {
            return;
        }
        let poll = if warned {
            REGISTERED_SLOW_POLL
        } else {
            REGISTERED_POLL
        };
        executor.timer(poll).await;
    }
    cx.update(|cx| rehighlight(&offer.language, cx));
    tell(Toast::info(format!("Installed {name} syntax")), cx);
}

/// Has every tab of `language`, in every window, highlight as it. One whose
/// language was chosen by hand stays as it is.
fn rehighlight(language: &str, cx: &mut App) {
    let workbenches: Vec<_> = (cx.default_global::<Prompts>().workbenches.iter())
        .filter_map(|(_, workbench)| workbench.upgrade())
        .collect();
    for workbench in workbenches {
        let workbench = workbench.read(cx);
        let mut editors = workbench.open_editors(cx);
        let mut diffs = Vec::new();
        for pane in workbench.panes(cx) {
            for tab in pane.read(cx).tabs() {
                if let Some(diff) = tab.item().downcast_ref::<DiffView>() {
                    let diff = diff.clone();
                    let view = diff.read(cx);
                    editors.extend([view.left().clone(), view.right().clone()]);
                    diffs.push(diff);
                }
            }
        }
        for editor in editors {
            rehighlight_editor(&editor, language, cx);
        }
        for diff in diffs {
            diff.update(cx, |diff, cx| diff.detect_languages_again(cx));
        }
    }
}

/// Detects `editor`'s language if it has a path saying it's `language`.
fn rehighlight_editor(editor: &Entity<EditorView>, language: &str, cx: &mut App) {
    let view = editor.read(cx);
    let already = view.syntax().map(|syntax| syntax.language().name()) == Some(language);
    let is_language = |path: &Path| fe_syntax::language_name_for_path(path) == Some(language);
    if already || view.is_language_chosen() || !view.language_path().is_some_and(is_language) {
        return;
    }
    editor.update(cx, |view, cx| view.detect_language(cx));
}

/// What a failed install says: on one line, whatever the reason has in it.
fn failure_line(name: &str, error: &GrammarInstallError) -> String {
    let reason = error
        .reason
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let hint = match error.offline {
        true => " The language pack installs it without a connection.",
        false => "",
    };
    format!("Couldn't install {name} syntax: {reason}.{hint}")
}

/// A download's size as the button shows it.
fn size_label(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_000_000.)
}

/// [`PROMPT_FILE`]'s contents.
#[derive(Default, Deserialize, Serialize)]
struct NeverFile {
    /// The extensions never to offer for, in lower case and without the dot.
    #[serde(default)]
    never: BTreeSet<String>,
}

/// The extensions `path` says never to offer for: none if it's missing or
/// doesn't read, which only means being asked again.
fn read_never(path: &Path) -> BTreeSet<String> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return BTreeSet::new(),
        Err(error) => {
            log::warn!("couldn't read {}: {error}", path.display());
            return BTreeSet::new();
        }
    };
    match serde_json::from_str::<NeverFile>(&text) {
        Ok(file) => file.never,
        Err(error) => {
            log::warn!("couldn't read {}: {error}", path.display());
            BTreeSet::new()
        }
    }
}

/// Adds `extension` to `path`, keeping what another window or process wrote:
/// read again and written under a lock, through a temporary file renamed over
/// it. Blocking.
fn write_never(path: &Path, extension: &str) -> io::Result<()> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(dir)?;
    let mut lock_name = path.file_name().unwrap_or_default().to_owned();
    lock_name.push(".lock");
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path.with_file_name(lock_name))?;
    // Released when it drops, after the rename.
    lock.lock()?;
    let mut never = read_never(path);
    never.insert(extension.to_owned());
    let text = serde_json::to_string_pretty(&NeverFile { never }).map_err(io::Error::other)?;
    let temporary = path.with_extension(format!(
        "{}.{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let written = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        fs::rename(&temporary, path)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    written
}
