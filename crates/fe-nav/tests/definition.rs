//! `SearchNavigation`: go-to-definition and workspace symbols by name.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use fe_buffer::Text;
use fe_nav::{
    BuildOptions, Definitions, NavRequest, Navigation, NavigationProvider, OutlineKind, Precision,
    SearchNavigation, SearchStats, SymbolIndex, Tier, build, extract,
};

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// Search over a complete index of `root`.
fn indexed(root: &Path) -> SearchNavigation {
    let mut index = SymbolIndex::new(root);
    build(
        root,
        &BuildOptions { threads: 1 },
        Arc::new(AtomicBool::new(false)),
        |batch| index.extend(batch),
    );
    index.finish();
    // A long budget, so a loaded machine can't leave work out.
    SearchNavigation::new(root, Arc::new(RwLock::new(index))).with_budget(Duration::from_secs(60))
}

/// Go-to-definition from `rel`, whose text is `text`, with the caret at
/// the `nth` occurrence of `at` plus `delta` bytes.
fn goto(
    nav: &SearchNavigation,
    root: &Path,
    (rel, language, text): (&str, &'static str, &str),
    at: &str,
    nth: usize,
    delta: usize,
) -> Definitions {
    let offset = text.match_indices(at).nth(nth).expect("caret text").0 + delta;
    let req = NavRequest {
        root: root.to_path_buf(),
        rel: PathBuf::from(rel),
        language: Some(language),
        text: Text::new(text),
        offset,
        buffer: None,
    };
    nav.definitions(&req, &AtomicBool::new(false)).unwrap()
}

/// Each item as `rel tier kind`.
fn listed(defs: &Definitions) -> Vec<String> {
    (defs.items.iter())
        .map(|def| {
            format!(
                "{} {:?} {:?}",
                def.location.rel.display(),
                def.tier,
                def.kind.expect("search says what each definition is")
            )
        })
        .collect()
}

const MAIN_RS: &str = "\
use crate::util;

fn parse() {}

fn main() {
    helper();
    parse();
    other();
}
";

fn rust_project(root: &Path) {
    write(root, "src/main.rs", MAIN_RS);
    write(root, "src/lexer.rs", "pub fn parse() {}\n");
    write(root, "src/util.rs", "pub fn helper() {}\n");
    write(root, "src/zzz/helper.rs", "pub fn helper() {}\n");
    write(root, "src/a.rs", "pub fn other() {}\n");
    write(root, "src/b.rs", "pub fn other() {}\n");
}

#[test]
fn same_file_beats_imports_beats_project() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    rust_project(root);
    let nav = indexed(root);
    let main = ("src/main.rs", "rust", MAIN_RS);

    let parse = goto(&nav, root, main, "parse()", 1, 1);
    assert_eq!(parse.word.as_deref(), Some("parse"));
    assert_eq!(
        listed(&parse),
        [
            "src/main.rs SameFile Function",
            "src/lexer.rs Project Function"
        ]
    );
    // Offsets and row into the request's text.
    let same = &parse.items[0].location;
    assert_eq!(&MAIN_RS[same.range.clone()], "parse");
    assert_eq!(
        (same.range.start, same.row),
        (MAIN_RS.find("parse").unwrap(), 2)
    );
    assert!(parse.unambiguous);
    assert!(!parse.partial);

    let helper = goto(&nav, root, main, "helper", 0, 0);
    assert_eq!(
        listed(&helper),
        [
            "src/util.rs Imported Function",
            "src/zzz/helper.rs Project Function"
        ]
    );
    assert_eq!(helper.items[0].location.range, 7..13);
    assert!(helper.unambiguous);

    let other = goto(&nav, root, main, "other", 0, 0);
    assert_eq!(
        listed(&other),
        ["src/a.rs Project Function", "src/b.rs Project Function"]
    );
    assert!(!other.unambiguous);
}

#[test]
fn kind_hint_orders_candidates() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "models.ts", "export class Item {}\n");
    write(root, "helpers.ts", "export function Item() {}\n");
    let main = "let x: Item;\nItem();\n";
    write(root, "main.ts", main);
    let nav = indexed(root);
    let main = ("main.ts", "typescript", main);

    let as_type = goto(&nav, root, main, "Item", 0, 1);
    assert_eq!(
        listed(&as_type),
        ["models.ts Project Class", "helpers.ts Project Function"]
    );
    assert!(!as_type.unambiguous);
    let as_call = goto(&nav, root, main, "Item", 1, 1);
    assert_eq!(
        listed(&as_call),
        ["helpers.ts Project Function", "models.ts Project Class"]
    );
}

#[test]
fn words_comments_and_definitions() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    rust_project(root);
    let text = "\
// call helper here
fn größe() {}
fn main() {
    let s = \"helper\";
    helper();
    größe();
}
";
    write(root, "src/z.rs", text);
    let nav = indexed(root);
    let z = ("src/z.rs", "rust", text);

    // Just after the word.
    let after = goto(&nav, root, z, "helper()", 0, "helper".len());
    assert_eq!(after.word.as_deref(), Some("helper"));
    assert_eq!(after.items[0].location.rel, Path::new("src/util.rs"));

    // A non-ASCII word, whole.
    let grosse = goto(&nav, root, z, "größe()", 1, "gr".len());
    assert_eq!(grosse.word.as_deref(), Some("größe"));
    assert_eq!(listed(&grosse), ["src/z.rs SameFile Function"]);

    // In a comment and in a string: the word, and nothing for it.
    for nth in [0, 1] {
        let quoted = goto(&nav, root, z, "helper", nth, 2);
        assert_eq!(quoted.word.as_deref(), Some("helper"));
        assert!(quoted.items.is_empty(), "{nth}: {:?}", quoted.items);
    }

    // On a definition's name: that one alone.
    let util = "pub fn helper() {}\n";
    let on_name = goto(&nav, root, ("src/util.rs", "rust", util), "helper", 0, 3);
    assert_eq!(listed(&on_name), ["src/util.rs SameFile Function"]);
    assert_eq!(on_name.items[0].location.range, 7..13);
    assert!(on_name.unambiguous);

    // `$` is part of a JavaScript word, and not of a Rust one.
    let js = "const $store = 1;\nconsole.log($store);\n";
    let store = goto(&nav, root, ("app.js", "javascript", js), "$store", 1, 3);
    assert_eq!(store.word.as_deref(), Some("$store"));
    assert_eq!(listed(&store), ["app.js SameFile Variable"]);
    let rust = goto(&nav, root, ("src/z.rs", "rust", "$store"), "store", 0, 0);
    assert_eq!(rust.word.as_deref(), Some("store"));

    // No word at the caret.
    let space = goto(&nav, root, z, "  helper", 0, 0);
    assert_eq!(space, Definitions::default());
}

#[test]
fn unsaved_text_is_used_for_the_same_file() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "src/main.rs", "fn moved() {}\n\nfn main() {}\n");
    write(root, "src/other.rs", "fn fresh() {}\n");
    let nav = indexed(root);

    // `fresh` is defined in the unsaved text only; on disk, `moved` is
    // elsewhere in the file.
    let text = "fn main() {\n    fresh();\n    moved();\n}\n\nfn fresh() {}\nfn moved() {}\n";
    let main = ("src/main.rs", "rust", text);
    let fresh = goto(&nav, root, main, "fresh", 0, 0);
    assert_eq!(
        listed(&fresh),
        [
            "src/main.rs SameFile Function",
            "src/other.rs Project Function"
        ]
    );
    let location = &fresh.items[0].location;
    assert_eq!(location.range.start, text.find("fn fresh").unwrap() + 3);
    assert_eq!(location.row, 5);

    let moved = goto(&nav, root, main, "moved", 0, 0);
    assert_eq!(listed(&moved), ["src/main.rs SameFile Function"]);
    assert_eq!(
        moved.items[0].location.range.start,
        text.find("fn moved").unwrap() + 3
    );
}

#[test]
fn ripgrep_fallback_before_the_index_is_complete() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    rust_project(root);
    write(root, "src/caller.rs", "fn call() { helper(); }\n");
    write(root, "notes.txt", "helper\n");
    write(root, "src/far.rs", "fn far() {}\n");
    let main = ("src/main.rs", "rust", MAIN_RS);
    // Search over `index`, never built, with a budget long enough that
    // counts don't race the clock.
    let unbuilt = |index: SymbolIndex| {
        SearchNavigation::new(root, Arc::new(RwLock::new(index)))
            .with_budget(Duration::from_secs(60))
    };

    // Nothing indexed yet: the files with the word are searched for and
    // extracted, the request's own file and those without symbols aside.
    let empty = unbuilt(SymbolIndex::new(root));
    let helper = goto(&empty, root, main, "helper", 0, 0);
    assert_eq!(
        listed(&helper),
        [
            "src/util.rs Imported Function",
            "src/zzz/helper.rs Project Function"
        ]
    );
    assert!(helper.partial);
    assert_eq!(
        empty.stats(),
        SearchStats {
            searches: 1,
            // util.rs, zzz/helper.rs and caller.rs.
            files_extracted: 3,
        }
    );
    // An answer that needs no search is partial too.
    let util = ("src/util.rs", "rust", "pub fn helper() {}\n");
    let on_name = goto(&empty, root, util, "helper", 0, 1);
    assert_eq!(listed(&on_name), ["src/util.rs SameFile Function"]);
    assert!(on_name.partial);

    // What the partial index has is merged, without duplicates.
    let mut partial = SymbolIndex::new(root);
    partial.extend(vec![extract(root, Path::new("src/util.rs")).unwrap()]);
    let partial = unbuilt(partial);
    let helper = goto(&partial, root, main, "helper", 0, 0);
    assert_eq!(
        listed(&helper),
        [
            "src/util.rs Imported Function",
            "src/zzz/helper.rs Project Function"
        ]
    );
    assert_eq!(partial.stats().files_extracted, 3);

    // Out of time before the fallback: no search runs.
    let late = SearchNavigation::new(root, Arc::new(RwLock::new(SymbolIndex::new(root))))
        .with_budget(Duration::ZERO);
    let helper = goto(&late, root, main, "helper", 0, 0);
    assert!(helper.items.is_empty());
    assert!(helper.partial);
    assert_eq!(
        late.stats(),
        SearchStats {
            searches: 0,
            files_extracted: 0,
        }
    );

    // At most 64 files are extracted.
    for ix in 0..80 {
        write(root, &format!("many/m{ix:02}.rs"), "fn m() { other(); }\n");
    }
    let empty = unbuilt(SymbolIndex::new(root));
    let other = goto(&empty, root, main, "other", 0, 0);
    assert!(other.partial);
    assert_eq!(
        empty.stats(),
        SearchStats {
            searches: 1,
            files_extracted: 64,
        }
    );
    // With the default 80 ms budget, no more.
    let timed = SearchNavigation::new(root, Arc::new(RwLock::new(SymbolIndex::new(root))));
    assert!(goto(&timed, root, main, "other", 0, 0).partial);
    assert!(timed.stats().files_extracted <= 64);

    // A complete index is authoritative: no search runs.
    let complete = indexed(root);
    let missing = goto(
        &complete,
        root,
        ("src/main.rs", "rust", "far2();"),
        "far2",
        0,
        0,
    );
    assert!(missing.items.is_empty());
    assert!(!missing.partial);
    assert_eq!(complete.stats(), SearchStats::default());
}

#[test]
fn imports_across_lines_and_in_unparsed_buffers() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for rel in ["util.ts", "zzz.ts", "w.ts"] {
        write(root, rel, "export function helper() {}\n");
    }
    let nav = indexed(root);

    // The module on the line after the imported names.
    let split = "import { helper }\n  from './util'\nhelper();\n";
    let helper = goto(&nav, root, ("main.ts", "typescript", split), "helper", 1, 0);
    assert_eq!(
        listed(&helper),
        [
            "util.ts Imported Function",
            "w.ts Project Function",
            "zzz.ts Project Function"
        ]
    );
    assert!(helper.unambiguous);
    let reexport = "export {\n  helper,\n}\nfrom './w'\nhelper();\n";
    let helper = goto(
        &nav,
        root,
        ("main.ts", "typescript", reexport),
        "helper",
        1,
        0,
    );
    assert_eq!(helper.items[0].location.rel, Path::new("w.ts"));
    assert_eq!(helper.items[0].tier, Tier::Imported);

    // A buffer over its language's parse limit still has imports.
    let limit = fe_syntax::language_by_name("typescript")
        .unwrap()
        .parse_limit();
    let mut large = String::from("import { helper } from './zzz'\nhelper();\n");
    while large.len() <= limit {
        large.push_str("// padding padding padding padding padding padding padding\n");
    }
    let helper = goto(
        &nav,
        root,
        ("main.ts", "typescript", &large),
        "helper",
        1,
        0,
    );
    assert_eq!(helper.items[0].location.rel, Path::new("zzz.ts"));
    assert_eq!(helper.items[0].tier, Tier::Imported);
    assert!(helper.unambiguous);
}

#[test]
fn workspace_symbols_from_the_index() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "src/util.rs", "pub fn helper() {}\n");
    let nav = Navigation::new();
    nav.add(Arc::new(indexed(root)), 0);
    let cancel = AtomicBool::new(false);

    let answer = nav.workspace_symbols("hlp", 10, &cancel).unwrap();
    assert_eq!(answer.provider, SearchNavigation::NAME);
    assert_eq!(answer.precision, Precision::Search);
    let [found] = &answer.value[..] else {
        panic!("{:?}", answer.value);
    };
    assert_eq!(found.name, "helper");
    assert_eq!(found.kind, OutlineKind::Function);
    assert_eq!(found.location.rel, Path::new("src/util.rs"));
    assert_eq!(
        (found.location.range.clone(), found.location.row),
        (7..13, 0)
    );
    assert_eq!(found.indices, [0, 2, 3]);

    // Empty while nothing is indexed.
    let empty = SearchNavigation::new(root, Arc::new(RwLock::new(SymbolIndex::new(root))));
    assert!(
        empty
            .workspace_symbols("hlp", 10, &cancel)
            .unwrap()
            .is_empty()
    );
    cancel.store(true, Ordering::Relaxed);
    assert!(empty.workspace_symbols("hlp", 10, &cancel).is_err());
}
