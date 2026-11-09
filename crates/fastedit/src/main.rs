//! Tenmoku, the standalone host: `tenmoku [options] [path[:line[:column]] ...]`
//! ([`cli`]), a [`Workbench`] in a window.
// A GUI app on Windows, so launching it opens no console window; started
// from one, it writes to it all the same (`windows_console`).
#![cfg_attr(windows, windows_subsystem = "windows")]

mod app_icon;
mod cli;
mod config;
mod grammar_install;
#[cfg(test)]
mod grammar_install_tests;
mod ipc;
#[cfg(unix)]
mod log_file;
#[cfg(target_os = "macos")]
mod mac_speller;
mod perf;
#[cfg(unix)]
mod shell_command;
mod sparkle;
#[cfg(test)]
mod sparkle_tests;
mod theme;
// The signer example shares the file; only the tests sign here.
#[cfg_attr(not(test), allow(dead_code))]
mod update_signature;
mod velopack_update;
#[cfg(test)]
mod velopack_update_tests;
#[cfg(test)]
mod vim_lazyvim_harpoon_tests;
mod vim_remaps;
mod wait;
#[cfg(windows)]
mod windows_console;
mod workspace_extensions;

use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use cli::Target;
use config::{config, log_keymap_report};
use perf::Startup;
use vim_remaps::VimRemaps;

use fe_buffer::Buffer;
use fe_config::paths::app_id;
use fe_config::{Diagnostic, Settings, SettingsEvent, SettingsStore, settings::VimClipboard};
use fe_gpui::EditorTheme;
use fe_workbench::{
    Launch, RootChanged, Workbench,
    loader::{self, Opened},
};
use gpui::{
    App, AppContext as _, Entity, Focusable as _, Global, Task, Window, WindowBounds, WindowHandle,
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
                target: "fastedit",
                "loaded {} bytes, {} rows in {:.1?}",
                buffer.len(),
                buffer.text().row_count(),
                started.elapsed()
            );
            Ok(buffer.into())
        }
        Opened::Large { view, index } => {
            log::info!(
                target: "fastedit",
                "viewing {} bytes read-only, opened in {:.1?}",
                view.len(),
                started.elapsed()
            );
            Ok(Opened::Large { view, index })
        }
        Opened::Image => Ok(Opened::Image),
    }
}

/// Waits for `reading` off the main thread, then shows the file in the
/// window: the task is done once it has.
fn open_in_background(
    reading: loader::Reading,
    window: WindowHandle<Workbench>,
    started: Instant,
    cx: &mut App,
) -> Task<()> {
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
    /// Each window's newest settings load, counting up: every `settle` (a
    /// re-root, a reload) starts one and outdates those before it.
    loads: HashMap<WindowId, u64>,
    /// Each non-launch root's settings layers, kept across reads so a file
    /// that no longer parses keeps its last accepted values, as the config
    /// service's do for the launch root.
    stores: Arc<Mutex<HashMap<PathBuf, SettingsStore>>>,
}

impl Global for WindowSettings {}

/// `root`'s settings, its files read again into the layers kept for it.
fn read_settings(stores: &Mutex<HashMap<PathBuf, SettingsStore>>, root: &Path) -> Arc<Settings> {
    let mut stores = stores.lock().unwrap_or_else(|e| e.into_inner());
    let store = stores
        .entry(root.to_owned())
        .or_insert_with(|| load_settings(Some(root)));
    let files: Vec<PathBuf> = fe_config::user_settings_path()
        .into_iter()
        .chain([fe_config::project_settings_path(root)])
        .collect();
    store.reload(&files);
    store.settings().clone()
}

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
        ..Default::default()
    });
    cx.observe_global::<GlobalSettings<Settings>>(move |cx| {
        let global = cx.global::<GlobalSettings<Settings>>();
        // The settings as loaded, before any reload: nothing to apply.
        if Arc::ptr_eq(global.settings(), global.previous()) {
            return;
        }
        let (now, before) = (global.settings(), global.previous());
        let (jump_on_s, jump_style) = (now.vim.jump_on_s, now.vim.jump_style);
        let (ignorecase, smartcase, incsearch) =
            (now.vim.ignorecase, now.vim.smartcase, now.vim.incsearch);
        let inccommand = now.vim.inccommand;
        let replace_with_register = now.vim.replace_with_register;
        let subword_motions = now.vim.subword_motions;
        let now_limit = now.vim.regex_time_limit;
        let which_key = (now.vim.which_key, now.vim.which_key_delay_ms);
        let yank = (now.vim.highlight_yank, now.vim.highlight_yank_ms);
        let quick_scope = now.vim.quick_scope;
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
        fe_gpui::set_vim_jump_style(cx, jump_style);
        fe_gpui::set_vim_search_case(cx, ignorecase, smartcase);
        fe_gpui::set_vim_incsearch(cx, incsearch);
        fe_gpui::set_vim_inccommand(cx, inccommand);
        fe_gpui::set_vim_replace_with_register(cx, replace_with_register);
        fe_gpui::set_vim_subword_motions(cx, subword_motions);
        fe_gpui::set_vim_time_limit(Duration::from_millis(now_limit));
        fe_gpui::set_vim_which_key(cx, which_key.0, Duration::from_millis(which_key.1));
        fe_gpui::set_vim_highlight_yank(cx, yank.0, Duration::from_millis(yank.1));
        fe_gpui::set_vim_quick_scope(cx, quick_scope);
        for window in cx.windows() {
            if let Some(window) = window.downcast::<Workbench>() {
                settle(window, diagnostics.clone(), cx);
            }
        }
    })
    .detach();
    // A settings file of a window's non-launch root, or the user's, changed:
    // its settings are read again, which outdates a load already running
    // (`GlobalSettings` doesn't notify for either).
    cx.observe_global::<kiln::platform::config::ChangedFiles>(|cx| {
        let user = fe_config::user_settings_path();
        let launch = (cx.try_global::<WindowSettings>()).and_then(|s| s.launch_root.clone());
        for window in cx.windows() {
            let Some(window) = window.downcast::<Workbench>() else {
                continue;
            };
            let Ok(root) = window.read_with(cx, |w, _| w.root().map(Path::to_owned)) else {
                continue;
            };
            if root.is_none() || root == launch {
                continue;
            }
            let project = root.as_deref().map(fe_config::project_settings_path);
            let changed = kiln::platform::config::changed_files(cx);
            if user
                .iter()
                .chain(project.iter())
                .any(|f| changed.contains(f))
            {
                settle(window, Vec::new(), cx);
            }
        }
    })
    .detach();
}

/// Gives `window` the settings of its root: the config service's for the
/// launch root (or none), else its root's own, read off the main thread.
/// Each call is a new load of the window's ([`begin_load`]); one read off
/// the main thread is given only if it's still the window's newest, for
/// the root it still has ([`finish_load`]).
fn settle(window: WindowHandle<Workbench>, diagnostics: Vec<Diagnostic>, cx: &mut App) {
    let Some((load, root)) = begin_load(window, cx) else {
        return;
    };
    let launch_root = cx
        .try_global::<WindowSettings>()
        .and_then(|settings| settings.launch_root.clone());
    if root.is_none() || root == launch_root {
        let settings = current_settings(cx);
        return give(window, settings, diagnostics, cx);
    }
    let stores = cx.default_global::<WindowSettings>().stores.clone();
    let read = cx.background_spawn({
        let root = root.clone();
        async move {
            match root.as_deref() {
                Some(root) => read_settings(&stores, root),
                None => load_settings(None).settings().clone(),
            }
        }
    });
    cx.spawn(async move |cx| {
        let settings = read.await;
        cx.update(|cx| finish_load(window, load, root, settings, diagnostics, cx));
    })
    .detach();
}

/// Starts a settings load for `window`: its number, which outdates every
/// load of the window's before it, and the root it's for. `None` once the
/// window is gone.
fn begin_load(window: WindowHandle<Workbench>, cx: &mut App) -> Option<(u64, Option<PathBuf>)> {
    let root = window
        .read_with(cx, |workbench, _| workbench.root().map(Path::to_owned))
        .ok()?;
    let loads = &mut cx.default_global::<WindowSettings>().loads;
    let load = loads.entry(window.window_id()).or_default();
    *load += 1;
    Some((*load, root))
}

/// Gives `window` `settings`, read by its load `load` for `root`, if that
/// is still its newest load and `root` still its root. A load overtaken
/// (by a re-root, back to the launch root too, or by a later load that
/// finished first) is dropped before it changes anything: the window's
/// `persistent_undo`, the undo reads waiting for it, or its settings.
fn finish_load(
    window: WindowHandle<Workbench>,
    load: u64,
    root: Option<PathBuf>,
    settings: Arc<Settings>,
    diagnostics: Vec<Diagnostic>,
    cx: &mut App,
) {
    let newest = (cx.try_global::<WindowSettings>())
        .and_then(|settings| settings.loads.get(&window.window_id()).copied());
    let current = window.read_with(cx, |workbench, _| workbench.root().map(Path::to_owned));
    if newest != Some(load) || current.ok() != Some(root) {
        return;
    }
    give(window, settings, diagnostics, cx);
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
    // Its editors keep undo history as its root says, whatever the others'.
    let persistent_undo = Some(settings.editor.persistent_undo);
    fe_gpui::pane_item::set_window_persistent_undo(id, persistent_undo, cx);
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
    // Its editors keep no undo history, and read none, until its root's
    // settings are in: `settle` gives them, at once for the launch root's,
    // else once read off the main thread.
    fe_gpui::pane_item::wait_for_window_persistent_undo(id, cx);
    cx.default_global::<WindowSettings>()
        .given
        .insert(id, settings);
    if let Ok(workbench) = window.update(cx, |_, _, cx| cx.entity()) {
        cx.subscribe(&workbench, move |_, _: &RootChanged, cx| {
            fe_gpui::pane_item::wait_for_window_persistent_undo(id, cx);
            settle(window, Vec::new(), cx)
        })
        .detach();
    }
    settle(window, Vec::new(), cx);
    cx.on_window_closed(move |cx, closed| {
        if closed == id {
            let settings = cx.default_global::<WindowSettings>();
            settings.given.remove(&id);
            settings.loads.remove(&id);
            // Its `persistent_undo` stays: its editors are released after
            // this, and their close writes still go by it.
        }
    })
    .detach();
}

/// Opens a window on `root` (`window.new`, Open Folder in New Window): the
/// root's newest saved window no live window holds, or else a fresh one,
/// a little below and right of the active window.
fn open_window_on(root: Option<PathBuf>, cx: &mut App) {
    open_window_at(root, cx);
}

/// [`open_window_on`], giving the window it opened.
fn open_window_at(root: Option<PathBuf>, cx: &mut App) -> Option<WindowHandle<Workbench>> {
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
    let mut options = window_options(
        &WindowSpec::main(fe_config::APP_NAME).app_id(app_id().reverse_dns),
        &capabilities,
        cx,
    );
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
        Ok(window) => {
            watch_window(window, given, cx);
            Some(window)
        }
        Err(error) => {
            log::error!(target: "fastedit", "can't open a window: {error}");
            None
        }
    }
}

/// Opens what the system handed the app: see
/// [`fe_workbench::open_documents`]. A file with no window on its folder
/// gets one, with the file opened in it.
fn open_documents(paths: &[PathBuf], cx: &mut App) {
    fe_workbench::open_documents(paths, cx, |root, file, cx| {
        let Some(window) = open_window_at(Some(root), cx) else {
            return;
        };
        if let Some(file) = file {
            window
                .update(cx, |workbench, window, cx| {
                    workbench.open_document(file, window, cx)
                })
                .ok();
        }
    });
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

/// Starts kiln-shell's frame watchdog for `window` (T1609), which `Root`
/// does for kiln's own windows. It is macOS's: elsewhere, and on GPUI's
/// test platform, there is nothing to start. The watch ends when the window
/// closes, so the task is detached.
fn start_frame_watch(window: &mut Window, cx: &mut App) {
    #[cfg(test)]
    let watch = tests::WATCH
        .with(|w| w.get())
        .unwrap_or(kiln::shell::watch_frame_requests);
    #[cfg(not(test))]
    let watch = kiln::shell::watch_frame_requests;
    if let Some(task) = watch(window, cx) {
        task.detach();
    }
}

/// A workbench for `launch`, in Tenmoku's chrome for this platform, with
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
    // Tenmoku's windows aren't kiln `Root`s, which do this themselves.
    window.follow_system_appearance(cx);
    start_frame_watch(window, cx);
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
        #[cfg(target_os = "macos")]
        workbench.set_speller(mac_speller::MacSpeller::new(), cx);
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

/// Quits as the Quit command does, asking about what it would ask about.
fn quit(cx: &mut App) {
    let quit = kiln::command::Invocation::of("app.quit").expect("a valid command id");
    cx.dispatch_action(&fe_gpui::commands::RunCommand(quit));
}

/// Logs to stderr: warnings and errors, and fastedit's own info, under the
/// binary's name too. `RUST_LOG` sets one level for everything instead
/// (`debug`, `trace`, …).
struct StderrLogger {
    level: Option<log::LevelFilter>,
}

impl log::Log for StderrLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        let target = metadata.target();
        let ours = target.starts_with("fastedit")
            || target.starts_with("tenmoku")
            || target.starts_with("fe_");
        // Velopack warns on every launch that we gave it the bundle's paths
        // (to find it through a symlink); our own lines say what it did.
        let velopack = target.starts_with("velopack");
        let level = self.level.unwrap_or(if ours {
            log::LevelFilter::Info
        } else if velopack {
            log::LevelFilter::Error
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

/// What's left of the launch's `files` for [`open_files`], the first being
/// the launch file. One whose read failed is over, its error shown, and the
/// rest are all still to open. Otherwise, with that one alone and no position,
/// the launch has done it all.
fn files_left(mut files: Vec<Target>, launch_failed: bool) -> Vec<Target> {
    if launch_failed {
        if !files.is_empty() {
            files.remove(0);
        }
    } else if let [Target { line: None, .. }] = files[..] {
        files.clear();
    }
    files
}

/// Opens `files` in `window`, each at its position, one at a time, so the
/// tabs keep their order whichever read is quickest. A first file that's the
/// launch file, in its tab or being read, isn't read again. The task is done
/// once they've all landed.
fn open_files(window: WindowHandle<Workbench>, files: Vec<Target>, cx: &mut App) -> Task<()> {
    let mut files = files.into_iter();
    let open = move |target: Target, cx: &mut App| {
        window.update(cx, |workbench, window, cx| {
            let Target { path, line, col } = target;
            workbench.open_document_at(path, line, col, window, cx)
        })
    };
    // At once: this is the launch file's read still being the host's.
    let Some(Ok(first)) = files.next().map(|first| open(first, cx)) else {
        return Task::ready(());
    };
    cx.spawn(async move |cx| {
        first.await;
        for target in files {
            match cx.update(|cx| open(target, cx)) {
                Ok(landed) => landed.await,
                Err(_) => return,
            }
        }
    })
}

fn main() {
    #[cfg(windows)]
    windows_console::attach_to_parent();
    let mut startup = Startup::new();
    let started = Instant::now();
    let mut how = ipc::How::current();
    #[cfg(unix)]
    log_file::install(
        log_file::LaunchKind::of(how.launch_services, how.foreground, how.terminal),
        log_file::display_count(),
    );
    init_logging();
    // The string catalogue, built for the system's languages on a thread of
    // its own, so that neither the first frame nor applying the settings'
    // `workbench.locale` parses it here.
    let warm = Instant::now();
    kiln_platform::locale::warm();
    startup.l10n_warm = Some(warm.elapsed());
    // Before the command line is parsed: Velopack's `--veloapp-*` hook
    // launches would be unknown flags to it.
    velopack_update::startup();
    let mut cli = cli::parse(std::env::args_os().skip(1).collect()).unwrap_or_else(|error| {
        error.print();
        std::process::exit(error.exit_code())
    });
    let perf = cli.perf;
    let cli_wait = cli.wait;
    how.no_ipc |= perf;
    // The headless Velopack update, if asked for: never in a `--perf` or
    // `--wait` launch, and before IPC, which would hand the launch on.
    velopack_update::update_now_if_asked(perf, cli.wait || how.parent_carries);
    // Over here if the running instance takes the launch, or a detached copy
    // of this process does.
    let request = ipc::Request::of(&cli, std::env::current_dir().unwrap_or_default());
    let handoffs = match ipc::start(&request, &how, || ipc::detach(request.wait)) {
        ipc::Start::Exit => return,
        ipc::Start::ServerLost => std::process::exit(1),
        ipc::Start::Serve(server) => Some(server.listen()),
        ipc::Start::Alone => None,
    };
    // Who's waiting on this launch's own files: its client when it's the
    // detached server of a `--wait` launch, or it itself when there's no other
    // process to do the waiting.
    let waits_by_itself = !perf && how.waits_by_itself(cli.wait);
    // A launch something's waiting on: it, or the detached server of one.
    let wait_launch = cli.wait || how.parent_carries;
    let waiter = handoffs
        .as_ref()
        .filter(|_| how.parent_carries)
        .and_then(|handoffs| ipc::launch_first_request(handoffs, &mut cli));
    let plan = cli.plan();

    let application = kiln::application();
    // What macOS hands the app as open-document events (Finder's Open With,
    // `open -a Tenmoku <path>`, a drop on the Dock icon), in the order they
    // came: read at launch, and routed to windows after it.
    let (opened_tx, opened_rx) = async_channel::unbounded::<Vec<PathBuf>>();
    application.on_open_urls(move |urls| {
        let paths: Vec<_> = urls
            .iter()
            .filter_map(|url| {
                let path = fe_workbench::path_of_url(url);
                if path.is_none() {
                    log::warn!(target: "fastedit", "not opening {url}: not a file URL");
                }
                path
            })
            .collect();
        opened_tx.try_send(paths).ok();
    });
    startup.platform = Some(Instant::now());
    application.run(move |cx: &mut App| {
        startup.app = Some(Instant::now());
        // Never mid-`--wait` or `--perf`. The first-launch decision reads the
        // config and state directories before the launch writes anything
        // there, its windows' layouts included.
        let setup = if cli_wait || perf {
            fe_workbench::SetupLaunch::Never
        } else {
            fe_workbench::SetupLaunch::Normal
        };
        fe_workbench::begin_setup(setup, cx);
        // Paths macOS handed the app before this point (a cold launch by
        // open-document event): the first is the launch's when argv gave
        // none, so no empty window opens beside it.
        let mut handed: Vec<PathBuf> = Vec::new();
        while let Ok(paths) = opened_rx.try_recv() {
            handed.extend(paths);
        }
        let files = plan.files;
        let waited_on: Vec<PathBuf> = files.iter().map(|file| file.path.clone()).collect();
        let mut launch = if plan.root.is_some() {
            Launch {
                root: plan.root,
                file: files.first().map(|first| (first.path.clone(), None)),
            }
        } else {
            let path = (!handed.is_empty()).then(|| handed.remove(0));
            fe_workbench::resolve_launch(path)
        };
        let reading = launch.file.take().map(|(path, _)| loader::start(path));
        let store = load_settings(launch.root.as_deref());
        let settings: Arc<Settings> = store.settings().clone();
        app_icon::set();
        // The one start of the updater: off for all three of these, as its
        // policy decides.
        let (under_test, automatic_checks) = (
            cx.background_executor().is_test(),
            settings.update.automatic_checks,
        );
        // The Velopack spike takes Check for Updates… only in a build made
        // for it, installed by it; otherwise Sparkle starts as before.
        if !velopack_update::start(perf, wait_launch, under_test, cx) {
            sparkle::start(perf, wait_launch, under_test, automatic_checks, cx);
        }
        // Commands, keys, menus, in-window prompts where there are no
        // native alerts, and the config files watched from here on: the
        // vimrc and `vim.remap` too, which give every editor its remaps.
        let remaps = VimRemaps::new(fe_config::paths::user_vimrc_path(), &store);
        // Extensions' language servers start only once the user consents,
        // which is recorded beside the other state files; set before the
        // extensions load, so their servers are checked against it.
        if let Some(dir) = fe_config::paths::state_dir() {
            fe_workbench::set_extension_consent_file(cx, dir.join(fe_workbench::CONSENT_FILE));
            let history = fe_buffer::LocalHistory::new(dir.join("history"));
            let pruned = history.clone();
            cx.background_executor()
                .spawn(async move {
                    if let Err(error) = pruned.prune(std::time::SystemTime::now()) {
                        log::debug!(target: "fastedit", "local history: pruning: {error}");
                    }
                })
                .detach();
            fe_gpui::pane_item::set_local_history(cx, Some(history));
            // Each file's undo history, kept between opens. Off, nothing
            // is removed, so the prune waits for it to be on.
            let undo = fe_buffer::UndoStore::new(dir.join("undo"));
            if settings.editor.persistent_undo {
                let pruned = undo.clone();
                cx.background_executor()
                    .spawn(async move {
                        if let Err(error) = pruned.prune(std::time::SystemTime::now()) {
                            log::debug!(target: "fastedit", "undo store: pruning: {error}");
                        }
                    })
                    .detach();
            }
            fe_gpui::pane_item::set_undo_store(cx, Some(undo));
        }
        // Syntax highlighting to offer for files with none, kept where the
        // extensions the loader reads are.
        if let Some(extensions) = fe_config::paths::app_paths().extensions_dir() {
            let never_file =
                fe_config::paths::state_dir().map(|dir| dir.join(fe_workbench::PROMPT_FILE));
            fe_workbench::set_grammar_offers(cx, grammar_install::offers(extensions, never_file));
        }
        kiln::init(cx, config(store).source(remaps));
        // The editor's colours, from kiln's theme from now on.
        theme::init(cx);
        fe_gpui::set_vim_enabled(cx, settings.vim.enabled);
        fe_gpui::set_vim_clipboard(cx, settings.vim.clipboard == VimClipboard::Unnamedplus);
        fe_gpui::set_vim_jump_on_s(cx, settings.vim.jump_on_s);
        fe_gpui::set_vim_jump_style(cx, settings.vim.jump_style);
        fe_gpui::set_vim_search_case(cx, settings.vim.ignorecase, settings.vim.smartcase);
        fe_gpui::set_vim_incsearch(cx, settings.vim.incsearch);
        fe_gpui::set_vim_inccommand(cx, settings.vim.inccommand);
        fe_gpui::set_vim_replace_with_register(cx, settings.vim.replace_with_register);
        fe_gpui::set_vim_subword_motions(cx, settings.vim.subword_motions);
        fe_gpui::set_vim_time_limit(Duration::from_millis(settings.vim.regex_time_limit));
        fe_gpui::set_vim_which_key(
            cx,
            settings.vim.which_key,
            Duration::from_millis(settings.vim.which_key_delay_ms),
        );
        fe_gpui::set_vim_highlight_yank(
            cx,
            settings.vim.highlight_yank,
            Duration::from_millis(settings.vim.highlight_yank_ms),
        );
        fe_gpui::set_vim_quick_scope(cx, settings.vim.quick_scope);
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
        // So may the launch root's own extensions load.
        workspace_extensions::follow(launch.root.clone(), cx);
        log_keymap_report(cx);
        #[cfg(unix)]
        shell_command::init(cx);
        if handoffs.is_some() || waits_by_itself {
            wait::init(cx);
        }
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
        let mut launch_failed = false;
        let mut launch_read = None;
        // The launch file's read hadn't finished as the window opened.
        let mut loading = false;
        let mut options = window_options(
            &WindowSpec::main(fe_config::APP_NAME).app_id(app_id().reverse_dns),
            &capabilities,
            cx,
        );
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
                    // Its error is shown: nothing to put a caret in.
                    launch_failed = true;
                    let path = reading.path().to_owned();
                    window
                        .update(cx, |workbench, window, cx| {
                            workbench.file_read(path, Err(error), window, cx)
                        })
                        .ok();
                }
                None if loading => {
                    launch_read = Some(open_in_background(reading, window, started, cx));
                }
                None => {}
            }
        }
        follow_settings(launch_root.clone(), cx);
        watch_window(window, settings.clone(), cx);
        // Once, for this first window.
        fe_workbench::offer_setup(window, setup, cx);
        fe_workbench::set_window_opener(cx, open_window_on);
        let opened = open_files(window, files_left(files, launch_failed), cx);
        let landed = cx.spawn(async move |_| {
            if let Some(read) = launch_read {
                read.await;
            }
            opened.await;
        });
        let landed = ipc::while_open(window, landed, cx);
        match waiter {
            Some(waiter) => wait::add(waiter, waited_on, landed, cx),
            None if waits_by_itself => wait::add_own(quit, waited_on, landed, cx),
            None => landed.detach(),
        }
        // The root's other saved windows.
        for (layout, entry) in windows {
            let launch = Launch {
                root: root.clone(),
                file: None,
            };
            let theme = theme.clone();
            let mut options = window_options(
                &WindowSpec::main(fe_config::APP_NAME).app_id(app_id().reverse_dns),
                &capabilities,
                cx,
            );
            if let Some(bounds) = entry.as_ref().and_then(saved_bounds) {
                options.window_bounds = Some(bounds);
            }
            let opened = cx.open_window(options, |window, cx| {
                new_workbench(launch, &settings, theme, layout, entry, window, cx)
            });
            match opened {
                Ok(window) => watch_window(window, settings.clone(), cx),
                Err(error) => log::error!(target: "fastedit", "can't open a saved window: {error}"),
            }
        }
        // The rest of what was handed at launch, and what comes later.
        open_documents(&handed, cx);
        cx.spawn(async move |cx| {
            while let Ok(paths) = opened_rx.recv().await {
                cx.update(|cx| {
                    open_documents(&paths, cx);
                    cx.activate(true);
                });
            }
        })
        .detach();
        // What later launches hand over.
        if let Some(handoffs) = handoffs {
            cx.spawn(async move |cx| {
                while let Ok(handoff) = handoffs.recv().await {
                    cx.update(|cx| wait::serve(handoff, cx, open_window_at));
                }
            })
            .detach();
        }
        cx.activate(true);
    });
}

#[cfg(test)]
mod tests {
    use std::fs;

    use fe_gpui::EditorSettings;
    use gpui::TestAppContext;
    use log::{Level, Log as _, Metadata};

    use std::cell::Cell;

    use super::{EditorTheme, Launch, StderrLogger, Target, Workbench, files_left, open_files};

    type Watch = fn(&mut gpui::Window, &mut gpui::App) -> Option<gpui::Task<()>>;

    thread_local! {
        /// The watchdog `new_workbench` starts, in place of kiln's, which
        /// GPUI's test platform has no AppKit view for.
        pub(super) static WATCH: Cell<Option<Watch>> = const { Cell::new(None) };
        /// Ticks of each window's stand-in watchdog task.
        static TICKS: std::cell::RefCell<std::collections::HashMap<gpui::WindowId, u32>> =
            std::cell::RefCell::new(Default::default());
    }

    fn ticks(id: gpui::WindowId) -> u32 {
        TICKS.with(|t| t.borrow().get(&id).copied().unwrap_or(0))
    }

    /// A watchdog that ticks every 10 ms while its task lives.
    fn ticking(window: &mut gpui::Window, cx: &mut gpui::App) -> Option<gpui::Task<()>> {
        let id = window.window_handle().window_id();
        Some(cx.spawn(async move |cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(10))
                    .await;
                TICKS.with(|t| *t.borrow_mut().entry(id).or_default() += 1);
            }
        }))
    }

    /// Every window `new_workbench` makes gets the frame watchdog, and its
    /// task stays alive after `new_workbench` returns (T1610).
    #[gpui::test]
    fn new_workbench_starts_the_frame_watchdog(cx: &mut TestAppContext) {
        cx.update(|cx| {
            fe_gpui::init(cx);
            fe_workbench::init(cx);
            cx.set_global(fe_workbench::ConfigDirOverride(None));
        });
        WATCH.with(|w| w.set(Some(ticking)));
        let mut windows = Vec::new();
        for _ in 0..2 {
            let settings = super::Settings::default();
            let window = cx
                .update(|cx| {
                    cx.open_window(gpui::WindowOptions::default(), |window, cx| {
                        let theme = EditorTheme::default_dark(cx);
                        let launch = Launch {
                            root: None,
                            file: None,
                        };
                        super::new_workbench(launch, &settings, theme, None, None, window, cx)
                    })
                })
                .unwrap();
            windows.push(window.window_id());
        }
        for id in &windows {
            assert_eq!(ticks(*id), 0);
        }
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(100));
        cx.run_until_parked();
        let first: Vec<u32> = windows.iter().map(|id| ticks(*id)).collect();
        assert!(first.iter().all(|n| *n > 0), "{first:?}");
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(100));
        cx.run_until_parked();
        for (id, before) in windows.iter().zip(first) {
            assert!(ticks(*id) > before, "a window's watchdog task stopped");
        }
        WATCH.with(|w| w.set(None));
    }

    fn at(path: &str, line: Option<u32>) -> Target {
        Target {
            path: path.into(),
            line,
            col: None,
        }
    }

    #[test]
    fn a_lone_launch_file_is_done_unless_it_has_a_place_or_failed() {
        assert_eq!(files_left(vec![at("/a", None)], false), []);
        let placed = vec![at("/a", Some(3))];
        assert_eq!(files_left(placed.clone(), false), placed);
        let two = vec![at("/a", None), at("/b", None)];
        assert_eq!(files_left(two.clone(), false), two);
        assert_eq!(files_left(vec![at("/a", None)], true), []);
        assert_eq!(files_left(vec![], true), []);
    }

    /// `tenmoku unreadable.txt valid.txt`: the launch file's read fails before
    /// the window has it, and what's left, a lone file with no position, is no
    /// launch file: it still has to open (T1310 F1).
    #[gpui::test]
    fn a_failed_launch_file_leaves_the_next_to_open(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let dir = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(dir.path()).unwrap();
        let valid = root.join("valid.txt");
        fs::write(&valid, "valid\n").unwrap();
        cx.update(|cx| {
            fe_gpui::init(cx);
            fe_workbench::init(cx);
        });
        let launch = Launch {
            root: Some(root.clone()),
            file: None,
        };
        let window = cx.add_window(move |window, cx| {
            let theme = EditorTheme::default_dark(cx);
            Workbench::new(launch, theme, EditorSettings::default(), window, cx)
        });
        let files = vec![
            at(root.join("unreadable.txt").to_str().unwrap(), None),
            at(valid.to_str().unwrap(), None),
        ];
        cx.update(|cx| open_files(window, files_left(files, true), cx).detach());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        let tabs = loop {
            cx.run_until_parked();
            let tabs: Vec<_> = window
                .read_with(cx, |workbench, cx| {
                    let pane = workbench.pane().read(cx);
                    (pane.tabs().iter())
                        .filter_map(|tab| tab.path.clone())
                        .collect()
                })
                .unwrap();
            if !tabs.is_empty() || std::time::Instant::now() > deadline {
                break tabs;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        };
        assert_eq!(tabs, [valid]);
    }

    /// Each window's editors keep undo history as its own root's settings
    /// say: giving one window settings with `editor.persistent_undo` off
    /// leaves another's on (T483 F1).
    #[gpui::test]
    fn each_window_keeps_undo_as_its_root_says(cx: &mut TestAppContext) {
        cx.update(|cx| {
            fe_gpui::init(cx);
            fe_workbench::init(cx);
        });
        let window = |cx: &mut TestAppContext| {
            let dir = tempfile::tempdir().unwrap();
            let launch = Launch {
                root: Some(fs::canonicalize(dir.path()).unwrap()),
                file: None,
            };
            let window = cx.add_window(move |window, cx| {
                let theme = EditorTheme::default_dark(cx);
                Workbench::new(launch, theme, EditorSettings::default(), window, cx)
            });
            (window, dir)
        };
        let ((on, _a), (off, _b)) = (window(cx), window(cx));
        let settings = |persistent_undo: bool| {
            let mut settings = fe_config::Settings::default();
            settings.editor.persistent_undo = persistent_undo;
            std::sync::Arc::new(settings)
        };
        cx.update(|cx| {
            super::give(on, settings(true), Vec::new(), cx);
            super::give(off, settings(false), Vec::new(), cx);
        });
        let given = |window: gpui::WindowHandle<Workbench>, cx: &mut TestAppContext| {
            cx.update(|cx| fe_gpui::pane_item::window_persistent_undo(window.window_id(), cx))
        };
        assert_eq!(given(on, cx), Some(true));
        assert_eq!(given(off, cx), Some(false));
        cx.update(|cx| super::give(off, settings(true), Vec::new(), cx));
        assert_eq!(given(off, cx), Some(true), "a reload turns it on again");
        assert_eq!(given(on, cx), Some(true));
    }

    /// A window on a root other than the launch root waits for its root's
    /// settings before its editors read or write undo history: until then
    /// it has no setting of its own, and the launch root's isn't used
    /// (T483 round 3 F2).
    #[gpui::test]
    fn a_window_on_another_root_waits_for_its_settings(cx: &mut TestAppContext) {
        cx.update(|cx| {
            fe_gpui::init(cx);
            fe_workbench::init(cx);
            fe_gpui::pane_item::set_undo_store(cx, None);
        });
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let launch = Launch {
            root: Some(root),
            file: None,
        };
        let window = cx.add_window(move |window, cx| {
            let theme = EditorTheme::default_dark(cx);
            Workbench::new(launch, theme, EditorSettings::default(), window, cx)
        });
        let given = |cx: &mut TestAppContext| {
            cx.update(|cx| fe_gpui::pane_item::window_persistent_undo(window.window_id(), cx))
        };
        cx.update(|cx| super::watch_window(window, Default::default(), cx));
        assert_eq!(
            given(cx),
            None,
            "nothing given before its settings are read"
        );
        cx.executor().allow_parking();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while given(cx).is_none() && std::time::Instant::now() < deadline {
            cx.run_until_parked();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(given(cx), Some(true), "its root's settings, once read");
    }

    /// A window on `root`, with the launch root `launch`, watched as the
    /// app watches its windows, its first load of `root`'s settings
    /// still to finish.
    fn watched_window(
        cx: &mut TestAppContext,
        root: &std::path::Path,
        launch: &std::path::Path,
    ) -> gpui::WindowHandle<Workbench> {
        cx.update(|cx| {
            fe_gpui::init(cx);
            fe_workbench::init(cx);
            fe_gpui::pane_item::set_undo_store(cx, None);
            cx.set_global(super::WindowSettings {
                launch_root: Some(launch.to_owned()),
                ..Default::default()
            });
        });
        let launch = Launch {
            root: Some(root.to_owned()),
            file: None,
        };
        let window = cx.add_window(move |window, cx| {
            let theme = EditorTheme::default_dark(cx);
            Workbench::new(launch, theme, EditorSettings::default(), window, cx)
        });
        cx.update(|cx| super::watch_window(window, Default::default(), cx));
        window
    }

    fn undo_setting(persistent_undo: bool) -> std::sync::Arc<fe_config::Settings> {
        let mut settings = fe_config::Settings::default();
        settings.editor.persistent_undo = persistent_undo;
        std::sync::Arc::new(settings)
    }

    fn given_undo(window: gpui::WindowHandle<Workbench>, cx: &mut TestAppContext) -> Option<bool> {
        cx.update(|cx| fe_gpui::pane_item::window_persistent_undo(window.window_id(), cx))
    }

    /// A load of a root's settings that a re-root overtakes, here back to
    /// the launch root, whose settings apply at once, is dropped when it
    /// finishes: it changes neither the window's `persistent_undo` nor its
    /// settings (T483 round 3 F1).
    #[gpui::test]
    fn a_settings_load_overtaken_by_a_re_root_is_discarded(cx: &mut TestAppContext) {
        let (dir, launch) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let root = fs::canonicalize(dir.path()).unwrap();
        let launch = fs::canonicalize(launch.path()).unwrap();
        let window = watched_window(cx, &root, &launch);
        let (load, for_root) = cx.update(|cx| super::begin_load(window, cx)).unwrap();
        assert_eq!(for_root.as_deref(), Some(root.as_path()));
        window
            .update(cx, |workbench, window, cx| {
                workbench.set_root(launch.clone(), window, cx)
            })
            .unwrap();
        assert_eq!(
            given_undo(window, cx),
            Some(true),
            "the launch root's, at once"
        );
        let given = |cx: &mut TestAppContext| {
            cx.update(|cx| cx.global::<super::WindowSettings>().given[&window.window_id()].clone())
        };
        let before = given(cx);
        cx.update(|cx| {
            super::finish_load(window, load, for_root, undo_setting(false), Vec::new(), cx);
        });
        assert_eq!(
            given_undo(window, cx),
            Some(true),
            "the old root's load is dropped"
        );
        assert!(
            std::sync::Arc::ptr_eq(&given(cx), &before),
            "and its settings too"
        );
    }

    /// Two loads of the window's settings finishing newest first: the older,
    /// finishing last, is dropped (T483 round 3 F1).
    #[gpui::test]
    fn settings_loads_completing_out_of_order_keep_the_newest(cx: &mut TestAppContext) {
        let (dir, launch) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let root = fs::canonicalize(dir.path()).unwrap();
        let launch = fs::canonicalize(launch.path()).unwrap();
        let window = watched_window(cx, &root, &launch);
        let older = cx.update(|cx| super::begin_load(window, cx)).unwrap();
        let newer = cx.update(|cx| super::begin_load(window, cx)).unwrap();
        cx.update(|cx| {
            super::finish_load(
                window,
                newer.0,
                newer.1,
                undo_setting(false),
                Vec::new(),
                cx,
            );
        });
        assert_eq!(
            given_undo(window, cx),
            Some(false),
            "the newest load applies"
        );
        cx.update(|cx| {
            super::finish_load(window, older.0, older.1, undo_setting(true), Vec::new(), cx);
        });
        assert_eq!(given_undo(window, cx), Some(false), "the older is dropped");
    }

    /// A change to the window's root's project file outdates a load of its
    /// settings already running, so the stale read, finishing later, gives
    /// nothing (T512 review F1).
    #[gpui::test]
    fn a_project_file_change_outdates_a_load_in_flight(cx: &mut TestAppContext) {
        let (dir, launch) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let root = fs::canonicalize(dir.path()).unwrap();
        let launch = fs::canonicalize(launch.path()).unwrap();
        let window = watched_window(cx, &root, &launch);
        cx.update(|cx| super::follow_settings(Some(launch.clone()), cx));
        let held = cx.update(|cx| super::begin_load(window, cx)).unwrap();
        let project = fe_config::project_settings_path(&root);
        cx.update(|cx| {
            cx.set_global(kiln::platform::config::ChangedFiles(vec![project]));
        });
        cx.update(|cx| {
            super::finish_load(window, held.0, held.1, undo_setting(false), Vec::new(), cx);
        });
        assert_ne!(
            given_undo(window, cx),
            Some(false),
            "the held load is dropped"
        );
    }

    fn enabled(level: Level, target: &str) -> bool {
        let logger = StderrLogger { level: None };
        logger.enabled(&Metadata::builder().level(level).target(target).build())
    }

    /// With no `RUST_LOG`: the binary was renamed from fastedit to tenmoku,
    /// and the host's modules log under the new name unless told otherwise.
    #[test]
    fn application_info_logs_stay_on_without_rust_log() {
        for target in ["fastedit", "fastedit::config", "fe_lsp", module_path!()] {
            assert!(enabled(Level::Info, target), "{target} is the app's");
            assert!(
                !enabled(Level::Debug, target),
                "{target} logs info, not debug"
            );
        }
        assert!(!enabled(Level::Info, "gpui"));
        assert!(enabled(Level::Warn, "gpui"));
    }
}
