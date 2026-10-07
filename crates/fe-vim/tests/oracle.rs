//! fe-vim against recorded Neovim. The gate needs no nvim: it compares with
//! `tests/corpus/*.expected.json`, which only the ignored recorder writes.

mod support;

use std::{path::Path, process::Command};

use support::{
    Case, Expected, NVIM, NvimResult, Recorded, check, load, parse_corpus, parse_expected,
};

/// Checks every case of `tests/corpus/<area>.toml`, at least `min` of them.
fn matches_neovim(area: &str, min: usize) {
    let recorded = load(area).unwrap();
    assert!(recorded.len() >= min, "{area}: {} cases", recorded.len());
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "{area}: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}

#[test]
fn motions_match_neovim() {
    matches_neovim("motions", 60);
}

#[test]
fn operators_match_neovim() {
    matches_neovim("operators", 20);
}

#[test]
fn text_objects_match_neovim() {
    matches_neovim("text_objects", 20);
}

#[test]
fn registers_match_neovim() {
    matches_neovim("registers", 10);
}

#[test]
fn insert_matches_neovim() {
    matches_neovim("insert", 20);
}

#[test]
fn edits_match_neovim() {
    matches_neovim("edits", 10);
}

#[test]
fn repeat_matches_neovim() {
    matches_neovim("repeat", 10);
}

/// docs/decisions/vim-engine.md §6's spot check: rows 1-21, 24 and 25 of its
/// table, on its buffer from (0, 4).
#[test]
fn spot_check_matches_neovim() {
    let rows: Vec<String> = (1..=21)
        .chain([24, 25])
        .map(|row| format!("row_{row:02}"))
        .collect();
    let recorded = load("spot_check").unwrap();
    let names: Vec<&str> = recorded.iter().map(|case| case.name.as_str()).collect();
    assert_eq!(names, rows);
    for case in &recorded {
        assert_eq!(
            (case.buffer.as_str(), case.cursor),
            (
                "fn foo(a, b) {\n    let x = foo(a);\n    x + b\n}\nfoo end",
                (0, 4)
            ),
            "{}",
            case.name
        );
    }
    matches_neovim("spot_check", rows.len());
}

/// The first fe-vim work's exit criterion: at least 150 cases, every one
/// recorded from nvim (`load` checks the version and that each case is in
/// its expected file).
#[test]
fn corpus_has_at_least_150_cases() {
    let mut areas: Vec<String> = std::fs::read_dir(support::corpus_dir())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
        .map(|path| path.file_stem().unwrap().to_string_lossy().into_owned())
        .collect();
    areas.sort();
    let cases: usize = areas.iter().map(|area| load(area).unwrap().len()).sum();
    assert!(cases >= 150, "{cases} cases in {areas:?}");
}

fn corpus(cases: &[(&str, &str, &str)]) -> String {
    cases
        .iter()
        .map(|(name, buffer, keys)| {
            format!(
                "[[case]]\nname = {name:?}\nbuffer = {buffer:?}\ncursor = [0, 0]\nkeys = {keys:?}\n"
            )
        })
        .collect()
}

fn expected(nvim: &str, cases: &[Case]) -> String {
    let cases = cases
        .iter()
        .map(|case| {
            Recorded::new(
                case.clone(),
                support::Outcome {
                    buffer: case.buffer.clone(),
                    cursor: case.cursor,
                    mode: "n".to_string(),
                    registers: Default::default(),
                    visual: None,
                    marks: Default::default(),
                },
            )
        })
        .collect();
    serde_json::to_string(&Expected {
        nvim: nvim.to_string(),
        cases,
    })
    .unwrap()
}

#[test]
fn corpus_loader_rejects_bad_cases() {
    let rejects = |cases: &[(&str, &str, &str)], why: &str| {
        let error = parse_corpus(&corpus(cases)).expect_err(why);
        assert!(error.contains(why), "{error:?} doesn't say {why:?}");
    };
    rejects(&[("nl", "abc\n", "l")], "ends in a newline");
    for keys in [
        "\"+yy",
        "2\"*p",
        "ff\"+p",
        "\"f\"*p",
        "@+",
        "i<C-r>*<Esc>",
        "if<C-r>+<Esc>",
        "it<C-r>*<Esc>",
        "i<C-r><C-r>+<Esc>",
        "i<C-r><C-o>*<Esc>",
        "qaq\"+yy",
        ":put +<CR>",
        "i<C-r>=getreg('*')<CR><Esc>",
        "f\"+",
    ] {
        rejects(&[("clipboard", "abc", keys)], "register + or *");
    }
    rejects(&[("dot", "abc", ".")], "start with `.`");
    rejects(&[("same", "abc", "l"), ("same", "xyz", "h")], "duplicate");
    // `+` and `*` with no key that could make them registers are fine, as
    // is `.` after the start.
    let good = parse_corpus(&corpus(&[("a", "a+b*", "f+t*"), ("b", "abc", "l.")])).unwrap();

    assert_eq!(
        parse_expected(&expected(NVIM, &good), &good).unwrap().len(),
        2
    );
    let missing = parse_expected(&expected(NVIM, &good[..1]), &good).unwrap_err();
    assert!(missing.contains("b: not recorded"), "{missing}");
    let twice = [good.clone(), good[1..].to_vec()].concat();
    let doubled = parse_expected(&expected(NVIM, &twice), &good).unwrap_err();
    assert!(doubled.contains("recorded twice"), "{doubled}");
    let extra = parse_expected(&expected(NVIM, &good), &good[..1]).unwrap_err();
    assert!(
        extra.contains("b: recorded but not in the corpus"),
        "{extra}"
    );
    let mut edited = good.clone();
    edited[1].keys = "h".to_string();
    let stale = parse_expected(&expected(NVIM, &good), &edited).unwrap_err();
    assert!(stale.contains("b: the corpus case differs"), "{stale}");
    let old = parse_expected(&expected("0.11.0", &good), &good).unwrap_err();
    assert!(old.contains("not 0.12.4"), "{old}");
}

/// Rewrites every `tests/corpus/*.expected.json` from nvim, one process per
/// corpus file:
/// `cargo nextest run -p fe-vim --run-ignored only -E 'test(=record_corpus_with_neovim)'`.
#[test]
#[ignore = "needs nvim 0.12.4; rewrites tests/corpus/*.expected.json"]
fn record_corpus_with_neovim() {
    check_nvim_version();

    let dir = support::corpus_dir();
    let driver = support::crate_dir().join("tests/support/oracle.lua");
    let mut corpora: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
        .collect();
    corpora.sort();
    assert!(!corpora.is_empty(), "no corpus files in {}", dir.display());
    let scratch = std::env::temp_dir().join(format!("fe-vim-oracle-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).unwrap();
    for corpus in corpora {
        let cases = parse_corpus(&std::fs::read_to_string(&corpus).unwrap())
            .unwrap_or_else(|error| panic!("{}: {error}", corpus.display()));
        let recorded = record(&driver, &scratch, &cases);
        let mut json = serde_json::to_string_pretty(&Expected {
            nvim: NVIM.to_string(),
            cases: recorded,
        })
        .unwrap();
        json.push('\n');
        std::fs::write(corpus.with_extension("expected.json"), json).unwrap();
    }
    std::fs::remove_dir_all(&scratch).ok();
}

/// Rewrites `src/chars.txt`, nvim's `charclass()` and `strdisplaywidth()`
/// for every code point, which word motions and goal columns read:
/// `cargo nextest run -p fe-vim --run-ignored only -E 'test(=record_char_table_with_neovim)'`.
#[test]
#[ignore = "needs nvim 0.12.4; rewrites src/chars.txt"]
fn record_char_table_with_neovim() {
    check_nvim_version();
    let dir = support::crate_dir();
    let status = Command::new("nvim")
        .args(["--clean", "--headless", "-l"])
        .arg(dir.join("tests/support/chars.lua"))
        .arg(dir.join("src/chars.txt"))
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("run nvim");
    assert!(status.success(), "nvim exited with {status}");
}

/// Rewrites `src/case.txt`, what nvim's `~` makes of every code point it
/// changes, which `vim.toggle_case` reads:
/// `cargo nextest run -p fe-vim --run-ignored only -E 'test(=record_case_table_with_neovim)'`.
#[test]
#[ignore = "needs nvim 0.12.4; rewrites src/case.txt"]
fn record_case_table_with_neovim() {
    check_nvim_version();
    let dir = support::crate_dir();
    let status = Command::new("nvim")
        .args(["--clean", "--headless", "-l"])
        .arg(dir.join("tests/support/case.lua"))
        .arg(dir.join("src/case.txt"))
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("run nvim");
    assert!(status.success(), "nvim exited with {status}");
}

fn check_nvim_version() {
    let version = Command::new("nvim")
        .arg("--version")
        .output()
        .expect("run nvim");
    let version = String::from_utf8_lossy(&version.stdout);
    let first = version.lines().next().unwrap_or_default();
    assert_eq!(
        first,
        format!("NVIM v{NVIM}"),
        "the oracle is pinned to nvim {NVIM}"
    );
}

/// Runs `cases` through one `nvim --clean --headless -l oracle.lua`.
fn record(driver: &Path, scratch: &Path, cases: &[Case]) -> Vec<Recorded> {
    let input = scratch.join("cases.json");
    let output = scratch.join("results.json");
    std::fs::write(&input, serde_json::to_string(cases).unwrap()).unwrap();
    std::fs::remove_file(&output).ok();
    let status = Command::new("nvim")
        .args(["--clean", "--headless", "-l"])
        .arg(driver)
        .arg(&input)
        .arg(&output)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("run nvim");
    assert!(status.success(), "nvim exited with {status}");
    let results: Vec<NvimResult> =
        serde_json::from_str(&std::fs::read_to_string(&output).unwrap()).unwrap();
    assert_eq!(results.len(), cases.len(), "nvim recorded every case");
    cases
        .iter()
        .zip(results)
        .map(|(case, result)| {
            assert_eq!(case.name, result.name, "nvim kept the cases' order");
            Recorded::new(case.clone(), result.outcome())
        })
        .collect()
}
