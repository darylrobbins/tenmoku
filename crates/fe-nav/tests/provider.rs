//! `Navigation`: routing each request to one provider, by precedence and
//! readiness, with a fake exact provider beside search.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use fe_buffer::Text;
use fe_nav::{
    BuildOptions, Definition, Definitions, Location, NavError, NavRequest, Navigation,
    NavigationProvider, OutlineKind, Precision, Readiness, SearchNavigation, SymbolIndex, Tier,
    WorkspaceSymbol, build,
};

/// A provider whose readiness and answers the test sets, counting calls.
struct Fake {
    readiness: Mutex<HashMap<Option<String>, Readiness>>,
    answer: Mutex<Result<Definitions, NavError>>,
    readiness_calls: AtomicUsize,
    calls: AtomicUsize,
}

impl Fake {
    fn new() -> Arc<Fake> {
        Arc::new(Fake {
            readiness: Mutex::new(HashMap::new()),
            answer: Mutex::new(Ok(Definitions::default())),
            readiness_calls: AtomicUsize::new(0),
            calls: AtomicUsize::new(0),
        })
    }

    fn set_ready(&self, language: Option<&str>, readiness: Readiness) {
        (self.readiness.lock().unwrap()).insert(language.map(str::to_owned), readiness);
    }

    fn answer(&self, answer: Result<Definitions, NavError>) {
        *self.answer.lock().unwrap() = answer;
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::Relaxed)
    }
}

impl NavigationProvider for Fake {
    fn name(&self) -> &str {
        "fake"
    }

    fn precision(&self) -> Precision {
        Precision::Exact
    }

    fn readiness(&self, language: Option<&str>) -> Readiness {
        self.readiness_calls.fetch_add(1, Ordering::Relaxed);
        let readiness = self.readiness.lock().unwrap();
        *readiness
            .get(&language.map(str::to_owned))
            .unwrap_or(&Readiness::NotYet)
    }

    fn definitions(
        &self,
        _req: &NavRequest,
        _cancel: &AtomicBool,
    ) -> Result<Definitions, NavError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.answer.lock().unwrap().clone()
    }

    fn workspace_symbols(
        &self,
        _query: &str,
        _limit: usize,
        _cancel: &AtomicBool,
    ) -> Result<Vec<WorkspaceSymbol>, NavError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        Ok(Vec::new())
    }
}

/// Search, counting its calls.
struct Spy {
    inner: SearchNavigation,
    calls: AtomicUsize,
}

impl Spy {
    fn calls(&self) -> usize {
        self.calls.load(Ordering::Relaxed)
    }
}

impl NavigationProvider for Spy {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn precision(&self) -> Precision {
        self.inner.precision()
    }

    fn readiness(&self, language: Option<&str>) -> Readiness {
        self.inner.readiness(language)
    }

    fn definitions(&self, req: &NavRequest, cancel: &AtomicBool) -> Result<Definitions, NavError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.inner.definitions(req, cancel)
    }

    fn workspace_symbols(
        &self,
        query: &str,
        limit: usize,
        cancel: &AtomicBool,
    ) -> Result<Vec<WorkspaceSymbol>, NavError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.inner.workspace_symbols(query, limit, cancel)
    }
}

/// A project with `helper` defined in `src/util.rs` and `util.py`, and
/// search over its complete index.
fn project() -> (tempfile::TempDir, Arc<Spy>) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/util.rs"), "pub fn helper() {}\n").unwrap();
    fs::write(root.join("util.py"), "def helper():\n    pass\n").unwrap();
    let mut index = SymbolIndex::new(root);
    build(
        root,
        &BuildOptions { threads: 1 },
        Arc::new(AtomicBool::new(false)),
        |batch| index.extend(batch),
    );
    index.finish();
    // A long budget, so a loaded machine can't leave work out.
    let search = SearchNavigation::new(root, Arc::new(RwLock::new(index)))
        .with_budget(Duration::from_secs(60));
    let spy = Arc::new(Spy {
        inner: search,
        calls: AtomicUsize::new(0),
    });
    (dir, spy)
}

/// A request with the caret on `helper` in a call.
fn request(root: &Path, rel: &str, language: &'static str) -> NavRequest {
    let text = "helper()\n";
    NavRequest {
        root: root.to_path_buf(),
        rel: PathBuf::from(rel),
        language: Some(language),
        text: Text::new(text),
        offset: 2,
        buffer: None,
    }
}

fn exact_answer() -> Definitions {
    Definitions {
        word: Some("helper".to_owned()),
        items: vec![Definition {
            location: Location {
                rel: PathBuf::from("src/util.rs"),
                range: 7..13,
                row: 0,
                snapshot: None,
            },
            name: "helper".to_owned(),
            kind: Some(OutlineKind::Function),
            container: None,
            tier: Tier::Project,
        }],
        unambiguous: true,
        partial: false,
    }
}

#[test]
fn exact_provider_takes_over_when_ready() {
    let (dir, search) = project();
    let fake = Fake::new();
    fake.answer(Ok(exact_answer()));
    let nav = Navigation::new();
    nav.add(search.clone(), 0);
    nav.add(fake.clone(), 10);
    assert_eq!(nav.providers(), ["fake", "search"]);
    let cancel = AtomicBool::new(false);
    let rust = request(dir.path(), "src/main.rs", "rust");

    // Not ready yet: search answers.
    let answer = nav.definitions(&rust, &cancel).unwrap();
    assert_eq!(answer.provider, "search");
    assert_eq!(answer.precision, Precision::Search);
    assert_eq!(answer.value.items[0].location.rel, Path::new("src/util.rs"));
    assert_eq!((fake.calls(), search.calls()), (0, 1));

    // Ready: it answers, and search isn't asked.
    fake.set_ready(Some("rust"), Readiness::Ready);
    let answer = nav.definitions(&rust, &cancel).unwrap();
    assert_eq!(answer.provider, "fake");
    assert_eq!(answer.precision, Precision::Exact);
    assert_eq!(answer.value, exact_answer());
    assert_eq!((fake.calls(), search.calls()), (1, 1));

    // Per request language: unsupported for python, so search answers.
    fake.set_ready(Some("python"), Readiness::Unsupported);
    let python = request(dir.path(), "main.py", "python");
    let answer = nav.definitions(&python, &cancel).unwrap();
    assert_eq!(answer.provider, "search");
    assert_eq!(answer.value.items[0].location.rel, Path::new("util.py"));
    assert_eq!((fake.calls(), search.calls()), (1, 2));
    assert_eq!(nav.definitions(&rust, &cancel).unwrap().provider, "fake");
    assert_eq!((fake.calls(), search.calls()), (2, 2));
}

#[test]
fn fallthrough_and_final_answers() {
    let (dir, search) = project();
    let fake = Fake::new();
    fake.set_ready(Some("rust"), Readiness::Ready);
    let nav = Navigation::new();
    nav.add(search.clone(), 0);
    nav.add(fake.clone(), 10);
    let cancel = AtomicBool::new(false);
    let req = request(dir.path(), "src/main.rs", "rust");

    // Unavailable falls through to search, as does Unsupported.
    fake.answer(Err(NavError::Unavailable("crashed".to_owned())));
    let answer = nav.definitions(&req, &cancel).unwrap();
    assert_eq!(answer.provider, "search");
    assert_eq!(answer.value.items[0].location.rel, Path::new("src/util.rs"));
    assert_eq!((fake.calls(), search.calls()), (1, 1));
    fake.answer(Err(NavError::Unsupported));
    assert_eq!(nav.definitions(&req, &cancel).unwrap().provider, "search");
    assert_eq!((fake.calls(), search.calls()), (2, 2));

    // An empty Ok is final.
    fake.answer(Ok(Definitions::default()));
    let answer = nav.definitions(&req, &cancel).unwrap();
    assert_eq!(answer.provider, "fake");
    assert_eq!(answer.value, Definitions::default());
    assert_eq!((fake.calls(), search.calls()), (3, 2));

    // So is Cancelled.
    fake.answer(Err(NavError::Cancelled));
    assert_eq!(nav.definitions(&req, &cancel), Err(NavError::Cancelled));
    assert_eq!((fake.calls(), search.calls()), (4, 2));

    // Removed after requests have run: search answers the next one; added
    // back, it does.
    fake.answer(Ok(exact_answer()));
    assert!(nav.remove("fake"));
    assert!(!nav.remove("fake"));
    assert_eq!(nav.definitions(&req, &cancel).unwrap().provider, "search");
    assert_eq!((fake.calls(), search.calls()), (4, 3));
    nav.add(fake.clone(), 10);
    assert_eq!(nav.definitions(&req, &cancel).unwrap().provider, "fake");
    assert_eq!((fake.calls(), search.calls()), (5, 3));
    // Added below search, which is always ready, it isn't asked.
    nav.add(fake.clone(), -1);
    assert_eq!(nav.providers(), ["search", "fake"]);
    assert_eq!(nav.definitions(&req, &cancel).unwrap().provider, "search");
    assert_eq!((fake.calls(), search.calls()), (5, 4));

    // Cancelled before the call: no provider is asked anything.
    nav.add(fake.clone(), 10);
    let readiness_calls = fake.readiness_calls.load(Ordering::Relaxed);
    cancel.store(true, Ordering::Relaxed);
    assert_eq!(nav.definitions(&req, &cancel), Err(NavError::Cancelled));
    assert_eq!(
        nav.workspace_symbols("hlp", 10, &cancel),
        Err(NavError::Cancelled)
    );
    assert_eq!((fake.calls(), search.calls()), (5, 4));
    assert_eq!(
        fake.readiness_calls.load(Ordering::Relaxed),
        readiness_calls
    );

    // With no provider ready, nothing answers.
    let empty = Navigation::new();
    assert_eq!(
        empty.definitions(&req, &AtomicBool::new(false)),
        Err(NavError::Unsupported)
    );
}

#[test]
fn providers_and_the_router_are_send_and_sync() {
    fn send_sync<T: Send + Sync + ?Sized>() {}
    send_sync::<dyn NavigationProvider>();
    send_sync::<Navigation>();
    send_sync::<SearchNavigation>();
}
