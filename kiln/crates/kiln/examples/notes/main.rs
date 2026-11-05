// SPDX-License-Identifier: Apache-2.0

//! Notes: a sidebar, a keyed menu command, a status item, a setting and
//! persistence, in about a hundred lines and no TOML.

mod note_view;
mod notes;
mod sidebar;

fn main() {
    kiln::app("dev.kiln.Notes", "Notes").run();
}
