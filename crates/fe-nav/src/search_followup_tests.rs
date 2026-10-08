//! Follow-ups to T105's review: bindings named `from`, several statements
//! on a line, and cancels during the steps after the searches.

use super::{tests::imports, *};

#[test]
fn a_binding_named_from_isnt_the_keyword() {
    let cases: &[(&str, &[&str])] = &[
        ("import from from './util'", &["./util"]),
        ("import * as from from './util'", &["./util"]),
        ("export * as from from './util'", &["./util"]),
        ("import from, { a } from './util'", &["./util"]),
        ("import from, * as b from './util'", &["./util"]),
        ("import type from from './util'", &["./util"]),
        ("import { from } from './util'", &["./util"]),
        ("import { a as from } from './util'", &["./util"]),
        ("export { from } from './util'", &["./util"]),
        ("import\n  from\nfrom './util'", &["./util"]),
        // `type` alone is a binding too, and modifiers come before one.
        ("import type from './util'", &["./util"]),
        ("import type, { a } from './util'", &["./util"]),
        ("import type * as t from './util'", &["./util"]),
        ("import typeof T from './util'", &["./util"]),
        ("import defer * as d from './util'", &["./util"]),
        // Not a module clause.
        ("import X = require('./x')", &["./x"]),
        ("import from = x", &[]),
        ("export * as from", &[]),
    ];
    for (source, modules) in cases {
        assert_eq!(&imports("typescript", source), modules, "{source:?}");
    }
}

#[test]
fn statements_on_one_line_are_all_read() {
    let cases: &[(&str, &[&str])] = &[
        (
            "import a from './a'; import { helper } from './util';",
            &["./a", "./util"],
        ),
        ("import a from './a' import b from './b'", &["./a", "./b"]),
        (
            "export * from './a'; export { b } from './b'; export * as c from './c'",
            &["./a", "./b", "./c"],
        ),
        (
            "import a from './a'; const b = require('./b'); import c from './c'",
            &["./a", "./b", "./c"],
        ),
        (
            "const b = require('./b'); import a from './a'; require('./c')",
            &["./b", "./a", "./c"],
        ),
        ("export { x }; import y from './y'", &["./y"]),
        (
            "import {\n  a,\n} from './a'; import b from './b'",
            &["./a", "./b"],
        ),
        ("import a from './a'; // import b from './b'", &["./a"]),
        ("import a from './a'; /* import b from './b' */", &["./a"]),
        ("const s = \"; import x from './x'\"", &[]),
        ("f(); import x from './x'", &["./x"]),
        // (Invalid code: the grammar's recovery reports the module.)
        ("f() import x from './x'", &["./x"]),
        ("function f() {} import a from './a'", &["./a"]),
        ("export function f() {} export * from './b'", &["./b"]),
        // An unclosed list isn't one, and the scan goes on after it.
        ("export { a; import b from './b'", &["./b"]),
    ];
    for (source, modules) in cases {
        assert_eq!(&imports("javascript", source), modules, "{source:?}");
    }
    let lines = "import a from './a'; import b from './b'\nimport c from './c'; x();\n";
    assert_eq!(imports("tsx", lines), ["./a", "./b", "./c"]);
}

#[test]
fn a_long_line_of_statements_takes_linear_time() {
    // A minified barrel file, and statements that start a list but don't
    // end it (`/;export {/` is a regular expression).
    for piece in [
        "export*from\"./x\";",
        "export{a}from\"./x\";",
        "a=/;export {/;",
    ] {
        let line = piece.repeat((1 << 20) / piece.len());
        let started = Instant::now();
        let found = imports("javascript", &line);
        assert!(found.len() <= line.len() / piece.len());
        let took = started.elapsed();
        // Quadratic, this took seconds even optimized.
        assert!(took < Duration::from_secs(10), "{piece}: {took:?}");
    }
}

#[test]
fn a_cancelled_import_scan_stops() {
    let cancelled = AtomicBool::new(true);
    let js = "import a from './a'\n".repeat(10 * CANCEL_EVERY);
    let rust = "use crate::a;\n".repeat(10 * CANCEL_EVERY);
    for (language, source) in [("javascript", &js), ("rust", &rust)] {
        assert_eq!(
            import_lines(language, source, &cancelled),
            Err(NavError::Cancelled),
            "{language}"
        );
        let all = import_lines(language, source, &AtomicBool::new(false)).unwrap();
        assert_eq!(all.len(), 10 * CANCEL_EVERY, "{language}");
    }
}

/// A navigation over an empty project, which cancels its request when it
/// reaches `at`.
fn cancelling_at(at: Phase, root: &Path) -> (SearchNavigation, Arc<AtomicBool>) {
    let cancel = Arc::new(AtomicBool::new(false));
    let index = Arc::new(RwLock::new(SymbolIndex::new(root)));
    let nav = SearchNavigation::new(root, index)
        .with_budget(Duration::from_secs(60))
        .with_on_phase({
            let cancel = cancel.clone();
            move |phase| {
                if phase == at {
                    cancel.store(true, Ordering::Relaxed);
                }
            }
        });
    (nav, cancel)
}

#[test]
fn a_cancel_while_reading_imports_or_ranking_is_answered() {
    let dir = tempfile::tempdir().unwrap();
    let req = NavRequest {
        root: dir.path().to_path_buf(),
        rel: PathBuf::from("main.ts"),
        language: Some("typescript"),
        text: Text::new("import { other } from './other'\nother();\n"),
        offset: "import { other } from './other'\n".len(),
    };
    for at in [Phase::Imports, Phase::Rank] {
        let (nav, cancel) = cancelling_at(at, dir.path());
        assert_eq!(
            nav.definitions(&req, &cancel),
            Err(NavError::Cancelled),
            "{at:?}"
        );
        assert!(cancel.load(Ordering::Relaxed), "{at:?} reached");
    }
    // Not cancelled, it answers.
    let (nav, cancel) = cancelling_at(Phase::Fuzzy, dir.path());
    let found = nav.definitions(&req, &cancel).unwrap();
    assert_eq!(found.word.as_deref(), Some("other"));
}

#[test]
fn a_cancel_while_matching_symbols_is_answered() {
    let dir = tempfile::tempdir().unwrap();
    let (nav, cancel) = cancelling_at(Phase::Fuzzy, dir.path());
    assert_eq!(
        nav.workspace_symbols("oth", 10, &cancel),
        Err(NavError::Cancelled)
    );
    assert!(cancel.load(Ordering::Relaxed));
    let (nav, cancel) = cancelling_at(Phase::Imports, dir.path());
    assert_eq!(nav.workspace_symbols("oth", 10, &cancel), Ok(Vec::new()));
}
