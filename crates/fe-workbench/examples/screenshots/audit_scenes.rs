//! `audit <out-dir>`: every surface the UI polish audit looks at
//! (`docs/decisions/25w38k498b-ui-polish-audit.md`), dark (Yohen) and light
//! (Hakuji), as `<out-dir>/<surface>_<dark|light>.png`, then the chrome under
//! every bundled theme as `<out-dir>/<theme id>.png`. Nothing is compared with
//! a golden, and the default run doesn't render these. Each capture's token
//! probe points are checked (`token_probe::check_points`), and the run fails
//! if any is off, if a surface has none, or if a step couldn't do its part.
//!
//! This file is the runner. Each surface is a file of its own in `audit/`,
//! with its steps and its probes: a new surface adds its file, its `mod`
//! line below and its entry in [`SURFACES`] (or [`LAST`]).
//!
//! Each surface is the demo project in a workbench, as [`capture_with`]
//! opens it, then a few steps, each on its own frame (`Window::on_next_frame`,
//! which the capture's settling delivers): a command dispatched as a key
//! would dispatch it, or a call into the workbench's public API. A step
//! notifies the workbench, so the capture draws again after it.
//!
//! Like every mode of the example, the audit runs with Tenmoku's config,
//! state and cache directories on a fresh temporary folder (`main.rs`'s
//! `isolate`), so the Settings and Keyboard Shortcuts tabs read no real user
//! file and the demo folders it opens are recorded in a throwaway
//! `recent.toml`.

#[path = "audit/chrome.rs"]
mod chrome;
#[path = "audit/command_palette.rs"]
mod command_palette;
#[path = "audit/file_finder.rs"]
mod file_finder;
#[path = "audit/find_bar.rs"]
mod find_bar;
#[path = "audit/git_log.rs"]
mod git_log;
#[path = "audit/git_log_card.rs"]
mod git_log_card;
#[path = "audit/git_panel.rs"]
mod git_panel;
#[path = "audit/hover.rs"]
mod hover;
#[path = "audit/problems_panel.rs"]
mod problems_panel;
#[path = "audit/problems_rows.rs"]
mod problems_rows;
#[path = "audit/project_tree_git.rs"]
mod project_tree_git;
#[path = "audit/prompt.rs"]
mod prompt;
#[path = "audit/search_panel.rs"]
mod search_panel;
#[path = "audit/settings.rs"]
mod settings;
#[path = "audit/shortcuts_editor.rs"]
mod shortcuts_editor;
#[path = "audit/start_page.rs"]
mod start_page;
#[path = "audit/terminal_session.rs"]
mod terminal_session;
#[path = "audit/toast.rs"]
mod toast;

use std::{
    cell::RefCell,
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use fe_command::Invocation;
use fe_config::themes;
use fe_gpui::commands::RunCommand;
use fe_ui::Appearance;
use fe_workbench::Workbench;
use gpui::{Context, Entity, VisualTestAppContext, Window};
use kiln_platform::screenshots::Result;

use crate::{
    scenes::{SCENE_SIZE, bundled_theme, capture_with, use_settings, write_project},
    token_probe::{Probe, check_points},
};

/// The surfaces, in the order they're captured: T1429's first, as the
/// audit's Captures table lists them. The first, `chrome`, is also the scene
/// every bundled theme is captured in.
const SURFACES: &[&Surface] = &[
    &chrome::SURFACE,
    &project_tree_git::SURFACE,
    &start_page::SURFACE,
    &settings::SURFACE,
    &shortcuts_editor::SURFACE,
    &command_palette::SURFACE,
    &file_finder::SURFACE,
    &find_bar::SURFACE,
    &search_panel::SURFACE,
    &toast::SURFACE,
    &git_panel::SURFACE,
    &git_log::SURFACE,
    &problems_panel::SURFACE,
    &hover::SURFACE,
    &problems_rows::SURFACE,
    &terminal_session::SURFACE,
    &git_log_card::SURFACE,
];

/// The surfaces captured after the bundled themes' chrome: their setup
/// leaves the app in a state nothing undoes.
const LAST: &[&Surface] = &[&prompt::SURFACE];

/// A step of a surface's scene, run in the workbench on a frame of its own.
pub(crate) type Step = Box<dyn FnOnce(&mut Workbench, &mut Window, &mut Context<Workbench>)>;

/// A surface: its file name's stem, whether the demo's two files are open,
/// whether the project is a Git repository, settings beside the theme's,
/// what's done to the app before its workbench opens (given the project's
/// parent folder), its steps (given the project's root), and the points its
/// captures are probed at.
pub(crate) struct Surface {
    pub(crate) name: &'static str,
    pub(crate) open_demo: bool,
    pub(crate) git: bool,
    pub(crate) settings: &'static str,
    pub(crate) setup: fn(&mut VisualTestAppContext, &Path),
    pub(crate) steps: fn(&Path) -> Vec<Step>,
    pub(crate) probes: &'static [Probe],
}

/// What a surface leaves out: the demo's files open in a plain folder, no
/// settings, setup or steps. Its probes, which every surface needs, are none.
pub(crate) const DEFAULT: Surface = Surface {
    name: "",
    open_demo: true,
    git: false,
    settings: "",
    setup: |_, _| {},
    steps: |_| Vec::new(),
    probes: &[],
};

/// The two appearances every surface is rendered in: the suffix, the
/// bundled theme and the settings that choose it, the defaults.
const APPEARANCES: [(&str, &str, &str); 2] = [
    ("dark", themes::DEFAULT_DARK, ""),
    (
        "light",
        themes::DEFAULT_LIGHT,
        "[theme]\nmode = \"light\"\n",
    ),
];

thread_local! {
    /// Why a step of the scene being captured couldn't do its part, if one
    /// couldn't ([`fail`]).
    static FAILED: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Fails the scene being captured with `why`: a step whose part didn't
/// happen, so the capture wouldn't show its surface.
pub(crate) fn fail(why: String) {
    FAILED.with(|failed| *failed.borrow_mut() = Some(why));
}

/// `audit <out-dir>`: renders the surfaces into `out-dir` and checks their
/// probes. Returns whether every probe passed; fails before rendering
/// anything if a surface has no probes.
pub(crate) fn main(cx: &mut VisualTestAppContext, args: &[String]) -> Result<bool> {
    let [out] = args else {
        return Err("usage: screenshots audit <out-dir>".into());
    };
    if let Some(surface) = (SURFACES.iter().chain(LAST)).find(|surface| surface.probes.is_empty()) {
        return Err(format!("audit surface {} has no probes", surface.name).into());
    }
    let out = PathBuf::from(out);
    fs::create_dir_all(&out)?;
    let (mut written, mut failed) = render_each(cx, SURFACES, &out)?;
    for theme in themes::bundled_themes() {
        let settings = match theme.appearance {
            Appearance::Light => "[theme]\nmode = \"light\"\n",
            _ => "",
        };
        failed += usize::from(!render(
            cx,
            SURFACES[0],
            theme.id,
            settings,
            &out,
            theme.id,
        )?);
        written += 1;
    }
    let (last_written, last_failed) = render_each(cx, LAST, &out)?;
    written += last_written;
    failed += last_failed;
    println!("wrote {written} captures to {}", out.display());
    if failed > 0 {
        println!("FAILED  {failed} captures' token probes");
    }
    Ok(failed == 0)
}

/// Renders each of `surfaces` in both [`APPEARANCES`] into `out`. Returns how
/// many captures it wrote and how many failed their probes.
fn render_each(
    cx: &mut VisualTestAppContext,
    surfaces: &[&Surface],
    out: &Path,
) -> Result<(usize, usize)> {
    let (mut written, mut failed) = (0, 0);
    for surface in surfaces {
        for (suffix, theme, settings) in APPEARANCES {
            let settings = format!("{settings}{}", surface.settings);
            let name = format!("{}_{suffix}", surface.name);
            failed += usize::from(!render(cx, surface, theme, &settings, out, &name)?);
            written += 1;
        }
    }
    Ok((written, failed))
}

/// Renders `surface` under bundled theme `theme` with `settings` to
/// `<out>/<name>.png`, then checks its probes. Returns whether they passed;
/// fails if a step did ([`fail`]).
fn render(
    cx: &mut VisualTestAppContext,
    surface: &Surface,
    theme: &str,
    settings: &str,
    out: &Path,
    name: &str,
) -> Result<bool> {
    let parent = tempfile::tempdir()?;
    let root = write_project(parent.path())?;
    if surface.git {
        git_history(&root)?;
    }
    (surface.setup)(cx, parent.path());
    let palette = use_settings(cx, parent.path(), theme, settings)?;
    let steps = (surface.steps)(&root);
    FAILED.with(|failed| failed.borrow_mut().take());
    let image = capture_with(cx, &root, SCENE_SIZE, surface.open_demo, &palette, {
        move |_, window, cx| chain(cx.entity(), steps.into(), window)
    })?;
    if let Some(why) = FAILED.with(|failed| failed.borrow_mut().take()) {
        return Err(format!("{name}: {why}").into());
    }
    let path = out.join(format!("{name}.png"));
    image.save(&path)?;
    println!("wrote {}", path.display());
    let (data, _) = bundled_theme(theme)?;
    check_points(name, &image, &data, surface.probes)
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

/// Does nothing but notify the workbench: a frame more, for work an earlier
/// step started to land and draw.
pub(crate) fn idle() -> Step {
    Box::new(|_, _, _| {})
}

/// Dispatches command `id` from the focused element, in its context, as
/// its key would.
pub(crate) fn run(id: &'static str) -> Step {
    Box::new(move |_, window, cx| {
        let context = kiln_platform::keys::context_stack(&window.context_stack(), cx);
        let invocation = Invocation::of(id).expect("a command ID").context(context);
        window.dispatch_action(Box::new(RunCommand(invocation)), cx);
    })
}

/// Selects the `nth` (from 0) `text` in the active editor, or puts the
/// caret at its start if `text` is a call path (has `::`).
pub(crate) fn select(text: &'static str, nth: usize) -> Step {
    Box::new(move |workbench, _, cx| {
        let Some(editor) = workbench.active_editor(cx).cloned() else {
            return fail("no active editor to select in".to_owned());
        };
        editor.update(cx, |editor, cx| {
            let model = editor.model_mut();
            let found = model.text().to_string();
            let Some((start, _)) = found.match_indices(text).nth(nth) else {
                return fail(format!("no {text:?} to select"));
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
