// SPDX-License-Identifier: Apache-2.0

//! [`TintedIcons`] gives a folder a module, solution, source-root, test-root,
//! resources or output role from its build files, its parent's and the names
//! above it ([`FolderContext`]), and its name's role where they say nothing.

use crate::{
    BuildFiles, FolderContext, Icon, IconRef, IconSet, LucideIcons, TintedIcons,
    file_icons_tinted::*,
};

/// A folder `parent/grandparent`-deep with `own` build files, beside `beside`.
fn at<'a>(
    parent: Option<&'a str>,
    grandparent: Option<&'a str>,
    own: BuildFiles,
    beside: BuildFiles,
) -> FolderContext<'a> {
    FolderContext {
        parent,
        grandparent,
        own,
        beside,
    }
}

fn role(role: FolderRole) -> IconRef {
    IconRef::Tinted(role.icon(), role.tint())
}

/// The plain folder, open or closed.
fn plain(expanded: bool) -> IconRef {
    TintedIcons.folder(expanded)
}

const NONE: BuildFiles = BuildFiles {
    gradle: false,
    gradle_settings: false,
    maven: false,
    dotnet: false,
    dotnet_solution: false,
    cargo: false,
    cargo_workspace: false,
    npm: false,
    go: false,
    python: false,
    main_or_test: false,
};
const GRADLE: BuildFiles = BuildFiles {
    gradle: true,
    ..NONE
};
const MAVEN: BuildFiles = BuildFiles {
    maven: true,
    ..NONE
};
const CARGO: BuildFiles = BuildFiles {
    cargo: true,
    ..NONE
};
const DOTNET: BuildFiles = BuildFiles {
    dotnet: true,
    ..NONE
};

#[test]
fn gradle_module_and_maven_roots_get_their_roles() {
    for expanded in [false, true] {
        // A Gradle submodule is a module, in the source tint, by whatever
        // name; a Gradle build with settings is a solution, in the folder's.
        let core = TintedIcons.folder_in("core", &at(None, None, GRADLE, NONE), expanded);
        assert_eq!(core, role(FolderRole::Module), "core");
        assert_eq!(core, IconRef::Tinted(Icon::FolderRoot, FOLDER_SOURCE));
        // A module named as a role is a module all the same.
        let docs = at(Some("libs"), None, MAVEN, NONE);
        assert_eq!(
            TintedIcons.folder_in("docs", &docs, expanded),
            role(FolderRole::Module)
        );
        for settings in [
            BuildFiles {
                gradle_settings: true,
                ..GRADLE
            },
            BuildFiles {
                gradle_settings: true,
                ..NONE
            },
        ] {
            assert_eq!(
                TintedIcons.folder_in("platform", &at(None, None, settings, NONE), expanded),
                IconRef::Tinted(Icon::FolderRoot, FOLDER),
                "{settings:?}"
            );
        }
        // Under a Gradle or Maven module, `src` holding `main` is their
        // container, a plain folder; the roots are below it.
        let src = BuildFiles {
            main_or_test: true,
            ..NONE
        };
        for beside in [GRADLE, MAVEN] {
            assert_eq!(
                TintedIcons.folder_in("src", &at(Some("core"), None, src, beside), expanded),
                plain(expanded),
                "{beside:?}"
            );
        }
        // `main` and `test` keep their names' roles.
        let below_src = at(Some("src"), Some("core"), NONE, NONE);
        assert_eq!(
            TintedIcons.folder_in("main", &below_src, expanded),
            plain(expanded)
        );
        assert_eq!(
            TintedIcons.folder_in("test", &below_src, expanded),
            role(FolderRole::Tests)
        );
        // Judged from the names above, not the disk.
        for (parent, name, want) in [
            ("main", "java", FolderRole::Source),
            ("main", "kotlin", FolderRole::Source),
            ("main", "scala", FolderRole::Source),
            ("main", "groovy", FolderRole::Source),
            ("main", "resources", FolderRole::Assets),
            ("test", "java", FolderRole::Tests),
            ("test", "Kotlin", FolderRole::Tests),
            ("test", "scala", FolderRole::Tests),
            ("test", "resources", FolderRole::Assets),
        ] {
            let context = at(Some(parent), Some("src"), NONE, NONE);
            assert_eq!(
                TintedIcons.folder_in(name, &context, expanded),
                role(want),
                "src/{parent}/{name}"
            );
        }
        // Elsewhere `java` and `kotlin` are plain folders.
        for (parent, grandparent) in [("main", "lib"), ("other", "src")] {
            let context = at(Some(parent), Some(grandparent), NONE, NONE);
            assert_eq!(
                TintedIcons.folder_in("java", &context, expanded),
                plain(expanded),
                "{grandparent}/{parent}/java"
            );
        }
        // Gradle's `build` is output; a module's own build files win.
        assert_eq!(
            TintedIcons.folder_in("build", &at(Some("core"), None, NONE, GRADLE), expanded),
            role(FolderRole::Output)
        );
        assert_eq!(
            TintedIcons.folder_in("build", &at(Some("core"), None, GRADLE, GRADLE), expanded),
            role(FolderRole::Module)
        );
    }
}

#[test]
fn dotnet_and_cargo_roots_get_their_roles() {
    for expanded in [false, true] {
        // A .NET project's folder is a module, a solution's a solution.
        assert_eq!(
            TintedIcons.folder_in("Api", &at(None, None, DOTNET, NONE), expanded),
            role(FolderRole::Module)
        );
        let sln = BuildFiles {
            dotnet_solution: true,
            ..NONE
        };
        assert_eq!(
            TintedIcons.folder_in("Shop", &at(None, None, sln, NONE), expanded),
            role(FolderRole::Solution)
        );
        // Its `bin` and `obj` are output; `obj` has no role by name.
        for name in ["bin", "obj", "OBJ"] {
            assert_eq!(
                TintedIcons.folder_in(name, &at(Some("Api"), None, NONE, DOTNET), expanded),
                role(FolderRole::Output),
                "{name}"
            );
        }
        assert_eq!(TintedIcons.folder_named("obj", expanded), plain(expanded));
        // A crate is a module, a workspace a solution.
        assert_eq!(
            TintedIcons.folder_in("parser", &at(Some("crates"), None, CARGO, NONE), expanded),
            role(FolderRole::Module)
        );
        let workspace = BuildFiles {
            cargo_workspace: true,
            ..CARGO
        };
        assert_eq!(
            TintedIcons.folder_in("kiln", &at(None, None, workspace, NONE), expanded),
            role(FolderRole::Solution)
        );
        // Beside a `Cargo.toml`: `src` is a source root even holding a
        // `test` module folder, `tests` and `benches` test roots, `target`
        // output.
        let src = BuildFiles {
            main_or_test: true,
            ..NONE
        };
        let in_crate = |own| at(Some("parser"), Some("crates"), own, CARGO);
        for own in [NONE, src] {
            assert_eq!(
                TintedIcons.folder_in("src", &in_crate(own), expanded),
                role(FolderRole::Source),
                "{own:?}"
            );
        }
        for (name, want) in [
            ("tests", FolderRole::Tests),
            ("benches", FolderRole::Tests),
            ("target", FolderRole::Output),
        ] {
            assert_eq!(
                TintedIcons.folder_in(name, &in_crate(NONE), expanded),
                role(want),
                "{name}"
            );
        }
        // Not beside their tool's file, `obj` is plain and `examples` keeps
        // its name's role.
        assert_eq!(
            TintedIcons.folder_in("obj", &in_crate(NONE), expanded),
            plain(expanded)
        );
        assert_eq!(
            TintedIcons.folder_in("examples", &in_crate(NONE), expanded),
            role(FolderRole::Examples)
        );
    }
}

#[test]
fn a_plain_src_keeps_its_name_role() {
    for expanded in [false, true] {
        // With nothing known, or no build files about, every folder has its
        // name's icon.
        for name in ["src", "tests", "build", "bin", "target", "java", "notes"] {
            for context in [
                FolderContext::default(),
                at(Some("main"), Some("lib"), NONE, NONE),
            ] {
                assert_eq!(
                    TintedIcons.folder_in(name, &context, expanded),
                    TintedIcons.folder_named(name, expanded),
                    "{name}: {context:?}"
                );
            }
        }
        assert_eq!(
            TintedIcons.folder_in("src", &FolderContext::default(), expanded),
            role(FolderRole::Source)
        );
        // A module's `src` holding no `main` or `test` folder is a source
        // root, as by name: npm, Go and Python layouts.
        for beside in [
            BuildFiles { npm: true, ..NONE },
            BuildFiles { go: true, ..NONE },
            BuildFiles {
                python: true,
                ..NONE
            },
            GRADLE,
        ] {
            assert_eq!(
                TintedIcons.folder_in("src", &at(Some("web"), None, NONE, beside), expanded),
                role(FolderRole::Source),
                "{beside:?}"
            );
        }
        // A set that doesn't look at the context gives its name's icon.
        let module = at(None, None, GRADLE, NONE);
        assert_eq!(
            LucideIcons.folder_in("core", &module, expanded),
            LucideIcons.folder_named("core", expanded)
        );
    }
}
