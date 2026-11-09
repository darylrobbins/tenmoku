//! Gradle and Maven files are known by name or extension: the wrapper scripts
//! are bash, the build files belong to the first-party Groovy, Kotlin and
//! properties grammars. `gradlew.bat` and `mvnw.cmd` stay plain text: there is
//! no batch grammar.

use std::fs;
use std::path::Path;

use fe_syntax::language_name_for_path;

#[allow(dead_code)] // every caller is feature-gated
fn language(path: &str) -> Option<&'static str> {
    language_name_for_path(Path::new(path))
}

#[cfg(feature = "bash")]
#[test]
fn gradle_wrapper_scripts_are_bash() {
    for name in ["gradlew", "mvnw", "sub/gradlew", "GRADLEW"] {
        assert_eq!(language(name), Some("bash"), "{name}");
    }
    assert_eq!(language("gradlew.bat"), None);
}

#[test]
fn maven_and_version_catalog_files_are_built_in() {
    #[cfg(feature = "xml")]
    assert_eq!(language("pom.xml"), Some("xml"));
    #[cfg(feature = "toml")]
    assert_eq!(language("gradle/libs.versions.toml"), Some("toml"));
    #[cfg(feature = "java")]
    assert_eq!(language("Main.java"), Some("java"));
}

/// The extensions and file names a first-party grammar's manifest claims.
fn claims(dir: &str) -> (Vec<String>, Vec<String>) {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../extensions/grammars")
        .join(dir)
        .join("extension.toml");
    let text = fs::read_to_string(&manifest).unwrap_or_else(|e| panic!("{manifest:?}: {e}"));
    let list = |key: &str| -> Vec<String> {
        let Some(value) = text.lines().find_map(|line| {
            let (name, value) = line.split_once('=')?;
            (name.trim() == key).then(|| value.trim().to_owned())
        }) else {
            return Vec::new();
        };
        value
            .trim_start_matches('[')
            .trim_end_matches(']')
            .split(',')
            .map(|item| item.trim().trim_matches('"').to_owned())
            .filter(|item| !item.is_empty())
            .collect()
    };
    (list("extensions"), list("file_names"))
}

#[test]
fn gradle_files_are_claimed_by_the_first_party_grammars() {
    let dirs = ["groovy", "kotlin", "properties"];
    let all: Vec<_> = dirs.iter().map(|d| (*d, claims(d))).collect();

    // The last extension, or the whole name, as the lookup compares them.
    let owner = |path: &str| -> Vec<&str> {
        let name = path.rsplit('/').next().unwrap();
        let ext = name.rsplit_once('.').map(|(_, e)| e);
        all.iter()
            .filter(|(_, (exts, names))| {
                names.iter().any(|n| n.eq_ignore_ascii_case(name))
                    || ext.is_some_and(|e| exts.iter().any(|x| x.eq_ignore_ascii_case(e)))
            })
            .map(|(d, _)| *d)
            .collect()
    };
    for (path, grammar) in [
        ("build.gradle", "groovy"),
        ("settings.gradle", "groovy"),
        ("build.gradle.kts", "kotlin"),
        ("settings.gradle.kts", "kotlin"),
        ("gradle.properties", "properties"),
        ("application.properties", "properties"),
    ] {
        assert_eq!(owner(path), [grammar], "{path}");
    }

    for (i, (a, (a_exts, a_names))) in all.iter().enumerate() {
        for (b, (b_exts, b_names)) in &all[i + 1..] {
            let ext = a_exts.iter().find(|e| b_exts.contains(e));
            assert!(ext.is_none(), "{a} and {b} both claim extension {ext:?}");
            let name = a_names.iter().find(|n| b_names.contains(n));
            assert!(name.is_none(), "{a} and {b} both claim file name {name:?}");
        }
    }
}
