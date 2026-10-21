//! Standalone fastedit host: `fastedit [--perf] [folder or file]`, a
//! [`Workbench`] in a window.

mod app_icon;
mod config;
mod perf;
mod theme;
mod vim_remaps;

use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use config::{config, log_keymap_report};
use perf::Startup;
use vim_remaps::VimRemaps;

use fe_buffer::Buffer;
use fe_config::{Diagnostic, Settings, SettingsEvent, SettingsStore, settings::VimClipboard};
use fe_gpui::EditorTheme;
use fe_workbench::{
    Launch, RootChanged, Workbench,
    loader::{self, Opened},
};
use gpui::{
    App, AppContext as _, Entity, Focusable as _, Global, Window, WindowBounds, WindowHandle,
    WindowId, point, px,
};
use kiln::{
    GlobalSettings,
    command::Value,
    core::layout::LayoutStore,
    prelude::{AppExt as _, WindowExt as _},
    shell::layout::WindowLayout,
};
use kiln_platform::{Capabilities, WindowSpec, window_options};

/// What a finished read produced: a buffer, a missing file being a new
/// one, which saving creates; or a large file's read-only view. The
/// workbench shows an error in the tab's place.
fn loaded(read: loader::Read, started: Instant) -> io::Result<Opened> {
    perf::file_loaded(read.at);
    match read.result? {
        Opened::Text(buffer) => {
            let buffer = buffer.unwrap_or_else(|| Buffer::new(""));
            log::info!(
                "loaded {} bytes, {} rows in {:.1?}",
                buffer.len(),
                buffer.text().row_count(),
                started.elapsed()
            );
            Ok(buffer.into())
        }
        Opened::Large { view, index } => {
            log::info!(
                "viewing {} bytes read-only, opened in {:.1?}",
                view.len(),
                started.elapsed()
            );
            Ok(Opened::Large { view, index })
        }
    }
}

/// Waits for `reading` off the main thread, then shows the file in the
/// window.
fn open_in_background(
    reading: loader::Reading,
    window: WindowHandle<Workbench>,
    started: Instant,
    cx: &mut App,
) {
    let path = reading.path().to_owned();
    let read = cx.background_spawn(async move { reading.finish() });
    cx.spawn(async move |cx| {
        let result = loaded(read.await, started);
        window
            .update(cx, |workbench, window, cx| {
                workbench.file_read(path, result, window, cx)
            })
            .ok();
    })
    .detach();
}

/// The settings store: the user's settings file and, with a folder open,
/// the project's.
fn load_settings(root: Option<&Path>) -> SettingsStore {
    let project = root.map(fe_config::project_settings_path);
    SettingsStore::load(fe_config::user_settings_path(), project)
}

/// Each window's settings: the user's file under its root's project file
/// (`app-shell.md` §4.1). Kiln's config service loads and watches the
/// launch root's; a window on another root reads its own off the main
/// thread when it opens, when it's re-rooted, and when the user's file
/// reloads.
#[derive(Default)]
struct WindowSettings {
    /// The root the config service's project layer is for.
    launch_root: Option<PathBuf>,
    /// What each window was last given, as the next event's `previous`.
    given: HashMap<WindowId, Arc<Settings>>,
}

impl Global for WindowSettings {}

/// The settings as the config service has them now.
fn current_settings(cx: &App) -> Arc<Settings> {
    cx.try_global::<GlobalSettings<Settings>>()
        .map(|global| global.settings().clone())
        .unwrap_or_default()
}

/// Applies each settings reload of kiln's config service to every
/// window: the theme mode and vim when the file changed them, then each
/// workbench's editors, status bar and language servers, with its root's
/// project layer. The service has logged the reload and its diagnostics.
fn follow_settings(launch_root: Option<PathBuf>, cx: &mut App) {
    cx.set_global(WindowSettings {
        launch_root,
        given: HashMap::new(),
    });
    cx.observe_global::<GlobalSettings<Settings>>(move |cx| {
        let global = cx.global::<GlobalSettings<Settings>>();
        // The settings as loaded, before any reload: nothing to apply.
        if Arc::ptr_eq(global.settings(), global.previous()) {
            return;
        }
        let (now, before) = (global.settings(), global.previous());
        let jump_on_s = now.vim.jump_on_s;
        let replace_with_register = now.vim.replace_with_register;
        let now_limit = now.vim.regex_time_limit;
        let unnamedplus = |s: &Settings| s.vim.clipboard == VimClipboard::Unnamedplus;
        let (enabled, unnamed) = (
            (now.vim.enabled, before.vim.enabled),
            (unnamedplus(now), unnamedplus(before)),
        );
        let mode = (now.theme.mode != before.theme.mode).then_some(now.theme.mode);
        let diagnostics = global.diagnostics().to_vec();
        if let Some(mode) = mode {
            cx.set_theme_mode(theme::mode(mode));
        }
        fe_gpui::apply_vim_reload(cx, enabled, unnamed);
        fe_gpui::set_vim_jump_on_s(cx, jump_on_s);
        fe_gpui::set_vim_replace_with_register(cx, replace_with_register);
        fe_gpui::set_vim_time_limit(Duration::from_millis(now_limit));
        for window in cx.windows() {
            if let Some(window) = window.downcast::<Workbench>() {
                settle(window, diagnostics.clone(), cx);
            }
        }
    })
    .detach();
}

/// Gives `window` the settings of its root: the config service's for the
/// launch root (or none), else its root's own, read off the main thread.
fn settle(window: WindowHandle<Workbench>, diagnostics: Vec<Diagnostic>, cx: &mut App) {
    let Ok(root) = window.read_with(cx, |workbench, _| workbench.root().map(Path::to_owned)) else {
        return;
    };
    let launch_root = cx
        .try_global::<WindowSettings>()
        .and_then(|settings| settings.launch_root.clone());
    if root.is_none() || root == launch_root {
        let settings = current_settings(cx);
        return give(window, settings, diagnostics, cx);
    }
    let read =
        cx.background_spawn(async move { load_settings(root.as_deref()).settings().clone() });
    cx.spawn(async move |cx| {
        let settings = read.await;
        cx.update(|cx| give(window, settings, diagnostics, cx));
    })
    .detach();
}

/// Applies `settings` to `window`'s workbench, against what it was given
/// last.
fn give(
    window: WindowHandle<Workbench>,
    settings: Arc<Settings>,
    diagnostics: Vec<Diagnostic>,
    cx: &mut App,
) {
    let id = window.window_id();
    let previous = cx
        .default_global::<WindowSettings>()
        .given
        .insert(id, settings.clone());
    let previous = previous.unwrap_or_else(|| current_settings(cx));
    if previous == settings {
        return;
    }
    let event = SettingsEvent {
        settings,
        previous,
        diagnostics,
    };
    window
        .update(cx, |workbench, _, cx| workbench.reload_settings(&event, cx))
        .ok();
}

/// Has a new window follow its root's settings: now, and whenever the
/// workbench is re-rooted.
fn watch_window(window: WindowHandle<Workbench>, settings: Arc<Settings>, cx: &mut App) {
    let id = window.window_id();
    cx.default_global::<WindowSettings>()
        .given
        .insert(id, settings);
    if let Ok(workbench) = window.update(cx, |_, _, cx| cx.entity()) {
        cx.subscribe(&workbench, move |_, _: &RootChanged, cx| {
            settle(window, Vec::new(), cx)
        })
        .detach();
    }
    settle(window, Vec::new(), cx);
    cx.on_window_closed(move |cx, closed| {
        if closed == id {
            cx.default_global::<WindowSettings>().given.remove(&id);
        }
    })
    .detach();
}

/// Opens a window on `root` (`window.new`, Open Folder in New Window): the
/// root's newest saved window no live window holds, or else a fresh one,
/// a little below and right of the active window.
fn open_window_on(root: Option<PathBuf>, cx: &mut App) {
    let settings = current_settings(cx);
    let theme = theme::editor_theme(&settings.editor, cx);
    let capabilities = Capabilities::current();
    let (layout, entry) = match layout_store(root.as_deref()) {
        Some(store) => match kiln::shell::layout::new_window(&store) {
            Some((layout, entry)) => (Some(layout), entry),
            None => (None, None),
        },
        None => (None, None),
    };
    let mut options = window_options(&WindowSpec::main("fastedit"), &capabilities, cx);
    // The active window, or the last opened where the platform has none
    // (an X server without a window manager).
    let from = cx.active_window().or_else(|| cx.windows().last().copied());
    let cascaded = from.and_then(|active| {
        active
            .update(cx, |_, window, _| window.window_bounds())
            .ok()
    });
    if let Some(bounds) = entry.as_ref().and_then(saved_bounds) {
        options.window_bounds = Some(bounds);
    } else if let Some(WindowBounds::Windowed(mut bounds)) = cascaded {
        bounds.origin += point(px(40.), px(40.));
        options.window_bounds = Some(WindowBounds::Windowed(bounds));
    }
    let launch = Launch { root, file: None };
    let given = settings.clone();
    let opened = cx.open_window(options, |window, cx| {
        new_workbench(launch, &settings, theme, layout, entry, window, cx)
    });
    match opened {
        Ok(window) => watch_window(window, given, cx),
        Err(error) => log::error!("can't open a window: {error}"),
    }
}

/// The windows a launch on `root` opens, each with its layout (unless
/// there's no state directory) and the entry it restores, if any: see
/// [`kiln::shell::layout::launch`]. A folder launch (`folder`) restores the
/// root's saved windows, and a launch with no argument (no root) those of
/// the last such launch; a file launch restores nothing.
fn launch_windows(root: Option<&Path>, folder: bool) -> Vec<(Option<WindowLayout>, Option<Value>)> {
    let windows: Vec<_> = (layout_store(root).iter())
        .flat_map(|store| kiln::shell::layout::launch(store, folder))
        .map(|(layout, entry)| (Some(layout), entry))
        .collect();
    if windows.is_empty() {
        return vec![(None, None)];
    }
    windows
}

/// The saved layouts of the windows on the folder `root`, or of those with
/// no root, if there's a state directory.
fn layout_store(root: Option<&Path>) -> Option<LayoutStore> {
    let dir = fe_config::paths::app_paths().layouts_dir()?;
    let root = root.map(kiln::core::trust::canonical);
    Some(fe_workbench::window_layouts(dir, root.as_deref()))
}

/// Where a saved window was.
fn saved_bounds(entry: &Value) -> Option<WindowBounds> {
    kiln::shell::layout::restore_bounds(entry.get("bounds")?)
}

/// A workbench for `launch`, in fastedit's chrome for this platform, with
/// `settings`, saving its layout in `layout` and restoring `entry`.
fn new_workbench(
    launch: Launch,
    settings: &Settings,
    theme: EditorTheme,
    layout: Option<WindowLayout>,
    entry: Option<Value>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<Workbench> {
    // fastedit's windows aren't kiln `Root`s, which do this themselves.
    window.follow_system_appearance(cx);
    let capabilities = Capabilities::current();
    let editor_settings = settings.editor.clone();
    let status_bar = settings.status_bar.visible;
    let workbench = cx.new(|cx| {
        let mut workbench = Workbench::new(launch, theme, editor_settings, window, cx);
        // The unified title bar is macOS's. Elsewhere the menus are drawn
        // in the window, and the frame too where the window manager leaves
        // it to the app (GNOME on Wayland).
        workbench.set_title_bar(capabilities.unified_title_bar, cx);
        workbench.set_menu_bar(!capabilities.native_menus, cx);
        workbench.set_window_frame(!capabilities.unified_title_bar, cx);
        workbench.set_status_bar_visible(status_bar, cx);
        workbench.set_terminal_settings(&settings.terminal, cx);
        if let Some(entry) = &entry {
            workbench.restore_layout(entry, window, cx);
        }
        if let Some(layout) = layout {
            workbench.set_layout(layout, window, cx);
        }
        workbench
    });
    // Focus before the window's first draw: focusing after it would make
    // the first frame request draw again.
    let focus = workbench.read(cx).focus_handle(cx);
    window.focus(&focus, cx);
    workbench
}

/// Logs to stderr: warnings and errors, and fastedit's own info. `RUST_LOG`
/// sets one level for everything instead (`debug`, `trace`, …).
struct StderrLogger {
    level: Option<log::LevelFilter>,
}

impl log::Log for StderrLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        let target = metadata.target();
        let ours = target.starts_with("fastedit") || target.starts_with("fe_");
        let level = self.level.unwrap_or(if ours {
            log::LevelFilter::Info
        } else {
            log::LevelFilter::Warn
        });
        metadata.level() <= level
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            eprintln!("[{} {}] {}", record.level(), record.target(), record.args());
        }
    }

    fn flush(&self) {}
}

fn init_logging() {
    let level = std::env::var("RUST_LOG")
        .ok()
        .and_then(|level| level.parse().ok());
    if log::set_logger(Box::leak(Box::new(StderrLogger { level }))).is_ok() {
        log::set_max_level(level.unwrap_or(log::LevelFilter::Info));
    }
}

fn main() {
    let mut startup = Startup::new();
    let started = Instant::now();
    init_logging();
    let mut args: Vec<_> = std::env::args_os().skip(1).collect();
    let perf = args.first().is_some_and(|arg| arg == "--perf");
    if perf {
        args.remove(0);
    }
    let mut launch = fe_workbench::resolve_launch(args.into_iter().next().map(PathBuf::from));
    let reading = launch.file.take().map(|(path, _)| loader::start(path));
    let store = load_settings(launch.root.as_deref());
    let settings: Arc<Settings> = store.settings().clone();

    let application = kiln::application();
    startup.platform = Some(Instant::now());
    application.run(move |cx: &mut App| {
        startup.app = Some(Instant::now());
        app_icon::set();
        // Commands, keys, menus, in-window prompts where there are no
        // native alerts, and the config files watched from here on: the
        // vimrc and `vim.remap` too, which give every editor its remaps.
        let remaps = VimRemaps::new(fe_config::paths::user_vimrc_path(), &store);
        // Extensions' language servers start only once the user consents,
        // which is recorded beside the other state files; set before the
        // extensions load, so their servers are checked against it.
        if let Some(dir) = fe_config::paths::state_dir() {
            fe_workbench::set_extension_consent_file(cx, dir.join(fe_workbench::CONSENT_FILE));
        }
        kiln::init(cx, config(store).source(remaps));
        // The editor's colours, from kiln's theme from now on.
        theme::init(cx);
        fe_gpui::set_vim_enabled(cx, settings.vim.enabled);
        fe_gpui::set_vim_clipboard(cx, settings.vim.clipboard == VimClipboard::Unnamedplus);
        fe_gpui::set_vim_jump_on_s(cx, settings.vim.jump_on_s);
        fe_gpui::set_vim_replace_with_register(cx, settings.vim.replace_with_register);
        fe_gpui::set_vim_time_limit(Duration::from_millis(settings.vim.regex_time_limit));
        fe_workbench::set_lsp_settings(cx, settings.lsp.clone());
        // Toggle Vim Mode and Toggle Status Bar save to the user's file,
        // which the watcher then reloads.
        if let Some(path) = fe_config::user_settings_path() {
            fe_workbench::save_toggles_to(cx, path);
        }
        // Language servers start only in folders the user trusts, which the
        // workbench asks about on a folder's first open.
        if let Some(path) = fe_config::paths::trust_path() {
            fe_workbench::set_trust_file(cx, path);
        }
        log_keymap_report(cx);
        let capabilities = Capabilities::current();
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        let theme = theme::editor_theme(&settings.editor, cx);
        fe_gpui::set_restore_style(cx, theme.clone(), settings.editor.clone());
        // A folder launch reopens its saved windows, and a launch with no
        // argument the last such launch's; a file launch opens the file, in
        // a window that saves its layout from now on.
        let mut windows = launch_windows(launch.root.as_deref(), reading.is_none());
        let (layout, entry) = windows.remove(0);
        let root = launch.root.clone();
        let launch_root = launch.root.clone();
        let mut failed = None;
        // The launch file's read hadn't finished as the window opened.
        let mut loading = false;
        let mut options = window_options(&WindowSpec::main("fastedit"), &capabilities, cx);
        if let Some(bounds) = entry.as_ref().and_then(saved_bounds) {
            options.window_bounds = Some(bounds);
        }
        let window = cx
            .open_window(options, |window, cx| {
                startup.view = Some(Instant::now());
                // Show the file on the first frame if it's read by now.
                // A read that failed shows its error once the window is
                // up.
                launch.file = reading.as_ref().map(|reading| {
                    let opened = match reading.try_finish().map(|read| loaded(read, started)) {
                        Some(Ok(opened)) => Some(opened),
                        Some(Err(error)) => {
                            failed = Some(error);
                            None
                        }
                        None => {
                            loading = true;
                            None
                        }
                    };
                    (reading.path().to_owned(), opened)
                });
                new_workbench(launch, &settings, theme.clone(), layout, entry, window, cx)
            })
            .expect("failed to open the main window");
        startup.window = Some(Instant::now());
        if perf {
            window
                .update(cx, |_, window, cx| {
                    perf::start(cx.entity(), startup, window)
                })
                .ok();
        }
        if let Some(reading) = reading {
            match failed {
                Some(error) => {
                    let path = reading.path().to_owned();
                    window
                        .update(cx, |workbench, window, cx| {
                            workbench.file_read(path, Err(error), window, cx)
                        })
                        .ok();
                }
                None if loading => open_in_background(reading, window, started, cx),
                None => {}
            }
        }
        follow_settings(launch_root.clone(), cx);
        watch_window(window, settings.clone(), cx);
        fe_workbench::set_window_opener(cx, open_window_on);
        // The root's other saved windows.
        for (layout, entry) in windows {
            let launch = Launch {
                root: root.clone(),
                file: None,
            };
            let theme = theme.clone();
            let mut options = window_options(&WindowSpec::main("fastedit"), &capabilities, cx);
            if let Some(bounds) = entry.as_ref().and_then(saved_bounds) {
                options.window_bounds = Some(bounds);
            }
            let opened = cx.open_window(options, |window, cx| {
                new_workbench(launch, &settings, theme, layout, entry, window, cx)
            });
            match opened {
                Ok(window) => watch_window(window, settings.clone(), cx),
                Err(error) => log::error!("can't open a saved window: {error}"),
            }
        }
        cx.activate(true);
    });
}
