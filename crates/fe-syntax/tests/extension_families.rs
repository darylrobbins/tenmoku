//! The XML, JSON, TOML and YAML families: project files, schemas, feeds and
//! the like by extension, lock files and tool settings by name.

#![cfg(all(feature = "xml", feature = "json", feature = "toml", feature = "yaml"))]

use std::path::Path;

use fe_syntax::language_name_for_path;

fn language(name: &str) -> Option<&'static str> {
    language_name_for_path(&Path::new("proj/sub").join(name))
}

#[test]
fn xml_family_extensions_and_names() {
    for extension in [
        "csproj",
        "vbproj",
        "fsproj",
        "vcxproj",
        "jsproj",
        "proj",
        "props",
        "targets",
        "nuspec",
        "resx",
        "xaml",
        "axaml",
        "xsd",
        "xsl",
        "xslt",
        "wsdl",
        "xliff",
        "xlf",
        "rss",
        "atom",
        "kml",
        "gpx",
        "storyboard",
        "xib",
        "appxmanifest",
        "vsixmanifest",
        "pubxml",
        "ruleset",
        "csl",
    ] {
        let name = format!("x.{extension}");
        assert_eq!(language(&name), Some("xml"), "{name}");
    }
    for name in [
        "web.config",
        "app.config",
        "nuget.config",
        "packages.config",
        "Directory.Build.props",
        "Directory.Build.targets",
        "Properties/PublishProfiles/x.pubxml",
    ] {
        assert_eq!(language(name), Some("xml"), "{name}");
    }
}

#[test]
fn json_toml_yaml_family_names() {
    for (name, want) in [
        ("poetry.lock", "toml"),
        ("uv.lock", "toml"),
        ("Pipfile", "toml"),
        ("pdm.lock", "toml"),
        ("Cargo.lock", "toml"),
        (".clang-format", "yaml"),
        (".clangd", "yaml"),
        (".clang-tidy", "yaml"),
        ("CITATION.cff", "yaml"),
        (".gemrc", "yaml"),
        (".yamllint", "yaml"),
        ("Pipfile.lock", "json"),
        (".prettierrc", "json"),
        (".lintstagedrc", "json"),
        (".stylelintrc", "json"),
    ] {
        assert_eq!(language(name), Some(want), "{name}");
    }
}

/// Names that say too little stay for the content to decide, or nothing.
#[test]
fn ambiguous_config_and_manifest_stay_undetected() {
    for name in [
        "config",
        "manifest",
        "x.config",
        "app.manifest",
        "MANIFEST",
        "web.config.bak",
        "poetry.lock.orig",
        "Pipfile.bak",
        "x.cff",
        "clang-format",
    ] {
        assert_eq!(language(name), None, "{name}");
    }
}

#[test]
fn names_ignore_ascii_case() {
    for (name, want) in [
        ("X.CSPROJ", "xml"),
        ("Main.XAML", "xml"),
        ("Web.Config", "xml"),
        ("NuGet.Config", "xml"),
        ("POETRY.LOCK", "toml"),
        ("pipfile", "toml"),
        (".CLANG-FORMAT", "yaml"),
        ("citation.cff", "yaml"),
        ("PIPFILE.LOCK", "json"),
        (".PrettierRC", "json"),
    ] {
        assert_eq!(language(name), Some(want), "{name}");
    }
}
