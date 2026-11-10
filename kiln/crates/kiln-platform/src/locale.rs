// SPDX-License-Identifier: Apache-2.0

//! The language of the app's own text: the user's preferred languages, as
//! the system has them ([`preferred`]), and the locale chain the string
//! catalogue (`kiln-l10n`) looks messages up in, as the [`ActiveLocale`]
//! global.
//!
//! - [`warm`] builds the catalogue on a thread of its own, for the system's
//!   languages, so the first frame's lookups don't parse it. Call it at the
//!   top of `main`.
//! - [`set_locale`] switches to the languages the app asks for, such as a
//!   setting's choice before the system's ([`requested`]): it rebuilds the
//!   bundles if the chain they negotiate changed, sets [`ActiveLocale`],
//!   rebuilds the native menus and redraws every window, as
//!   [`keys::set_keymap`](crate::keys::set_keymap) does for a keymap.
//!
//! Views that compute text outside `render` follow the global with
//! `cx.observe_global::<ActiveLocale>(..)`.
//!
//! On macOS the preferred languages are `CFLocaleCopyPreferredLanguages`'s,
//! which honour a language picked for the app alone in System Settings; on
//! Linux they come from `LANGUAGE`, then `LC_ALL`, `LC_MESSAGES` and `LANG`,
//! in gettext's order. Both are `sys-locale`'s.

use std::sync::{Mutex, PoisonError};

use gpui::{App, Global};
use kiln_l10n::LanguageIdentifier;

/// The user's preferred languages, best first, as the system has them.
/// Tags that aren't BCP 47 language tags, and the POSIX `C` locale, are
/// left out, so it may be empty.
pub fn preferred() -> Vec<LanguageIdentifier> {
    with_pseudo_gate(&parse_tags(sys_locale::get_locales()))
}

/// The environment variable that selects the pseudo-locale `en-XA`, which
/// finds uncatalogued, clipped and concatenated text. Nothing else does.
pub const PSEUDO_ENV: &str = "TENMOKU_PSEUDO_LOCALE";

fn pseudo() -> LanguageIdentifier {
    kiln_l10n::PSEUDO_LOCALE
        .parse()
        .expect("en-XA is a valid language tag")
}

/// `requested` after the pseudo-locale's gate: `en-XA` is dropped wherever
/// it came from, a setting or a system language, and put first if
/// `TENMOKU_PSEUDO_LOCALE=1`, whatever the app's own choice came before it.
/// It is the only way in, applied where the languages are negotiated and
/// switched to.
fn with_pseudo_gate(requested: &[LanguageIdentifier]) -> Vec<LanguageIdentifier> {
    gate_pseudo(
        requested,
        pseudo_selected(std::env::var(PSEUDO_ENV).ok().as_deref()),
    )
}

/// [`with_pseudo_gate`] with the variable's state given: `enabled` is
/// `TENMOKU_PSEUDO_LOCALE=1`.
pub fn gate_pseudo(requested: &[LanguageIdentifier], enabled: bool) -> Vec<LanguageIdentifier> {
    let pseudo = pseudo();
    let mut languages: Vec<LanguageIdentifier> = Vec::new();
    if enabled {
        languages.push(pseudo.clone());
    }
    languages.extend(requested.iter().filter(|l| **l != pseudo).cloned());
    languages
}

/// Whether the variable's `value` selects the pseudo-locale: only `1`.
fn pseudo_selected(value: Option<&str>) -> bool {
    value == Some("1")
}

/// `tags` as language identifiers, in order, without duplicates, leaving
/// out what doesn't parse and the POSIX locales `C` and `POSIX`, which name
/// no language.
pub fn parse_tags<S: AsRef<str>>(tags: impl IntoIterator<Item = S>) -> Vec<LanguageIdentifier> {
    let mut languages: Vec<LanguageIdentifier> = Vec::new();
    for tag in tags {
        let tag = tag.as_ref().trim();
        if tag.eq_ignore_ascii_case("C") || tag.eq_ignore_ascii_case("POSIX") {
            continue;
        }
        if let Ok(language) = tag.parse::<LanguageIdentifier>()
            && !languages.contains(&language)
        {
            languages.push(language);
        }
    }
    languages
}

/// The languages to ask the catalogue for: `choice`, the app's own
/// override if it has one, first, then the `system`'s. A choice nothing
/// ships is dropped when they're negotiated, so the system's languages
/// still come before `en-US`.
pub fn requested(
    choice: Option<&LanguageIdentifier>,
    system: &[LanguageIdentifier],
) -> Vec<LanguageIdentifier> {
    let mut languages: Vec<LanguageIdentifier> = choice.cloned().into_iter().collect();
    for language in system {
        if !languages.contains(language) {
            languages.push(language.clone());
        }
    }
    languages
}

/// The chain `requested` negotiates against the locales the linked crates
/// ship, ending in `en-US`, without building any bundle.
pub fn negotiate(requested: &[LanguageIdentifier]) -> Vec<LanguageIdentifier> {
    let requested = &with_pseudo_gate(requested);
    let mut available = kiln_l10n::shipped_locales();
    // Built on request, so the chain has it as the catalogue's will.
    if requested.contains(&pseudo()) {
        available.push(pseudo());
    }
    kiln_l10n::negotiate(requested, &available)
}

/// The locale chain messages are looked up in, as a GPUI global set by
/// [`set_locale`]: observe it to follow a change of language.
#[derive(Clone, Debug, PartialEq)]
pub struct ActiveLocale {
    chain: Vec<LanguageIdentifier>,
    generation: u64,
}

impl Global for ActiveLocale {}

impl ActiveLocale {
    /// The locales lookups walk, best first, ending in `en-US`.
    pub fn chain(&self) -> &[LanguageIdentifier] {
        &self.chain
    }

    /// The language the text is in, where a translation has it: the chain's
    /// first.
    pub fn language(&self) -> &LanguageIdentifier {
        &self.chain[0]
    }

    /// The catalogue's generation when it was set, which every switch bumps.
    pub fn generation(&self) -> u64 {
        self.generation
    }
}

/// The chain messages are looked up in: [`ActiveLocale`]'s, or before any
/// [`set_locale`], the catalogue's own.
pub fn active(cx: &App) -> Vec<LanguageIdentifier> {
    match cx.try_global::<ActiveLocale>() {
        Some(active) => active.chain.clone(),
        None => kiln_l10n::locales(),
    }
}

/// The chain this module last had the catalogue switch to, and the
/// catalogue's generation just after: a switch from elsewhere moves the
/// generation on, so the chain is no longer known to be the catalogue's.
struct Applied {
    chain: Vec<LanguageIdentifier>,
    generation: u64,
}

static APPLIED: Mutex<Option<Applied>> = Mutex::new(None);

/// Switches the catalogue to `requested`, unless it already has the chain
/// they negotiate. Returns the chain, and whether the catalogue switched.
fn switch(
    requested: &[LanguageIdentifier],
    applied: &mut Option<Applied>,
) -> (Vec<LanguageIdentifier>, bool) {
    let requested = &with_pseudo_gate(requested);
    let chain = negotiate(requested);
    let current = applied
        .as_ref()
        .is_some_and(|a| a.chain == chain && a.generation == kiln_l10n::generation());
    if current {
        return (chain, false);
    }
    let chain = kiln_l10n::set_locales(requested);
    *applied = Some(Applied {
        chain: chain.clone(),
        generation: kiln_l10n::generation(),
    });
    (chain, true)
}

/// Builds the catalogue for the system's preferred languages on a thread
/// of its own, so that neither the first lookup nor [`set_locale`] with the
/// same languages parses it on the main thread. Call it once, at the top of
/// `main`. A [`set_locale`] that comes first wins: the warm-up then does
/// nothing.
pub fn warm() {
    let spawned = std::thread::Builder::new()
        .name(kiln_l10n::verbatim!("l10n warm-up").into())
        .spawn(|| {
            let system = preferred();
            let mut applied = APPLIED.lock().unwrap_or_else(PoisonError::into_inner);
            if applied.is_none() {
                switch(&system, &mut applied);
            }
        });
    if let Err(error) = spawned {
        log::warn!("can't warm the string catalogue up: {error}");
    }
}

/// Switches the app's text to `requested`, best first: the bundles are
/// rebuilt if the chain they negotiate changed, and [`ActiveLocale`] is
/// set. If the text changed, the native menus are rebuilt
/// ([`crate::menus::refresh`]) and every window redraws; the first call,
/// for the languages [`warm`] built, changes nothing drawn. Returns the
/// chain, which ends in `en-US`.
///
/// A requested language nothing ships is dropped, so its chain is the
/// rest's: see [`requested`].
pub fn set_locale(cx: &mut App, requested: &[LanguageIdentifier]) -> Vec<LanguageIdentifier> {
    let (chain, switched) = {
        let mut applied = APPLIED.lock().unwrap_or_else(PoisonError::into_inner);
        switch(requested, &mut applied)
    };
    let active = ActiveLocale {
        chain: chain.clone(),
        generation: kiln_l10n::generation(),
    };
    let before = cx.try_global::<ActiveLocale>();
    if before == Some(&active) {
        return chain;
    }
    let redraw = switched || before.is_some();
    cx.set_global(active);
    if redraw {
        crate::menus::refresh(cx);
        cx.refresh_windows();
    }
    chain
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(tags: &[&str]) -> Vec<LanguageIdentifier> {
        tags.iter().map(|t| t.parse().unwrap()).collect()
    }

    #[test]
    fn pseudo_locale_is_selected_only_by_the_variable_set_to_1() {
        assert!(pseudo_selected(Some("1")));
        for other in [None, Some(""), Some("0"), Some("true"), Some("en-XA")] {
            assert!(!pseudo_selected(other), "{other:?}");
        }
        // Not among what ships, so no setting or system language names it.
        assert!(!kiln_l10n::shipped_locales().contains(&pseudo()));
        // A setting or a system language of en-XA is dropped without the
        // variable, and with it en-XA leads whatever the setting chose.
        let asked = ids(&["en-US", "en-XA", "de"]);
        assert_eq!(gate_pseudo(&asked, false), ids(&["en-US", "de"]));
        assert_eq!(gate_pseudo(&asked, true), ids(&["en-XA", "en-US", "de"]));
        assert_eq!(gate_pseudo(&[], true), ids(&["en-XA"]));
    }
}
