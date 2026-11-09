//! Grammars loaded at run time from WebAssembly ([`register_language`]): the
//! JSON fixture, registered as `json_wasm`, highlights as the compiled-in
//! JSON does on any thread, is found after the built-ins, and bad grammars are
//! errors naming their file: one whose code never returns is stopped at its
//! deadline, and a mistyped module is refused without aborting.

#![cfg(feature = "json")]

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, Once, OnceLock, mpsc},
    time::{Duration, Instant},
};

use fe_buffer::{Buffer, Origin};
use fe_syntax::{
    Clash, GrammarDeadlines, GrammarError, GrammarSpec, HighlightSpan, Language, MAX_GRAMMAR_BYTES,
    Syntax, language_by_name, language_name_for_path, language_names, register_language,
    set_grammar_deadlines,
};

fn fixture() -> PathBuf {
    fixture_file("tree-sitter-json.wasm")
}

fn fixture_file(name: &str) -> PathBuf {
    // Read at run time: a path compiled in can name a deleted snapshot.
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR").expect("run by cargo");
    Path::new(&manifest).join("tests/fixtures").join(name)
}

/// Deadlines short enough for the looping grammars to stop soon, and long
/// enough for the JSON fixture's load (a few milliseconds) on a loaded
/// machine. Every test sets them before loading anything.
const LOAD_DEADLINE: Duration = Duration::from_secs(10);
const PARSE_DEADLINE: Duration = Duration::from_millis(1500);

fn short_deadlines() {
    static SET: Once = Once::new();
    SET.call_once(|| {
        set_grammar_deadlines(GrammarDeadlines {
            load: LOAD_DEADLINE,
            parse: PARSE_DEADLINE,
        });
    });
}

/// `f`'s result on another thread, or a failure if it takes over 30 s,
/// as code nothing stops would.
fn bounded<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || send.send(f()));
    receive
        .recv_timeout(Duration::from_secs(30))
        .expect("stopped by its deadline")
}

/// This process's directory for the queries and bad modules, removed when
/// the last test holding it finishes.
struct Scratch(PathBuf);

static HELD: Mutex<usize> = Mutex::new(0);

fn scratch() -> Scratch {
    let mut held = HELD.lock().unwrap();
    let dir =
        std::env::temp_dir().join(format!("fe-syntax-runtime-grammar-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    *held += 1;
    Scratch(dir)
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let mut held = HELD.lock().unwrap();
        *held -= 1;
        if *held == 0 {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}

impl std::ops::Deref for Scratch {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

fn write(scratch: &Scratch, name: &str, contents: impl AsRef<[u8]>) -> PathBuf {
    let path = scratch.join(name);
    fs::write(&path, contents).unwrap();
    path
}

fn json_wasm() -> &'static Language {
    static LANGUAGE: OnceLock<&'static Language> = OnceLock::new();
    short_deadlines();
    LANGUAGE.get_or_init(|| {
        let scratch = scratch();
        register_language(&GrammarSpec {
            language: "json_wasm".into(),
            wasm: fixture(),
            extensions: vec!["jsonw".into()],
            highlights: Some(write(
                &scratch,
                "highlights.scm",
                tree_sitter_json::HIGHLIGHTS_QUERY,
            )),
            earlier_patterns_win: true,
            ..GrammarSpec::default()
        })
        .unwrap()
    })
}

const TEXT: &str = r#"{
  "name": "fastedit",
  "numbers": [1, -2.5e3, 0],
  "flags": { "on": true, "off": false, "none": null },
  // a comment
  "escaped": "a\"b\n",
  "broken": [1,
}
"#;

fn parsed_here(language: &'static Language, buffer: &Buffer) -> Vec<HighlightSpan> {
    let mut syntax = Syntax::new(language, &buffer.snapshot());
    assert!(syntax.parse_within(Duration::from_secs(10)));
    syntax.highlights(0..buffer.snapshot().text().len())
}

/// Parsed by a job on another thread, then edited and reparsed there
/// incrementally, as the driver does.
fn parsed_elsewhere(language: &'static Language, buffer: &mut Buffer) -> Vec<HighlightSpan> {
    let mut syntax = Syntax::new(language, &buffer.snapshot());
    let job = syntax.parse_job();
    syntax.finish(std::thread::spawn(move || job.run()).join().unwrap());
    buffer.edit([(4..4, "\"added\": [true],\n  ")], Origin::User);
    syntax.edit(&buffer.snapshot());
    let job = syntax.parse_job();
    syntax.finish(std::thread::spawn(move || job.run()).join().unwrap());
    assert!(!syntax.needs_parse() && !syntax.failed());
    syntax.highlights(0..buffer.snapshot().text().len())
}

#[test]
fn a_wasm_grammar_highlights_as_the_compiled_one() {
    let wasm = json_wasm();
    let json = language_by_name("json").unwrap();
    assert_eq!(wasm.name(), "json_wasm");

    let buffer = Buffer::new(TEXT);
    let expected = parsed_here(json, &buffer);
    assert!(expected.len() > 10, "{expected:?}");
    assert_eq!(parsed_here(wasm, &buffer), expected);

    let mut json_buffer = Buffer::new(TEXT);
    let mut wasm_buffer = Buffer::new(TEXT);
    let expected = parsed_elsewhere(json, &mut json_buffer);
    assert_eq!(parsed_elsewhere(wasm, &mut wasm_buffer), expected);
}

#[test]
fn runtime_languages_are_found_after_built_ins() {
    let wasm = json_wasm();
    assert_eq!(
        language_name_for_path(Path::new("dir/x.jsonw")),
        Some("json_wasm")
    );
    assert_eq!(
        language_name_for_path(Path::new("dir/X.JSONW")),
        Some("json_wasm")
    );
    assert!(std::ptr::eq(language_by_name("json_wasm").unwrap(), wasm));
    assert!(language_names().any(|name| name == "json_wasm"));
    assert_eq!(
        language_name_for_path(Path::new("dir/x.json")),
        Some("json")
    );

    let clash = |spec: GrammarSpec| match register_language(&spec) {
        Err(GrammarError::Clash {
            built_in, clash, ..
        }) => (built_in, clash),
        other => panic!("{other:?}"),
    };
    let spec = GrammarSpec {
        language: "JSON".into(),
        wasm: fixture(),
        extensions: vec!["jsonu".into()],
        ..GrammarSpec::default()
    };
    assert_eq!(clash(spec), ("json", Clash::Name));
    #[cfg(feature = "rust")]
    {
        let spec = GrammarSpec {
            language: "rust".into(),
            wasm: fixture(),
            extensions: vec!["rsw".into()],
            ..GrammarSpec::default()
        };
        assert_eq!(clash(spec), ("rust", Clash::Name));
    }
    let spec = GrammarSpec {
        language: "json_other".into(),
        wasm: fixture(),
        extensions: vec!["jsonv".into(), "JSON".into()],
        ..GrammarSpec::default()
    };
    assert_eq!(clash(spec), ("json", Clash::Extension("JSON".into())));
    assert!(language_by_name("json_other").is_none());
    assert_eq!(
        language_name_for_path(Path::new("dir/x.json")),
        Some("json")
    );
    assert_eq!(language_name_for_path(Path::new("dir/x.jsonv")), None);

    // The same files again are the language already loaded.
    let scratch = scratch();
    let again = GrammarSpec {
        language: "json_wasm".into(),
        wasm: fixture(),
        extensions: vec!["jsonw".into()],
        highlights: Some(write(
            &scratch,
            "highlights.scm",
            tree_sitter_json::HIGHLIGHTS_QUERY,
        )),
        earlier_patterns_win: true,
        ..GrammarSpec::default()
    };
    assert!(std::ptr::eq(register_language(&again).unwrap(), wasm));
}

/// The JSON fixture registered as `name`, for files with `extensions`.
fn json_fixture_as(name: &str, extensions: &[&str]) {
    short_deadlines();
    register_language(&GrammarSpec {
        language: name.into(),
        wasm: fixture(),
        extensions: extensions
            .iter()
            .map(|&extension| extension.into())
            .collect(),
        ..GrammarSpec::default()
    })
    .unwrap();
}

#[test]
fn a_compound_extension_wins_over_a_plain_one() {
    json_wasm();
    // Neither clashes with the built-in json: claims compare whole.
    json_fixture_as("zz_blade", &["blade.json", "Blade.JSONW"]);
    json_fixture_as("zz_long_blade", &["long.blade.json"]);
    let name = |path: &str| language_name_for_path(Path::new(path));

    // Over a built-in's plain extension and a registered one's, ignoring case.
    assert_eq!(name("views/x.blade.json"), Some("zz_blade"));
    assert_eq!(name("views/X.BLADE.Json"), Some("zz_blade"));
    assert_eq!(name("views/x.blade.jsonw"), Some("zz_blade"));
    // The longest compound one wins.
    assert_eq!(name("views/x.long.blade.json"), Some("zz_long_blade"));
    assert_eq!(name("views/long.blade.json"), Some("zz_blade"));
    // A file named just the compound extension, or a dot and it, hasn't got it.
    assert_eq!(name("views/blade.json"), Some("json"));
    assert_eq!(name("views/.blade.json"), Some("json"));
    assert_eq!(name("views/blade.jsonw"), Some("json_wasm"));
    // Plain extensions are as before.
    assert_eq!(name("views/x.json"), Some("json"));
    assert_eq!(name("views/x.jsonw"), Some("json_wasm"));
    assert_eq!(name("views/x.blade"), None);

    // Who a claim made now would take files from.
    let would_take =
        |path: &str, extension| Language::extension_would_take(Path::new(path), extension);
    assert!(would_take("x.new.blade.json", "new.blade.json"));
    assert!(would_take("x.y.jsonw", "Y.jsonw"));
    assert!(!would_take("x.blade.json", "BLADE.json"));
    assert!(!would_take("x.long.blade.json", "blade.json"));
    assert!(!would_take("x.json", "json"));
    assert!(!would_take("x.jsonw", "jsonw"));
    assert!(would_take("x.jsonz", "jsonz"));
    assert!(!would_take("blade.jsonz", "blade.jsonz"));

    assert!(Language::file_has_extension("x.blade.php", "blade.php"));
    assert!(Language::file_has_extension("x.blade.php", "php"));
    assert!(!Language::file_has_extension("x.blade.php", "x.blade.php"));
    assert!(!Language::file_has_extension("x.blade.php", "lade.php"));
    assert!(!Language::file_has_extension("x.blade.php", ".php"));
    assert!(Language::file_has_extension("é.blade.php", "BLADE.php"));
    assert!(!Language::file_has_extension("é.php", "x.php"));
}

#[test]
fn bad_grammars_are_errors() {
    short_deadlines();
    let spec = |name: &str, wasm: PathBuf| GrammarSpec {
        language: name.into(),
        wasm,
        extensions: vec![format!("{name}x")],
        ..GrammarSpec::default()
    };

    let scratch = scratch();
    let garbage = write(
        &scratch,
        "tree-sitter-json-garbage.wasm",
        b"\0asm not really",
    );
    match register_language(&spec("garbage", garbage.clone())) {
        Err(error @ GrammarError::Module { .. }) => {
            assert_eq!(error.file(), Some(garbage.as_path()));
            assert!(
                error
                    .to_string()
                    .starts_with(&garbage.display().to_string())
            );
        }
        other => panic!("{other:?}"),
    }

    let huge = scratch.join("tree-sitter-huge.wasm");
    fs::File::create(&huge)
        .unwrap()
        .set_len(MAX_GRAMMAR_BYTES + 1)
        .unwrap();
    let result = register_language(&spec("huge", huge.clone()));
    fs::remove_file(&huge).unwrap();
    match result {
        Err(GrammarError::TooLarge { file, bytes }) => {
            assert_eq!(file, huge);
            assert_eq!(bytes, MAX_GRAMMAR_BYTES + 1);
        }
        other => panic!("{other:?}"),
    }

    let query = write(
        &scratch,
        "bad-highlights.scm",
        "(string) @string\n\n  (pair key: (no_such_node)) @keyword\n",
    );
    let mut bad_query = spec("bad_query", fixture());
    bad_query.highlights = Some(query.clone());
    match register_language(&bad_query) {
        Err(GrammarError::Query { file, error }) => {
            assert_eq!(file, query);
            assert_eq!((error.row, error.column), (2, 14), "{error}");
        }
        other => panic!("{other:?}"),
    }

    let missing = scratch.join("tree-sitter-missing.wasm");
    match register_language(&spec("missing", missing.clone())) {
        Err(error @ GrammarError::Read { .. }) => assert_eq!(error.file(), Some(missing.as_path())),
        other => panic!("{other:?}"),
    }
    for name in ["garbage", "huge", "bad_query", "missing"] {
        assert!(language_by_name(name).is_none(), "{name}");
    }
}

#[test]
fn a_looping_grammar_is_stopped_at_its_deadline() {
    short_deadlines();
    let looping_init = fixture_file("tree-sitter-loopinit.wasm");
    let spec = GrammarSpec {
        language: "loopinit".into(),
        wasm: looping_init.clone(),
        extensions: vec!["loopinit".into()],
        ..GrammarSpec::default()
    };
    let started = Instant::now();
    match bounded(move || register_language(&spec)) {
        Err(error @ GrammarError::Deadline { .. }) => {
            assert_eq!(error.file(), Some(looping_init.as_path()));
        }
        other => panic!("{other:?}"),
    }
    assert!(started.elapsed() >= LOAD_DEADLINE);
    assert!(language_by_name("loopinit").is_none());

    // Its scanner never returns, so every parse stops at the deadline, on
    // the thread's cached parser; the same parser then parses JSON.
    let looping = register_language(&GrammarSpec {
        language: "loop".into(),
        wasm: fixture_file("tree-sitter-loop.wasm"),
        extensions: vec!["loop".into()],
        ..GrammarSpec::default()
    })
    .unwrap();
    let json = json_wasm();
    let (job, here, highlights) = bounded(move || {
        let buffer = Buffer::new("some words\n");
        // A job's cap (5 s) is longer than the parse deadline.
        let mut syntax = Syntax::new(looping, &buffer.snapshot());
        let started = Instant::now();
        syntax.finish(syntax.parse_job().run());
        let job = (syntax.failed(), started.elapsed());
        // A parse with a budget shorter than the deadline stops at the budget.
        let mut syntax = Syntax::new(looping, &buffer.snapshot());
        let started = Instant::now();
        let parsed = syntax.parse_within(Duration::from_millis(20));
        let here = (parsed, started.elapsed());
        (job, here, parsed_here(json, &Buffer::new(TEXT)))
    });
    assert!(job.0, "the parse job gave up");
    assert!(
        (PARSE_DEADLINE..Duration::from_secs(5)).contains(&job.1),
        "stopped at the deadline: {:?}",
        job.1
    );
    assert!(!here.0, "the parse on this thread gave up");
    assert!(
        here.1 < PARSE_DEADLINE / 2,
        "stopped at its budget: {:?}",
        here.1
    );
    let expected = parsed_here(language_by_name("json").unwrap(), &Buffer::new(TEXT));
    assert_eq!(highlights, expected);
}

/// An injection of the looping grammar stops at the parse deadline, each
/// time it's tried, and the host still highlights.
#[cfg(feature = "markdown")]
#[test]
fn a_looping_injection_is_stopped_at_its_deadline() {
    short_deadlines();
    let looping = || {
        register_language(&GrammarSpec {
            language: "loop".into(),
            wasm: fixture_file("tree-sitter-loop.wasm"),
            extensions: vec!["loop".into()],
            ..GrammarSpec::default()
        })
        .unwrap()
    };
    looping();
    let highlighted = bounded(|| {
        let buffer = Buffer::new("# Title\n\n```loop\nsome words\n```\n\nText.\n");
        let mut syntax = Syntax::new(language_by_name("markdown").unwrap(), &buffer.snapshot());
        assert!(syntax.parse_within(Duration::from_secs(10)));
        let len = buffer.snapshot().text().len();
        (syntax.highlights(0..len), syntax.highlights(0..len))
    });
    assert!(!highlighted.0.is_empty());
    assert_eq!(highlighted.0, highlighted.1);
}

/// Grammars whose code calls back into the host wrongly, or returns tokens
/// they don't have, are errors or failed parses, not crashes.
#[test]
fn hostile_grammars_are_errors_not_crashes() {
    short_deadlines();
    let spec = |name: &str| GrammarSpec {
        language: name.into(),
        wasm: fixture_file(&format!("tree-sitter-{name}.wasm")),
        extensions: vec![name.into()],
        ..GrammarSpec::default()
    };
    // Their initializers call the host: a lexer function while no lexer
    // runs, and a debug message whose string is outside the module's memory.
    for (name, message) in [
        ("lexnull", "lexer function called outside a lex"),
        ("debugmsg", "debug message outside Wasm memory"),
    ] {
        let wasm = fixture_file(&format!("tree-sitter-{name}.wasm"));
        let spec = spec(name);
        match bounded(move || register_language(&spec)) {
            Err(error @ GrammarError::Module { .. }) => {
                assert_eq!(error.file(), Some(wasm.as_path()));
                assert!(error.to_string().contains(message), "{error}");
            }
            other => panic!("{name}: {other:?}"),
        }
    }
    // A token past the grammar's own, from its scanner, its main lexer and
    // its keyword lexer.
    for (name, text) in [
        ("badscan", "some words\n"),
        ("badlex", "some words\n"),
        ("badkeyword", "let it be\n"),
    ] {
        let language = register_language(&spec(name)).unwrap();
        let failed = bounded(move || {
            let buffer = Buffer::new(text);
            let mut syntax = Syntax::new(language, &buffer.snapshot());
            syntax.finish(syntax.parse_job().run());
            syntax.failed()
        });
        assert!(failed, "{name}");
    }
    // And JSON still parses, on a fresh thread and this one.
    let json = json_wasm();
    let expected = parsed_here(language_by_name("json").unwrap(), &Buffer::new(TEXT));
    assert_eq!(
        bounded(move || parsed_here(json, &Buffer::new(TEXT))),
        expected
    );
    assert_eq!(parsed_here(json, &Buffer::new(TEXT)), expected);
}

#[test]
fn a_bad_export_is_an_error_not_an_abort() {
    short_deadlines();
    // An export: its name's length and bytes, its kind (0, a function),
    // then the function's index.
    let bytes = fs::read(fixture()).unwrap();
    let name = b"\x10tree_sitter_json\x00";
    let at = bytes
        .windows(name.len())
        .position(|window| window == name)
        .expect("the fixture exports tree_sitter_json")
        + name.len();
    let scratch = scratch();
    let load = |dir: &str, module: Vec<u8>| {
        // The file's name says the grammar is `json`.
        fs::create_dir_all(scratch.join(dir)).unwrap();
        let wasm = write(&scratch, &format!("{dir}/tree-sitter-json.wasm"), module);
        let spec = GrammarSpec {
            language: format!("json_{dir}"),
            wasm: wasm.clone(),
            extensions: vec![format!("json{dir}")],
            ..GrammarSpec::default()
        };
        match bounded(move || register_language(&spec)) {
            Err(error @ GrammarError::Module { .. }) => {
                assert_eq!(error.file(), Some(wasm.as_path()));
                error.to_string()
            }
            other => panic!("{other:?}"),
        }
    };

    // Another function, which takes or returns something else.
    let mut mistyped = bytes.clone();
    assert_eq!(mistyped[at], 1, "a one-byte index");
    mistyped[at] = 0;
    let error = load("mistyped", mistyped);
    assert!(error.contains("error calling language function"), "{error}");

    let mut missing = bytes.clone();
    missing[at - 2] = b'x';
    let error = load("missing", missing);
    assert!(error.contains("tree_sitter_json"), "{error}");
    assert!(language_by_name("json_mistyped").is_none());

    // And loading carries on.
    assert_eq!(json_wasm().name(), "json_wasm");
}

/// The budget the finish notes record: run with
/// `cargo test --release -p fe-syntax --test runtime_grammar -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement, not a check"]
fn wasm_grammar_timings() {
    let started = Instant::now();
    let wasm = json_wasm();
    println!("first load: {:?}", started.elapsed());

    let mut text = String::from("[\n");
    for row in 0..9_998 {
        text.push_str(&format!(
            "  {{ \"id\": {row}, \"name\": \"item {row}\", \"on\": true }},\n"
        ));
    }
    text.push_str("  null\n]\n");
    for language in [language_by_name("json").unwrap(), wasm] {
        let mut buffer = Buffer::new(&text);
        let mut syntax = Syntax::new(language, &buffer.snapshot());
        let started = Instant::now();
        assert!(syntax.parse_within(Duration::from_secs(10)));
        let full = started.elapsed();
        let mut reparses = Vec::new();
        for round in 0..20 {
            let offset = buffer
                .snapshot()
                .text()
                .point_to_offset(fe_buffer::Point::new(5_000, 10));
            buffer.edit([(offset..offset, format!("{round}"))], Origin::User);
            syntax.edit(&buffer.snapshot());
            let started = Instant::now();
            assert!(syntax.parse_within(Duration::from_secs(10)));
            reparses.push(started.elapsed());
        }
        reparses.sort();
        println!(
            "{}: full parse {full:?}, one-row reparse median {:?}, max {:?}",
            language.name(),
            reparses[reparses.len() / 2],
            reparses.last().unwrap()
        );
    }
}
