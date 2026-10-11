//! Standalone fastedit host: `fastedit [--perf] [folder or file]`, a
//! [`Workbench`] in a window.

mod perf;

use std::{
    io,
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

use perf::Startup;

use fe_buffer::Buffer;
use fe_config::{Settings, SettingsEvent, SettingsStore};
use fe_gpui::{EditorTheme, fe_keymap};
use fe_workbench::{Workbench, loader};
use gpui::{App, AppContext as _, Focusable as _, WindowHandle};
use kiln::{Config, GlobalSettings, ui::ThemeMode};
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

/// Applies each settings reload of kiln's config service to `window`: vim
/// when the file changed it, then the workbench's editors, status bar and
/// language servers. The service has logged the reload and its
/// diagnostics.
fn follow_settings(window: WindowHandle<Workbench>, cx: &mut App) {
    cx.observe_global::<GlobalSettings<Settings>>(move |cx| {
        let global = cx.global::<GlobalSettings<Settings>>();
        // The settings as loaded, before any reload: nothing to apply.
        if Arc::ptr_eq(global.settings(), global.previous()) {
            return;
        }
        let event = SettingsEvent {
            settings: global.settings().clone(),
            previous: global.previous().clone(),
            diagnostics: global.diagnostics().to_vec(),
        };
        // Only when the file changed it, so that reloading anything else
        // keeps a Toggle Vim Mode.
        let vim = event.settings.vim.enabled;
        if vim != event.previous.vim.enabled {
            fe_gpui::set_vim_enabled(cx, vim);
        }
        // The app quits once its window closes.
        window
            .update(cx, |workbench, _, cx| workbench.reload_settings(&event, cx))
            .ok();
    })
    .detach();
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
        let mut failed = None;
        let window = cx
            .open_window(
                window_options(&WindowSpec::main("fastedit"), &capabilities, cx),
                |window, cx| {
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
                    let editor_settings = settings.editor.clone();
                    let workbench = cx.new(|cx| {
                        let mut workbench =
                            Workbench::new(launch, theme, editor_settings, window, cx);
                        // The unified title bar is macOS's. Elsewhere the menus
                        // are drawn in the window, and the frame too where the
                        // window manager leaves it to the app (GNOME on Wayland).
                        workbench.set_title_bar(capabilities.unified_title_bar, cx);
                        workbench.set_menu_bar(!capabilities.native_menus, cx);
                        workbench.set_window_frame(!capabilities.unified_title_bar, cx);
                        workbench.set_status_bar_visible(settings.status_bar.visible, cx);
                        workbench
                    });
                    // Focus before the window's first draw: focusing after
                    // it would make the first frame request draw again.
                    let focus = workbench.read(cx).focus_handle(cx);
                    window.focus(&focus, cx);
                    workbench
                },
            )
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
        follow_settings(window, cx);
        cx.activate(true);
    });
}
