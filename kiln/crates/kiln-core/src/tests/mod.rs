// SPDX-License-Identifier: Apache-2.0

mod keymap;
mod layout;
mod paths;
mod settings;
mod text_edit;
mod theme;
mod watch;
// Its fixture is made of symlinks.
#[cfg(unix)]
mod watch_late;
pub(crate) mod watch_reconcile;
mod watch_tree;
// Its races change a file's mode and follow a symlink.
#[cfg(unix)]
mod write;
