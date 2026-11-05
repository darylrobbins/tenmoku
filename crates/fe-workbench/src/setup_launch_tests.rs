//! The setup wizard's first-launch offer and `app.set_up`, with temporary
//! config and state directories.

use std::{fs, path::PathBuf};

use fe_command::Invocation;
use fe_config::first_run::{self, Outcome};
use fe_gpui::{EditorSettings, EditorTheme, commands::RunCommand};
use gpui::{Focusable as _, TestAppContext, VisualTestContext, WindowHandle};

use crate::{
    Launch, SetupLaunch, Workbench, offer_setup, setup_wizard::SetupPaths, setup_wizard_model::Step,
};

struct Dirs {
    _dir: tempfile::TempDir,
    config: PathBuf,
    state: PathBuf,
}

impl Dirs {
    fn marker(&self) -> Option<String> {
        fs::read_to_string(self.state.join(first_run::MARKER_FILE)).ok()
    }
}

fn init(cx: &mut TestAppContext) -> Dirs {
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    fresh(cx)
}

/// New temporary config and state directories, for the sheet to use.
fn fresh(cx: &mut TestAppContext) -> Dirs {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config");
    let state = dir.path().join("state");
    fs::create_dir(&config).unwrap();
    let paths = SetupPaths {
        settings: Some(config.join("settings.toml")),
        state: Some(state.clone()),
    };
    cx.update(|cx| cx.set_global(paths));
    Dirs {
        _dir: dir,
        config,
        state,
    }
}

fn window(cx: &mut TestAppContext) -> WindowHandle<Workbench> {
    let window = cx.add_window(|window, cx| {
        window.activate_window();
        let launch = Launch {
            root: None,
            file: None,
        };
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    cx.run_until_parked();
    window
}

fn showing(window: WindowHandle<Workbench>, cx: &mut TestAppContext) -> bool {
    cx.run_until_parked();
    window
        .read_with(cx, |workbench, _| workbench.is_setup_showing())
        .unwrap()
}

fn offer(window: WindowHandle<Workbench>, launch: SetupLaunch, cx: &mut TestAppContext) {
    cx.update(|cx| offer_setup(window, launch, cx));
    cx.run_until_parked();
}

fn run(window: WindowHandle<Workbench>, command: &str, cx: &mut TestAppContext) {
    let invocation = Invocation::of(command).unwrap();
    let mut vcx = VisualTestContext::from_window(window.into(), cx);
    let focus = window
        .read_with(&vcx, |workbench, cx| workbench.focus_handle(cx))
        .unwrap();
    vcx.update(|window, cx| {
        window.focus(&focus, cx);
        window.dispatch_action(Box::new(RunCommand(invocation)), cx)
    });
    vcx.run_until_parked();
}

#[gpui::test]
fn first_launch_shows_once(cx: &mut TestAppContext) {
    let dirs = init(cx);
    let first = window(cx);
    offer(first, SetupLaunch::Normal, cx);
    assert!(showing(first, cx));
    assert!(dirs.marker().unwrap().contains("shown"));

    // Another window, or a relaunch, finds it decided.
    let second = window(cx);
    offer(second, SetupLaunch::Normal, cx);
    assert!(!showing(second, cx));
}

#[gpui::test]
fn never_and_existing(cx: &mut TestAppContext) {
    let dirs = init(cx);
    let first = window(cx);
    // A launch that shows nothing, on a new machine, records pending.
    offer(first, SetupLaunch::Never, cx);
    assert!(!showing(first, cx));
    assert!(dirs.marker().unwrap().contains("pending"));
    // It wrote layouts meanwhile, which a pending marker discounts.
    fs::create_dir_all(dirs.state.join("layouts")).unwrap();
    fs::write(dirs.state.join("layouts/key.json"), "{}").unwrap();
    offer(first, SetupLaunch::Normal, cx);
    assert!(showing(first, cx));
    assert!(dirs.marker().unwrap().contains("shown"));

    // With any marker, it writes nothing.
    let dirs = fresh(cx);
    first_run::record(&dirs.state, Outcome::Completed).unwrap();
    let before = dirs.marker();
    let second = window(cx);
    offer(second, SetupLaunch::Never, cx);
    assert!(!showing(second, cx));
    assert_eq!(dirs.marker(), before);

    // Existing use: nothing shows, and it's recorded.
    let dirs = fresh(cx);
    fs::write(
        dirs.config.join("settings.toml"),
        "[editor]\nfont_size = 15\n",
    )
    .unwrap();
    let third = window(cx);
    offer(third, SetupLaunch::Normal, cx);
    assert!(!showing(third, cx));
    assert!(dirs.marker().unwrap().contains("existing"));
}

#[gpui::test]
fn decides_off_the_main_thread(cx: &mut TestAppContext) {
    let dirs = init(cx);
    let first = window(cx);
    cx.update(|cx| offer_setup(first, SetupLaunch::Normal, cx));
    // Nothing has been decided or written until the background work runs.
    assert_eq!(dirs.marker(), None);
    assert!(!first.read_with(cx, |w, _| w.is_setup_showing()).unwrap());
    // The window closes before the decision lands.
    first
        .update(cx, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
    assert!(first.read_with(cx, |_, _| ()).is_err());
}

#[gpui::test]
fn command_reopens(cx: &mut TestAppContext) {
    let dirs = init(cx);
    fs::write(
        dirs.config.join("settings.toml"),
        "[editor]\nfont_size = 15\n",
    )
    .unwrap();
    first_run::record(&dirs.state, Outcome::Completed).unwrap();
    let first = window(cx);
    let second = window(cx);
    run(first, "app.set_up", cx);
    assert!(showing(first, cx));
    assert!(dirs.marker().unwrap().contains("completed"));
    first
        .read_with(cx, |w, cx| {
            assert_eq!(w.setup().unwrap().read(cx).current_step(), Step::Appearance)
        })
        .unwrap();

    // One sheet in the app: another window activates the first.
    run(second, "app.set_up", cx);
    assert!(showing(first, cx));
    assert!(!showing(second, cx));
}

#[gpui::test]
fn files_open_behind(cx: &mut TestAppContext) {
    let dirs = init(cx);
    let file = dirs.config.join("a.txt");
    fs::write(&file, "alpha\n").unwrap();
    let first = window(cx);
    offer(first, SetupLaunch::Normal, cx);
    assert!(showing(first, cx));
    let tabs = |cx: &mut TestAppContext| {
        first
            .read_with(cx, |w, cx| w.pane().read(cx).tabs().len())
            .unwrap()
    };
    let before = tabs(cx);
    first
        .update(cx, |w, window, cx| {
            w.open_document(file.clone(), window, cx)
        })
        .unwrap();
    cx.run_until_parked();
    assert_eq!(tabs(cx), before + 1);
    assert!(showing(first, cx));
    let mut vcx = VisualTestContext::from_window(first.into(), cx);
    let focused = first
        .read_with(&vcx, |w, cx| w.setup().unwrap().read(cx).focus_handle())
        .unwrap();
    vcx.update(|window, _| assert!(focused.is_focused(window)));
}

#[cfg(target_os = "macos")]
#[gpui::test]
fn ready_installs_the_shell_command(cx: &mut TestAppContext) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static INSTALLS: AtomicUsize = AtomicUsize::new(0);
    fn handler(install: bool, _: &mut gpui::App) {
        assert!(install);
        INSTALLS.fetch_add(1, Ordering::SeqCst);
    }
    init(cx);
    cx.update(|cx| Workbench::set_shell_command_handler(cx, handler));
    let first = window(cx);
    run(first, "app.set_up", cx);
    let mut vcx = VisualTestContext::from_window(first.into(), cx);
    let click = |selector: &'static str, vcx: &mut VisualTestContext| {
        let at = vcx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector}"))
            .center();
        for up in [false, true] {
            if up {
                vcx.simulate_event(gpui::MouseUpEvent {
                    position: at,
                    modifiers: gpui::Modifiers::none(),
                    button: gpui::MouseButton::Left,
                    click_count: 1,
                });
            } else {
                vcx.simulate_event(gpui::MouseDownEvent {
                    position: at,
                    modifiers: gpui::Modifiers::none(),
                    button: gpui::MouseButton::Left,
                    click_count: 1,
                    first_mouse: false,
                });
            }
        }
        vcx.run_until_parked();
        vcx.update(|window, cx| window.draw(cx).clear(cx));
        vcx.run_until_parked();
    };
    vcx.update(|window, cx| window.draw(cx).clear(cx));
    for _ in 0..4 {
        click("setup-primary", &mut vcx);
    }
    click("setup-install-shell", &mut vcx);
    assert_eq!(INSTALLS.load(Ordering::SeqCst), 1);
}

/// The sheet over a window `width` by `height`, on Appearance.
fn small(width: f32, height: f32, cx: &mut TestAppContext) -> VisualTestContext {
    init(cx);
    let first = window(cx);
    run(first, "app.set_up", cx);
    let mut vcx = VisualTestContext::from_window(first.into(), cx);
    vcx.simulate_resize(gpui::size(gpui::px(width), gpui::px(height)));
    draw(&mut vcx);
    vcx
}

fn draw(cx: &mut VisualTestContext) {
    for _ in 0..3 {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
    cx.run_until_parked();
}

#[gpui::test]
fn narrow_window_keeps_the_sample_and_tiles(cx: &mut TestAppContext) {
    let mut vcx = small(500., 760., cx);
    let card = vcx.debug_bounds("setup-card").unwrap();
    let sample = vcx.debug_bounds("setup-sample").unwrap();
    assert!(sample.size.width > gpui::px(100.), "{sample:?}");
    assert!(sample.size.height > gpui::px(60.), "{sample:?}");
    assert!(sample.right() <= card.right(), "{sample:?} {card:?}");
    for option in 0..2 {
        let tile = vcx
            .debug_bounds(Box::leak(
                format!("setup-radio-DarkTheme-{option}").into_boxed_str(),
            ))
            .unwrap();
        assert!(
            tile.left() >= card.left() && tile.right() <= card.right(),
            "{tile:?}"
        );
    }
}

#[gpui::test]
fn short_window_keeps_the_chosen_tile_in_view(cx: &mut TestAppContext) {
    let mut vcx = small(900., 360., cx);
    let window = vcx.update(|window, _| window.window_handle().downcast::<Workbench>().unwrap());
    let stop = |vcx: &mut VisualTestContext| {
        window
            .read_with(vcx, |w, cx| w.setup().unwrap().read(cx).current_stop())
            .unwrap()
    };
    for _ in 0..4 {
        if stop(&mut vcx)
            == crate::setup_wizard::Stop::Control(crate::setup_wizard_model::ControlId::DarkTheme)
        {
            break;
        }
        vcx.simulate_keystrokes("tab");
        draw(&mut vcx);
    }
    let count = window
        .read_with(&vcx, |w, cx| {
            let sheet = w.setup().unwrap().read(cx);
            sheet
                .control(crate::setup_wizard_model::ControlId::DarkTheme)
                .unwrap()
                .options
                .len()
        })
        .unwrap();
    for _ in 0..count {
        let chosen = window
            .read_with(&vcx, |w, cx| {
                let sheet = w.setup().unwrap().read(cx);
                sheet
                    .control(crate::setup_wizard_model::ControlId::DarkTheme)
                    .unwrap()
                    .selected
            })
            .unwrap();
        let tile = vcx
            .debug_bounds(Box::leak(
                format!("setup-radio-DarkTheme-{chosen}").into_boxed_str(),
            ))
            .unwrap();
        let card = vcx.debug_bounds("setup-card").unwrap();
        let footer = vcx.debug_bounds("setup-skip").unwrap();
        assert!(
            tile.bottom() <= footer.top(),
            "tile {chosen}: {tile:?} {footer:?}"
        );
        assert!(tile.top() >= card.top(), "tile {chosen}: {tile:?}");
        vcx.simulate_keystrokes("right");
        draw(&mut vcx);
    }
}

#[gpui::test]
fn offer_leaves_a_sheet_that_shows_elsewhere(cx: &mut TestAppContext) {
    let dirs = init(cx);
    let first = window(cx);
    let second = window(cx);
    run(first, "app.set_up", cx);
    assert!(showing(first, cx));
    offer(second, SetupLaunch::Normal, cx);
    assert!(dirs.marker().unwrap().contains("shown"));
    assert!(!showing(second, cx));
    assert!(showing(first, cx));
}

#[gpui::test]
fn begin_then_offer_shows_once(cx: &mut TestAppContext) {
    let dirs = init(cx);
    let first = window(cx);
    cx.update(|cx| crate::begin_setup(SetupLaunch::Normal, cx));
    offer(first, SetupLaunch::Normal, cx);
    assert!(showing(first, cx));
    assert!(dirs.marker().unwrap().contains("shown"));
}

#[cfg(target_os = "macos")]
#[gpui::test]
fn install_is_a_tab_stop_that_dispatches(cx: &mut TestAppContext) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static INSTALLS: AtomicUsize = AtomicUsize::new(0);
    fn handler(_: bool, _: &mut gpui::App) {
        INSTALLS.fetch_add(1, Ordering::SeqCst);
    }
    init(cx);
    cx.update(|cx| Workbench::set_shell_command_handler(cx, handler));
    let first = window(cx);
    run(first, "app.set_up", cx);
    let mut vcx = VisualTestContext::from_window(first.into(), cx);
    draw(&mut vcx);
    for _ in 0..4 {
        vcx.simulate_keystrokes("enter");
        draw(&mut vcx);
    }
    // On Ready, Tab from Start Editing wraps to Install; Enter presses it.
    vcx.simulate_keystrokes("tab");
    draw(&mut vcx);
    vcx.simulate_keystrokes("enter");
    draw(&mut vcx);
    assert_eq!(INSTALLS.load(Ordering::SeqCst), 1);
}

/// What a fresh launch writes under `state`: the production layout launch on
/// the store, its claim's file and a layout write, and a folder in recent.
fn launch_writes(state: &std::path::Path) -> Vec<impl Sized> {
    let store = crate::window_layouts(state.join("layouts"), None);
    let windows = kiln::shell::layout::launch(&store, true);
    assert_eq!(windows.len(), 1);
    let claim = store.claim_new().unwrap();
    store
        .write(&claim, serde_json::json!({"bounds": null}))
        .unwrap();
    fe_config::recent::record(
        &state.join("recent.toml"),
        fe_config::recent::Entry::Folder(std::path::Path::new("/tmp/folder")),
    )
    .unwrap();
    windows
}

#[gpui::test]
fn launch_writes_are_not_prior_use(cx: &mut TestAppContext) {
    // The background decision is held until after the launch's own writes.
    let dirs = init(cx);
    let first = window(cx);
    let (release, gate) = futures::channel::oneshot::channel();
    cx.update(|cx| {
        cx.set_global(crate::setup_launch::SetupGate(std::sync::Mutex::new(Some(
            gate,
        ))));
        crate::begin_setup(SetupLaunch::Normal, cx);
    });
    let _windows = launch_writes(&dirs.state);
    assert!(
        dirs.state
            .join("layouts")
            .read_dir()
            .unwrap()
            .next()
            .is_some()
    );
    release.send(()).unwrap();
    offer(first, SetupLaunch::Normal, cx);
    assert!(showing(first, cx));
    assert!(dirs.marker().unwrap().contains("shown"));

    // A returning user's files, written before the launch, still count.
    let dirs = fresh(cx);
    // Written by an earlier launch: this one never notes them.
    fs::create_dir_all(dirs.state.join("layouts")).unwrap();
    fs::write(dirs.state.join("layouts/key.json"), "{}").unwrap();
    fs::write(dirs.state.join("recent.toml"), "folders = []\n").unwrap();
    let second = window(cx);
    cx.update(|cx| crate::begin_setup(SetupLaunch::Normal, cx));
    offer(second, SetupLaunch::Normal, cx);
    assert!(!showing(second, cx));
    assert!(dirs.marker().unwrap().contains("existing"));
}

#[cfg(target_os = "macos")]
#[gpui::test]
fn install_stop_scrolls_into_view_in_a_short_window(cx: &mut TestAppContext) {
    let mut vcx = small(900., 300., cx);
    for _ in 0..4 {
        vcx.simulate_keystrokes("enter");
        draw(&mut vcx);
    }
    vcx.simulate_keystrokes("tab");
    draw(&mut vcx);
    let install = vcx.debug_bounds("setup-install-shell").unwrap();
    let footer = vcx.debug_bounds("setup-skip").unwrap();
    let card = vcx.debug_bounds("setup-card").unwrap();
    assert!(install.bottom() <= footer.top(), "{install:?} {footer:?}");
    assert!(install.top() >= card.top(), "{install:?} {card:?}");
}
