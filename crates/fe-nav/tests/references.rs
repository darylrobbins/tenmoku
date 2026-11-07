//! `SearchNavigation::references`: a word search of the project, with the
//! hits in comments and strings dropped by a parse of their file.

use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
};

use fe_buffer::{Buffer, Text};
use fe_nav::{
    Definitions, FileReferences, NavError, NavRequest, Navigation, NavigationProvider, Precision,
    Readiness, Reference, ReferenceOptions, ReferencesSummary, SearchNavigation, SymbolIndex,
    WorkspaceSymbol,
};

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn write_bytes(root: &Path, rel: &str, bytes: &[u8]) {
    fs::write(root.join(rel), bytes).unwrap();
}

fn search(root: &Path, verify_files: usize) -> SearchNavigation {
    SearchNavigation::new(root, Arc::new(RwLock::new(SymbolIndex::new(root))))
        .with_reference_options(ReferenceOptions { verify_files })
}

/// A request from `rel`, whose text is `text`, with the caret on the first
/// `at`.
fn request(root: &Path, rel: &str, text: &str, at: &str) -> NavRequest {
    NavRequest {
        root: root.to_path_buf(),
        rel: PathBuf::from(rel),
        language: Some("rust"),
        text: Text::new(text),
        offset: text.find(at).expect("caret text"),
        buffer: None,
    }
}

/// The batches `nav` hands over, and its summary.
fn find(
    nav: &dyn NavigationProvider,
    req: &NavRequest,
) -> (Vec<Vec<FileReferences>>, ReferencesSummary) {
    let mut batches = Vec::new();
    let summary = nav
        .references(req, &AtomicBool::new(false), &mut |batch| {
            batches.push(batch)
        })
        .unwrap();
    (batches, summary)
}

/// Each reference, as `rel:row verified definition`, sorted.
fn listed(batches: &[Vec<FileReferences>]) -> Vec<String> {
    let mut listed: Vec<String> = (batches.iter().flatten())
        .flat_map(|file| &file.refs)
        .map(|r| {
            // With `/` on Windows too.
            let rel = r.location.rel.to_string_lossy().replace('\\', "/");
            let mut line = format!("{rel}:{}", r.location.row);
            if !r.verified {
                line.push_str(" unverified");
            }
            if r.is_definition {
                line.push_str(" definition");
            }
            line
        })
        .collect();
    listed.sort();
    listed
}

fn all(batches: &[Vec<FileReferences>]) -> Vec<&Reference> {
    batches
        .iter()
        .flatten()
        .flat_map(|file| &file.refs)
        .collect()
}

/// The file's text as the editor loads it.
fn loaded(root: &Path, rel: &Path) -> Text {
    let file = fs::File::open(root.join(rel)).unwrap();
    Buffer::from_reader_detecting(file).unwrap().text().clone()
}

#[test]
fn code_uses_only() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let lib = "mod util;\nfn main() {\n    util::helper();\n}\n";
    write(root, "src/lib.rs", lib);
    write(
        root,
        "src/util.rs",
        "pub fn helper() {}\n\
         pub fn helper_two() {}\n\
         pub struct Helper;\n\
         // helper is called below\n\
         /* a helper\n\
         \x20  block */\n\
         fn run() {\n\
         \x20   helper();\n\
         \x20   let s = \"helper\";\n\
         \x20   helper_two();\n\
         \x20   let _ = Helper;\n\
         \x20   helper(); // helper again\n\
         }\n",
    );
    let nav = search(root, 1_000);
    let (batches, summary) = find(&nav, &request(root, "src/lib.rs", lib, "helper"));
    assert_eq!(
        listed(&batches),
        [
            "src/lib.rs:2",
            "src/util.rs:0 definition",
            "src/util.rs:11",
            "src/util.rs:7",
        ]
    );
    assert_eq!(summary.word.as_deref(), Some("helper"));
    // Two `//` comments, the block comment and the string.
    assert_eq!(summary.dropped, 4);
    assert_eq!(summary.files_verified, 2);
    assert!(!summary.truncated && !summary.cancelled);
}

#[test]
fn offsets_index_the_loaded_text() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "crlf.rs",
        "fn helper() {}\r\n\r\nfn x() {\r\n    helper();\r\n    helper(); helper();\r\n}\r\n",
    );
    write(
        root,
        "multi.rs",
        "fn y() {\n    let s = \"ééé\"; helper();\n    /* ü */ «helper»; let ünï = helper;\n}\n",
    );
    // A lone `\r` is a line break once loaded, not to the search.
    write(
        root,
        "cr.rs",
        "// a\r helper\nfn f() { helper(); } // x\r helper();\n",
    );
    // Invalid UTF-8 grows once replaced, moving what follows on its line:
    // the string hit is dropped, and the call kept where it is.
    write_bytes(
        root,
        "invalid.rs",
        b"fn w() {\n    let s = \"\xff\xfe\xfd\xfc helper\"; helper();\n}\n",
    );
    let req_text = "fn z() { helper(); }\n";
    write(root, "req.rs", req_text);
    let (batches, summary) = find(
        &search(root, 1_000),
        &request(root, "req.rs", req_text, "helper"),
    );
    let refs = all(&batches);
    let on_disk: Vec<_> = (refs.iter())
        .filter(|r| r.location.rel != Path::new("req.rs"))
        .collect();
    // In cr.rs, rows 1, 2 and 3: each `\r` ends the `//` comment before it.
    assert_eq!(on_disk.len(), 11);
    assert_eq!(summary.dropped, 1, "invalid.rs's string");
    let invalid: Vec<_> = (on_disk.iter())
        .filter(|r| r.location.rel == Path::new("invalid.rs"))
        .map(|r| (r.location.row, r.verified))
        .collect();
    assert_eq!(invalid, [(1, true)]);
    let mut cr_rows = Vec::new();
    for r in on_disk {
        assert!(r.verified, "{r:?}");
        let text = loaded(root, &r.location.rel);
        assert_eq!(text.slice_to_string(r.location.range.clone()), "helper");
        assert_eq!(
            text.offset_to_point(r.location.range.start).row,
            r.location.row
        );
        if r.location.rel == Path::new("cr.rs") {
            cr_rows.push(r.location.row);
        } else {
            assert_eq!(r.location.row as u64 + 1, r.line_number);
        }
        let [in_preview] = &r.preview_ranges[..] else {
            panic!("one preview range: {r:?}");
        };
        assert_eq!(&r.preview[in_preview.clone()], "helper");
    }
    cr_rows.sort();
    assert_eq!(cr_rows, [1, 2, 3]);
}

#[test]
fn the_request_file_comes_first_from_unsaved_text() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "src/lib.rs", "fn a() {\n    helper();\n}\n");
    write(root, "src/other.rs", "pub fn helper() {}\n");
    let unsaved = "fn a() {\n    // edited\n    helper();\n    helper(1);\n}\n";
    let (batches, summary) = find(
        &search(root, 1_000),
        &request(root, "src/lib.rs", unsaved, "helper"),
    );
    let [first] = &batches[0][..] else {
        panic!("one file first: {batches:?}");
    };
    assert_eq!(first.rel, Path::new("src/lib.rs"));
    let ranges: Vec<_> = (first.refs.iter())
        .map(|r| (r.location.row, r.location.range.clone()))
        .collect();
    let at = |nth| unsaved.match_indices("helper").nth(nth).unwrap().0;
    assert_eq!(
        ranges,
        [(2, at(0)..at(0) + 6), (3, at(1)..at(1) + 6)],
        "offsets into the request's text"
    );
    assert!(first.refs.iter().all(|r| r.verified));
    assert_eq!(first.refs[1].preview, "    helper(1);");
    // The disk copy of the request's file isn't reported.
    assert_eq!(
        listed(&batches),
        ["src/lib.rs:2", "src/lib.rs:3", "src/other.rs:0 definition"]
    );
    assert_eq!(summary.files_verified, 2);
}

#[test]
fn unverified_past_the_budget_or_language() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for ix in 0..5 {
        write(
            root,
            &format!("f{ix}.rs"),
            "fn f() {\r\n    helper();\r\n} // helper\r\n",
        );
    }
    write(root, "notes.txt", "see helper\n\"helper\" // helper\n");
    let req_text = "fn helper() {}\n";
    write(root, "req.rs", req_text);
    let (batches, summary) = find(
        &search(root, 2),
        &request(root, "req.rs", req_text, "helper"),
    );

    let files: Vec<&FileReferences> = batches.iter().flatten().collect();
    let rs: Vec<_> = (files.iter())
        .filter(|file| file.rel.to_str().unwrap().starts_with('f'))
        .collect();
    assert_eq!(rs.len(), 5);
    let verified: Vec<_> = (rs.iter())
        .filter(|file| file.refs.iter().all(|r| r.verified))
        .collect();
    assert_eq!(verified.len(), 2, "{rs:?}");
    for file in &rs {
        let expected = if file.refs[0].verified { 1 } else { 2 };
        assert_eq!(file.refs.len(), expected, "{file:?}");
        assert!(
            file.refs
                .iter()
                .all(|r| r.verified == file.refs[0].verified)
        );
    }
    let txt = (files.iter())
        .find(|file| file.rel == Path::new("notes.txt"))
        .unwrap();
    assert_eq!(txt.refs.len(), 3);
    assert!(txt.refs.iter().all(|r| !r.verified && !r.is_definition));
    // Unverified ranges still index the loaded text.
    for r in all(&batches) {
        let text = loaded(root, &r.location.rel);
        assert_eq!(text.slice_to_string(r.location.range.clone()), "helper");
    }
    assert_eq!(summary.dropped, 2);
    assert_eq!(summary.files_verified, 3, "2 and the request's file");
}

#[test]
fn streams_and_cancels() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let files = 150;
    for ix in 0..files {
        write(
            root,
            &format!("src/m{ix:03}.rs"),
            "fn a() { helper(); }\nfn b() { helper(); }\n",
        );
    }
    let req_text = "fn helper() {}\n";
    write(root, "req.rs", req_text);
    let nav = search(root, 1_000);
    let req = request(root, "req.rs", req_text, "helper");

    // Whole files, each in one batch.
    let (batches, summary) = find(&nav, &req);
    assert!(batches.len() > 2, "{} batches", batches.len());
    let mut seen = HashSet::new();
    for file in batches.iter().flatten() {
        assert!(
            seen.insert(file.rel.clone()),
            "{} twice",
            file.rel.display()
        );
        let expected = if file.rel == Path::new("req.rs") {
            1
        } else {
            2
        };
        assert_eq!(file.refs.len(), expected);
    }
    assert_eq!(seen.len(), files + 1);
    assert!(!summary.cancelled && !summary.truncated);
    assert!(summary.files_searched > files);

    // A cancel in the first call, or the first batch of the search.
    for cancel_at in [1, 2] {
        let cancel = AtomicBool::new(false);
        let mut calls = 0;
        let mut reported = 0;
        let summary = nav
            .references(&req, &cancel, &mut |batch| {
                calls += 1;
                reported += batch.len();
                if calls == cancel_at {
                    cancel.store(true, Ordering::Relaxed);
                }
            })
            .unwrap();
        assert_eq!(calls, cancel_at);
        assert!(summary.cancelled);
        assert!(reported < files, "{reported} files reported");
    }
    let cancelled = AtomicBool::new(true);
    assert_eq!(
        nav.references(&req, &cancelled, &mut |_| panic!("no batch")),
        Err(NavError::Cancelled)
    );

    // Past the cap of 10,000 hits, the request's own included. Its disk
    // copy would count towards the search's, so it has none there.
    write(root, "req.rs", "\n");
    write(root, "big.txt", &"helper\n".repeat(10_000));
    let (batches, summary) = find(&nav, &req);
    assert!(summary.truncated);
    assert_eq!(all(&batches).len(), 10_000);
}

/// An exact provider whose references are one fixed batch.
struct Fake;

impl NavigationProvider for Fake {
    fn name(&self) -> &str {
        "fake"
    }

    fn precision(&self) -> Precision {
        Precision::Exact
    }

    fn readiness(&self, _language: Option<&str>) -> Readiness {
        Readiness::Ready
    }

    fn definitions(&self, _: &NavRequest, _: &AtomicBool) -> Result<Definitions, NavError> {
        Err(NavError::Unsupported)
    }

    fn references(
        &self,
        req: &NavRequest,
        _cancel: &AtomicBool,
        sink: &mut dyn FnMut(Vec<FileReferences>),
    ) -> Result<ReferencesSummary, NavError> {
        sink(vec![FileReferences {
            rel: req.rel.clone(),
            refs: Vec::new(),
        }]);
        Ok(ReferencesSummary {
            word: Some("exact".to_owned()),
            ..ReferencesSummary::default()
        })
    }

    fn workspace_symbols(
        &self,
        _: &str,
        _: usize,
        _: &AtomicBool,
    ) -> Result<Vec<WorkspaceSymbol>, NavError> {
        Err(NavError::Unsupported)
    }
}

/// A provider that keeps the trait's default `references`.
struct Defaulted;

impl NavigationProvider for Defaulted {
    fn name(&self) -> &str {
        "defaulted"
    }

    fn precision(&self) -> Precision {
        Precision::Exact
    }

    fn readiness(&self, _language: Option<&str>) -> Readiness {
        Readiness::Ready
    }

    fn definitions(&self, _: &NavRequest, _: &AtomicBool) -> Result<Definitions, NavError> {
        Err(NavError::Unsupported)
    }

    fn workspace_symbols(
        &self,
        _: &str,
        _: usize,
        _: &AtomicBool,
    ) -> Result<Vec<WorkspaceSymbol>, NavError> {
        Err(NavError::Unsupported)
    }
}

#[test]
fn router_routes_references() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let text = "fn helper() {}\n";
    write(root, "req.rs", text);
    let req = request(root, "req.rs", text, "helper");
    let run = |navigation: &Navigation| {
        let mut batches = Vec::new();
        let answer = navigation
            .references(&req, &AtomicBool::new(false), &mut |batch| {
                batches.push(batch)
            })
            .unwrap();
        (answer, batches)
    };

    let navigation = Navigation::new();
    navigation.add(Arc::new(search(root, 1_000)), 0);
    navigation.add(Arc::new(Fake), 10);
    let (answer, batches) = run(&navigation);
    assert_eq!(answer.provider, "fake");
    assert_eq!(answer.precision, Precision::Exact);
    assert_eq!(answer.value.word.as_deref(), Some("exact"));
    assert_eq!(batches.len(), 1);

    let navigation = Navigation::new();
    navigation.add(Arc::new(search(root, 1_000)), 0);
    navigation.add(Arc::new(Defaulted), 10);
    let (answer, batches) = run(&navigation);
    assert_eq!(answer.provider, SearchNavigation::NAME);
    assert_eq!(answer.precision, Precision::Search);
    assert_eq!(answer.value.word.as_deref(), Some("helper"));
    assert_eq!(listed(&batches), ["req.rs:0 definition"]);
}

#[test]
fn the_request_files_disk_copy_leaves_room() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // 6,000 hits in the request's text and on disk, 5,000 elsewhere: its
    // disk copy, which isn't reported, can't take the others' room.
    let text = "fn f() { helper(); }\n".repeat(6_000);
    write(root, "req.rs", &text);
    write(root, "other.txt", &"helper\n".repeat(5_000));
    let (batches, summary) = find(
        &search(root, 1_000),
        &request(root, "req.rs", &text, "helper"),
    );
    assert_eq!(all(&batches).len(), 10_000);
    assert!(summary.truncated);
}

#[test]
fn exactly_the_cap_is_not_truncated() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let text = "fn f() { helper(); }\n".repeat(10_000);
    write(root, "req.rs", &text);
    let (batches, summary) = find(
        &search(root, 1_000),
        &request(root, "req.rs", &text, "helper"),
    );
    assert_eq!(all(&batches).len(), 10_000);
    assert!(!summary.truncated);
    // One more use elsewhere is past it, though the request's file on disk
    // (ignored, so never searched) seemed to leave room.
    write(root, ".gitignore", "req.rs\n");
    write(root, "other.txt", "helper\n");
    let (batches, summary) = find(
        &search(root, 1_000),
        &request(root, "req.rs", &text, "helper"),
    );
    assert_eq!(all(&batches).len(), 10_000);
    assert!(summary.truncated);
}

#[test]
fn verified_only_when_parsed_as_loaded() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // Under Rust's parse limit on disk, over it once each invalid byte is a
    // 3-byte `\u{20AC}` (windows-1252's 0x80): its comment hit can't be
    // checked, so it's kept.
    let mut bytes = b"// helper\n".to_vec();
    bytes.extend(std::iter::repeat_n(0x80, 2_700_000));
    write_bytes(root, "big.rs", &bytes);
    let req_text = "fn helper() {}\n";
    write(root, "req.rs", req_text);
    let (batches, summary) = find(
        &search(root, 1_000),
        &request(root, "req.rs", req_text, "helper"),
    );
    let big: Vec<_> = (all(&batches).into_iter())
        .filter(|r| r.location.rel == Path::new("big.rs"))
        .map(|r| (r.location.range.clone(), r.verified))
        .collect();
    assert_eq!(big, [(3..9, false)]);
    assert_eq!(summary.dropped, 0);
    assert_eq!(summary.files_verified, 1, "the request's file alone");
}

#[test]
fn word_chars_as_the_search_has_them() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // A combining mark and connector punctuation are word chars, a
    // superscript digit isn't.
    let line = "helper\u{301} helper\u{2040}x helper\u{b2} helper\n";
    write(root, "other.txt", line);
    write(root, "req.rs", "\n");
    // The caret on the last, a plain `helper`.
    let (batches, _) = find(
        &search(root, 1_000),
        &request(root, "req.rs", line, "helper\n"),
    );
    let columns = |rel: &str| -> Vec<usize> {
        (all(&batches).into_iter())
            .filter(|r| r.location.rel == Path::new(rel))
            .map(|r| r.location.range.start)
            .collect()
    };
    assert_eq!(columns("req.rs"), columns("other.txt"));
    assert_eq!(columns("req.rs").len(), 2);
}

#[test]
fn dropped_hits_count_towards_the_cap() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // 10,000 hits in the request's text, all in comments; its disk copy is
    // ignored, so never searched. Another file's use is past the cap.
    let text = "// helper\n".repeat(10_000);
    write(root, "req.rs", &text);
    write(root, ".gitignore", "req.rs\n");
    write(root, "other.txt", "helper\n");
    let (batches, summary) = find(
        &search(root, 1_000),
        &request(root, "req.rs", &text, "helper"),
    );
    // Each comment hit is dropped, or kept unverified if its highlights
    // didn't come in time (a loaded machine).
    let refs = all(&batches);
    assert!(
        refs.iter()
            .all(|r| r.location.rel == Path::new("req.rs") && !r.verified)
    );
    assert_eq!(summary.dropped + refs.len(), 10_000);
    assert!(summary.truncated);
}

#[test]
fn utf16_hits_are_located() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend("x x x\n".encode_utf16().flat_map(u16::to_le_bytes));
    write_bytes(root, "wide.rs", &bytes);
    let req_text = "fn x() {}\n";
    write(root, "req.rs", req_text);
    let (batches, _) = find(
        &search(root, 1_000),
        &request(root, "req.rs", req_text, "x()"),
    );
    let wide: Vec<_> = (all(&batches).into_iter())
        .filter(|r| r.location.rel == Path::new("wide.rs"))
        .collect();
    assert_eq!(wide.len(), 3);
    let text = Buffer::from_reader_detecting(bytes.as_slice()).unwrap();
    for r in &wide {
        let range = r.location.range.clone();
        assert_eq!(text.text().slice_to_string(range), "x", "{r:?}");
    }
}

#[test]
fn overlapping_candidates_as_the_search_has_them() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // `$$` is a JavaScript word; in `a$$$`, the candidate at 5 isn't whole
    // and the one at 6, overlapping it, is.
    let text = "$$; a$$$;\n";
    write(root, "other.js", text);
    let req = NavRequest {
        root: root.to_path_buf(),
        rel: PathBuf::from("req.js"),
        language: Some("javascript"),
        text: Text::new(text),
        offset: 0,
        buffer: None,
    };
    let (batches, _) = find(&search(root, 1_000), &req);
    let starts = |rel: &str| -> Vec<usize> {
        (all(&batches).into_iter())
            .filter(|r| r.location.rel == Path::new(rel))
            .map(|r| r.location.range.start)
            .collect()
    };
    assert_eq!(starts("other.js"), [0, 6]);
    assert_eq!(starts("req.js"), starts("other.js"));
}

#[test]
// Swaps a folder for a symlink, which Windows makes only with a privilege.
#[cfg(unix)]
fn files_are_never_read_through_a_directory_being_swapped() {
    let dir = tempfile::tempdir().unwrap();
    let dir = dunce::canonicalize(dir.path()).unwrap();
    let (root, outside) = (dir.join("project"), dir.join("outside"));
    // The same bytes, the word in code inside and in a comment outside:
    // a hit read from outside would be dropped.
    write(&root, "src/a/x.rs", "fn f() { helper(); }\n");
    write(&outside, "x.rs", "// f() { helper(); }\n");
    let req_text = "fn helper() {}\n";
    write(&root, "req.rs", req_text);
    let (a, aside) = (root.join("src/a"), root.join("src/a.aside"));
    let nav = search(&root, 1_000);
    let req = request(&root, "req.rs", req_text, "helper");
    let done = AtomicBool::new(false);
    // The handshake with the swapper: `hold` asks it to stop between swaps,
    // and it sets `parked` once it has, with `src/a` in place.
    let (hold, parked) = (AtomicBool::new(false), AtomicBool::new(false));
    std::thread::scope(|scope| {
        let _stop = Stop(&done);
        let swapper = scope.spawn(|| {
            while !done.load(Ordering::Relaxed) {
                if hold.load(Ordering::Acquire) {
                    parked.store(true, Ordering::Release);
                    std::thread::yield_now();
                    continue;
                }
                parked.store(false, Ordering::Release);
                fs::rename(&a, &aside).unwrap();
                std::os::unix::fs::symlink(&outside, &a).unwrap();
                fs::remove_file(&a).unwrap();
                fs::rename(&aside, &a).unwrap();
            }
        });
        // Safety: searches racing the swaps never read from outside.
        for _ in 0..60 {
            let (_, summary) = find(&nav, &req);
            assert_eq!(summary.dropped, 0, "read from outside");
        }
        // Liveness: a search with the directory known to be in place.
        hold.store(true, Ordering::Release);
        while !parked.load(Ordering::Acquire) {
            assert!(!swapper.is_finished(), "the swapper died");
            std::thread::yield_now();
        }
        let (batches, summary) = find(&nav, &req);
        assert_eq!(summary.dropped, 0, "read from outside");
        let verified = (all(&batches).into_iter())
            .filter(|r| r.location.rel == Path::new("src/a/x.rs") && r.verified)
            .count();
        assert_eq!(verified, 1, "{:?}", listed(&batches));
        hold.store(false, Ordering::Release);
    });
}

/// Sets its flag when dropped.
#[cfg(unix)]
struct Stop<'a>(&'a AtomicBool);

#[cfg(unix)]
impl Drop for Stop<'_> {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

#[test]
fn the_word_at_the_caret_has_the_searchs_word_chars() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // A combining mark is a word char: the word is all of `cafe\u{301}`.
    let text = "let cafe\u{301} = 1;\n";
    write(root, "req.rs", text);
    let (batches, summary) = find(&search(root, 1_000), &request(root, "req.rs", text, "cafe"));
    assert_eq!(summary.word.as_deref(), Some("cafe\u{301}"));
    assert_eq!(listed(&batches), ["req.rs:0"]);
}
