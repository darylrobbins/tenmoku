// SPDX-License-Identifier: Apache-2.0

//! [`TintedIcons`] gives each language a glyph of its own, in its family's
//! `file_icon` token, and never the blank file to an extension Material Icon
//! Theme's set knows.

use std::collections::BTreeMap;

use crate::{Icon, IconRef, IconSet, TintedIcons, file_icons_coloured, file_icons_tinted::*};

/// Each language with sample file names, dialects and language-selecting
/// names among them, and its family's token.
const LANGUAGES: &[(&str, &[&str], &str)] = &[
    ("Rust", &["main.rs"], RUST),
    ("C", &["main.c", "util.h"], CODE),
    (
        "C++",
        &["main.cpp", "util.hpp", "lib.cc", "x.cxx", "x.hh"],
        CODE,
    ),
    ("C#", &["Program.cs", "script.csx"], CODE),
    ("F#", &["Program.fs", "lib.fsi", "script.fsx"], CODE),
    ("Go", &["main.go"], CODE),
    ("Java", &["Main.java"], CODE),
    (
        "Kotlin",
        &["Main.kt", "script.kts", "plugin.gradle.kts"],
        CODE,
    ),
    ("Scala", &["Main.scala", "build.sc"], CODE),
    ("Groovy", &["script.groovy", "settings.gradle"], CODE),
    ("Swift", &["main.swift"], CODE),
    ("Dart", &["main.dart"], CODE),
    ("Zig", &["main.zig", "build.zig.zon"], CODE),
    ("Haskell", &["Main.hs", "Lit.lhs"], CODE),
    ("OCaml", &["main.ml", "main.mli"], CODE),
    ("Elm", &["Main.elm"], CODE),
    ("Gleam", &["app.gleam"], CODE),
    ("Erlang", &["app.erl", "app.hrl"], CODE),
    ("Elixir", &["app.ex", "app_test.exs", "page.heex"], CODE),
    ("Racket", &["main.rkt"], CODE),
    ("Solidity", &["Token.sol"], CODE),
    ("Python", &["main.py", "stubs.pyi", "app.pyw"], SCRIPT),
    (
        "Ruby",
        &["app.rb", "view.erb", "tasks.rake", "gem.gemspec"],
        SCRIPT,
    ),
    ("Lua", &["init.lua"], SCRIPT),
    ("Perl", &["script.pl", "Module.pm"], SCRIPT),
    ("PHP", &["index.php"], SCRIPT),
    ("R", &["analysis.r", "report.Rmd"], SCRIPT),
    ("Julia", &["main.jl"], SCRIPT),
    (
        "Shell",
        &[
            "build.sh", "x.bash", "x.zsh", "x.fish", ".bashrc", ".zshrc", ".profile", ".envrc",
        ],
        SCRIPT,
    ),
    (
        "PowerShell",
        &["build.ps1", "Module.psm1", "Data.psd1"],
        SCRIPT,
    ),
    ("Vim script", &["plugin.vim", ".vimrc"], SCRIPT),
    ("JavaScript", &["app.js", "app.mjs", "app.cjs"], WEB),
    (
        "TypeScript",
        &["app.ts", "types.d.ts", "app.mts", "app.cts"],
        WEB,
    ),
    ("JSX", &["App.jsx", "App.tsx"], WEB),
    ("HTML", &["index.html", "index.htm", "page.xhtml"], WEB),
    ("CSS", &["style.css"], WEB),
    ("Sass", &["style.scss", "style.sass"], WEB),
    ("Vue", &["App.vue"], WEB),
    ("Svelte", &["App.svelte"], WEB),
    ("Astro", &["index.astro"], WEB),
    ("Twig", &["page.twig"], WEB),
    (
        "Markdown",
        &["notes.md", "x.markdown", "page.mdx", "README", "README.md"],
        MARKDOWN,
    ),
    ("LaTeX", &["paper.tex", "style.sty"], MARKDOWN),
    ("TOML", &["config.toml"], TOML),
    ("YAML", &["ci.yaml", "ci.yml"], CONFIG),
    (
        "INI",
        &[
            "setup.ini",
            "setup.cfg",
            "app.conf",
            "app.properties",
            ".editorconfig",
        ],
        CONFIG,
    ),
    (
        "Docker",
        &["Dockerfile", "Containerfile", "app.dockerfile"],
        CONFIG,
    ),
    ("Make", &["Makefile", "GNUmakefile", "rules.mk"], CONFIG),
    ("just", &["justfile", "tasks.just"], CONFIG),
    ("CMake", &["CMakeLists.txt", "deps.cmake"], CONFIG),
    ("Nginx", &["nginx.conf"], CONFIG),
    ("Nix", &["flake.nix"], CONFIG),
    (
        "Terraform",
        &["main.tf", "prod.tfvars", "config.hcl"],
        CONFIG,
    ),
    (
        "Starlark",
        &["defs.bzl", "rules.star", "BUILD.bazel"],
        CONFIG,
    ),
    ("KDL", &["config.kdl"], CONFIG),
    (
        "Git",
        &[
            ".gitignore",
            ".gitattributes",
            ".gitmodules",
            ".gitconfig",
            "COMMIT_EDITMSG",
            "git-rebase-todo",
        ],
        CONFIG,
    ),
    ("JSON", &["data.json", "tsconfig.jsonc", "x.json5"], DATA),
    (
        "XML",
        &["layout.xml", "schema.xsd", "style.xsl", "Info.plist"],
        DATA,
    ),
    ("CSV", &["table.csv", "table.tsv"], DATA),
    ("SQL", &["schema.sql"], DATA),
    ("Protocol Buffers", &["api.proto"], DATA),
    ("Diff", &["fix.diff", "fix.patch"], DATA),
];

#[test]
fn every_language_has_its_own_icon() {
    let mut owner: BTreeMap<String, &str> = BTreeMap::new();
    for &(language, samples, tint) in LANGUAGES {
        let IconRef::Tinted(icon, _) = TintedIcons.file(samples[0]) else {
            panic!("{language}: not tinted");
        };
        assert!(
            !matches!(icon, Icon::File | Icon::FileCode),
            "{language}: {icon:?}"
        );
        for &sample in samples {
            assert_eq!(
                TintedIcons.file(sample),
                IconRef::Tinted(icon, tint),
                "{language}: {sample}"
            );
        }
        if let Some(other) = owner.insert(format!("{icon:?}"), language) {
            panic!("{language} and {other} share {icon:?}");
        }
    }
}

#[test]
fn material_extensions_are_never_blank() {
    for &(extension, _) in file_icons_coloured::EXTENSIONS {
        let name = format!("file.{extension}");
        let icon = TintedIcons.file(&name);
        assert!(matches!(icon, IconRef::Tinted(..)), "{name}");
        assert_ne!(icon, Icon::File, "{name}");
    }
}
