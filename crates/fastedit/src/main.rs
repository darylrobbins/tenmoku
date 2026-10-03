//! Standalone fastedit host: `fastedit [--perf] [file]`.
//!
//! The editor fills the window; the project panel on the right is a
//! placeholder until the workbench lands (M3).

mod perf;

use std::path::PathBuf;

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use fe_gpui::{EditorTheme, EditorView};
use gpui::{
    App, AppContext as _, Context, Entity, Focusable as _, IntoElement, KeyBinding,
    ParentElement as _, Render, Styled as _, TitlebarOptions, Window, WindowBounds, WindowOptions,
    actions, div, px, rgb, size,
};

actions!(fastedit, [Quit]);

const WELCOME: &str = "\
fastedit

Open a file with:  cargo run -p fastedit -- path/to/file

Keys: arrows, alt-arrows (words), cmd-arrows (line/document),
shift to select, double/triple-click, cmd-c/x/v, cmd-z/shift-cmd-z, cmd-s.
";

struct Workbench {
    editor: Entity<EditorView>,
}

impl Workbench {
    fn title(&self, cx: &App) -> String {
        let model = self.editor.read(cx).model();
        let name = model
            .path()
            .and_then(|path| path.file_name())
            .map_or("untitled".into(), |name| {
                name.to_string_lossy().into_owned()
            });
        let dirty = if model.is_dirty() { " ●" } else { "" };
        format!("{name}{dirty} — fastedit")
    }
}

impl Render for Workbench {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        window.set_window_title(&self.title(cx));
        div()
            .size_full()
            .flex()
            .bg(rgb(0x16181d))
            .text_color(rgb(0x8b8f98))
            .text_size(px(12.))
            .child(div().flex_1().h_full().child(self.editor.clone()))
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

fn load(path: Option<PathBuf>) -> EditorModel {
    let Some(path) = path else {
        return EditorModel::new(Buffer::new(WELCOME), None);
    };
    match EditorModel::open(&path) {
        Ok(model) => model,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // A new file: saving creates it.
            EditorModel::new(Buffer::new(""), Some(path))
        }
        Err(error) => {
            eprintln!("fastedit: cannot open {}: {error}", path.display());
            std::process::exit(1);
        }
    }
}

fn main() {
    let started = std::time::Instant::now();
    let mut args: Vec<_> = std::env::args_os().skip(1).collect();
    let perf = args.first().is_some_and(|arg| arg == "--perf");
    if perf {
        args.remove(0);
    }
    let path = args.into_iter().next().map(PathBuf::from);
    let model = load(path);
    eprintln!(
        "fastedit: loaded {} bytes, {} rows in {:.1?}",
        model.text().len(),
        model.text().row_count(),
        started.elapsed()
    );

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
                |_, cx| {
                    let editor = cx.new(|cx| EditorView::new(model, theme, cx));
                    cx.new(|cx| {
                        // Re-render (and retitle) whenever the editor changes.
                        cx.observe(&editor, |_, _, cx| cx.notify()).detach();
                        Workbench { editor }
                    })
                },
            )
            .expect("failed to open the main window");
        window
            .update(cx, |workbench, window, cx| {
                window.focus(&workbench.editor.focus_handle(cx), cx);
                if perf {
                    perf::start(workbench.editor.clone(), started, window, cx);
                }
            })
            .ok();
        cx.activate(true);
    });
}
