// SPDX-License-Identifier: LicenseRef-Proprietary

//! A host's root folder's trust: untrusted until resolved, the host's
//! prompt the first time a folder is opened as one, the answer written and
//! remembered, a decision made before the root resolves applied once it
//! does, and nothing asked without a trust file. Generalised from the
//! editor's workspace trust tests.

use std::{
    cell::RefCell,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    sync::atomic::{AtomicUsize, Ordering},
};

use gpui::{
    Context, EventEmitter, IntoElement, Render, TestAppContext, VisualTestContext, Window, div,
};
use kiln_core::trust::{Trust, TrustStore, canonical};
use kiln_shell::trust::{self, RootTrust, TrustChanged, TrustHost, TrustPrompt, set_trust_file};

/// A view with a root folder.
struct Host {
    trust: RootTrust,
}

impl EventEmitter<TrustChanged> for Host {}

impl TrustHost for Host {
    fn root_trust(&mut self) -> &mut RootTrust {
        &mut self.trust
    }
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// A fresh directory under the system's temporary one, removed when
/// dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl std::ops::Deref for Scratch {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

fn scratch(name: &str) -> Scratch {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "kiln-trust-{}-{}-{name}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    Scratch(canonical(&dir))
}

struct Opened {
    host: gpui::Entity<Host>,
    cx: VisualTestContext,
    changes: Rc<RefCell<Vec<bool>>>,
}

impl Opened {
    /// A host whose root is `root`, opened as a folder if `as_folder`.
    fn new(root: Option<&Path>, as_folder: bool, cx: &mut TestAppContext) -> Self {
        let root = root.map(Path::to_owned);
        let window = cx.add_window(move |window, cx| {
            let prompt = TrustPrompt::new("Trust it?", |folder| {
                format!("{} may run code.", folder.display())
            });
            let mut host = Host {
                trust: RootTrust::new(prompt),
            };
            RootTrust::follow(&mut host, window, cx);
            RootTrust::resolve(&mut host, root, as_folder, window, cx);
            host
        });
        let host = window.root(cx).unwrap();
        let mut cx = VisualTestContext::from_window(*window, cx);
        let changes = Rc::new(RefCell::new(Vec::new()));
        let sink = changes.clone();
        cx.update(|_, cx| {
            cx.subscribe(&host, move |_, TrustChanged(trusted), _| {
                sink.borrow_mut().push(*trusted)
            })
            .detach()
        });
        Self { host, cx, changes }
    }

    fn trusted(&mut self) -> bool {
        self.host
            .read_with(&self.cx, |host, _| host.trust.trusted())
    }

    fn set_trusted(&mut self, trusted: bool) {
        self.host.update(&mut self.cx, |host, cx| {
            RootTrust::set_trusted(host, trusted, cx)
        });
    }
}

fn init(cx: &mut TestAppContext) {
    cx.update(trust::init);
}

#[gpui::test]
fn the_first_open_asks_in_the_hosts_words(cx: &mut TestAppContext) {
    init(cx);
    let (state, projects) = (scratch("state"), scratch("projects"));
    let file = state.join("trust.toml");
    cx.update(|cx| set_trust_file(cx, file.clone()));
    let a = projects.join("a");
    fs::create_dir_all(a.join("sub")).unwrap();

    let mut opened = Opened::new(Some(&a), true, cx);
    assert!(!opened.trusted(), "untrusted until resolved");
    cx.run_until_parked();
    let (question, detail) = cx.pending_prompt().expect("a prompt");
    assert_eq!(question, "Trust it?");
    assert_eq!(detail, format!("{} may run code.", a.display()));
    cx.simulate_prompt_answer("Trust Folder");
    cx.run_until_parked();
    assert!(opened.trusted());
    assert_eq!(*opened.changes.borrow(), [true]);
    assert_eq!(TrustStore::load(&file).trust_of(&a), Trust::Trusted);

    // Remembered, for its subfolders too, and not asked again.
    let mut sub = Opened::new(Some(&a.join("sub")), true, cx);
    cx.run_until_parked();
    assert!(!cx.has_pending_prompt());
    assert!(sub.trusted());

    // Opened as a file's parent, an undecided folder isn't asked about.
    let b = projects.join("b");
    fs::create_dir_all(&b).unwrap();
    let mut parent = Opened::new(Some(&b), false, cx);
    cx.run_until_parked();
    assert!(!cx.has_pending_prompt());
    assert!(!parent.trusted());

    // Escape is "Don't Trust", remembered.
    let mut folder = Opened::new(Some(&b), true, cx);
    cx.run_until_parked();
    cx.simulate_prompt_answer("Don't Trust");
    cx.run_until_parked();
    assert!(!folder.trusted());
    assert_eq!(TrustStore::load(&file).trust_of(&b), Trust::Untrusted);
}

#[gpui::test]
fn a_decision_before_the_root_resolves_applies_once_it_does(cx: &mut TestAppContext) {
    init(cx);
    let (state, projects) = (scratch("state"), scratch("projects"));
    let file = state.join("trust.toml");
    cx.update(|cx| set_trust_file(cx, file.clone()));
    let a = projects.join("a");
    fs::create_dir_all(&a).unwrap();

    let mut opened = Opened::new(Some(&a), false, cx);
    opened.set_trusted(true);
    assert!(!opened.trusted());
    cx.run_until_parked();
    assert!(opened.trusted());
    assert_eq!(*opened.changes.borrow(), [true]);
    opened.set_trusted(false);
    assert!(!opened.trusted());
    cx.run_until_parked();
    assert_eq!(TrustStore::load(&file).trust_of(&a), Trust::Untrusted);

    // With no root, nothing to decide.
    let mut rootless = Opened::new(None, true, cx);
    rootless.set_trusted(true);
    cx.run_until_parked();
    assert!(!rootless.trusted());
}

#[gpui::test]
fn without_a_trust_file_nothing_is_asked(cx: &mut TestAppContext) {
    init(cx);
    let projects = scratch("projects");
    let mut opened = Opened::new(Some(&projects), true, cx);
    cx.run_until_parked();
    assert!(!cx.has_pending_prompt());
    assert!(!opened.trusted());
    // Decisions hold for the session.
    opened.set_trusted(true);
    cx.run_until_parked();
    assert!(opened.trusted());
}
