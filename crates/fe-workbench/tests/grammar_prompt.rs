//! The grammar prompt from outside the crate: two files opening at once are
//! asked about once, and an install stops with its workbench.

use std::{
    cell::RefCell,
    fs,
    path::Path,
    rc::Rc,
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
};

use fe_gpui::{EditorSettings, EditorTheme};
use fe_workbench::{GrammarOffer, GrammarOffers, Launch, Workbench, set_grammar_offers};
use gpui::{AppContext as _, Entity, TestAppContext, VisualTestContext};

static SERIAL: Mutex<()> = Mutex::new(());

/// Runs the rest of a test with no other of this binary running: with nothing
/// naming a config directory, the workbench neither reads the user's settings
/// nor watches them from a thread the test scheduler would reject.
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

fn kotlin() -> GrammarOffer {
    GrammarOffer {
        language: "kotlin".into(),
        name: "Kotlin".into(),
        extensions: vec!["kt".into()],
        size: 2_800_000,
        sha256: "ab".repeat(32),
    }
}

/// Goes when the fake install's task does.
struct DroppedFlag(Arc<AtomicBool>);

impl Drop for DroppedFlag {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// Offers Kotlin, whose install never ends. The number of installs, and whether
/// the last has been dropped.
fn give(dir: &Path, cx: &mut TestAppContext) -> (Rc<RefCell<u32>>, Arc<AtomicBool>) {
    let (installs, dropped) = (Rc::new(RefCell::new(0)), Arc::new(AtomicBool::new(false)));
    let fetch = {
        let (installs, dropped) = (installs.clone(), dropped.clone());
        Rc::new(move |_: &GrammarOffer, cx: &mut gpui::App| {
            *installs.borrow_mut() += 1;
            let flag = DroppedFlag(dropped.clone());
            cx.background_spawn(async move {
                let _flag = flag;
                std::future::pending::<()>().await;
                Ok(Vec::new())
            })
        })
    };
    let offers = GrammarOffers {
        catalog: vec![kotlin()],
        installed: Rc::new(|_| false),
        fetch,
        install: Rc::new(|_, _, _| panic!("the fetch never ends")),
        never_file: Some(dir.join(fe_workbench::PROMPT_FILE)),
    };
    cx.update(|cx| {
        fe_gpui::init(cx);
        fe_workbench::init(cx);
        set_grammar_offers(cx, offers);
    });
    (installs, dropped)
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

/// Opens both in the one update, as a restored layout or a drop does.
fn open_both(workbench: &Entity<Workbench>, dir: &Path, cx: &mut VisualTestContext) {
    let paths = ["a.kt", "b.kt"].map(|name| {
        let path = dir.join(name);
        fs::write(&path, "fun main() {}\n").unwrap();
        path
    });
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            for path in paths {
                workbench.open_document(path, window, cx);
            }
        });
    });
    draw(cx);
}

/// Whether the window draws what `selector` names.
fn has(selector: String, cx: &mut VisualTestContext) -> bool {
    cx.debug_bounds(Box::leak(selector.into_boxed_str()))
        .is_some()
}

#[gpui::test]
fn two_tabs_opening_at_once_are_one_toast_and_one_install(cx: &mut TestAppContext) {
    let _serial = serially();
    let dir = tempfile::tempdir().unwrap();
    let (installs, _) = give(dir.path(), cx);
    let (workbench, cx) = window(cx);

    open_both(&workbench, dir.path(), cx);

    assert!(has("toast-0".into(), cx), "the offer shows");
    assert!(!has("toast-1".into(), cx), "and only the one");
    assert_eq!(*installs.borrow(), 0);

    let install = cx.debug_bounds("toast-choice-0-0").expect("Install");
    cx.simulate_click(install.center(), gpui::Modifiers::none());
    draw(cx);
    assert_eq!(*installs.borrow(), 1);
    // The offer's gone, and the one toast there is says it's going.
    assert!(!has("toast-0".into(), cx));
    assert!(has("toast-1".into(), cx));
    assert!(!has("toast-2".into(), cx));
}

#[gpui::test]
fn closing_the_workbench_mid_fetch_drops_the_install(cx: &mut TestAppContext) {
    let _serial = serially();
    let dir = tempfile::tempdir().unwrap();
    let (installs, dropped) = give(dir.path(), cx);
    let (workbench, visual) = window(cx);

    open_both(&workbench, dir.path(), visual);
    let install = visual.debug_bounds("toast-choice-0-0").expect("Install");
    visual.simulate_click(install.center(), gpui::Modifiers::none());
    draw(visual);
    assert_eq!(*installs.borrow(), 1);
    assert!(!dropped.load(Ordering::SeqCst));

    visual.update(|window, _| window.remove_window());
    drop(workbench);
    visual.run_until_parked();

    assert!(
        dropped.load(Ordering::SeqCst),
        "the install stopped with it"
    );
}
