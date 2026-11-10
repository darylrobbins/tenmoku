// SPDX-License-Identifier: Apache-2.0

//! The pseudo-locale `en-XA`: generated from en-US, accented, longer and
//! bracketed, with placeables and selectors intact, and built only on request.

use kiln_l10n::{Args, Catalogue, LanguageIdentifier, PSEUDO_LOCALE, pseudo_source};

const SOURCE: &str = "\
greeting = Hello, { $name }!
files =
    { $count ->
        [one] One file changed
       *[other] { $count } files changed
    }
-brand = Tenmoku
about = About { -brand }
    .title = Open the page
go = Go to { about }
starred = **{ $name }** bold
";

fn ids(tags: &[&str]) -> Vec<LanguageIdentifier> {
    tags.iter().map(|t| t.parse().unwrap()).collect()
}

fn catalogue(requested: &[&str]) -> Catalogue {
    Catalogue::from_sources(
        [("en-US".to_owned(), "probe".to_owned(), SOURCE.to_owned())],
        &ids(requested),
    )
}

#[test]
fn pseudo_locale_transforms_text_not_placeables() {
    let catalogue = catalogue(&["en-XA"]);
    assert_eq!(catalogue.errors(), [] as [String; 0]);
    assert_eq!(catalogue.chain()[0].to_string(), PSEUDO_LOCALE);

    let args = Args::new().with("name", "Ada");
    let text = catalogue.try_format("greeting", &args).unwrap().text;
    assert!(text.starts_with('⟦') && text.ends_with('⟧'), "{text}");
    assert!(text.contains("Ada"), "the argument is intact: {text}");
    assert!(text.contains("Ĥéļļö"), "accented: {text}");
    // About 35% longer than "Hello, Ada!" (11 characters) plus brackets.
    let plain = "Hello, Ada!".chars().count();
    assert!(text.chars().count() >= plain * 135 / 100, "longer: {text}");

    // Selectors pick the same variant, and the plural argument survives.
    let one = catalogue
        .try_format("files", &Args::new().with("count", 1))
        .unwrap();
    let many = catalogue
        .try_format("files", &Args::new().with("count", 5))
        .unwrap();
    assert!(one.errors.is_empty() && many.errors.is_empty());
    assert!(one.text.contains("Öñé"), "{}", one.text);
    assert!(
        many.text.contains('5') && many.text.contains("ƒîļéš"),
        "{}",
        many.text
    );
    assert_eq!(
        one.text.matches('⟦').count(),
        1,
        "bracketed once: {}",
        one.text
    );

    // A message and a term referenced from one: one pair of brackets.
    let go = catalogue.try_format("go", &Args::new()).unwrap();
    assert!(go.errors.is_empty(), "{:?}", go.errors);
    assert_eq!(go.text.matches('⟦').count(), 1, "{}", go.text);
    assert_eq!(go.text.matches('⟧').count(), 1, "{}", go.text);
    assert!(go.text.contains("Ţéñmöķü"), "{}", go.text);

    // A term reference and an attribute.
    let about = catalogue.try_format("about", &Args::new()).unwrap();
    assert!(about.errors.is_empty(), "{:?}", about.errors);
    assert!(about.text.contains("Ţéñmöķü"), "{}", about.text);
    assert_eq!(about.text.matches('⟦').count(), 1, "{}", about.text);
    assert_eq!(about.text.matches('⟧').count(), 1, "{}", about.text);
    let title = catalogue.attribute("about", "title", &Args::new()).unwrap();
    assert!(title.starts_with('⟦') && title.ends_with('⟧'), "{title}");

    // The generated source still parses with no errors.
    let source = pseudo_source(SOURCE);
    assert!(
        fluent_syntax::parser::parse(source.as_str()).is_ok(),
        "{source}"
    );
}

#[test]
fn pseudo_locale_is_built_only_on_request() {
    let plain = catalogue(&["en-US"]);
    assert_eq!(plain.chain()[0].to_string(), "en-US");
    assert_eq!(
        plain.try_format("about", &Args::new()).unwrap().text,
        "About Tenmoku"
    );
    let pseudo = catalogue(&["en-XA"]);
    assert_eq!(pseudo.shipped(), ids(&["en-US"]), "never a shipped locale");
}

#[test]
fn pseudo_locale_keeps_text_that_starts_with_a_syntax_character() {
    let catalogue = catalogue(&["en-XA"]);
    assert_eq!(catalogue.errors(), [] as [String; 0]);
    let text = catalogue
        .try_format("starred", &Args::new().with("name", "Ada"))
        .unwrap()
        .text;
    assert!(text.starts_with("⟦**Ada**"), "{text}");
}
