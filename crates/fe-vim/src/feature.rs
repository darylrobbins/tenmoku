//! Where a feature that lives in a module of its own registers its keys and
//! commands, so adding one doesn't edit the reducer's or registry's hub.
//!
//! A feature module defines `pub(crate) const FEATURE: Feature` and is listed
//! in [`FEATURES`]. Its hooks see only keys the reducer doesn't take itself
//! and return `None` for any it doesn't own; the first feature to answer
//! `Some` wins, in [`FEATURES`] order. Its `register` adds its commands to
//! vim's registry after vim's own.
//!
//! A feature adds a motion, text object or operator as a variant of
//! [`Motion`](crate::Motion), [`TextObject`] or [`Operator`](crate::Operator)
//! in `args.rs`, whose dispatch arms (`motion::motion`,
//! `text_object::select`, `operator::operate`, the visual operators, and
//! `Motion::class`, `Motion::is_jump` and `Target::writes_numbered`) call
//! into its module, where the behaviour lives. Its keys reach them through
//! its hooks: `key` or `g` commit a motion with [`Pending::motion`], start
//! an operator with [`Pending::with_operator`] or commit one with
//! [`Pending::operate`] (a doubled one, `gUU`, over
//! [`Motion::Line`](crate::Motion::Line)); `object` maps the char after `i`
//! or `a` to a text object. The variants for the M7 breadth cards are
//! already in place, each delegating to its stub module ([`prose`],
//! [`line_motions`], [`delimiters`], [`g_operators`]; [`jumplist`] takes
//! keys only).

use fe_command::Registry;

use crate::{
    Pending, Step, TextObject, VimCommands, VimKey, changes, delimiters, folds, g_operators,
    increment, jump, jumplist, line_motions, partial_word, prose, put_keys, quit_keys, scroll,
    unmatched,
};

pub(crate) mod changelist;
pub(crate) mod line_undo;
pub(crate) mod reindent;

/// A feature's answer to the key after `[` or `]` (`true`).
pub(crate) type BracketHook = fn(&Pending, bool, VimKey) -> Option<Step>;

/// One feature's hooks into the reducer and the registry.
pub(crate) struct Feature {
    /// A key `step` would otherwise reject, no prefix waiting (the count
    /// already ended).
    pub(crate) key: fn(&Pending, VimKey) -> Option<Step>,
    /// A key after `g` that `step` doesn't take.
    pub(crate) g: fn(&Pending, VimKey) -> Option<Step>,
    /// The key after `z`; `None` if the feature takes no `z` key.
    pub(crate) z: Option<fn(&Pending, VimKey) -> Option<Step>>,
    /// The key after `Z`; `None` if the feature takes no `Z` key.
    pub(crate) big_z: Option<fn(&Pending, VimKey) -> Option<Step>>,
    /// The key after `[`, or after `]` (`forward`), with the count ended;
    /// `None` if the feature takes no such key.
    pub(crate) bracket: Option<BracketHook>,
    /// The char after `i` or `a` (`true` for `i`) in operator-pending or a
    /// visual mode that the reducer maps to no text object.
    pub(crate) object: fn(&Pending, char, bool) -> Option<TextObject>,
    /// Adds its commands to vim's registry.
    pub(crate) register: fn(&mut Registry<VimCommands>),
}

impl Feature {
    /// A feature that takes nothing.
    pub(crate) const NONE: Feature = Feature {
        key: |_, _| None,
        g: |_, _| None,
        z: None,
        big_z: None,
        bracket: None,
        object: |_, _, _| None,
        register: |_| {},
    };
}

pub(crate) const FEATURES: [&Feature; 17] = [
    &increment::FEATURE,
    &folds::FEATURE,
    &scroll::FEATURE,
    &jump::FEATURE,
    &prose::FEATURE,
    &line_motions::FEATURE,
    &delimiters::FEATURE,
    &g_operators::FEATURE,
    &jumplist::FEATURE,
    &quit_keys::FEATURE,
    &changes::FEATURE,
    &unmatched::FEATURE,
    &partial_word::FEATURE,
    &put_keys::FEATURE,
    &reindent::FEATURE,
    &line_undo::FEATURE,
    &changelist::FEATURE,
];

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

/// Whether some feature takes a key after `Z`.
pub(crate) fn has_big_z() -> bool {
    FEATURES.iter().any(|feature| feature.big_z.is_some())
}

/// The first feature's answer to the key after `Z`.
pub(crate) fn big_z(pending: &Pending, key: VimKey) -> Option<Step> {
    FEATURES
        .iter()
        .filter_map(|feature| feature.big_z)
        .find_map(|big_z| big_z(pending, key))
}

/// Whether some feature takes a key after `[` or `]`.
pub(crate) fn has_bracket() -> bool {
    FEATURES.iter().any(|feature| feature.bracket.is_some())
}

/// The first feature's answer to the key after `[` or `]` (`forward`).
pub(crate) fn bracket(pending: &Pending, forward: bool, key: VimKey) -> Option<Step> {
    FEATURES
        .iter()
        .filter_map(|feature| feature.bracket)
        .find_map(|bracket| bracket(pending, forward, key))
}

/// The first feature's text object for the char after `i` (`inner`) or
/// `a`.
pub(crate) fn object(pending: &Pending, ch: char, inner: bool) -> Option<TextObject> {
    FEATURES
        .iter()
        .find_map(|feature| (feature.object)(pending, ch, inner))
}

/// Runs every feature's `register`.
pub(crate) fn register(registry: &mut Registry<VimCommands>) {
    for feature in FEATURES {
        (feature.register)(registry);
    }
}
