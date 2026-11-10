// SPDX-License-Identifier: Apache-2.0

//! What to tell the user when the platform's file dialog fails.

use kiln_core::Os;
use kiln_l10n::tr;

/// The prompt to show when the open panel fails with `error` on `os`, in
/// the active locale: a title, and a detail that on Linux names what to
/// install, as the panel there is xdg-desktop-portal's, which a minimal
/// desktop may lack.
pub fn open_panel_failed(error: &str, os: Os) -> (String, String) {
    let detail = if os == Os::Linux {
        tr!("kiln-platform-open-panel-failed-linux", error = error).to_string()
    } else {
        error.to_owned()
    };
    (
        tr!("kiln-platform-open-panel-unavailable").to_string(),
        detail,
    )
}
