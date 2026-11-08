// SPDX-License-Identifier: Apache-2.0

//! Section 5 of the i18n design: arguments are isolated at interpolation
//! only, so selectors see raw values, and en-US with ASCII arguments is
//! byte-equal to the message.

use kiln_l10n::{Args, Catalogue, FSI, LanguageIdentifier, PDI, strip_isolates};

const EN: &str = "
greeting = { $gender ->
    [male] He opened { $name }
   *[other] They opened { $name }
}
kind = { $kind ->
    [file] File { $kind }
   *[other] Other { $kind }
}
open-failed = Can't open { $path }
files = { $count ->
    [one] One file
   *[other] { $count } files
}
";

const HE: &str = "
greeting = { $gender ->
    [male] הוא פתח את { $name }
   *[other] הם פתחו את { $name }
}
kind = { $kind ->
    [file] קובץ { $kind }
   *[other] אחר { $kind }
}
files = { $count ->
    [one] קובץ אחד
   *[other] { $count } קבצים
}
";

fn catalogue(requested: &str) -> Catalogue {
    let sources = [("en-US", "en.ftl", EN), ("he", "he.ftl", HE)]
        .map(|(l, n, t)| (l.to_string(), n.to_string(), t.to_string()));
    let requested: Vec<LanguageIdentifier> = vec![requested.parse().unwrap()];
    let catalogue = Catalogue::from_sources(sources, &requested);
    assert!(catalogue.errors().is_empty(), "{:?}", catalogue.errors());
    catalogue
}

fn isolated(text: &str) -> String {
    format!("{FSI}{text}{PDI}")
}

#[test]
fn an_rtl_locale_selects_on_the_raw_string_value() {
    let catalogue = catalogue("he");
    let text = catalogue.format(
        "greeting",
        &Args::new().with("gender", "male").with("name", "main.rs"),
    );
    assert_eq!(
        text.as_str(),
        format!("הוא פתח את {}", isolated("main.rs")),
        "[male] must match the raw `male`, not an isolated one"
    );
}

#[test]
fn an_argument_used_as_selector_and_text_is_isolated_only_where_shown() {
    let catalogue = catalogue("he");
    let text = catalogue.format("kind", &Args::new().with("kind", "file"));
    assert_eq!(text.as_str(), format!("קובץ {}", isolated("file")));

    // The same in en-US: selected on the raw value, and ASCII isn't isolated.
    let text = catalogue_en().format("kind", &Args::new().with("kind", "file"));
    assert_eq!(text.as_str(), "File file");
}

fn catalogue_en() -> Catalogue {
    catalogue("en-US")
}

#[test]
fn a_hebrew_argument_in_en_us_is_isolated_and_an_ascii_one_is_not() {
    let catalogue = catalogue_en();
    let hebrew = catalogue.format("open-failed", &Args::new().with("path", "קובץ.txt"));
    assert_eq!(
        hebrew.as_str(),
        format!("Can't open {}", isolated("קובץ.txt"))
    );
    let arabic = catalogue.format("open-failed", &Args::new().with("path", "ملف"));
    assert_eq!(arabic.as_str(), format!("Can't open {}", isolated("ملف")));
    let ascii = catalogue.format("open-failed", &Args::new().with("path", "notes.txt"));
    assert_eq!(
        ascii.as_str(),
        "Can't open notes.txt",
        "byte-equal to the message"
    );
}

#[test]
fn a_number_is_never_isolated_in_an_ltr_locale() {
    let catalogue = catalogue_en();
    assert_eq!(
        catalogue
            .format("files", &Args::new().with("count", 3))
            .as_str(),
        "3 files"
    );
    assert_eq!(
        catalogue
            .format("files", &Args::new().with("count", 1))
            .as_str(),
        "One file"
    );
    assert_eq!(
        catalogue
            .format("files", &Args::new().with("count", 2.5))
            .as_str(),
        "2.5 files"
    );
}

#[test]
fn a_number_selects_its_plural_category_in_an_rtl_locale() {
    let catalogue = catalogue("he");
    assert_eq!(
        catalogue
            .format("files", &Args::new().with("count", 1))
            .as_str(),
        "קובץ אחד"
    );
    assert_eq!(
        catalogue
            .format("files", &Args::new().with("count", 7))
            .as_str(),
        "7 קבצים"
    );
}

#[test]
fn strip_isolates_removes_every_isolate_mark() {
    let marked = format!("Can't open {}", isolated("קובץ.txt"));
    assert_eq!(strip_isolates(&marked), "Can't open קובץ.txt");
    assert_eq!(strip_isolates("a\u{2066}b\u{2067}c\u{2069}"), "abc");
    assert!(matches!(
        strip_isolates("plain"),
        std::borrow::Cow::Borrowed("plain")
    ));
}
