//! A trusted folder's own extensions (extensions record §3):
//! `<root>/.fastedit/extensions`, for the launch root only, as the project
//! settings file is, and only while the user trusts it. kiln loads them
//! beside the user's ([`kiln::extensions`]), rescans them as they change,
//! and drops them when trust is withdrawn; until then they're listed as
//! skipped. They may contribute grammars, language servers and snippets
//! ([`crate::config::WORKSPACE_KINDS`]), which
//! [`crate::config::with_extensions`] takes as it takes the user's: a
//! workspace extension's language server is keyed by its own directory, so
//! it needs consent of its own, whatever the user granted a server of the
//! same name.

use std::path::PathBuf;

use gpui::App;

/// Loads the extensions of the launch root, `root`, while it's trusted:
/// call once kiln is set up and the trust decisions are loaded.
pub(crate) fn follow(root: Option<PathBuf>, cx: &mut App) {
    kiln::extensions::set_workspace_root(cx, root);
}
