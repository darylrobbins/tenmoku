// SPDX-License-Identifier: Apache-2.0

//! The process catalogue: one Fluent bundle per locale of the negotiated
//! chain, each holding only that locale's resources, and the lookups.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, PoisonError, RwLock};

use fluent_bundle::resolver::Scope;
use fluent_bundle::{FluentArgs, FluentError, FluentResource, concurrent::FluentBundle};
use fluent_langneg::{NegotiationStrategy, negotiate_languages};
use fluent_syntax::ast;
use gpui_shared_string::SharedString;
use unic_langid::{CharacterDirection, LanguageIdentifier};

use crate::isolate::{format_ltr, format_rtl};
use crate::pseudo::{bracket, pseudo_source};
use crate::{Args, FALLBACK_LOCALE, PSEUDO_LOCALE, Resources};

pub(crate) type Bundle = FluentBundle<Arc<FluentResource>>;

fn fallback_locale() -> LanguageIdentifier {
    FALLBACK_LOCALE
        .parse()
        .expect("en-US is a valid language tag")
}

/// The chain to look messages up in: the `requested` locales that
/// `available` ships, best match first, ending in `en-US`.
///
/// Uses Fluent's filtering negotiation, so `de-AT, fr` against `en-US, de`
/// gives `de, en-US`, and a requested locale nothing ships is dropped.
pub fn negotiate(
    requested: &[LanguageIdentifier],
    available: &[LanguageIdentifier],
) -> Vec<LanguageIdentifier> {
    let fallback = fallback_locale();
    let chosen = negotiate_languages(
        requested,
        available,
        Some(&fallback),
        NegotiationStrategy::Filtering,
    );
    let mut chain: Vec<LanguageIdentifier> = Vec::new();
    for locale in chosen {
        if chain.contains(locale) {
            continue;
        }
        chain.push(locale.clone());
        // en-US is complete, so nothing after it is ever reached.
        if *locale == fallback {
            break;
        }
    }
    if !chain.contains(&fallback) {
        chain.push(fallback);
    }
    chain
}

fn is_pseudo(locale: &LanguageIdentifier) -> bool {
    *locale == PSEUDO_LOCALE
}

/// A message formatted in one locale of the chain.
#[derive(Clone, Debug, PartialEq)]
pub struct Formatted {
    pub text: SharedString,
    /// The locale whose bundle had the message.
    pub locale: LanguageIdentifier,
    /// Fluent's errors while formatting it, such as a missing argument or an
    /// unknown term. The text still has a placeholder for each.
    pub errors: Vec<String>,
}

/// One bundle per locale of a negotiated chain.
pub struct Catalogue {
    chain: Vec<LanguageIdentifier>,
    shipped: Vec<LanguageIdentifier>,
    bundles: Vec<Bundle>,
    errors: Vec<String>,
    /// Messages that take no arguments, formatted once.
    cache: RwLock<HashMap<Box<str>, SharedString>>,
}

impl Catalogue {
    /// The catalogue of `resources` for the `requested` locales.
    pub fn new(resources: &[&Resources], requested: &[LanguageIdentifier]) -> Self {
        let sources = resources.iter().flat_map(|resources| {
            resources.files.iter().map(move |file| {
                (
                    file.locale.to_string(),
                    format!("{}: {}", resources.crate_name, file.path),
                    file.source.to_string(),
                )
            })
        });
        Self::from_sources(sources, requested)
    }

    /// The catalogue of `(locale, name, text)` sources for the `requested`
    /// locales. `name` only labels errors.
    pub fn from_sources(
        sources: impl IntoIterator<Item = (String, String, String)>,
        requested: &[LanguageIdentifier],
    ) -> Self {
        let mut errors = Vec::new();
        let mut by_locale: Vec<(LanguageIdentifier, Vec<(String, String)>)> = Vec::new();
        for (locale, name, text) in sources {
            let Ok(locale) = locale.parse::<LanguageIdentifier>() else {
                errors.push(format!("{name}: `{locale}` is not a BCP 47 language tag"));
                continue;
            };
            match by_locale.iter_mut().find(|(l, _)| *l == locale) {
                Some((_, files)) => files.push((name, text)),
                None => by_locale.push((locale, vec![(name, text)])),
            }
        }
        let shipped: Vec<LanguageIdentifier> = by_locale.iter().map(|(l, _)| l.clone()).collect();
        // The pseudo-locale is built from en-US only when asked for, and is
        // never one of the `shipped` locales.
        let mut available = shipped.clone();
        if requested.iter().any(is_pseudo)
            && let Some((_, files)) = by_locale.iter().find(|(l, _)| *l == FALLBACK_LOCALE)
        {
            let files = files
                .iter()
                .map(|(name, text)| (name.clone(), pseudo_source(text)))
                .collect();
            let pseudo: LanguageIdentifier = PSEUDO_LOCALE.parse().expect("en-XA is a valid tag");
            available.push(pseudo.clone());
            by_locale.push((pseudo, files));
        }
        let chain = negotiate(requested, &available);
        let bundles = chain
            .iter()
            .map(|locale| {
                let files = by_locale
                    .iter()
                    .find(|(l, _)| l == locale)
                    .map(|(_, files)| files.as_slice())
                    .unwrap_or_default();
                bundle(locale, files, &mut errors)
            })
            .collect();
        for error in &errors {
            log_once("catalogue", error, || {
                log::error!("l10n catalogue: {error}")
            });
        }
        Self {
            chain,
            shipped,
            bundles,
            errors,
            cache: RwLock::default(),
        }
    }

    /// The locales lookups walk, ending in `en-US`.
    pub fn chain(&self) -> &[LanguageIdentifier] {
        &self.chain
    }

    /// Every locale the resources ship, chosen or not.
    pub fn shipped(&self) -> &[LanguageIdentifier] {
        &self.shipped
    }

    /// Problems building the bundles: parse errors and duplicate keys. The
    /// first definition of a duplicate wins.
    pub fn errors(&self) -> &[String] {
        &self.errors
    }

    /// Whether some locale of the chain has `key`.
    pub fn has_message(&self, key: &str) -> bool {
        self.bundles.iter().any(|b| b.has_message(key))
    }

    fn lookup(
        &self,
        key: &str,
        attribute: Option<&str>,
        args: Option<&FluentArgs>,
    ) -> Option<Formatted> {
        for (locale, bundle) in self.chain.iter().zip(&self.bundles) {
            let Some(message) = bundle.get_message(key) else {
                continue;
            };
            let pattern = match attribute {
                None => message.value(),
                Some(name) => message.get_attribute(name).map(|a| a.value()),
            };
            let Some(pattern) = pattern else {
                continue;
            };
            let mut errors = Vec::new();
            let mut text = write_pattern(bundle, pattern, args, &mut errors);
            if is_pseudo(locale) {
                text = bracket(&text);
            }
            return Some(Formatted {
                text: SharedString::from(text),
                locale: locale.clone(),
                errors: errors.iter().map(ToString::to_string).collect(),
            });
        }
        None
    }

    /// `key` with `args`, or `None` when no locale has it. Never logs.
    pub fn try_format(&self, key: &str, args: &Args) -> Option<Formatted> {
        let fluent = (!args.is_empty()).then(|| args.to_fluent());
        self.lookup(key, None, fluent.as_ref())
    }

    /// `key`'s attribute `name` with `args`, with Fluent's errors, or `None`
    /// when no locale has it. Never logs.
    pub fn try_attribute(&self, key: &str, name: &str, args: &Args) -> Option<Formatted> {
        let fluent = (!args.is_empty()).then(|| args.to_fluent());
        self.lookup(key, Some(name), fluent.as_ref())
    }

    /// `key`'s attribute `name`, such as a command's `.description`, or
    /// `None` when no locale has it. Never logs a miss.
    pub fn attribute(&self, key: &str, name: &str, args: &Args) -> Option<SharedString> {
        let fluent = (!args.is_empty()).then(|| args.to_fluent());
        let formatted = self.lookup(key, Some(name), fluent.as_ref())?;
        log_format_errors(key, &formatted);
        Some(formatted.text)
    }

    /// `key`, which takes no arguments. Cached until the locale changes.
    pub fn message(&self, key: &str) -> SharedString {
        let cached = self
            .cache
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(key)
            .cloned();
        if let Some(text) = cached {
            return text;
        }
        let text = self.format(key, &Args::new());
        self.cache
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key.into(), text.clone());
        text
    }

    /// `key` with `args`. A key no locale has shows as itself, and is logged
    /// once.
    pub fn format(&self, key: &str, args: &Args) -> SharedString {
        match self.try_format(key, args) {
            Some(formatted) => {
                log_format_errors(key, &formatted);
                formatted.text
            }
            None => {
                log_once("missing", key, || {
                    log::warn!("l10n: no message `{key}` in any locale")
                });
                SharedString::from(key)
            }
        }
    }

    /// `key`, or `fallback` when no locale has it, silently: for text that may
    /// have no key by design, such as a plugin's command title.
    pub fn tr_or(&self, key: &str, fallback: &str) -> SharedString {
        match self.try_format(key, &Args::new()) {
            Some(formatted) => {
                log_format_errors(key, &formatted);
                formatted.text
            }
            None => SharedString::from(fallback),
        }
    }
}

/// Formats `pattern` as `FluentBundle::format_pattern` does, but without
/// passing the whole result through the bundle's formatter once more:
/// `format_pattern` ends in `FluentValue::into_string`, which would isolate
/// the entire message in a right-to-left locale. Only placeables reach the
/// formatter here, which is where isolation belongs.
pub(crate) fn write_pattern<'b>(
    bundle: &'b Bundle,
    pattern: &'b ast::Pattern<&'b str>,
    args: Option<&FluentArgs>,
    errors: &mut Vec<FluentError>,
) -> String {
    let mut scope = Scope::new(bundle, args, Some(errors));
    let mut out = String::new();
    for element in &pattern.elements {
        if scope.dirty {
            break;
        }
        match element {
            ast::PatternElement::TextElement { value } => out.push_str(value),
            ast::PatternElement::Placeable { expression } => {
                // Writing to a String can't fail.
                let _ = scope.maybe_track(&mut out, pattern, expression);
            }
        }
    }
    out
}

pub(crate) fn bundle(
    locale: &LanguageIdentifier,
    files: &[(String, String)],
    errors: &mut Vec<String>,
) -> Bundle {
    let mut bundle = Bundle::new_concurrent(vec![locale.clone()]);
    bundle.set_use_isolating(false);
    match locale.character_direction() {
        CharacterDirection::RTL => bundle.set_formatter(Some(format_rtl)),
        _ => bundle.set_formatter(Some(format_ltr)),
    }
    for (name, text) in files {
        let resource = match FluentResource::try_new(text.clone()) {
            Ok(resource) => resource,
            Err((resource, parse_errors)) => {
                for error in parse_errors {
                    errors.push(format!("{locale}: {name}: {error}"));
                }
                resource
            }
        };
        if let Err(add_errors) = bundle.add_resource(Arc::new(resource)) {
            for error in add_errors {
                errors.push(format!("{locale}: {name}: {error}"));
            }
        }
    }
    bundle
}

fn log_format_errors(key: &str, formatted: &Formatted) {
    if formatted.errors.is_empty() {
        return;
    }
    let locale = &formatted.locale;
    log_once("format", key, || {
        log::warn!(
            "l10n: formatting `{key}` in {locale}: {}",
            formatted.errors.join("; ")
        )
    });
}

static LOGGED: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Mutex::default);

/// Runs `log` the first time `kind` and `key` come up in this process.
fn log_once(kind: &str, key: &str, log: impl FnOnce()) {
    let first = LOGGED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(format!("{kind}\u{0}{key}"));
    if first {
        log();
    }
}

static CURRENT: RwLock<Option<Arc<Catalogue>>> = RwLock::new(None);
static REQUESTED: Mutex<Vec<LanguageIdentifier>> = Mutex::new(Vec::new());
static GENERATION: AtomicU64 = AtomicU64::new(0);

fn current() -> Arc<Catalogue> {
    if let Some(catalogue) = CURRENT
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
    {
        return catalogue.clone();
    }
    let mut slot = CURRENT.write().unwrap_or_else(PoisonError::into_inner);
    if let Some(catalogue) = slot.as_ref() {
        return catalogue.clone();
    }
    let requested = REQUESTED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let catalogue = Arc::new(Catalogue::new(&Resources::registered(), &requested));
    *slot = Some(catalogue.clone());
    catalogue
}

/// Builds the process catalogue now, if no lookup has: call it on a thread
/// at startup so the first frame doesn't parse.
pub fn warm() {
    current();
}

/// Switches the process catalogue to `requested`, the user's preferred
/// locales, best first, and returns the chain it negotiated, which ends in
/// `en-US`. A requested locale nothing ships is dropped, and logged once.
/// Bumps [`generation`] and clears the cache of formatted messages.
pub fn set_locales(requested: &[LanguageIdentifier]) -> Vec<LanguageIdentifier> {
    *REQUESTED.lock().unwrap_or_else(PoisonError::into_inner) = requested.to_vec();
    let catalogue = Arc::new(Catalogue::new(&Resources::registered(), requested));
    let chain = catalogue.chain().to_vec();
    for locale in requested {
        if !chain
            .iter()
            .any(|chosen| chosen.matches(locale, true, true))
        {
            let tag = locale.to_string();
            log_once("unshipped", &tag, || {
                log::info!("l10n: no `{tag}` translation ships; using {}", chain[0])
            });
        }
    }
    *CURRENT.write().unwrap_or_else(PoisonError::into_inner) = Some(catalogue);
    GENERATION.fetch_add(1, Ordering::SeqCst);
    chain
}

/// The process catalogue's chain, ending in `en-US`.
pub fn locales() -> Vec<LanguageIdentifier> {
    current().chain().to_vec()
}

/// Every locale some linked crate ships.
pub fn shipped_locales() -> Vec<LanguageIdentifier> {
    let mut tags = BTreeSet::new();
    for resources in Resources::registered() {
        tags.extend(resources.locales());
    }
    tags.into_iter()
        .filter_map(|tag| tag.parse().ok())
        .collect()
}

/// Bumped by every [`set_locales`], so text computed outside a render can
/// tell it is stale.
pub fn generation() -> u64 {
    GENERATION.load(Ordering::SeqCst)
}

/// `key`, which takes no arguments, in the active locale: what a bare
/// [`tr!`](crate::tr) calls.
pub fn message(key: &str) -> SharedString {
    current().message(key)
}

/// `key` with `args` in the active locale: what [`tr!`](crate::tr) with
/// arguments calls.
pub fn format(key: &str, args: &Args) -> SharedString {
    current().format(key, args)
}

/// `key` in the active locale, or `fallback` when no locale has it, without
/// logging the miss.
pub fn tr_or(key: &str, fallback: &str) -> SharedString {
    current().tr_or(key, fallback)
}

/// `key` with `args` in the active locale, or `None`, without logging.
pub fn try_format(key: &str, args: &Args) -> Option<Formatted> {
    current().try_format(key, args)
}

/// `key`'s attribute `name` in the active locale, or `None`, without logging
/// the miss.
pub fn attribute(key: &str, name: &str, args: &Args) -> Option<SharedString> {
    current().attribute(key, name, args)
}
