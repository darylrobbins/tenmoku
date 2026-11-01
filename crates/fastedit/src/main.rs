//! Tenmoku, the standalone host: `tenmoku [options] [path[:line[:column]] ...]`
//! ([`cli`]), a [`Workbench`] in a window.

mod app_icon;
mod cli;
mod config;
// Its only caller is the grammar prompt's Install action, the next card.
#[allow(dead_code)]
mod grammar_install;
#[cfg(test)]
mod grammar_install_tests;
mod ipc;
mod perf;
mod theme;
mod vim_remaps;
mod wait;
mod workspace_extensions;

use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
    sync::Arc,
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
        let (jump_on_s, jump_style) = (now.vim.jump_on_s, now.vim.jump_style);
        let (ignorecase, smartcase, incsearch) =
            (now.vim.ignorecase, now.vim.smartcase, now.vim.incsearch);
        let inccommand = now.vim.inccommand;
        let replace_with_register = now.vim.replace_with_register;
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
    let mut startup = Startup::new();
    let started = Instant::now();
    init_logging();
    let mut how = ipc::How::current();
    let mut cli = cli::parse(std::env::args_os().skip(1).collect()).unwrap_or_else(|error| {
        error.print();
        std::process::exit(error.exit_code())
    });
    let perf = cli.perf;
    how.no_ipc |= perf;
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

    use super::{EditorTheme, Launch, StderrLogger, Target, Workbench, files_left, open_files};

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
        let root = fs::canonicalize(dir.path()).unwrap();
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
