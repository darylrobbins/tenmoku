// SPDX-License-Identifier: LicenseRef-Proprietary

//! A late event for the user's chain, after the first batch, must not hide
//! the project's edit.

use std::fs;

use super::watch::project_edit_seen_while_registering;

/// Registering the last directory, `c`, rewrites the user's target with its
/// content unchanged. The user's chain then always gets an event after the
/// first batch, as a late fixture event would give.
#[test]
fn late_user_event_does_not_hide_project_edit() {
    project_edit_seen_while_registering(true, |watched, dir| {
        if watched == dir.join("c") {
            let target = dir.join("a").join("settings.toml");
            fs::write(&target, "").unwrap();
        }
    });
}
