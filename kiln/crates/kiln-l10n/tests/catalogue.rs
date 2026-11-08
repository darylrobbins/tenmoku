// SPDX-License-Identifier: Apache-2.0

//! The catalogue: per-message en-US fallback, the missing-key and log-once
//! contract, negotiation, the arg-less cache across locale changes, `Text`
//! and the macros. The process catalogue is global, so every test that
//! switches its locale does so in `the_process_catalogue_follows_set_locales`
//! alone.

use std::sync::{Mutex, PoisonError};

use kiln_l10n::{
    Args, Catalogue, LanguageIdentifier, Resources, SharedString, Text, generation, locales,
    negotiate, public_keys, set_locales, shipped_locales, text, tr, tr_or, verbatim,
};

mod probe {
    kiln_l10n::resources! {
        "en-US" => ["fixtures/l10n/en-US/probe.ftl"],
        "qps-ploc" => ["fixtures/l10n/qps-ploc/probe.ftl"],
    }
}

mod dynamic {
    kiln_l10n::resources! {
        dynamic_prefixes = ["probe-dynamic-"];
        "en-US" => ["fixtures/l10n/en-US/dynamic.ftl"],
    }
}

fn ids(tags: &[&str]) -> Vec<LanguageIdentifier> {
    tags.iter().map(|t| t.parse().unwrap()).collect()
}

/// Records warnings, to count how often a key is logged.
struct Recorder(Mutex<Vec<String>>);

impl log::Log for Recorder {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }
    fn log(&self, record: &log::Record) {
        if record.level() <= log::Level::Warn {
            self.0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(record.args().to_string());
        }
    }
    fn flush(&self) {}
}

static RECORDER: Recorder = Recorder(Mutex::new(Vec::new()));

fn logged_about(key: &str) -> usize {
    RECORDER
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .filter(|line| line.contains(key))
        .count()
}

#[test]
fn the_process_catalogue_follows_set_locales() {
    log::set_logger(&RECORDER).unwrap();
    log::set_max_level(log::LevelFilter::Trace);

    // Lazily built, en-US only until a locale is set.
    assert_eq!(locales(), ids(&["en-US"]));
    assert_eq!(tr!("probe-title"), "Probe Labels");
    assert_eq!(tr!("probe-files", count = 1), "One file");
    assert_eq!(tr!("probe-files", count = 4), "4 files");
    let stored = text!("probe-title");
    let counted = text!("probe-files", count = 2);
    assert_eq!(stored.resolve(), "Probe Labels");

    let before = generation();
    let chain = set_locales(&ids(&["qps-ploc"]));
    assert_eq!(chain, ids(&["qps-ploc", "en-US"]));
    assert_eq!(locales(), chain);
    assert!(
        generation() > before,
        "a locale change bumps the generation"
    );

    // The cached bare message follows the switch, as do stored texts.
    assert_eq!(tr!("probe-title"), "⟦Pröbe Läbels⟧");
    assert_eq!(stored.resolve(), "⟦Pröbe Läbels⟧");
    assert_eq!(counted.resolve(), "⟦2 fílés⟧");
    assert_eq!(stored.to_string(), "⟦Pröbe Läbels⟧");
    // A message the translation lacks comes from en-US, per message.
    assert_eq!(tr!("probe-only-english"), "Only in English");
    assert_eq!(
        kiln_l10n::attribute("probe-command", "description", &Args::new()).as_deref(),
        Some("Runs the probe")
    );

    // A requested locale nothing ships is dropped, leaving en-US.
    assert_eq!(
        set_locales(&ids(&["fr", "qps-ploc"])),
        ids(&["qps-ploc", "en-US"])
    );
    assert_eq!(set_locales(&ids(&["fr"])), ids(&["en-US"]));
    assert_eq!(tr!("probe-title"), "Probe Labels");

    // A key no locale has shows as itself, logged once however often asked.
    assert_eq!(tr!("probe-missing-key"), "probe-missing-key");
    assert_eq!(tr!("probe-missing-key"), "probe-missing-key");
    assert_eq!(
        tr!("probe-missing-key-with-args", name = "x"),
        "probe-missing-key-with-args"
    );
    assert_eq!(logged_about("probe-missing-key`"), 1);
    // A formatting error shows the placeable and is logged once too.
    assert_eq!(tr!("probe-files"), "{$count} files");
    assert_eq!(tr!("probe-files", other = 1), "{$count} files");
    assert_eq!(logged_about("probe-files"), 1);
    // tr_or falls back silently: plugin commands have no key by design.
    assert_eq!(
        tr_or("probe-plugin-command", "Plugin Command"),
        "Plugin Command"
    );
    assert_eq!(tr_or("probe-title", "unused"), "Probe Labels");
    assert_eq!(logged_about("probe-plugin-command"), 0);
    assert_eq!(
        kiln_l10n::attribute("probe-title", "description", &Args::new()),
        None
    );
}

#[test]
fn shipped_locales_are_every_linked_crates() {
    let shipped = shipped_locales();
    assert!(shipped.contains(&"en-US".parse().unwrap()));
    assert!(shipped.contains(&"qps-ploc".parse().unwrap()));
    let registered = Resources::registered();
    assert!(
        registered
            .iter()
            .any(|r| std::ptr::eq(*r, &probe::RESOURCES))
    );
    assert!(
        registered
            .iter()
            .any(|r| std::ptr::eq(*r, &dynamic::RESOURCES))
    );
}

#[test]
fn resources_records_the_crate_its_files_and_its_dynamic_prefixes() {
    assert_eq!(probe::RESOURCES.crate_name, "kiln-l10n");
    assert_eq!(probe::RESOURCES.files.len(), 2);
    assert_eq!(probe::RESOURCES.files[1].locale, "qps-ploc");
    assert!(
        probe::RESOURCES.files[0]
            .source
            .contains("probe-title = Probe Labels")
    );
    assert!(probe::RESOURCES.declared_in.ends_with("catalogue.rs"));
    assert!(probe::RESOURCES.dynamic_prefixes.is_empty());
    assert_eq!(dynamic::RESOURCES.dynamic_prefixes, ["probe-dynamic-"]);
}

#[test]
fn public_keys_are_those_in_public_groups() {
    let source = probe::RESOURCES.files[0].source;
    assert_eq!(public_keys(source), ["probe-cancel"]);
    let terms = "## public\n-brand = Tenmoku\nshared = Shared\n\n## Private\nmine = Mine\n";
    assert_eq!(public_keys(terms), ["-brand", "shared"]);
}

#[test]
fn negotiation_filters_against_the_shipped_locales_and_ends_in_en_us() {
    let shipped = ids(&["en-US", "de"]);
    assert_eq!(
        negotiate(&ids(&["de-AT", "fr"]), &shipped),
        ids(&["de", "en-US"])
    );
    assert_eq!(
        negotiate(&ids(&["xx", "de"]), &shipped),
        ids(&["de", "en-US"])
    );
    assert_eq!(negotiate(&ids(&["xx"]), &shipped), ids(&["en-US"]));
    assert_eq!(negotiate(&[], &shipped), ids(&["en-US"]));
    // Nothing after en-US is reached, so the chain stops there.
    assert_eq!(negotiate(&ids(&["en-US", "de"]), &shipped), ids(&["en-US"]));
    assert_eq!(
        negotiate(&ids(&["de"]), &ids(&["de"])),
        ids(&["de", "en-US"])
    );
}

fn sources(list: &[(&str, &str, &str)]) -> Vec<(String, String, String)> {
    list.iter()
        .map(|(l, n, t)| (l.to_string(), n.to_string(), t.to_string()))
        .collect()
}

#[test]
fn each_bundle_holds_only_its_own_locale() {
    // pl's message references a term only en-US defines: at run time it is
    // formatted in pl's bundle, which doesn't have it.
    let catalogue = Catalogue::from_sources(
        sources(&[
            (
                "en-US",
                "en.ftl",
                "-brand = Tenmoku\nabout = About { -brand }\n",
            ),
            ("pl", "pl.ftl", "about = O { -brand }\n"),
        ]),
        &ids(&["pl"]),
    );
    let formatted = catalogue.try_format("about", &Args::new()).unwrap();
    assert_eq!(
        formatted.locale,
        "pl".parse::<LanguageIdentifier>().unwrap()
    );
    assert_eq!(formatted.text, "O {-brand}");
    assert_eq!(formatted.errors.len(), 1, "{:?}", formatted.errors);
}

#[test]
fn a_duplicate_key_is_an_error_and_the_first_wins() {
    let catalogue = Catalogue::from_sources(
        sources(&[
            ("en-US", "a.ftl", "same = From a\n"),
            ("en-US", "b.ftl", "same = From b\n"),
        ]),
        &[],
    );
    assert_eq!(catalogue.errors().len(), 1, "{:?}", catalogue.errors());
    assert!(catalogue.errors()[0].contains("b.ftl"));
    assert_eq!(catalogue.message("same"), "From a");
}

#[test]
fn a_parse_error_is_reported_and_the_rest_still_loads() {
    let catalogue = Catalogue::from_sources(
        sources(&[(
            "en-US",
            "a.ftl",
            "good = Good\nbad = { $\nalso-good = Also\n",
        )]),
        &[],
    );
    assert!(!catalogue.errors().is_empty());
    assert_eq!(catalogue.message("good"), "Good");
    assert_eq!(catalogue.message("also-good"), "Also");
}

#[test]
fn text_converts_from_strings_as_verbatim() {
    assert_eq!(Text::from("Literal"), Text::Verbatim("Literal".into()));
    assert_eq!(Text::from(String::from("Owned")).resolve(), "Owned");
    assert_eq!(Text::from(&String::from("Borrowed")).resolve(), "Borrowed");
    assert_eq!(Text::from(SharedString::from("Shared")).resolve(), "Shared");
    assert_eq!(
        Text::from(std::borrow::Cow::Borrowed("Cow")).resolve(),
        "Cow"
    );
    assert_eq!(Text::verbatim("x").key(), None);
    assert_eq!(text!("probe-title").key(), Some("probe-title"));
    assert_eq!(Text::default().resolve(), "");
    assert_eq!(verbatim!("Cargo.toml"), "Cargo.toml");
}

#[test]
fn args_are_named_and_a_repeated_name_replaces() {
    let args = Args::new()
        .with("name", "a")
        .with("count", 2)
        .with("name", "b");
    assert_eq!(args.len(), 2);
    assert_eq!(args.get("name"), Some(&kiln_l10n::Arg::Str("b".into())));
    assert_eq!(
        args.iter().map(|(n, _)| n).collect::<Vec<_>>(),
        ["name", "count"]
    );
}
