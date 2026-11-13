//! The workbench in a headless window: clicks go to the laid-out bounds of
//! the panel's rows and the pane's tabs, keys through the default keymap.
//! Fixtures are `fe_project::fixture` trees in a temporary directory.

/// What a rooted command such as `/usr/bin/node` runs as: itself, or on
/// Windows that path on the current drive (see `fe_lsp::LoginPath::resolve`).
pub(crate) fn rooted(command: impl AsRef<std::path::Path>) -> std::path::PathBuf {
    let command = command.as_ref();
    if cfg!(windows) {
        std::path::absolute(command).unwrap()
    } else {
        command.to_owned()
    }
}

/// Terminal settings for a shell that prints `word` (if any), then runs until
/// [`SHELL_END`] is typed: `sh` running `cat`, or on Windows `cmd`.
pub(crate) fn printing_shell(word: &str) -> fe_config::TerminalSettings {
    let (shell, args) = if cfg!(windows) {
        let mut args = vec!["/D".to_owned()];
        if !word.is_empty() {
            args.extend(["/K".to_owned(), format!("echo {word}")]);
        }
        ("cmd.exe", args)
    } else {
        let script = if word.is_empty() {
            "exec cat".to_owned()
        } else {
            format!("printf {word}; exec cat")
        };
        ("/bin/sh", vec!["-c".to_owned(), script])
    };
    fe_config::TerminalSettings {
        shell: shell.to_owned(),
        shell_args: args,
        ..fe_config::TerminalSettings::default()
    }
}

/// What ends a [`printing_shell`]: an EOF for `cat`, `exit` for `cmd`.
pub(crate) const SHELL_END: &[u8] = if cfg!(windows) { b"exit\r" } else { b"\x04" };

/// A path as a panel shows it, with `/` for the platform's separator, so
/// tests write it alike everywhere (`src/a.rs`, shown `src\a.rs` on
/// Windows).
pub(crate) fn slashed(shown: impl Into<String>) -> String {
    let shown = shown.into();
    if std::path::MAIN_SEPARATOR == '/' {
        shown
    } else {
        shown.replace(std::path::MAIN_SEPARATOR, "/")
    }
}

mod activity_rail;
mod after_frame;
mod bottom_panel_b3;
mod breadcrumb_symbol_icons;
mod caret_keys_hosts;
mod caret_keys_lines;
mod changes_b3;
mod coloured_icon_surfaces;
mod commit_diff_b3;
mod default_fonts;
mod default_keymaps;
mod dock_titles_locale;
mod file_icon_set_surfaces;
pub(crate) mod find_bar;
mod find_bar_b3;
mod find_keys;
mod git_commit_box_size;
mod git_log_b3;
mod git_log_graph_width;
mod git_sync_ui;
mod icon_theme_install;
mod launch;
pub(crate) mod lsp;
mod lsp_aliases;
mod lsp_aliases_language;
mod lsp_aliases_reroute;
mod lsp_completion;
mod lsp_diagnostics;
mod lsp_folds_single_request;
mod lsp_fragments_locale;
mod lsp_hover;
mod lsp_message_request;
mod lsp_nav;
mod lsp_quit;
mod lsp_resolve_retry;
mod lsp_resolve_retry_pending;
mod lsp_symlinks;
mod m3_signoff;
mod menu_bar;
mod minimap_setting;
mod nav;
mod nav_click;
mod nav_partial;
mod nav_pending;
mod open_folder;
mod open_settings;
mod outline_detail;
mod outline_kind_icons;
mod outline_panel;
mod outline_panel_followups;
mod palette_recent;
mod pane_controls_b3;
mod pane_cycle;
mod pane_drag;
mod pane_maximize;
mod pane_toolbar;
mod pane_toolbar_no_path_item;
mod problems_b3;
mod project_drag;
mod project_menu;
mod project_multi_select;
mod project_panel;
pub(crate) mod quick_open;
mod quick_open_races;
mod rail_b3;
mod references_landing;
mod references_panel;
mod restore_lsp_clean;
mod run_widget_b3;
mod search_panel;
mod search_panel_b3;
mod search_panel_cancel;
mod search_panel_followups;
mod settings_b3;
mod settings_caret;
mod sidebar_b3;
mod splits;
mod splits_followups;
mod status_bar;
mod status_bar_b3;
mod status_bar_go_to_line;
mod status_bar_icons;
mod symbol_icon_tints;
mod tabs;
mod terminal_b3;
mod title_bar;
mod title_bar_b3;
mod toast_b3;
mod toast_overflow;
mod toolbar_history;
mod trash_isolation;
mod tree_keys;
mod tree_style;
mod user_config_hermetic;
mod vim_line;
mod vim_ls_layout;
mod vim_pick_layout;
mod vim_recording;
mod window_frame;
mod window_restore;
mod workspace_focus_split;

use std::{cell::Cell, path::Path, rc::Rc};

use fe_gpui::{EditorSettings, EditorTheme};
use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext};

use crate::{Launch, ProjectPanel, Workbench};

/// A workbench for `launch` in a headless window, run until parked, and how
/// many entries its tree had loaded right after `Workbench::new` returned.
pub(crate) fn open(
    cx: &mut TestAppContext,
    launch: Launch,
) -> (Entity<Workbench>, &mut VisualTestContext, usize) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let loaded = Rc::new(Cell::new(usize::MAX));
    let (workbench, cx) = cx.add_window_view({
        let loaded = loaded.clone();
        move |window, cx| {
            let theme = EditorTheme::default_dark(cx);
            let mut workbench =
                Workbench::new(launch, theme, EditorSettings::default(), window, cx);
            // A click on a file opens it to stay, as these tests were
            // written for; `preview_tab_tests` turns previews on.
            workbench.set_preview_tabs(false);
            let panel = workbench.panel().read(cx);
            loaded.set(panel.tree().map_or(0, |tree| tree.stats().loaded_entries));
            workbench
        }
    });
    (workbench, cx, loaded.get())
}

/// A workbench rooted at `root`.
pub(crate) fn open_root<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    let launch = Launch {
        root: Some(root.to_owned()),
        file: None,
    };
    let (workbench, cx, _) = open(cx, launch);
    (workbench, cx)
}

fn panel(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<ProjectPanel> {
    workbench.read_with(cx, |workbench, _| workbench.panel().clone())
}

/// The panel's rows as `(depth, name)`.
fn rows(panel: &Entity<ProjectPanel>, cx: &mut VisualTestContext) -> Vec<(usize, String)> {
    panel.read_with(cx, |panel, _| {
        let tree = panel.tree().expect("the root is open");
        tree.rows(0..tree.row_count())
            .map(|row| (row.depth, row.name.to_string_lossy().into_owned()))
            .collect()
    })
}

/// Clicks the row named `name` where the last frame laid it out.
fn click_row(panel: &Entity<ProjectPanel>, name: &str, cx: &mut VisualTestContext) {
    let ix = rows(panel, cx)
        .iter()
        .position(|(_, row)| row == name)
        .unwrap_or_else(|| panic!("no row {name}"));
    let bounds = panel.read_with(cx, |panel, _| panel.row_bounds(ix).expect("drawn"));
    cx.simulate_click(bounds.center(), Modifiers::none());
}
