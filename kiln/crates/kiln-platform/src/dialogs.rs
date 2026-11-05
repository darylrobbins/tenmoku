// SPDX-License-Identifier: Apache-2.0

//! What to tell the user when the platform's file dialog fails.

use kiln_core::Os;

/// The prompt to show when the open panel fails with `error` on `os`: a
/// title, and a detail that on Linux names what to install, as the panel
/// there is xdg-desktop-portal's, which a minimal desktop may lack.
pub fn open_panel_failed(error: &str, os: Os) -> (&'static str, String) {
    let mut detail = error.to_owned();
    if os == Os::Linux {
        detail.push_str(
            "\n\nThe open panel comes from xdg-desktop-portal: install it with a \
             backend for your desktop, such as xdg-desktop-portal-gtk.",
        );
    }
    ("The open panel isn't available", detail)
}
