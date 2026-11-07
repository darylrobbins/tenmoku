//! `audit <out-dir>`: every surface the UI polish audit looks at
//! (`docs/decisions/25w38k498b-ui-polish-audit.md`), dark and light, as
//! `<out-dir>/<surface>_<dark|light>.png`, then the chrome under every
//! bundled theme as `<out-dir>/<theme id>.png`. Nothing is compared with a
//! golden, and the default run doesn't render these.
//!
//! Each surface is the demo project in a workbench, as [`capture_with`]
//! opens it, then a few steps, each on its own frame (`Window::on_next_frame`,
//! which the capture's settling delivers): a command dispatched as a key
//! would dispatch it, or a call into the workbench's public API. A step
//! notifies the workbench, so the capture draws again after it.
//!
//! The run re-executes itself with Tenmoku's config, state and cache
//! directories on a fresh temporary folder, whatever the environment
//! already says, so the Settings and Keyboard Shortcuts tabs read no real
//! user file and the demo folders it opens are recorded in a throwaway
//! `recent.toml`, not the user's. The child checks it was started that way
//! (`AUDIT_ROOT`) before it opens anything. The prompt scenes come last:
//! they turn on trust prompting (`set_trust_file`) and the in-window
//! prompt, which nothing turns off.

use std::{
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use fe_command::Invocation;
use fe_config::themes;
use fe_gpui::{HoverContent, commands::RunCommand};
use fe_ui::Appearance;
use fe_workbench::Workbench;
use gpui::{Context, Entity, VisualTestAppContext, Window};
use kiln_platform::screenshots::Result;

use crate::scenes::{SCENE_SIZE, capture_with, use_settings, write_project};

/// The variable that marks the child run: the temporary folder whose
/// `config`, `state` and `cache` the other variables point at.
const AUDIT_ROOT: &str = "TENMOKU_AUDIT_ROOT";

/// What the child's directories are, under [`AUDIT_ROOT`]: the variable and
/// its folder.
const ISOLATED: [(&str, &str); 3] = [
    ("TENMOKU_CONFIG_DIR", "config"),
    ("TENMOKU_STATE_DIR", "state"),
    ("XDG_CACHE_HOME", "cache"),
];

/// A step of a surface's scene, run in the workbench on a frame of its own.
type Step = Box<dyn FnOnce(&mut Workbench, &mut Window, &mut Context<Workbench>)>;

/// A surface: its file name's stem, whether the demo's two files are open,
/// whether the project is a Git repository, settings beside the theme's,
/// and its steps, given the project's root.
struct Surface {
    name: &'static str,
    open_demo: bool,
    git: bool,
    settings: &'static str,
    steps: fn(&Path) -> Vec<Step>,
}

/// The surfaces, in the order the audit's Captures table lists them. The
/// trust prompt isn't here: see [`prompt`]. Nor is the completion menu: the
/// workbench puts its language servers' provider back on every editor
/// after each draw (`sync_lsp`), which closes a menu another provider
/// opened.
const SURFACES: &[Surface] = &[
    Surface {
        name: "chrome",
        open_demo: true,
        git: false,
        settings: "",
        steps: |_| Vec::new(),
    },
    Surface {
        name: "project_tree_git",
        open_demo: true,
        git: true,
        settings: "",
        steps: |_| Vec::new(),
    },
    Surface {
        name: "start_page",
        open_demo: false,
        git: false,
        settings: "",
        steps: |_| Vec::new(),
    },
    Surface {
        name: "settings_editor",
        open_demo: true,
        git: false,
        settings: "",
        steps: |_| vec![run("workbench.open_settings_ui")],
    },
    Surface {
        name: "shortcuts_editor",
        open_demo: true,
        git: false,
        settings: "",
        steps: |_| vec![run("workbench.open_keyboard_shortcuts")],
    },
    Surface {
        name: "command_palette",
        open_demo: true,
        git: false,
        settings: "",
        steps: |_| vec![run("command_palette.toggle")],
    },
    Surface {
        name: "file_finder",
        open_demo: true,
        git: false,
        settings: "",
        steps: |_| vec![run("quick_open.toggle")],
    },
    Surface {
        name: "find_bar",
        open_demo: true,
        git: false,
        settings: "",
        steps: |_| vec![select("counts", 0), run("find.show")],
    },
    Surface {
        name: "search_panel",
        open_demo: true,
        git: false,
        settings: "",
        steps: |_| vec![run("search.show"), search("count")],
    },
    Surface {
        name: "toast",
        open_demo: true,
        git: false,
        settings: "",
        steps: |_| vec![toast()],
    },
    Surface {
        name: "git_panel",
        open_demo: true,
        git: true,
        settings: "",
        steps: |_| vec![run("git.show_changes")],
    },
    Surface {
        name: "git_log",
        open_demo: true,
        git: true,
        settings: "",
        steps: |_| vec![run("git.log")],
    },
    Surface {
        name: "problems_panel",
        open_demo: true,
        git: false,
        settings: "",
        steps: |_| vec![run("problems.show")],
    },
    Surface {
        name: "hover",
        open_demo: true,
        git: false,
        settings: "",
        steps: |_| vec![select("HashMap::new", 0), hover()],
    },
];

/// The two appearances every surface is rendered in: the suffix, the
/// bundled theme and the settings that choose it.
const APPEARANCES: [(&str, &str, &str); 2] = [
    ("dark", "tenmoku-dark", ""),
    ("light", "tenmoku-light", "[theme]\nmode = \"light\"\n"),
];

/// `audit <out-dir>`: renders the surfaces into `out-dir`. Re-runs itself
/// first with its directories on a temporary folder ([`isolated`]).
pub(crate) fn main(cx: &mut VisualTestAppContext, args: &[String]) -> Result<bool> {
    let [out] = args else {
        return Err("usage: screenshots audit <out-dir>".into());
    };
    let Some(root) = std::env::var_os(AUDIT_ROOT) else {
        let root = tempfile::tempdir()?;
        let mut child = Command::new(std::env::current_exe()?);
        child.arg("audit").arg(out).env(AUDIT_ROOT, root.path());
        for (var, dir) in ISOLATED {
            let dir = root.path().join(dir);
            fs::create_dir_all(&dir)?;
            child.env(var, dir);
        }
        return Ok(child.status()?.success());
    };
    isolated(Path::new(&root))?;
    let out = PathBuf::from(out);
    fs::create_dir_all(&out)?;
    let mut written = 0;
    for surface in SURFACES {
        for (suffix, theme, settings) in APPEARANCES {
            let settings = format!("{settings}{}", surface.settings);
            let path = out.join(format!("{}_{suffix}.png", surface.name));
            render(cx, surface, theme, &settings, &path)?;
            written += 1;
        }
    }
    for theme in themes::bundled_themes() {
        let settings = match theme.appearance {
            Appearance::Light => "[theme]\nmode = \"light\"\n",
            _ => "",
        };
        let path = out.join(format!("{}.png", theme.id));
        render(cx, &SURFACES[0], theme.id, settings, &path)?;
        written += 1;
    }
    for (suffix, theme, settings) in APPEARANCES {
        let path = out.join(format!("prompt_{suffix}.png"));
        prompt(cx, theme, settings, &path)?;
        written += 1;
    }
    println!("wrote {written} captures to {}", out.display());
    Ok(true)
}

/// Fails unless every directory Tenmoku reads or records in is the
/// parent's temporary one under `root`: a child started some other way,
/// say with `TENMOKU_AUDIT_ROOT` set by hand, would touch the user's.
fn isolated(root: &Path) -> Result<()> {
    for (var, dir) in ISOLATED {
        let expected = root.join(dir);
        if std::env::var_os(var).as_deref() != Some(expected.as_os_str()) {
            return Err(format!("{var} isn't {}: not starting", expected.display()).into());
        }
    }
    let paths = fe_config::paths::app_paths();
    for (got, dir) in [
        (paths.config_dir(), "config"),
        (paths.state_dir(), "state"),
        (paths.cache_dir(), "cache"),
    ] {
        if !got.is_some_and(|got| got.starts_with(root.join(dir))) {
            return Err(format!("Tenmoku's {dir} directory is {got:?}: not starting").into());
        }
    }
    Ok(())
}

/// Renders `surface` under bundled theme `theme` with `settings` to `path`.
fn render(
    cx: &mut VisualTestAppContext,
    surface: &Surface,
    theme: &str,
    settings: &str,
    path: &Path,
) -> Result<()> {
    let parent = tempfile::tempdir()?;
    let root = write_project(parent.path())?;
    if surface.git {
        git_history(&root)?;
    }
    let palette = use_settings(cx, parent.path(), theme, settings)?;
    let steps = (surface.steps)(&root);
    let image = capture_with(cx, &root, SCENE_SIZE, surface.open_demo, &palette, {
        move |_, window, cx| chain(cx.entity(), steps.into(), window)
    })?;
    image.save(path)?;
    println!("wrote {}", path.display());
    Ok(())
}

/// The folder trust prompt as it shows the first time a folder is opened,
/// drawn in the window by kiln's prompt (`kiln_ui::prompt_renderer`), as
/// where there are no native alerts. On macOS the app shows an NSAlert
/// sheet instead, which a capture off screen can't draw.
fn prompt(cx: &mut VisualTestAppContext, theme: &str, settings: &str, path: &Path) -> Result<()> {
    let parent = tempfile::tempdir()?;
    let root = write_project(parent.path())?;
    let trust = parent.path().join("trust.toml");
    cx.update(|cx| {
        fe_workbench::set_trust_file(cx, trust);
        cx.set_prompt_builder(fe_ui::prompt_renderer);
    });
    let palette = use_settings(cx, parent.path(), theme, settings)?;
    // The root's trust resolves off the main thread: draw a few frames for
    // the prompt to come.
    let steps: Vec<Step> = (0..3).map(|_| -> Step { Box::new(|_, _, _| {}) }).collect();
    let image = capture_with(cx, &root, SCENE_SIZE, true, &palette, {
        move |_, window, cx| chain(cx.entity(), steps.into(), window)
    })?;
    image.save(path)?;
    println!("wrote {}", path.display());
    Ok(())
}

/// Runs `steps` in `workbench`, one a frame from the next, each notifying it.
fn chain(workbench: Entity<Workbench>, mut steps: VecDeque<Step>, window: &mut Window) {
    let Some(step) = steps.pop_front() else {
        return;
    };
    window.on_next_frame(move |window, cx| {
        workbench.update(cx, |workbench, cx| {
            step(workbench, window, cx);
            cx.notify();
        });
        chain(workbench, steps, window);
    });
}

/// Dispatches command `id` from the focused element, in its context, as
/// its key would.
fn run(id: &'static str) -> Step {
    Box::new(move |_, window, cx| {
        let context = kiln_platform::keys::context_stack(&window.context_stack(), cx);
        let invocation = Invocation::of(id).expect("a command ID").context(context);
        window.dispatch_action(Box::new(RunCommand(invocation)), cx);
    })
}

/// Selects the `nth` (from 0) `text` in the active editor, or puts the
/// caret at its start if `text` is a call path (has `::`).
fn select(text: &'static str, nth: usize) -> Step {
    Box::new(move |workbench, _, cx| {
        let Some(editor) = workbench.active_editor(cx).cloned() else {
            return;
        };
        editor.update(cx, |editor, cx| {
            let model = editor.model_mut();
            let found = model.text().to_string();
            let Some((start, _)) = found.match_indices(text).nth(nth) else {
                return;
            };
            let end = if text.contains("::") {
                start
            } else {
                start + text.len()
            };
            model.select_range(start..end, false);
            cx.notify();
        });
    })
}

/// Types `query` into the search panel's input and runs the search.
fn search(query: &'static str) -> Step {
    Box::new(move |workbench, _, cx| {
        let panel = workbench.dock().read(cx).search().clone();
        let input = panel.read(cx).input().clone();
        input.update(cx, |input, cx| input.set_text(query, cx));
        panel.update(cx, |panel, cx| panel.run(cx));
    })
}

/// A warning toast with a detail line, as a failed save shows one.
fn toast() -> Step {
    Box::new(|_, _, cx| {
        Workbench::notify(
            cx,
            log::Level::Warn,
            "Couldn't save src/main.rs".into(),
            Some("Permission denied (os error 13)".into()),
        );
    })
}

/// A hover popover at the caret, with a signature and a sentence, as
/// rust-analyzer's hover on `HashMap::new` reads.
fn hover() -> Step {
    Box::new(|workbench, _, cx| {
        let Some(editor) = workbench.active_editor(cx).cloned() else {
            return;
        };
        let text = "```rust\nstd::collections::hash::map::HashMap\n\npub fn new() -> \
                    HashMap<K, V, RandomState>\n```\n\n---\n\nCreates an empty `HashMap`.\n\n\
                    The hash map is initially created with a capacity of 0, so it will not \
                    allocate until it is first inserted into."
            .to_owned();
        editor.update(cx, |editor, cx| {
            editor.show_popover(HoverContent { text, range: None }, cx)
        });
    })
}

/// Makes `root` a repository with two commits, then changes it: a modified
/// file, a staged new one and an untracked one. Dates and names are fixed.
fn git_history(root: &Path) -> Result<()> {
    let git = |args: &[&str]| -> Result<()> {
        let status = Command::new("git")
            .args(args)
            .current_dir(root)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "Ada Lovelace")
            .env("GIT_AUTHOR_EMAIL", "ada@example.com")
            .env("GIT_COMMITTER_NAME", "Ada Lovelace")
            .env("GIT_COMMITTER_EMAIL", "ada@example.com")
            .env("GIT_AUTHOR_DATE", "2026-11-01T10:00:00Z")
            .env("GIT_COMMITTER_DATE", "2026-11-01T10:00:00Z")
            .status()?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("git {args:?}: {status}").into())
        }
    };
    git(&["init", "-q", "-b", "main"])?;
    git(&["add", "."])?;
    git(&["commit", "-q", "-m", "Count words per line"])?;
    fs::write(
        root.join("README.md"),
        "# demo\n\nA small project to look at.\n\nRun it with `cargo run -- <lines>`.\n",
    )?;
    git(&[
        "commit",
        "-q",
        "-am",
        "Say how to run it\n\nCo-authored-by: Grace Hopper <grace@example.com>",
    ])?;
    let main_rs = root.join("src/main.rs");
    let text = fs::read_to_string(&main_rs)?;
    fs::write(&main_rs, text.replace("Counts the words", "Counts words"))?;
    fs::write(root.join("src/util/split.rs"), "pub fn split() {}\n")?;
    git(&["add", "src/util/split.rs"])?;
    fs::write(root.join("NOTES.md"), "- try a BTreeMap\n")?;
    Ok(())
}
