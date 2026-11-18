//! A Run Test lens above each test, as rust-analyzer's in VS Code: a
//! [`LensProvider`] per editor, giving `▷ Run test` over each test function
//! and `▷ Run tests` over each test module or class
//! (`docs/design/design-system.md#editor`; the editor sets lens rows in UI
//! small sans), whose click runs `tests.run_row` ([`crate::test_commands`])
//! for that row, wherever the caret is.
//!
//! The provider is asked each frame for a snapshot but has no tree, so it
//! shows what [`sync`] read off the editor's parse: the rows, anchored to
//! the text, so an edit moves them at once and a new parse corrects them.
//! What it read is of a version and a language: a parse in another language
//! of the same text (the editor's language changed) is read again, and an
//! editor in no language with runnables shows none.
//! After Run Test, a plain-text segment says how the test did in its last
//! runs, from the root's test history (`fe-runlog`): "passed in the last 8
//! runs", "failed in 3 of the last 8 runs", or nothing for a test with no
//! results (or "flaky", in the warning colour, one that failed before it
//! last passed), matched to the test by its crate, binary and module path
//! as its file says them. The history is read again when a test run's results are saved.
//! The editors that get one are the tabs' showing a file inside a trusted
//! root, in a language with runnables, while `editor.code_lens` is on.

use std::{
    cell::RefCell,
    collections::HashMap,
    path::{Path, PathBuf},
    rc::Rc,
};

use fe_buffer::{Anchor, BufferSnapshot, Version};
use fe_command::{Invocation, Value};
use fe_gpui::{EditorView, Lens, LensItem, LensProvider};
use fe_runlog::{History, Outcome};
use gpui::{App, Context, Entity, EntityId, SharedString, Subscription, WeakEntity};
use kiln_l10n::{tr, verbatim};

use crate::{
    Workbench,
    test_runnables::{RunnableKind, TEST_LANGUAGES, runnables},
    test_runner::{HistorySaved, history_dir},
};

/// What a click on a test's lens runs.
const RUN_ROW: &str = "tests.run_row";

/// One lens row: where it sits, what it runs, and for a test the name the
/// history records it under.
type Row = (Anchor, RunnableKind, Option<String>);

/// One target of the root's packages that has tests: the id of the binary
/// its tests run in, as nextest names it and the runner records it
/// ([`CargoTarget::id`](crate::test_runner::CargoTarget): `p`, `p::bin/p`,
/// `p::more`), and its root source file.
#[derive(Clone, Debug, PartialEq)]
struct TestTarget {
    binary: String,
    root: PathBuf,
}

/// The targets `cargo metadata` lists, and which root and read they are of.
#[derive(Default)]
struct Targets {
    list: Vec<TestTarget>,
    /// Changes whenever the list or the root it is of does, so what was
    /// worked out from it is worked out again.
    generation: u64,
}

/// The test targets in `metadata`, the JSON of `cargo metadata
/// --no-deps --format-version 1`: each package's library, binaries and
/// integration tests, as the runner names their binaries.
fn parse_targets(metadata: &str) -> Vec<TestTarget> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(metadata) else {
        return Vec::new();
    };
    let packages = value.get("packages").and_then(|p| p.as_array());
    let mut out = Vec::new();
    for package in packages.into_iter().flatten() {
        let Some(name) = package.get("name").and_then(|n| n.as_str()) else {
            continue;
        };
        let targets = package.get("targets").and_then(|t| t.as_array());
        for target in targets.into_iter().flatten() {
            let (Some(target_name), Some(src)) = (
                target.get("name").and_then(|n| n.as_str()),
                target.get("src_path").and_then(|n| n.as_str()),
            ) else {
                continue;
            };
            let kinds: Vec<&str> = (target.get("kind").and_then(|k| k.as_array()))
                .into_iter()
                .flatten()
                .filter_map(|k| k.as_str())
                .collect();
            let has = |wanted: &[&str]| kinds.iter().any(|kind| wanted.contains(kind));
            let binary = if has(&["lib", "rlib", "dylib", "cdylib", "staticlib", "proc-macro"]) {
                name.to_owned()
            } else if has(&["bin"]) {
                format!("{name}{}{target_name}", verbatim!("::bin/"))
            } else if has(&["test"]) {
                format!("{name}::{target_name}")
            } else {
                continue;
            };
            out.push(TestTarget {
                binary,
                root: PathBuf::from(src),
            });
        }
    }
    out
}

/// `cargo metadata` of the packages of `root`, blocking: none if it can't
/// be run.
fn read_targets(root: &Path) -> Vec<TestTarget> {
    let output = std::process::Command::new(verbatim!("cargo"))
        .args([
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--offline",
        ])
        .current_dir(root)
        .stdin(std::process::Stdio::null())
        .output();
    match output {
        Ok(output) if output.status.success() => {
            parse_targets(&String::from_utf8_lossy(&output.stdout))
        }
        _ => Vec::new(),
    }
}

/// Where a test's file puts it in a run's ids: the binary it is in, and the
/// module path to its selector.
struct Identity {
    binary: String,
    module: String,
}

/// The folder the modules of the target rooted at `root` are in: beside a
/// `lib.rs`, `main.rs` or `mod.rs`, in a folder of the file's name beside
/// any other root (`src/bin/tool.rs`'s is `src/bin/tool`).
fn module_dir(root: &Path) -> Option<PathBuf> {
    let stem = root.file_stem()?.to_str()?;
    match stem {
        "lib" | "main" | "mod" => root.parent().map(Path::to_owned),
        _ => Some(root.with_extension("")),
    }
}

/// `path` with symlinks resolved (cargo reports `/private/var/...` for a
/// `/var/...` temp root), else as given.
fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned())
}

/// The binary `file` is built into and the module path it has there:
/// `file` is a target's root, or in the folder of the modules of exactly
/// one target (the deepest folder; a root belongs to its own target alone).
/// A file two targets could own, as `src/lib.rs`'s and `src/main.rs`'s
/// modules both are, has no identity, nor does one no target owns, rather
/// than another test's results. `#[path]` modules are found where they are,
/// not where their `mod` says.
fn identity_in(targets: &[TestTarget], file: &Path) -> Option<Identity> {
    let file = canonical(file);
    let file = file.as_path();
    let roots: Vec<PathBuf> = targets
        .iter()
        .map(|target| canonical(&target.root))
        .collect();
    if let Some((target, _)) = (targets.iter().zip(&roots)).find(|(_, root)| root.as_path() == file)
    {
        return Some(Identity {
            binary: target.binary.clone(),
            module: String::new(),
        });
    }
    let owned: Vec<(&TestTarget, PathBuf)> = (targets.iter().zip(&roots))
        .filter_map(|(target, root)| Some((target, module_dir(root)?)))
        .filter(|(_, dir)| file.starts_with(dir))
        .collect();
    let deepest = owned
        .iter()
        .map(|(_, dir)| dir.components().count())
        .max()?;
    let mut nearest = owned
        .iter()
        .filter(|(_, dir)| dir.components().count() == deepest);
    let (target, dir) = nearest.next()?;
    if nearest.any(|(other, _)| other.binary != target.binary) {
        return None;
    }
    let relative = file.strip_prefix(dir).ok()?.with_extension("");
    let mut parts: Vec<String> = relative
        .iter()
        .map(|part| part.to_string_lossy().into_owned())
        .collect();
    if parts.last().is_some_and(|last| last == "mod") {
        parts.pop();
    }
    Some(Identity {
        binary: target.binary.clone(),
        module: parts.join("::"),
    })
}

/// A window's test history, shared by its providers.
type Shared = Rc<RefCell<History>>;

/// What a provider shows: the rows of the parse at `read`'s version, in its
/// language.
struct Shown {
    read: Option<(Version, &'static str, Option<PathBuf>, u64)>,
    rows: Vec<Row>,
    /// The binary the rows' tests are in, if the targets say.
    binary: Option<String>,
    history: Shared,
}

impl Shown {
    fn new(history: Shared) -> Self {
        Self {
            read: None,
            rows: Vec::new(),
            binary: None,
            history,
        }
    }

    /// What the test called `name` did in its last runs, if it has run, and
    /// whether that is to be warned of.
    fn results(&self, name: &str) -> Option<(SharedString, bool)> {
        let history = self.history.borrow();
        let binary = self.binary.clone()?;
        let id = history.find(name, std::slice::from_ref(&binary))?;
        let window = history.last8(id);
        let failed = window.iter().filter(|o| **o == Outcome::Failed).count();
        let count = window.len();
        // Flaky: it has failed and its last run passed.
        if failed > 0 && window.last() == Some(&Outcome::Passed) {
            return Some((tr!("workbench-lens-flaky"), true));
        }
        let text = match (count, failed) {
            (0, _) => return None,
            (_, 0) => tr!("workbench-lens-passed", count = count),
            (count, failed) if failed == count => tr!("workbench-lens-failed-all", count = count),
            (count, failed) => tr!("workbench-lens-failed-some", failed = failed, count = count),
        };
        Some((text, false))
    }
}

/// What the lens over a runnable of `kind` says.
fn title(kind: RunnableKind) -> SharedString {
    match kind {
        RunnableKind::Module => tr!("workbench-lens-run-tests"),
        RunnableKind::Test | RunnableKind::File => tr!("workbench-lens-run-test"),
    }
}

impl LensProvider for Shown {
    fn lenses(&mut self, snapshot: &BufferSnapshot) -> Vec<Lens> {
        let text = snapshot.text();
        (self.rows.iter())
            .map(|(anchor, kind, path)| {
                let row = text.offset_to_point(snapshot.resolve(anchor)).row;
                let mut items = vec![LensItem {
                    title: title(*kind),
                    warning: false,
                    invocation: Some(click(row)),
                }];
                let results = path.as_deref().and_then(|path| self.results(path));
                items.extend(results.map(|(title, warning)| LensItem {
                    title,
                    warning,
                    invocation: None,
                }));
                Lens { row, items }
            })
            .collect()
    }
}

/// The click on the lens over `row`.
fn click(row: u32) -> Invocation {
    Invocation::of(RUN_ROW)
        .expect("tests.run_row is registered")
        .args(Value::Object(
            [("row".to_owned(), row.into())].into_iter().collect(),
        ))
}

/// An editor's provider.
struct Slot {
    editor: WeakEntity<EditorView>,
    shown: Rc<RefCell<Shown>>,
    provider: Rc<RefCell<dyn LensProvider>>,
    /// Whether the editor has the provider added.
    added: bool,
    _observed: Subscription,
}

impl State {
    fn new() -> Self {
        Self {
            slots: Slots::default(),
            history: Rc::new(RefCell::new(History::in_memory())),
            loaded: Rc::default(),
            reads: Rc::default(),
            targets: Rc::default(),
        }
    }
}

/// A window's providers, by editor.
type Slots = Rc<RefCell<HashMap<EntityId, Slot>>>;

/// What a window's lens sync works with.
#[derive(Clone)]
struct State {
    slots: Slots,
    history: Shared,
    /// The root `history` was read for.
    loaded: Rc<RefCell<Option<std::path::PathBuf>>>,
    /// How many reads have been started: only the latest one's result is
    /// kept, so a slow older read can't put back what a newer one replaced.
    reads: Rc<std::cell::Cell<u64>>,
    /// The root's test targets.
    targets: Rc<RefCell<Targets>>,
}

pub(crate) fn init(cx: &mut App) {
    cx.observe_new(|_: &mut Workbench, window, cx| {
        if window.is_none() {
            return;
        }
        let state = State::new();
        cx.observe_self({
            let state = state.clone();
            move |workbench, cx| sync(workbench, &state, cx)
        })
        .detach();
        // A run's results were saved: the lenses read them.
        cx.observe_global::<HistorySaved>({
            let state = state.clone();
            move |workbench, cx| load_history(workbench, &state, cx)
        })
        .detach();
    })
    .detach();
}

/// Brings the providers up to date with the tabs' editors, the root's trust,
/// the setting and each parse. Quiet when nothing changed.
fn sync(workbench: &mut Workbench, again: &State, cx: &mut Context<Workbench>) {
    let root = workbench.root().map(std::path::Path::to_owned);
    if *again.loaded.borrow() != root {
        load_history(workbench, again, cx);
        load_targets(workbench, again, cx);
    }
    let on = workbench.lsp.code_lens_on.get() && workbench.root_trusted();
    let editors = match (&root, on) {
        (Some(_), true) => workbench.open_editors(cx),
        _ => Vec::new(),
    };
    let Ok(mut slots) = again.slots.try_borrow_mut() else {
        return;
    };
    let open: Vec<EntityId> = editors.iter().map(Entity::entity_id).collect();
    let gone: Vec<EntityId> = (slots.keys().copied())
        .filter(|id| !open.contains(id))
        .collect();
    for id in gone {
        let slot = slots.remove(&id).expect("just listed");
        if let Some(editor) = slot.editor.upgrade() {
            editor.update(cx, |view, cx| {
                view.remove_lens_provider(&slot.provider);
                cx.notify();
            });
        }
    }
    for editor in editors {
        let slot = slots.entry(editor.entity_id()).or_insert_with(|| {
            let shown = Rc::new(RefCell::new(Shown::new(again.history.clone())));
            // The editor changed: its parse may be new.
            let observed = cx.observe(&editor, {
                let state = again.clone();
                move |workbench, _, cx| sync(workbench, &state, cx)
            });
            Slot {
                editor: editor.downgrade(),
                provider: shown.clone(),
                shown,
                added: false,
                _observed: observed,
            }
        });
        let inside = {
            let view = editor.read(cx);
            let path = view.model().path();
            matches!((path, &root), (Some(path), Some(root)) if path.starts_with(root))
        };
        let wanted = inside && read_rows(&editor, &again.targets, &slot.shown, cx).is_some();
        if wanted != slot.added {
            slot.added = wanted;
            let provider = slot.provider.clone();
            editor.update(cx, |view, cx| {
                if wanted {
                    view.add_lens_provider(provider);
                } else {
                    view.remove_lens_provider(&provider);
                }
                cx.notify();
            });
        }
    }
}

/// Reads the root's test targets off the main thread, then has the editors
/// work out their tests' identities from them.
fn load_targets(workbench: &mut Workbench, state: &State, cx: &mut Context<Workbench>) {
    let root = workbench.root().map(Path::to_owned);
    let asked = {
        let mut targets = state.targets.borrow_mut();
        targets.list.clear();
        targets.generation += 1;
        targets.generation
    };
    let Some(root) = root else {
        return;
    };
    let state = state.clone();
    cx.spawn(async move |workbench, cx| {
        let listed = {
            let root = root.clone();
            cx.background_executor()
                .spawn(async move { read_targets(&root) })
                .await
        };
        workbench
            .update(cx, |workbench, cx| {
                // The root changed while they were read.
                if state.targets.borrow().generation != asked
                    || workbench.root() != Some(root.as_path())
                {
                    return;
                }
                {
                    let mut targets = state.targets.borrow_mut();
                    targets.list = listed;
                    targets.generation += 1;
                }
                cx.notify();
            })
            .ok();
    })
    .detach();
}

/// Reads the root's test history off the main thread, then has the editors
/// show it. A root without a state directory has none.
fn load_history(workbench: &mut Workbench, state: &State, cx: &mut Context<Workbench>) {
    let root = workbench.root().map(std::path::Path::to_owned);
    *state.loaded.borrow_mut() = root.clone();
    let read_id = state.reads.get() + 1;
    state.reads.set(read_id);
    let Some(root) = root else {
        *state.history.borrow_mut() = History::in_memory();
        return;
    };
    let dir = history_dir(cx);
    let state = state.clone();
    cx.spawn(async move |workbench, cx| {
        let read = {
            let root = root.clone();
            cx.background_executor()
                .spawn(async move {
                    match dir {
                        Some(dir) => History::load(&dir, &root),
                        None => History::in_memory(),
                    }
                })
                .await
        };
        workbench
            .update(cx, |workbench, cx| {
                // A newer read was started while this one was reading.
                if state.reads.get() != read_id || workbench.root() != Some(root.as_path()) {
                    return;
                }
                *state.history.borrow_mut() = read;
                let editors: Vec<_> = (state.slots.borrow().values())
                    .filter_map(|slot| slot.editor.upgrade())
                    .collect();
                for editor in editors {
                    editor.update(cx, |_, cx| cx.notify());
                }
                cx.notify();
            })
            .ok();
    })
    .detach();
}

/// Reads `editor`'s parse into `shown` if it is new, and whether the editor
/// is in a language with runnables. One that isn't, or has no parse, empties
/// `shown`, so no rows of a language it was in show if it gets one again.
fn read_rows(
    editor: &Entity<EditorView>,
    targets: &Rc<RefCell<Targets>>,
    shown: &Rc<RefCell<Shown>>,
    cx: &mut Context<Workbench>,
) -> Option<()> {
    let view = editor.read(cx);
    let syntax = view.syntax();
    let language = syntax
        .map(|syntax| syntax.language().name())
        .filter(|language| TEST_LANGUAGES.contains(language));
    let (Some(syntax), Some(language)) = (syntax, language) else {
        let mut shown = shown.borrow_mut();
        shown.read = None;
        shown.rows.clear();
        shown.binary = None;
        return None;
    };
    let snapshot = view.model().snapshot();
    let current =
        syntax.version() == snapshot.version() && !syntax.needs_parse() && !syntax.failed();
    // The file's path and the root's targets say its tests' identity: a
    // rename without an edit, or other targets, read again.
    let targets = targets.borrow();
    let read = (
        snapshot.version(),
        language,
        view.model().path().map(Path::to_owned),
        targets.generation,
    );
    if !current || shown.borrow().read.as_ref() == Some(&read) {
        return Some(());
    }
    let text = snapshot.text();
    let found = (view.model().path())
        .filter(|_| language == "rust")
        .and_then(|path| identity_in(&targets.list, path));
    let (binary, module) = match found {
        Some(Identity { binary, module }) => (Some(binary), module),
        None => (None, String::new()),
    };
    let rows = (runnables(language, syntax, text).into_iter())
        .filter(|runnable| runnable.kind != RunnableKind::File)
        .map(|runnable| {
            let row = runnable.rows.start.min(text.row_count().saturating_sub(1));
            let path =
                (runnable.kind == RunnableKind::Test && runnable.exact).then(|| {
                    match module.is_empty() {
                        true => runnable.selector.clone(),
                        false => format!("{module}::{}", runnable.selector),
                    }
                });
            (
                snapshot.anchor_before(text.row_start(row)),
                runnable.kind,
                path,
            )
        })
        .collect();
    let mut shown = shown.borrow_mut();
    shown.read = Some(read);
    shown.rows = rows;
    shown.binary = binary;
    drop(shown);
    editor.update(cx, |_, cx| cx.notify());
    Some(())
}

#[cfg(test)]
mod identity_tests {
    use std::path::{Path, PathBuf};

    use super::{TestTarget, identity_in, parse_targets};

    fn target(binary: &str, root: &str) -> TestTarget {
        TestTarget {
            binary: binary.to_owned(),
            root: PathBuf::from(root),
        }
    }

    fn id(targets: &[TestTarget], file: &str) -> Option<(String, String)> {
        identity_in(targets, Path::new(file)).map(|id| (id.binary, id.module))
    }

    fn some(binary: &str, module: &str) -> Option<(String, String)> {
        Some((binary.to_owned(), module.to_owned()))
    }

    /// A package `p` of a library, a binary and an integration test; a
    /// second package `q` with a library and an integration test of the
    /// same name.
    fn workspace() -> Vec<TestTarget> {
        vec![
            target("p", "/w/p/src/lib.rs"),
            target("p::bin/p", "/w/p/src/main.rs"),
            target("p::bin/tool", "/w/p/src/bin/tool.rs"),
            target("p::api", "/w/p/tests/api.rs"),
            target("p::nested", "/w/p/tests/nested/main.rs"),
            target("p::custom", "/w/p/ci/custom.rs"),
            target("q", "/w/q/src/lib.rs"),
            target("q::api", "/w/q/tests/api.rs"),
        ]
    }

    #[test]
    fn a_target_root_is_its_own() {
        let targets = workspace();
        assert_eq!(id(&targets, "/w/p/src/lib.rs"), some("p", ""));
        assert_eq!(id(&targets, "/w/p/src/main.rs"), some("p::bin/p", ""));
        assert_eq!(
            id(&targets, "/w/p/src/bin/tool.rs"),
            some("p::bin/tool", "")
        );
        assert_eq!(id(&targets, "/w/p/tests/api.rs"), some("p::api", ""));
        assert_eq!(id(&targets, "/w/q/tests/api.rs"), some("q::api", ""));
        assert_eq!(id(&targets, "/w/p/ci/custom.rs"), some("p::custom", ""));
    }

    #[test]
    fn modules_are_under_their_target() {
        let mut targets = workspace();
        // Both `src/lib.rs`'s and `src/main.rs`'s modules are in `src/`:
        // which one a file is in isn't known.
        assert_eq!(id(&targets, "/w/p/src/shapes.rs"), None);
        targets.remove(1);
        assert_eq!(id(&targets, "/w/p/src/shapes.rs"), some("p", "shapes"));
        assert_eq!(id(&targets, "/w/p/src/a/mod.rs"), some("p", "a"));
        assert_eq!(id(&targets, "/w/p/src/a/b.rs"), some("p", "a::b"));
        // Only the crate root is a library's; a nested `lib.rs` is a module.
        assert_eq!(id(&targets, "/w/p/src/foo/lib.rs"), some("p", "foo::lib"));
        // The deepest target's folder owns: a binary's modules aren't the
        // library's.
        assert_eq!(
            id(&targets, "/w/p/src/bin/tool/util.rs"),
            some("p::bin/tool", "util")
        );
        assert_eq!(
            id(&targets, "/w/p/tests/nested/helpers.rs"),
            some("p::nested", "helpers")
        );
        assert_eq!(
            id(&targets, "/w/p/ci/custom/inner.rs"),
            some("p::custom", "inner")
        );
    }

    #[test]
    fn a_file_no_target_owns_has_none() {
        let targets = workspace();
        assert_eq!(id(&targets, "/w/p/tests/helpers.rs"), None);
        assert_eq!(id(&targets, "/w/p/benches/b.rs"), None);
        assert_eq!(id(&targets, "/w/other/src/lib.rs"), None);
        assert_eq!(id(&[], "/w/p/src/lib.rs"), None);
    }

    #[test]
    fn cargo_metadata_is_read_into_runner_binary_names() {
        let json = r#"{"packages":[{"name":"p","targets":[
            {"kind":["lib"],"name":"p_lib","src_path":"/w/p/src/lib.rs"},
            {"kind":["bin"],"name":"tool","src_path":"/w/p/src/bin/tool.rs"},
            {"kind":["test"],"name":"api","src_path":"/w/p/tests/api.rs"},
            {"kind":["example"],"name":"e","src_path":"/w/p/examples/e.rs"},
            {"kind":["bench"],"name":"b","src_path":"/w/p/benches/b.rs"}]}]}"#;
        assert_eq!(
            parse_targets(json),
            [
                target("p", "/w/p/src/lib.rs"),
                target("p::bin/tool", "/w/p/src/bin/tool.rs"),
                target("p::api", "/w/p/tests/api.rs"),
            ]
        );
        assert!(parse_targets("not json").is_empty());
    }
}
