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
//! or `a` to a text object, and `object_prefix` may instead wait on keys
//! of its own after it (`in(`, [`seek`]). The variants for the M7 breadth cards are
//! already in place, each delegating to its stub module ([`prose`],
//! [`line_motions`], [`delimiters`], [`g_operators`]; [`jumplist`] takes
//! keys only).
//!
//! A feature waits on keys of its own, as `ds{char}` and `cs{old}{new}` do
//! ([`surround`]), with a [`FeaturePrefix`]: a hook returns
//! [`Pending::with_feature_prefix`], naming itself, and its `prefix` hook
//! gets the next key with what it has gathered so far (the keys in the
//! prefix, the operator and count in the [`Pending`]). It waits again on the
//! prefix with a key added ([`FeaturePrefix::then`]) or commits; `<Esc>`, and
//! any key it refuses, cancel. While it waits `vim.pending_kind` is the
//! feature's name. An operator of the feature's that waits for keys after
//! its target (`ys{motion}{char}`) has the reducer's commit hand the target
//! to the feature, which waits on a prefix holding it
//! ([`FeaturePrefix::with_target`]).

pub(crate) mod argument;
pub(crate) mod comment;
pub(crate) mod definition;
pub(crate) mod definition_motions;
pub(crate) mod exchange;
pub(crate) mod filter;
pub(crate) mod line_edits;
pub(crate) mod objects;
pub(crate) mod replace_register;
pub(crate) mod sections;
pub(crate) mod seek;
pub(crate) mod surround;

use fe_command::Registry;

use crate::{
    Pending, Step, Target, TextObject, VimCommands, VimKey, changes, delimiters, folds,
    g_operators, increment, jump, jumplist, line_motions, partial_word, prose, put_keys, quit_keys,
    scroll, unmatched,
};

pub(crate) mod changelist;
pub(crate) mod line_undo;
pub(crate) mod reindent;

/// A feature's answer to the key after `[` or `]` (`true`).
pub(crate) type BracketHook = fn(&Pending, bool, VimKey) -> Option<Step>;

/// A feature's answer to the char after `i` or `a` (`true` for `i`).
pub(crate) type ObjectPrefixHook = fn(&Pending, char, bool) -> Option<Step>;

/// A feature's answer to the key after its own prefix.
pub(crate) type PrefixHook = fn(&Pending, &FeaturePrefix, VimKey) -> Option<Step>;

/// A prefix a feature owns, waiting for its next key: which feature, by
/// name (`vim.pending_kind`), the keys it has gathered since it began, and
/// the target of the operator it waits to finish, if one is
/// ([`FeaturePrefix::with_target`]: `ys{motion}` waiting for its pair).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FeaturePrefix {
    feature: &'static str,
    keys: Vec<char>,
    target: Option<Target>,
}

impl FeaturePrefix {
    /// How many keys a prefix holds: enough for an HTML tag's attributes.
    pub(crate) const MAX_KEYS: usize = 1024;

    /// `feature`'s prefix, with no keys gathered yet.
    pub(crate) const fn new(feature: &'static str) -> Self {
        Self {
            feature,
            keys: Vec::new(),
            target: None,
        }
    }

    /// The same prefix, holding the target its operator acts on.
    pub(crate) fn with_target(self, target: Target) -> Self {
        Self {
            target: Some(target),
            ..self
        }
    }

    /// The name of the feature that owns it.
    pub(crate) fn feature(&self) -> &'static str {
        self.feature
    }

    /// The keys gathered, in the order typed.
    pub(crate) fn keys(&self) -> &[char] {
        &self.keys
    }

    /// The target its operator acts on, if it holds one.
    pub(crate) fn target(&self) -> Option<Target> {
        self.target
    }

    /// The same prefix with `ch` gathered too; `None` if it holds
    /// [`Self::MAX_KEYS`] already.
    pub(crate) fn then(&self, ch: char) -> Option<Self> {
        if self.keys.len() >= Self::MAX_KEYS {
            return None;
        }
        let mut next = self.clone();
        next.keys.push(ch);
        Some(next)
    }

    /// The same prefix without the last key gathered.
    pub(crate) fn back(&self) -> Self {
        let mut next = self.clone();
        next.keys.pop();
        next
    }
}

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
    /// The char after `i` or `a` (`true` for `i`), as for `object`, that
    /// starts a prefix of the feature's own ([`Pending::with_feature_prefix`])
    /// whose `prefix` hook then commits the object with
    /// [`Pending::object`]; `None` if the feature has none.
    pub(crate) object_prefix: Option<ObjectPrefixHook>,
    /// The key after a [`FeaturePrefix`], any feature's: `None` from a
    /// feature that doesn't own it, or for a key it refuses. `None` if the
    /// feature has no prefix of its own.
    pub(crate) prefix: Option<PrefixHook>,
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
        object_prefix: None,
        prefix: None,
        register: |_| {},
    };
}

pub(crate) const FEATURES: &[&Feature] = &[
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
    &sections::FEATURE,
    &definition_motions::FEATURE,
    &partial_word::FEATURE,
    &put_keys::FEATURE,
    &line_edits::FEATURE,
    &reindent::FEATURE,
    &line_undo::FEATURE,
    &changelist::FEATURE,
    &objects::FEATURE,
    &argument::FEATURE,
    &definition::FEATURE,
    &surround::FEATURE,
    &replace_register::FEATURE,
    &comment::FEATURE,
    &exchange::FEATURE,
    &filter::FEATURE,
    &seek::FEATURE,
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

/// The owning feature's answer to the key after its prefix.
pub(crate) fn prefix(pending: &Pending, prefix: &FeaturePrefix, key: VimKey) -> Option<Step> {
    FEATURES
        .iter()
        .filter_map(|feature| feature.prefix)
        .find_map(|hook| hook(pending, prefix, key))
}

/// The first feature's text object for the char after `i` (`inner`) or
/// `a`.
pub(crate) fn object(pending: &Pending, ch: char, inner: bool) -> Option<TextObject> {
    FEATURES
        .iter()
        .find_map(|feature| (feature.object)(pending, ch, inner))
}

/// The first feature's prefix for the char after `i` (`inner`) or `a`.
pub(crate) fn object_prefix(pending: &Pending, ch: char, inner: bool) -> Option<Step> {
    FEATURES
        .iter()
        .filter_map(|feature| feature.object_prefix)
        .find_map(|hook| hook(pending, ch, inner))
}

/// Runs every feature's `register`.
pub(crate) fn register(registry: &mut Registry<VimCommands>) {
    for feature in FEATURES {
        (feature.register)(registry);
    }
}
