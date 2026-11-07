//! The offer to install syntax highlighting, through a real workbench with a
//! fake catalog and installer.
//!
//! Kotlin is not built into test builds, so `a.kt` only ever stands for what's
//! asked and answered. What must start plain and then highlight uses
//! [`FIXTURE_EXTENSION`], which nothing else knows, with the JSON fixture
//! grammar registered under it.

use std::{
    cell::RefCell,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Once,
    time::Duration,
};

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use fe_gpui::{DiffSide, DiffView, EditorView};
use fe_syntax::{GrammarDeadlines, GrammarSpec};
use futures::channel::oneshot;
use gpui::{App, AppContext as _, Entity, Focusable as _, Task, TestAppContext, VisualTestContext};

use super::{
    FetchGrammar, GrammarInstallError, GrammarOffer, GrammarOffers, InstallGrammar, Launch,
    PROMPT_FILE, Workbench, set_grammar_offers,
    workbench::toast::{Choices, Level},
};

const FIXTURE_LANGUAGE: &str = "ktfixture";
const FIXTURE_EXTENSION: &str = "ktfix";
const INSTALL: usize = 0;
const NOT_NOW: usize = 1;
const NEVER: usize = 2;

fn kotlin() -> GrammarOffer {
    GrammarOffer {
        language: "kotlin".into(),
        name: "Kotlin".into(),
        extensions: vec!["kt".into(), "kts".into()],
        size: 2_800_000,
        sha256: "ab".repeat(32),
    }
}

/// Stands in for Kotlin where the tab has to start plain.
fn fixture() -> GrammarOffer {
    GrammarOffer {
        language: FIXTURE_LANGUAGE.into(),
        extensions: vec![FIXTURE_EXTENSION.into()],
        ..kotlin()
    }
}

fn swift() -> GrammarOffer {
    GrammarOffer {
        language: "swift".into(),
        name: "Swift".into(),
        extensions: vec!["swift".into()],
        size: 3_500_000,
        sha256: "cd".repeat(32),
    }
}

/// What the fake installer was asked to do, in order.
type Calls = Rc<RefCell<Vec<String>>>;

/// A channel a test holds a step of the fake installer on.
type Hold = Rc<RefCell<Option<oneshot::Receiver<()>>>>;

fn hold() -> (oneshot::Sender<()>, Hold) {
    let (send, waiting) = oneshot::channel();
    (send, Rc::new(RefCell::new(Some(waiting))))
}

/// How the fake installer answers.
#[derive(Clone)]
enum Fake {
    /// The fetch fails with this.
    Fails(GrammarInstallError),
    /// Writes it, and registers the fixture grammar.
    Registers,
    /// Writes it, but nothing ever registers.
    NeverRegisters,
    /// The fetch runs until the test sends, or the task's dropped.
    FetchWaits(Hold),
    /// As `FetchWaits`, and the install registers the fixture grammar.
    FetchWaitsThenRegisters(Hold),
    /// The write runs until the test sends, whatever else happens.
    WriteWaits(Hold),
}

/// What [`GrammarOffers`] takes, for `fake`: each step recorded in `calls`.
fn installer(calls: &Calls, fake: Fake) -> (FetchGrammar, InstallGrammar) {
    let fetch = {
        let (calls, fake) = (calls.clone(), fake.clone());
        Rc::new(move |offer: &GrammarOffer, cx: &mut App| {
            calls.borrow_mut().push(format!("fetch {}", offer.language));
            match fake.clone() {
                Fake::Fails(error) => Task::ready(Err(error)),
                Fake::FetchWaits(waiting) | Fake::FetchWaitsThenRegisters(waiting) => {
                    let waiting = waiting.borrow_mut().take().expect("one fetch");
                    cx.background_spawn(async move {
                        waiting.await.ok();
                        Ok(Vec::new())
                    })
                }
                _ => Task::ready(Ok(Vec::new())),
            }
        })
    };
    let install = {
        let calls = calls.clone();
        Rc::new(move |offer: &GrammarOffer, _: Vec<u8>, cx: &mut App| {
            calls
                .borrow_mut()
                .push(format!("install {}", offer.language));
            match fake.clone() {
                Fake::Registers | Fake::FetchWaitsThenRegisters(_) => {
                    let spec = fixture_spec();
                    cx.background_spawn(async move {
                        fe_syntax::register_language(&spec).expect("the fixture registers");
                        Ok(())
                    })
                }
                Fake::WriteWaits(waiting) => {
                    let waiting = waiting.borrow_mut().take().expect("one write");
                    cx.background_spawn(async move {
                        waiting.await.ok();
                        Ok(())
                    })
                }
                _ => Task::ready(Ok(())),
            }
        })
    };
    (fetch, install)
}

/// The fixture grammar, with a highlight of its strings, under
/// [`FIXTURE_LANGUAGE`].
fn fixture_spec() -> GrammarSpec {
    static DEADLINES: Once = Once::new();
    // A busy machine shouldn't fail this.
    DEADLINES.call_once(|| {
        fe_syntax::set_grammar_deadlines(GrammarDeadlines {
            load: Duration::from_secs(120),
            parse: Duration::from_secs(120),
        });
    });
    let dir = tempfile::tempdir().unwrap().keep();
    let highlights = dir.join("highlights.scm");
    fs::write(&highlights, "(string) @string\n").unwrap();
    GrammarSpec {
        language: FIXTURE_LANGUAGE.into(),
        wasm: Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../fe-syntax/tests/fixtures/tree-sitter-json.wasm"),
        highlights: Some(highlights),
        injections: None,
        extensions: vec![FIXTURE_EXTENSION.into()],
        file_names: Vec::new(),
        earlier_patterns_win: false,
        ..GrammarSpec::default()
    }
}

struct Setup {
    calls: Calls,
    dir: tempfile::TempDir,
}

impl Setup {
    fn never_file(&self) -> PathBuf {
        self.dir.path().join("state").join(PROMPT_FILE)
    }

    /// A file called `name` in the fixture's directory.
    fn file(&self, name: &str) -> PathBuf {
        let path = self.dir.path().join(name);
        fs::write(&path, "{\"a\": 1}\n").unwrap();
        path
    }

    fn give(
        &self,
        catalog: Vec<GrammarOffer>,
        installed: impl Fn(&str) -> bool + 'static,
        fake: Fake,
        cx: &mut TestAppContext,
    ) {
        let (fetch, install) = installer(&self.calls, fake);
        let offers = GrammarOffers {
            catalog,
            installed: Rc::new(installed),
            fetch,
            install,
            never_file: Some(self.never_file()),
        };
        cx.update(|cx| {
            fe_gpui::init(cx);
            crate::init(cx);
            set_grammar_offers(cx, offers);
        });
    }
}

fn setup() -> Setup {
    Setup {
        calls: Calls::default(),
        dir: tempfile::tempdir().unwrap(),
    }
}

fn nothing_installed(_: &str) -> bool {
    false
}

/// Whether fe-syntax has it, as the app's `installed` asks.
fn registered(language: &str) -> bool {
    fe_syntax::language_names().any(|name| name == language)
}

fn workbench(cx: &mut TestAppContext) -> (Entity<Workbench>, &mut VisualTestContext) {
    let launch = Launch {
        root: None,
        file: None,
    };
    let (workbench, cx, _) = crate::tests::open(cx, launch);
    (workbench, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn open(
    workbench: &Entity<Workbench>,
    path: &Path,
    cx: &mut VisualTestContext,
) -> Entity<EditorView> {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let path = path.to_owned();
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("the file is shown")
}

#[derive(Debug, Clone)]
struct Shown {
    level: Level,
    message: String,
    choices: Option<Choices>,
}

fn shown(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<Shown> {
    let layer = workbench.read_with(cx, |workbench, _| workbench.toasts().clone());
    layer.read_with(cx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| Shown {
                level: entry.toast.level,
                message: entry.toast.message.clone(),
                choices: entry.toast.choices.clone(),
            })
            .collect()
    })
}

fn messages(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    shown(workbench, cx)
        .into_iter()
        .map(|t| t.message)
        .collect()
}

/// The one toast shown, which must be an offer.
fn the_offer(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Choices {
    let toasts = shown(workbench, cx);
    assert_eq!(toasts.len(), 1, "{toasts:?}");
    toasts[0].choices.clone().expect("an offer has buttons")
}

/// Answers the one offer shown, and takes it away as a click does.
fn answer(workbench: &Entity<Workbench>, choice: usize, cx: &mut VisualTestContext) {
    let offer = the_offer(workbench, cx);
    offer.choose(choice);
    drop(offer);
    dismiss(workbench, cx);
}

fn dismiss(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    let layer = workbench.read_with(cx, |workbench, _| workbench.toasts().clone());
    cx.update(|_, cx| layer.update(cx, |layer, cx| layer.dismiss_all(cx)));
    draw(cx);
}

/// Far enough for an installed language to be seen to have registered.
fn settle(cx: &mut VisualTestContext) {
    for _ in 0..12 {
        cx.executor().advance_clock(Duration::from_millis(250));
        draw(cx);
    }
}

#[gpui::test]
fn offers_for_an_uninstalled_catalog_language(cx: &mut TestAppContext) {
    let setup = setup();
    setup.give(vec![kotlin()], nothing_installed, Fake::NeverRegisters, cx);
    let (workbench, cx) = workbench(cx);

    let editor = open(&workbench, &setup.file("a.kt"), cx);

    let toasts = shown(&workbench, cx);
    assert_eq!(toasts.len(), 1, "{toasts:?}");
    assert_eq!(
        toasts[0].message,
        "Syntax highlighting for Kotlin isn't installed"
    );
    assert_eq!(
        toasts[0].choices.as_ref().unwrap().titles(),
        ["Install (2.8 MB)", "Not now", "Never for .kt"]
    );
    // The tab didn't wait for an answer.
    editor.update(cx, |view, cx| {
        view.model_mut().insert("edited");
        cx.notify();
    });
    let text = editor.read_with(cx, |view, _| view.model().text().to_string());
    assert!(text.contains("edited"), "{text}");
    assert!(setup.calls.borrow().is_empty());
}

#[gpui::test]
fn nothing_downloads_without_the_click(cx: &mut TestAppContext) {
    let setup = setup();
    setup.give(
        vec![kotlin(), swift()],
        nothing_installed,
        Fake::NeverRegisters,
        cx,
    );
    let (workbench, cx) = workbench(cx);

    // Not now.
    open(&workbench, &setup.file("a.swift"), cx);
    assert!(setup.calls.borrow().is_empty());
    answer(&workbench, NOT_NOW, cx);
    assert!(setup.calls.borrow().is_empty());

    // Closing the toast.
    open(&workbench, &setup.file("a.kt"), cx);
    the_offer(&workbench, cx);
    assert!(setup.calls.borrow().is_empty());
    dismiss(&workbench, cx);
    assert!(setup.calls.borrow().is_empty());

    // Ten more files of it.
    for i in 0..10 {
        open(&workbench, &setup.file(&format!("more{i}.kt")), cx);
    }
    assert_eq!(messages(&workbench, cx), Vec::<String>::new());
    assert!(setup.calls.borrow().is_empty());
}

/// The first highlighted span of `editor` and its colour, once it's parsed.
fn highlighted(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<gpui::Hsla> {
    let theme = cx.update(|_, cx| fe_gpui::EditorTheme::default_dark(cx));
    editor.read_with(cx, |view, _| {
        let syntax = view.syntax()?;
        let len = view.model().text().len();
        let span = syntax.highlights(0..len).into_iter().next()?;
        theme.syntax_color(span.highlight)
    })
}

#[gpui::test]
fn install_fetches_registers_and_rehighlights(cx: &mut TestAppContext) {
    let setup = setup();
    setup.give(vec![fixture()], registered, Fake::Registers, cx);
    let (workbench, cx) = workbench(cx);

    let name = format!("a.{FIXTURE_EXTENSION}");
    let hidden = open(&workbench, &setup.file(&name), cx);
    assert!(hidden.read_with(cx, |view, _| view.syntax().is_none()));
    let other = format!("b.{FIXTURE_EXTENSION}");
    let active = open(&workbench, &setup.file(&other), cx);
    assert!(active.read_with(cx, |view, _| view.syntax().is_none()));
    assert_ne!(hidden, active);

    answer(&workbench, INSTALL, cx);
    settle(cx);

    assert_eq!(
        *setup.calls.borrow(),
        [
            format!("fetch {FIXTURE_LANGUAGE}"),
            format!("install {FIXTURE_LANGUAGE}"),
        ]
    );
    for editor in [&hidden, &active] {
        let language = editor.read_with(cx, |view, _| {
            view.syntax().map(|syntax| syntax.language().name())
        });
        assert_eq!(language, Some(FIXTURE_LANGUAGE));
    }
    let toasts = messages(&workbench, cx);
    assert_eq!(toasts, ["Installed Kotlin syntax"]);

    // It paints in a colour of its own, as the theme gives a string.
    for _ in 0..200 {
        draw(cx);
        if highlighted(&active, cx).is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let theme = cx.update(|_, cx| fe_gpui::EditorTheme::default_dark(cx));
    let colour = highlighted(&active, cx).expect("a highlighted row");
    assert_ne!(colour, theme.foreground);
}

#[gpui::test]
fn a_language_chosen_by_hand_stays_after_an_install(cx: &mut TestAppContext) {
    let setup = setup();
    setup.give(vec![fixture()], registered, Fake::Registers, cx);
    let (workbench, cx) = workbench(cx);

    let chosen = open(
        &workbench,
        &setup.file(&format!("a.{FIXTURE_EXTENSION}")),
        cx,
    );
    answer(&workbench, INSTALL, cx);
    chosen.update(cx, |view, cx| view.set_language(None, cx));
    settle(cx);

    assert!(chosen.read_with(cx, |view, _| view.syntax().is_none()));
    assert_eq!(messages(&workbench, cx), ["Installed Kotlin syntax"]);
}

#[gpui::test]
fn an_install_that_never_registers_says_so(cx: &mut TestAppContext) {
    let setup = setup();
    setup.give(vec![fixture()], nothing_installed, Fake::NeverRegisters, cx);
    let (workbench, cx) = workbench(cx);

    open(
        &workbench,
        &setup.file(&format!("a.{FIXTURE_EXTENSION}")),
        cx,
    );
    answer(&workbench, INSTALL, cx);
    cx.executor().advance_clock(Duration::from_secs(61));
    draw(cx);

    let toasts = shown(&workbench, cx);
    assert_eq!(toasts.len(), 1, "{toasts:?}");
    assert_eq!(toasts[0].level, Level::Warning);
    assert!(toasts[0].message.contains("hasn't loaded"), "{toasts:?}");
}

#[gpui::test]
fn failure_is_one_readable_line(cx: &mut TestAppContext) {
    let setup = setup();
    let offline = GrammarInstallError {
        reason: "no connection\nto the download".into(),
        offline: true,
    };
    setup.give(vec![fixture()], nothing_installed, Fake::Fails(offline), cx);
    let (workbench, cx) = workbench(cx);

    let name = format!("a.{FIXTURE_EXTENSION}");
    let editor = open(&workbench, &setup.file(&name), cx);
    answer(&workbench, INSTALL, cx);
    draw(cx);

    let toasts = shown(&workbench, cx);
    assert_eq!(toasts.len(), 1, "{toasts:?}");
    assert_eq!(toasts[0].level, Level::Error);
    assert_eq!(
        toasts[0].message,
        "Couldn't install Kotlin syntax: no connection to the download. \
         The language pack installs it without a connection."
    );
    assert!(editor.read_with(cx, |view, _| view.syntax().is_none()));
    dismiss(&workbench, cx);

    // Opening another asks again. A reason other than being offline has no
    // mention of the pack.
    let mismatch = GrammarInstallError {
        reason: "the download didn't match its recorded hash".into(),
        offline: false,
    };
    setup.give(
        vec![fixture()],
        nothing_installed,
        Fake::Fails(mismatch),
        cx,
    );
    open(
        &workbench,
        &setup.file(&format!("b.{FIXTURE_EXTENSION}")),
        cx,
    );
    answer(&workbench, INSTALL, cx);
    draw(cx);
    assert_eq!(
        messages(&workbench, cx),
        ["Couldn't install Kotlin syntax: the download didn't match its recorded hash."]
    );
}

#[gpui::test]
fn not_now_is_per_session_and_never_is_remembered(
    cx: &mut TestAppContext,
    next_launch: &mut TestAppContext,
) {
    let setup = setup();
    setup.give(
        vec![kotlin(), swift()],
        nothing_installed,
        Fake::NeverRegisters,
        cx,
    );
    let (first, cx) = workbench(cx);

    // Not now holds for the language, in a window opened since too.
    open(&first, &setup.file("a.swift"), cx);
    answer(&first, NOT_NOW, cx);
    open(&first, &setup.file("b.swift"), cx);
    assert_eq!(messages(&first, cx), Vec::<String>::new());
    assert!(!setup.never_file().exists(), "Not now is not written");

    open(&first, &setup.file("a.kt"), cx);
    answer(&first, NEVER, cx);
    open(&first, &setup.file("b.kt"), cx);
    assert_eq!(messages(&first, cx), Vec::<String>::new());

    let written: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(setup.never_file()).unwrap()).unwrap();
    assert_eq!(written, serde_json::json!({ "never": ["kt"] }));

    // Never is per extension: Kotlin's other one still asks.
    open(&first, &setup.file("c.kts"), cx);
    let toasts = shown(&first, cx);
    assert_eq!(toasts.len(), 1, "{toasts:?}");
    assert_eq!(
        toasts[0].choices.as_ref().unwrap().titles()[NEVER],
        "Never for .kts"
    );

    // A new launch: its own app, with only the file to go by.
    setup.give(
        vec![kotlin(), swift()],
        nothing_installed,
        Fake::NeverRegisters,
        next_launch,
    );
    let (fresh, next_launch) = workbench(next_launch);
    open(&fresh, &setup.file("d.kt"), next_launch);
    assert_eq!(messages(&fresh, next_launch), Vec::<String>::new());
    // What Not now silenced asks again.
    open(&fresh, &setup.file("c.swift"), next_launch);
    let toasts = messages(&fresh, next_launch);
    assert_eq!(toasts, ["Syntax highlighting for Swift isn't installed"]);
}

#[gpui::test]
fn no_offer_when_it_would_not_help(cx: &mut TestAppContext) {
    let setup = setup();
    let unreleased = GrammarOffer {
        language: "unreleased".into(),
        extensions: vec!["unrel".into()],
        sha256: String::new(),
        ..fixture()
    };
    // Its extension is Rust's: another language has the file.
    let claimed = GrammarOffer {
        language: "other-rust".into(),
        extensions: vec!["rs".into()],
        ..fixture()
    };
    let there = GrammarOffer {
        language: "there".into(),
        extensions: vec!["there".into()],
        ..fixture()
    };
    let (send, waiting) = hold();
    setup.give(
        vec![fixture(), unreleased, claimed, there],
        |language| language == "there",
        Fake::FetchWaitsThenRegisters(waiting),
        cx,
    );
    let (workbench, cx) = workbench(cx);

    for name in ["a.there", "a.nothing", "a.unrel", "a.rs", "no_extension"] {
        open(&workbench, &setup.file(name), cx);
        assert_eq!(messages(&workbench, cx), Vec::<String>::new(), "{name}");
    }

    // One toast and one fetch, whatever opens while it installs.
    open(&workbench, &setup.file("a.ktfix"), cx);
    open(&workbench, &setup.file("b.ktfix"), cx);
    answer(&workbench, INSTALL, cx);
    assert_eq!(*setup.calls.borrow(), [format!("fetch {FIXTURE_LANGUAGE}")]);
    let before = messages(&workbench, cx);
    assert_eq!(before, ["Installing Kotlin syntax…"]);
    open(&workbench, &setup.file("c.ktfix"), cx);
    assert_eq!(messages(&workbench, cx), before);
    assert_eq!(*setup.calls.borrow(), [format!("fetch {FIXTURE_LANGUAGE}")]);
    send.send(()).unwrap();
    settle(cx);
    assert_eq!(
        *setup.calls.borrow(),
        [
            format!("fetch {FIXTURE_LANGUAGE}"),
            format!("install {FIXTURE_LANGUAGE}")
        ]
    );
    // The install registers the grammar, so it's there at once.
    assert_eq!(messages(&workbench, cx), ["Installed Kotlin syntax"]);
}

#[gpui::test]
fn a_registered_or_built_in_language_is_not_offered(cx: &mut TestAppContext) {
    let setup = setup();
    // Asked as the app asks, Go's built in here.
    let go = GrammarOffer {
        language: "go".into(),
        extensions: vec!["go".into()],
        ..kotlin()
    };
    setup.give(vec![go], registered, Fake::NeverRegisters, cx);
    let (workbench, cx) = workbench(cx);

    open(&workbench, &setup.file("a.go"), cx);
    assert_eq!(messages(&workbench, cx), Vec::<String>::new());
}

#[gpui::test]
fn a_rename_into_the_language_offers(cx: &mut TestAppContext) {
    let setup = setup();
    setup.give(vec![kotlin()], nothing_installed, Fake::NeverRegisters, cx);
    let (workbench, cx) = workbench(cx);

    let from = setup.file("a.txt");
    open(&workbench, &from, cx);
    assert_eq!(messages(&workbench, cx), Vec::<String>::new());
    let to = setup.file("moved.kt");
    workbench.update(cx, |workbench, cx| workbench.follow_rename(&from, &to, cx));
    draw(cx);
    the_offer(&workbench, cx);
}

#[gpui::test]
fn a_file_read_before_the_window_is_offered(cx: &mut TestAppContext) {
    let setup = setup();
    setup.give(vec![kotlin()], nothing_installed, Fake::NeverRegisters, cx);
    let launch = Launch {
        root: None,
        file: Some((
            setup.file("a.kt"),
            Some(Buffer::new("fun main() {}\n").into()),
        )),
    };
    let (workbench, cx, _) = crate::tests::open(cx, launch);
    draw(cx);
    the_offer(&workbench, cx);
}

#[gpui::test]
fn a_file_read_after_the_window_is_offered_once_it_is_there(cx: &mut TestAppContext) {
    let setup = setup();
    setup.give(vec![kotlin()], nothing_installed, Fake::NeverRegisters, cx);
    let path = setup.file("a.kt");
    let launch = Launch {
        root: None,
        file: Some((path.clone(), None)),
    };
    let (workbench, cx, _) = crate::tests::open(cx, launch);
    draw(cx);
    assert_eq!(messages(&workbench, cx), Vec::<String>::new());

    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            let read = Ok(Buffer::new("fun main() {}\n").into());
            workbench.file_read(path, read, window, cx);
        });
    });
    draw(cx);
    the_offer(&workbench, cx);
}

#[gpui::test]
fn the_sides_of_a_diff_tab_highlight_too(cx: &mut TestAppContext) {
    let setup = setup();
    setup.give(vec![fixture()], registered, Fake::Registers, cx);
    let (workbench, cx) = workbench(cx);

    let name = format!("a.{FIXTURE_EXTENSION}");
    let path = setup.file(&name);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let diff = cx.update(|_, cx| {
        let model = |path: Option<PathBuf>| EditorModel::new(Buffer::new("{\"a\": 1}\n"), path);
        let diff = cx.new(|cx| {
            let left = DiffSide::new("HEAD", model(None)).read_only();
            let right = DiffSide::new("Working Tree", model(Some(path.clone())));
            DiffView::new(name.clone(), left, right, cx)
        });
        pane.update(cx, |pane, cx| pane.add_item(Box::new(diff.clone()), cx));
        diff
    });
    draw(cx);
    let (left, right) = diff.read_with(cx, |diff, _| (diff.left().clone(), diff.right().clone()));
    for side in [&left, &right] {
        assert!(side.read_with(cx, |view, _| view.syntax().is_none()));
    }

    answer(&workbench, INSTALL, cx);
    settle(cx);

    for side in [&left, &right] {
        let language = side.read_with(cx, |view, _| {
            view.syntax().map(|syntax| syntax.language().name())
        });
        assert_eq!(language, Some(FIXTURE_LANGUAGE));
    }
}

/// Opens each of `names` as a tab, all in the one update, so that they're
/// looked at together.
fn open_together(
    workbench: &Entity<Workbench>,
    setup: &Setup,
    names: &[&str],
    cx: &mut VisualTestContext,
) {
    let paths: Vec<_> = names.iter().map(|name| setup.file(name)).collect();
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|_, cx| {
        for path in paths {
            let view = cx.new(|cx| {
                let model = EditorModel::new(Buffer::new("x\n"), Some(path));
                EditorView::new(model, fe_gpui::EditorTheme::default_dark(cx), cx)
            });
            pane.update(cx, |pane, cx| pane.add_item(Box::new(view), cx));
        }
    });
    draw(cx);
}

#[gpui::test]
fn a_remembered_never_leaves_the_other_extension_to_ask(cx: &mut TestAppContext) {
    let setup = setup();
    setup.give(vec![kotlin()], nothing_installed, Fake::NeverRegisters, cx);
    fs::create_dir_all(setup.never_file().parent().unwrap()).unwrap();
    fs::write(setup.never_file(), r#"{"never": ["kt"]}"#).unwrap();
    let (workbench, cx) = workbench(cx);

    open_together(&workbench, &setup, &["a.kt", "b.kts"], cx);

    assert_eq!(the_offer(&workbench, cx).titles()[NEVER], "Never for .kts");
}

#[gpui::test]
fn never_goes_on_to_the_other_extension_and_not_now_does_not(cx: &mut TestAppContext) {
    let setup = setup();
    setup.give(
        vec![kotlin(), swift()],
        nothing_installed,
        Fake::NeverRegisters,
        cx,
    );
    let (workbench, cx) = workbench(cx);

    open_together(&workbench, &setup, &["a.kt", "b.kts", "c.kt"], cx);
    assert_eq!(the_offer(&workbench, cx).titles()[NEVER], "Never for .kt");
    answer(&workbench, NEVER, cx);
    draw(cx);
    assert_eq!(the_offer(&workbench, cx).titles()[NEVER], "Never for .kts");
    answer(&workbench, NEVER, cx);
    draw(cx);
    assert_eq!(messages(&workbench, cx), Vec::<String>::new());

    // Not now is for the language, so nothing waiting asks after it.
    fs::remove_file(setup.never_file()).unwrap();
    let dart = GrammarOffer {
        language: "dart".into(),
        name: "Dart".into(),
        extensions: vec!["dart".into(), "dt".into()],
        ..kotlin()
    };
    setup.give(vec![dart], nothing_installed, Fake::NeverRegisters, cx);
    open_together(&workbench, &setup, &["a.dart", "b.dt"], cx);
    answer(&workbench, NOT_NOW, cx);
    draw(cx);
    assert_eq!(messages(&workbench, cx), Vec::<String>::new());
}

#[gpui::test]
fn a_late_registration_still_highlights_after_the_warning(cx: &mut TestAppContext) {
    let setup = setup();
    setup.give(vec![fixture()], nothing_installed, Fake::NeverRegisters, cx);
    let (workbench, cx) = workbench(cx);

    let editor = open(
        &workbench,
        &setup.file(&format!("a.{FIXTURE_EXTENSION}")),
        cx,
    );
    answer(&workbench, INSTALL, cx);
    cx.executor().advance_clock(Duration::from_secs(61));
    draw(cx);
    let toasts = shown(&workbench, cx);
    assert_eq!(toasts[0].level, Level::Warning, "{toasts:?}");
    assert!(editor.read_with(cx, |view, _| view.syntax().is_none()));

    fe_syntax::register_language(&fixture_spec()).unwrap();
    settle(cx);

    assert_eq!(language_of(&editor, cx), Some(FIXTURE_LANGUAGE));
    assert!(
        messages(&workbench, cx).contains(&"Installed Kotlin syntax".to_owned()),
        "{:?}",
        messages(&workbench, cx)
    );
}

fn language_of(editor: &Entity<EditorView>, cx: &VisualTestContext) -> Option<&'static str> {
    editor.read_with(cx, |view, _| {
        view.syntax().map(|syntax| syntax.language().name())
    })
}

#[gpui::test]
fn closing_the_window_after_the_install_still_highlights_the_others(cx: &mut TestAppContext) {
    let setup = setup();
    setup.give(vec![fixture()], nothing_installed, Fake::NeverRegisters, cx);
    let launch = || Launch {
        root: None,
        file: None,
    };
    let (first, first_cx, _) = crate::tests::open(cx, launch());
    open(
        &first,
        &setup.file(&format!("a.{FIXTURE_EXTENSION}")),
        first_cx,
    );
    let mut first_cx = first_cx.clone();
    let (second, second_cx, _) = crate::tests::open(cx, launch());
    let elsewhere = open(
        &second,
        &setup.file(&format!("b.{FIXTURE_EXTENSION}")),
        second_cx,
    );
    let mut second_cx = second_cx.clone();

    answer(&first, INSTALL, &mut first_cx);
    first_cx.update(|window, _| window.remove_window());
    draw(&mut second_cx);
    assert!(language_of(&elsewhere, &second_cx).is_none());

    fe_syntax::register_language(&fixture_spec()).unwrap();
    settle(&mut second_cx);

    assert_eq!(language_of(&elsewhere, &second_cx), Some(FIXTURE_LANGUAGE));
    assert_eq!(
        messages(&second, &mut second_cx),
        ["Installed Kotlin syntax"]
    );
}

#[gpui::test]
fn closing_the_window_while_it_writes_still_highlights_the_others(cx: &mut TestAppContext) {
    let setup = setup();
    let (send, waiting) = hold();
    setup.give(
        vec![fixture()],
        nothing_installed,
        Fake::WriteWaits(waiting),
        cx,
    );
    let launch = || Launch {
        root: None,
        file: None,
    };
    let (first, first_cx, _) = crate::tests::open(cx, launch());
    open(
        &first,
        &setup.file(&format!("a.{FIXTURE_EXTENSION}")),
        first_cx,
    );
    let mut first_cx = first_cx.clone();
    let (second, second_cx, _) = crate::tests::open(cx, launch());
    let elsewhere = open(
        &second,
        &setup.file(&format!("b.{FIXTURE_EXTENSION}")),
        second_cx,
    );
    let mut second_cx = second_cx.clone();

    answer(&first, INSTALL, &mut first_cx);
    assert_eq!(
        *setup.calls.borrow(),
        [
            format!("fetch {FIXTURE_LANGUAGE}"),
            format!("install {FIXTURE_LANGUAGE}"),
        ]
    );
    first_cx.update(|window, _| window.remove_window());
    draw(&mut second_cx);

    // It's still under way, so a file opening asks nothing and fetches nothing.
    open(
        &second,
        &setup.file(&format!("c.{FIXTURE_EXTENSION}")),
        &mut second_cx,
    );
    assert_eq!(messages(&second, &mut second_cx), Vec::<String>::new());
    assert_eq!(setup.calls.borrow().len(), 2);

    send.send(()).unwrap();
    fe_syntax::register_language(&fixture_spec()).unwrap();
    settle(&mut second_cx);

    assert_eq!(language_of(&elsewhere, &second_cx), Some(FIXTURE_LANGUAGE));
    assert_eq!(
        messages(&second, &mut second_cx),
        ["Installed Kotlin syntax"]
    );
}

#[gpui::test]
fn closing_the_window_while_it_downloads_writes_nothing_and_can_ask_again(cx: &mut TestAppContext) {
    let setup = setup();
    let (send, waiting) = hold();
    setup.give(
        vec![fixture()],
        nothing_installed,
        Fake::FetchWaits(waiting),
        cx,
    );
    let launch = || Launch {
        root: None,
        file: None,
    };
    let (first, first_cx, _) = crate::tests::open(cx, launch());
    open(
        &first,
        &setup.file(&format!("a.{FIXTURE_EXTENSION}")),
        first_cx,
    );
    let mut first_cx = first_cx.clone();
    let (second, second_cx, _) = crate::tests::open(cx, launch());
    let mut second_cx = second_cx.clone();

    answer(&first, INSTALL, &mut first_cx);
    first_cx.update(|window, _| window.remove_window());
    draw(&mut second_cx);
    // Too late to matter: nobody's left to hear.
    send.send(()).ok();
    settle(&mut second_cx);
    assert_eq!(
        *setup.calls.borrow(),
        [format!("fetch {FIXTURE_LANGUAGE}")],
        "nothing is written"
    );

    open(
        &second,
        &setup.file(&format!("b.{FIXTURE_EXTENSION}")),
        &mut second_cx,
    );
    the_offer(&second, &mut second_cx);
}

#[gpui::test]
fn never_is_written_even_if_its_window_closes_at_once(
    cx: &mut TestAppContext,
    next_launch: &mut TestAppContext,
) {
    let setup = setup();
    setup.give(vec![kotlin()], nothing_installed, Fake::NeverRegisters, cx);
    let launch = || Launch {
        root: None,
        file: None,
    };
    let (first, first_cx, _) = crate::tests::open(cx, launch());
    open(&first, &setup.file("a.kt"), first_cx);
    let offer = the_offer(&first, first_cx);
    let mut first_cx = first_cx.clone();
    let (_second, second_cx, _) = crate::tests::open(cx, launch());
    let mut second_cx = second_cx.clone();

    // Nothing has had a chance to run between the two.
    offer.choose(NEVER);
    first_cx.update(|window, _| window.remove_window());
    drop(offer);
    draw(&mut second_cx);

    setup.give(
        vec![kotlin()],
        nothing_installed,
        Fake::NeverRegisters,
        next_launch,
    );
    let (fresh, next_launch) = workbench(next_launch);
    open(&fresh, &setup.file("b.kt"), next_launch);
    assert_eq!(messages(&fresh, next_launch), Vec::<String>::new());
}

/// A diff tab of `name` whose git side's text starts with a shebang, and its two
/// sides.
fn diff_with_a_shebang(
    workbench: &Entity<Workbench>,
    setup: &Setup,
    name: &str,
    cx: &mut VisualTestContext,
) -> (Entity<EditorView>, Entity<EditorView>) {
    let path = setup.file(name);
    let name = name.to_owned();
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let diff = cx.update(|_, cx| {
        let diff = cx.new(|cx| {
            let text = "#!/bin/bash\necho hi\n";
            let left = EditorModel::new(Buffer::new(text), None);
            let left = DiffSide::new("HEAD", left).read_only();
            let right = EditorModel::new(Buffer::new(text), Some(path.clone()));
            let right = DiffSide::new("Working Tree", right);
            DiffView::new(name.clone(), left, right, cx)
        });
        pane.update(cx, |pane, cx| pane.add_item(Box::new(diff.clone()), cx));
        diff
    });
    settle(cx);
    diff.read_with(cx, |diff, _| (diff.left().clone(), diff.right().clone()))
}

#[gpui::test]
fn a_git_side_goes_by_the_files_name_over_its_first_line(cx: &mut TestAppContext) {
    let setup = setup();
    setup.give(vec![fixture()], registered, Fake::Registers, cx);
    let (workbench, cx) = workbench(cx);

    // As soon as the view opens, the name being known.
    let (left, right) = diff_with_a_shebang(&workbench, &setup, "a.rs", cx);
    assert_eq!(language_of(&left, cx), Some("rust"));
    assert_eq!(language_of(&right, cx), Some("rust"));
    assert!(!left.read_with(cx, |view, _| view.is_language_chosen()));

    // And once the language arrives, as the first line was all there was.
    let name = format!("b.{FIXTURE_EXTENSION}");
    let (left, right) = diff_with_a_shebang(&workbench, &setup, &name, cx);
    assert_eq!(language_of(&left, cx), Some("bash"));
    assert_eq!(language_of(&right, cx), Some("bash"));

    answer(&workbench, INSTALL, cx);
    settle(cx);

    assert_eq!(language_of(&left, cx), Some(FIXTURE_LANGUAGE));
    assert_eq!(language_of(&right, cx), Some(FIXTURE_LANGUAGE));
}

#[gpui::test]
fn detecting_again_keeps_a_git_sides_syntax_when_nothing_changed(cx: &mut TestAppContext) {
    let setup = setup();
    setup.give(vec![fixture()], registered, Fake::Registers, cx);
    let (workbench, cx) = workbench(cx);

    let (left, _) = diff_with_a_shebang(&workbench, &setup, "a.rs", cx);
    let parsed = left.read_with(cx, |view, _| view.syntax().unwrap().tree().is_some());
    assert!(parsed);

    open(
        &workbench,
        &setup.file(&format!("b.{FIXTURE_EXTENSION}")),
        cx,
    );
    answer(&workbench, INSTALL, cx);
    // Before any frame could parse it again, were it reset.
    cx.executor().advance_clock(Duration::from_millis(250));
    cx.run_until_parked();

    let kept = left.read_with(cx, |view, _| {
        (
            view.syntax().unwrap().language().name(),
            view.syntax().unwrap().tree().is_some(),
        )
    });
    assert_eq!(kept, ("rust", true));
}

/// Runs `:diffsplit other` from the active editor, as Compare Active File With
/// does once a file's picked. The tab it opens, and its two sides.
fn compare_with(
    workbench: &Entity<Workbench>,
    other: &Path,
    cx: &mut VisualTestContext,
) -> (Entity<EditorView>, Entity<EditorView>) {
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&workbench.focus_handle(cx), cx);
        fe_gpui::set_vim_enabled(cx, true);
    });
    draw(cx);
    cx.simulate_keystrokes(":");
    cx.simulate_input(&format!("diffsplit {}", other.display()));
    cx.simulate_keystrokes("enter");
    draw(cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let diff = pane.read_with(cx, |pane, _| {
        (pane.tabs().iter())
            .find_map(|tab| tab.item().downcast_ref::<DiffView>().cloned())
            .expect("the comparison's tab")
    });
    diff.read_with(cx, |diff, _| (diff.left().clone(), diff.right().clone()))
}

#[gpui::test]
fn compare_with_file_of_the_same_extension_highlights_both(cx: &mut TestAppContext) {
    let setup = setup();
    setup.give(vec![fixture()], registered, Fake::Registers, cx);
    let (workbench, cx) = workbench(cx);

    open(
        &workbench,
        &setup.file(&format!("a.{FIXTURE_EXTENSION}")),
        cx,
    );
    let other = setup.file(&format!("other.{FIXTURE_EXTENSION}"));
    let (left, right) = compare_with(&workbench, &other, cx);
    assert_eq!(language_of(&left, cx), None);
    assert!(!left.read_with(cx, |view, _| view.is_language_chosen()));

    answer(&workbench, INSTALL, cx);
    settle(cx);

    assert_eq!(language_of(&left, cx), Some(FIXTURE_LANGUAGE));
    assert_eq!(language_of(&right, cx), Some(FIXTURE_LANGUAGE));
}

#[gpui::test]
fn compare_with_file_highlights_each_side_from_its_own_file(cx: &mut TestAppContext) {
    let setup = setup();
    setup.give(vec![fixture()], registered, Fake::Registers, cx);
    let (workbench, cx) = workbench(cx);

    // Something to answer the offer from, as a left side doesn't ask.
    open(
        &workbench,
        &setup.file(&format!("asks.{FIXTURE_EXTENSION}")),
        cx,
    );
    // The left is of the language and the right isn't.
    open(&workbench, &setup.file("a.txt"), cx);
    let other = setup.file(&format!("other.{FIXTURE_EXTENSION}"));
    let (left, right) = compare_with(&workbench, &other, cx);

    answer(&workbench, INSTALL, cx);
    settle(cx);

    assert_eq!(language_of(&left, cx), Some(FIXTURE_LANGUAGE));
    assert_eq!(language_of(&right, cx), None);
}

#[gpui::test]
fn compare_with_file_of_another_language_keeps_its_own(cx: &mut TestAppContext) {
    let setup = setup();
    setup.give(vec![fixture()], registered, Fake::Registers, cx);
    let (workbench, cx) = workbench(cx);

    // The right is of the language and the left is Rust.
    open(
        &workbench,
        &setup.file(&format!("a.{FIXTURE_EXTENSION}")),
        cx,
    );
    let (left, right) = compare_with(&workbench, &setup.file("other.rs"), cx);
    for _ in 0..3 {
        settle(cx);
    }
    assert_eq!(language_of(&left, cx), Some("rust"));

    answer(&workbench, INSTALL, cx);
    settle(cx);

    assert_eq!(language_of(&left, cx), Some("rust"));
    assert_eq!(language_of(&right, cx), Some(FIXTURE_LANGUAGE));
}

#[gpui::test]
fn plain_text_picked_on_a_compared_side_stays(cx: &mut TestAppContext) {
    let setup = setup();
    setup.give(vec![fixture()], registered, Fake::Registers, cx);
    let (workbench, cx) = workbench(cx);

    open(
        &workbench,
        &setup.file(&format!("a.{FIXTURE_EXTENSION}")),
        cx,
    );
    let other = setup.file(&format!("other.{FIXTURE_EXTENSION}"));
    let (left, right) = compare_with(&workbench, &other, cx);
    left.update(cx, |view, cx| view.set_language(None, cx));

    answer(&workbench, INSTALL, cx);
    settle(cx);

    assert_eq!(language_of(&left, cx), None);
    assert_eq!(language_of(&right, cx), Some(FIXTURE_LANGUAGE));
}

#[gpui::test]
fn a_tab_told_where_its_language_comes_from_highlights(cx: &mut TestAppContext) {
    let setup = setup();
    setup.give(vec![fixture()], registered, Fake::Registers, cx);
    let (workbench, cx) = workbench(cx);

    // What a file at a revision is: no path, and its language from the file's.
    let source = setup.file(&format!("a.{FIXTURE_EXTENSION}"));
    let editor = cx.update(|_, cx| {
        cx.new(|cx| {
            let model = EditorModel::new(Buffer::new("{\"a\": 1}\n"), None);
            let mut view = EditorView::new(model, fe_gpui::EditorTheme::default_dark(cx), cx);
            view.detect_language_from(source, cx);
            view
        })
    });
    assert!(!editor.read_with(cx, |view, _| view.is_language_chosen()));
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|_, cx| {
        pane.update(cx, |pane, cx| pane.add_item(Box::new(editor.clone()), cx));
    });
    draw(cx);
    let file = setup.file(&format!("b.{FIXTURE_EXTENSION}"));
    open(&workbench, &file, cx);

    answer(&workbench, INSTALL, cx);
    settle(cx);

    assert_eq!(language_of(&editor, cx), Some(FIXTURE_LANGUAGE));
}

#[gpui::test]
fn plain_text_picked_on_a_diff_side_stays_after_an_install(cx: &mut TestAppContext) {
    let setup = setup();
    setup.give(vec![fixture()], registered, Fake::Registers, cx);
    let (workbench, cx) = workbench(cx);

    let name = format!("a.{FIXTURE_EXTENSION}");
    let path = setup.file(&name);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let diff = cx.update(|_, cx| {
        let model = |path| EditorModel::new(Buffer::new("{\"a\": 1}\n"), path);
        let diff = cx.new(|cx| {
            let left = DiffSide::new("HEAD", model(None)).read_only();
            let right = DiffSide::new("Working Tree", model(Some(path.clone())));
            DiffView::new(name.clone(), left, right, cx)
        });
        pane.update(cx, |pane, cx| pane.add_item(Box::new(diff.clone()), cx));
        diff
    });
    draw(cx);
    let (left, right) = diff.read_with(cx, |diff, _| (diff.left().clone(), diff.right().clone()));
    assert!(!left.read_with(cx, |view, _| view.is_language_chosen()));

    left.update(cx, |view, cx| view.set_language(None, cx));
    answer(&workbench, INSTALL, cx);
    settle(cx);

    assert_eq!(language_of(&left, cx), None);
    assert_eq!(language_of(&right, cx), Some(FIXTURE_LANGUAGE));
}

#[gpui::test]
fn another_window_highlights_too_and_is_not_asked(cx: &mut TestAppContext) {
    let setup = setup();
    setup.give(vec![fixture()], registered, Fake::Registers, cx);
    let launch = || Launch {
        root: None,
        file: None,
    };
    let (first, first_cx, _) = crate::tests::open(cx, launch());
    let name = format!("a.{FIXTURE_EXTENSION}");
    let editor = open(&first, &setup.file(&name), first_cx);
    the_offer(&first, first_cx);
    let mut first_cx = first_cx.clone();

    let (second, second_cx, _) = crate::tests::open(cx, launch());
    let other = format!("b.{FIXTURE_EXTENSION}");
    let elsewhere = open(&second, &setup.file(&other), second_cx);
    assert_eq!(messages(&second, second_cx), Vec::<String>::new());

    answer(&first, INSTALL, &mut first_cx);
    settle(&mut first_cx);

    for editor in [&editor, &elsewhere] {
        let language = editor.read_with(&first_cx, |view, _| {
            view.syntax().map(|syntax| syntax.language().name())
        });
        assert_eq!(language, Some(FIXTURE_LANGUAGE));
    }
}
