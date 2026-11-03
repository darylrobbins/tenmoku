use std::{ffi::OsString, fs, path::Path};

use super::{Cli, CliError, Plan, Target, parse_in};

fn parse(args: &[&str], cwd: &Path) -> Result<Cli, CliError> {
    let args = args.iter().map(OsString::from).collect();
    parse_in(args, cwd)
}

fn at(path: impl AsRef<Path>, line: Option<u32>, col: Option<u32>) -> Target {
    Target {
        path: path.as_ref().to_owned(),
        line,
        col,
    }
}

/// The one target `arg` makes.
fn one(arg: &str, cwd: &Path) -> Target {
    let mut targets = parse(&[arg], cwd).unwrap().targets;
    assert_eq!(targets.len(), 1);
    targets.remove(0)
}

#[test]
fn position_suffix_parses_line_and_column() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path();
    let a = cwd.join("a.rs");
    assert_eq!(one("a.rs", cwd), at(&a, None, None));
    assert_eq!(one("a.rs:12", cwd), at(&a, Some(12), None));
    assert_eq!(one("a.rs:12:5", cwd), at(&a, Some(12), Some(5)));

    // Too small or too big clamps; what isn't a number is part of the name.
    assert_eq!(one("a.rs:0", cwd), at(&a, Some(1), None));
    assert_eq!(one("a.rs:0:0", cwd), at(&a, Some(1), Some(1)));
    assert_eq!(one("a.rs:99999999999", cwd), at(&a, Some(u32::MAX), None));
    assert_eq!(one("a.rs:x", cwd), at(cwd.join("a.rs:x"), None, None));
    assert_eq!(one("a.rs:12:x", cwd), at(cwd.join("a.rs:12:x"), None, None));
    assert_eq!(one("a.rs:x:5", cwd), at(cwd.join("a.rs:x"), Some(5), None));
    assert_eq!(one("a.rs:+5", cwd), at(cwd.join("a.rs:+5"), None, None));
    assert_eq!(one("a.rs:", cwd), at(cwd.join("a.rs:"), None, None));
    assert_eq!(one(":12", cwd), at(cwd.join(":12"), None, None));

    // A real file wins over the suffix, and may have one of its own.
    fs::write(cwd.join("a:1"), "").unwrap();
    assert_eq!(one("a:1", cwd), at(cwd.join("a:1"), None, None));
    assert_eq!(one("a:1:2", cwd), at(cwd.join("a:1"), Some(2), None));
    assert_eq!(one("a:2", cwd), at(cwd.join("a"), Some(2), None));

    // Absolute and untidy paths too.
    let absolute = a.to_str().unwrap();
    assert_eq!(one(&format!("{absolute}:3"), cwd), at(&a, Some(3), None));
    assert_eq!(one("./a.rs:3", cwd), at(&a, Some(3), None));

    // A name that isn't UTF-8 still takes a position.
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt as _;
        let name = |suffix: &[u8]| OsString::from_vec([b"\xff.rs", suffix].concat());
        let targets = parse_in(vec![name(b":7:2")], cwd).unwrap().targets;
        assert_eq!(targets, [at(cwd.join(name(b"")), Some(7), Some(2))]);
    }
}

#[test]
fn several_paths_keep_order_and_folder_first_is_root() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path();
    fs::create_dir(cwd.join("first")).unwrap();
    fs::create_dir(cwd.join("second")).unwrap();
    let plan = |args: &[&str]| parse(args, cwd).unwrap().plan();

    // Files in argument order, around the folders, the first folder the root.
    assert_eq!(
        plan(&["b.rs:2", "second", "a.rs", "first:9", "c.rs:3:4"]),
        Plan {
            root: Some(cwd.join("second")),
            files: vec![
                at(cwd.join("b.rs"), Some(2), None),
                at(cwd.join("a.rs"), None, None),
                at(cwd.join("c.rs"), Some(3), Some(4)),
            ],
        }
    );

    // A folder alone, with or without a position it has no use for.
    let folder = Plan {
        root: Some(cwd.join("first")),
        files: Vec::new(),
    };
    assert_eq!(plan(&["first"]), folder);
    assert_eq!(plan(&["first:9"]), folder);

    // Only files: the first one's parent.
    assert_eq!(
        plan(&["first/x.rs", "second/y.rs"]),
        Plan {
            root: Some(cwd.join("first")),
            files: vec![
                at(cwd.join("first/x.rs"), None, None),
                at(cwd.join("second/y.rs"), None, None),
            ],
        }
    );

    // Nothing at all.
    assert_eq!(
        plan(&[]),
        Plan {
            root: None,
            files: Vec::new(),
        }
    );
}

#[test]
fn flags_help_version_double_dash_and_unknown_flag_exit_codes() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path();

    for args in [&["-h"][..], &["--help"], &["a.rs", "--help", "--bogus"]] {
        let error = parse(args, cwd).unwrap_err();
        assert_eq!(error, CliError::Help);
        assert_eq!(error.exit_code(), 0);
        assert!(error.text().contains("Usage: tenmoku"));
    }
    for args in [&["-V"][..], &["--version"], &["--version", "a.rs"]] {
        let error = parse(args, cwd).unwrap_err();
        assert_eq!(error, CliError::Version);
        assert_eq!(error.exit_code(), 0);
        let version = format!("tenmoku {}\n", env!("CARGO_PKG_VERSION"));
        assert_eq!(error.text(), version);
    }
    for (args, flag) in [
        (&["--bogus"][..], "--bogus"),
        (&["a.rs", "-x", "--bogus"], "-x"),
        (&["--perf=1"], "--perf=1"),
        (&["--bogus", "--", "a.rs"], "--bogus"),
    ] {
        let error = parse(args, cwd).unwrap_err();
        assert_eq!(error, CliError::UnknownFlag(flag.to_owned()));
        assert_eq!(error.exit_code(), 2);
        assert!(error.text().starts_with("tenmoku: unknown flag"));
        assert!(error.text().contains(flag) && error.text().contains("Usage: tenmoku"));
    }

    // The flags, anywhere and repeated, leave the paths in order.
    let cli = parse(
        &["--wait", "a.rs", "--perf", "--new-window", "b.rs", "--wait"],
        cwd,
    )
    .unwrap();
    assert!(cli.perf && cli.new_window && cli.wait);
    assert_eq!(
        cli.targets,
        [
            at(cwd.join("a.rs"), None, None),
            at(cwd.join("b.rs"), None, None)
        ]
    );
    let cli = parse(&["a.rs"], cwd).unwrap();
    assert!(!cli.perf && !cli.new_window && !cli.wait);

    // After `--` everything is a path, still taking a position, even `--`.
    let cli = parse(&["--perf", "a.rs", "--", "--help", "-x:3", "--"], cwd).unwrap();
    assert!(cli.perf);
    assert_eq!(
        cli.targets,
        [
            at(cwd.join("a.rs"), None, None),
            at(cwd.join("--help"), None, None),
            at(cwd.join("-x"), Some(3), None),
            at(cwd.join("--"), None, None),
        ]
    );

    // A lone `-` is a path, not a flag.
    let cli = parse(&["-"], cwd).unwrap();
    assert_eq!(cli.targets, [at(cwd.join("-"), None, None)]);
}
