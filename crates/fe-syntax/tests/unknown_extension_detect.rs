//! A file whose extension no language claims is detected by its content, as
//! one with no extension is: its first line's markers (`<?xml`, `<?php`,
//! `<!DOCTYPE html`, `<html`, ...), then the XML and JSON sniff. A claimed
//! extension still wins over both, and a modeline over it.

#![cfg(all(feature = "xml", feature = "json", feature = "html"))]

use std::path::{Path, PathBuf};

use fe_syntax::{
    GrammarSpec, detect_language, detect_language_name, language_names, register_language,
    unregister_language,
};

fn detect(path: &str, text: &str) -> Option<&'static str> {
    detect_language_name(Some(Path::new(path)), text)
}

#[test]
fn an_unknown_extension_goes_by_the_content() {
    for (text, language) in [
        ("<?xml version=\"1.0\"?>\n<notes/>\n", Some("xml")),
        ("<notes><note>a</note></notes>\n", Some("xml")),
        ("{\"a\":1}\n", Some("json")),
        ("[1, 2]\n", Some("json")),
        ("<!doctype HTML>\n<p>hi</p>\n", Some("html")),
        (
            "<!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.0//EN\">\n",
            Some("html"),
        ),
        ("  <html lang=\"en\">\n", Some("html")),
        ("<HTML>\n", Some("html")),
        ("<html>", Some("html")),
        ("just some notes\n", None),
        ("<htmlish/>\n", Some("xml")),
        ("<!doctypehtml>\n", None),
        ("<!doctype htmlx>\n", None),
        ("\n<!doctype html>\n", None),
    ] {
        assert_eq!(detect("notes.zz", text), language, "{text:?}");
        assert_eq!(detect("notes", text), language, "{text:?}");
    }
}

#[test]
fn a_known_extension_wins_over_the_content() {
    assert_eq!(detect("a.json", "<?xml version=\"1.0\"?>\n"), Some("json"));
    assert_eq!(detect("a.json", "<a><b/></a>"), Some("json"));
    assert_eq!(detect("a.xml", "<!DOCTYPE html>\n"), Some("xml"));
    assert_eq!(detect("a.html", "{\"a\":1}"), Some("html"));
}

#[test]
fn a_modeline_wins_over_a_known_extension() {
    let text = "{\"a\": 1}\n";
    let end = "<!-- vim: set ft=xml: -->\n";
    assert_eq!(
        detect_language(Some(Path::new("a.json")), text, end),
        Some("xml")
    );
    assert_eq!(
        detect_language(
            Some(Path::new("notes.zz")),
            "<!-- vim: set ft=json: -->\n<a/>",
            ""
        ),
        Some("json")
    );
}

/// `<?php` picks PHP only where a PHP grammar is compiled in or registered:
/// the JSON fixture stands in for one that is installed.
#[test]
fn php_needs_its_grammar() {
    let text = "<?php\necho 1;\n";
    let built_in = language_names().any(|name| name == "php");
    if !built_in {
        assert_eq!(detect("page.zz", text), None);
        let manifest = std::env::var_os("CARGO_MANIFEST_DIR").expect("run by cargo");
        let wasm = PathBuf::from(manifest).join("tests/fixtures/tree-sitter-json.wasm");
        register_language(&GrammarSpec {
            language: "php".into(),
            wasm,
            ..GrammarSpec::default()
        })
        .unwrap();
    }
    assert_eq!(detect("page.zz", text), Some("php"));
    assert_eq!(detect("page", "  <?php\n"), Some("php"));
    assert_eq!(detect("page.zz", "x <?php\n"), None);
    assert_eq!(detect("page.json", text), Some("json"));
    if !built_in {
        assert!(unregister_language("php"));
    }
}
