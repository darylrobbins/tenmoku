// SPDX-License-Identifier: LicenseRef-Proprietary

//! `#[kiln::model]` and `#[kiln::model(persist)]`: a model of the app made
//! at start with no register call, and one kept in the state directory,
//! under its snake-case name or a `file = ".."`, restored on the next
//! start.

use std::{
    cell::Cell,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

use gpui::{AppContext as _, TestAppContext};
use kiln::core::{AppId, AppPaths, Os, PathStyle};
use kiln::{
    model::{self, persist::DEBOUNCE},
    platform::config::batch_channel,
    serde::{Deserialize, Serialize},
    testing::TestApp,
};

thread_local! {
    /// How many `Selection`s this test's thread made.
    static MADE: Cell<usize> = const { Cell::new(0) };
}

/// In memory only.
#[kiln::model]
struct Selection(Option<usize>);

impl Default for Selection {
    fn default() -> Self {
        MADE.set(MADE.get() + 1);
        Self(None)
    }
}

/// In `<state>/open_files.json`.
#[kiln::model(persist)]
#[derive(Default, Serialize, Deserialize)]
#[serde(crate = "kiln::serde")]
struct OpenFiles {
    paths: Vec<String>,
}

/// In `<state>/recent.json`.
#[kiln::model(persist, file = "recent")]
#[derive(Default, Serialize, Deserialize)]
#[serde(crate = "kiln::serde")]
enum Recent {
    #[default]
    Nothing,
    Last(String),
}

#[gpui::test]
fn a_model_is_there_from_start_with_no_register_call(cx: &mut TestAppContext) {
    let builder = kiln::app("com.example.Models", "Models").root(|_, cx| cx.new(|_| Empty).into());
    let mut app = TestApp::open(builder, cx);
    // Nothing read it yet; a read finds it made, and makes no other.
    assert_eq!(MADE.get(), 1, "made at start");
    app.cx().update(|_, cx| {
        model::update::<Selection, _>(cx, |selection, _| selection.0 = Some(3));
        assert_eq!(model::read::<Selection>(cx).0, Some(3));
    });
    assert_eq!(MADE.get(), 1, "a read made none");
}

struct Empty;

impl gpui::Render for Empty {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        gpui::div()
    }
}

fn start(home: &Path, cx: &mut TestAppContext) -> AppPaths {
    let config_dir = home.join("config");
    let home: OsString = home.into();
    let paths = AppPaths::resolve(
        &AppId::new("com.example.Models", "models", "Models"),
        PathStyle::Native,
        Os::current(),
        |name| match name {
            "HOME" | "APPDATA" | "LOCALAPPDATA" => Some(home.clone()),
            _ => None,
        },
    );
    let (_sender, batches) = batch_channel();
    let builder = kiln::app("com.example.Models", "Models")
        .paths(paths.clone())
        // No real watcher thread: GPUI's test scheduler is single-threaded.
        .config(move |config| config.user_files_in(config_dir).watch_with(batches))
        .root(|_, cx| cx.new(|_| Empty).into());
    cx.update(|cx| {
        builder.start(cx);
    });
    paths
}

#[gpui::test]
fn a_persisted_model_is_kept_by_name_or_file_and_restored_on_the_next_start(
    cx: &mut TestAppContext,
) {
    let home = TempDir::new();
    let paths = start(&home.0, cx);
    let state = paths.state_dir().expect("a state directory").to_owned();

    cx.update(|cx| {
        model::update::<OpenFiles, _>(cx, |open, _| open.paths.push("a.txt".into()));
        model::update::<Recent, _>(cx, |recent, _| *recent = Recent::Last("b.txt".into()));
    });
    cx.executor().advance_clock(DEBOUNCE * 2);
    cx.run_until_parked();

    let open: serde_json::Value =
        serde_json::from_slice(&fs::read(state.join("open_files.json")).expect("by its name"))
            .unwrap();
    assert_eq!(open["paths"][0], "a.txt");
    assert!(state.join("recent.json").exists(), "by its `file`");
    assert!(!state.join("recent_file.json").exists());

    // A second start, in a new app, reads them back.
    let mut second = gpui::TestAppContext::single();
    let _ = start(&home.0, &mut second);
    second.update(|cx| {
        assert_eq!(model::read::<OpenFiles>(cx).paths, ["a.txt"]);
        assert!(matches!(model::read::<Recent>(cx), Recent::Last(last) if last == "b.txt"));
    });
}

/// A directory of its own under the target's, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("macro_model-{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}
