//! A test run's detail in the Runs tool window (`docs/design/design-system.md`,
//! "Runs: builds, tests and agent tasks in one list": "a split bar (green
//! passed, red failed, track remaining), a test tree, and each test's last 8
//! results as small bars, so flaky tests stand out"). The panel shows it
//! beside its list, or under it when the panel is narrow, while a test run
//! is selected ([`crate::runs_panel`]).
//!
//! - **The split bar**, [`SPLIT_HEIGHT`] tall under the run's title and
//!   counts: celadon (`status.success`) for the tests passed, shinsha
//!   (`status.error`) for those failed, and the track (`border.control`,
//!   as an off switch's) for the rest, each in proportion to the run's
//!   `total`, or, until that's known, to the tests ended so far
//!   ([`split`]).
//! - **The test tree**: a kiln-ui [`List`] of 24 px rows, so the arrow keys
//!   move through it and each row is an option a screen reader names. The
//!   tests are grouped by module ([`modules`]): their binary and the path of
//!   their name before its last `::`, so `p`'s `tests::adds` is in
//!   `p::tests`. A module with a failed test starts expanded, the others
//!   collapsed; a click on a module's row, Enter, or right and left, opens
//!   and closes it, and it stays as it's left whatever its tests do after.
//!   A test's row has its result's glyph ([`Glyph`]: ✓, ✕, ▲ for a flaky
//!   test that passed, a dash for one ignored), its name in code small,
//!   how long it took at the right, and its last 8 results as eight
//!   [`BAR_WIDTH`] x [`BAR_HEIGHT`] bars, oldest left, newest right, those it
//!   hasn't had yet empty, in the track ([`bars`]). A flaky test's bars are
//!   outlined in kiseto (`status.warning`). The last results are its root's
//!   history's (`fe_runlog::History`), read off the main thread, one read at
//!   a time, for each test as it comes, and for all again once the run's
//!   ended, so they take in its own results.
//! - **The detail slot**: selecting a test's row shows its name and binary
//!   under the tree. For a failed test it goes on with the failure
//!   ([`Failure`]), in code small: the panic's location and the assertion's
//!   message above; the assertion's `left` and `right` as two blocks, left
//!   on a shinsha-soft ground with a minus and right on a celadon-soft one
//!   with a plus, so neither rests on hue; the captured output below,
//!   scrolling. A failure that isn't an assertion shows its output alone.
//!   A test that failed some of its last runs ([`fe_runlog::Flaky`]) adds
//!   the flaky note, and the action row after it is empty, for later cards,
//!   but for `Open test`, which jumps to the test's function ([`test_source`]
//!   finds it with the runnables of `test_runnables.rs`).
//!
//! `runs.rerun_failed` (`␣tf`, "Runs: Rerun Failed Tests") runs the failed
//! tests of the run selected in the Runs list, else the newest test run,
//! and only those, as new runs ([`Workbench::rerun_failed`]). Its text is the catalogue's
//! (`l10n/en-US/test_runs.ftl`).

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    rc::Rc,
};

use fe_gpui::EditorTheme;
use fe_runlog::{Assertion, Outcome, TestId};
use fe_ui::{
    ActiveTheme as _, CODE_SMALL, FamilySlot, FontFamilies, Icon, List, ListDelegate, ListEvent,
    ListRow, RowState, Tokens, UI_SMALL,
};
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, Hsla, InteractiveElement as _, IntoElement,
    KeyDownEvent, ParentElement as _, Pixels, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, WeakEntity, Window, div,
    prelude::FluentBuilder as _, px, relative, svg,
};
use kiln::Cx;
use kiln_l10n::tr;

use crate::{
    Workbench,
    pane::Reveal,
    runs::{RunId, RunKind, RunStatus, RunSummary},
    runs_panel::summary_text,
    test_commands::on_workbench,
    test_runnables::{RunnableKind, runnables},
    test_runner::{TestOutcome, TestResult, TestTrack, Tracks, test_tracks},
    workbench::toast::{self, Toast},
};

/// The command's declaration: see the module's docs. It's handled by
/// [`init`]'s listener.
#[allow(dead_code)]
struct TestRunViewCommands;

#[kiln::command(context = "workbench")]
#[allow(dead_code)]
impl TestRunViewCommands {
    /// Runs: Rerun Failed Tests
    #[command(id = "runs.rerun_failed")]
    fn runs_rerun_failed(&mut self, _: &mut Cx<Self>) {}
}

/// How many results a test's row shows.
pub(crate) const BAR_COUNT: usize = 8;
/// One result's bar.
pub(crate) const BAR_WIDTH: Pixels = px(3.);
pub(crate) const BAR_HEIGHT: Pixels = px(10.);
const BAR_GAP: Pixels = px(1.);
/// The split bar's height.
pub(crate) const SPLIT_HEIGHT: Pixels = px(4.);

/// A test row's indent past its module's.
const TEST_INDENT: Pixels = px(20.);

/// The test selectors: the split bar, a tree row with its index after a
/// colon, and the detail slot.
pub(crate) const SPLIT_SELECTOR: &str = "test-run-split";
pub(crate) const TREE_ROW_SELECTOR: &str = "test-run-row";
pub(crate) const SLOT_SELECTOR: &str = "test-run-detail";
/// The failure's blocks: the panic, the left value, the right, the output,
/// the flaky note, the action row and `Open test`.
pub(crate) const PANIC_SELECTOR: &str = "test-run-panic";
pub(crate) const LEFT_SELECTOR: &str = "test-run-left";
pub(crate) const RIGHT_SELECTOR: &str = "test-run-right";
pub(crate) const OUTPUT_SELECTOR: &str = "test-run-output";
pub(crate) const FLAKY_SELECTOR: &str = "test-run-flaky";
pub(crate) const ACTIONS_SELECTOR: &str = "test-run-actions";
pub(crate) const OPEN_TEST_SELECTOR: &str = "test-run-open-test";

/// The tallest the failure's detail gets, the output scrolling in it.
const FAILURE_MAX_HEIGHT: Pixels = px(280.);

/// Tree row `ix`'s test selector.
pub(crate) fn tree_row_selector(ix: usize) -> String {
    format!("{TREE_ROW_SELECTOR}:{ix}")
}

/// A result's glyph, on a test's or a module's row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Glyph {
    Passed,
    Failed,
    /// It both passed and failed in its last runs.
    Flaky,
    Ignored,
}

impl Glyph {
    /// A test's: how it ended, a pass flaky if its last runs say so. A
    /// failure is always a failure: a test that's just broken has a pass
    /// and a fail in its last runs, so it reads as flaky, and its glyph
    /// mustn't hide that it failed. Its outlined bars still say flaky.
    pub(crate) fn of(outcome: TestOutcome, flaky: bool) -> Self {
        match outcome {
            TestOutcome::Ignored => Self::Ignored,
            TestOutcome::Failed => Self::Failed,
            TestOutcome::Passed if flaky => Self::Flaky,
            TestOutcome::Passed => Self::Passed,
        }
    }

    pub(crate) fn icon(self) -> Icon {
        match self {
            Self::Passed => Icon::Check,
            Self::Failed => Icon::X,
            Self::Flaky => Icon::Triangle,
            Self::Ignored => Icon::Minus,
        }
    }

    /// Celadon, shinsha, kiseto, muted.
    pub(crate) fn colour(self, c: &Tokens<Hsla>) -> Hsla {
        match self {
            Self::Passed => c.status.success,
            Self::Failed => c.status.error,
            Self::Flaky => c.status.warning,
            Self::Ignored => c.text.muted,
        }
    }
}

/// One of a test's last results, as its bar shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Bar {
    /// A run it hasn't had yet.
    Empty,
    Passed,
    Failed,
}

impl Bar {
    /// Celadon, shinsha, or the track.
    pub(crate) fn colour(self, c: &Tokens<Hsla>) -> Hsla {
        match self {
            Self::Empty => track(c),
            Self::Passed => c.status.success,
            Self::Failed => c.status.error,
        }
    }
}

/// What the split bar and an empty result's bar are drawn on.
pub(crate) fn track(c: &Tokens<Hsla>) -> Hsla {
    c.border.control
}

/// The outline of a flaky test's bars.
pub(crate) fn flaky_outline(c: &Tokens<Hsla>) -> Hsla {
    c.status.warning
}

/// The bars of `last`, a test's last results oldest first: the last
/// [`BAR_COUNT`], oldest left and newest right, the runs it hasn't had yet
/// empty at the left, so every row's newest is in the same place.
pub(crate) fn bars(last: &[Outcome]) -> [Bar; BAR_COUNT] {
    let last = &last[last.len().saturating_sub(BAR_COUNT)..];
    let mut bars = [Bar::Empty; BAR_COUNT];
    for (bar, outcome) in bars[BAR_COUNT - last.len()..].iter_mut().zip(last) {
        *bar = match outcome {
            Outcome::Passed => Bar::Passed,
            Outcome::Failed => Bar::Failed,
        };
    }
    bars
}

/// The fractions of the split bar the passed and the failed tests fill, of
/// `total`, or, while that isn't known, of the `ended` so far: none if
/// there's nothing yet.
pub(crate) fn split(
    passed: usize,
    failed: usize,
    ended: usize,
    total: Option<usize>,
) -> (f32, f32) {
    let of = total.unwrap_or(ended).max(passed + failed);
    if of == 0 {
        return (0., 0.);
    }
    (passed as f32 / of as f32, failed as f32 / of as f32)
}

/// How long a test took, as its row says it: whole milliseconds under a
/// second, else seconds to a tenth.
pub(crate) fn duration_label(secs: f64) -> SharedString {
    if secs < 1. {
        let ms = (secs * 1000.).round() as u64;
        tr!("workbench-test-runs-duration-ms", ms = ms.min(999))
    } else {
        tr!(
            "workbench-test-runs-duration-secs",
            secs = (secs * 10.).round() / 10.
        )
    }
}

/// A module's identity: its binary and its module path apart, so a library's
/// `p` with `checks::x` and an integration target `p::checks` with `x` are
/// two modules though they read alike.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct ModuleKey {
    pub(crate) binary: String,
    /// The path of its tests' names before their last `::`: empty for a
    /// doc test's, and a top-level test's.
    pub(crate) path: String,
}

impl ModuleKey {
    /// How its row reads: the binary and the path joined by `::`, empty
    /// when both are.
    pub(crate) fn label(&self) -> String {
        [self.binary.as_str(), self.path.as_str()]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("::")
    }
}

/// A module of a run's tests, and how its tests ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Module {
    /// Its binary and the path of its tests' names before their last `::`.
    pub(crate) key: ModuleKey,
    /// Its tests, as indices into the results, in the order they came.
    pub(crate) tests: Vec<usize>,
    pub(crate) passed: usize,
    pub(crate) failed: usize,
}

/// `id`'s module, and its name in it: a doc test's name is a file and an
/// item, not a path, so it's in its binary's.
pub(crate) fn module_of(id: &TestId) -> (ModuleKey, &str) {
    let (module, leaf) = match id.binary.starts_with("Doc-tests") {
        true => ("", id.name.as_str()),
        false => id.name.rsplit_once("::").unwrap_or(("", &id.name)),
    };
    let key = ModuleKey {
        binary: id.binary.clone(),
        path: module.to_owned(),
    };
    (key, leaf)
}

/// `results`' modules, in the order their first tests came.
pub(crate) fn modules(results: &[TestResult]) -> Vec<Module> {
    let mut modules: Vec<Module> = Vec::new();
    for (ix, result) in results.iter().enumerate() {
        let (key, _) = module_of(&result.id);
        let at = match modules.iter().position(|module| module.key == key) {
            Some(at) => at,
            None => {
                modules.push(Module {
                    key,
                    tests: Vec::new(),
                    passed: 0,
                    failed: 0,
                });
                modules.len() - 1
            }
        };
        let module = &mut modules[at];
        module.tests.push(ix);
        match result.outcome {
            TestOutcome::Passed => module.passed += 1,
            TestOutcome::Failed => module.failed += 1,
            TestOutcome::Ignored => {}
        }
    }
    modules
}

/// Whether `module` shows its tests: one with a failed test does, unless
/// it's been closed, and one without doesn't, unless it's been opened.
/// Once opened or closed, it stays so, whatever its tests do after.
pub(crate) fn expanded(module: &Module, opened: &HashMap<ModuleKey, bool>) -> bool {
    (opened.get(&module.key).copied()).unwrap_or(module.failed > 0)
}

/// What a tree row is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RowKey {
    /// A module, by its key.
    Module(ModuleKey),
    /// A test.
    Test(TestId),
}

/// One tree row, as it's drawn.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TreeRow {
    pub(crate) key: RowKey,
    /// A module's: whether it shows its tests.
    pub(crate) expanded: Option<bool>,
    pub(crate) glyph: Glyph,
    /// A module's path, or a test's name in its module.
    pub(crate) label: SharedString,
    /// At the right: a module's count of tests, a test's duration.
    pub(crate) trailing: Option<SharedString>,
    /// A test's last results' bars, and whether it's flaky, outlining them.
    pub(crate) bars: Option<([Bar; BAR_COUNT], bool)>,
    /// Its accessible name.
    pub(crate) name: SharedString,
}

/// The tree's rows: each module, and, under each expanded one, its tests.
pub(crate) fn tree_rows(
    results: &[TestResult],
    tracks: &Tracks,
    opened: &HashMap<ModuleKey, bool>,
) -> Vec<TreeRow> {
    let mut rows = Vec::new();
    for module in modules(results) {
        let open = expanded(&module, opened);
        let flaky = |ix: &usize| {
            let flaky = tracks.get(&results[*ix].id).and_then(|t| t.flaky);
            flaky.is_some() && results[*ix].outcome != TestOutcome::Ignored
        };
        let glyph = if module.failed > 0 {
            Glyph::Failed
        } else if module.tests.iter().any(flaky) {
            Glyph::Flaky
        } else if module.passed > 0 {
            Glyph::Passed
        } else {
            Glyph::Ignored
        };
        let name = module.key.label();
        let label: SharedString = match name.is_empty() {
            true => tr!("workbench-test-runs-module-top"),
            false => name.into(),
        };
        let (passed, failed) = (module.passed, module.failed);
        let name = match open {
            true => tr!(
                "workbench-test-runs-module-row-expanded",
                module = &label,
                passed = passed,
                failed = failed
            ),
            false => tr!(
                "workbench-test-runs-module-row-collapsed",
                module = &label,
                passed = passed,
                failed = failed
            ),
        };
        rows.push(TreeRow {
            key: RowKey::Module(module.key.clone()),
            expanded: Some(open),
            glyph,
            label,
            trailing: Some(tr!(
                "workbench-test-runs-module-count",
                count = module.tests.len()
            )),
            bars: None,
            name,
        });
        if open {
            rows.extend((module.tests.iter()).map(|ix| test_row(&results[*ix], tracks)));
        }
    }
    rows
}

/// A test's row.
fn test_row(result: &TestResult, tracks: &Tracks) -> TreeRow {
    let track = tracks.get(&result.id).cloned().unwrap_or_default();
    let TestTrack { last, flaky } = track;
    let flaky = flaky.is_some();
    let glyph = Glyph::of(result.outcome, flaky);
    let (_, leaf) = module_of(&result.id);
    let duration = result.secs.map(duration_label);
    let outcome = match (result.outcome, flaky) {
        (TestOutcome::Passed, false) => tr!("workbench-test-runs-outcome-passed"),
        (TestOutcome::Passed, true) => tr!("workbench-test-runs-outcome-passed-flaky"),
        (TestOutcome::Failed, false) => tr!("workbench-test-runs-outcome-failed"),
        (TestOutcome::Failed, true) => tr!("workbench-test-runs-outcome-failed-flaky"),
        (TestOutcome::Ignored, _) => tr!("workbench-test-runs-outcome-ignored"),
    };
    let passed = last.iter().filter(|o| **o == Outcome::Passed).count();
    let failed = last.len() - passed;
    let name = match &duration {
        Some(duration) => tr!(
            "workbench-test-runs-test-row-timed",
            name = &result.id.name,
            outcome = outcome,
            duration = duration,
            passed = passed,
            failed = failed
        ),
        None => tr!(
            "workbench-test-runs-test-row",
            name = &result.id.name,
            outcome = outcome,
            passed = passed,
            failed = failed
        ),
    };
    TreeRow {
        key: RowKey::Test(result.id.clone()),
        expanded: None,
        glyph,
        label: leaf.to_owned().into(),
        trailing: duration,
        bars: Some((bars(&last), flaky)),
        name,
    }
}

/// A drawn row's bars, for tests: each one's fill, and the outline round
/// them, if any.
#[cfg(test)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DrawnBars {
    pub(crate) fills: [Hsla; BAR_COUNT],
    pub(crate) outline: Option<Hsla>,
}

/// What a click on a module's row does: opens or closes it.
type Toggle = Rc<dyn Fn(ModuleKey, &mut Window, &mut App)>;

/// The tree's rows.
pub(crate) struct TestRows {
    rows: Vec<TreeRow>,
    toggle: Toggle,
    /// Each drawn test row's bars, by row, for tests.
    #[cfg(test)]
    pub(crate) drawn_bars: std::collections::BTreeMap<usize, DrawnBars>,
    /// Each drawn row's glyph and its colour, by row, for tests.
    #[cfg(test)]
    pub(crate) drawn_glyphs: std::collections::BTreeMap<usize, (Icon, Hsla)>,
}

impl TestRows {
    pub(crate) fn rows(&self) -> &[TreeRow] {
        &self.rows
    }

    /// The bars of a test's row, outlined in kiseto if it's flaky; a
    /// transparent outline otherwise, so every row's bars line up.
    fn bars(bars: &[Bar; BAR_COUNT], flaky: bool, c: &Tokens<Hsla>) -> gpui::Div {
        let outline = match flaky {
            true => flaky_outline(c),
            false => gpui::transparent_black(),
        };
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(BAR_GAP)
            .p(px(1.))
            .border_1()
            .border_color(outline)
            .rounded(px(2.))
            .children((bars.iter()).map(|bar| div().w(BAR_WIDTH).h(BAR_HEIGHT).bg(bar.colour(c))))
    }
}

impl ListDelegate for TestRows {
    fn count(&self) -> usize {
        self.rows.len()
    }

    fn render_row(
        &mut self,
        ix: usize,
        state: RowState,
        _: &mut Window,
        cx: &mut Context<List<Self>>,
    ) -> AnyElement {
        let c = cx.theme().colors;
        let code = (cx.try_global::<FontFamilies>().cloned().unwrap_or_default())
            .family(FamilySlot::Code, cx);
        #[cfg(test)]
        {
            let row = &self.rows[ix];
            self.drawn_glyphs
                .insert(ix, (row.glyph.icon(), row.glyph.colour(&c)));
            match &row.bars {
                Some((bars, flaky)) => self.drawn_bars.insert(
                    ix,
                    DrawnBars {
                        fills: bars.map(|bar| bar.colour(&c)),
                        outline: flaky.then(|| flaky_outline(&c)),
                    },
                ),
                None => self.drawn_bars.remove(&ix),
            };
        }
        let row = &self.rows[ix];
        let chevron = row.expanded.map(|open| match open {
            true => Icon::ChevronDown,
            false => Icon::ChevronRight,
        });
        let body = ListRow::new(state)
            .gap_2()
            .px_3()
            .when(chevron.is_none(), |body| body.pl(TEST_INDENT + px(12.)))
            .when_some(chevron, |body, chevron| {
                body.child(
                    svg()
                        .path(chevron.path())
                        .size(px(12.))
                        .flex_none()
                        .text_color(c.list.chevron),
                )
            })
            .child(
                svg()
                    .path(row.glyph.icon().path())
                    .size(px(14.))
                    .flex_none()
                    .text_color(row.glyph.colour(&c)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_family(code)
                    .text_size(CODE_SMALL.size)
                    .line_height(CODE_SMALL.line_height)
                    .text_color(c.text.primary)
                    .child(row.label.clone()),
            )
            .when_some(row.trailing.clone(), |body, trailing| {
                body.child(
                    div()
                        .flex_none()
                        .text_size(UI_SMALL.size)
                        .line_height(UI_SMALL.line_height)
                        .text_color(c.text.muted)
                        .child(trailing),
                )
            })
            .when_some(row.bars, |body, (bars, flaky)| {
                body.child(Self::bars(&bars, flaky, &c))
            });
        let module = match &row.key {
            RowKey::Module(key) => Some(key.clone()),
            RowKey::Test(_) => None,
        };
        let toggle = self.toggle.clone();
        // The list selects the row; a click on a module's opens or closes
        // it too.
        div()
            .id(("test-run-row", ix))
            .debug_selector(move || tree_row_selector(ix))
            .w_full()
            .when_some(module, |row, module| {
                row.on_click(move |_, window, cx| toggle(module.clone(), window, cx))
            })
            .child(body)
            .into_any_element()
    }

    fn accessible_name(&self, _: &App) -> SharedString {
        tr!("workbench-test-runs-tree")
    }

    fn row_name(&self, ix: usize, _: &App) -> SharedString {
        self.rows[ix].name.clone()
    }
}

/// What a failed test's output says, as the detail slot shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Failure {
    /// The panic's own line, `thread 'x' panicked at src/lib.rs:7:5:`, when
    /// the failure is an assertion.
    pub(crate) location: Option<String>,
    /// The failed `assert_eq!` or `assert_ne!`; none for another failure.
    pub(crate) assertion: Option<Assertion>,
    /// The captured output, whole.
    pub(crate) output: String,
}

impl Failure {
    pub(crate) fn of(output: &str) -> Self {
        let assertion = Assertion::parse(output);
        let location = assertion.as_ref().and_then(|_| {
            output
                .lines()
                .find(|line| line.contains("panicked at"))
                .map(str::to_owned)
        });
        Self {
            location,
            assertion,
            output: output.to_owned(),
        }
    }
}

/// One of an assertion's two values, as its block shows it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Block {
    pub(crate) selector: &'static str,
    /// Minus for the left, plus for the right.
    pub(crate) icon: Icon,
    /// Shinsha for the left, celadon for the right.
    pub(crate) glyph: Hsla,
    /// Their soft grounds, the diff view's removed and added rows'.
    pub(crate) ground: Hsla,
    pub(crate) value: String,
}

/// `failure`'s assertion's blocks, left then right; none for another failure.
pub(crate) fn blocks(failure: &Failure, c: &Tokens<Hsla>, diff: &EditorTheme) -> Vec<Block> {
    let Some(assertion) = &failure.assertion else {
        return Vec::new();
    };
    vec![
        Block {
            selector: LEFT_SELECTOR,
            icon: Icon::Minus,
            glyph: c.status.error,
            ground: diff.diff_removed_row,
            value: assertion.left.clone(),
        },
        Block {
            selector: RIGHT_SELECTOR,
            icon: Icon::Plus,
            glyph: c.status.success,
            ground: diff.diff_added_row,
            value: assertion.right.clone(),
        },
    ]
}

/// The directories [`test_source`] doesn't look in.
const SKIPPED_DIRS: [&str; 3] = ["target", "node_modules", "vendor"];
/// The most files [`test_source`] reads.
const SOURCE_FILES_LIMIT: usize = 20_000;

/// The file under `root` that has the Rust test `id` names, and the row of
/// its function. The runnables of each file that has the function's name
/// are read (`test_runnables.rs`): the test is the one whose selector ends
/// the test's name, and whose module path before it, if any, names the file
/// or a folder above it.
pub(crate) fn test_source(root: &Path, id: &TestId) -> Option<(PathBuf, u32)> {
    if id.binary.starts_with("Doc-tests") {
        return None;
    }
    let leaf = id.name.rsplit("::").next()?;
    let needle = leaf;
    let mut dirs = vec![root.to_owned()];
    let mut seen = 0;
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                if !name.starts_with('.') && !SKIPPED_DIRS.contains(&name.as_ref()) {
                    dirs.push(path);
                }
            } else if kind.is_file() && path.extension().is_some_and(|ext| ext == "rs") {
                seen += 1;
                if seen > SOURCE_FILES_LIMIT {
                    return None;
                }
                let Ok(text) = fs::read_to_string(&path) else {
                    continue;
                };
                if !text.contains(needle) {
                    continue;
                }
                if let Some(row) = test_row_in(&text, &path, &id.name, needle) {
                    return Some((path, row));
                }
            }
        }
    }
    None
}

/// The row of the test `name`'s function in `text`, the Rust file at `path`.
fn test_row_in(text: &str, path: &Path, name: &str, needle: &str) -> Option<u32> {
    let buffer = fe_buffer::Buffer::new(text);
    let mut syntax = fe_syntax::Syntax::new(fe_syntax::language_by_name("rust")?, buffer.current());
    syntax.finish(syntax.parse_job().run());
    let found = runnables("rust", &syntax, buffer.text());
    let words: Vec<String> = path
        .components()
        .map(|part| {
            let part = part.as_os_str().to_string_lossy();
            part.strip_suffix(".rs").unwrap_or(&part).to_owned()
        })
        .collect();
    let runnable = found.iter().find(|runnable| {
        let Some(module) = name.strip_suffix(runnable.selector.as_str()) else {
            return false;
        };
        let module = match module.strip_suffix("::") {
            Some(module) => module,
            None if module.is_empty() => module,
            None => return false,
        };
        runnable.kind == RunnableKind::Test
            && (module.split("::").filter(|word| !word.is_empty()))
                .all(|word| words.contains(&word.to_owned()))
    })?;
    let lines: Vec<&str> = text.lines().collect();
    (runnable.rows.clone())
        .find(|row| {
            lines
                .get(*row as usize)
                .is_some_and(|line| line.contains(needle))
        })
        .or(Some(runnable.rows.start))
}

/// The run shown, as the view last read it.
#[derive(Clone, Debug, PartialEq)]
struct Shown {
    id: RunId,
    title: SharedString,
    status: RunStatus,
    summary: RunSummary,
}

/// A test run's detail: see the module docs.
pub(crate) struct TestRunView {
    workbench: WeakEntity<Workbench>,
    shown: Option<Shown>,
    results: Vec<TestResult>,
    tracks: Tracks,
    /// The tests whose last results have been asked of the history.
    requested: HashSet<TestId>,
    /// The history's read, under way: one at a time.
    reading: Option<Task<()>>,
    /// Whether the run's been seen to end, and its tests asked for again.
    read_ended: bool,
    /// The modules opened (true) or closed by hand.
    opened: HashMap<ModuleKey, bool>,
    /// The row selected, kept as the rows change.
    selected: Option<RowKey>,
    tree: Entity<List<TestRows>>,
    /// The split bar's drawn fractions, passed and failed, for tests.
    #[cfg(test)]
    pub(crate) drawn_split: Option<(f32, f32)>,
    /// The failure's blocks as last drawn, for tests.
    #[cfg(test)]
    pub(crate) drawn_blocks: Vec<Block>,
    _tree: Subscription,
}

impl TestRunView {
    pub(crate) fn new(
        workbench: WeakEntity<Workbench>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let view = cx.weak_entity();
        let toggle: Toggle = Rc::new(move |module, window, cx| {
            let view = view.clone();
            // Outside the click's update of the list.
            window.defer(cx, move |_, cx| {
                view.update(cx, |view, cx| view.toggle(&module, cx)).ok();
            });
        });
        let rows = TestRows {
            rows: Vec::new(),
            toggle,
            #[cfg(test)]
            drawn_bars: Default::default(),
            #[cfg(test)]
            drawn_glyphs: Default::default(),
        };
        let tree = cx.new(|cx| List::new(rows, cx));
        let events = cx.subscribe_in(&tree, window, |view, tree, event, _, cx| {
            let (ListEvent::Selected(ix) | ListEvent::Confirmed(ix)) = *event;
            let key = tree
                .read(cx)
                .delegate()
                .rows
                .get(ix)
                .map(|row| row.key.clone());
            view.selected = key.clone();
            if let (ListEvent::Confirmed(_), Some(RowKey::Module(module))) = (event, key) {
                view.toggle(&module, cx);
            }
            cx.notify();
        });
        Self {
            workbench,
            shown: None,
            results: Vec::new(),
            tracks: Tracks::default(),
            requested: HashSet::new(),
            reading: None,
            read_ended: false,
            opened: HashMap::new(),
            selected: None,
            tree,
            #[cfg(test)]
            drawn_split: None,
            #[cfg(test)]
            drawn_blocks: Vec::new(),
            _tree: events,
        }
    }

    /// The run shown, if any.
    pub(crate) fn run(&self) -> Option<RunId> {
        self.shown.as_ref().map(|shown| shown.id)
    }

    /// The tree, for tests.
    #[cfg(test)]
    pub(crate) fn tree(&self) -> &Entity<List<TestRows>> {
        &self.tree
    }

    /// The results shown, for tests.
    #[cfg(test)]
    pub(crate) fn results_for_tests(&self) -> Vec<TestResult> {
        self.results.clone()
    }

    /// The test whose row is selected, for the detail slot.
    pub(crate) fn selected_test(&self) -> Option<&TestResult> {
        let Some(RowKey::Test(id)) = &self.selected else {
            return None;
        };
        self.results.iter().rev().find(|result| &result.id == id)
    }

    /// Shows the test run `run`, as it is now, or nothing. A run shown
    /// afresh starts with its modules as they start and nothing selected;
    /// one already shown keeps them. The last results of tests new to the
    /// view are read from the history as they come, and all again once the
    /// run's ended, which then has its own.
    pub(crate) fn show(&mut self, run: Option<RunId>, cx: &mut Context<Self>) {
        let workbench = self.workbench.upgrade();
        let found = run.zip(workbench.as_ref()).and_then(|(id, workbench)| {
            let workbench = workbench.read(cx);
            let run = workbench.runs().read(cx).get(id)?;
            if run.kind != RunKind::Test {
                return None;
            }
            let shown = Shown {
                id,
                title: run.title.clone().into(),
                status: run.status,
                summary: run.summary,
            };
            Some((shown, workbench.test_results(id, cx).unwrap_or_default()))
        });
        let Some((shown, results)) = found else {
            if self.shown.take().is_some() {
                self.reset();
                self.rebuild(cx);
            }
            return;
        };
        let afresh = self.run() != Some(shown.id);
        if afresh {
            self.reset();
        }
        if !shown.status.is_running() && !self.read_ended {
            // Its own results are in the history now, or queued for it.
            self.read_ended = true;
            self.requested.clear();
        }
        let id = shown.id;
        self.shown = Some(shown);
        self.results = results;
        self.read_history(id, cx);
        self.rebuild(cx);
    }

    fn reset(&mut self) {
        self.results.clear();
        self.tracks.clear();
        self.requested.clear();
        self.reading = None;
        self.read_ended = false;
        self.opened.clear();
        self.selected = None;
    }

    /// Reads the last results of `run`'s tests not asked for yet, once
    /// what's queued for the history before is done, unless a read's under
    /// way: when it's done, it reads those that came meanwhile. A read is
    /// started after the update that asked, so a run that's just ended has
    /// had its results queued for the history first. Reads come back in the
    /// order they were made, each newer than the last; one for a run no
    /// longer shown is dropped with it.
    fn read_history(&mut self, run: RunId, cx: &mut Context<Self>) {
        if self.reading.is_some() {
            return;
        }
        let mut ids: Vec<TestId> = Vec::new();
        for result in &self.results {
            if self.requested.insert(result.id.clone()) {
                ids.push(result.id.clone());
            }
        }
        if ids.is_empty() {
            return;
        }
        let workbench = self.workbench.clone();
        self.reading = Some(cx.spawn(async move |view, cx| {
            let reading = cx.update(|cx| {
                let workbench = workbench.upgrade()?;
                test_tracks(&workbench, run, ids, cx)
            });
            let tracks = match reading {
                Some(reading) => reading.await,
                None => Tracks::default(),
            };
            let read = view.update(cx, |view, cx| {
                if view.run() == Some(run) {
                    view.tracks.extend(tracks);
                    view.reading = None;
                    view.read_history(run, cx);
                    view.rebuild(cx);
                }
            });
            read.ok();
        }));
    }

    /// Opens `module` if it's closed, else closes it, and keeps it so.
    pub(crate) fn toggle(&mut self, module: &ModuleKey, cx: &mut Context<Self>) {
        let rows = self.tree.read(cx).delegate().rows();
        let key = RowKey::Module(module.clone());
        let row = rows.iter().find(|row| row.key == key);
        let open = row.and_then(|row| row.expanded).unwrap_or(false);
        self.opened.insert(module.clone(), !open);
        self.rebuild(cx);
    }

    /// Opens (`open`) or closes the selected module, if a module's
    /// selected and it isn't so already.
    fn open_selected(&mut self, open: bool, cx: &mut Context<Self>) -> bool {
        let Some(RowKey::Module(module)) = self.selected.clone() else {
            return false;
        };
        let rows = self.tree.read(cx).delegate().rows();
        let row = rows
            .iter()
            .find(|row| row.key == RowKey::Module(module.clone()));
        if row.and_then(|row| row.expanded) == Some(open) {
            return false;
        }
        self.toggle(&module, cx);
        true
    }

    /// Builds the tree's rows again, keeping the selected row selected
    /// where it's moved to, none if it's gone, without scrolling.
    fn rebuild(&mut self, cx: &mut Context<Self>) {
        let rows = match self.shown {
            Some(_) => tree_rows(&self.results, &self.tracks, &self.opened),
            None => Vec::new(),
        };
        let selected = self.selected.clone();
        let at = selected.and_then(|key| rows.iter().position(|row| row.key == key));
        if at.is_none() {
            self.selected = None;
        }
        self.tree.update(cx, |tree, cx| {
            let before = tree.selected();
            tree.delegate_mut().rows = rows;
            tree.rows_changed(cx);
            if at != before {
                tree.keep_selected(at, cx);
            }
        });
        cx.notify();
    }

    /// The split bar.
    fn split_bar(&mut self, c: &Tokens<Hsla>) -> gpui::Div {
        let ended = (self.results.iter())
            .filter(|result| result.outcome != TestOutcome::Ignored)
            .count();
        let (passed, failed, total) = match self.shown.as_ref().map(|shown| shown.summary) {
            Some(RunSummary::Tests {
                passed,
                failed,
                total,
            }) => (passed, failed, total),
            _ => (0, 0, None),
        };
        let (passed, failed) = split(passed, failed, ended, total);
        #[cfg(test)]
        {
            self.drawn_split = Some((passed, failed));
        }
        div()
            .debug_selector(|| SPLIT_SELECTOR.into())
            .flex_none()
            .mx_3()
            .h(SPLIT_HEIGHT)
            .flex()
            .rounded(SPLIT_HEIGHT / 2.)
            .overflow_hidden()
            .bg(track(c))
            .child(div().h_full().w(relative(passed)).bg(c.status.success))
            .child(div().h_full().w(relative(failed)).bg(c.status.error))
    }

    /// The detail slot, while a test's row is selected: its glyph, its name
    /// and its binary, and `Open test` if it failed; then the failure.
    fn slot(
        &self,
        c: &Tokens<Hsla>,
        code: SharedString,
        diff: &EditorTheme,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Div> {
        let test = self.selected_test()?;
        let track = self.tracks.get(&test.id);
        let flaky = track.and_then(|track| track.flaky);
        let glyph = Glyph::of(test.outcome, flaky.is_some());
        let failed = test.outcome == TestOutcome::Failed;
        let header = div()
            .flex_none()
            .h(fe_ui::ROW_HEIGHT)
            .px_3()
            .flex()
            .items_center()
            .gap_2()
            .child(
                svg()
                    .path(glyph.icon().path())
                    .size(px(14.))
                    .flex_none()
                    .text_color(glyph.colour(c)),
            )
            .child(
                div()
                    .flex_shrink(1.)
                    .min_w_0()
                    .truncate()
                    .font_family(code.clone())
                    .text_size(CODE_SMALL.size)
                    .line_height(CODE_SMALL.line_height)
                    .text_color(c.text.primary)
                    .child(SharedString::from(test.id.name.clone())),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(UI_SMALL.size)
                    .line_height(UI_SMALL.line_height)
                    .text_color(c.text.muted)
                    .child(SharedString::from(test.id.binary.clone())),
            )
            .when(failed, |header| {
                header.child(
                    div()
                        .id("test-run-open-test")
                        .debug_selector(|| OPEN_TEST_SELECTOR.into())
                        .flex_none()
                        .cursor_pointer()
                        .text_size(UI_SMALL.size)
                        .line_height(UI_SMALL.line_height)
                        .text_color(c.text.accent)
                        .child(tr!("workbench-test-runs-open-test"))
                        .on_click(cx.listener(|view, _, window, cx| view.open_test(window, cx))),
                )
            });
        let failure = failed.then(|| Failure::of(&test.output));
        let slot = div()
            .debug_selector(|| SLOT_SELECTOR.into())
            .flex_none()
            .max_h(FAILURE_MAX_HEIGHT)
            .flex()
            .flex_col()
            .border_t_1()
            .border_color(c.border.subtle)
            .child(header)
            .children(failure.map(|failure| {
                let note = flaky.map(|flaky| {
                    let note = tr!(
                        "workbench-test-runs-failure-flaky",
                        failed = flaky.failed,
                        of = flaky.of
                    );
                    (div().debug_selector(|| FLAKY_SELECTOR.into()))
                        .flex_none()
                        .text_size(UI_SMALL.size)
                        .line_height(UI_SMALL.line_height)
                        .text_color(c.status.warning)
                        .child(note)
                });
                let actions = div()
                    .debug_selector(|| ACTIONS_SELECTOR.into())
                    .flex_none()
                    .flex()
                    .gap_2();
                Self::failure(&failure, c, code, diff)
                    .children(note)
                    .child(actions)
            }));
        Some(slot)
    }

    /// A failure's detail: see the module docs.
    fn failure(
        failure: &Failure,
        c: &Tokens<Hsla>,
        code: SharedString,
        diff: &EditorTheme,
    ) -> gpui::Div {
        let text = |selector: &'static str, text: &str| {
            div()
                .debug_selector(|| selector.into())
                .flex_none()
                .font_family(code.clone())
                .text_size(CODE_SMALL.size)
                .line_height(CODE_SMALL.line_height)
                .text_color(c.text.primary)
                .child(SharedString::from(text.to_owned()))
        };
        // A value's block: its glyph, so it doesn't rest on hue, and its
        // value, wrapping.
        let side = |block: Block| {
            div()
                .debug_selector(|| block.selector.into())
                .flex_none()
                .flex()
                .gap_2()
                .px_2()
                .py_1()
                .rounded(px(3.))
                .bg(block.ground)
                .child(
                    svg()
                        .path(block.icon.path())
                        .size(px(12.))
                        .flex_none()
                        .mt(px(2.))
                        .text_color(block.glyph),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .font_family(code.clone())
                        .text_size(CODE_SMALL.size)
                        .line_height(CODE_SMALL.line_height)
                        .text_color(c.text.primary)
                        .child(SharedString::from(block.value)),
                )
        };
        let above = (failure.location.iter())
            .map(|location| text(PANIC_SELECTOR, location))
            .chain(failure.assertion.iter().filter_map(|assertion| {
                (assertion.message.as_deref()).map(|message| text(PANIC_SELECTOR, message))
            }));
        let blocks = blocks(failure, c, diff).into_iter().map(side);
        div()
            .px_3()
            .pb_2()
            .flex()
            .flex_col()
            .gap_1()
            .min_h_0()
            .children(above)
            .children(blocks)
            .child(
                div()
                    .id("test-run-output-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(text(OUTPUT_SELECTOR, &failure.output)),
            )
    }

    /// The editor's theme, for the failure's grounds.
    fn editor_theme(&self, cx: &App) -> EditorTheme {
        match self.workbench.upgrade() {
            Some(workbench) => workbench.read(cx).pane().read(cx).theme().clone(),
            None => EditorTheme::default_dark(cx),
        }
    }

    /// Opens the selected test's function in the file that has it, if it's
    /// found: looked for off the main thread.
    fn open_test(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(test) = self.selected_test() else {
            return;
        };
        let id = test.id.clone();
        let Some(workbench) = self.workbench.upgrade() else {
            return;
        };
        let Some(root) = workbench.read(cx).root().map(Path::to_owned) else {
            return;
        };
        cx.spawn_in(window, async move |view, cx| {
            let found = cx
                .background_spawn(async move { test_source(&root, &id) })
                .await;
            let Some((path, row)) = found else {
                return;
            };
            view.update_in(cx, |_, window, cx| {
                let Some(workbench) = workbench.read(cx).pane().clone().into() else {
                    return;
                };
                let reveal = Reveal {
                    row,
                    columns: 0..0,
                    within_row: true,
                };
                let pane: Entity<crate::pane::Pane> = workbench;
                pane.update(cx, |pane, cx| pane.open_at(path, Some(reveal), window, cx));
            })
            .ok();
        })
        .detach();
    }
}

impl Render for TestRunView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = cx.theme().colors;
        let code = (cx.try_global::<FontFamilies>().cloned().unwrap_or_default())
            .family(FamilySlot::Code, cx);
        let Some(shown) = self.shown.clone() else {
            return div().into_any_element();
        };
        let split = self.split_bar(&c);
        let diff = self.editor_theme(cx);
        #[cfg(test)]
        {
            let failed = (self.selected_test()).filter(|test| test.outcome == TestOutcome::Failed);
            self.drawn_blocks = failed
                .map(|test| blocks(&Failure::of(&test.output), &c, &diff))
                .unwrap_or_default();
        }
        let slot = self.slot(&c, code.clone(), &diff, cx);
        div()
            .size_full()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_none()
                    .h(fe_ui::ROW_HEIGHT)
                    .px_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_shrink(1.)
                            .min_w_0()
                            .truncate()
                            .font_family(code)
                            .text_size(CODE_SMALL.size)
                            .line_height(CODE_SMALL.line_height)
                            .text_color(c.text.primary)
                            .child(shown.title),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(UI_SMALL.size)
                            .line_height(UI_SMALL.line_height)
                            .text_color(c.text.muted)
                            .child(summary_text(shown.status, shown.summary)),
                    ),
            )
            .child(split)
            .child(
                div()
                    .id("test-run-tree")
                    .flex_1()
                    .min_h_0()
                    .mt_1()
                    .flex()
                    .flex_col()
                    .on_key_down(cx.listener(|view, event: &KeyDownEvent, _, cx| {
                        let keys = &event.keystroke;
                        if keys.modifiers.modified() {
                            return;
                        }
                        let open = match keys.key.as_str() {
                            "right" => true,
                            "left" => false,
                            _ => return,
                        };
                        if view.open_selected(open, cx) {
                            cx.stop_propagation();
                        }
                    }))
                    .child(self.tree.clone()),
            )
            .children(slot)
            .into_any_element()
    }
}

impl Workbench {
    /// The run `runs.rerun_failed` reruns the failed tests of: the one
    /// selected in the Runs list if it's a test run, else the newest test
    /// run.
    fn rerun_target(&self, cx: &App) -> Option<RunId> {
        let selected = self.runs_panel.read(cx).selected_run(cx);
        let runs = self.runs().read(cx);
        let test = |id: &RunId| runs.get(*id).is_some_and(|run| run.kind == RunKind::Test);
        selected.filter(test).or_else(|| {
            (runs.list())
                .find(|run| run.kind == RunKind::Test)
                .map(|run| run.id)
        })
    }
}

/// Handles the command.
pub(crate) fn init(cx: &mut App) {
    cx.on_action(|_: &RunsRerunFailed, cx| {
        on_workbench(cx, |workbench, window, cx| {
            match workbench.rerun_target(cx) {
                Some(run) => workbench.rerun_failed(run, window, cx),
                None => toast::show(cx, Toast::info(tr!("workbench-test-runs-rerun-none"))),
            }
        });
    });
}
