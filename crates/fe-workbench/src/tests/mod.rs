//! The workbench in a headless window: clicks go to the laid-out bounds of
//! the panel's rows and the pane's tabs, keys through the default keymap.
//! Fixtures are `fe_project::fixture` trees in a temporary directory.

mod activity_rail;
mod default_keymaps;
mod find_bar;
mod find_keys;
mod launch;
pub(crate) mod lsp;
mod lsp_aliases;
mod lsp_aliases_language;
mod lsp_aliases_reroute;
mod lsp_completion;
mod lsp_diagnostics;
mod lsp_hover;
mod lsp_nav;
mod lsp_quit;
mod lsp_resolve_retry;
mod lsp_resolve_retry_pending;
mod lsp_symlinks;
mod m3_signoff;
mod menu_bar;
mod nav;
mod nav_click;
mod nav_partial;
mod nav_pending;
mod open_folder;
mod open_settings;
mod outline_panel;
mod outline_panel_followups;
mod pane_cycle;
mod pane_drag;
mod pane_toolbar;
mod project_panel;
pub(crate) mod quick_open;
mod quick_open_races;
mod references_landing;
mod references_panel;
mod search_panel;
mod search_panel_cancel;
mod search_panel_followups;
mod splits;
mod splits_followups;
mod status_bar;
mod tabs;
mod title_bar;
mod toolbar_history;
mod tree_keys;
mod tree_style;
mod vim_line;
mod vim_recording;
mod window_frame;

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
            let workbench = Workbench::new(launch, theme, EditorSettings::default(), window, cx);
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
