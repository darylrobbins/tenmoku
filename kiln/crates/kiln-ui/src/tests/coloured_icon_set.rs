// SPDX-License-Identifier: LicenseRef-Proprietary

//! [`ColouredIcons`]: its tables, its bundled files, and what it answers.

use std::{collections::BTreeSet, path::PathBuf, sync::Arc};

use gpui::{AssetSource as _, SvgRenderer, TestAppContext};

use crate::{
    Appearance, Assets, ColouredIcons, IconRef, IconSet, LucideIcons, Theme,
    file_icons_coloured::{
        DEFAULT_FILE, DEFAULT_FOLDER, ENV_FILE, EXTENSIONS, FILE_NAMES, FILES, FOLDER_NAMES, LIGHT,
        OPEN, path,
    },
    folder_icon_ref_named, set_icon_set, set_theme,
};

/// The file stems the tables name, the defaults included, and each folder
/// stem's open one.
fn named_stems() -> BTreeSet<String> {
    let files = (FILE_NAMES.iter().chain(EXTENSIONS)).map(|&(_, stem)| stem.to_owned());
    let folders = (FOLDER_NAMES.iter().map(|&(_, stem)| stem))
        .chain([DEFAULT_FOLDER])
        .flat_map(|stem| [stem.to_owned(), format!("{stem}{OPEN}")]);
    files
        .chain([DEFAULT_FILE, ENV_FILE].map(str::to_owned))
        .chain(folders)
        .collect()
}

fn bundled_stems() -> BTreeSet<&'static str> {
    FILES.iter().map(|&(stem, ..)| stem).collect()
}

fn dark_file(name: &str) -> &'static str {
    ColouredIcons::file_path(name, Appearance::Dark)
}

fn dark_folder(name: &str, expanded: bool) -> &'static str {
    ColouredIcons::folder_path(name, expanded, Appearance::Dark)
}

#[test]
fn tables_point_at_bundled_files() {
    let bundled = bundled_stems();
    assert_eq!(bundled.len(), FILES.len(), "no two files share a stem");
    for stem in named_stems() {
        assert!(bundled.contains(&*stem), "{stem} isn't bundled");
    }

    let renderer = SvgRenderer::new(Arc::new(Assets));
    for &(stem, path, svg) in FILES {
        assert_eq!(path, format!("kiln-ui/file-icons/{stem}.svg"));
        assert!(svg.len() < 8 * 1024, "{stem}: {} bytes", svg.len());
        let text = std::str::from_utf8(svg).unwrap_or_else(|_| panic!("{stem}: UTF-8"));
        assert!(text.contains("viewBox"), "{stem}: no viewBox");
        assert!(renderer.parse_svg(svg).is_ok(), "{stem} doesn't parse");
        let served = Assets.load(path).expect("load").expect("served");
        assert_eq!(*served, *svg, "{stem}: Assets serves it");
    }
    let listed = Assets.list("kiln-ui/file-icons/").expect("list");
    assert_eq!(listed.len(), FILES.len());

    // At run time, not `env!`: a shared target can keep another checkout's path.
    let dir = std::env::var_os("CARGO_MANIFEST_DIR")
        .map_or_else(|| env!("CARGO_MANIFEST_DIR").into(), PathBuf::from)
        .join("assets/file-icons");
    let mut on_disk = BTreeSet::new();
    for entry in std::fs::read_dir(&dir).expect("assets/file-icons") {
        let path = entry.expect("entry").path();
        if path.extension().is_some_and(|e| e == "svg") {
            let stem = path.file_stem().expect("stem").to_string_lossy();
            on_disk.insert(stem.into_owned());
        }
    }
    let bundled: BTreeSet<String> = bundled.iter().map(|&s| s.to_owned()).collect();
    assert_eq!(on_disk, bundled, "every file on disk is compiled in");
    let license = std::fs::read_to_string(dir.join("LICENSE")).expect("LICENSE");
    assert!(license.contains("The MIT License"));
    assert!(license.contains("Material Extensions"));
}

#[test]
fn every_bundled_file_is_used() {
    let named = named_stems();
    for stem in bundled_stems() {
        let used = match stem.strip_suffix(LIGHT) {
            Some(base) => named.contains(base),
            None => named.contains(stem),
        };
        assert!(used, "{stem} is bundled but nothing names it");
    }
}

#[test]
fn tables_are_lowercase_and_unique() {
    for (what, table) in [
        ("names", FILE_NAMES),
        ("extensions", EXTENSIONS),
        ("folders", FOLDER_NAMES),
    ] {
        let mut seen = BTreeSet::new();
        for &(key, _) in table {
            assert_eq!(key, key.to_ascii_lowercase(), "{what}: {key}");
            assert!(seen.insert(key), "{what}: {key} twice");
        }
    }
}

#[test]
fn build_files_get_distinct_icons() {
    let names = [
        "Makefile",
        "justfile",
        ".editorconfig",
        "Cargo.toml",
        "a.toml",
    ];
    let default = dark_file("unknown");
    assert_eq!(default, path(DEFAULT_FILE));
    let icons: BTreeSet<&str> = names.iter().map(|name| dark_file(name)).collect();
    assert_eq!(icons.len(), names.len(), "{icons:?}");
    assert!(!icons.contains(default), "{icons:?}");
}

/// Every extension the highlighter of kiln's first host picks a language
/// by, as its language table lists them. kiln can't depend on an app, so
/// they're copied here.
const SYNTAX_EXTENSIONS: &[(&str, &[&str])] = &[
    ("rust", &["rs"]),
    ("python", &["py", "pyi", "pyw"]),
    ("javascript", &["js", "mjs", "cjs", "jsx"]),
    ("typescript", &["ts", "mts", "cts"]),
    ("tsx", &["tsx"]),
    (
        "json",
        &[
            "json",
            "jsonc",
            "webmanifest",
            "har",
            "jsonld",
            "geojson",
            "ipynb",
            "bowerrc",
            "jscsrc",
            "jslintrc",
            "vuerc",
            "eslintrc",
            "jsfmtrc",
            "jshintrc",
            "swcrc",
            "hintrc",
            "babelrc",
            "code-workspace",
            "code-snippets",
        ],
    ),
    ("toml", &["toml"]),
    ("go", &["go"]),
    ("c", &["c", "h"]),
    (
        "cpp",
        &["cc", "cpp", "cxx", "c++", "hh", "hpp", "hxx", "h++"],
    ),
    ("bash", &["sh", "bash", "zsh", "ksh"]),
    ("html", &["html", "htm"]),
    ("css", &["css"]),
    ("yaml", &["yaml", "yml"]),
    ("markdown", &["md", "markdown"]),
    ("java", &["java"]),
    ("ruby", &["rb", "gemspec", "rake", "ru", "podspec"]),
    ("lua", &["lua"]),
    ("php", &["php"]),
    ("csharp", &["cs"]),
    ("scala", &["scala", "sc"]),
    ("xml", &["xml", "svg", "plist"]),
    ("swift", &["swift"]),
    ("kotlin", &["kt", "kts"]),
    ("sql", &["sql"]),
    ("dockerfile", &["dockerfile"]),
    ("make", &["mk", "mak"]),
    ("gitignore", &["gitignore"]),
    ("ini", &["ini", "cfg"]),
    ("dotenv", &["env"]),
    ("just", &["just"]),
    ("diff", &["diff", "patch"]),
    ("json5", &["json5"]),
    ("csv", &["csv"]),
    ("tsv", &["tsv", "tab"]),
];

#[test]
fn every_syntax_language_has_an_icon() {
    let default = path(DEFAULT_FILE);
    for &(language, extensions) in SYNTAX_EXTENSIONS {
        for ext in extensions {
            let icon = dark_file(&format!("a.{ext}"));
            assert_ne!(icon, default, "{language}: .{ext}");
        }
    }
    // The languages with their own logo get it, not a shared one.
    let mut logos = BTreeSet::new();
    for name in ["a.rs", "a.py", "a.go", "a.js", "a.ts", "a.java", "a.rb"] {
        assert!(logos.insert(dark_file(name)), "{name}");
    }
}

#[test]
fn folders_by_name_open_and_closed() {
    let set = ColouredIcons;
    for expanded in [false, true] {
        let plain = dark_folder("misc", expanded);
        assert_eq!(plain, dark_folder("", expanded));
        assert_eq!(set.folder(expanded), set.folder_named("", expanded));
        for name in ["src", "node_modules"] {
            assert_ne!(dark_folder(name, expanded), plain, "{name} {expanded}");
        }
        assert_ne!(
            dark_folder("src", expanded),
            dark_folder("node_modules", expanded)
        );
    }

    // Each folder icon, the plain one too, opens.
    let stems: BTreeSet<&str> = FOLDER_NAMES.iter().map(|&(_, stem)| stem).collect();
    for stem in stems.into_iter().chain([DEFAULT_FOLDER]) {
        let (closed, open) = (path(stem), path(&format!("{stem}{OPEN}")));
        assert_ne!(closed, open, "{stem}");
    }
    for &(name, stem) in FOLDER_NAMES {
        assert_eq!(dark_folder(name, false), path(stem), "{name}");
        assert_eq!(
            dark_folder(name, true),
            path(&format!("{stem}{OPEN}")),
            "{name}"
        );
    }

    // A leading dot or underscores fall away when the name isn't listed.
    assert_eq!(dark_folder(".git", false), dark_folder("git", false));
    assert_eq!(dark_folder(".github", true), dark_folder("github", true));
    assert_eq!(dark_folder("__tests__", false), dark_folder("tests", false));
    assert_eq!(dark_folder("...", false), dark_folder("", false));
}

#[test]
fn lookup_is_case_insensitive() {
    for (upper, lower) in [
        ("MAKEFILE", "makefile"),
        ("CARGO.TOML", "Cargo.toml"),
        ("Main.RS", "main.rs"),
        ("ReadMe.md", "README.md"),
        ("A.TAR.GZ", "a.tar.gz"),
        ("index.D.TS", "index.d.ts"),
    ] {
        assert_eq!(dark_file(upper), dark_file(lower), "{upper}");
    }
    assert_ne!(dark_file("index.d.ts"), dark_file("index.ts"));
    for (upper, lower) in [
        ("SRC", "src"),
        ("Node_Modules", "node_modules"),
        (".GIT", ".git"),
    ] {
        assert_eq!(
            dark_folder(upper, true),
            dark_folder(lower, true),
            "{upper}"
        );
    }
    assert_eq!(dark_file(".env.local"), path(ENV_FILE));
    assert_eq!(dark_file(".ENV"), path(ENV_FILE));
}

#[test]
fn light_variant_under_a_light_theme() {
    let light = |name| ColouredIcons::file_path(name, Appearance::Light);
    assert_eq!(dark_file("a.toml"), path("toml"));
    assert_eq!(light("a.toml"), path(&format!("toml{LIGHT}")));
    assert_eq!(light("pnpm-lock.yaml"), path(&format!("pnpm{LIGHT}")));
    // Only some have one; the rest are the same in both.
    for name in ["a.rs", "Makefile", "unknown"] {
        assert_eq!(light(name), dark_file(name), "{name}");
    }
    assert_eq!(
        ColouredIcons::folder_path("src", true, Appearance::Light),
        dark_folder("src", true)
    );
}

/// The set reads the active theme as it answers.
#[gpui::test]
fn follows_the_active_theme(cx: &mut TestAppContext) {
    let set = ColouredIcons;
    let toml = |cx: &mut TestAppContext, theme: Theme| {
        cx.update(|cx| set_theme(cx, theme));
        set.file("Taplo.toml")
    };
    let dark = IconRef::Coloured(path("toml").into());
    let light = IconRef::Coloured(path(&format!("toml{LIGHT}")).into());
    assert_eq!(set.file("a.toml"), dark, "dark until a theme is set");
    assert_eq!(toml(cx, Theme::light()), light);
    assert!(set.file("a.toml").is_coloured());
    assert_eq!(toml(cx, Theme::dark()), dark);
}

#[gpui::test]
fn installed_set_names_folders(cx: &mut TestAppContext) {
    // Lucide's folder is the same whatever it's called.
    assert_eq!(
        LucideIcons.folder_named("src", true),
        LucideIcons.folder(true)
    );
    cx.update(|cx| set_icon_set(Arc::new(ColouredIcons), cx));
    assert_eq!(
        folder_icon_ref_named("src", false),
        IconRef::Coloured(path("folder-src").into())
    );
    assert_eq!(
        folder_icon_ref_named("photos", true),
        IconRef::Coloured(path("folder-images-open").into())
    );
}
