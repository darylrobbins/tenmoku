// SPDX-License-Identifier: Apache-2.0

//! The root catalogue check's fixtures (layer 3 of section 8).
//!
//! First the per-bundle reference walk's table: a reference must resolve in
//! the bundle that formats it, its own locale's, in every branch. A term and
//! an attribute that only en-US defines, used in a branch the dummy
//! arguments don't select, pass formatting but fail the walk, and the
//! runtime agrees with the walk once that branch is selected. Then unused
//! keys, duplicates and stale translations, on a fixture crate read from
//! disk.

use std::path::PathBuf;

use kiln_l10n::lint::{CatalogueReport, catalogue};
use kiln_l10n::{Args, Catalogue, LanguageIdentifier};

const EN_FILES: &str = "
-brand = Tenmoku
files-left = { $count ->
    [one] One file left
    [few] { $count } files left in { -brand }
   *[other] { $count } files left
}
save = Save
    .tooltip = Save the file
save-hint = { $kind ->
    [file] { save.tooltip }
   *[other] Save all
}
";

/// `pl` with `-brand` and `save.tooltip` only in en-US, each used in a
/// branch the dummy arguments don't select.
const PL_BROKEN: &str = "
files-left = { $count ->
    [one] Został jeden plik
    [few] Zostały { $count } pliki { -brand }
   *[other] Zostało { $count } plików
}
save = Zapisz
save-hint = { $kind ->
    [file] { save.tooltip }
   *[other] Zapisz wszystko
}
";

/// The same, fixed: `pl` defines its own `-brand` and `save.tooltip`.
const PL_FIXED: &str = "
-brand = Tenmoku
files-left = { $count ->
    [one] Został jeden plik
    [few] Zostały { $count } pliki { -brand }
   *[other] Zostało { $count } plików
}
save = Zapisz
    .tooltip = Zapisz plik
save-hint = { $kind ->
    [file] { save.tooltip }
   *[other] Zapisz wszystko
}
";

fn report(sources: &[(&str, &str)]) -> CatalogueReport {
    let mut check = catalogue();
    for (locale, text) in sources {
        check = check.resource(locale, &format!("{locale}/files.ftl"), text);
    }
    check.report()
}

fn runtime(sources: &[(&str, &str)], locale: &str) -> Catalogue {
    let sources = sources
        .iter()
        .map(|(l, t)| (l.to_string(), format!("{l}/files.ftl"), t.to_string()));
    let requested: Vec<LanguageIdentifier> = vec![locale.parse().unwrap()];
    Catalogue::from_sources(sources, &requested)
}

#[test]
fn an_undefined_term_in_en_us_is_rejected_by_the_walk_though_formatting_passes() {
    let en = "files-left = { $count ->\n    [one] One file left\n    [few] { $count } in { -brand }\n   *[other] { $count } files left\n}\n";
    let report = report(&[("en-US", en)]);
    assert_eq!(
        report.walk,
        ["en-US: en-US/files.ftl: files-left [few]: `-brand` isn't defined in en-US"],
        "{report}"
    );
    assert!(
        report.format.is_empty(),
        "$count = 1 selects [one]: {report}"
    );
}

#[test]
fn a_term_only_in_en_us_in_an_unselected_branch_is_rejected() {
    let sources = [("en-US", EN_FILES), ("pl", PL_BROKEN)];
    let report = report(&sources);
    assert!(
        report.walk.contains(
            &"pl: pl/files.ftl: files-left [few]: `-brand` isn't defined in pl (defined only in en-US)"
                .to_string()
        ),
        "{report}"
    );
    assert!(
        report.format.is_empty(),
        "the dummy $count = 1 selects [one]: {report}"
    );

    // At run time, in pl, $count = 3 selects [few]: Fluent reports the unknown term.
    let pl = runtime(&sources, "pl");
    let one = pl
        .try_format("files-left", &Args::new().with("count", 1))
        .unwrap();
    assert!(one.errors.is_empty(), "{one:?}");
    let few = pl
        .try_format("files-left", &Args::new().with("count", 3))
        .unwrap();
    assert_eq!(few.locale.to_string(), "pl");
    assert_eq!(few.text, "Zostały 3 pliki {-brand}");
    assert_eq!(few.errors.len(), 1, "{few:?}");
    assert!(few.errors[0].contains("brand"), "{few:?}");
}

#[test]
fn an_attribute_only_in_en_us_in_an_unselected_branch_is_rejected() {
    let sources = [("en-US", EN_FILES), ("pl", PL_BROKEN)];
    let report = report(&sources);
    assert!(
        report.walk.contains(
            &"pl: pl/files.ftl: save-hint [file]: `save.tooltip`: pl's `save` has no `.tooltip` (defined only in en-US)"
                .to_string()
        ),
        "{report}"
    );
    assert_eq!(report.walk.len(), 2, "{report}");
    assert!(
        report.format.is_empty(),
        "the dummy $kind selects *[other]: {report}"
    );

    // At run time, in pl, $kind = "file" reports the unknown attribute.
    let pl = runtime(&sources, "pl");
    let other = pl
        .try_format("save-hint", &Args::new().with("kind", "all"))
        .unwrap();
    assert!(other.errors.is_empty(), "{other:?}");
    let file = pl
        .try_format("save-hint", &Args::new().with("kind", "file"))
        .unwrap();
    assert_eq!(file.text, "{save.tooltip}");
    assert_eq!(file.errors.len(), 1, "{file:?}");
}

#[test]
fn the_same_two_fixed_are_accepted_and_format_in_every_branch() {
    let sources = [("en-US", EN_FILES), ("pl", PL_FIXED)];
    let report = report(&sources);
    assert!(report.walk.is_empty(), "{report}");
    assert!(report.format.is_empty(), "{report}");
    assert!(report.stale.is_empty(), "{report}");
    assert!(report.duplicates.is_empty(), "{report}");

    let pl = runtime(&sources, "pl");
    for count in [1, 3, 5, 22] {
        let formatted = pl
            .try_format("files-left", &Args::new().with("count", count))
            .unwrap();
        assert!(formatted.errors.is_empty(), "{formatted:?}");
    }
    assert_eq!(
        pl.format("files-left", &Args::new().with("count", 3)),
        "Zostały 3 pliki Tenmoku"
    );
    for kind in ["file", "all"] {
        let formatted = pl
            .try_format("save-hint", &Args::new().with("kind", kind))
            .unwrap();
        assert!(formatted.errors.is_empty(), "{formatted:?}");
    }
    assert_eq!(
        pl.format("save-hint", &Args::new().with("kind", "file")),
        "Zapisz plik"
    );
}

#[test]
fn every_message_formats_with_dummy_arguments() {
    let report = report(&[("en-US", "a = { b }\nb = { a }\nfine = { $x } and { $y }\n")]);
    assert!(report.walk.is_empty(), "{report}");
    assert_eq!(report.format.len(), 2, "both halves of the cycle: {report}");
    assert!(
        report.format.iter().all(|e| e.contains("Cyclic")),
        "{report}"
    );
}

#[test]
fn a_key_defined_twice_in_a_locale_is_an_error() {
    let report = catalogue()
        .resource("en-US", "a.ftl", "same = A\n-term = T\n")
        .resource("en-US", "b.ftl", "same = B\n-term = U\n")
        .resource("de", "c.ftl", "same = C\n")
        .report();
    assert_eq!(
        report
            .duplicates
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        [
            "en-US: message `same` is defined in a.ftl and b.ftl",
            "en-US: term `-term` is defined in a.ftl and b.ftl"
        ],
        "{report}"
    );
}

#[test]
fn a_parse_error_is_reported() {
    let report = report(&[("en-US", "ok = Fine\nbroken = { $\n")]);
    assert_eq!(report.parse.len(), 1, "{report}");
    assert!(!report.is_clean());
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/root")
}

#[test]
fn a_root_reports_unused_keys_stale_translations_and_missing_counts() {
    let report = catalogue()
        .crate_dir(root().join("alpha"))
        .data_keys(["alpha-data"])
        .report();
    assert!(report.parse.is_empty(), "{report}");
    // Used: a tr! literal, any literal, a dynamic prefix the crate's
    // resources! declares, bundled data, and references from messages.
    // Unused: a public key without exempt_public, a key used only in
    // tests, a key nothing names, and a term nothing references.
    assert_eq!(
        report.unused,
        [
            "en-US: alpha/l10n/en-US/alpha.ftl: `alpha-public` is unused",
            "en-US: alpha/l10n/en-US/alpha.ftl: `alpha-test-only` is unused",
            "en-US: alpha/l10n/en-US/alpha.ftl: `alpha-unused` is unused",
            "en-US: alpha/l10n/en-US/alpha.ftl: `-alpha-unused-term` is unused",
        ],
        "{report}"
    );
    assert_eq!(
        report.stale,
        [
            "pl: alpha/l10n/pl/alpha.ftl: `alpha-literal` takes `$extra`, which en-US's doesn't",
            "pl: alpha/l10n/pl/alpha.ftl: `alpha-stale` isn't in en-US",
        ],
        "{report}"
    );
    assert_eq!(
        report.walk,
        [
            "pl: alpha/l10n/pl/alpha.ftl: alpha-used: `alpha-referenced` isn't defined in pl (defined only in en-US)"
        ],
        "{report}"
    );
    assert_eq!(
        report.format,
        [
            "pl: alpha/l10n/pl/alpha.ftl: alpha-used: Resolver error: Unknown message: alpha-referenced"
        ],
        "the same reference, which every branch reaches: {report}"
    );
    // pl has two of en-US's eight messages; the other six fall back.
    assert_eq!(report.missing.get("pl"), Some(&6));
}

#[test]
fn a_library_root_exempts_its_public_keys() {
    let report = catalogue()
        .crate_dir(root().join("alpha"))
        .data_keys(["alpha-data"])
        .exempt_public()
        .dynamic_prefix("alpha-test-")
        .report();
    assert_eq!(
        report.unused,
        [
            "en-US: alpha/l10n/en-US/alpha.ftl: `alpha-unused` is unused",
            "en-US: alpha/l10n/en-US/alpha.ftl: `-alpha-unused-term` is unused",
        ],
        "{report}"
    );
}

#[test]
fn crates_under_reads_each_crate_with_a_manifest() {
    let under = catalogue()
        .crates_under(root())
        .data_keys(["alpha-data"])
        .report();
    let direct = catalogue()
        .crate_dir(root().join("alpha"))
        .data_keys(["alpha-data"])
        .report();
    assert_eq!(under.to_string(), direct.to_string());
    assert_eq!(under.unused.len(), 4, "{under}");
}

#[test]
fn a_term_defined_only_in_another_translation_does_not_resolve_in_pl() {
    let de = "-brand = Tenmoku\n";
    let pl = "greeting = Cześć { -brand }\n";
    let report = report(&[("en-US", "greeting = Hi\n"), ("de", de), ("pl", pl)]);
    assert!(
        report
            .walk
            .iter()
            .any(|line| line.starts_with("pl: ") && line.contains("`-brand` isn't defined in pl")),
        "{report}"
    );
}
