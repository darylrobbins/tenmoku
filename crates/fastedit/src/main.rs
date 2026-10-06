//! Standalone fastedit host: `fastedit [--perf] [file]`.
//!
//! The editor fills the window; the project panel on the right is a
//! placeholder until the workbench lands (M3).

mod loader;
mod perf;

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

use perf::Startup;

use fe_buffer::Buffer;
use fe_config::{ConfigWatcher, Diagnostic, Settings, SettingsEvent, SettingsStore};
use fe_editor::EditorModel;
use fe_gpui::{
    EditorSettings, EditorTheme, EditorView,
    fe_keymap::{self, KeymapStore},
};
use gpui::{
    App, AppContext as _, Context, Entity, Focusable as _, IntoElement, ParentElement as _, Render,
    Styled as _, TitlebarOptions, Window, WindowBounds, WindowHandle, WindowOptions, div, px, rgb,
    size,
};

const WELCOME: &str = "\
fastedit

Open a file with:  cargo run -p fastedit -- path/to/file

Keys: arrows, alt-arrows (words), cmd-arrows (line/document),
shift to select, double/triple-click, cmd-c/x/v, cmd-z/shift-cmd-z, cmd-s.
";

enum Content {
    Loading { path: PathBuf },
    Ready { editor: Entity<EditorView> },
}

struct Workbench {
    content: Content,
    theme: EditorTheme,
    /// For editors yet to open.
    settings: EditorSettings,
}

impl Workbench {
    /// `file` is the file to show: its path, and its buffer if it's read.
    fn new(
        file: Option<(PathBuf, Option<Buffer>)>,
        theme: EditorTheme,
        settings: EditorSettings,
        cx: &mut Context<Self>,
    ) -> Self {
        let model = match file {
            None => EditorModel::new(Buffer::new(WELCOME), None),
            Some((path, Some(buffer))) => EditorModel::new(buffer, Some(path)),
            Some((path, None)) => {
                return Self {
                    content: Content::Loading { path },
                    theme,
                    settings,
                };
            }
        };
        let editor = Self::new_editor(model, theme.clone(), &settings, cx);
        Self {
            content: Content::Ready { editor },
            theme,
            settings,
        }
    }

    fn new_editor(
        mut model: EditorModel,
        theme: EditorTheme,
        settings: &EditorSettings,
        cx: &mut Context<Self>,
    ) -> Entity<EditorView> {
        if model.tab_size() != settings.tab_size {
            model.set_tab_size(settings.tab_size);
        }
        if model.soft_wrap() != settings.soft_wrap {
            model.set_soft_wrap(settings.soft_wrap);
        }
        let editor = cx.new(|cx| EditorView::new(model, theme, cx));
        // Re-render (and retitle) whenever the editor changes.
        cx.observe(&editor, |_, _, cx| cx.notify()).detach();
        editor
    }

    /// Replaces the loading placeholder with the editor and focuses it.
    fn show(&mut self, model: EditorModel, window: &mut Window, cx: &mut Context<Self>) {
        let editor = Self::new_editor(model, self.theme.clone(), &self.settings, cx);
        window.focus(&editor.focus_handle(cx), cx);
        self.content = Content::Ready { editor };
        cx.notify();
    }

    /// Applies reloaded settings to the open editor and to editors yet to
    /// open.
    fn reload_settings(&mut self, event: &SettingsEvent, cx: &mut Context<Self>) {
        self.settings = event.settings.editor.clone();
        self.theme.set_metrics(&self.settings, cx);
        if let Some(editor) = self.editor() {
            editor.update(cx, |editor, cx| editor.reload_settings(event, cx));
        }
        cx.notify();
    }

    fn editor(&self) -> Option<&Entity<EditorView>> {
        match &self.content {
            Content::Loading { .. } => None,
            Content::Ready { editor } => Some(editor),
        }
    }

    fn title(&self, cx: &App) -> String {
        match &self.content {
            Content::Loading { path } => format!("{} — loading", file_name(Some(path))),
            Content::Ready { editor } => {
                let model = editor.read(cx).model();
                let dirty = if model.is_dirty() { " ●" } else { "" };
                format!("{}{dirty} — fastedit", file_name(model.path()))
            }
        }
    }

    fn loading_pane(&self, path: &Path) -> impl IntoElement {
        div()
            .size_full()
            .bg(self.theme.background)
            .text_color(self.theme.gutter_foreground)
            .px_3()
            .py_2()
            .child(format!("Loading {}…", file_name(Some(path))))
    }
}

fn file_name(path: Option<&Path>) -> String {
    path.and_then(|path| path.file_name())
        .map_or("untitled".into(), |name| {
            name.to_string_lossy().into_owned()
        })
}

impl Render for Workbench {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        window.set_window_title(&self.title(cx));
        let main = match &self.content {
            Content::Loading { path } => self.loading_pane(path).into_any_element(),
            Content::Ready { editor } => editor.clone().into_any_element(),
        };
        div()
            .size_full()
            .flex()
            .bg(rgb(0x16181d))
            .text_color(rgb(0x8b8f98))
            .text_size(px(12.))
            .child(div().flex_1().h_full().child(main))
            .child(
                div()
                    .w(px(240.))
                    .h_full()
                    .border_l_1()
                    .border_color(rgb(0x23262d))
                    .bg(rgb(0x121418))
                    .px_3()
                    .py_2()
                    .child("PROJECT"),
            )
    }
}

/// The buffer a finished read produced. A read error quits with exit code 1.
fn loaded(path: &Path, read: loader::Read, started: Instant) -> Buffer {
    perf::file_loaded(read.at);
    let buffer = match read.result {
        Ok(Some(buffer)) => buffer,
        // A new file: saving creates it.
        Ok(None) => Buffer::new(""),
        Err(error) => {
            log::error!("cannot open {}: {error}", path.display());
            std::process::exit(1);
        }
    };
    log::info!(
        "loaded {} bytes, {} rows in {:.1?}",
        buffer.len(),
        buffer.text().row_count(),
        started.elapsed()
    );
    buffer
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
        let buffer = loaded(&path, read.await, started);
        let model = EditorModel::new(buffer, Some(path));
        window
            .update(cx, |workbench, window, cx| {
                workbench.show(model, window, cx)
            })
            .ok();
    })
    .detach();
}

/// The settings store: the user's settings file and, with a file to open,
/// the project's, rooted at the file's directory.
fn load_settings(file: Option<&Path>) -> SettingsStore {
    let root = file
        .and_then(|file| std::path::absolute(file).ok())
        .and_then(|file| file.parent().map(Path::to_owned));
    let project = root.map(|root| fe_config::project_settings_path(&root));
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
/// checked against the editor's and the app's commands.
fn load_keymap() -> KeymapStore {
    let store = KeymapStore::load(
        fe_keymap::DEFAULT_MACOS,
        fe_keymap::user_keymap_path(),
        Some(fe_gpui::commands::command_catalog()),
    );
    log_diagnostics(store.diagnostics());
    store
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
    let reading = args
        .into_iter()
        .next()
        .map(|path| loader::start(PathBuf::from(path)));
    let store = load_settings(reading.as_ref().map(loader::Reading::path));
    let settings: Arc<Settings> = store.settings().clone();
    let keymap = load_keymap();

    let application = gpui_platform::application();
    startup.platform = Some(Instant::now());
    application.run(move |cx: &mut App| {
        startup.app = Some(Instant::now());
        fe_gpui::init_with_keymap(cx, keymap.keymap().clone());
        log_keymap_report(cx);
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        let theme = EditorTheme::from_settings(&settings.editor, cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::centered(size(px(1200.), px(800.)), cx)),
                    titlebar: Some(TitlebarOptions {
                        title: Some("fastedit".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                |window, cx| {
                    startup.view = Some(Instant::now());
                    // Show the file on the first frame if it's read by now.
                    let file = reading.as_ref().map(|reading| {
                        let buffer = reading
                            .try_finish()
                            .map(|read| loaded(reading.path(), read, started));
                        (reading.path().to_owned(), buffer)
                    });
                    let editor_settings = settings.editor.clone();
                    let workbench = cx.new(|cx| Workbench::new(file, theme, editor_settings, cx));
                    // Focus before the window's first draw: focusing after
                    // it would make the first frame request draw again.
                    let focus = workbench
                        .read(cx)
                        .editor()
                        .map(|editor| editor.focus_handle(cx));
                    if let Some(focus) = focus {
                        window.focus(&focus, cx);
                    }
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
                workbench.editor().is_none()
            })
            .unwrap_or(false);
        if loading && let Some(reading) = reading {
            open_in_background(reading, window, started, cx);
        }
        // Not before the window opens: watching isn't needed for the first
        // frame.
        watch_config(store, keymap, window, cx);
        cx.activate(true);
    });
}
