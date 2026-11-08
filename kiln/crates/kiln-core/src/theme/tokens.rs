// SPDX-License-Identifier: Apache-2.0

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
    /// What things sit on. The design system's names cross these:
    /// `background` is its editor surface, `panel` its tool window,
    /// `raised` its overlay, and `overlay` here is no surface of its own
    /// but the scrim behind a modal.
    surface: Surface {
        /// The window's and the main content's background: the editor.
        background,
        /// Side panels, bars and inactive tabs: a tool window.
        panel,
        /// What floats over content: menus, popovers, palettes.
        raised,
        /// The dimming behind a modal: a scrim, not the design's overlay
        /// surface, which is `raised`.
        overlay,
        /// The window's frame: the title bar, the activity rail, the tab
        /// strip and the status bar.
        chrome,
    }
    text: Text {
        primary,
        /// Secondary text, placeholders and ignored entries.
        muted,
        /// The third step below `primary` and `muted`: hints, shortcuts
        /// and paths beside a name.
        faint,
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
        /// Around an input or a secondary button.
        control,
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
    /// Signals, each with a soft ground: the signal at low alpha, behind a
    /// badge, a banner or a row it marks.
    status: Status {
        error,
        warning,
        info,
        success,
        /// Something changed and not yet saved or committed.
        modified,
        success_soft,
        error_soft,
        warning_soft,
        modified_soft,
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
    /// File and folder icons as the tinted icon set draws them: a file's
    /// by its type, a folder's by its role. Each is `list.icon` unless a
    /// theme sets it, so a theme that sets none draws them all alike.
    file_icon: FileIcon {
        /// Rust sources.
        rust,
        /// The compiled and systems languages: C, C++, C#, Go, Java,
        /// Kotlin, Swift, Haskell, Elixir and the like.
        code,
        /// TOML files, `Cargo.toml` among them.
        toml,
        /// Markdown, READMEs included.
        markdown,
        /// Other configuration: YAML, INI, build files (Make, just, CMake,
        /// Starlark), Nix, Terraform, Git's and Docker's files, `.env`.
        config,
        /// Data: JSON, XML, tables, SQL, Protocol Buffers, diffs and lock
        /// files.
        data,
        /// Shell scripts and the scripting languages: Python, Ruby, Lua,
        /// Perl, PHP, R, Julia, PowerShell, Vim script.
        script,
        /// The web's languages: JavaScript, TypeScript, JSX, HTML, CSS,
        /// Sass, Vue, Svelte, Astro, Twig.
        web,
        /// Images, video and sound.
        image,
        /// A file no type names.
        default,
        /// A source folder: `src`, `lib`, `crates`.
        folder_source,
        /// A tests folder: `tests`, `spec`, `benches`.
        folder_tests,
        /// A documentation folder: `docs`.
        folder_docs,
        /// Any other folder.
        folder,
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
    /// What an agent did or is doing: its edits' marks, its toasts. Never
    /// the accent, so an agent's work reads apart from the user's focus.
    agent: Agent {
        base,
        /// The agent's colour at low alpha, behind what it marks.
        soft,
    }
    /// A file's or a line's version-control state.
    vcs: Vcs {
        added,
        modified,
        deleted,
        untracked,
        ignored,
        /// An unresolved merge conflict.
        conflict,
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
    (
        "text.faint",
        Rule::Mix("text.muted", "surface.background", 0.4),
    ),
    ("text.disabled", Rule::Same("text.muted")),
    ("text.accent", Rule::Same("accent.base")),
    (
        "border.subtle",
        Rule::Mix("border.default", "surface.background", 0.5),
    ),
    ("border.focused", Rule::Same("accent.base")),
    ("border.control", Rule::Same("border.default")),
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
    ("file_icon.rust", Rule::Same("list.icon")),
    ("file_icon.code", Rule::Same("list.icon")),
    ("file_icon.toml", Rule::Same("list.icon")),
    ("file_icon.markdown", Rule::Same("list.icon")),
    ("file_icon.config", Rule::Same("list.icon")),
    ("file_icon.data", Rule::Same("list.icon")),
    ("file_icon.script", Rule::Same("list.icon")),
    ("file_icon.web", Rule::Same("list.icon")),
    ("file_icon.image", Rule::Same("list.icon")),
    ("file_icon.default", Rule::Same("list.icon")),
    ("file_icon.folder_source", Rule::Same("list.icon")),
    ("file_icon.folder_tests", Rule::Same("list.icon")),
    ("file_icon.folder_docs", Rule::Same("list.icon")),
    ("file_icon.folder", Rule::Same("list.icon")),
    ("tab.active", Rule::Same("surface.background")),
    ("tab.inactive", Rule::Same("surface.panel")),
    ("scrollbar.thumb", Rule::Alpha("text.muted", 0x80)),
    ("scrollbar.thumb_hover", Rule::Alpha("text.primary", 0x99)),
    ("surface.chrome", Rule::Same("surface.panel")),
    ("status.success_soft", Rule::Alpha("status.success", SOFT)),
    ("status.error_soft", Rule::Alpha("status.error", SOFT)),
    ("status.warning_soft", Rule::Alpha("status.warning", SOFT)),
    ("status.modified_soft", Rule::Alpha("status.modified", SOFT)),
    ("agent.soft", Rule::Alpha("agent.base", SOFT)),
    ("vcs.added", Rule::Same("status.success")),
    ("vcs.modified", Rule::Same("status.modified")),
    ("vcs.deleted", Rule::Same("status.error")),
    ("vcs.untracked", Rule::Same("status.success")),
    ("vcs.ignored", Rule::Same("text.muted")),
    ("vcs.conflict", Rule::Same("status.warning")),
];

/// A soft ground's alpha: a signal's colour at 15%.
pub const SOFT: u8 = 0x26;

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
