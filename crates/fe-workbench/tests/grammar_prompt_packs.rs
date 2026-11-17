//! The grammar prompt's packs, shebangs and Don't ask, from outside the crate
//! with fake offers: what's offered, what Don't ask silences across launches,
//! and a pack installing each missing language in turn.

use std::{
    cell::RefCell,
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Mutex, MutexGuard, Once},
    time::Duration,
};

use fe_gpui::{EditorSettings, EditorTheme, EditorView};
use fe_syntax::{GrammarDeadlines, GrammarSpec};
use fe_workbench::{
    DetectGrammar, GrammarInstallError, GrammarOffer, GrammarOffers, GrammarPack, Launch,
    PROMPT_FILE, Workbench, set_grammar_offers,
};
use futures::channel::oneshot;
use gpui::{App, AppContext as _, Entity, Task, TestAppContext, VisualTestContext};

static SERIAL: Mutex<()> = Mutex::new(());

/// Runs the rest of a test with no other of this binary running, and with no
/// config directory for the workbench to read or watch.
fn serially() -> MutexGuard<'static, ()> {
    let guard = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for var in [
        "TENMOKU_CONFIG_DIR",
        "XDG_CONFIG_HOME",
        "HOME",
        "APPDATA",
        "USERPROFILE",
    ] {
        // SAFETY: every test takes the lock before it starts gpui, so no
        // other thread is running to read the environment.
        unsafe { std::env::remove_var(var) };
    }
    guard
}

/// A catalog entry: `language`'s files have `extension`, its download is
/// `megabytes` MB, and it's in `packs`.
fn offer(
    language: &str,
    name: &str,
    extension: &str,
    packs: &[&str],
    megabytes: u64,
) -> GrammarOffer {
    GrammarOffer {
        language: language.into(),
        name: name.into(),
        extensions: vec![extension.into()],
        file_names: Vec::new(),
        shebangs: Vec::new(),
        packs: packs.iter().map(|pack| (*pack).to_owned()).collect(),
        size: megabytes * 1_000_000,
        sha256: "ab".repeat(32),
    }
}

fn pack(id: &str, name: &str, languages: &[&str]) -> GrammarPack {
    GrammarPack {
        id: id.into(),
        name: name.into(),
        languages: languages
            .iter()
            .map(|language| (*language).to_owned())
            .collect(),
    }
}

/// The JSON fixture grammar under `language`, for files with `extension`.
fn spec(language: &str, extension: &str) -> GrammarSpec {
    static DEADLINES: Once = Once::new();
    // A busy machine shouldn't fail this.
    DEADLINES.call_once(|| {
        fe_syntax::set_grammar_deadlines(GrammarDeadlines {
            load: Duration::from_secs(120),
            parse: Duration::from_secs(120),
        });
    });
    GrammarSpec {
        language: language.into(),
        wasm: Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../fe-syntax/tests/fixtures/tree-sitter-json.wasm"),
        extensions: vec![extension.into()],
        ..GrammarSpec::default()
    }
}

fn registered(language: &str) -> bool {
    fe_syntax::language_names().any(|name| name == language)
}

/// A fetch held until the test sends.
type Hold = Rc<RefCell<Option<oneshot::Receiver<()>>>>;

/// The fake installer: each fetch and install recorded in order. A language
/// in `fails` fails its fetch; one in `waits` fetches once the test sends;
/// one in `registers` registers its spec when it's written.
#[derive(Clone, Default)]
struct Fake {
    calls: Rc<RefCell<Vec<String>>>,
    fails: Vec<String>,
    waits: HashMap<String, Hold>,
    registers: HashMap<String, GrammarSpec>,
}

impl Fake {
    fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }
}

struct Setup {
    dir: tempfile::TempDir,
}

impl Setup {
    fn new() -> Setup {
        Setup {
            dir: tempfile::tempdir().unwrap(),
        }
    }

    fn never_file(&self) -> PathBuf {
        self.dir.path().join("state").join(PROMPT_FILE)
    }

    /// A file called `name` holding `text`.
    fn file(&self, name: &str, text: &str) -> PathBuf {
        let path = self.dir.path().join(name);
        fs::write(&path, text).unwrap();
        path
    }

    fn give(
        &self,
        catalog: Vec<GrammarOffer>,
        packs: Vec<GrammarPack>,
        detect: Option<DetectGrammar>,
        installed: impl Fn(&str) -> bool + 'static,
        fake: &Fake,
        cx: &mut TestAppContext,
    ) {
        let offers = self.offers(catalog, packs, detect, installed, fake);
        cx.update(|cx| {
            fe_gpui::init(cx);
            fe_workbench::init(cx);
            set_grammar_offers(cx, offers);
        });
    }

    fn offers(
        &self,
        catalog: Vec<GrammarOffer>,
        packs: Vec<GrammarPack>,
        detect: Option<DetectGrammar>,
        installed: impl Fn(&str) -> bool + 'static,
        fake: &Fake,
    ) -> GrammarOffers {
        let fetch = {
            let fake = fake.clone();
            Rc::new(move |offer: &GrammarOffer, cx: &mut App| {
                fake.calls
                    .borrow_mut()
                    .push(format!("fetch {}", offer.language));
                if let Some(hold) = fake.waits.get(&offer.language) {
                    let waiting = hold.borrow_mut().take().expect("one fetch");
                    return cx.background_spawn(async move {
                        waiting.await.ok();
                        Ok(Vec::new())
                    });
                }
                match fake.fails.contains(&offer.language) {
                    true => Task::ready(Err(GrammarInstallError {
                        reason: "the server answered 404".into(),
                        offline: false,
                    })),
                    false => Task::ready(Ok(Vec::new())),
                }
            })
        };
        let install = {
            let fake = fake.clone();
            Rc::new(move |offer: &GrammarOffer, _: Vec<u8>, cx: &mut App| {
                (fake.calls.borrow_mut()).push(format!("install {}", offer.language));
                match fake.registers.get(&offer.language).cloned() {
                    Some(spec) => cx.background_spawn(async move {
                        fe_syntax::register_language(&spec).expect("the fixture registers");
                        Ok(())
                    }),
                    None => Task::ready(Ok(())),
                }
            })
        };
        GrammarOffers {
            catalog,
            packs,
            detect,
            installed: Rc::new(installed),
            fetch,
            install,
            never_file: Some(self.never_file()),
        }
    }
}

/// Puts offers of `catalog` in place, with nothing installed, as the app does
/// when it has read the index.
fn set_offers(setup: &Setup, catalog: Vec<GrammarOffer>, fake: &Fake, cx: &mut App) {
    set_grammar_offers(cx, setup.offers(catalog, Vec::new(), None, |_| false, fake));
}

fn window(cx: &mut TestAppContext) -> (Entity<Workbench>, &mut VisualTestContext) {
    let launch = Launch {
        root: None,
        file: None,
    };
    cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    })
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// Far enough for an installed language to be seen to have registered.
fn settle(cx: &mut VisualTestContext) {
    for _ in 0..12 {
        cx.executor().advance_clock(Duration::from_millis(250));
        draw(cx);
    }
}

/// Opens `path` in a tab of its own, and the tab's editor.
fn open(
    workbench: &Entity<Workbench>,
    path: PathBuf,
    cx: &mut VisualTestContext,
) -> Entity<EditorView> {
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.open_document(path, window, cx)
        });
    });
    draw(cx);
    workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("the file is shown")
}

/// The toasts shown, each its message and, for an offer, its buttons.
fn toasts(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<(String, Vec<String>)> {
    workbench.read_with(cx, |workbench, cx| workbench.grammar_prompt_toasts(cx))
}

fn messages(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    toasts(workbench, cx)
        .into_iter()
        .map(|(message, _)| message)
        .collect()
}

/// The offers shown: each toast with buttons.
fn offers_shown(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<Vec<String>> {
    (toasts(workbench, cx).into_iter())
        .filter_map(|(_, titles)| (!titles.is_empty()).then_some(titles))
        .collect()
}

/// The one offer shown: its buttons.
fn the_offer(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    let offers = offers_shown(workbench, cx);
    assert_eq!(offers.len(), 1, "{:?}", toasts(workbench, cx));
    offers[0].clone()
}

/// Clicks the one offer's button titled `title`: the first is on the toast,
/// and the rest in its More menu.
fn click(workbench: &Entity<Workbench>, title: &str, cx: &mut VisualTestContext) {
    let titles = the_offer(workbench, cx);
    let ix = (titles.iter().position(|t| t == title)).unwrap_or_else(|| panic!("{titles:?}"));
    let press = |selector: String, cx: &mut VisualTestContext| {
        let selector: &'static str = Box::leak(selector.into_boxed_str());
        let button = cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(button.center(), gpui::Modifiers::none());
        draw(cx);
    };
    // The offer's place among the toasts shown: the one with choices.
    let toast = (0..8)
        .find(|toast| {
            let selector: &'static str =
                Box::leak(format!("toast-choice-{toast}-0").into_boxed_str());
            cx.debug_bounds(selector).is_some()
        })
        .expect("the offer is drawn");
    if ix == 0 {
        press(format!("toast-choice-{toast}-0"), cx);
    } else {
        press(format!("toast-more-{toast}"), cx);
        press(format!("toast-more-item-{toast}-{ix}"), cx);
    }
}

fn language_of(editor: &Entity<EditorView>, cx: &VisualTestContext) -> Option<&'static str> {
    editor.read_with(cx, |view, _| {
        view.syntax().map(|syntax| syntax.language().name())
    })
}

#[gpui::test]
fn offers_the_language_and_its_main_pack(cx: &mut TestAppContext) {
    let _serial = serially();
    let setup = Setup::new();
    let fake = Fake::default();
    setup.give(
        vec![
            offer("alpha", "Alpha", "alph", &["test", "other"], 1),
            offer("beta", "Beta", "bet", &["test"], 5),
            offer("gamma", "Gamma", "gamm", &["test"], 2),
            offer("delta", "Delta", "delt", &["other"], 7),
        ],
        vec![
            pack("test", "Test", &["alpha", "beta", "gamma"]),
            pack("other", "Other", &["alpha", "delta"]),
        ],
        None,
        |language| language == "beta",
        &fake,
        cx,
    );
    let (workbench, cx) = window(cx);

    open(&workbench, setup.file("a.alph", "x\n"), cx);

    // Its main pack, with what of it isn't installed: Beta is.
    assert_eq!(
        the_offer(&workbench, cx),
        [
            "Install Alpha (1.0 MB)",
            "Install Test pack (2 languages, 3.0 MB)",
            "Not now",
            "Don't ask for Alpha",
        ]
    );
    assert_eq!(
        messages(&workbench, cx),
        ["Syntax highlighting for Alpha isn't installed"]
    );
    assert!(
        fake.calls().is_empty(),
        "nothing's fetched without the click"
    );

    click(&workbench, "Install Test pack (2 languages, 3.0 MB)", cx);
    assert_eq!(
        fake.calls(),
        [
            "fetch alpha",
            "install alpha",
            "fetch gamma",
            "install gamma"
        ]
    );
}

#[gpui::test]
fn no_pack_button_when_nothing_else_in_the_pack_is_missing(cx: &mut TestAppContext) {
    let _serial = serially();
    let setup = Setup::new();
    let unreleased = GrammarOffer {
        sha256: String::new(),
        ..offer("gamma", "Gamma", "gamm", &["test"], 2)
    };
    setup.give(
        vec![
            offer("alpha", "Alpha", "alph", &["test"], 1),
            offer("beta", "Beta", "bet", &["test"], 5),
            unreleased,
            offer("lone", "Lone", "lone", &[], 1),
        ],
        // Epsilon isn't in the catalog.
        vec![pack("test", "Test", &["alpha", "beta", "gamma", "epsilon"])],
        None,
        |language| language == "beta",
        &Fake::default(),
        cx,
    );
    let (workbench, cx) = window(cx);

    open(&workbench, setup.file("a.alph", "x\n"), cx);
    assert_eq!(
        the_offer(&workbench, cx),
        ["Install Alpha (1.0 MB)", "Not now", "Don't ask for Alpha"]
    );
    click(&workbench, "Not now", cx);

    // In no pack at all.
    open(&workbench, setup.file("a.lone", "x\n"), cx);
    assert_eq!(
        the_offer(&workbench, cx),
        ["Install Lone (1.0 MB)", "Not now", "Don't ask for Lone"]
    );
}

#[gpui::test]
fn dont_ask_is_per_language_and_survives_a_new_process(
    cx: &mut TestAppContext,
    next_launch: &mut TestAppContext,
) {
    let _serial = serially();
    let setup = Setup::new();
    let catalog = || {
        let mut alpha = offer("alpha", "Alpha", "alph", &[], 1);
        alpha.extensions.push("alx".into());
        vec![alpha, offer("beta", "Beta", "bet", &[], 1)]
    };
    setup.give(catalog(), Vec::new(), None, |_| false, &Fake::default(), cx);
    let (first, cx) = window(cx);

    open(&first, setup.file("a.alph", "x\n"), cx);
    click(&first, "Don't ask for Alpha", cx);
    let written: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(setup.never_file()).unwrap()).unwrap();
    assert_eq!(written, serde_json::json!({ "never_languages": ["alpha"] }));

    // Alpha's other extension doesn't ask; another language does.
    open(&first, setup.file("b.alx", "x\n"), cx);
    assert_eq!(messages(&first, cx), Vec::<String>::new());
    open(&first, setup.file("a.bet", "x\n"), cx);
    assert_eq!(
        messages(&first, cx),
        ["Syntax highlighting for Beta isn't installed"]
    );

    // A new process: its own app and prompts, with only the file to go by.
    setup.give(
        catalog(),
        Vec::new(),
        None,
        |_| false,
        &Fake::default(),
        next_launch,
    );
    let (fresh, next_launch) = window(next_launch);
    open(&fresh, setup.file("c.alph", "x\n"), next_launch);
    assert_eq!(messages(&fresh, next_launch), Vec::<String>::new());
    open(&fresh, setup.file("d.alx", "x\n"), next_launch);
    assert_eq!(messages(&fresh, next_launch), Vec::<String>::new());
}

#[gpui::test]
fn old_per_extension_never_entries_still_silence_the_language(cx: &mut TestAppContext) {
    let _serial = serially();
    let setup = Setup::new();
    fs::create_dir_all(setup.never_file().parent().unwrap()).unwrap();
    // As Never for .alph, and for a file name, wrote it.
    fs::write(setup.never_file(), r#"{"never": ["alph", "gofile"]}"#).unwrap();
    let mut alpha = offer("alpha", "Alpha", "alph", &[], 1);
    alpha.extensions.push("alx".into());
    let named = GrammarOffer {
        extensions: Vec::new(),
        file_names: vec!["GoFile".into()],
        ..offer("named", "Named", "", &[], 1)
    };
    setup.give(
        vec![alpha, named, offer("beta", "Beta", "bet", &[], 1)],
        Vec::new(),
        None,
        |_| false,
        &Fake::default(),
        cx,
    );
    let (workbench, cx) = window(cx);

    // Each of the language's extensions, not only the one listed.
    for name in ["a.alph", "b.alx", "GoFile"] {
        open(&workbench, setup.file(name, "x\n"), cx);
        assert_eq!(messages(&workbench, cx), Vec::<String>::new(), "{name}");
    }

    // Don't ask for another keeps the old entries, beside the new list.
    open(&workbench, setup.file("a.bet", "x\n"), cx);
    click(&workbench, "Don't ask for Beta", cx);
    let written: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(setup.never_file()).unwrap()).unwrap();
    assert_eq!(
        written,
        serde_json::json!({ "never": ["alph", "gofile"], "never_languages": ["beta"] })
    );
}

/// A file's name and first line, as detection was asked about it.
type Asked = (String, Option<String>);

#[gpui::test]
fn a_file_name_or_shebang_match_offers_like_an_extension(cx: &mut TestAppContext) {
    let _serial = serially();
    let setup = Setup::new();
    let fishy = GrammarOffer {
        extensions: Vec::new(),
        shebangs: vec!["pkfish".into()],
        ..offer("pkfish", "Pkfish", "", &[], 2)
    };
    let named = GrammarOffer {
        extensions: Vec::new(),
        file_names: vec!["Pkfile".into()],
        ..offer("pkfile", "Pkfile", "", &[], 3)
    };
    let catalog = vec![
        fishy,
        named,
        offer("pkext", "Pkext", "pkext", &[], 1),
        offer("pkdet", "Pkdet", "pkdet", &[], 4),
    ];
    let fake = Fake::default();
    setup.give(catalog.clone(), Vec::new(), None, |_| false, &fake, cx);
    let (workbench, cx) = window(cx);
    let expect = |name: &str, size: &str| {
        vec![
            format!("Install {name} ({size} MB)"),
            "Not now".to_owned(),
            format!("Don't ask for {name}"),
        ]
    };

    // The catalog's own claims: an extension, a whole name, an interpreter
    // with a version.
    open(&workbench, setup.file("a.pkext", "x\n"), cx);
    assert_eq!(the_offer(&workbench, cx), expect("Pkext", "1.0"));
    click(&workbench, "Not now", cx);
    open(&workbench, setup.file("Pkfile", "x\n"), cx);
    assert_eq!(the_offer(&workbench, cx), expect("Pkfile", "3.0"));
    click(&workbench, "Not now", cx);
    open(
        &workbench,
        setup.file("tool", "#!/usr/bin/env pkfish2.1 -x\necho\n"),
        cx,
    );
    assert_eq!(the_offer(&workbench, cx), expect("Pkfish", "2.0"));
    click(&workbench, "Install Pkfish (2.0 MB)", cx);
    assert_eq!(fake.calls(), ["fetch pkfish", "install pkfish"]);

    // The app's detection decides when it's given, from the path and the
    // first line.
    let asked: Rc<RefCell<Vec<Asked>>> = Rc::default();
    let detect: DetectGrammar = {
        let asked = asked.clone();
        Rc::new(move |path: &Path, first_line: Option<&str>| {
            let name = path.file_name()?.to_str()?.to_owned();
            asked
                .borrow_mut()
                .push((name.clone(), first_line.map(str::to_owned)));
            (first_line == Some("#!/bin/detected")).then(|| "pkdet".to_owned())
        })
    };
    let fresh = Fake::default();
    setup.give(catalog, Vec::new(), Some(detect), |_| false, &fresh, cx);
    open(&workbench, setup.file("b.pkext", "x\n"), cx);
    assert!(
        offers_shown(&workbench, cx).is_empty(),
        "detection said none"
    );
    open(&workbench, setup.file("run", "#!/bin/detected\nmore\n"), cx);
    assert_eq!(the_offer(&workbench, cx), expect("Pkdet", "4.0"));
    let asked = asked.borrow().clone();
    assert!(
        asked.contains(&("run".to_owned(), Some("#!/bin/detected".to_owned()))),
        "{asked:?}"
    );
}

#[gpui::test]
fn pack_install_installs_each_missing_language_and_rehighlights_their_tabs(
    cx: &mut TestAppContext,
) {
    let _serial = serially();
    let setup = Setup::new();
    let fake = Fake {
        registers: HashMap::from([
            ("pk_one".to_owned(), spec("pk_one", "pkone")),
            ("pk_two".to_owned(), spec("pk_two", "pktwo")),
        ]),
        ..Fake::default()
    };
    setup.give(
        vec![
            offer("pk_one", "One", "pkone", &["duo"], 1),
            offer("pk_two", "Two", "pktwo", &["duo"], 1),
        ],
        vec![pack("duo", "Duo", &["pk_one", "pk_two"])],
        None,
        registered,
        &fake,
        cx,
    );
    let (workbench, cx) = window(cx);

    // Two's tab is open and plain, its own offer put off.
    let two = open(&workbench, setup.file("b.pktwo", "{\"a\": 1}\n"), cx);
    click(&workbench, "Not now", cx);
    let one = open(&workbench, setup.file("a.pkone", "{\"a\": 1}\n"), cx);
    assert_eq!((language_of(&one, cx), language_of(&two, cx)), (None, None));

    click(&workbench, "Install Duo pack (2 languages, 2.0 MB)", cx);

    assert_eq!(
        fake.calls(),
        [
            "fetch pk_one",
            "install pk_one",
            "fetch pk_two",
            "install pk_two"
        ]
    );
    let shown = messages(&workbench, cx);
    assert!(
        shown.contains(&"Installed Duo pack".to_owned()),
        "{shown:?}"
    );
    settle(cx);
    assert_eq!(language_of(&one, cx), Some("pk_one"));
    assert_eq!(language_of(&two, cx), Some("pk_two"));
}

#[gpui::test]
fn a_pack_member_failing_names_it_and_installs_the_rest(cx: &mut TestAppContext) {
    let _serial = serially();
    let setup = Setup::new();
    let fake = Fake {
        fails: vec!["pk_mid".to_owned()],
        registers: HashMap::from([
            ("pk_first".to_owned(), spec("pk_first", "pkfirst")),
            ("pk_last".to_owned(), spec("pk_last", "pklast")),
        ]),
        ..Fake::default()
    };
    setup.give(
        vec![
            offer("pk_first", "First", "pkfirst", &["trio"], 1),
            offer("pk_mid", "Middle", "pkmid", &["trio"], 1),
            offer("pk_last", "Last", "pklast", &["trio"], 1),
        ],
        vec![pack("trio", "Trio", &["pk_first", "pk_mid", "pk_last"])],
        None,
        registered,
        &fake,
        cx,
    );
    let (workbench, cx) = window(cx);

    let first = open(&workbench, setup.file("a.pkfirst", "{\"a\": 1}\n"), cx);
    click(&workbench, "Install Trio pack (3 languages, 3.0 MB)", cx);

    assert_eq!(
        fake.calls(),
        [
            "fetch pk_first",
            "install pk_first",
            "fetch pk_mid",
            "fetch pk_last",
            "install pk_last"
        ]
    );
    let shown = messages(&workbench, cx);
    assert!(
        shown.contains(&"Couldn't install Middle syntax: the server answered 404.".to_owned()),
        "{shown:?}"
    );
    assert!(
        shown.contains(&"Installed 2 of 3 languages of the Trio pack".to_owned()),
        "{shown:?}"
    );
    settle(cx);
    assert_eq!(language_of(&first, cx), Some("pk_first"));
    assert!(registered("pk_last"));
    assert!(!registered("pk_mid"));
}

/// Offers published after a file opened, as the app's are once the cached or
/// refreshed index has been read, offer for it: after none, and after offers
/// that didn't have its language.
#[gpui::test]
fn new_offers_offer_for_files_already_open(cx: &mut TestAppContext) {
    let _serial = serially();
    let setup = Setup::new();
    let fake = Fake::default();
    setup.give(Vec::new(), Vec::new(), None, |_| false, &fake, cx);
    let (workbench, cx) = window(cx);
    open(&workbench, setup.file("a.alph", "x\n"), cx);
    open(&workbench, setup.file("a.bet", "x\n"), cx);
    assert!(offers_shown(&workbench, cx).is_empty());

    // Offers without Alpha's language: Beta's file is offered for.
    let beta = offer("beta", "Beta", "bet", &[], 1);
    cx.update(|_, cx| {
        set_offers(&setup, vec![beta.clone()], &fake, cx);
    });
    cx.run_until_parked();
    assert_eq!(the_offer(&workbench, cx)[0], "Install Beta (1.0 MB)");
    click(&workbench, "Not now", cx);

    // Then with it: Alpha's is too, and Beta, put off, isn't asked again.
    cx.update(|_, cx| {
        set_offers(
            &setup,
            vec![beta, offer("alpha", "Alpha", "alph", &[], 1)],
            &fake,
            cx,
        );
    });
    cx.run_until_parked();
    assert_eq!(the_offer(&workbench, cx)[0], "Install Alpha (1.0 MB)");
}

/// A pack member that failed, opened again while the pack goes on, is its own
/// prompt's: the pack ending doesn't take it from that prompt, so another of
/// its files isn't offered twice.
#[gpui::test]
fn a_failed_member_asked_about_again_keeps_its_own_prompt(cx: &mut TestAppContext) {
    let _serial = serially();
    let setup = Setup::new();
    let (send, waiting) = oneshot::channel();
    let fake = Fake {
        fails: vec!["pk_mid2".to_owned()],
        waits: HashMap::from([("pk_last2".to_owned(), Rc::new(RefCell::new(Some(waiting))))]),
        ..Fake::default()
    };
    setup.give(
        vec![
            offer("pk_first2", "First", "pkfirst2", &["trio"], 1),
            offer("pk_mid2", "Middle", "pkmid2", &["trio"], 1),
            offer("pk_last2", "Last", "pklast2", &["trio"], 1),
        ],
        vec![pack("trio", "Trio", &["pk_first2", "pk_mid2", "pk_last2"])],
        None,
        |_| false,
        &fake,
        cx,
    );
    let (workbench, cx) = window(cx);

    open(&workbench, setup.file("a.pkfirst2", "x\n"), cx);
    click(&workbench, "Install Trio pack (3 languages, 3.0 MB)", cx);
    assert_eq!(
        fake.calls(),
        [
            "fetch pk_first2",
            "install pk_first2",
            "fetch pk_mid2",
            "fetch pk_last2"
        ]
    );
    // Middle failed; Last is still downloading. Middle's file asks.
    open(&workbench, setup.file("a.pkmid2", "x\n"), cx);
    assert_eq!(the_offer(&workbench, cx)[0], "Install Middle (1.0 MB)");

    send.send(()).unwrap();
    draw(cx);
    assert!(fake.calls().contains(&"install pk_last2".to_owned()));
    // The pack's over, and Middle's prompt still has it.
    open(&workbench, setup.file("b.pkmid2", "x\n"), cx);
    assert_eq!(
        offers_shown(&workbench, cx).len(),
        1,
        "{:?}",
        toasts(&workbench, cx)
    );
}
