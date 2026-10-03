//! Standalone fastedit host: `fastedit [--perf] [file]`.
//!
//! The editor fills the window; the project panel on the right is a
//! placeholder until the workbench lands (M3).

mod loader;
mod perf;

use std::{
    path::{Path, PathBuf},
    time::Instant,
};

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use fe_gpui::{EditorTheme, EditorView};
use gpui::{
    App, AppContext as _, Context, Entity, Focusable as _, IntoElement, KeyBinding,
    ParentElement as _, Render, Styled as _, TitlebarOptions, Window, WindowBounds, WindowHandle,
    WindowOptions, actions, div, px, rgb, size,
};

actions!(fastedit, [Quit]);

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
}

impl Workbench {
    fn new(path: Option<PathBuf>, theme: EditorTheme, cx: &mut Context<Self>) -> Self {
        let content = match path {
            Some(path) => Content::Loading { path },
            None => Content::Ready {
                editor: Self::new_editor(
                    EditorModel::new(Buffer::new(WELCOME), None),
                    theme.clone(),
                    cx,
                ),
            },
        };
        Self { content, theme }
    }

    fn new_editor(
        model: EditorModel,
        theme: EditorTheme,
        cx: &mut Context<Self>,
    ) -> Entity<EditorView> {
        let editor = cx.new(|cx| EditorView::new(model, theme, cx));
        // Re-render (and retitle) whenever the editor changes.
        cx.observe(&editor, |_, _, cx| cx.notify()).detach();
        editor
    }

    /// Replaces the loading placeholder with the editor and focuses it.
    fn show(&mut self, model: EditorModel, window: &mut Window, cx: &mut Context<Self>) {
        let editor = Self::new_editor(model, self.theme.clone(), cx);
        window.focus(&editor.focus_handle(cx), cx);
        self.content = Content::Ready { editor };
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

/// Reads `path` on a background thread, then shows it in the window. A read
/// error quits with exit code 1.
fn open_in_background(
    path: PathBuf,
    window: WindowHandle<Workbench>,
    started: Instant,
    cx: &mut App,
) {
    let read = cx.background_spawn({
        let path = path.clone();
        async move { loader::read(path) }
    });
    cx.spawn(async move |cx| {
        let buffer = match read.await {
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
        let model = EditorModel::new(buffer, Some(path));
        window
            .update(cx, |workbench, window, cx| {
                workbench.show(model, window, cx)
            })
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
    let started = Instant::now();
    init_logging();
    let mut args: Vec<_> = std::env::args_os().skip(1).collect();
    let perf = args.first().is_some_and(|arg| arg == "--perf");
    if perf {
        args.remove(0);
    }
    let path = args.into_iter().next().map(PathBuf::from);

    gpui_platform::application().run(move |cx: &mut App| {
        fe_gpui::init(cx);
        cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        let theme = EditorTheme::default_dark(cx);
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
                {
                    let path = path.clone();
                    |_, cx| cx.new(|cx| Workbench::new(path, theme, cx))
                },
            )
            .expect("failed to open the main window");
        window
            .update(cx, |workbench, window, cx| {
                if let Some(editor) = workbench.editor() {
                    window.focus(&editor.focus_handle(cx), cx);
                }
                if perf {
                    perf::start(cx.entity(), started, window);
                }
            })
            .ok();
        if let Some(path) = path {
            open_in_background(path, window, started, cx);
        }
        cx.activate(true);
    });
}
