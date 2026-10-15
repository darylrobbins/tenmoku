//! Workspace trust in the workbench: the prompt on a folder's first open,
//! remembered in a trust file under a temporary state directory, and the
//! Trust and Untrust This Folder commands.

use std::{
    cell::RefCell,
    fs,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
    rc::Rc,
};

use fe_buffer::Buffer;
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_config::trust::{Trust, TrustStore, canonical};
use fe_gpui::{EditorSettings, EditorTheme, commands::RunCommand};
use gpui::{Entity, Focusable as _, Subscription, TestAppContext, VisualTestContext};

use super::{TrustChanged, set_trust_file};
use crate::{Launch, Workbench};

/// A workbench in a window of its own, the [`TrustChanged`]s it emits, and
/// the subscription that collects them.
struct Opened {
    workbench: Entity<Workbench>,
    cx: VisualTestContext,
    changes: Rc<RefCell<Vec<bool>>>,
    _changes: Subscription,
}

impl Opened {
    /// Opens `launch` without letting anything in the background land.
    fn new(launch: Launch, cx: &mut TestAppContext) -> Self {
        let window = cx.add_window(move |window, cx| {
            let theme = EditorTheme::default_dark(cx);
            Workbench::new(launch, theme, EditorSettings::default(), window, cx)
        });
        let workbench = window.root(cx).unwrap();
        let mut cx = VisualTestContext::from_window(*window, cx);
        let changes = Rc::new(RefCell::new(Vec::new()));
        let subscription = cx.update(|window, cx| {
            window.focus(&workbench.focus_handle(cx), cx);
            let changes = changes.clone();
            cx.subscribe(&workbench, move |_, TrustChanged(trusted), _| {
                changes.borrow_mut().push(*trusted)
            })
        });
        Self {
            workbench,
            cx,
            changes,
            _changes: subscription,
        }
    }

    fn folder(root: &Path, cx: &mut TestAppContext) -> Self {
        Self::new(
            Launch {
                root: Some(root.to_owned()),
                file: None,
            },
            cx,
        )
    }

    fn trusted(&mut self) -> bool {
        self.workbench
            .read_with(&self.cx, |workbench, _| workbench.root_trusted())
    }

    /// The `TrustChanged`s since the last call.
    fn changes(&self) -> Vec<bool> {
        self.changes.take()
    }

    fn set_root(&mut self, root: &Path) {
        let workbench = self.workbench.clone();
        self.cx.update(|window, cx| {
            workbench.update(cx, |workbench, cx| {
                workbench.set_root(root.to_owned(), window, cx)
            })
        });
    }

    /// Dispatches `command` as a key or menu item would.
    fn run(&mut self, command: &str) {
        let run = RunCommand(
            Invocation::of(command)
                .unwrap()
                .context(ContextStack::new().with(ContextFrame::new("workbench"))),
        );
        let workbench = self.workbench.clone();
        self.cx.update(|window, cx| {
            window.focus(&workbench.focus_handle(cx), cx);
            window.dispatch_action(Box::new(run), cx)
        });
    }

    fn close(mut self) {
        self.cx.update(|window, _| window.remove_window());
    }
}

fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
}

/// A trust file under a state directory not made yet, with `trusted`
/// already trusted in it.
fn trust_file(state: &tempfile::TempDir, trusted: &[&Path]) -> PathBuf {
    let file = state.path().join("state").join("trust.toml");
    let mut store = TrustStore::load(&file);
    for folder in trusted {
        store.set(&canonical(folder), true).unwrap();
    }
    file
}

/// A new folder `name` in `dir`, by its canonical path.
fn folder(dir: &tempfile::TempDir, name: &str) -> PathBuf {
    let path = canonical(dir.path()).join(name);
    fs::create_dir_all(&path).unwrap();
    path
}

/// The pending prompt's detail, or `None`.
fn prompt(cx: &TestAppContext) -> Option<String> {
    cx.pending_prompt().map(|(_, detail)| detail)
}

/// Answers the pending prompt with `button`, and lets that land.
fn answer(button: &str, cx: &mut TestAppContext) {
    cx.simulate_prompt_answer(button);
    cx.run_until_parked();
}

#[gpui::test]
fn first_open_prompts(cx: &mut TestAppContext) {
    let (state, projects) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let file = trust_file(&state, &[]);
    init(cx);
    cx.update(|cx| set_trust_file(cx, file.clone()));
    let a = folder(&projects, "a");
    fs::create_dir(a.join("sub")).unwrap();

    let mut opened = Opened::folder(&a, cx);
    assert!(!opened.trusted());
    cx.run_until_parked();
    let (question, detail) = cx.pending_prompt().expect("a prompt");
    assert_eq!(
        question,
        "Do you trust the authors of the files in this folder?"
    );
    assert!(detail.contains(&a.display().to_string()), "{detail}");
    assert!(!opened.trusted());
    answer("Trust Folder", cx);
    assert!(!cx.has_pending_prompt());
    assert_eq!(opened.changes(), vec![true]);
    assert!(opened.trusted());
    assert_eq!(TrustStore::load(&file).trust_of(&a), Trust::Trusted);

    // Remembered, for the folder and its subfolders.
    for root in [a.clone(), a.join("sub")] {
        let mut again = Opened::folder(&root, cx);
        cx.run_until_parked();
        assert!(!cx.has_pending_prompt(), "{root:?}");
        assert!(again.trusted(), "{root:?}");
    }

    let b = folder(&projects, "b");
    let mut opened = Opened::folder(&b, cx);
    cx.run_until_parked();
    assert!(prompt(cx).unwrap().contains(&b.display().to_string()));
    answer("Don't Trust", cx);
    assert!(!cx.has_pending_prompt());
    assert!(!opened.trusted());
    assert_eq!(opened.changes(), Vec::<bool>::new());
    assert_eq!(TrustStore::load(&file).trust_of(&b), Trust::Untrusted);
    let mut again = Opened::folder(&b, cx);
    cx.run_until_parked();
    assert!(!cx.has_pending_prompt());
    assert!(!again.trusted());

    // Two windows asking about one folder: the first answer stands, and
    // the other window's (Escape, say) changes nothing.
    let c = folder(&projects, "c");
    let mut first = Opened::folder(&c, cx);
    let mut second = Opened::folder(&c, cx);
    cx.run_until_parked();
    answer("Trust Folder", cx);
    assert!(first.trusted() && second.trusted());
    answer("Don't Trust", cx);
    assert!(!cx.has_pending_prompt());
    assert!(first.trusted() && second.trusted());
    assert_eq!(TrustStore::load(&file).trust_of(&c), Trust::Trusted);
}

#[gpui::test]
fn when_not_to_prompt(cx: &mut TestAppContext) {
    let (state, projects) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let undecided = folder(&projects, "undecided");
    let trusted = folder(&projects, "trusted");
    fs::create_dir(trusted.join("sub")).unwrap();
    let link = projects.path().join("link");
    symlink(&trusted, &link).unwrap();
    init(cx);

    // Without a trust file: nothing is asked, and nothing is trusted.
    let mut opened = Opened::folder(&undecided, cx);
    cx.run_until_parked();
    assert!(!cx.has_pending_prompt());
    assert!(!opened.trusted());
    opened.close();
    cx.run_until_parked();

    let file = trust_file(&state, &[&trusted]);
    cx.update(|cx| set_trust_file(cx, file));

    // A file's parent isn't asked about.
    let path = undecided.join("main.rs");
    let mut opened = Opened::new(
        Launch {
            root: Some(undecided.clone()),
            file: Some((path, Some(Buffer::new("fn main() {}\n").into()))),
        },
        cx,
    );
    cx.run_until_parked();
    assert!(!cx.has_pending_prompt());
    assert!(!opened.trusted());

    // Nor is no root.
    let mut opened = Opened::new(
        Launch {
            root: None,
            file: None,
        },
        cx,
    );
    cx.run_until_parked();
    assert!(!cx.has_pending_prompt());
    assert!(!opened.trusted());

    // Under a trusted folder, or through a link to one: trusted.
    for root in [trusted.join("sub"), link.clone()] {
        let mut opened = Opened::folder(&root, cx);
        assert!(!opened.trusted(), "untrusted until resolved");
        cx.run_until_parked();
        assert!(!cx.has_pending_prompt(), "{root:?}");
        assert!(opened.trusted(), "{root:?}");
    }
}

#[gpui::test]
fn open_folder_prompts_for_the_new_root(cx: &mut TestAppContext) {
    let (state, projects) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let home = folder(&projects, "home");
    let (a, b) = (folder(&projects, "a"), folder(&projects, "b"));
    let file = trust_file(&state, &[&home]);
    init(cx);
    cx.update(|cx| set_trust_file(cx, file.clone()));
    let mut opened = Opened::folder(&home, cx);
    cx.run_until_parked();
    assert!(!cx.has_pending_prompt());
    assert_eq!(opened.changes(), vec![true]);

    opened.set_root(&a);
    cx.run_until_parked();
    assert_eq!(opened.changes(), vec![false]);
    let detail = prompt(cx).expect("a prompt for A");
    assert!(detail.contains(&a.display().to_string()), "{detail}");

    // B while A's prompt shows: no second prompt.
    opened.set_root(&b);
    cx.run_until_parked();
    assert_eq!(prompt(cx), Some(detail));

    // The answer is A's; then B is asked about.
    cx.simulate_prompt_answer("Trust Folder");
    cx.run_until_parked();
    let saved = TrustStore::load(&file);
    assert_eq!(saved.trust_of(&a), Trust::Trusted);
    assert_eq!(saved.trust_of(&b), Trust::Undecided);
    assert!(!opened.trusted());
    assert_eq!(opened.changes(), Vec::<bool>::new());
    let detail = prompt(cx).expect("a prompt for B");
    assert!(detail.contains(&b.display().to_string()), "{detail}");
    answer("Don't Trust", cx);
    assert!(!cx.has_pending_prompt());

    // Trusted to untrusted and back.
    opened.set_root(&a);
    cx.run_until_parked();
    assert_eq!(opened.changes(), vec![true]);
    opened.set_root(&b);
    cx.run_until_parked();
    assert_eq!(opened.changes(), vec![false]);
    opened.set_root(&a);
    cx.run_until_parked();
    assert_eq!(opened.changes(), vec![true]);
    assert!(!cx.has_pending_prompt());
}

#[gpui::test]
fn commands_change_trust(cx: &mut TestAppContext) {
    let (state, projects) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let root = folder(&projects, "root");
    let file = trust_file(&state, &[&root]);
    init(cx);
    cx.update(|cx| set_trust_file(cx, file.clone()));
    let mut first = Opened::folder(&root, cx);
    let second = Opened::folder(&root, cx);
    cx.run_until_parked();
    assert_eq!(first.changes(), vec![true]);
    assert_eq!(second.changes(), vec![true]);

    first.run("workbench.untrust_folder");
    cx.run_until_parked();
    assert!(!first.trusted());
    assert_eq!(first.changes(), vec![false]);
    assert_eq!(second.changes(), vec![false]);
    assert_eq!(TrustStore::load(&file).trust_of(&root), Trust::Untrusted);

    // Another fastedit's decision meanwhile is kept.
    let elsewhere = folder(&projects, "elsewhere");
    TrustStore::load(&file).set(&elsewhere, true).unwrap();
    first.run("workbench.trust_folder");
    cx.run_until_parked();
    assert_eq!(TrustStore::load(&file).trust_of(&elsewhere), Trust::Trusted);
    assert!(first.trusted());
    assert_eq!(first.changes(), vec![true]);
    assert_eq!(second.changes(), vec![true]);
    assert_eq!(TrustStore::load(&file).trust_of(&root), Trust::Trusted);

    // With no root, they change nothing.
    let saved = fs::read(&file).unwrap();
    let mut rootless = Opened::new(
        Launch {
            root: None,
            file: None,
        },
        cx,
    );
    cx.run_until_parked();
    for command in ["workbench.trust_folder", "workbench.untrust_folder"] {
        rootless.run(command);
        cx.run_until_parked();
        assert!(!rootless.trusted());
        assert_eq!(rootless.changes(), Vec::<bool>::new());
        assert_eq!(first.changes(), Vec::<bool>::new());
        assert_eq!(fs::read(&file).unwrap(), saved);
    }
    for opened in [first, second, rootless] {
        opened.close();
    }
    cx.run_until_parked();

    // A trust file that can't be written: trusted for the session.
    let blocker = state.path().join("blocker");
    fs::write(&blocker, "").unwrap();
    cx.update(|cx| set_trust_file(cx, blocker.join("trust.toml")));
    let other = folder(&projects, "other");
    let mut opened = Opened::new(
        Launch {
            root: Some(other.clone()),
            file: Some((other.join("a.txt"), Some(Buffer::new("a\n").into()))),
        },
        cx,
    );
    cx.run_until_parked();
    assert!(!opened.trusted());
    opened.run("workbench.trust_folder");
    cx.run_until_parked();
    assert!(opened.trusted());
    assert_eq!(opened.changes(), vec![true]);
    assert!(blocker.is_file());
}

#[gpui::test]
fn an_ancestors_decision_leaves_a_prompt_open(cx: &mut TestAppContext) {
    let (state, projects) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let parent = folder(&projects, "parent");
    let child = folder(&projects, "parent/child");
    let file = trust_file(&state, &[]);
    init(cx);
    cx.update(|cx| set_trust_file(cx, file.clone()));

    // Asked about the child; meanwhile its parent is trusted in another
    // window (a file launch there, so that one isn't asked).
    let mut asked = Opened::folder(&child, cx);
    let mut other = Opened::new(
        Launch {
            root: Some(parent.clone()),
            file: Some((parent.join("a.txt"), Some(Buffer::new("a\n").into()))),
        },
        cx,
    );
    cx.run_until_parked();
    assert!(prompt(cx).unwrap().contains(&child.display().to_string()));
    other.run("workbench.trust_folder");
    cx.run_until_parked();
    assert!(other.trusted() && asked.trusted());

    // The child's own answer still counts.
    answer("Don't Trust", cx);
    assert!(!asked.trusted());
    assert_eq!(asked.changes(), vec![true, false]);
    let saved = TrustStore::load(&file);
    assert_eq!(saved.trust_of(&parent), Trust::Trusted);
    assert_eq!(saved.trust_of(&child), Trust::Untrusted);
}

#[gpui::test]
fn writes_keep_other_decisions(cx: &mut TestAppContext) {
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt as _};

    let (state, projects) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (a, b, c) = (
        folder(&projects, "a"),
        folder(&projects, "b"),
        folder(&projects, "c"),
    );
    let file = trust_file(&state, &[]);
    init(cx);
    cx.update(|cx| set_trust_file(cx, file.clone()));

    // Trusted and written here, then untrusted by another fastedit: an
    // unrelated decision here keeps that.
    cx.update(|cx| super::decide(&a, true, cx));
    cx.run_until_parked();
    assert_eq!(TrustStore::load(&file).trust_of(&a), Trust::Trusted);
    TrustStore::load(&file).set(&a, false).unwrap();
    cx.update(|cx| super::decide(&b, true, cx));
    cx.run_until_parked();
    let saved = TrustStore::load(&file);
    assert_eq!(saved.trust_of(&a), Trust::Untrusted);
    assert_eq!(saved.trust_of(&b), Trust::Trusted);

    // A folder whose path isn't UTF-8, decided with another before the
    // writer runs, doesn't keep the other from being saved.
    let not_utf8 = projects.path().join(OsStr::from_bytes(b"\xff"));
    cx.update(|cx| {
        super::decide(&c, false, cx);
        super::decide(&not_utf8, true, cx);
    });
    cx.run_until_parked();
    let saved = TrustStore::load(&file);
    assert_eq!(saved.trust_of(&c), Trust::Untrusted);
    assert_eq!(saved.trust_of(&not_utf8), Trust::Undecided);
}
