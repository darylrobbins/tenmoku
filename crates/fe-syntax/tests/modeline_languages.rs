//! Modelines naming a file's language: Vim's, Emacs's and our own.

#![cfg(all(
    feature = "xml",
    feature = "rust",
    feature = "python",
    feature = "bash",
    feature = "yaml",
    feature = "json"
))]

use std::path::Path;

use fe_syntax::{
    detect_language, detect_language_name, language_name_for_modeline, language_names,
};

fn head(text: &str) -> Option<&'static str> {
    language_name_for_modeline(text, "")
}

fn tail(text: &str) -> Option<&'static str> {
    language_name_for_modeline("", text)
}

#[test]
fn vim_set_and_bare_forms() {
    for text in [
        "// vim: set ft=xml:\n",
        "// vim: set filetype=xml ts=2 sw=2:\n",
        "# vim: se ft=xml:\n",
        "# vim: ft=xml\n",
        "# vim:ft=xml:ts=4\n",
        "# vi: ts=4 syntax=xml\n",
        "# vim: syn=xml\n",
        "vim: ft=xml\n",
        "/* vim: set ft=xml: */\n",
        "# Vim: set ft=xml:\n",
    ] {
        assert_eq!(head(text), Some("xml"), "{text:?}");
        assert_eq!(tail(text), Some("xml"), "{text:?}");
    }
    for text in [
        // The set form ends at a colon.
        "# vim: set ft=xml\n",
        // `Vim:` takes only the set form.
        "# Vim: ft=xml\n",
        // A marker must start a word.
        "#vim: ft=xml\n",
        "# nvim: ft=xml\n",
        "# vim ft=xml\n",
    ] {
        assert_eq!(head(text), None, "{text:?}");
    }
}

#[test]
fn vim_versioned_and_ex_prefixes() {
    for text in [
        "# vim700: ft=xml\n",
        "# vim>600: set ft=xml:\n",
        "# vim<900: ft=xml\n",
        "# vim=800: ft=xml\n",
        "# ex: ft=xml\n",
        "# ex: set ft=xml:\n",
    ] {
        assert_eq!(head(text), Some("xml"), "{text:?}");
    }
    // Not at the start of a line, as in Vim.
    assert_eq!(head("ex: ft=xml\n"), None);
    assert_eq!(head("# vimx: ft=xml\n"), None);
}

#[test]
fn vim_only_first_and_last_five_lines() {
    let filler = "x\n".repeat(4);
    let fifth = format!("{filler}# vim: ft=xml\n");
    assert_eq!(head(&fifth), Some("xml"));
    let sixth = format!("{filler}x\n# vim: ft=xml\n");
    assert_eq!(head(&sixth), None);

    let fifth_from_end = format!("# vim: ft=xml\n{filler}");
    assert_eq!(tail(&fifth_from_end), Some("xml"));
    let sixth_from_end = format!("# vim: ft=xml\nx\n{filler}");
    assert_eq!(tail(&sixth_from_end), None);

    // Head's lines come first.
    assert_eq!(
        language_name_for_modeline("# vim: ft=rust\n", "# vim: ft=xml\n"),
        Some("rust")
    );
}

#[test]
fn vim_ignores_other_options_and_takes_the_last_ft() {
    for text in [
        "# vim: ts=2 sw=2 noet ft=xml\n",
        "# vim: set ts=2 ft=python sw=2 ft=xml:\n",
        "# vim: ft=python:ft=xml:\n",
        "# vim: filetype=python syntax=xml\n",
        "# vim: set ft=xml: ft=python\n",
        "# vim: ft=xml.doxygen\n",
        "# vim: fileformat=unix ft=xml\n",
    ] {
        assert_eq!(head(text), Some("xml"), "{text:?}");
    }
    assert_eq!(head("# vim: ts=2 sw=2 et\n"), None);
    assert_eq!(head("# vim: fenc=python\n"), None);
}

#[test]
fn emacs_mode_and_lone_token_forms() {
    for text in [
        "# -*- mode: xml -*-\n",
        "<!-- -*- Mode: xml -*- -->\n",
        "# -*- MODE:xml -*-\n",
        "# -*- coding: utf-8; mode: xml; indent-tabs-mode: nil -*-\n",
        "# -*- xml -*-\n",
        "# -*-xml-*-\n",
        "# -*- mode: nxml -*-\n",
    ] {
        assert_eq!(head(text), Some("xml"), "{text:?}");
    }
    for text in [
        "# -*- coding: utf-8 -*-\n",
        "# -*- mode: xml\n",
        "# -*- two words -*-\n",
        // The first line only.
        "\n# -*- mode: xml -*-\n",
    ] {
        assert_eq!(head(text), None, "{text:?}");
    }
    // Not at the end of a file: that's its Local Variables block.
    assert_eq!(tail("# -*- mode: xml -*-\nx\n"), None);
}

#[test]
fn emacs_second_line_after_shebang() {
    assert_eq!(
        head("#!/usr/bin/env python3\n# -*- mode: yaml -*-\n"),
        Some("yaml")
    );
    assert_eq!(head("#!/bin/sh\n# -*- yaml -*-\n"), Some("yaml"));
    assert_eq!(head("x\n# -*- mode: yaml -*-\n"), None);
    assert_eq!(head("#!/bin/sh\nx\n# -*- mode: yaml -*-\n"), None);
}

#[test]
fn tenmoku_form() {
    for text in [
        "# tenmoku: language=xml\n",
        "// tenmoku: filetype=xml\n",
        "<!-- tenmoku: ft=xml -->\n",
        "/* tenmoku: language=xml */\n",
        "--tenmoku: language=xml\n",
        "tenmoku: language=xml\n",
        "; Tenmoku: language=xml\n",
    ] {
        assert_eq!(head(text), Some("xml"), "{text:?}");
        assert_eq!(tail(text), Some("xml"), "{text:?}");
    }
    assert_eq!(head("# tenmoku: tabs=2\n"), None);
    assert_eq!(head("# mytenmoku: language=xml\n"), None);
    // Vim's and Emacs's names come first.
    assert_eq!(
        head("# tenmoku: language=xml\n# vim: ft=python\n"),
        Some("python")
    );
    // Ours stands in for a name theirs don't resolve.
    assert_eq!(
        head("# vim: ft=nosuchlanguage\n# tenmoku: language=xml\n"),
        Some("xml")
    );
}

#[test]
fn name_mapping_table() {
    let compiled = |language: &str| language_names().any(|name| name == language);
    for (name, ours) in [
        ("sh", "bash"),
        ("bash", "bash"),
        ("zsh", "bash"),
        ("ksh", "bash"),
        ("shell-script", "bash"),
        ("sh-mode", "bash"),
        ("c++", "cpp"),
        ("cpp", "cpp"),
        ("cs", "csharp"),
        ("csharp", "csharp"),
        ("js", "javascript"),
        ("javascript", "javascript"),
        ("javascriptreact", "javascript"),
        ("jsx", "javascript"),
        ("ts", "typescript"),
        ("typescript", "typescript"),
        ("typescriptreact", "tsx"),
        ("py", "python"),
        ("python", "python"),
        ("rb", "ruby"),
        ("ruby", "ruby"),
        ("md", "markdown"),
        ("markdown", "markdown"),
        ("yml", "yaml"),
        ("yaml", "yaml"),
        ("make", "make"),
        ("makefile", "make"),
        ("dockerfile", "dockerfile"),
        ("nxml", "xml"),
        ("xml", "xml"),
        ("xsd", "xml"),
        ("xslt", "xml"),
        ("svg", "xml"),
        ("json", "json"),
        ("jsonc", "json"),
        ("toml", "toml"),
        ("ini", "ini"),
        ("dosini", "ini"),
        ("conf", "ini"),
        ("gitcommit", "gitcommit"),
        ("diff", "diff"),
        // Not in the table: our names, extensions and fence aliases.
        ("rust", "rust"),
        ("go", "go"),
        ("lua", "lua"),
        ("golang", "go"),
        ("rs", "rust"),
    ] {
        let want = compiled(ours).then_some(ours);
        for text in [
            format!("# vim: ft={name}\n"),
            format!("# vim: ft={}\n", name.to_ascii_uppercase()),
            format!("# -*- mode: {name} -*-\n"),
            format!("# tenmoku: language={name}\n"),
        ] {
            assert_eq!(head(&text), want, "{text:?}");
        }
    }
}

#[test]
fn unknown_name_falls_through_to_path() {
    for text in [
        "# vim: ft=nosuchlanguage\n",
        "# -*- mode: nosuchlanguage -*-\n",
        "# tenmoku: language=nosuchlanguage\n",
        // Only reached through injections.
        "# vim: ft=markdown_inline\n",
    ] {
        assert_eq!(head(text), None, "{text:?}");
        assert_eq!(
            detect_language(Some(Path::new("a.py")), text, text),
            Some("python"),
            "{text:?}"
        );
        assert_eq!(detect_language(None, text, ""), None, "{text:?}");
    }
}

#[test]
fn modeline_beats_path_and_shebang() {
    let rust = Some(Path::new("main.rs"));
    assert_eq!(
        detect_language(rust, "fn main() {}\n", "// vim: set ft=xml:\n"),
        Some("xml")
    );
    assert_eq!(
        detect_language(rust, "#!/usr/bin/env python3\n# -*- mode: yaml -*-\n", ""),
        Some("yaml")
    );
    assert_eq!(
        detect_language(None, "#!/usr/bin/env python3\n", "# vim: ft=json\n"),
        Some("json")
    );
    assert_eq!(
        detect_language(None, "#!/bin/sh\n", "# tenmoku: language=xml\n"),
        Some("xml")
    );
    // The head-only call reads modelines in the head.
    assert_eq!(
        detect_language_name(rust, "// vim: ft=python\n"),
        Some("python")
    );
    // Without one, the path and then the shebang decide, as before.
    assert_eq!(
        detect_language(rust, "#!/usr/bin/env python3\n", "x\n"),
        Some("rust")
    );
    assert_eq!(
        detect_language(None, "#!/usr/bin/env python3\n", "x\n"),
        Some("python")
    );
}
