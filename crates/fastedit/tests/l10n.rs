//! fastedit's catalogue: every key its source looks up is in its en-US
//! resources, and no user-facing literal is left in its source. Also that
//! `--help` prints today's text, with no isolation marks.

// The app's own catalogue: its binary's modules aren't a library to link.
#[path = "../src/l10n.rs"]
mod l10n;

#[test]
fn keys_used_here_exist() {
    kiln_l10n::lint::keys(env!("CARGO_MANIFEST_DIR"), &l10n::RESOURCES).check();
}

#[test]
fn ui_text_is_in_the_catalogue() {
    kiln_l10n::lint::literals(env!("CARGO_MANIFEST_DIR"))
        .allow_list("l10n-allow.toml")
        .check();
}

/// `--help` as it was before the text moved into the catalogue.
const HELP: &str = "\
Tenmoku, a code editor.

Usage: tenmoku [options] [--] [path[:line[:column]] ...]

Each path opens as a tab, in order, the last one active; a path that doesn't
exist is a new file, which saving creates. The first folder is the project
root, or with no folder, the first file's folder. Lines and columns count
from 1. A file really named a:1 opens as itself.

Options:
      --new-window  Open in a new window on the path's folder
      --wait        Wait until the files' tabs have closed, then exit
      --perf        Measure startup and scrolling on the file, print, and quit
  -h, --help        Print this help
  -V, --version     Print the version
      --            Treat everything after it as a path
";

fn tenmoku(args: &[&str]) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_tenmoku"))
        .args(args)
        .env_remove("LANG")
        .env("LC_ALL", "en_US.UTF-8")
        .output()
        .unwrap()
}

#[test]
fn help_is_todays_text_without_isolation_marks() {
    let out = tenmoku(&["--help"]);
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(stdout, HELP);
    assert!(!stdout.contains(['\u{2068}', '\u{2069}']));
}

#[test]
fn an_unknown_flag_prints_the_error_then_the_usage() {
    let out = tenmoku(&["--bogus"]);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert_eq!(stderr, format!("tenmoku: unknown flag --bogus\n\n{HELP}"));
}

#[test]
fn the_pseudo_locale_reaches_help_and_the_unknown_flag_error_whole() {
    let pseudo = |args: &[&str]| {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_tenmoku"))
            .args(args)
            .env("TENMOKU_PSEUDO_LOCALE", "1")
            .output()
            .unwrap();
        String::from_utf8(if out.status.success() {
            out.stdout
        } else {
            out.stderr
        })
        .unwrap()
    };
    let help = pseudo(&["--help"]);
    assert_ne!(help, HELP, "the selected locale must apply");
    assert!(!help.contains(['\u{2068}', '\u{2069}']));
    let error = pseudo(&["--bogus"]);
    // The prefix and the flag stay as they are; the rest is the locale's.
    assert!(error.starts_with("tenmoku: "), "{error}");
    assert!(error.contains("--bogus"), "{error}");
    assert!(error.ends_with(&help), "{error}");
    assert!(!error.contains("unknown flag"), "{error}");
}
