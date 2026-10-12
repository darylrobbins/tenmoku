//! `ScipNavigation`: go-to-definition, references and workspace symbols
//! answered from a SCIP index, each site checked against its file's text.
//! The indexes are built in code over files written to a tempdir.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use fe_buffer::{Buffer, Text};
use fe_nav::{
    BuildOptions, FileReferences, Location, NavError, NavRequest, Navigation, NavigationProvider,
    OutlineKind, Precision, Readiness, ScipIndex, ScipNavigation, SearchNavigation, SymbolIndex,
    Tier, WorkspaceSymbol, build,
};
use protobuf::{EnumOrUnknown, Message};
use scip::types::{
    Document, Index, Occurrence, PositionEncoding, SymbolInformation, SymbolRole,
    symbol_information::Kind,
};

const FOO: &str = "rust-analyzer cargo demo 0.1.0 foo().";
const DUP: &str = "rust-analyzer cargo demo 0.1.0 dup().";
const LOCAL: &str = "local 0";
const DEF: i32 = SymbolRole::Definition as i32;

/// src/lib.rs: `foo` defined on row 1 (bytes 14..17) and called on row 2
/// (bytes 38..41); `dup` defined on row 4; a `local 0` of its own on row 3.
const LIB: &str =
    "// lib\npub fn foo() {}\npub fn bar() { foo() }\nfn baz(x: u8) {}\npub fn dup() {}\n";
/// src/main.rs: `foo` called on row 2 with the local `x`, defined on row 1.
const MAIN: &str = "fn main() {\n    let x = 1;\n    foo(x);\n    x\n}\n";
/// src/other.rs as it was indexed, with `dup` on row 0...
const OTHER_INDEXED: &str = "pub fn dup() {}\n";
/// ...and as it is now.
const OTHER_NOW: &str = "// moved\npub fn dup() {}\n";

fn occ(range: &[i32], symbol: &str, roles: i32) -> Occurrence {
    Occurrence {
        range: range.to_vec(),
        symbol: symbol.to_owned(),
        symbol_roles: roles,
        ..Default::default()
    }
}

fn doc(rel: &str, language: &str, occurrences: Vec<Occurrence>) -> Document {
    Document {
        language: language.to_owned(),
        relative_path: rel.to_owned(),
        position_encoding: EnumOrUnknown::new(PositionEncoding::UTF8CodeUnitOffsetFromLineStart),
        occurrences,
        ..Default::default()
    }
}

fn info(symbol: &str, name: &str, kind: Kind) -> SymbolInformation {
    SymbolInformation {
        symbol: symbol.to_owned(),
        display_name: name.to_owned(),
        kind: EnumOrUnknown::new(kind),
        ..Default::default()
    }
}

fn index(documents: Vec<Document>, external: Vec<SymbolInformation>) -> Arc<ScipIndex> {
    let bytes = Index {
        documents,
        external_symbols: external,
        ..Default::default()
    }
    .write_to_bytes()
    .unwrap();
    Arc::new(ScipIndex::from_bytes(&bytes).unwrap())
}

/// The project above, written to a tempdir, and its index.
fn project() -> (tempfile::TempDir, ScipNavigation) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), LIB).unwrap();
    fs::write(root.join("src/main.rs"), MAIN).unwrap();
    fs::write(root.join("src/other.rs"), OTHER_NOW).unwrap();
    assert_ne!(OTHER_INDEXED, OTHER_NOW);
    let mut lib = doc(
        "src/lib.rs",
        "rust",
        vec![
            occ(&[1, 7, 10], FOO, DEF),
            occ(&[2, 15, 18], FOO, 0),
            occ(&[3, 7, 8], LOCAL, DEF),
            occ(&[4, 7, 10], DUP, DEF),
        ],
    );
    lib.symbols.push(info(FOO, "foo", Kind::Function));
    lib.symbols.push(info(DUP, "dup", Kind::Function));
    let main = doc(
        "src/main.rs",
        "rust",
        vec![
            occ(&[1, 8, 9], LOCAL, DEF),
            occ(&[2, 4, 7], FOO, 0),
            occ(&[2, 8, 9], LOCAL, 0),
            occ(&[3, 4, 5], LOCAL, 0),
        ],
    );
    let other = doc("src/other.rs", "rust", vec![occ(&[0, 7, 10], DUP, DEF)]);
    let nav = ScipNavigation::new(index(vec![lib, main, other], Vec::new()), root);
    (dir, nav)
}

fn request(root: &Path, rel: &str, text: &str, offset: usize) -> NavRequest {
    NavRequest {
        root: root.to_path_buf(),
        rel: PathBuf::from(rel),
        language: Some("rust"),
        text: Text::new(text),
        offset,
        buffer: None,
    }
}

fn not_cancelled() -> AtomicBool {
    AtomicBool::new(false)
}

#[test]
fn identity_and_readiness_by_indexed_language() {
    let tsx = doc("src/app.tsx", "TypeScriptReact", Vec::new());
    let rust = doc("src/lib.rs", "rust", vec![occ(&[0, 0, 3], FOO, DEF)]);
    let mut described = rust.clone();
    described.symbols.push(info(FOO, "foo", Kind::Function));
    let nav = ScipNavigation::new(index(vec![tsx.clone(), described], Vec::new()), "/nowhere");
    assert_eq!(nav.name(), "scip");
    assert_eq!(nav.precision(), Precision::Exact);
    assert_eq!(nav.readiness(Some("tsx")), Readiness::Ready);
    assert_eq!(nav.readiness(Some("rust")), Readiness::Ready);
    assert_eq!(nav.readiness(Some("python")), Readiness::Unsupported);
    assert_eq!(nav.readiness(Some("typescript")), Readiness::Unsupported);
    assert_eq!(nav.readiness(None), Readiness::Ready);

    // The other spellings fe-syntax names differently.
    let nav = ScipNavigation::new(
        index(
            vec![
                doc("a.cc", "C++", Vec::new()),
                doc("a.cs", "C#", Vec::new()),
                doc("a.sh", "Shell", Vec::new()),
            ],
            Vec::new(),
        ),
        "/nowhere",
    );
    for language in ["cpp", "csharp", "bash"] {
        assert_eq!(
            nav.readiness(Some(language)),
            Readiness::Ready,
            "{language}"
        );
    }

    // A file split across documents of different languages is in each.
    let nav = ScipNavigation::new(
        index(
            vec![
                doc("src/gen.rs", "rust", Vec::new()),
                doc("src/gen.rs", "python", Vec::new()),
            ],
            Vec::new(),
        ),
        "/nowhere",
    );
    assert_eq!(nav.readiness(Some("rust")), Readiness::Ready);
    assert_eq!(nav.readiness(Some("python")), Readiness::Ready);

    // Locals alone aren't global symbols: nothing for workspace symbols.
    let locals = doc("src/lib.rs", "rust", vec![occ(&[0, 0, 3], LOCAL, DEF)]);
    let nav = ScipNavigation::new(index(vec![tsx, locals], Vec::new()), "/nowhere");
    assert_eq!(nav.readiness(Some("rust")), Readiness::Ready);
    assert_eq!(nav.readiness(None), Readiness::Unsupported);
    let empty = ScipNavigation::new(index(Vec::new(), Vec::new()), "/nowhere");
    assert_eq!(empty.readiness(None), Readiness::Unsupported);
    assert_eq!(empty.readiness(Some("rust")), Readiness::Unsupported);

    // Any global symbol will do: one the index doesn't describe, one it
    // only describes as external, or one of a kind no outline has.
    let undescribed = doc("src/lib.rs", "rust", vec![occ(&[0, 0, 3], FOO, 0)]);
    let external = vec![info(FOO, "foo", Kind::Function)];
    let mut unmapped = doc("src/lib.rs", "rust", vec![occ(&[0, 0, 3], FOO, DEF)]);
    unmapped.symbols.push(info(FOO, "foo", Kind::Parameter));
    for (documents, external) in [
        (vec![undescribed], Vec::new()),
        (Vec::new(), external),
        (vec![unmapped], Vec::new()),
    ] {
        let nav = ScipNavigation::new(index(documents, external), "/nowhere");
        assert_eq!(nav.readiness(None), Readiness::Ready);
    }
}

/// src/alias.rs: calls `foo` as `bar`.
const ALIAS: &str = "use crate::foo as bar;\nfn f() { bar() }\n";
/// The alias's own symbol, which the index has on its declaration and on
/// each use of it, beside `foo`'s.
const ALIAS_LOCAL: &str = "local 1";

/// src/lib.rs as [`LIB`], src/alias.rs as [`ALIAS`], and src/other.rs, which
/// was indexed calling `foo` but now calls `bar`, with no alias.
fn alias_project() -> (tempfile::TempDir, ScipNavigation) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), LIB).unwrap();
    fs::write(root.join("src/alias.rs"), ALIAS).unwrap();
    fs::write(root.join("src/other.rs"), "fn g() { bar() }\n").unwrap();
    let mut lib = doc(
        "src/lib.rs",
        "rust",
        vec![occ(&[1, 7, 10], FOO, DEF), occ(&[2, 15, 18], FOO, 0)],
    );
    lib.symbols.push(info(FOO, "foo", Kind::Function));
    let alias = doc(
        "src/alias.rs",
        "rust",
        vec![
            occ(&[0, 11, 14], FOO, 0),
            occ(&[0, 18, 21], FOO, 0),
            occ(&[0, 18, 21], ALIAS_LOCAL, DEF),
            occ(&[1, 9, 12], FOO, 0),
            occ(&[1, 9, 12], ALIAS_LOCAL, 0),
        ],
    );
    let other = doc("src/other.rs", "rust", vec![occ(&[0, 9, 12], FOO, 0)]);
    let nav = ScipNavigation::new(index(vec![lib, alias, other], Vec::new()), root);
    (dir, nav)
}

#[test]
fn an_alias_resolves_to_its_symbol_spelt_as_defined() {
    let (dir, nav) = alias_project();
    let root = dir.path();
    let cancel = not_cancelled();
    let req = request(root, "src/alias.rs", ALIAS, ALIAS.rfind("bar").unwrap());

    let found = nav.definitions(&req, &cancel).unwrap();
    assert_eq!(found.word.as_deref(), Some("bar"));
    assert_eq!(found.items.len(), 1);
    assert_eq!(found.items[0].name, "foo");
    assert_eq!(found.items[0].location.rel, Path::new("src/lib.rs"));
    assert_eq!(found.items[0].location.range, 14..17);

    let mut files = Vec::new();
    let summary = nav
        .references(&req, &cancel, &mut |batch| files.extend(batch))
        .unwrap();
    assert_eq!(summary.word.as_deref(), Some("bar"));
    let seen: Vec<(PathBuf, String)> = (files.iter())
        .flat_map(|file| &file.refs)
        .map(|r| {
            let preview = &r.preview[r.preview_ranges[0].clone()];
            (r.location.rel.clone(), preview.to_owned())
        })
        .collect();
    let at = |rel: &str, word: &str| (PathBuf::from(rel), word.to_owned());
    assert_eq!(
        seen,
        [
            at("src/alias.rs", "foo"),
            at("src/alias.rs", "bar"),
            at("src/alias.rs", "bar"),
            at("src/lib.rs", "foo"),
            at("src/lib.rs", "foo"),
        ]
    );

    // `foo`'s definition renamed in place to the alias's spelling has
    // changed all the same: no definition is left, so search answers, and
    // references leave it out.
    let renamed = LIB.replacen("pub fn foo", "pub fn bar", 1);
    assert_eq!(renamed.len(), LIB.len());
    fs::write(root.join("src/lib.rs"), &renamed).unwrap();
    assert!(matches!(
        nav.definitions(&req, &cancel),
        Err(NavError::Unavailable(_))
    ));
    let mut files = Vec::new();
    nav.references(&req, &cancel, &mut |batch| files.extend(batch))
        .unwrap();
    let seen: Vec<(PathBuf, u64, bool)> = (files.iter())
        .flat_map(|file| &file.refs)
        .map(|r| (r.location.rel.clone(), r.line_number, r.is_definition))
        .collect();
    assert_eq!(
        seen,
        [
            (PathBuf::from("src/alias.rs"), 1, false),
            (PathBuf::from("src/alias.rs"), 1, false),
            (PathBuf::from("src/alias.rs"), 2, false),
            (PathBuf::from("src/lib.rs"), 3, false),
        ]
    );
}

#[test]
fn definitions_resolve_into_other_files_and_locals_stay_in_their_document() {
    let (dir, nav) = project();
    let root = dir.path();
    let cancel = not_cancelled();

    // `foo` called in main.rs: defined in lib.rs, at its bytes there.
    let foo = MAIN.find("foo").unwrap() + 1;
    let found = nav
        .definitions(&request(root, "src/main.rs", MAIN, foo), &cancel)
        .unwrap();
    assert_eq!(found.word.as_deref(), Some("foo"));
    assert!(found.unambiguous);
    assert!(!found.partial);
    assert_eq!(found.items.len(), 1);
    let def = &found.items[0];
    assert_eq!(
        def.location,
        Location {
            rel: PathBuf::from("src/lib.rs"),
            range: 14..17,
            row: 1,
            snapshot: None,
        }
    );
    assert_eq!(&LIB[def.location.range.clone()], "foo");
    assert_eq!(def.tier, Tier::Project);
    assert_eq!(def.kind, Some(OutlineKind::Function));
    assert_eq!(def.name, "foo");

    // The local `x` in main.rs: its own `let`, not lib.rs's `local 0`; the
    // request's file is located in its text and buffer.
    let buffer = Buffer::new(MAIN);
    let mut req = request(root, "src/main.rs", MAIN, MAIN.rfind('x').unwrap());
    req.buffer = Some((buffer.id(), buffer.version()));
    let found = nav.definitions(&req, &cancel).unwrap();
    let x = MAIN.find("x =").unwrap();
    assert_eq!(found.items.len(), 1);
    assert_eq!(
        found.items[0].location,
        Location {
            rel: PathBuf::from("src/main.rs"),
            range: x..x + 1,
            row: 1,
            snapshot: req.buffer,
        }
    );
    assert_eq!(found.items[0].tier, Tier::SameFile);
    assert_eq!(found.items[0].kind, None);
    assert!(found.unambiguous);

    // And lib.rs's `x` is its own parameter.
    let x = LIB.find("x:").unwrap();
    let found = nav
        .definitions(&request(root, "src/lib.rs", LIB, x), &cancel)
        .unwrap();
    assert_eq!(found.items.len(), 1);
    assert_eq!(found.items[0].location.rel, Path::new("src/lib.rs"));
    assert_eq!(found.items[0].location.range, x..x + 1);
}

#[test]
fn a_stale_request_falls_through_to_search_and_a_changed_file_is_dropped() {
    let (dir, nav) = project();
    let root = dir.path();
    let cancel = not_cancelled();

    // An unsaved edit shifts `foo` off its indexed position.
    let edited = MAIN.replace("    foo(x)", "     foo(x)");
    let offset = edited.find("foo").unwrap() + 1;
    let req = request(root, "src/main.rs", &edited, offset);
    assert_eq!(
        nav.definitions(&req, &cancel),
        Err(NavError::Unavailable("not in the SCIP index".to_owned()))
    );
    // A caret on nothing indexed, or in a file not in the index, the same.
    let req_space = request(root, "src/main.rs", MAIN, 0);
    assert!(matches!(
        nav.definitions(&req_space, &cancel),
        Err(NavError::Unavailable(_))
    ));
    assert!(matches!(
        nav.definitions(&request(root, "src/new.rs", MAIN, 1), &cancel),
        Err(NavError::Unavailable(_))
    ));

    // Behind it in a Navigation, search answers instead.
    let mut symbols = SymbolIndex::new(root);
    build(
        root,
        &BuildOptions { threads: 1 },
        Arc::new(AtomicBool::new(false)),
        |batch| symbols.extend(batch),
    );
    symbols.finish();
    let search = SearchNavigation::new(root, Arc::new(RwLock::new(symbols)))
        .with_budget(Duration::from_secs(60));
    let navigation = Navigation::new();
    navigation.add(Arc::new(nav), 10);
    navigation.add(Arc::new(search), 0);
    let answer = navigation.definitions(&req, &cancel).unwrap();
    assert_eq!(answer.provider, "search");
    assert_eq!(answer.precision, Precision::Search);
    assert!(
        (answer.value.items.iter()).any(
            |def| def.location.rel == Path::new("src/lib.rs") && def.location.range == (14..17)
        )
    );
    // Unedited, the index answers.
    let unedited = request(root, "src/main.rs", MAIN, MAIN.find("foo").unwrap());
    assert_eq!(
        navigation.definitions(&unedited, &cancel).unwrap().provider,
        "scip"
    );

    // `dup` is defined in lib.rs and other.rs, but other.rs has changed
    // since: only lib.rs's is left, and it's unambiguous.
    let (dir, nav) = project();
    let root = dir.path();
    let dup = LIB.find("dup").unwrap();
    let found = nav
        .definitions(&request(root, "src/lib.rs", LIB, dup), &cancel)
        .unwrap();
    assert_eq!(found.items.len(), 1);
    assert_eq!(found.items[0].location.rel, Path::new("src/lib.rs"));
    assert_eq!(found.items[0].location.range, dup..dup + 3);
    assert_eq!(found.items[0].location.row, 4);
    assert_eq!(found.items[0].tier, Tier::SameFile);
    assert!(found.unambiguous);

    // With the file as it was indexed, both are found, the request's own
    // first.
    fs::write(root.join("src/other.rs"), OTHER_INDEXED).unwrap();
    let found = nav
        .definitions(&request(root, "src/lib.rs", LIB, dup), &cancel)
        .unwrap();
    let rels: Vec<_> = (found.items.iter())
        .map(|def| (def.location.rel.clone(), def.tier))
        .collect();
    assert_eq!(
        rels,
        [
            (PathBuf::from("src/lib.rs"), Tier::SameFile),
            (PathBuf::from("src/other.rs"), Tier::Project),
        ]
    );
    assert!(!found.unambiguous);

    // No site left at all (lib.rs, `foo`'s one, has changed too):
    // unavailable, so search answers.
    fs::write(root.join("src/lib.rs"), format!("// moved\n{LIB}")).unwrap();
    let req = request(root, "src/main.rs", MAIN, MAIN.find("foo").unwrap());
    assert!(matches!(
        nav.definitions(&req, &cancel),
        Err(NavError::Unavailable(_))
    ));
}

fn unavailable<T>(answer: Result<T, NavError>) -> bool {
    matches!(answer, Err(NavError::Unavailable(_)))
}

#[test]
fn an_alias_spelling_is_taken_only_where_the_index_records_it() {
    let cancel = not_cancelled();
    let refs =
        |nav: &ScipNavigation, req: &NavRequest| -> Vec<(PathBuf, std::ops::Range<usize>, bool)> {
            let mut files = Vec::new();
            nav.references(req, &not_cancelled(), &mut |batch| files.extend(batch))
                .unwrap();
            (files.into_iter())
                .flat_map(|file| file.refs)
                .map(|r| (r.location.rel, r.location.range, r.is_definition))
                .collect()
        };

    // (1) `foo`'s definition renamed in place to `bar`, the caret on it:
    // it isn't the alias, it's a changed file. No exact answer, so search
    // answers.
    let (dir, nav) = alias_project();
    let root = dir.path();
    let renamed = LIB.replacen("pub fn foo", "pub fn bar", 1);
    fs::write(root.join("src/lib.rs"), &renamed).unwrap();
    let at = renamed.find("bar").unwrap();
    assert_eq!(at, 14);
    let req = request(root, "src/lib.rs", &renamed, at + 1);
    assert!(unavailable(nav.definitions(&req, &cancel)));
    let mut calls = 0;
    assert!(unavailable(
        nav.references(&req, &cancel, &mut |_| calls += 1)
    ));
    assert_eq!(calls, 0);
    // Asked from the alias's use, the renamed definition isn't reported
    // either: not as a definition, and not as a verified reference.
    let from_alias = request(root, "src/alias.rs", ALIAS, ALIAS.rfind("bar").unwrap());
    assert!(unavailable(nav.definitions(&from_alias, &cancel)));
    let found = refs(&nav, &from_alias);
    assert!(!found.is_empty());
    assert!(
        !(found.iter()).any(|(rel, range, is_definition)| *is_definition
            || (rel == Path::new("src/lib.rs") && *range == (14..17)))
    );

    // (2) The canonical import `crate::foo` changed to `crate::bar`, the
    // caret on the final `bar`: the alias's declaration and use are taken,
    // where the index has the alias's own symbol, but the changed import
    // isn't, though it's in the request's file and spelt as the caret's word.
    let (dir, nav) = alias_project();
    let root = dir.path();
    let changed = ALIAS.replacen("crate::foo", "crate::bar", 1);
    assert_eq!(changed.len(), ALIAS.len());
    fs::write(root.join("src/alias.rs"), &changed).unwrap();
    let req = request(
        root,
        "src/alias.rs",
        &changed,
        changed.rfind("bar").unwrap(),
    );
    let found = nav.definitions(&req, &cancel).unwrap();
    assert_eq!(found.items.len(), 1);
    assert_eq!(found.items[0].location.rel, Path::new("src/lib.rs"));
    let use_at = changed.rfind("bar").unwrap();
    let alias = |range: std::ops::Range<usize>| (PathBuf::from("src/alias.rs"), range, false);
    let lib = |range: std::ops::Range<usize>, def| (PathBuf::from("src/lib.rs"), range, def);
    assert_eq!(
        refs(&nav, &req),
        [
            alias(18..21),
            alias(use_at..use_at + 3),
            lib(14..17, true),
            lib(38..41, false),
        ]
    );
    // A caret on the changed import itself has no exact answer.
    let on_import = request(root, "src/alias.rs", &changed, 12);
    assert!(unavailable(nav.definitions(&on_import, &cancel)));
    assert!(unavailable(nav.references(
        &on_import,
        &cancel,
        &mut |_| {}
    )));
}

#[test]
fn a_name_grown_into_a_longer_identifier_is_changed() {
    let cancel = not_cancelled();
    let names = |nav: &ScipNavigation| -> Vec<String> {
        (nav.workspace_symbols("foo", 10, &not_cancelled())
            .unwrap()
            .into_iter())
        .map(|symbol| symbol.name)
        .collect()
    };
    // Grown by letters, and by a combining mark: `foó` is one identifier.
    for grown_name in ["foobar", "foo\u{301}"] {
        let (dir, nav) = project();
        let root = dir.path();
        let main_req = request(root, "src/main.rs", MAIN, MAIN.find("foo").unwrap());
        assert_eq!(names(&nav), ["foo"]);

        // `foo`'s definition grown: `foo` still sits at its indexed range,
        // but it isn't the identifier there.
        let grown = LIB.replacen("pub fn foo()", &format!("pub fn {grown_name}()"), 1);
        fs::write(root.join("src/lib.rs"), &grown).unwrap();
        assert!(
            unavailable(nav.definitions(&main_req, &cancel)),
            "{grown_name}"
        );
        assert!(names(&nav).is_empty(), "{grown_name}");
        let mut files = Vec::new();
        nav.references(&main_req, &cancel, &mut |batch| files.extend(batch))
            .unwrap();
        let call = grown.find("foo()").unwrap();
        let seen: Vec<(PathBuf, std::ops::Range<usize>, bool)> = (files.into_iter())
            .flat_map(|file| file.refs)
            .map(|r| (r.location.rel, r.location.range, r.is_definition))
            .collect();
        let at = MAIN.find("foo").unwrap();
        assert_eq!(
            seen,
            [
                (PathBuf::from("src/main.rs"), at..at + 3, false),
                (PathBuf::from("src/lib.rs"), call..call + 3, false),
            ],
            "{grown_name}"
        );
        // A caret on the grown name isn't on an indexed occurrence of it.
        let on_grown = request(root, "src/lib.rs", &grown, 15);
        assert!(
            unavailable(nav.definitions(&on_grown, &cancel)),
            "{grown_name}"
        );
    }

    let alias = |range: std::ops::Range<usize>| (PathBuf::from("src/alias.rs"), range);
    let lib = |range: std::ops::Range<usize>| (PathBuf::from("src/lib.rs"), range);
    for grown_alias in ["barx", "bar\u{301}"] {
        // The alias's declaration grown: no longer evidence that `bar` is
        // the alias, so a caret on its use has no exact answer, and the use
        // isn't a reference.
        let (dir, nav) = alias_project();
        let root = dir.path();
        let grown = ALIAS.replacen("as bar", &format!("as {grown_alias}"), 1);
        fs::write(root.join("src/alias.rs"), &grown).unwrap();
        let req = request(root, "src/alias.rs", &grown, grown.rfind("bar").unwrap());
        assert!(unavailable(nav.definitions(&req, &cancel)), "{grown_alias}");
        assert_eq!(
            foo_references(&nav, root),
            [alias(11..14), lib(14..17), lib(38..41)],
            "{grown_alias}"
        );

        // The use grown, the declaration as it was: the declaration is
        // still the alias, but the use isn't a whole `bar`.
        let grown = ALIAS.replacen("bar()", &format!("{grown_alias}()"), 1);
        fs::write(root.join("src/alias.rs"), &grown).unwrap();
        assert_eq!(
            foo_references(&nav, root),
            [alias(11..14), alias(18..21), lib(14..17), lib(38..41)],
            "{grown_alias}"
        );
    }

    // `$` is an identifier char in TypeScript: `foo$` isn't `foo` there.
    const TS: &str = "export const foo$ = 1;\n";
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join("a.ts"), TS).unwrap();
    let mut ts = doc("a.ts", "typescript", vec![occ(&[0, 13, 16], FOO, DEF)]);
    ts.symbols.push(info(FOO, "foo", Kind::Variable));
    let nav = ScipNavigation::new(index(vec![ts], Vec::new()), root);
    assert!(names(&nav).is_empty());
    fs::write(root.join("a.ts"), TS.replace("foo$", "foo ")).unwrap();
    assert_eq!(names(&nav), ["foo"]);
}

#[test]
fn identifier_chars_follow_the_indexed_language() {
    // Extensionless scripts the index records as JavaScript, where `$` is an
    // identifier char though their names say nothing of it.
    const RUN: &str = "function foo() {}\n";
    const CLI: &str = "foo();\n";
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("bin")).unwrap();
    fs::write(root.join("bin/run"), RUN).unwrap();
    fs::write(root.join("bin/cli"), CLI).unwrap();
    let mut run = doc("bin/run", "JavaScript", vec![occ(&[0, 9, 12], FOO, DEF)]);
    run.symbols.push(info(FOO, "foo", Kind::Function));
    let cli = doc("bin/cli", "JavaScript", vec![occ(&[0, 0, 3], FOO, 0)]);
    let nav = ScipNavigation::new(index(vec![run, cli], Vec::new()), root);
    let cancel = not_cancelled();
    let mut req = request(root, "bin/cli", CLI, 1);
    req.language = Some("javascript");
    let references = |nav: &ScipNavigation| -> Vec<(PathBuf, std::ops::Range<usize>, bool)> {
        let mut files = Vec::new();
        nav.references(&req, &not_cancelled(), &mut |batch| files.extend(batch))
            .unwrap();
        (files.into_iter())
            .flat_map(|file| file.refs)
            .map(|r| (r.location.rel, r.location.range, r.is_definition))
            .collect()
    };
    let symbols =
        |nav: &ScipNavigation| nav.workspace_symbols("foo", 10, &not_cancelled()).unwrap();

    assert_eq!(
        nav.definitions(&req, &cancel).unwrap().items[0]
            .location
            .range,
        9..12
    );
    assert_eq!(
        references(&nav),
        [
            (PathBuf::from("bin/cli"), 0..3, false),
            (PathBuf::from("bin/run"), 9..12, true),
        ]
    );
    assert_eq!(symbols(&nav).len(), 1);

    // `foo` grown into `foo$`: no longer the identifier at its site.
    fs::write(root.join("bin/run"), RUN.replace("foo()", "foo$()")).unwrap();
    assert!(unavailable(nav.definitions(&req, &cancel)));
    assert_eq!(references(&nav), [(PathBuf::from("bin/cli"), 0..3, false)]);
    assert!(symbols(&nav).is_empty());
}

#[test]
fn the_request_text_follows_the_request_language() {
    let cancel = not_cancelled();
    let refs = |nav: &ScipNavigation, req: &NavRequest| -> Vec<(std::ops::Range<usize>, bool)> {
        let mut files = Vec::new();
        nav.references(req, &not_cancelled(), &mut |batch| files.extend(batch))
            .unwrap();
        (files.into_iter())
            .flat_map(|file| file.refs)
            .map(|r| (r.location.range, r.is_definition))
            .collect()
    };
    // An extensionless JavaScript script the index gives no language, or
    // another one: in a JavaScript request, `$` is an identifier char in its
    // text all the same.
    for indexed_language in ["", "Rust"] {
        // A definition grown into `foo$` in the unsaved text.
        const APP: &str = "function foo() {}\nfoo();\n";
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("app"), APP).unwrap();
        let mut app = doc(
            "app",
            indexed_language,
            vec![occ(&[0, 9, 12], FOO, DEF), occ(&[1, 0, 3], FOO, 0)],
        );
        app.symbols.push(info(FOO, "foo", Kind::Function));
        let nav = ScipNavigation::new(index(vec![app], Vec::new()), root);
        let mut req = request(root, "app", APP, APP.rfind("foo").unwrap());
        req.language = Some("javascript");
        assert_eq!(refs(&nav, &req), [(9..12, true), (18..21, false)]);
        let grown = APP.replacen("foo()", "foo$()", 1);
        let mut req = request(root, "app", &grown, grown.rfind("foo").unwrap());
        req.language = Some("javascript");
        let use_at = grown.rfind("foo").unwrap();
        assert!(
            unavailable(nav.definitions(&req, &cancel)),
            "{indexed_language:?}"
        );
        assert_eq!(
            refs(&nav, &req),
            [(use_at..use_at + 3, false)],
            "{indexed_language:?}"
        );

        // An import alias, `bar`, of `foo` from `lib`.
        const IMPORT: &str = "import { foo as bar } from './lib';\nbar();\n";
        const LIB_JS: &str = "export function foo() {}\n";
        fs::write(root.join("lib"), LIB_JS).unwrap();
        fs::write(root.join("app"), IMPORT).unwrap();
        let mut lib = doc("lib", "JavaScript", vec![occ(&[0, 16, 19], FOO, DEF)]);
        lib.symbols.push(info(FOO, "foo", Kind::Function));
        let app = doc(
            "app",
            indexed_language,
            vec![
                occ(&[0, 9, 12], FOO, 0),
                occ(&[0, 16, 19], FOO, 0),
                occ(&[0, 16, 19], ALIAS_LOCAL, DEF),
                occ(&[1, 0, 3], FOO, 0),
                occ(&[1, 0, 3], ALIAS_LOCAL, 0),
            ],
        );
        let nav = ScipNavigation::new(index(vec![lib, app], Vec::new()), root);
        let js = |text: &str, offset: usize| {
            let mut req = request(root, "app", text, offset);
            req.language = Some("javascript");
            req
        };
        let on_use = |text: &str| js(text, text.rfind("bar").unwrap());
        let on_foo = |text: &str| js(text, 10);
        let use_at = IMPORT.rfind("bar").unwrap();
        assert_eq!(
            nav.definitions(&on_use(IMPORT), &cancel).unwrap().items[0]
                .location
                .rel,
            Path::new("lib")
        );
        assert_eq!(
            refs(&nav, &on_foo(IMPORT)),
            [
                (9..12, false),
                (16..19, false),
                (use_at..use_at + 3, false),
                // lib's definition.
                (16..19, true),
            ]
        );
        // The alias's declaration grown into `bar$`: no evidence for `bar`.
        let grown = IMPORT.replacen("as bar", "as bar$", 1);
        assert!(
            unavailable(nav.definitions(&on_use(&grown), &cancel)),
            "{indexed_language:?}"
        );
        assert_eq!(
            refs(&nav, &on_foo(&grown)),
            [(9..12, false), (16..19, true)],
            "{indexed_language:?}"
        );
        // The alias's use grown into `bar$`, its declaration as it was.
        let grown = IMPORT.replacen("bar()", "bar$()", 1);
        assert_eq!(
            refs(&nav, &on_foo(&grown)),
            [(9..12, false), (16..19, false), (16..19, true)],
            "{indexed_language:?}"
        );
    }
}

/// [`ALIAS`]'s project with the alias's own symbol `alias` (on its
/// declaration and its use, beside `foo`'s); `describe` gives a global one
/// the name `bar`; `split` puts it in a second document for src/alias.rs.
fn alias_with(
    alias_symbol: &str,
    describe: bool,
    split: bool,
) -> (tempfile::TempDir, ScipNavigation) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), LIB).unwrap();
    fs::write(root.join("src/alias.rs"), ALIAS).unwrap();
    let mut lib = doc(
        "src/lib.rs",
        "rust",
        vec![occ(&[1, 7, 10], FOO, DEF), occ(&[2, 15, 18], FOO, 0)],
    );
    lib.symbols.push(info(FOO, "foo", Kind::Function));
    let foo_sites = vec![
        occ(&[0, 11, 14], FOO, 0),
        occ(&[0, 18, 21], FOO, 0),
        occ(&[1, 9, 12], FOO, 0),
    ];
    let alias_sites = vec![
        occ(&[0, 18, 21], alias_symbol, DEF),
        occ(&[1, 9, 12], alias_symbol, 0),
    ];
    let mut documents = vec![lib];
    if split {
        documents.push(doc("src/alias.rs", "rust", foo_sites));
        documents.push(doc("src/alias.rs", "rust", alias_sites));
    } else {
        documents.push(doc(
            "src/alias.rs",
            "rust",
            [foo_sites, alias_sites].concat(),
        ));
    }
    if describe {
        documents[1]
            .symbols
            .push(info(alias_symbol, "bar", Kind::Variable));
    }
    let nav = ScipNavigation::new(index(documents, Vec::new()), root);
    (dir, nav)
}

/// The references to `foo` from its definition in [`LIB`], as (file, bytes),
/// sorted.
fn foo_references(nav: &ScipNavigation, root: &Path) -> Vec<(PathBuf, std::ops::Range<usize>)> {
    let req = request(root, "src/lib.rs", LIB, 15);
    let mut files = Vec::new();
    nav.references(&req, &not_cancelled(), &mut |batch| files.extend(batch))
        .unwrap();
    let mut found: Vec<(PathBuf, std::ops::Range<usize>)> = (files.into_iter())
        .flat_map(|file| file.refs)
        .map(|r| (r.location.rel, r.location.range))
        .collect();
    found.sort_by_key(|(rel, range)| (rel.clone(), range.start));
    found
}

#[test]
fn a_global_alias_is_spelt_by_its_own_name() {
    const ALIAS_GLOBAL: &str = "rust-analyzer cargo demo 0.1.0 alias/bar.";
    let alias = |range: std::ops::Range<usize>| (PathBuf::from("src/alias.rs"), range);
    let lib = |range: std::ops::Range<usize>| (PathBuf::from("src/lib.rs"), range);
    let use_at = ALIAS.rfind("bar").unwrap();
    for describe in [true, false] {
        let (dir, nav) = alias_with(ALIAS_GLOBAL, describe, false);
        let root = dir.path();
        assert_eq!(
            foo_references(&nav, root),
            [
                alias(11..14),
                alias(18..21),
                alias(use_at..use_at + 3),
                lib(14..17),
                lib(38..41),
            ],
            "describe {describe}"
        );

        // The alias renamed to `baz`, declaration and use together: the
        // index still names the alias `bar`, so they're changed sites.
        let renamed = ALIAS.replace("bar", "baz");
        fs::write(root.join("src/alias.rs"), &renamed).unwrap();
        assert_eq!(
            foo_references(&nav, root),
            [alias(11..14), lib(14..17), lib(38..41)],
            "describe {describe}"
        );
    }
}

#[test]
fn documents_split_from_one_file_keep_their_scopes() {
    let cancel = not_cancelled();
    let alias = |range: std::ops::Range<usize>| (PathBuf::from("src/alias.rs"), range);
    let lib = |range: std::ops::Range<usize>| (PathBuf::from("src/lib.rs"), range);

    // The alias's own symbol in another document for src/alias.rs than
    // `foo`'s sites is no evidence for them.
    let (dir, nav) = alias_with(ALIAS_LOCAL, false, true);
    let root = dir.path();
    assert_eq!(
        foo_references(&nav, root),
        [alias(11..14), lib(14..17), lib(38..41)]
    );
    // In the same document, it is.
    let (dir, nav) = alias_with(ALIAS_LOCAL, false, false);
    let use_at = ALIAS.rfind("bar").unwrap();
    assert_eq!(
        foo_references(&nav, dir.path()),
        [
            alias(11..14),
            alias(18..21),
            alias(use_at..use_at + 3),
            lib(14..17),
            lib(38..41),
        ]
    );

    // A local on the span of a global from another document of its file is
    // still the local.
    const SPLIT: &str = "let foo = 1;\nfoo\n";
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), LIB).unwrap();
    fs::write(root.join("src/split.rs"), SPLIT).unwrap();
    let mut lib_doc = doc("src/lib.rs", "rust", vec![occ(&[1, 7, 10], FOO, DEF)]);
    lib_doc.symbols.push(info(FOO, "foo", Kind::Function));
    let nav = ScipNavigation::new(
        index(
            vec![
                lib_doc,
                doc("src/split.rs", "rust", vec![occ(&[1, 0, 3], FOO, 0)]),
                doc(
                    "src/split.rs",
                    "rust",
                    vec![occ(&[0, 4, 7], LOCAL, DEF), occ(&[1, 0, 3], LOCAL, 0)],
                ),
            ],
            Vec::new(),
        ),
        root,
    );
    let at = SPLIT.rfind("foo").unwrap();
    let found = nav
        .definitions(&request(root, "src/split.rs", SPLIT, at), &cancel)
        .unwrap();
    assert_eq!(found.items.len(), 1);
    assert_eq!(found.items[0].location.rel, Path::new("src/split.rs"));
    assert_eq!(found.items[0].location.range, 4..7);
}

#[test]
fn a_site_in_two_encodings_is_one_reference() {
    // `foo` at bytes 3..6, UTF-16 units 2..5, after the two-byte `é`.
    const ENC: &str = "é foo\nfoo\n";
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/enc.rs"), ENC).unwrap();
    let mut utf8 = doc("src/enc.rs", "rust", vec![occ(&[0, 3, 6], FOO, DEF)]);
    utf8.symbols.push(info(FOO, "foo", Kind::Function));
    let mut utf16 = doc(
        "src/enc.rs",
        "rust",
        vec![
            occ(&[0, 2, 5], FOO, 0),
            occ(&[0, 3, 6], FOO, DEF),
            occ(&[1, 0, 3], FOO, 0),
        ],
    );
    utf16.position_encoding =
        EnumOrUnknown::new(PositionEncoding::UTF16CodeUnitOffsetFromLineStart);
    // UTF-16 `(0, 3)` is past `foo`'s start: a definition there doesn't
    // resolve to it, so it's dropped as changed.
    let nav = ScipNavigation::new(index(vec![utf8, utf16], Vec::new()), root);
    let cancel = not_cancelled();
    let req = request(root, "src/enc.rs", ENC, 7);

    let mut files = Vec::new();
    nav.references(&req, &cancel, &mut |batch| files.extend(batch))
        .unwrap();
    let seen: Vec<(std::ops::Range<usize>, bool)> = (files.into_iter())
        .flat_map(|file| file.refs)
        .map(|r| (r.location.range, r.is_definition))
        .collect();
    assert_eq!(seen, [(3..6, true), (7..10, false)]);

    let found = nav.definitions(&req, &cancel).unwrap();
    assert_eq!(found.items.len(), 1);
    assert_eq!(found.items[0].location.range, 3..6);
    assert!(found.unambiguous);
}

#[test]
fn references_come_a_file_at_a_time_in_line_order() {
    let (dir, nav) = project();
    let root = dir.path();
    let cancel = not_cancelled();
    let req = request(root, "src/main.rs", MAIN, MAIN.find("foo").unwrap());
    let mut batches: Vec<Vec<FileReferences>> = Vec::new();
    let summary = nav
        .references(&req, &cancel, &mut |batch| batches.push(batch))
        .unwrap();
    assert_eq!(summary.word.as_deref(), Some("foo"));
    assert!(!summary.cancelled);
    assert!(!summary.truncated);
    assert_eq!(summary.files_searched, 2);

    // The request's own file first, then the rest; one file a batch.
    assert!(batches.iter().all(|batch| batch.len() == 1));
    let files: Vec<FileReferences> = batches.into_iter().flatten().collect();
    let rels: Vec<&Path> = files.iter().map(|file| file.rel.as_path()).collect();
    assert_eq!(rels, [Path::new("src/main.rs"), Path::new("src/lib.rs")]);
    let seen: Vec<(u64, std::ops::Range<usize>, bool)> = (files.iter())
        .flat_map(|file| &file.refs)
        .map(|r| (r.line_number, r.location.range.clone(), r.is_definition))
        .collect();
    let call = MAIN.find("foo").unwrap();
    assert_eq!(
        seen,
        [
            (3, call..call + 3, false),
            (2, 14..17, true),
            (3, 38..41, false),
        ]
    );
    for file in &files {
        for r in &file.refs {
            assert!(r.verified);
            assert_eq!(r.location.rel, file.rel);
            assert_eq!(r.location.row as u64 + 1, r.line_number);
            assert_eq!(r.preview_ranges.len(), 1);
            assert_eq!(&r.preview[r.preview_ranges[0].clone()], "foo");
        }
    }
    assert_eq!(files[1].refs[1].preview, "pub fn bar() { foo() }");

    // A local's references are its document's only.
    let req = request(root, "src/main.rs", MAIN, MAIN.rfind('x').unwrap());
    let mut files = Vec::new();
    nav.references(&req, &cancel, &mut |batch| files.extend(batch))
        .unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].rel, Path::new("src/main.rs"));
    let lines: Vec<(u64, bool)> = (files[0].refs.iter())
        .map(|r| (r.line_number, r.is_definition))
        .collect();
    assert_eq!(lines, [(2, true), (3, false), (4, false)]);
}

#[test]
fn a_cancel_ends_references_between_files() {
    let (dir, nav) = project();
    let root = dir.path();
    let req = request(root, "src/main.rs", MAIN, MAIN.find("foo").unwrap());

    // After the first batch: a summary saying so, and no more batches.
    let cancel = not_cancelled();
    let mut calls = 0;
    let summary = nav
        .references(&req, &cancel, &mut |_| {
            calls += 1;
            cancel.store(true, Ordering::Relaxed);
        })
        .unwrap();
    assert!(summary.cancelled);
    assert_eq!(calls, 1);

    // Before any: cancelled.
    let cancel = AtomicBool::new(true);
    let mut calls = 0;
    assert_eq!(
        nav.references(&req, &cancel, &mut |_| calls += 1),
        Err(NavError::Cancelled)
    );
    assert_eq!(calls, 0);

    // A stale request isn't answered.
    let edited = MAIN.replace("    foo(x)", "     foo(x)");
    let req = request(root, "src/main.rs", &edited, edited.find("foo").unwrap());
    assert!(matches!(
        nav.references(&req, &not_cancelled(), &mut |_| {}),
        Err(NavError::Unavailable(_))
    ));
}

#[test]
fn workspace_symbols_rank_global_definitions_and_drop_changed_files() {
    const SYMS: &str = "pub fn foo() {}\npub struct Food;\npub fn bar() {}\npub fn foobar() {}\n";
    let sym = |name: &str| format!("rust-analyzer cargo demo 0.1.0 {name}().");
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/sym.rs"), SYMS).unwrap();
    // `fool` was on row 0 here, before the file changed.
    fs::write(root.join("src/stale.rs"), "// changed\n").unwrap();
    let mut syms = doc(
        "src/sym.rs",
        "rust",
        vec![
            occ(&[0, 7, 10], &sym("foo"), DEF),
            occ(&[1, 11, 15], &sym("Food"), DEF),
            occ(&[2, 7, 10], &sym("bar"), DEF),
            occ(&[3, 7, 13], &sym("foobar"), DEF),
            occ(&[3, 14, 15], &sym("foo_param"), DEF),
        ],
    );
    syms.symbols.extend([
        info(&sym("foo"), "foo", Kind::Function),
        info(&sym("Food"), "Food", Kind::Struct),
        info(&sym("bar"), "bar", Kind::Function),
        // No display name: named by its descriptor.
        info(&sym("foobar"), "", Kind::Method),
        // Not a kind an outline lists.
        info(&sym("foo_param"), "foo_param", Kind::Parameter),
    ]);
    let mut stale = doc(
        "src/stale.rs",
        "rust",
        vec![occ(&[0, 7, 11], &sym("fool"), DEF)],
    );
    stale
        .symbols
        .push(info(&sym("fool"), "fool", Kind::Function));
    // Described, but defined nowhere in the project.
    let external = vec![info(&sym("foot"), "foot", Kind::Function)];
    let nav = ScipNavigation::new(index(vec![syms, stale], external), root);
    let cancel = not_cancelled();

    let at =
        |name: &str, range: std::ops::Range<usize>, row: u32, kind: OutlineKind| WorkspaceSymbol {
            location: Location {
                rel: PathBuf::from("src/sym.rs"),
                range,
                row,
                snapshot: None,
            },
            name: name.to_owned(),
            kind,
            container: None,
            indices: Vec::new(),
        };
    let foo = at("foo", 7..10, 0, OutlineKind::Function);
    let food = at("Food", 27..31, 1, OutlineKind::Struct);
    let foobar = at("foobar", 56..62, 3, OutlineKind::Method);
    let mut fool = at("fool", 0..0, 0, OutlineKind::Function);
    fool.location.rel = PathBuf::from("src/stale.rs");
    let all = WorkspaceSymbol::rank("foo", vec![food, foo, foobar, fool], 4);
    let fresh = |symbols: &[WorkspaceSymbol]| -> Vec<WorkspaceSymbol> {
        (symbols.iter())
            .filter(|symbol| symbol.name != "fool")
            .cloned()
            .collect()
    };
    let ranked = fresh(&all);
    assert_eq!(ranked.len(), 3);

    let found = nav.workspace_symbols("foo", 10, &cancel).unwrap();
    assert_eq!(found, ranked);
    assert_eq!(
        &SYMS[found
            .iter()
            .find(|s| s.name == "Food")
            .unwrap()
            .location
            .range
            .clone()],
        "Food"
    );
    // Only the best `limit` are resolved: the stale `fool` among them is
    // left out, not replaced by the next match.
    for limit in 1..=4 {
        let found = nav.workspace_symbols("foo", limit, &cancel).unwrap();
        assert_eq!(found, fresh(&all[..limit]), "limit {limit}");
    }
    let stale_at = all.iter().position(|symbol| symbol.name == "fool").unwrap();
    assert!(stale_at < 3, "`fool` ranks among the first three");
    let found = nav.workspace_symbols("foo", stale_at + 1, &cancel).unwrap();
    assert_eq!(found.len(), stale_at);

    assert_eq!(nav.workspace_symbols("", 10, &cancel), Ok(Vec::new()));
    assert_eq!(nav.workspace_symbols("zzz", 10, &cancel), Ok(Vec::new()));
    assert_eq!(
        nav.workspace_symbols("foo", 10, &AtomicBool::new(true)),
        Err(NavError::Cancelled)
    );
}
