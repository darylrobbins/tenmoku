//! `SymbolIndex` and `build`: the project's definitions by name.

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use fe_nav::{
    BuildOptions, BuildSummary, FileSymbols, IndexStats, OutlineKind, Symbol, SymbolIndex, build,
};
use fe_project::FinderIndex;
use fe_syntax::{language_name_for_path, symbols_supported};

fn write(root: &Path, rel: &str, text: impl AsRef<[u8]>) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// Builds an index of `root`, extending it from the sink as a caller would.
fn built(root: &Path, threads: usize) -> (SymbolIndex, BuildSummary) {
    let mut index = SymbolIndex::new(root);
    let summary = build(
        root,
        &BuildOptions { threads },
        Arc::new(AtomicBool::new(false)),
        |batch| index.extend(batch),
    );
    index.finish();
    (index, summary)
}

/// Every definition of the given files, sorted.
fn dump(index: &SymbolIndex, rels: &BTreeSet<PathBuf>) -> Vec<String> {
    let mut lines: Vec<String> = rels
        .iter()
        .flat_map(|rel| index.file_symbols(rel).unwrap())
        .map(|s| {
            format!(
                "{} {:?} {} {:?} {:?} {} {:?}",
                s.rel.display(),
                s.kind,
                s.name,
                s.name_range,
                s.range,
                s.row,
                s.container
            )
        })
        .collect();
    lines.sort();
    lines
}

fn symbol(name: &str, kind: OutlineKind, start: usize) -> Symbol {
    Symbol {
        name: name.to_owned(),
        kind,
        name_range: start..start + name.len(),
        range: start..start + name.len() + 10,
        row: start as u32,
        container: None,
    }
}

fn file(rel: &str, symbols: Vec<Symbol>) -> FileSymbols {
    FileSymbols {
        rel: PathBuf::from(rel),
        language: "rust",
        len: 100,
        modified: None,
        symbols,
        truncated: false,
    }
}

fn functions(rel: &str, names: &[&str]) -> FileSymbols {
    let symbols = (names.iter().enumerate())
        .map(|(ix, name)| symbol(name, OutlineKind::Function, ix * 20))
        .collect();
    file(rel, symbols)
}

fn counts(index: &SymbolIndex) -> (usize, usize, usize) {
    let IndexStats {
        files,
        symbols,
        distinct_names,
        approx_bytes,
    } = index.stats();
    assert!(approx_bytes > 0);
    (files, symbols, distinct_names)
}

fn names_in<'a>(index: &'a SymbolIndex, name: &str) -> Vec<(&'a Path, usize)> {
    (index.definitions(name).into_iter())
        .map(|s| (s.rel, s.range.start))
        .collect()
}

#[test]
fn build_indexes_what_the_finder_lists() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, ".gitignore", "target/\n");
    write(root, "src/main.rs", "fn main() {}\nstruct Config;\n");
    write(root, "src/a/b/deep.rs", "pub fn deep() {}\n");
    write(root, "src/a/mod.rs", "pub mod b;\nconst LIMIT: u32 = 3;\n");
    write(root, "tools/gen.py", "def generate():\n    pass\n");
    write(root, "web/app.ts", "export class App {}\n");
    write(root, "README.md", "# Readme\n");
    write(root, "notes.txt", "fn not_code() {}\n");
    write(root, "bin.rs", b"fn binary() {}\0\n");
    write(root, "target/debug/gen.rs", "fn ignored_fn() {}\n");
    write(root, ".git/hooks/hook.rs", "fn git_fn() {}\n");

    let mut walked: BTreeSet<PathBuf> = BTreeSet::new();
    FinderIndex::walk(root, Arc::new(AtomicBool::new(false)), |batch| {
        walked.extend(batch)
    });
    let supported: BTreeSet<PathBuf> = (walked.iter())
        .filter(|rel| language_name_for_path(rel).is_some_and(symbols_supported))
        .cloned()
        .collect();
    let mut expected = supported.clone();
    assert!(expected.remove(Path::new("bin.rs")));

    let (index, summary) = built(root, 1);
    let indexed: BTreeSet<PathBuf> = (walked.iter())
        .filter(|rel| index.file_state(rel).is_some())
        .cloned()
        .collect();
    assert_eq!(indexed, expected);
    assert_eq!(index.progress(), (expected.len(), true));
    assert_eq!(
        names_in(&index, "deep"),
        [(Path::new("src/a/b/deep.rs"), 0)]
    );
    assert!(index.definitions("ignored_fn").is_empty());
    assert!(index.definitions("git_fn").is_empty());
    assert!(index.definitions("binary").is_empty());
    assert!(index.file_state("target/debug/gen.rs").is_none());

    assert_eq!(summary.files_seen, walked.len());
    assert_eq!(summary.files_indexed, expected.len());
    assert_eq!(summary.skipped.binary, 1);
    assert_eq!(summary.skipped.unsupported, walked.len() - supported.len());
    assert_eq!(summary.skipped.too_large, 0);
    assert_eq!(summary.skipped.unreadable, 0);
    assert_eq!(summary.skipped.parse_failed, 0);
    assert_eq!(
        summary.files_seen,
        summary.files_indexed + summary.skipped.total()
    );
    assert_eq!(summary.symbols, index.stats().symbols);
    assert!(!summary.cancelled);

    let (parallel, parallel_summary) = built(root, 0);
    assert_eq!(dump(&parallel, &expected), dump(&index, &expected));
    assert_eq!(
        BuildSummary {
            elapsed: summary.elapsed,
            ..parallel_summary
        },
        summary
    );
}

#[test]
fn definitions_are_exact_and_ordered() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "src/lib.rs",
        "fn helper() {}\nstruct Helper;\nfn helper_two() {}\nmod m {\n    fn helper() {}\n}\n",
    );
    write(
        root,
        "py/tools.py",
        "def helper():\n    pass\n\nclass Tools:\n    def helper(self):\n        pass\n",
    );
    write(
        root,
        "web/app.ts",
        "export function helper() {}\nconst Helper = 1;\n",
    );
    write(root, "go/main.go", "package main\n\nfunc helper() {}\n");
    let (mut index, _) = built(root, 0);
    // A container named helper, around a symbol that isn't.
    let mut inner = symbol("inner", OutlineKind::Function, 0);
    inner.container = Some("helper".to_owned());
    index.extend(vec![file("a/manual.rs", vec![inner])]);

    let found = index.definitions("helper");
    let rows: Vec<(&Path, u32, OutlineKind, Option<&str>)> = (found.iter())
        .map(|s| (s.rel, s.row, s.kind, s.container))
        .collect();
    assert_eq!(
        rows,
        [
            (Path::new("go/main.go"), 2, OutlineKind::Function, None),
            (Path::new("py/tools.py"), 0, OutlineKind::Function, None),
            (
                Path::new("py/tools.py"),
                4,
                OutlineKind::Method,
                Some("Tools")
            ),
            (Path::new("src/lib.rs"), 0, OutlineKind::Function, None),
            (Path::new("src/lib.rs"), 4, OutlineKind::Function, Some("m")),
            (Path::new("web/app.ts"), 0, OutlineKind::Function, None),
        ]
    );
    assert!(found.iter().all(|s| s.name == "helper"));
    assert_eq!(index.definitions("Helper").len(), 2);
    assert_eq!(index.definitions("helper_two").len(), 1);
    assert!(index.definitions("HELPER").is_empty());
    assert!(index.definitions("help").is_empty());
}

#[test]
fn extend_replaces_and_remove_under_is_exact() {
    fn shareable<T: Send + Sync>() {}
    shareable::<SymbolIndex>();
    let mut index = SymbolIndex::new("/project");
    assert_eq!(index.root(), Path::new("/project"));
    index.extend(vec![
        functions("src/a/x.rs", &["shared", "old_name"]),
        functions("src/a/b/y.rs", &["shared", "only_y"]),
        functions("src/ab.rs", &["shared", "only_ab"]),
    ]);
    assert_eq!(counts(&index), (3, 6, 4));
    assert_eq!(index.definitions("shared").len(), 3);

    index.extend(vec![functions("src/a/x.rs", &["shared", "new_name"])]);
    assert!(index.definitions("old_name").is_empty());
    assert_eq!(
        names_in(&index, "new_name"),
        [(Path::new("src/a/x.rs"), 20)]
    );
    assert_eq!(index.definitions("shared").len(), 3);
    assert_eq!(counts(&index), (3, 6, 4));

    assert!(index.remove("src/a/b/y.rs"));
    assert!(!index.remove("src/a/b/y.rs"));
    assert!(index.definitions("only_y").is_empty());
    assert_eq!(counts(&index), (2, 4, 3));

    index.extend(vec![functions("src/a/b/y.rs", &["only_y"])]);
    assert_eq!(counts(&index), (3, 5, 4));
    assert_eq!(index.remove_under("src/a"), 2);
    assert_eq!(names_in(&index, "shared"), [(Path::new("src/ab.rs"), 0)]);
    assert!(index.definitions("new_name").is_empty());
    assert!(index.definitions("only_y").is_empty());
    assert_eq!(names_in(&index, "only_ab"), [(Path::new("src/ab.rs"), 20)]);
    assert!(index.file_state("src/a/x.rs").is_none());
    assert_eq!(index.file_state("src/ab.rs"), Some((100, None)));
    assert_eq!(counts(&index), (1, 2, 2));

    assert_eq!(index.remove_under("src"), 1);
    assert_eq!(counts(&index), (0, 0, 0));
}

#[test]
fn fuzzy_ranks_workspace_symbols() {
    let mut index = SymbolIndex::new("/project");
    index.extend(vec![
        file(
            "src/lib.rs",
            vec![
                symbol("foo_bar", OutlineKind::Function, 0),
                symbol("Bar", OutlineKind::Struct, 20),
                symbol("fizzbuzz", OutlineKind::Function, 40),
            ],
        ),
        file(
            "src/types.rs",
            vec![symbol("FooBar", OutlineKind::Struct, 0)],
        ),
        // The same name as a fn too, in a shorter path: the struct still
        // comes first.
        file("a.rs", vec![symbol("FooBar", OutlineKind::Function, 0)]),
    ]);
    let found = index.fuzzy("fb", 10);
    let listed: Vec<(&str, OutlineKind, &Path, &[usize])> = (found.iter())
        .map(|m| {
            (
                m.symbol.name,
                m.symbol.kind,
                m.symbol.rel,
                m.indices.as_slice(),
            )
        })
        .collect();
    let mut names: Vec<&str> = listed.iter().map(|&(name, ..)| name).collect();
    names.dedup();
    names.sort_unstable();
    assert_eq!(names, ["FooBar", "fizzbuzz", "foo_bar"]);
    for pair in found.windows(2) {
        assert!(pair[0].score >= pair[1].score, "by score: {listed:?}");
        if pair[0].score == pair[1].score {
            let rank = |kind| (kind == OutlineKind::Function) as u8;
            assert!(rank(pair[0].symbol.kind) <= rank(pair[1].symbol.kind));
        }
    }
    let struct_ix = (listed.iter())
        .position(|&(name, kind, ..)| name == "FooBar" && kind == OutlineKind::Struct)
        .unwrap();
    assert_eq!(
        listed[struct_ix + 1],
        (
            "FooBar",
            OutlineKind::Function,
            Path::new("a.rs"),
            &[0, 3][..]
        )
    );
    assert_eq!(listed[struct_ix].3, [0, 3]);
    let indices = |name: &str| listed.iter().find(|l| l.0 == name).unwrap().3;
    assert_eq!(indices("foo_bar"), [0, 4]);
    assert_eq!(indices("fizzbuzz"), [0, 4]);
    assert_eq!(found.len(), 4);

    let capped = index.fuzzy("fb", 2);
    assert_eq!(capped, found[..2]);
    assert!(index.fuzzy("", 10).is_empty());
    assert!(index.fuzzy("fb", 0).is_empty());
    assert!(index.fuzzy("zzzzq", 10).is_empty());
}

#[test]
fn fuzzy_keeps_the_best_of_many_ties() {
    // A common name with many definitions, all scoring the same: ties go
    // by kind, then shorter path, then path, then start.
    let mut index = SymbolIndex::new("/project");
    let mut batch: Vec<FileSymbols> = (0..2_000)
        .map(|n| {
            let symbols = vec![
                symbol("new", OutlineKind::Function, 0),
                symbol("new", OutlineKind::Method, 40),
            ];
            file(&format!("src/m{n}/lib.rs"), symbols)
        })
        .collect();
    batch.push(file(
        "deep/er/path.rs",
        vec![symbol("new", OutlineKind::Struct, 7)],
    ));
    batch.push(file(
        "x.rs",
        vec![symbol("renew", OutlineKind::Function, 0)],
    ));
    index.extend(batch);

    let all = index.fuzzy("new", usize::MAX);
    assert_eq!(all.len(), 4_002);
    let found = index.fuzzy("new", 5);
    assert_eq!(found, all[..5]);
    let listed: Vec<(&Path, OutlineKind, usize)> = (found.iter())
        .map(|m| (m.symbol.rel, m.symbol.kind, m.symbol.range.start))
        .collect();
    assert_eq!(
        listed,
        [
            (Path::new("deep/er/path.rs"), OutlineKind::Struct, 7),
            (Path::new("src/m0/lib.rs"), OutlineKind::Function, 0),
            (Path::new("src/m0/lib.rs"), OutlineKind::Method, 40),
            (Path::new("src/m1/lib.rs"), OutlineKind::Function, 0),
            (Path::new("src/m1/lib.rs"), OutlineKind::Method, 40),
        ]
    );
    assert!(found.iter().all(|m| m.indices == [0, 1, 2]));
    let last = all.last().unwrap();
    assert_eq!(
        (last.symbol.name, last.indices.as_slice()),
        ("renew", &[2, 3, 4][..])
    );
}

/// `count` Rust files under `src/`, each defining `fn f<n>`.
fn many_files(root: &Path, count: usize) {
    for n in 0..count {
        write(
            root,
            &format!("src/f{n:04}.rs"),
            format!("fn f{n}() {{}}\n"),
        );
    }
}

#[test]
fn queries_see_files_indexed_so_far() {
    let dir = tempfile::tempdir().unwrap();
    many_files(dir.path(), 300);
    let mut index = SymbolIndex::new(dir.path());
    let mut first: Option<String> = None;
    let summary = build(
        dir.path(),
        &BuildOptions { threads: 1 },
        Arc::new(AtomicBool::new(false)),
        |batch| {
            let name = batch[0].symbols[0].name.clone();
            index.extend(batch);
            if first.is_none() {
                assert_eq!(index.definitions(&name).len(), 1);
                assert!(!index.is_complete());
                assert!(index.progress().0 > 0);
                first = Some(name);
            }
        },
    );
    assert_eq!(summary.files_indexed, 300);
    assert_eq!(index.definitions(first.as_deref().unwrap()).len(), 1);
    assert!(!index.is_complete());
    index.finish();
    assert!(index.is_complete());
    assert_eq!(index.progress(), (300, true));
}

#[test]
fn cancel_stops_the_build() {
    let dir = tempfile::tempdir().unwrap();
    many_files(dir.path(), 600);

    let cancel = Arc::new(AtomicBool::new(true));
    let mut calls = 0;
    let summary = build(dir.path(), &BuildOptions::default(), cancel, |_| calls += 1);
    assert_eq!(calls, 0);
    assert!(summary.cancelled);
    assert_eq!(summary.files_indexed, 0);

    let cancel = Arc::new(AtomicBool::new(false));
    let mut calls = 0;
    let summary = build(
        dir.path(),
        &BuildOptions { threads: 1 },
        cancel.clone(),
        |batch| {
            assert!(batch.len() <= 256);
            calls += 1;
            cancel.store(true, Ordering::Relaxed);
        },
    );
    assert_eq!(calls, 1);
    assert!(summary.cancelled);
    assert!(summary.files_indexed <= 256);

    for threads in [1, 0, 3] {
        let mut sizes = Vec::new();
        let summary = build(
            dir.path(),
            &BuildOptions { threads },
            Arc::new(AtomicBool::new(false)),
            |batch| sizes.push(batch.len()),
        );
        assert!(
            sizes.iter().all(|&size| size <= 256),
            "{threads}: {sizes:?}"
        );
        assert_eq!(sizes.iter().sum::<usize>(), 600);
        assert_eq!(summary.files_indexed, 600);
        assert!(!summary.cancelled);
    }
}
