//! Document outlines from the outline queries in `queries/outline/`.
//!
//! Each sample in `tests/outline_samples/`, named after its language, is
//! outlined whole and compared with `<sample>.outline` next to it: one line
//! per item, indented two spaces per depth, `<kind> "<name>" <range>` with
//! the item's range as `row:col-row:col`, 1-based rows and byte columns.
//! `UPDATE_GOLDEN=1 cargo nextest run -p fe-syntax --test outline` rewrites
//! them; review the diff before committing it.
//!
//! Built only with every outlined language, and HTML for an unsupported one.

#![cfg(all(
    feature = "rust",
    feature = "python",
    feature = "javascript",
    feature = "typescript",
    feature = "go",
    feature = "java",
    feature = "c",
    feature = "markdown",
    feature = "toml",
    feature = "html"
))]

use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use fe_buffer::{Buffer, Origin};
use fe_syntax::{
    MAX_OUTLINE_ITEMS, NoOutline, Outline, OutlineItem, OutlineJob, OutlineKind, Syntax,
    language_by_name, language_for_path, language_names, outline, outline_job, outline_supported,
};

const SUFFIX: &str = ".outline";

/// Read at run time: the shared cargo target can keep a compile-time path
/// to a snapshot that has since been deleted.
fn samples_dir() -> PathBuf {
    Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("tests/outline_samples")
}

/// Every sample whose language is in this build, with its language name and
/// text.
fn samples() -> Vec<(PathBuf, &'static str, String)> {
    let mut paths: Vec<PathBuf> = fs::read_dir(samples_dir())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| !path.to_string_lossy().ends_with(SUFFIX))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .filter_map(|path| {
            let language = language_for_path(&path)?;
            let text = fs::read_to_string(&path).unwrap();
            Some((path, language.name(), text))
        })
        .collect()
}

/// `text` parsed whole as `language` and outlined.
fn outline_of(language: &str, text: &str) -> Outline {
    let buffer = Buffer::new(text);
    let mut syntax = Syntax::new(language_by_name(language).unwrap(), buffer.current());
    // No deadline: a loaded machine can't make the parse give up.
    assert!(syntax.parse_until(Duration::MAX, || false));
    assert!(
        !syntax.tree().unwrap().root_node().has_error(),
        "the {language} sample parses without errors"
    );
    outline(&syntax).unwrap()
}

fn sample(language: &str) -> Outline {
    let (_, _, text) = samples()
        .into_iter()
        .find(|(_, name, _)| *name == language)
        .unwrap();
    outline_of(language, &text)
}

/// The one item named `name`.
fn item<'a>(outline: &'a Outline, name: &str) -> &'a OutlineItem {
    let mut found = outline.items.iter().filter(|item| item.name == name);
    let item = found.next().unwrap_or_else(|| panic!("no item {name:?}"));
    assert!(found.next().is_none(), "one item {name:?}");
    item
}

/// The kind and name of an item's parent.
fn parent(outline: &Outline, item: &OutlineItem) -> Option<(OutlineKind, String)> {
    let parent = &outline.items[item.parent? as usize];
    Some((parent.kind, parent.name.clone()))
}

fn render(outline: &Outline, text: &str) -> String {
    let buffer = Buffer::new(text);
    let point = |offset| {
        let point = buffer.text().offset_to_point(offset);
        format!("{}:{}", point.row + 1, point.column + 1)
    };
    let mut out = String::new();
    for item in &outline.items {
        let indent = "  ".repeat(item.depth as usize);
        let (start, end) = (point(item.range.start), point(item.range.end));
        writeln!(out, "{indent}{:?} {:?} {start}-{end}", item.kind, item.name).unwrap();
    }
    if outline.truncated {
        out.push_str("truncated\n");
    }
    out
}

#[test]
fn outlines_match_golden_files() {
    let update = std::env::var_os("UPDATE_GOLDEN").is_some();
    let mut covered = Vec::new();
    let mut failures = Vec::new();
    for (path, language, text) in samples() {
        let file_name = path.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(
            path.file_stem().unwrap().to_string_lossy(),
            language,
            "{file_name} is named after its language"
        );
        covered.push(language);
        let actual = render(&outline_of(language, &text), &text);
        let golden = path.with_file_name(format!("{file_name}{SUFFIX}"));
        if update {
            fs::write(&golden, &actual).unwrap();
            continue;
        }
        let expected = fs::read_to_string(&golden).unwrap_or_default();
        if expected != actual {
            failures.push(format!(
                "{file_name}{SUFFIX}\n--- expected\n{expected}--- actual\n{actual}"
            ));
        }
    }
    let mut supported: Vec<_> = language_names()
        .filter(|name| outline_supported(name))
        .collect();
    supported.sort_unstable();
    // The languages T113 added have their samples, and goldens, in
    // more_languages_samples/, which tests/more_languages.rs checks; Bash,
    // Make and SQL have theirs in outline_scripts_samples/, which
    // tests/outline_scripts.rs checks, and JSON, YAML, CSS and Dockerfile
    // in outline_data_samples/, which tests/outline_data.rs checks.
    for dir in [
        "more_languages_samples",
        "outline_scripts_samples",
        "outline_data_samples",
    ] {
        covered.extend(
            fs::read_dir(samples_dir().with_file_name(dir))
                .unwrap()
                .filter_map(|entry| language_for_path(&entry.unwrap().path()))
                .map(|language| language.name()),
        );
    }
    covered.sort_unstable();
    assert_eq!(covered, supported, "every outlined language has a sample");
    assert!(
        failures.is_empty(),
        "outlines differ from the golden files. If the change is intended, rerun with \
         UPDATE_GOLDEN=1 and review the diff.\n\n{}",
        failures.join("\n")
    );
}

#[test]
fn rust_items_nest_under_impls_traits_and_modules() {
    use OutlineKind::*;
    let outline = sample("rust");
    let module = Some((Module, "shapes".to_owned()));
    for (name, kind) in [("Kind", Enum), ("Draw", Trait), ("Bits", Struct)] {
        let item = item(&outline, name);
        assert_eq!(item.kind, kind, "{name}");
        assert_eq!(parent(&outline, item), module, "{name}");
    }
    let foos: Vec<_> = outline.items.iter().filter(|i| i.name == "Foo").collect();
    assert_eq!(foos.len(), 2);
    assert_eq!(foos[0].kind, Struct);
    assert_eq!(parent(&outline, foos[0]), module);
    assert_eq!(parent(&outline, foos[1]), None);

    let display = item(&outline, "impl Display for Foo");
    assert_eq!(display.kind, Impl);
    let fmt = item(&outline, "fmt");
    assert_eq!(fmt.kind, Method);
    assert_eq!(parent(&outline, fmt), Some((Impl, display.name.clone())));
    assert_eq!(
        item(&outline, "impl<T: Clone> shapes::Draw for shapes::Foo<T>").kind,
        Impl
    );

    let draws: Vec<_> = outline.items.iter().filter(|i| i.name == "draw").collect();
    assert_eq!(draws.len(), 2, "the trait's signature and the impl's fn");
    for draw in draws {
        assert_eq!(draw.kind, Method);
    }
    let signature = outline
        .items
        .iter()
        .find(|i| i.name == "draw" && i.depth == 2)
        .unwrap();
    assert_eq!(
        parent(&outline, signature),
        Some((Trait, "Draw".to_owned()))
    );
    assert_eq!(item(&outline, "name").kind, Method);
    let output = item(&outline, "Output");
    assert_eq!(output.kind, Type);
    assert_eq!(parent(&outline, output), Some((Trait, "Draw".to_owned())));

    // A signature is a Method in an impl, and a Function elsewhere. The
    // impl's matches both patterns but is listed once.
    let declared = item(&outline, "declared");
    assert_eq!(declared.kind, Method);
    assert_eq!(
        parent(&outline, declared),
        Some((Impl, "impl Foo".to_owned()))
    );
    let abs = item(&outline, "abs");
    assert_eq!(abs.kind, Function);
    assert_eq!(parent(&outline, abs), None);

    let helper = item(&outline, "helper");
    assert_eq!(helper.kind, Function);
    assert_eq!(parent(&outline, helper).unwrap().1, "draw");
    assert_eq!(item(&outline, "new").kind, Method);
    assert_eq!(item(&outline, "main").kind, Function);
    assert_eq!(item(&outline, "Pair").kind, Type);
    assert_eq!(item(&outline, "LIMIT").kind, Constant);
    assert_eq!(item(&outline, "GREETING").kind, Constant);
    assert_eq!(item(&outline, "square").kind, Macro);

    // fmt matches both the method and the function pattern, but is listed
    // once.
    let fmt_items = outline.items.iter().filter(|i| i.range == fmt.range);
    assert_eq!(fmt_items.count(), 1);
}

#[test]
fn python_and_typescript_methods_nest_in_classes() {
    use OutlineKind::*;
    let python = sample("python");
    let shape = Some((Class, "Shape".to_owned()));
    for name in ["__init__", "area", "perimeter"] {
        let method = item(&python, name);
        assert_eq!(method.kind, Method, "{name}");
        assert_eq!(parent(&python, method), shape, "{name}");
    }
    let side = item(&python, "side");
    assert_eq!(side.kind, Function);
    assert_eq!(
        parent(&python, side),
        Some((Method, "perimeter".to_owned()))
    );
    let inner = item(&python, "inner");
    assert_eq!(inner.kind, Function);
    assert_eq!(parent(&python, inner), Some((Function, "build".to_owned())));
    assert_eq!(item(&python, "build").kind, Function);
    assert_eq!(
        parent(&python, item(&python, "describe")),
        Some((Class, "Meta".to_owned()))
    );
    for name in ["MAX_SIZE", "API_V2"] {
        assert_eq!(item(&python, name).kind, Constant, "{name}");
    }
    for name in ["default_name", "SIDES", "LOCAL"] {
        assert!(!python.items.iter().any(|i| i.name == name), "{name}");
    }

    let typescript = sample("typescript");
    for (name, kind) in [
        ("Shape", Interface),
        ("Pair", Type),
        ("Color", Enum),
        ("Base", Class),
        ("Circle", Class),
        ("Geometry", Module),
        ("makeCircle", Function),
        ("external", Function),
    ] {
        assert_eq!(item(&typescript, name).kind, kind, "{name}");
    }
    let circle = Some((Class, "Circle".to_owned()));
    let methods = typescript
        .items
        .iter()
        .filter(|i| parent(&typescript, i) == circle);
    let methods: Vec<_> = methods.map(|i| (i.kind, i.name.as_str())).collect();
    assert_eq!(
        methods,
        [
            (Method, "constructor"),
            (Method, "area"),
            (Method, "describe")
        ]
    );
    let abstract_method = typescript
        .items
        .iter()
        .find(|i| i.name == "describe" && parent(&typescript, i).unwrap().1 == "Base")
        .unwrap();
    assert_eq!(abstract_method.kind, Method);
    let area = typescript
        .items
        .iter()
        .find(|i| i.name == "area" && parent(&typescript, i).unwrap().1 == "Shape")
        .unwrap();
    assert_eq!(area.kind, Method);

    let tsx = sample("tsx");
    for (name, kind) in [
        ("Props", Interface),
        ("State", Type),
        ("Mode", Enum),
        ("Counter", Class),
        ("App", Function),
        ("Header", Function),
    ] {
        assert_eq!(item(&tsx, name).kind, kind, "{name}");
    }
    let render = item(&tsx, "render");
    assert_eq!(render.kind, Method);
    assert_eq!(parent(&tsx, render), Some((Class, "Counter".to_owned())));
}

#[test]
fn markdown_headings_nest_by_level() {
    use OutlineKind::Heading;
    let text = "\
intro

# A

## B

### C

```sh
# not a heading
```

## D ##

# E

Setext
------

body
";
    let outline = outline_of("markdown", text);
    let items: Vec<_> = outline
        .items
        .iter()
        .map(|item| (item.depth, item.kind, item.name.as_str()))
        .collect();
    assert_eq!(
        items,
        [
            (0, Heading(1), "A"),
            (1, Heading(2), "B"),
            (2, Heading(3), "C"),
            (1, Heading(2), "D"),
            (0, Heading(1), "E"),
            (1, Heading(2), "Setext"),
        ]
    );
    assert_eq!(
        parent(&outline, &outline.items[3]),
        Some((Heading(1), "A".into()))
    );
    assert_eq!(
        parent(&outline, &outline.items[5]),
        Some((Heading(1), "E".into()))
    );
}

/// The depth, kind and name of each item.
fn headings(outline: &Outline) -> Vec<(u32, OutlineKind, &str)> {
    (outline.items.iter())
        .map(|item| (item.depth, item.kind, item.name.as_str()))
        .collect()
}

#[test]
fn markdown_sections_hold_setext_and_empty_headings() {
    use OutlineKind::Heading;
    let text = "\
A
===

## B

# C

D
=

#

## E

Title #
-------
";
    let outline = outline_of("markdown", text);
    assert_eq!(
        headings(&outline),
        [
            (0, Heading(1), "A"),
            (1, Heading(2), "B"),
            (0, Heading(1), "C"),
            (0, Heading(1), "D"),
            (0, Heading(1), ""),
            (1, Heading(2), "E"),
            (1, Heading(2), "Title #"),
        ]
    );
    // A section runs to the next heading of the same or a higher level, in
    // either form.
    let at = |heading: &str| text.find(heading).unwrap();
    let ranges: Vec<_> = (outline.items.iter())
        .map(|item| item.range.clone())
        .collect();
    assert_eq!(
        ranges,
        [
            0..at("# C"),
            at("## B")..at("# C"),
            at("# C")..at("D\n"),
            at("D\n")..at("#\n"),
            at("#\n")..text.len(),
            at("## E")..at("Title"),
            at("Title")..text.len(),
        ]
    );
    assert_eq!(outline.item_at(at("## B") + 5), Some(1));
    assert_eq!(outline.item_at(text.len() - 1), Some(6));

    // An empty heading's name is empty, after its marker.
    let empty = &outline.items[4];
    assert_eq!(empty.name_range, at("#\n") + 1..at("#\n") + 1);
    let levels = outline_of("markdown", "#\n##\n###\n####\n#####\n######\n");
    let expected: Vec<_> = (1..=6)
        .map(|level| (level - 1, Heading(level as u8), ""))
        .collect();
    assert_eq!(headings(&levels), expected);
}

#[test]
fn atx_closing_sequences_are_left_out_of_names() {
    let long = "x".repeat(198);
    let text = format!(
        "# T #\n\n# T ##   \n\n# T2 \\#\n\n# a # #\n\n# a#\n\n# ##\n\n# {long} #y\n\nS #\n--\n"
    );
    let outline = outline_of("markdown", &text);
    let names: Vec<_> = (outline.items.iter())
        .map(|item| item.name.as_str())
        .collect();
    // Hashes cut by the 200-char limit stay: they don't end the heading.
    let cut = format!("{long} #");
    assert_eq!(names, ["T", "T", "T2 \\#", "a #", "a#", "", &cut, "S #"]);
    let only_hashes = &outline.items[5];
    assert!(only_hashes.name_range.is_empty());

    // Only spaces and tabs set a closing sequence apart, not a nonbreaking
    // space before or after it.
    let outline = outline_of("markdown", "# T\u{a0}#\n\n# T #\u{a0}\n\n# T\t#\t\n");
    let names: Vec<_> = (outline.items.iter())
        .map(|item| item.name.as_str())
        .collect();
    assert_eq!(names, ["T #", "T #", "T"]);
}

#[test]
fn omitted_headings_end_sections() {
    // The cap leaves out the last two headings, which still end the
    // sections of the listed ones.
    let mut text = "# top\n".to_owned();
    for ix in 1..MAX_OUTLINE_ITEMS {
        writeln!(text, "## s{ix}").unwrap();
    }
    text.push_str("## same\nbody\n# higher\ntail\n");
    let outline = outline_of("markdown", &text);
    assert!(outline.truncated);
    assert_eq!(outline.items.len(), MAX_OUTLINE_ITEMS);
    let at = |s: &str| text.find(s).unwrap();
    let last = outline.items.last().unwrap();
    assert_eq!(last.name, format!("s{}", MAX_OUTLINE_ITEMS - 1));
    assert_eq!(last.range.end, at("## same"));
    assert_eq!(outline.items[0].range, 0..at("# higher"));
    assert_eq!(
        outline.item_at(at("## same") - 1),
        Some(MAX_OUTLINE_ITEMS - 1)
    );
    assert_eq!(outline.item_at(at("body")), Some(0));
    assert_eq!(outline.item_at(at("tail")), None);
}

#[test]
fn items_are_preorder_and_ranges_nest() {
    for (path, language, text) in samples() {
        let outline = outline_of(language, &text);
        let buffer = Buffer::new(&text);
        let file = path.file_name().unwrap().to_string_lossy();
        assert!(!outline.items.is_empty(), "{file}");
        for (ix, item) in outline.items.iter().enumerate() {
            let at = format!("{file}: {:?} {:?}", item.kind, item.name);
            if ix > 0 {
                assert!(
                    outline.items[ix - 1].range.start <= item.range.start,
                    "{at}"
                );
            }
            assert!(item.range.start <= item.name_range.start, "{at}");
            assert!(item.name_range.end <= item.range.end, "{at}");
            let row = buffer.text().offset_to_point(item.name_range.start).row;
            assert_eq!(item.row, row, "{at}");
            match item.parent {
                None => assert_eq!(item.depth, 0, "{at}"),
                Some(parent) => {
                    assert!((parent as usize) < ix, "{at}");
                    let parent = &outline.items[parent as usize];
                    assert_eq!(item.depth, parent.depth + 1, "{at}");
                    assert!(parent.range.start <= item.range.start, "{at}");
                    assert!(item.range.end <= parent.range.end, "{at}");
                }
            }
        }
        let mut outside = 0;
        for offset in 0..=text.len() {
            // The deepest item containing the offset.
            let expected = (outline.items.iter().enumerate())
                .filter(|(_, item)| item.range.contains(&offset))
                .max_by_key(|(_, item)| item.depth)
                .map(|(ix, _)| ix);
            outside += usize::from(expected.is_none());
            assert_eq!(outline.item_at(offset), expected, "{file} at {offset}");
        }
        assert!(outside > 0, "{file} has text outside every item");
    }
}

#[test]
fn no_outline_without_a_full_tree() {
    let html = Buffer::new("<p>a</p>\n");
    let mut syntax = Syntax::new(language_by_name("html").unwrap(), html.current());
    syntax.finish(syntax.parse_job().run());
    assert!(!outline_supported("html"));
    assert_eq!(outline(&syntax).err(), Some(NoOutline::Unsupported));

    let rust = language_by_name("rust").unwrap();
    let line = "fn f() {}\n";
    let len = rust.parse_limit() + 1;
    let mut text = line.repeat(len / line.len());
    text.push_str(&" ".repeat(len - text.len()));
    let huge = Buffer::new(&text);
    let mut syntax = Syntax::new(rust, huge.current());
    assert_eq!(outline(&syntax).err(), Some(NoOutline::TooLarge));
    syntax.finish(syntax.window_job(0..100).unwrap().run());
    assert!(syntax.window().is_some());
    assert_eq!(outline(&syntax).err(), Some(NoOutline::TooLarge));

    let mut buffer = Buffer::new("fn f() {}\n");
    let mut syntax = Syntax::new(rust, buffer.current());
    assert_eq!(outline(&syntax).err(), Some(NoOutline::NotParsed));
    assert!(syntax.parse_until(Duration::MAX, || false));
    assert_eq!(outline(&syntax).unwrap().items.len(), 1);
    buffer.edit([(0..0, "fn g() {}\n")], Origin::User);
    syntax.edit(buffer.current());
    assert_eq!(outline(&syntax).err(), Some(NoOutline::NotParsed));
    assert!(syntax.parse_until(Duration::MAX, || false));
    let outline = outline(&syntax).unwrap();
    assert_eq!(outline.version, syntax.version());
    let names: Vec<_> = outline
        .items
        .iter()
        .map(|item| item.name.as_str())
        .collect();
    assert_eq!(names, ["g", "f"]);
}

#[test]
fn outline_is_capped() {
    let fns = |count: usize| {
        (0..count)
            .map(|ix| format!("fn f{ix}() {{}}\n"))
            .collect::<String>()
    };
    let capped = outline_of("rust", &fns(2 * MAX_OUTLINE_ITEMS));
    assert_eq!(MAX_OUTLINE_ITEMS, 10_000);
    assert_eq!(capped.items.len(), MAX_OUTLINE_ITEMS);
    assert!(capped.truncated);
    let full = outline_of("rust", &fns(MAX_OUTLINE_ITEMS));
    assert_eq!(full.items.len(), MAX_OUTLINE_ITEMS);
    assert!(!full.truncated);
}

const _: () = {
    const fn send<T: Send>() {}
    send::<OutlineJob>();
};

#[test]
fn outline_job_runs_on_another_thread() {
    let (_, _, text) = samples()
        .into_iter()
        .find(|(_, name, _)| *name == "rust")
        .unwrap();
    let buffer = Buffer::new(&text);
    let mut syntax = Syntax::new(language_by_name("rust").unwrap(), buffer.current());
    syntax.finish(syntax.parse_job().run());
    let job = outline_job(&syntax).unwrap();
    let background = std::thread::spawn(move || job.run()).join().unwrap();
    assert_eq!(background, outline(&syntax).unwrap());
}
