//! Standalone fastedit host: `fastedit [--perf] [folder or file]`, a
//! [`Workbench`] in a window.

mod perf;

use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

use perf::Startup;

use fe_buffer::Buffer;
use fe_config::{Diagnostic, Settings, SettingsEvent, SettingsStore, settings::VimClipboard};
use fe_gpui::{EditorTheme, fe_keymap};
use fe_workbench::{Launch, RootChanged, Workbench, loader};
use gpui::{
    App, AppContext as _, Entity, Focusable as _, Global, Window, WindowBounds, WindowHandle,
    WindowId, point, px,
};
use kiln::{
    Config, GlobalSettings, command::Value, core::layout::LayoutStore, shell::layout::WindowLayout,
    ui::ThemeMode,
};
use kiln_platform::{Capabilities, WindowSpec, window_options};

/// The buffer a finished read produced: a missing file is a new one, which
/// saving creates. The workbench shows an error in the tab's place.
fn loaded(read: loader::Read, started: Instant) -> io::Result<Buffer> {
    perf::file_loaded(read.at);
    let buffer = read.result?.unwrap_or_else(|| Buffer::new(""));
    log::info!(
        "loaded {} bytes, {} rows in {:.1?}",
        buffer.len(),
        buffer.text().row_count(),
        started.elapsed()
    );
    Ok(buffer)
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
        let result = loaded(read.await, started).map(Some);
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

/// What [`kiln::init`] sets up for fastedit, from its config paths (in the
/// XDG layout, as they've always been):
///
/// - the editor's, vim's and the workbench's commands, with vim as the key
///   owner ([`fe_gpui::init_with_keymap`]);
/// - the built-in keymap with the user's `keymap.toml` over it, and the
///   built-in menus, checked against those commands
///   ([`fe_workbench::command_catalog`]), the menus' shortcuts resolved
///   with an editor focused in the workbench;
/// - `settings`, from the user's file and the project's (which may only
///   turn language servers off), and the user's keymap, each watched and
///   reloaded when it changes, the keymap's issues logged after a reload;
/// - kiln's dark theme, which fastedit's chrome has always had.
///
/// Diagnostics and reloads are logged as fastedit's own.
fn config(settings: SettingsStore) -> Config {
    let app = fe_config::paths::app_id();
    let config = Config::new(app.reverse_dns, app.display)
        .log_target("fastedit")
        .commands(|cx| {
            // The keymap comes after, once every command is registered.
            fe_gpui::init_with_keymap(cx, fe_keymap::Keymap::default());
            fe_workbench::init(cx);
        })
        .catalog(fe_workbench::command_catalog())
        .keymap(fe_keymap::DEFAULT_KEYMAP_PATH, fe_keymap::DEFAULT)
        .on_keymap_reload(|cx| log_keymap_report(cx))
        .menus(fe_keymap::menu::DEFAULT_MENUS_PATH, fe_keymap::MENUS)
        .menu_shortcut_context(fe_keymap::MENU_SHORTCUT_CONTEXT)
        .settings_store(settings.into_inner())
        .theme_mode(ThemeMode::Dark);
    match fe_keymap::user_keymap_path() {
        Some(path) => config.user_keymap(path),
        None => config,
    }
}

/// Logs what [`fe_gpui::keymap_report`] finds in the keymap in use.
fn log_keymap_report(cx: &App) {
    for issue in fe_gpui::keymap_report(cx) {
        log::info!("keymap: {issue}");
    }
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
/// window: vim when the file changed it, then each workbench's editors,
/// status bar and language servers, with its root's project layer. The
/// service has logged the reload and its diagnostics.
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
        let unnamedplus = |s: &Settings| s.vim.clipboard == VimClipboard::Unnamedplus;
        let (enabled, unnamed) = (
            (now.vim.enabled, before.vim.enabled),
            (unnamedplus(now), unnamedplus(before)),
        );
        let diagnostics = global.diagnostics().to_vec();
        fe_gpui::apply_vim_reload(cx, enabled, unnamed);
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
    let theme = EditorTheme::from_settings(&settings.editor, cx);
    let capabilities = Capabilities::current();
    let (layout, entry) = match root.as_deref().and_then(layout_store) {
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
/// there's no root or no state directory) and the entry it restores, if
/// any: see [`kiln::shell::layout::launch`]. A folder launch (`folder`)
/// restores the root's saved windows; a file launch restores nothing.
fn launch_windows(root: Option<&Path>, folder: bool) -> Vec<(Option<WindowLayout>, Option<Value>)> {
    let windows: Vec<_> = (root.and_then(layout_store).iter())
        .flat_map(|store| kiln::shell::layout::launch(store, folder))
        .map(|(layout, entry)| (Some(layout), entry))
        .collect();
    if windows.is_empty() {
        return vec![(None, None)];
    }
    windows
}

/// The saved layouts of the folder `root`, if there's a state directory.
fn layout_store(root: &Path) -> Option<LayoutStore> {
    let dir = fe_config::paths::app_paths().layouts_dir()?;
    Some(LayoutStore::new(dir, &kiln::core::trust::canonical(root)))
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
        // Commands, keys, menus, in-window prompts where there are no
        // native alerts, and the config files watched from here on.
        kiln::init(cx, config(store));
        fe_gpui::set_vim_enabled(cx, settings.vim.enabled);
        fe_gpui::set_vim_clipboard(cx, settings.vim.clipboard == VimClipboard::Unnamedplus);
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

        let theme = EditorTheme::from_settings(&settings.editor, cx);
        fe_gpui::set_restore_style(cx, theme.clone(), settings.editor.clone());
        // A folder launch reopens its saved windows; a file launch opens
        // the file, in a window that saves its layout from now on.
        let mut windows = launch_windows(launch.root.as_deref(), reading.is_none());
        let (layout, entry) = windows.remove(0);
        let root = launch.root.clone();
        let launch_root = launch.root.clone();
        let mut failed = None;
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
                    let buffer = match reading.try_finish().map(|read| loaded(read, started)) {
                        Some(Ok(buffer)) => Some(buffer),
                        Some(Err(error)) => {
                            failed = Some(error);
                            None
                        }
                        None => None,
                    };
                    (reading.path().to_owned(), buffer)
                });
                new_workbench(launch, &settings, theme.clone(), layout, entry, window, cx)
            })
            .expect("failed to open the main window");
        startup.window = Some(Instant::now());
        let loading = window
            .update(cx, |workbench, window, cx| {
                if perf {
                    perf::start(cx.entity(), startup, window);
                }
                workbench.active_editor(cx).is_none()
            })
            .unwrap_or(false);
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
