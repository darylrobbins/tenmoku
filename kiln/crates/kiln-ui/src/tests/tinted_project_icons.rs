// SPDX-License-Identifier: Apache-2.0

//! [`TintedIcons`] gives every well-known project file Material Icon Theme's
//! set knows a glyph other than the blank file, and every folder role its own
//! Lucide folder glyph.

use std::collections::{BTreeMap, BTreeSet};

use crate::{Icon, IconRef, IconSet, TintedIcons, file_icons_coloured, file_icons_tinted::*};

/// The project files by glyph and token, as the design's table has them:
/// manifests, a tool's own files, lock files, rc files, licences and
/// changelogs.
const PROJECT_FILES: &[(&[&str], Icon, &str)] = &[
    (&["Cargo.toml"], Icon::Package, RUST),
    (
        &[
            "go.mod",
            "go.work",
            "pom.xml",
            "build.gradle",
            "build.gradle.kts",
            "gradle.properties",
            "gradlew",
        ],
        Icon::Package,
        CODE,
    ),
    (
        &["pyproject.toml", "requirements.txt", "Pipfile", "Gemfile"],
        Icon::Package,
        SCRIPT,
    ),
    (&[".python-version"], Icon::BrandPython, SCRIPT),
    (
        &[
            "Rakefile",
            "Podfile",
            "Brewfile",
            "Guardfile",
            "Capfile",
            ".ruby-version",
        ],
        Icon::Gem,
        SCRIPT,
    ),
    (
        &["package.json", ".nvmrc", ".node-version"],
        Icon::BrandNodejs,
        WEB,
    ),
    (&[".npmrc", ".npmignore"], Icon::BrandNpm, CONFIG),
    (
        &["pnpm-workspace.yaml", ".pnpmfile.cjs"],
        Icon::BrandPnpm,
        CONFIG,
    ),
    (
        &[".yarnrc", ".yarnrc.yml", ".yarnrc.yaml"],
        Icon::BrandYarn,
        CONFIG,
    ),
    (&["bunfig.toml", ".bun-version"], Icon::Croissant, CONFIG),
    (&["deno.json", "deno.jsonc"], Icon::BrandDeno, CONFIG),
    (
        &[
            "Cargo.lock",
            "package-lock.json",
            "pnpm-lock.yaml",
            "yarn.lock",
            "bun.lock",
            "bun.lockb",
            "go.sum",
            "go.work.sum",
            "composer.lock",
        ],
        Icon::FileLock,
        DATA,
    ),
    (
        &[
            "docker-compose.yml",
            "docker-compose.yaml",
            "compose.yml",
            "compose.yaml",
            ".containerignore",
        ],
        Icon::BrandDocker,
        CONFIG,
    ),
    (
        &[
            ".gitkeep",
            ".git-blame-ignore-revs",
            "MERGE_MSG",
            "TAG_EDITMSG",
        ],
        Icon::BrandGit,
        CONFIG,
    ),
    (&["CODEOWNERS"], Icon::BrandGithub, CONFIG),
    (
        &[
            ".babelrc",
            ".bowerrc",
            ".eslintrc",
            ".hintrc",
            ".jscsrc",
            ".jsfmtrc",
            ".jshintrc",
            ".jslintrc",
            ".swcrc",
            ".vuerc",
            ".watchmanconfig",
        ],
        Icon::FileBraces,
        DATA,
    ),
    (
        &[".env", ".env.local", ".env.production"],
        Icon::FileKey,
        CONFIG,
    ),
    (
        &[
            "LICENSE",
            "LICENSE.md",
            "LICENSE.txt",
            "LICENSE-MIT",
            "LICENSE-APACHE",
            "LICENCE",
            "LICENCE.md",
            "LICENCE.txt",
            "COPYING",
            "COPYING.md",
            "COPYING.txt",
            "NOTICE",
            "NOTICE.md",
            "NOTICE.txt",
            "COPYRIGHT",
            "COPYRIGHT.md",
            "COPYRIGHT.txt",
        ],
        Icon::Scale,
        DEFAULT,
    ),
    (
        &[
            "CHANGELOG",
            "CHANGELOG.md",
            "CHANGELOG.txt",
            "CHANGES",
            "CHANGES.md",
            "CHANGES.txt",
        ],
        Icon::FileClock,
        MARKDOWN,
    ),
];

#[test]
fn material_file_names_are_never_blank() {
    let mut want = BTreeMap::new();
    for &(names, icon, tint) in PROJECT_FILES {
        for &name in names {
            let previous = want.insert(name.to_ascii_lowercase(), (name, icon, tint));
            assert!(previous.is_none(), "{name} twice");
        }
    }
    // Every name Material's set knows has a glyph, the design's where it
    // gives one.
    for &(name, _) in file_icons_coloured::FILE_NAMES {
        let icon = TintedIcons.file(name);
        let IconRef::Tinted(glyph, _) = icon else {
            panic!("{name}: not tinted");
        };
        assert_ne!(glyph, Icon::File, "{name}");
        if let Some(&(_, glyph, tint)) = want.get(name) {
            assert_eq!(icon, IconRef::Tinted(glyph, tint), "{name}");
        }
    }
    // And each of the design's, in the case a project spells it.
    for (_, (name, glyph, tint)) in want {
        assert_eq!(
            TintedIcons.file(name),
            IconRef::Tinted(glyph, tint),
            "{name}"
        );
    }
    // A README keeps the Markdown glyph.
    for name in ["README", "README.md", "readme.rst"] {
        assert_eq!(
            TintedIcons.file(name),
            IconRef::Tinted(Icon::Markdown, MARKDOWN),
            "{name}"
        );
    }
}

/// The design's folder roles with sample names, Material's spellings
/// (`.github`, `__tests__`) among them, and each role's glyph and token.
const ROLES: &[(FolderRole, &[&str], Icon, &str)] = &[
    (
        FolderRole::Source,
        &[
            "src", "source", "code", "lib", "libs", "crates", "packages", "pkg", "app",
        ],
        Icon::FolderCode,
        FOLDER_SOURCE,
    ),
    (
        FolderRole::Tests,
        &[
            "tests",
            "__tests__",
            "spec",
            "fixtures",
            "mocks",
            "testdata",
            "snapshots",
            "benches",
            "bench",
        ],
        Icon::FolderCheck,
        FOLDER_TESTS,
    ),
    (
        FolderRole::Docs,
        &["docs", "doc", "documentation", "wiki"],
        Icon::FolderPen,
        FOLDER_DOCS,
    ),
    (
        FolderRole::Config,
        &[
            "config", "cfg", "conf", "settings", ".vscode", ".idea", ".config",
        ],
        Icon::FolderCog,
        FOLDER,
    ),
    (
        FolderRole::Git,
        &[".git", ".githooks", "patches", "submodules"],
        Icon::FolderGit,
        FOLDER,
    ),
    (
        FolderRole::Forge,
        &[".github", ".gitlab"],
        Icon::FolderGit2,
        FOLDER,
    ),
    (
        FolderRole::Output,
        &["target", "dist", "out", "build", "bin", "release"],
        Icon::FolderOutput,
        FOLDER,
    ),
    (
        FolderRole::Dependencies,
        &["node_modules", "vendor", "third_party", "external"],
        Icon::FolderArchive,
        FOLDER,
    ),
    (
        FolderRole::Examples,
        &["examples", "demo", "samples"],
        Icon::FolderKanban,
        FOLDER,
    ),
    (
        FolderRole::Scripts,
        &["scripts", "xtask", "tools"],
        Icon::FolderInput,
        FOLDER,
    ),
    (
        FolderRole::Assets,
        &[
            "assets",
            "resources",
            "static",
            "public",
            "images",
            "icons",
            "fonts",
            "themes",
            "styles",
            "i18n",
            "locales",
        ],
        Icon::FolderBookmark,
        FOLDER,
    ),
    (
        FolderRole::Temporary,
        &["tmp", "temp", "cache"],
        Icon::FolderClock,
        FOLDER,
    ),
];

#[test]
fn folder_roles_have_their_own_glyph() {
    let plain = [Icon::Folder, Icon::FolderOpen];
    // Every role is the design's, with a glyph no other role has, and none
    // the plain folder's.
    let roles: BTreeSet<_> = ROLES
        .iter()
        .map(|&(role, ..)| format!("{role:?}"))
        .collect();
    let all: BTreeSet<_> = FolderRole::ALL
        .iter()
        .map(|role| format!("{role:?}"))
        .collect();
    assert_eq!(roles, all, "the design's roles");
    let mut owner = BTreeMap::new();
    for &(role, names, icon, tint) in ROLES {
        assert_eq!((role.icon(), role.tint()), (icon, tint), "{role:?}");
        assert!(!plain.contains(&icon), "{role:?}: {icon:?}");
        if let Some(other) = owner.insert(format!("{icon:?}"), role) {
            panic!("{role:?} and {other:?} share {icon:?}");
        }
        for &name in names {
            for expanded in [false, true] {
                assert_eq!(
                    TintedIcons.folder_named(name, expanded),
                    IconRef::Tinted(icon, tint),
                    "{role:?}: {name}"
                );
            }
        }
    }
    // Every folder name Material's set knows, as it is or as a project
    // hides it, has a role's glyph, the same open or closed.
    for &(name, _) in file_icons_coloured::FOLDER_NAMES {
        for spelled in [name.to_string(), format!(".{name}"), format!("_{name}_")] {
            let closed = TintedIcons.folder_named(&spelled, false);
            let IconRef::Tinted(icon, _) = closed else {
                panic!("{spelled}: not tinted");
            };
            assert!(!plain.contains(&icon), "{spelled}: {icon:?}");
            assert_eq!(
                TintedIcons.folder_named(&spelled, true),
                closed,
                "{spelled}"
            );
        }
    }
    // A name with no role is the plain folder, open or closed.
    for name in ["demo-app", "notes", "", "..", "__"] {
        for expanded in [false, true] {
            let want = if expanded {
                Icon::FolderOpen
            } else {
                Icon::Folder
            };
            assert_eq!(
                TintedIcons.folder_named(name, expanded),
                IconRef::Tinted(want, FOLDER),
                "{name}"
            );
        }
    }
}
