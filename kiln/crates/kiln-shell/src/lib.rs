// SPDX-License-Identifier: LicenseRef-Proprietary

//! Kiln's app shell: the root view that draws the window frame, the
//! in-window menu bar and the modal layer, and the shell that hosts panes,
//! docks, the palette and status items.

use gpui::App;
use kiln_platform::Capabilities;

/// Sets up what the shell draws for the platform: where it has no native
/// alerts, `window.prompt` shows kiln-ui's in-window prompt, themed and
/// wrapped, instead of GPUI's plain fallback.
pub fn init(cx: &mut App) {
    if !Capabilities::current().native_prompts {
        cx.set_prompt_builder(kiln_ui::prompt_renderer);
    }
}
