//! INI-style files, dotenv files and justfiles are known by name, extension
//! or, for `.env.local` and the like, how their name starts.

use std::path::Path;

use fe_syntax::language_name_for_path;

fn language(path: &str) -> Option<&'static str> {
    language_name_for_path(Path::new(path))
}

#[cfg(feature = "ini")]
#[test]
fn ini_style_files_are_ini() {
    for name in [
        "x.ini",
        "X.INI",
        "setup.cfg",
        "tox.ini",
        ".editorconfig",
        ".npmrc",
        ".pypirc",
        ".flake8",
        ".coveragerc",
        ".pylintrc",
        "pylintrc",
        "repo/sub/.editorconfig",
    ] {
        assert_eq!(language(name), Some("ini"), "{name}");
    }
}

#[cfg(feature = "bash")]
#[test]
fn env_files_are_dotenv() {
    for name in [
        ".env",
        ".env.local",
        ".env.production",
        ".env.example",
        ".ENV.LOCAL",
        "prod.env",
        "repo/sub/.env.test",
    ] {
        assert_eq!(language(name), Some("dotenv"), "{name}");
    }
}

/// `.env.` only names a file when nothing else does.
#[cfg(feature = "bash")]
#[test]
fn shell_files_named_like_env_files_stay_bash() {
    for name in [".envrc", ".env.sh", "repo/.env.bash"] {
        assert_eq!(language(name), Some("bash"), "{name}");
    }
}

/// The prefix is the start of the name, dot included.
#[test]
fn names_only_like_the_env_prefix_are_not_dotenv() {
    for name in ["env", "env.local", "x.env.local", ".environment", ".envy"] {
        assert_eq!(language(name), None, "{name}");
    }
}

/// With bash left out, the prefix finds no language rather than a stale name.
#[cfg(not(feature = "bash"))]
#[test]
fn a_prefix_of_a_language_left_out_is_skipped() {
    assert_eq!(language(".env.local"), None);
}

#[cfg(feature = "just")]
#[test]
fn justfiles_are_just() {
    for name in [
        "justfile",
        "Justfile",
        ".justfile",
        "x.just",
        "a/b/justfile",
    ] {
        assert_eq!(language(name), Some("just"), "{name}");
    }
    assert_eq!(
        fe_syntax::detect_language_name(None, "#!/usr/bin/env -S just --justfile\nbuild:\n"),
        Some("just")
    );
}
