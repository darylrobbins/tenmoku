#![cfg(all(
    feature = "xml",
    feature = "yaml",
    feature = "bash",
    feature = "python"
))]

use std::path::Path;

use fe_syntax::detect_language_name;

#[test]
fn first_lines_pick_their_language_as_vs_code_does() {
    for (text, language) in [
        ("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<a/>\n", "xml"),
        ("<svg xmlns=\"http://www.w3.org/2000/svg\">\n", "xml"),
        ("<!doctype svg PUBLIC \"x\">\n", "xml"),
        ("  <?xml version=\"1.0\"?>\n", "xml"),
        ("<!doctype\u{feff}svg\n", "xml"),
        ("#\u{feff}-*- mode: shell-script -*-\n", "bash"),
        ("# -*- mode:\u{feff}shell-script -*-\n", "bash"),
        ("#cloud-config\nusers: []\n", "yaml"),
        ("# -*- mode: shell-script -*-\n", "bash"),
        ("#-*- coding: utf-8; mode:shell-script; -*-\n", "bash"),
    ] {
        assert_eq!(detect_language_name(None, text), Some(language), "{text:?}");
        assert_eq!(
            detect_language_name(Some(Path::new("user-data")), text),
            Some(language),
            "{text:?}"
        );
    }
}

#[test]
fn a_path_that_names_a_language_wins() {
    let text = "<?xml version=\"1.0\"?>\n";
    assert_eq!(
        detect_language_name(Some(Path::new("a.py")), text),
        Some("python")
    );
}

#[test]
fn near_misses_stay_undetected() {
    for text in [
        "# cloud-config\n",
        "# mode: shell-script\n",
        "# -*- mode: shell-script\n",
        "\n<?xml version=\"1.0\"?>\n",
        "<?XML version=\"1.0\"?>\n",
        "<!doctypesvg\n",
        "<!doctype\u{85}svg\n",
        "#\u{85}-*- mode: shell-script -*-\n",
        "# -*- mode:\u{85}shell-script -*-\n",
        "x #cloud-config\n",
    ] {
        assert_eq!(detect_language_name(None, text), None, "{text:?}");
    }
}
