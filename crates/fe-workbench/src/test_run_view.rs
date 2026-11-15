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
//!   under the tree, where the failure's detail goes (a later card).
//!
//! `runs.rerun_failed` (`␣tf`, "Runs: Rerun Failed Tests") runs the failed
//! tests of the run selected in the Runs list, else the newest test run,
//! and only those, as new runs ([`Workbench::rerun_failed`]). Its text is the catalogue's
//! (`l10n/en-US/test_runs.ftl`).

use std::{
    collections::{HashMap, HashSet},
    rc::Rc,
};

use fe_runlog::{Outcome, TestId};
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
    runs::{RunId, RunKind, RunStatus, RunSummary},
    runs_panel::summary_text,
    test_commands::on_workbench,
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

/// A module of a run's tests, and how its tests ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Module {
    /// Its binary and the path of its tests' names before their last `::`,
    /// joined by `::`: empty for tests whose binary and name say none.
    pub(crate) path: String,
    /// Its tests, as indices into the results, in the order they came.
    pub(crate) tests: Vec<usize>,
    pub(crate) passed: usize,
    pub(crate) failed: usize,
}

/// `id`'s module path, as [`Module::path`], and its name in it: a doc
/// test's name is a file and an item, not a path, so it's in its binary's.
pub(crate) fn module_of(id: &TestId) -> (String, &str) {
    let (module, leaf) = match id.binary.starts_with("Doc-tests") {
        true => ("", id.name.as_str()),
        false => id.name.rsplit_once("::").unwrap_or(("", &id.name)),
    };
    let path = [id.binary.as_str(), module]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("::");
    (path, leaf)
}

/// `results`' modules, in the order their first tests came.
pub(crate) fn modules(results: &[TestResult]) -> Vec<Module> {
    let mut modules: Vec<Module> = Vec::new();
    for (ix, result) in results.iter().enumerate() {
        let (path, _) = module_of(&result.id);
        let at = match modules.iter().position(|module| module.path == path) {
            Some(at) => at,
            None => {
                modules.push(Module {
                    path,
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
pub(crate) fn expanded(module: &Module, opened: &HashMap<String, bool>) -> bool {
    (opened.get(&module.path).copied()).unwrap_or(module.failed > 0)
}

/// What a tree row is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RowKey {
    /// A module, by its path.
    Module(String),
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
    opened: &HashMap<String, bool>,
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
        let label: SharedString = match module.path.is_empty() {
            true => tr!("workbench-test-runs-module-top"),
            false => module.path.clone().into(),
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
            key: RowKey::Module(module.path.clone()),
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
type Toggle = Rc<dyn Fn(String, &mut Window, &mut App)>;

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
            RowKey::Module(path) => Some(path.clone()),
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
    opened: HashMap<String, bool>,
    /// The row selected, kept as the rows change.
    selected: Option<RowKey>,
    tree: Entity<List<TestRows>>,
    /// The split bar's drawn fractions, passed and failed, for tests.
    #[cfg(test)]
    pub(crate) drawn_split: Option<(f32, f32)>,
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
            if let (ListEvent::Confirmed(_), Some(RowKey::Module(path))) = (event, key) {
                view.toggle(&path, cx);
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

    /// Opens the module at `path` if it's closed, else closes it, and keeps
    /// it so.
    pub(crate) fn toggle(&mut self, path: &str, cx: &mut Context<Self>) {
        let rows = self.tree.read(cx).delegate().rows();
        let key = RowKey::Module(path.to_owned());
        let row = rows.iter().find(|row| row.key == key);
        let open = row.and_then(|row| row.expanded).unwrap_or(false);
        self.opened.insert(path.to_owned(), !open);
        self.rebuild(cx);
    }

    /// Opens (`open`) or closes the selected module, if a module's
    /// selected and it isn't so already.
    fn open_selected(&mut self, open: bool, cx: &mut Context<Self>) -> bool {
        let Some(RowKey::Module(path)) = self.selected.clone() else {
            return false;
        };
        let rows = self.tree.read(cx).delegate().rows();
        let row = rows
            .iter()
            .find(|row| row.key == RowKey::Module(path.clone()));
        if row.and_then(|row| row.expanded) == Some(open) {
            return false;
        }
        self.toggle(&path, cx);
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
    /// and its binary.
    fn slot(&self, c: &Tokens<Hsla>, code: SharedString) -> Option<gpui::Div> {
        let test = self.selected_test()?;
        let flaky = (self.tracks.get(&test.id)).is_some_and(|track| track.flaky.is_some());
        let glyph = Glyph::of(test.outcome, flaky);
        let slot = div()
            .debug_selector(|| SLOT_SELECTOR.into())
            .flex_none()
            .h(fe_ui::ROW_HEIGHT)
            .px_3()
            .flex()
            .items_center()
            .gap_2()
            .border_t_1()
            .border_color(c.border.subtle)
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
                    .font_family(code)
                    .text_size(CODE_SMALL.size)
                    .line_height(CODE_SMALL.line_height)
                    .text_color(c.text.primary)
                    .child(SharedString::from(test.id.name.clone())),
            )
            .child(
                div()
                    .flex_none()
                    .text_size(UI_SMALL.size)
                    .line_height(UI_SMALL.line_height)
                    .text_color(c.text.muted)
                    .child(SharedString::from(test.id.binary.clone())),
            );
        Some(slot)
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
        let slot = self.slot(&c, code.clone());
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
