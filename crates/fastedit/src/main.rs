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
use fe_config::{ConfigWatcher, Diagnostic, Settings, SettingsStore};
use fe_gpui::{
    EditorTheme,
    fe_keymap::{self, KeymapStore},
};
use fe_workbench::{Workbench, loader};
use gpui::{App, AppContext as _, Focusable as _, WindowHandle};
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
    let store = SettingsStore::load(fe_config::user_settings_path(), project);
    log_diagnostics(store.diagnostics());
    store
}

fn log_diagnostics(diagnostics: &[Diagnostic]) {
    for diagnostic in diagnostics {
        if diagnostic.is_error() {
            log::error!("{diagnostic}");
        } else {
            log::warn!("{diagnostic}");
        }
    }
}

/// The keymap store: the built-in keymap and the user's `keymap.toml`,
/// checked against the workbench's, the editor's and the app's commands.
fn load_keymap() -> KeymapStore {
    let store = KeymapStore::load(
        fe_keymap::DEFAULT,
        fe_keymap::user_keymap_path(),
        Some(fe_workbench::command_catalog()),
    );
    log_diagnostics(store.diagnostics());
    store
}

/// Sets the built-in menu bar, checked against every command. Its
/// shortcuts follow the keymap: hot reload reinstalls it.
fn install_menus(cx: &mut App) {
    let catalog = fe_workbench::command_catalog();
    let (menus, diagnostics) = fe_keymap::default_menus(Some(&*catalog));
    log_diagnostics(&diagnostics);
    if let Some(menus) = menus {
        fe_gpui::menu::install(cx, menus);
    }
}

/// Logs what [`fe_gpui::keymap_report`] finds in the keymap in use.
fn log_keymap_report(cx: &App) {
    for issue in fe_gpui::keymap_report(cx) {
        log::info!("keymap: {issue}");
    }
}

/// Watches the settings and keymap files, applying each change on the main
/// thread: settings to the window's editors, the keymap to key input.
/// Diagnostics are logged when they change. The watcher's first batch names
/// every file, so a save made since the stores loaded applies too.
fn watch_config(
    mut settings: SettingsStore,
    mut keymap: KeymapStore,
    window: WindowHandle<Workbench>,
    cx: &mut App,
) {
    // The watcher skips a directory that's missing, so Settings… creating
    // the user's file in a config directory made after launch wouldn't be
    // seen until a restart.
    if let Some(dir) = fe_config::config_dir()
        && let Err(error) = std::fs::create_dir_all(&dir)
    {
        log::warn!("can't create {}: {error}", dir.display());
    }
    let (sender, batches) = async_channel::unbounded();
    let paths = settings.paths().into_iter().chain(keymap.paths()).collect();
    let watcher = ConfigWatcher::new(paths, move |batch| {
        sender.send_blocking(batch).ok();
    });
    let watcher = match watcher {
        Ok(watcher) => watcher,
        Err(error) => {
            log::warn!("settings and keymap won't reload when changed: {error}");
            return;
        }
    };
    cx.spawn(async move |cx| {
        // Watching for as long as this task runs.
        let _watcher = watcher;
        while let Ok(batch) = batches.recv().await {
            let previous = keymap.diagnostics().to_vec();
            if let Some(event) = keymap.reload(&batch) {
                if event.diagnostics != previous {
                    log_diagnostics(&event.diagnostics);
                }
                if event.keymap != event.previous {
                    log::info!("keymap reloaded");
                    cx.update(|cx| {
                        fe_gpui::set_user_keymap(cx, event.keymap);
                        log_keymap_report(cx);
                    });
                }
            }
            let previous = settings.diagnostics().to_vec();
            let Some(event) = settings.reload(&batch) else {
                continue;
            };
            if event.diagnostics != previous {
                log_diagnostics(&event.diagnostics);
            }
            if *event.settings != *event.previous {
                log::info!("settings reloaded");
            }
            // Only when the file changed it, so that reloading anything else
            // keeps a Toggle Vim Mode.
            let vim = event.settings.vim.enabled;
            if vim != event.previous.vim.enabled {
                cx.update(|cx| fe_gpui::set_vim_enabled(cx, vim));
            }
            let applied = window.update(cx, |workbench, _, cx| {
                workbench.reload_settings(&event, cx);
            });
            if applied.is_err() {
                break;
            }
        }
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
    let keymap = load_keymap();

    let application = gpui_platform::application().with_assets(fe_ui::Assets);
    startup.platform = Some(Instant::now());
    application.run(move |cx: &mut App| {
        startup.app = Some(Instant::now());
        fe_gpui::init_with_keymap(cx, keymap.keymap().clone());
        fe_workbench::init(cx);
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
        install_menus(cx);
        log_keymap_report(cx);
        // Prompts are drawn in the window where there are no native alerts.
        kiln_shell::init(cx);
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
        // Not before the window opens: watching isn't needed for the first
        // frame.
        watch_config(store, keymap, window, cx);
        cx.activate(true);
    });
}
