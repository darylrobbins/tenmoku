//! Where a feature that lives in a module of its own registers its keys and
//! commands, so adding one doesn't edit the reducer's or registry's hub.
//!
//! A feature module defines `pub(crate) const FEATURE: Feature` and is listed
//! in [`FEATURES`]. Its hooks see only keys the reducer doesn't take itself
//! and return `None` for any it doesn't own; the first feature to answer
//! `Some` wins, in [`FEATURES`] order. Its `register` adds its commands to
//! vim's registry after vim's own.

use fe_command::Registry;

use crate::{Pending, Step, VimCommands, VimKey, folds, increment, scroll};

/// One feature's hooks into the reducer and the registry.
pub(crate) struct Feature {
    /// A key `step` would otherwise reject, no prefix waiting (the count
    /// already ended).
    pub(crate) key: fn(&Pending, VimKey) -> Option<Step>,
    /// A key after `g` that `step` doesn't take.
    pub(crate) g: fn(&Pending, VimKey) -> Option<Step>,
    /// The key after `z`; `None` if the feature takes no `z` key.
    pub(crate) z: Option<fn(&Pending, VimKey) -> Option<Step>>,
    /// Adds its commands to vim's registry.
    pub(crate) register: fn(&mut Registry<VimCommands>),
}

impl Feature {
    /// A feature that takes nothing.
    pub(crate) const NONE: Feature = Feature {
        key: |_, _| None,
        g: |_, _| None,
        z: None,
        register: |_| {},
    };
}

pub(crate) const FEATURES: [&Feature; 3] = [&increment::FEATURE, &folds::FEATURE, &scroll::FEATURE];

/// The first feature's answer to a key the reducer would reject.
pub(crate) fn key(pending: &Pending, key: VimKey) -> Option<Step> {
    FEATURES
        .iter()
        .find_map(|feature| (feature.key)(pending, key))
}

/// The first feature's answer to a key after `g` the reducer doesn't take.
pub(crate) fn g(pending: &Pending, key: VimKey) -> Option<Step> {
    FEATURES
        .iter()
        .find_map(|feature| (feature.g)(pending, key))
}

/// Whether some feature takes a key after `z`.
pub(crate) fn has_z() -> bool {
    FEATURES.iter().any(|feature| feature.z.is_some())
}

/// The first feature's answer to the key after `z`.
pub(crate) fn z(pending: &Pending, key: VimKey) -> Option<Step> {
    FEATURES
        .iter()
        .filter_map(|feature| feature.z)
        .find_map(|z| z(pending, key))
}

/// Runs every feature's `register`.
pub(crate) fn register(registry: &mut Registry<VimCommands>) {
    for feature in FEATURES {
        (feature.register)(registry);
    }
}
