// SPDX-License-Identifier: Apache-2.0

//! The checks that keep the catalogue complete (the `lint` feature; a crate
//! takes it as a dev-dependency only and calls the checks from its tests):
//!
//! ```ignore
//! #[test]
//! fn ui_text_is_in_the_catalogue() {
//!     kiln_l10n::lint::literals(env!("CARGO_MANIFEST_DIR"))
//!         .allow_list("l10n-allow.toml")
//!         .check();
//! }
//!
//! #[test]
//! fn keys_used_here_exist() {
//!     kiln_l10n::lint::keys(env!("CARGO_MANIFEST_DIR"), &my_crate::l10n::RESOURCES).check();
//! }
//! ```
//!
//! - [`literals`]: layer 2, no hard-coded user-facing literal in the crate's
//!   non-test source, with the crate's `l10n-allow.toml`.
//! - [`keys`]: layer 3 per crate, every key used exists and other crates'
//!   keys are public.
//! - [`catalogue`]: layer 3 at a catalogue root, read from disk: unused
//!   keys, references that don't resolve in their own locale, formatting,
//!   duplicates and stale translations.

mod allow;
mod catalogue;
mod keys;
mod literals;
mod source;

pub use catalogue::{CatalogueCheck, CatalogueReport, catalogue};
pub use keys::{Keys, KeysReport, keys};
pub use literals::{Finding, LiteralReport, Literals, Rule, literals, scan_source};
