// SPDX-License-Identifier: LicenseRef-Proprietary

//! The semantic colour tokens, in groups, and how a token a theme leaves
//! out is derived from the ones it sets.
//!
//! Tokens are generic over the colour type: [`Color`] here, and GPUI's own
//! colour once a UI crate has resolved them, so the list is written once.

use std::collections::BTreeSet;

use super::Color;

/// Declares the token groups: a struct per group, [`Tokens`] holding them
/// all, and [`TOKEN_KEYS`], each token's `group.name`.
macro_rules! tokens {
    ($(
        $(#[$group_meta:meta])*
        $group:ident: $Group:ident {
            $($(#[$field_meta:meta])* $field:ident,)*
        }
    )*) => {
        $(
            $(#[$group_meta])*
            #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
            pub struct $Group<C = Color> {
                $($(#[$field_meta])* pub $field: C,)*
            }

            impl<C> $Group<C> {
                /// The group with `f` applied to each colour.
                pub fn map<D>(self, f: &mut impl FnMut(C) -> D) -> $Group<D> {
                    $Group { $($field: f(self.$field),)* }
                }
            }
        )*

        /// Every token, by group: `tokens.list.row_selected`.
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
        pub struct Tokens<C = Color> {
            $($(#[$group_meta])* pub $group: $Group<C>,)*
        }

        /// Every token's key, `group.name`, in declaration order.
        pub const TOKEN_KEYS: &[&str] = &[
            $($(concat!(stringify!($group), ".", stringify!($field)),)*)*
        ];

        /// The groups' names, as a theme file's tables.
        pub const TOKEN_GROUPS: &[&str] = &[$(stringify!($group)),*];

        impl<C> Tokens<C> {
            /// The token at `key`, such as `"list.row_selected"`.
            pub fn get(&self, key: &str) -> Option<&C> {
                match key {
                    $($(concat!(stringify!($group), ".", stringify!($field))
                        => Some(&self.$group.$field),)*)*
                    _ => None,
                }
            }

            pub fn get_mut(&mut self, key: &str) -> Option<&mut C> {
                match key {
                    $($(concat!(stringify!($group), ".", stringify!($field))
                        => Some(&mut self.$group.$field),)*)*
                    _ => None,
                }
            }

            /// The tokens with `f` applied to each colour, as when turning
            /// them into a UI toolkit's colours.
            pub fn map<D>(self, mut f: impl FnMut(C) -> D) -> Tokens<D> {
                Tokens { $($group: self.$group.map(&mut f),)* }
            }
        }
    };
}

tokens! {
    /// What things sit on.
    surface: Surface {
        /// The window's and the main content's background.
        background,
        /// Side panels, bars and inactive tabs.
        panel,
        /// What floats over content: menus, popovers, palettes.
        raised,
        /// The dimming behind a modal.
        overlay,
    }
    text: Text {
        primary,
        /// Secondary text, placeholders and ignored entries.
        muted,
        disabled,
        /// Links and accented labels.
        accent,
        /// Text and marks on an accent fill, as on a primary button.
        on_accent,
    }
    border: Border {
        default,
        /// Separators inside a panel.
        subtle,
        /// Around the control that has focus.
        focused,
    }
    accent: Accent {
        base,
        hover,
        /// A tint of the accent, as behind a selection.
        subtle,
    }
    /// Interaction states, as overlays on whatever is underneath.
    state: State {
        hover,
        pressed,
        selected,
        /// The selection in a view that doesn't have focus.
        selected_inactive,
        focus_ring,
        /// Where a drag would drop.
        drop_target,
        /// The mark on something with unsaved changes.
        dirty,
        /// The fill of a disabled control that's otherwise filled with a
        /// colour, such as a primary button: neutral, so its text reads.
        disabled,
    }
    status: Status {
        error,
        warning,
        info,
        success,
    }
    /// Rows in trees and lists.
    list: List {
        row_hover,
        row_selected,
        /// The selected row while its list doesn't have focus.
        row_selected_inactive,
        /// The outline of the selected row while its list has focus.
        row_focus_border,
        /// The vertical lines marking a row's ancestor levels.
        indent_guide,
        /// A row's disclosure chevron.
        chevron,
        /// A leaf row's icon.
        icon,
        /// A folder row's icon.
        folder_icon,
    }
    tab: Tab {
        active,
        active_text,
        inactive,
    }
    /// A scrolling view's scrollbar.
    scrollbar: Scrollbar {
        thumb,
        /// The thumb while it's hovered or dragged.
        thumb_hover,
    }
}

/// How a token follows others when a theme doesn't set it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Rule {
    /// The same colour as another token.
    Same(&'static str),
    /// Another token's colour at this alpha.
    Alpha(&'static str, u8),
    /// The first token's colour moved this far towards the second's.
    Mix(&'static str, &'static str, f32),
}

impl Rule {
    /// The tokens this rule reads.
    pub fn inputs(self) -> impl Iterator<Item = &'static str> {
        let (first, second) = match self {
            Rule::Same(key) | Rule::Alpha(key, _) => (key, None),
            Rule::Mix(from, to, _) => (from, Some(to)),
        };
        std::iter::once(first).chain(second)
    }

    fn apply(self, tokens: &Tokens) -> Color {
        let get = |key| *tokens.get(key).expect("rules name tokens");
        match self {
            Rule::Same(key) => get(key),
            Rule::Alpha(key, alpha) => get(key).with_alpha(alpha),
            Rule::Mix(from, to, amount) => get(from).mix(get(to), amount),
        }
    }
}

/// The derived tokens, each after the tokens it reads. A token not listed
/// is a root: every bundled base sets it.
pub const RULES: &[(&str, Rule)] = &[
    (
        "surface.raised",
        Rule::Mix("surface.background", "text.primary", 0.08),
    ),
    (
        "text.muted",
        Rule::Mix("text.primary", "surface.background", 0.5),
    ),
    ("text.disabled", Rule::Same("text.muted")),
    ("text.accent", Rule::Same("accent.base")),
    (
        "border.subtle",
        Rule::Mix("border.default", "surface.background", 0.5),
    ),
    ("border.focused", Rule::Same("accent.base")),
    ("accent.hover", Rule::Alpha("accent.base", 0xd9)),
    ("accent.subtle", Rule::Alpha("accent.base", 0x33)),
    ("state.selected", Rule::Same("accent.subtle")),
    ("state.focus_ring", Rule::Alpha("accent.base", 0xaa)),
    ("state.drop_target", Rule::Same("accent.base")),
    ("state.dirty", Rule::Same("status.warning")),
    (
        "state.disabled",
        Rule::Mix("surface.panel", "text.muted", 0.25),
    ),
    ("status.info", Rule::Same("accent.base")),
    ("list.row_hover", Rule::Same("state.hover")),
    ("list.row_selected", Rule::Same("state.selected")),
    (
        "list.row_selected_inactive",
        Rule::Same("state.selected_inactive"),
    ),
    ("list.row_focus_border", Rule::Same("state.focus_ring")),
    ("list.chevron", Rule::Mix("text.primary", "text.muted", 0.5)),
    ("list.icon", Rule::Same("text.primary")),
    ("list.folder_icon", Rule::Same("accent.base")),
    ("tab.active", Rule::Same("surface.background")),
    ("tab.inactive", Rule::Same("surface.panel")),
    ("scrollbar.thumb", Rule::Alpha("text.muted", 0x80)),
    ("scrollbar.thumb_hover", Rule::Alpha("text.primary", 0x99)),
];

/// The rule for `key`, if it's derived.
pub fn rule(key: &str) -> Option<Rule> {
    RULES
        .iter()
        .find(|(derived, _)| *derived == key)
        .map(|&(_, rule)| rule)
}

/// The tokens a theme sets, by key, over `base`. A token it leaves out
/// keeps `base`'s colour unless its rule reads a token the theme changed,
/// directly or by derivation, in which case it follows that: a theme that
/// sets only `accent.base` moves the selection and the focus ring with it.
/// Setting a token to `base`'s colour changes nothing, so a theme that
/// spells out the base's colours is the base. With no `base`, every
/// derived token is derived, and a root left out is transparent.
pub fn resolve<'a>(
    set: impl IntoIterator<Item = (&'a str, Color)>,
    base: Option<&Tokens>,
) -> Tokens {
    let mut tokens = base.copied().unwrap_or_default();
    let differs = |key: &str, color: Color| base.is_none_or(|base| base.get(key) != Some(&color));
    let mut explicit = BTreeSet::new();
    let mut changed = BTreeSet::new();
    for (key, color) in set {
        if let Some(slot) = tokens.get_mut(key) {
            *slot = color;
            explicit.insert(key);
            if differs(key, color) {
                changed.insert(key);
            }
        }
    }
    for &(key, rule) in RULES {
        if explicit.contains(key) {
            continue;
        }
        if base.is_none() || rule.inputs().any(|input| changed.contains(input)) {
            let color = rule.apply(&tokens);
            *tokens.get_mut(key).expect("rules name tokens") = color;
            if differs(key, color) {
                changed.insert(key);
            }
        }
    }
    tokens
}
