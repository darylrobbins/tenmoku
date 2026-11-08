// SPDX-License-Identifier: Apache-2.0

//! The UI string catalogue: Mozilla Fluent messages, embedded by each crate
//! that owns them and looked up in the active locale.
//!
//! - Each crate keeps its messages in `l10n/<bcp47>/<name>.ftl` and lists
//!   them once with [`resources!`], which embeds them with `include_str!` and
//!   records them with `inventory`. There is no build script.
//! - The process has one [`Catalogue`], built lazily on the first lookup from
//!   every crate's resources linked into the binary. It holds one Fluent
//!   bundle per locale of the negotiated chain, each with only that locale's
//!   resources, and always ends in `en-US`.
//! - A lookup walks the chain message by message, so a message missing from a
//!   translation comes from `en-US`. A key missing from `en-US` too shows the
//!   key itself and logs once; a lookup never panics.
//! - [`tr!`] resolves a message now, to a [`SharedString`]; [`text!`] keeps a
//!   key and its arguments as a [`Text`] to resolve when it is shown;
//!   [`verbatim!`] marks a literal that is deliberately not translated.
//! - Arguments are named. String arguments are wrapped in bidi isolates only
//!   where they are written into the text, and only when the locale is
//!   right-to-left or the value holds right-to-left characters, so selectors
//!   see raw values and en-US with ASCII arguments is byte-equal to the
//!   message. [`strip_isolates`] removes them from text that leaves the UI.
//! - With the `lint` feature, [`lint`] holds the checks a crate runs from its
//!   tests: no hard-coded user-facing literal, and keys and catalogue match.
//!
//! Must not depend on GPUI: [`SharedString`] is GPUI's own type from its
//! GPUI-free crate, so headless crates can use the catalogue too.

mod args;
mod catalogue;
mod isolate;
mod resources;
mod text;

#[cfg(feature = "lint")]
pub mod lint;

pub use args::{Arg, Args, IntoArg};
pub use catalogue::{
    Catalogue, Formatted, attribute, format, generation, locales, message, negotiate, set_locales,
    shipped_locales, tr_or, try_format, warm,
};
pub use gpui_shared_string::SharedString;
pub use isolate::{FSI, PDI, strip_isolates};
pub use resources::{Registered, ResourceFile, Resources, public_keys};
pub use text::Text;
pub use unic_langid::{CharacterDirection, LanguageIdentifier};

/// The locale every chain ends in, and the one every key must exist in.
pub const FALLBACK_LOCALE: &str = "en-US";

#[doc(hidden)]
pub mod __private {
    pub use inventory;
}

/// A message now, in the active locale, as a [`SharedString`].
///
/// ```
/// # use kiln_l10n::tr;
/// let _ = tr!("app-git-pull");
/// let _ = tr!("app-git-switch-failed", name = "main");
/// let _ = tr!("app-git-changed-files", count = 3);
/// ```
///
/// A bare key is cached until the locale changes. Arguments are named, and
/// take anything [`IntoArg`]: strings, and numbers for plural selectors.
#[macro_export]
macro_rules! tr {
    ($key:expr $(,)?) => {
        $crate::message($key)
    };
    ($key:expr, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::format($key, &$crate::Args::new()$(.with(stringify!($name), $value))+)
    };
}

/// A message to resolve later, when it is shown: a [`Text`] holding the key
/// and its arguments. Takes the same key and arguments as [`tr!`].
///
/// ```
/// # use kiln_l10n::text;
/// let label = text!("app-nav-definitions");
/// let _shown = label.resolve();
/// ```
#[macro_export]
macro_rules! text {
    ($key:expr $(,)?) => {
        $crate::Text::keyed($key, $crate::Args::new())
    };
    ($key:expr, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::Text::keyed($key, $crate::Args::new()$(.with(stringify!($name), $value))+)
    };
}

/// Text that is deliberately not translated, such as an id, a file name or a
/// proper noun: expands to the literal itself. The literal lint doesn't
/// report it, so it is the in-code allow marker.
///
/// ```
/// # use kiln_l10n::verbatim;
/// assert_eq!(verbatim!("Cargo.toml"), "Cargo.toml");
/// ```
#[macro_export]
macro_rules! verbatim {
    ($text:literal) => {
        $text
    };
}

/// Embeds a crate's catalogue files and registers them with the process
/// catalogue. Call it once per crate, in `src/l10n.rs`:
///
/// ```ignore
/// kiln_l10n::resources! {
///     "en-US" => ["../l10n/en-US/shell.ftl", "../l10n/en-US/picker.ftl"],
///     "de" => ["../l10n/de/shell.ftl"],
/// }
/// ```
///
/// Paths are relative to the file that calls it, as `include_str!`'s are, so
/// cargo rebuilds the crate when a file changes. It defines
/// `pub static RESOURCES: kiln_l10n::Resources`, which the crate's
/// `lint::keys` test takes.
///
/// Keys a crate builds at run time from a prefix, such as `command-<id>`, are
/// declared first, so the unused-key check counts them as used:
///
/// ```ignore
/// kiln_l10n::resources! {
///     dynamic_prefixes = ["command-"];
///     "en-US" => ["../l10n/en-US/commands.ftl"],
/// }
/// ```
#[macro_export]
macro_rules! resources {
    (
        dynamic_prefixes = [$($prefix:literal),* $(,)?];
        $($locale:literal => [$($path:literal),* $(,)?]),+ $(,)?
    ) => {
        $crate::__resources!([$($prefix),*] $($locale => [$($path),*]),+);
    };
    ($($locale:literal => [$($path:literal),* $(,)?]),+ $(,)?) => {
        $crate::__resources!([] $($locale => [$($path),*]),+);
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __resources {
    ([$($prefix:literal),*] $($locale:literal => [$($path:literal),*]),+) => {
        /// This crate's catalogue files, as `kiln_l10n::resources!` embedded them.
        #[allow(unreachable_pub)]
        pub static RESOURCES: $crate::Resources = $crate::Resources {
            crate_name: env!("CARGO_PKG_NAME"),
            manifest_dir: env!("CARGO_MANIFEST_DIR"),
            declared_in: file!(),
            files: &[$($($crate::ResourceFile {
                locale: $locale,
                path: $path,
                source: include_str!($path),
            },)*)+],
            dynamic_prefixes: &[$($prefix),*],
        };

        $crate::__private::inventory::submit! {
            $crate::Registered(&RESOURCES)
        }
    };
}
