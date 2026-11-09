//! The extension registry: the index of what the app can download, and its
//! generator. Card T325 of the grammar registry
//! (`docs/decisions/xb65tqark9-grammar-registry-index.md`).
//!
//! [`grammars`] holds the first-party grammars' part: the index's types,
//! which the app and the signer parse with, and [`grammars::generate`],
//! which the `grammar-registry` binary runs to build each language's
//! artifact and the index from `extensions/grammars/`. Plugins' entries
//! will sit in a module beside it.
//!
//! No network, no GPUI and no fe-syntax: CI builds the binary in seconds.

pub mod grammars;
