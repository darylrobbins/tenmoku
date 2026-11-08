// SPDX-License-Identifier: Apache-2.0

//! Font fallback: the first installed of a list, by family, for a font the
//! app prefers but can't count on.

use gpui::{App, SharedString, font};
use kiln_core::Os;

/// The monospace families worth trying, in order: Spline Sans Mono, the
/// design's code family, which an app that bundles it registers; then
/// JetBrains Mono if installed; macOS ships SF Mono and Menlo, and common
/// Linux distributions the others.
pub const MONO_FAMILIES: [&str; 8] = [
    "Spline Sans Mono",
    "JetBrains Mono",
    "SF Mono",
    "Menlo",
    "DejaVu Sans Mono",
    "Ubuntu Mono",
    "Liberation Mono",
    "Noto Sans Mono",
];

/// The first of `candidates` that's installed. Resolving a family loads
/// just that family, so this is quick; listing every installed font isn't.
pub fn first_installed<'a>(candidates: &[&'a str], cx: &App) -> Option<&'a str> {
    let text_system = cx.text_system();
    // A missing family resolves to a fallback font under another name.
    candidates.iter().copied().find(|family| {
        let id = text_system.resolve_font(&font(*family));
        text_system
            .get_font_for_id(id)
            .is_some_and(|resolved| resolved.family == *family)
    })
}

/// The monospace family to use when none of [`MONO_FAMILIES`] is found:
/// one that's surely on `os`. Unknown families fall back to a proportional
/// font on Linux, so its last resort is one most distributions have.
pub const fn mono_last_resort(os: Os) -> &'static str {
    match os {
        Os::MacOs => "Menlo",
        Os::Windows => "Consolas",
        Os::Linux => "DejaVu Sans Mono",
    }
}

/// A monospace family that's installed here: the first of
/// [`MONO_FAMILIES`], or this platform's [`mono_last_resort`].
pub fn mono_family(cx: &App) -> SharedString {
    first_installed(&MONO_FAMILIES, cx)
        .unwrap_or(mono_last_resort(Os::current()))
        .into()
}
