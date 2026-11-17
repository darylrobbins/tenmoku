//! Offers to install syntax highlighting when a file opens in a language that
//! can be installed and isn't: a toast with Install <Language> (size), Install
//! <Pack> pack (n languages, size) when the language's main pack has others
//! missing, Not now and Don't ask for <Language>. The decision records are
//! `docs/decisions/pp118n3cn5-grammar-prompt.md`,
//! `docs/decisions/qjkd44dekm-go-module-grammars.md` (file names) and
//! `docs/decisions/mmjrn0hj4f-grammar-prompt-packs.md` (packs, shebangs and
//! Don't ask).
//!
//! The app supplies the [`GrammarOffers`] ([`set_grammar_offers`]): what can be
//! installed, how a file's language is found, and how to install it, so this
//! crate has no network code, no index and no installer. Without them nothing
//! is ever offered.
//!
//! - **Consent.** Only the Install buttons call [`GrammarOffers::fetch`] and
//!   then [`GrammarOffers::install`]. Closing the toast, or losing it any other
//!   way, counts as Not now.
//! - **Not now** silences a language in every window until the next launch.
//!   **Don't ask** is per language, in [`PROMPT_FILE`] in the state directory,
//!   which is read again before each offer so that another window or process's
//!   answer counts. An extension or file name the file lists from before, when
//!   Never was per extension, silences the language claiming it.
//! - **One at a time.** A language being asked about or installed isn't offered
//!   again, in any window. Each prompt belongs to the window that showed it and
//!   stops with it, a download in progress too, until that download is done.
//! - **Nothing waits for this.** Tabs open as usual; the file is read, and the
//!   toast shown, off the back of that.
//!
//! An installed extension is registered by the extension loader on a thread of
//! its own, with no signal, so an install then watches
//! [`fe_syntax::language_names`] for the language, and detects the language of
//! every tab of it, in every window, once it's there. Writing it and all of that
//! belong to no window, as they can't be called back, and the watching goes on past
//! the warning that it's taking too long. A pack installs its languages one after
//! another, each written and watched like that as it lands.

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

use fe_buffer::Bias;
use fe_gpui::{DiffView, EditorView};
use futures::channel::oneshot;
use gpui::{
    App, AppContext as _, AsyncApp, Context, Entity, EntityId, Global, Task, WeakEntity, Window,
    WindowId,
};
use kiln_l10n::tr;
use serde::{Deserialize, Serialize};

use crate::{
    Workbench,
    workbench::toast::{self, Toast, ToastId, ToastLayer, notification_inbox::State},
};

/// The file in the state directory that holds the languages never to offer,
/// and the extensions and file names it held before Don't ask was per
/// language.
pub const PROMPT_FILE: &str = "grammar_prompt.json";

/// How often an install looks for the language to have registered.
const REGISTERED_POLL: Duration = Duration::from_millis(250);

/// How long before an install warns that it hasn't: the loader's watcher looks
/// every few seconds at a directory that didn't exist before, and compiling a
/// module of a few MB can take seconds more, longer on a busy machine.
const REGISTERED_DEADLINE: Duration = Duration::from_secs(60);

/// How often it looks after that, in case it still does.
const REGISTERED_SLOW_POLL: Duration = Duration::from_secs(2);

/// The most of a file's first line looked at for a shebang.
const FIRST_LINE_BYTES: usize = 1024;

/// A grammar that can be installed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrammarOffer {
    /// The name fe-syntax will know it by.
    pub language: String,
    /// What to call it: "Kotlin".
    pub name: String,
    /// The extensions of its files, without the dot.
    pub extensions: Vec<String>,
    /// The whole names of its files, such as `go.mod`, as its manifest gives
    /// them: matched ignoring ASCII case, and before any extension.
    pub file_names: Vec<String>,
    /// The interpreters whose `#!` lines are its files', such as `fish`.
    pub shebangs: Vec<String>,
    /// The keys of the [`GrammarPack`]s it's in: the first is its main pack,
    /// the one offered with it.
    pub packs: Vec<String>,
    /// Its download's size in bytes.
    pub size: u64,
    /// Its download's SHA-256, or its files' digest: empty until it's
    /// released, and never offered or installed until then.
    pub sha256: String,
}

/// Languages offered together, as a language's main pack.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrammarPack {
    /// Its key, as [`GrammarOffer::packs`] names it.
    pub id: String,
    /// What to call it: "JVM".
    pub name: String,
    /// Its languages, as catalog entries' [`GrammarOffer::language`].
    pub languages: Vec<String>,
}

/// Why an install failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrammarInstallError {
    /// One line, to follow "Couldn't install Kotlin syntax: ".
    pub reason: String,
    /// Whether it looks like there's no connection, so that the offline
    /// language packs are worth a mention.
    pub offline: bool,
}

/// Whether a language is there already, registered, built in or on its way:
/// asked of a catalog entry's own language, even when fe-syntax knows that name.
pub type GrammarInstalled = Rc<dyn Fn(&str) -> bool>;

/// The catalog language to offer for the file at a path, whose first line is
/// given if it has one: none if no language that isn't installed claims it,
/// or one that's registered does.
pub type DetectGrammar = Rc<dyn Fn(&Path, Option<&str>) -> Option<String>>;

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
    /// The packs the catalog's languages are in.
    pub packs: Vec<GrammarPack>,
    /// Which language a file is: with none, the catalog's own extensions, file
    /// names and shebangs decide ([`offer_for`]).
    pub detect: Option<DetectGrammar>,
    pub installed: GrammarInstalled,
    pub fetch: FetchGrammar,
    pub install: InstallGrammar,
    /// `<state dir>/`[`PROMPT_FILE`]. Without one, Don't ask lasts for the
    /// session.
    pub never_file: Option<PathBuf>,
}

/// Has every workbench, from now on, offer from `offers`, and look again at
/// the files it has open: a language these offers have that the last didn't
/// is offered for them now.
pub fn set_grammar_offers(cx: &mut App, offers: GrammarOffers) {
    let prompts = cx.default_global::<Prompts>();
    prompts.offers = Some(Rc::new(offers));
    prompts.generation += 1;
    let workbenches: Vec<_> = (prompts.workbenches.iter())
        .filter_map(|(_, workbench)| workbench.upgrade())
        .collect();
    for workbench in workbenches {
        workbench.update(cx, |_, cx| cx.notify());
    }
}

/// The app's offers and what's been answered, across its windows.
#[derive(Default)]
struct Prompts {
    offers: Option<Rc<GrammarOffers>>,
    /// The languages being asked about or installed, each with what has it.
    busy: HashMap<String, Busy>,
    /// Each prompt's task, which stops with its window.
    tasks: Vec<(u64, WindowId, Task<()>)>,
    /// The next prompt's or write's [`Busy::owner`].
    next_owner: u64,
    /// How many offers there have been: a workbench looks again at its files
    /// when it changes.
    generation: u64,
    /// The languages answered Not now, until the next launch.
    silenced: HashSet<String>,
    /// The languages answered Don't ask in this session, whether or not the
    /// file could be written.
    never: HashSet<String>,
    /// Every workbench by its window, for the tabs to highlight after an
    /// install.
    workbenches: Vec<(WindowId, WeakEntity<Workbench>)>,
}

impl Global for Prompts {}

/// What has a language under way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Busy {
    /// The prompt or the write that has it: only it takes the language off.
    owner: u64,
    /// The window it stops with: none once it has started writing.
    window: Option<WindowId>,
}

/// A prompt, and the window it belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Claim {
    owner: u64,
    window: WindowId,
}

impl Claim {
    fn busy(self) -> Busy {
        Busy {
            owner: self.owner,
            window: Some(self.window),
        }
    }
}

impl Prompts {
    /// A new [`Busy::owner`].
    fn new_owner(&mut self) -> u64 {
        self.next_owner += 1;
        self.next_owner
    }
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
        prompts.tasks.retain(|(_, window, _)| *window != closed);
    })
    .detach();
}

impl Workbench {
    /// The notifications this window has that are still open, oldest first:
    /// each its message and, for an offer, its buttons' titles. For tests
    /// outside the crate.
    #[doc(hidden)]
    pub fn grammar_prompt_toasts(&self, cx: &App) -> Vec<(String, Vec<String>)> {
        (self.toasts.read(cx).inbox().all().iter())
            .filter(|notification| notification.state == State::Open)
            .map(|notification| (notification.title.clone(), notification.answers.clone()))
            .collect()
    }
}

/// Ends the prompt `claim`, and the languages it marked as under way that it
/// still has, not handed to a write nor given up to another prompt. Last in
/// its task, as this drops the task running it.
fn end_prompt(claim: Claim, languages: &[String], cx: &mut AsyncApp) {
    cx.update(|cx| {
        let prompts = cx.default_global::<Prompts>();
        for language in languages {
            release(prompts, language, claim);
        }
        prompts.tasks.retain(|(task, ..)| *task != claim.owner);
    });
}

/// Takes `language` off the languages under way if the prompt `claim` still
/// has it.
fn release(prompts: &mut Prompts, language: &str, claim: Claim) {
    if prompts.busy.get(language) == Some(&claim.busy()) {
        prompts.busy.remove(language);
    }
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
    /// Each editor's path, and the [`Prompts::generation`] of the offers it
    /// was looked at with.
    paths: RefCell<HashMap<EntityId, (PathBuf, u64)>>,
}

/// Offers for each editor whose path it hasn't seen.
fn sweep(workbench: &Workbench, seen: &Seen, cx: &mut Context<Workbench>) {
    let Some(prompts) = cx.try_global::<Prompts>() else {
        return;
    };
    let generation = prompts.generation;
    let Some(offers) = (prompts.offers.clone()).filter(|offers| !offers.catalog.is_empty()) else {
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
        let looked = (path.clone(), generation);
        if paths.get(&editor.entity_id()) == Some(&looked) {
            continue;
        }
        let chosen = view.is_language_chosen();
        paths.insert(editor.entity_id(), looked);
        if chosen {
            continue;
        }
        let first_line = first_line(view);
        if let Some(offer) = offer_for(&path, first_line.as_deref(), &offers) {
            let offer = offer.clone();
            ask(
                offer,
                offers.clone(),
                workbench.toasts.clone(),
                seen.window,
                cx,
            );
        }
    }
}

/// The first line of `view`'s text, if it has any: what a shebang is in.
fn first_line(view: &EditorView) -> Option<String> {
    let text = view.model().text();
    let end = match text.row_count() {
        0 => return None,
        1 => text.len(),
        _ => text.row_start(1),
    };
    let end = text.clip_offset(end.min(FIRST_LINE_BYTES), Bias::Left);
    let line = text.slice_to_string(0..end);
    let line = line.trim_end_matches(['\n', '\r']);
    (!line.is_empty()).then(|| line.to_owned())
}

/// The catalog entry to offer for the file at `path`, whose first line is
/// `first_line`. What the app knows of the language decides; what's been
/// answered is [`ask`]'s to check.
///
/// With [`GrammarOffers::detect`], it says which language. Without it, the
/// catalog's own claims do, as fe-syntax would once the language is there: a
/// file whose whole name an entry claims, ignoring ASCII case, gets that
/// entry; otherwise the entry with the file's longest extension
/// ([`fe_syntax::Language::file_has_extension`]), `blade.php`'s over `php`'s
/// for `x.blade.php`; otherwise the entry claiming its `#!` interpreter, for
/// a file no language has by its name.
///
/// `installed` alone speaks for the entry's own language. fe-syntax says only
/// whether another language would keep the file once it's installed, as an
/// extension's might: not a plain `php` when the entry's is `blade.php`
/// ([`fe_syntax::Language::extension_would_take`]). A file claimed by name or
/// shebang is offered only if no language has it.
fn offer_for<'a>(
    path: &Path,
    first_line: Option<&str>,
    offers: &'a GrammarOffers,
) -> Option<&'a GrammarOffer> {
    let released = || (offers.catalog.iter()).filter(|offer| !offer.sha256.is_empty());
    let offer = match &offers.detect {
        Some(detect) => {
            let language = detect(path, first_line)?;
            released().find(|offer| offer.language == language)?
        }
        None => catalog_claim(path, first_line, released)?,
    };
    (!(offers.installed)(&offer.language)).then_some(offer)
}

/// The released entry whose claims say it's `path`'s language, as
/// [`offer_for`] has it without [`GrammarOffers::detect`].
fn catalog_claim<'a, I>(
    path: &Path,
    first_line: Option<&str>,
    released: impl Fn() -> I,
) -> Option<&'a GrammarOffer>
where
    I: Iterator<Item = &'a GrammarOffer>,
{
    let file_name = path.file_name()?.to_str()?;
    let claimed = fe_syntax::language_name_for_path(path);
    let by_name = released().find(|offer| claims_file_name(offer, file_name));
    if let Some(offer) = by_name {
        return claimed
            .is_none_or(|name| name == offer.language)
            .then_some(offer);
    }
    let by_extension = released()
        .flat_map(|offer| (offer.extensions.iter()).map(move |extension| (offer, extension)))
        .filter(|(_, extension)| fe_syntax::Language::file_has_extension(file_name, extension))
        // The first of the longest.
        .min_by_key(|(_, extension)| std::cmp::Reverse(extension.len()));
    if let Some((offer, extension)) = by_extension {
        let key = extension.to_ascii_lowercase();
        let kept = claimed.is_some_and(|name| name != offer.language)
            && !fe_syntax::Language::extension_would_take(path, &key);
        return (!kept).then_some(offer);
    }
    let first_line = first_line?;
    if claimed.is_some() || fe_syntax::language_name_for_shebang(first_line).is_some() {
        return None;
    }
    let program = fe_syntax::shebang_program(first_line)?;
    released().find(|offer| {
        (offer.shebangs.iter()).any(|interpreter| interpreter_runs(interpreter, &program))
    })
}

/// Whether `offer` claims files called `file_name`, ignoring ASCII case.
fn claims_file_name(offer: &GrammarOffer, file_name: &str) -> bool {
    (offer.file_names.iter()).any(|name| name.eq_ignore_ascii_case(file_name))
}

/// Whether a shebang's `program` is `interpreter`, or it with a version
/// suffix (`python3.12` for `python`).
fn interpreter_runs(interpreter: &str, program: &str) -> bool {
    program.strip_prefix(interpreter).is_some_and(|suffix| {
        suffix.is_empty()
            || (suffix.split('.'))
                .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
    })
}

/// Starts the prompt for `offer`, unless it's answered already or one's under
/// way.
fn ask(
    offer: GrammarOffer,
    offers: Rc<GrammarOffers>,
    layer: Entity<ToastLayer>,
    window: WindowId,
    cx: &mut Context<Workbench>,
) {
    let prompts = cx.default_global::<Prompts>();
    let language = offer.language.clone();
    if prompts.silenced.contains(&language)
        || prompts.never.contains(&language)
        || prompts.busy.contains_key(&language)
    {
        return;
    }
    let claim = Claim {
        owner: prompts.new_owner(),
        window,
    };
    let task = cx.spawn(async move |_, cx: &mut AsyncApp| {
        let marked = prompt(&offer, &offers, &layer.downgrade(), claim, cx).await;
        end_prompt(claim, &marked, cx);
    });
    let prompts = cx.default_global::<Prompts>();
    prompts.busy.insert(language, claim.busy());
    prompts.tasks.push((claim.owner, window, task));
}

/// What an offer's button does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Answer {
    Install,
    InstallPack,
    NotNow,
    DontAsk,
}

/// The pack offered with a language: its main pack, and the languages of it
/// to install, the language first.
struct PackOffer {
    pack: GrammarPack,
    members: Vec<GrammarOffer>,
}

impl PackOffer {
    fn size(&self) -> u64 {
        self.members.iter().map(|member| member.size).sum()
    }
}

/// `offer`'s main pack, with the released languages of it that aren't
/// installed or under way: none unless that's another besides `offer`.
fn pack_offer(offer: &GrammarOffer, offers: &GrammarOffers, cx: &mut App) -> Option<PackOffer> {
    let main = offer.packs.first()?;
    let pack = offers.packs.iter().find(|pack| pack.id == *main)?;
    let busy = &cx.default_global::<Prompts>().busy;
    let others = (pack.languages.iter())
        .filter(|language| **language != offer.language)
        .filter_map(|language| (offers.catalog.iter()).find(|entry| entry.language == *language))
        .filter(|entry| {
            !entry.sha256.is_empty()
                && !busy.contains_key(&entry.language)
                && !(offers.installed)(&entry.language)
        });
    let members: Vec<GrammarOffer> = [offer].into_iter().chain(others).cloned().collect();
    (members.len() > 1).then(|| PackOffer {
        pack: pack.clone(),
        members,
    })
}

/// Asks about `offer`, and does what's answered: the languages it marked as
/// under way, for [`end_prompt`].
async fn prompt(
    offer: &GrammarOffer,
    offers: &Rc<GrammarOffers>,
    layer: &WeakEntity<ToastLayer>,
    claim: Claim,
    cx: &mut AsyncApp,
) -> Vec<String> {
    let mut marked = vec![offer.language.clone()];
    if remembered_never(offer, offers, cx).await {
        return marked;
    }
    let pack = cx.update(|cx| pack_offer(offer, offers, cx));
    let Some(answer) = ask_toast(offer, pack.as_ref(), offers, layer, cx).await else {
        return marked;
    };
    match (answer, pack) {
        (Some(Answer::Install), _) => install(offer, offers, layer, claim.window, cx).await,
        (Some(Answer::InstallPack), Some(pack)) => {
            // Its other languages, but any another prompt took up meanwhile.
            cx.update(|cx| {
                let busy = &mut cx.default_global::<Prompts>().busy;
                for member in &pack.members[1..] {
                    if !busy.contains_key(&member.language) {
                        busy.insert(member.language.clone(), claim.busy());
                        marked.push(member.language.clone());
                    }
                }
            });
            let members: Vec<GrammarOffer> = (pack.members.into_iter())
                .filter(|member| marked.contains(&member.language))
                .collect();
            install_pack(&pack.pack, &members, offers, layer, claim, cx).await;
        }
        // The session's copy, for if the file can't be written.
        (Some(Answer::DontAsk), _) => cx.update(|cx| {
            (cx.default_global::<Prompts>().never).insert(offer.language.clone());
        }),
        // Not now, or closing the toast or losing it any other way.
        _ => cx.update(|cx| {
            (cx.default_global::<Prompts>().silenced).insert(offer.language.clone());
        }),
    }
    marked
}

/// Whether the file says never to offer `offer`: its language is listed, or
/// one of its extensions or file names from before Don't ask was per language.
async fn remembered_never(offer: &GrammarOffer, offers: &GrammarOffers, cx: &mut AsyncApp) -> bool {
    let Some(file) = offers.never_file.clone() else {
        return false;
    };
    let never = cx.background_spawn(async move { read_never(&file) }).await;
    never.silences(offer)
}

/// Shows the offer and waits for the answer, which is none if the toast went
/// without one. `None` if there's no layer left to show it in.
///
/// Don't ask is written from the answer itself, on no window's task, so that
/// closing the window straight after can't lose it.
async fn ask_toast(
    offer: &GrammarOffer,
    pack: Option<&PackOffer>,
    offers: &GrammarOffers,
    layer: &WeakEntity<ToastLayer>,
    cx: &mut AsyncApp,
) -> Option<Option<Answer>> {
    let (answer, answered) = oneshot::channel();
    let remember = (offers.never_file.clone()).map(|file| {
        let executor = cx.background_executor().clone();
        (file, offer.language.clone(), executor)
    });
    let (titles, answers) = choices(offer, pack);
    let toast = Toast::info(tr!(
        "workbench-extensions-grammar-offer",
        name = offer.name.as_str()
    ))
    .choices(titles, move |choice| {
        let choice = choice.and_then(|choice| answers.get(choice).copied());
        if choice == Some(Answer::DontAsk)
            && let Some((file, language, executor)) = remember
        {
            executor
                .spawn(async move {
                    if let Err(error) = write_never(&file, &language) {
                        log::warn!("couldn't write {}: {error}", file.display());
                    }
                })
                .detach();
        }
        answer.send(choice).ok();
    });
    layer.update(cx, |layer, cx| layer.push(toast, cx)).ok()?;
    Some(answered.await.ok().flatten())
}

/// The offer's buttons, and what each does.
fn choices(offer: &GrammarOffer, pack: Option<&PackOffer>) -> (Vec<String>, Vec<Answer>) {
    let mut choices = vec![(
        tr!(
            "workbench-extensions-grammar-install",
            name = offer.name.as_str(),
            size = size_label(offer.size)
        )
        .to_string(),
        Answer::Install,
    )];
    if let Some(pack) = pack {
        let title = tr!(
            "workbench-extensions-grammar-install-pack",
            pack = pack.pack.name.as_str(),
            count = pack.members.len(),
            size = size_label(pack.size())
        );
        choices.push((title.to_string(), Answer::InstallPack));
    }
    choices.push((
        tr!("workbench-extensions-grammar-not-now").to_string(),
        Answer::NotNow,
    ));
    choices.push((
        tr!(
            "workbench-extensions-grammar-dont-ask",
            name = offer.name.as_str()
        )
        .to_string(),
        Answer::DontAsk,
    ));
    choices.into_iter().unzip()
}

/// Downloads `offer`, then hands it to a write that follows it until it
/// registers. The window closing stops the download. From the moment that's
/// done it no longer can, so that whatever gets written is seen through to its
/// tabs.
async fn install(
    offer: &GrammarOffer,
    offers: &Rc<GrammarOffers>,
    layer: &WeakEntity<ToastLayer>,
    window: WindowId,
    cx: &mut AsyncApp,
) {
    let name = &offer.name;
    let toast = Toast::info(tr!("workbench-extensions-grammar-installing", name = name));
    let going = layer.update(cx, |layer, cx| layer.push(toast, cx)).ok();
    let fetching = cx.update(|cx| (offers.fetch)(offer, cx));
    let fetched = fetching.await;
    let owner = Owner {
        window,
        layer: layer.clone(),
        going,
    };
    match fetched {
        Ok(bytes) => {
            // Its result is the write's to tell.
            let _written = write(offer, bytes, offers, owner, false, cx);
        }
        Err(error) => {
            owner.dismiss_going(cx);
            owner.tell(Toast::error(failure_line(offer, &error)), cx);
        }
    }
}

/// Installs `members` of `pack`, the languages the prompt has marked as under
/// way, one after another: each downloaded while the window can stop it, then
/// written, registered and highlighted as [`install`] does. One that fails is
/// named, and the rest go on.
async fn install_pack(
    pack: &GrammarPack,
    members: &[GrammarOffer],
    offers: &Rc<GrammarOffers>,
    layer: &WeakEntity<ToastLayer>,
    claim: Claim,
    cx: &mut AsyncApp,
) {
    let window = claim.window;
    let total = members.len();
    let owner = Owner {
        window,
        layer: layer.clone(),
        going: None,
    };
    let mut going: Option<ToastId> = None;
    let mut installed = 0;
    for (ix, member) in members.iter().enumerate() {
        // Another install may have brought it meanwhile, as one that injects it.
        if (offers.installed)(&member.language) {
            cx.update(|cx| release(cx.default_global::<Prompts>(), &member.language, claim));
            installed += 1;
            continue;
        }
        let toast = Toast::info(tr!(
            "workbench-extensions-grammar-installing-pack",
            pack = pack.name.as_str(),
            done = ix + 1,
            count = total
        ));
        let pushed = layer.update(cx, |layer, cx| {
            if let Some(going) = going {
                layer.dismiss(going, cx);
            }
            layer.push(toast, cx)
        });
        going = pushed.ok();
        let fetching = cx.update(|cx| (offers.fetch)(member, cx));
        match fetching.await {
            Ok(bytes) => {
                let member_owner = Owner {
                    window,
                    layer: layer.clone(),
                    going: None,
                };
                let written = write(member, bytes, offers, member_owner, true, cx);
                if written.await.unwrap_or(false) {
                    installed += 1;
                }
            }
            Err(error) => {
                cx.update(|cx| release(cx.default_global::<Prompts>(), &member.language, claim));
                owner.tell(Toast::error(failure_line(member, &error)), cx);
            }
        }
    }
    if let Some(going) = going {
        layer.update(cx, |layer, cx| layer.dismiss(going, cx)).ok();
    }
    let done = match installed == total {
        true => tr!(
            "workbench-extensions-grammar-pack-installed",
            pack = pack.name.as_str()
        ),
        false => tr!(
            "workbench-extensions-grammar-pack-partly",
            pack = pack.name.as_str(),
            done = installed,
            count = total
        ),
    };
    owner.tell(Toast::info(done), cx);
}

/// Starts writing `offer`'s downloaded `bytes`, belonging to no window from now
/// on, then follows it until it registers: [`report_registered`], `quiet`ly for
/// a pack's language. The language stays under way until then. Whether it was
/// written, once it is or isn't.
fn write(
    offer: &GrammarOffer,
    bytes: Vec<u8>,
    offers: &Rc<GrammarOffers>,
    owner: Owner,
    quiet: bool,
    cx: &mut AsyncApp,
) -> oneshot::Receiver<bool> {
    let (written, was_written) = oneshot::channel();
    let offer = offer.clone();
    cx.update(|cx| {
        // Given up and started in the one step, so no window can drop it.
        let prompts = cx.default_global::<Prompts>();
        let busy = Busy {
            owner: prompts.new_owner(),
            window: None,
        };
        prompts.busy.insert(offer.language.clone(), busy);
        let writing = (offers.install)(&offer, bytes, cx);
        cx.spawn(async move |cx| {
            match writing.await {
                Ok(()) => {
                    written.send(true).ok();
                    report_registered(&offer, owner, quiet, cx).await;
                }
                Err(error) => {
                    written.send(false).ok();
                    owner.dismiss_going(cx);
                    owner.tell(Toast::error(failure_line(&offer, &error)), cx);
                }
            }
            cx.update(|cx| {
                let prompts = cx.default_global::<Prompts>();
                if prompts.busy.get(&offer.language) == Some(&busy) {
                    prompts.busy.remove(&offer.language);
                }
            });
        })
        .detach();
    });
    was_written
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

/// Waits for fe-syntax to know `offer`'s language, then highlights its tabs and,
/// unless `quiet`, says so. Past [`REGISTERED_DEADLINE`] it warns, as when the
/// loader refused it or it's disabled, and goes on looking while a workbench is
/// open.
async fn report_registered(offer: &GrammarOffer, owner: Owner, quiet: bool, cx: &mut AsyncApp) {
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
            let toast = Toast::warning(tr!("workbench-extensions-grammar-not-loaded", name = name));
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
    if !quiet {
        let installed = tr!("workbench-extensions-grammar-installed", name = name);
        tell(Toast::info(installed), cx);
    }
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

/// Detects `editor`'s language if it has a path saying it's `language`, or a
/// path saying nothing and a first line whose `#!` says so.
fn rehighlight_editor(editor: &Entity<EditorView>, language: &str, cx: &mut App) {
    let view = editor.read(cx);
    let already = view.syntax().map(|syntax| syntax.language().name()) == Some(language);
    let is_language = |path: &Path| match fe_syntax::language_name_for_path(path) {
        Some(name) => name == language,
        None => (first_line(view).as_deref())
            .and_then(fe_syntax::language_name_for_shebang)
            .is_some_and(|name| name == language),
    };
    if already || view.is_language_chosen() || !view.language_path().is_some_and(is_language) {
        return;
    }
    editor.update(cx, |view, cx| view.detect_language(cx));
}

/// What a failed install of `offer` says: on one line, whatever the reason
/// has in it. Offline, it names the offline pack with the language, its main
/// pack's, or every language's.
fn failure_line(offer: &GrammarOffer, error: &GrammarInstallError) -> String {
    let name = offer.name.as_str();
    let reason = error
        .reason
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    match error.offline {
        true => tr!(
            "workbench-extensions-grammar-failed-offline",
            name = name,
            reason = reason,
            tarball = tarball_name(offer.packs.first().map(String::as_str))
        ),
        false => tr!(
            "workbench-extensions-grammar-failed",
            name = name,
            reason = reason
        ),
    }
    .into()
}

/// The offline language pack's tarball for `pack`, or with every language
/// (`docs/release/language-pack.md`).
fn tarball_name(pack: Option<&str>) -> String {
    match pack {
        Some(pack) => format!("Tenmoku-language-pack-{pack}.tar.gz"),
        None => "Tenmoku-language-pack.tar.gz".to_owned(),
    }
}

/// A download's size as the button shows it.
fn size_label(bytes: u64) -> String {
    let megabytes = format!("{:.1}", bytes as f64 / 1_000_000.);
    tr!("workbench-extensions-grammar-size", size = megabytes).into()
}

/// [`PROMPT_FILE`]'s contents.
#[derive(Default, Deserialize, Serialize)]
struct NeverFile {
    /// The extensions never to offer for, in lower case and without the dot,
    /// and the file names, in lower case: what Never was before it was per
    /// language. Each silences the language claiming it, and is kept.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    never: BTreeSet<String>,
    /// The languages never to offer.
    #[serde(default)]
    never_languages: BTreeSet<String>,
}

impl NeverFile {
    /// Whether it says never to offer `offer`.
    fn silences(&self, offer: &GrammarOffer) -> bool {
        self.never_languages.contains(&offer.language)
            || (offer.extensions.iter().chain(&offer.file_names))
                .any(|claim| self.never.contains(&claim.to_ascii_lowercase()))
    }
}

/// What `path` says never to offer: nothing if it's missing or doesn't read,
/// which only means being asked again.
fn read_never(path: &Path) -> NeverFile {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return NeverFile::default(),
        Err(error) => {
            log::warn!("couldn't read {}: {error}", path.display());
            return NeverFile::default();
        }
    };
    serde_json::from_str::<NeverFile>(&text).unwrap_or_else(|error| {
        log::warn!("couldn't read {}: {error}", path.display());
        NeverFile::default()
    })
}

/// Adds `language` to `path`, keeping what another window or process wrote:
/// read again and written under a lock, through a temporary file renamed over
/// it. Blocking.
fn write_never(path: &Path, language: &str) -> io::Result<()> {
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
    never.never_languages.insert(language.to_owned());
    let text = serde_json::to_string_pretty(&never).map_err(io::Error::other)?;
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
