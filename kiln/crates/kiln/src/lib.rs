// SPDX-License-Identifier: LicenseRef-Proprietary

//! Kiln: an app framework on GPUI that hides the differences between
//! platforms and provides high-level widgets, in GPUI's own idioms. This
//! crate re-exports the others; see `docs/decisions/app-framework.md`.

pub use gpui;
pub use kiln_command as command;
pub use kiln_core as core;
pub use kiln_platform as platform;
pub use kiln_shell as shell;
