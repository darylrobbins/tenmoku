// SPDX-License-Identifier: LicenseRef-Proprietary

//! `kiln::model::persist`: a model kept in the state directory, loaded at
//! start, written debounced off the main thread and at quit.

use std::{
    fs,
    path::Path,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use gpui::TestAppContext;
use kiln::{
    model::{
        self,
        persist::{DEBOUNCE, StateDir, persist, persist_with},
    },
    serde::{Deserialize, Serialize},
};

#[derive(Default, Serialize, Deserialize, Debug, PartialEq)]
#[serde(crate = "kiln::serde")]
struct Recent {
    items: Vec<String>,
}

/// The writes made: whether each ran on the main thread, and what it wrote.
type Writes = Arc<Mutex<Vec<(bool, Vec<u8>)>>>;

fn start(dir: &Path, cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.set_global(StateDir(dir.to_owned()));
        persist::<Recent>(cx, "recent");
    });
}

/// Starts with every write recorded, each taking `delay`.
fn start_recording(dir: &Path, delay: Duration, cx: &mut TestAppContext) -> Writes {
    let writes = Writes::default();
    let log = writes.clone();
    let executor = cx.executor().clone();
    cx.update(|cx| {
        cx.set_global(StateDir(dir.to_owned()));
        persist_with::<Recent>(cx, "recent", move |path, bytes| {
            thread::sleep(delay);
            fs::write(path, bytes)?;
            log.lock()
                .unwrap()
                .push((executor.is_main_thread(), bytes.to_vec()));
            Ok(())
        });
    });
    writes
}

fn push(cx: &mut TestAppContext, item: &str) {
    cx.update(|cx| model::update::<Recent, _>(cx, |m, _| m.items.push(item.into())));
}

fn saved(dir: &Path) -> Option<Recent> {
    let text = fs::read(dir.join("recent.json")).ok()?;
    Some(serde_json::from_slice(&text).expect("what's saved parses"))
}

fn saved_items(dir: &Path) -> Option<Vec<String>> {
    saved(dir).map(|recent| recent.items)
}

fn items(cx: &mut TestAppContext) -> Vec<String> {
    cx.update(|cx| model::read::<Recent>(cx).items.clone())
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| item.to_string()).collect()
}

#[gpui::test]
fn a_burst_of_changes_is_one_write_after_the_debounce(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let writes = start_recording(dir.path(), Duration::ZERO, cx);
    push(cx, "a");
    cx.executor().advance_clock(DEBOUNCE / 2);
    push(cx, "b");
    push(cx, "c");
    cx.executor()
        .advance_clock(DEBOUNCE - Duration::from_millis(1));
    cx.run_until_parked();
    cx.executor().allow_parking();
    thread::sleep(Duration::from_millis(50));
    assert_eq!(
        writes.lock().unwrap().len(),
        0,
        "nothing is written before the debounce"
    );

    cx.executor().advance_clock(Duration::from_millis(2));
    cx.run_until_parked();
    for _ in 0..300 {
        if !writes.lock().unwrap().is_empty() {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    // Let a second write show itself, were there one.
    thread::sleep(Duration::from_millis(100));
    cx.run_until_parked();
    let writes = writes.lock().unwrap();
    assert_eq!(writes.len(), 1, "the burst is one write");
    assert!(!writes[0].0, "written off the main thread");
    let written: Recent = serde_json::from_slice(&writes[0].1).unwrap();
    assert_eq!(written.items, strings(&["a", "b", "c"]));
    assert_eq!(saved_items(dir.path()), Some(strings(&["a", "b", "c"])));
}

#[gpui::test]
fn quit_waits_for_the_latest_value_to_be_written(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    // A slow write, so that returning before it's done would show.
    start_recording(dir.path(), Duration::from_millis(200), cx);
    push(cx, "a");
    push(cx, "b");
    assert_eq!(saved(dir.path()), None, "not yet written");

    cx.update(|cx| cx.shutdown());
    assert_eq!(saved_items(dir.path()), Some(strings(&["a", "b"])));
}

#[gpui::test]
fn quit_writes_a_change_made_in_the_same_update(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    start(dir.path(), cx);
    cx.update(|cx| {
        model::update::<Recent, _>(cx, |m, _| m.items.push("a".into()));
        cx.shutdown();
    });
    assert_eq!(saved_items(dir.path()), Some(strings(&["a"])));
}

#[gpui::test]
fn an_unchanged_model_writes_nothing_at_quit(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    start(dir.path(), cx);
    cx.update(|cx| cx.shutdown());
    assert_eq!(saved(dir.path()), None);
}

#[gpui::test]
fn a_model_a_module_registers_is_persisted(cx: &mut TestAppContext) {
    use kiln::{
        Contributions,
        core::{AppId, AppPaths, Os, PathStyle},
        gpui::{AnyView, AppContext as _, Context, IntoElement, Render, Window, div},
    };

    struct Blank;
    impl Render for Blank {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    fs::create_dir_all(&state).unwrap();
    fs::write(state.join("recent.json"), r#"{"items":["x"]}"#).unwrap();
    let state_var: std::ffi::OsString = state.clone().into();
    let paths = AppPaths::resolve(
        &AppId::new("dev.kiln.Persist", "kiln-persist", "Persist"),
        PathStyle::Native,
        Os::current(),
        |name| (name == "KILN_PERSIST_STATE_DIR").then(|| state_var.clone()),
    );
    let (_, closed) = kiln::platform::config::batch_channel();
    let app = kiln::app("dev.kiln.Persist", "Persist")
        .paths(paths)
        .root(|_, cx| AnyView::from(cx.new(|_| Blank)))
        .contributions(|cx| {
            Contributions::add_model(cx, |cx| persist::<Recent>(cx, "recent"));
        })
        .config(move |config| config.watch_with(closed));
    cx.update(|cx| app.start(cx));
    cx.run_until_parked();

    assert_eq!(items(cx), ["x"], "loaded from the state directory");
    push(cx, "y");
    cx.update(|cx| cx.shutdown());
    assert_eq!(saved_items(&state), Some(strings(&["x", "y"])));
}

#[gpui::test]
fn a_later_start_loads_the_value(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("recent.json"), r#"{"items":["x","y"]}"#).unwrap();
    start(dir.path(), cx);
    assert_eq!(items(cx), ["x", "y"]);
}

#[gpui::test]
fn a_missing_file_starts_from_default(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    start(dir.path(), cx);
    assert_eq!(items(cx), Vec::<String>::new());
    assert!(!dir.path().join("recent.json.bad").exists());
}

#[gpui::test]
fn an_unparsable_file_is_set_aside(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("recent.json"), "{ not json").unwrap();
    start(dir.path(), cx);
    assert_eq!(items(cx), Vec::<String>::new());
    assert!(!dir.path().join("recent.json").exists());
    assert_eq!(
        fs::read_to_string(dir.path().join("recent.json.bad")).unwrap(),
        "{ not json"
    );
}
