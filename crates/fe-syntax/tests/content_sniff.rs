//! A file whose path has no extension, or a buffer with no path, is XML or
//! JSON when its content starts like one, after everything else has had its
//! say.

#![cfg(all(feature = "xml", feature = "json"))]

use std::path::Path;

use fe_syntax::detect_language_name;

fn detect(path: Option<&str>, text: &str) -> Option<&'static str> {
    detect_language_name(path.map(Path::new), text)
}

#[test]
fn extensionless_xml_and_json_are_sniffed() {
    let xml = [
        "<Project Sdk=\"Microsoft.NET.Sdk\">\n  <PropertyGroup>\n  </PropertyGroup>\n</Project>\n",
        "<Project Sdk=\"Microsoft.NET.Sdk\" />\n",
        "\u{feff}<root><item/></root>",
        "\n\n  <config>\n  <key>value</key>\n</config>\n",
        "<!-- generated -->\n<feed xmlns=\"http://www.w3.org/2005/Atom\"></feed>\n",
        "<_private a=\"1\"></_private >",
        "<x:doc xmlns:x=\"urn:x\">text</x:doc>",
    ];
    let json = [
        "{\"name\": \"x\"}",
        "{\n  \"a\": [1, 2]\n}\n",
        "{}",
        "\u{feff}{ }",
        "  [1, 2, 3]",
        "[-1]",
        "[\n  {\"a\": 1}\n]",
        "[\"a\", \"b\"]",
        "[]",
    ];
    for path in [None, Some("bin/config"), Some("data"), Some(".myapprc")] {
        for text in xml {
            assert_eq!(detect(path, text), Some("xml"), "{path:?} {text:?}");
        }
        for text in json {
            assert_eq!(detect(path, text), Some("json"), "{path:?} {text:?}");
        }
    }
}

/// Only a closing tag or `/>` in the first 512 bytes makes it XML.
#[test]
fn xml_closes_within_the_head() {
    let text = format!("<root>{}</root>", "x".repeat(600));
    assert_eq!(detect(None, &text), None);
    let text = format!("<root>{}</root>", "x".repeat(400));
    assert_eq!(detect(None, &text), Some("xml"));
}

#[test]
fn unknown_extension_is_never_sniffed() {
    for path in [
        "notes.unknownext",
        "build.log",
        "dir.d/data.out",
        "Project.mine",
        ".myapprc.bak",
    ] {
        assert_eq!(detect(Some(path), "{\"a\": 1}"), None, "{path}");
        assert_eq!(detect(Some(path), "<a><b/></a>"), None, "{path}");
    }
}

#[test]
fn html_and_plain_text_are_not_sniffed() {
    for text in [
        "<!DOCTYPE html>\n<html><body/></html>\n",
        "<!doctype html>\n<p>hi</p>",
        "<html lang=\"en\"><br/></html>",
        "<HTML><BODY></BODY></HTML>",
        "<!-- page -->\n<html></html>",
        "hello <b>world</b>\n",
        "plain text\n{\"a\": 1}",
        "<root>never closed\n",
        "<a>mismatched</b>",
        "<1a/>",
        "< a/>",
        "<!-- comment never ends <a/>",
        "<!DOCTYPE note><note/>",
        "[section]\nkey = 1\n",
        "[[bin]]\nname = \"x\"\n",
        "{ key: 1 }",
        "{",
        "[",
        "",
        "   \n",
    ] {
        assert_eq!(detect(None, text), None, "{text:?}");
        assert_eq!(detect(Some("bin/config"), text), None, "{text:?}");
    }
}

#[test]
fn path_and_shebang_beat_the_sniff() {
    // The path decides first.
    assert_eq!(detect(Some("x.xml"), "{\"a\": 1}"), Some("xml"));
    assert_eq!(detect(Some("x.json"), "<a><b/></a>"), Some("json"));
    // A first line VS Code knows decides before the sniff.
    #[cfg(feature = "yaml")]
    assert_eq!(detect(None, "#cloud-config\n{\"a\": 1}\n"), Some("yaml"));
    assert_eq!(
        detect(None, "<?xml version=\"1.0\"?>\n{\"a\": 1}\n"),
        Some("xml")
    );
    #[cfg(feature = "python")]
    {
        assert_eq!(detect(Some("a.py"), "{\"a\": 1}"), Some("python"));
        assert_eq!(detect(Some("a.py"), "<a></a>"), Some("python"));
        assert_eq!(
            detect(Some("bin/tool"), "#!/usr/bin/env python3\n{\"a\": 1}\n"),
            Some("python")
        );
    }
    #[cfg(feature = "bash")]
    assert_eq!(detect(None, "#!/bin/sh\n<a></a>\n"), Some("bash"));
}
