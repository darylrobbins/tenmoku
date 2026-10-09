//! `extract`: one file's definitions, with offsets into the loaded text.

use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use fe_buffer::Buffer;
use fe_nav::{OutlineKind, Skipped, extract};
use fe_syntax::language_by_name;

const RUST: &str = "\
pub struct Foo {
    pub bar: u32,
}

impl Foo {
    pub fn get(&self) -> u32 {
        self.bar
    }
}

fn helper() {}
";

/// The file as the editor's loader reads it.
fn loaded(path: &Path) -> Buffer {
    Buffer::from_reader(fs::File::open(path).unwrap()).unwrap()
}

/// Each symbol's kind, name and container, after checking that the loaded
/// text at its name range is its name.
fn checked(root: &Path, rel: &str) -> Vec<(OutlineKind, String, Option<String>)> {
    let file = extract(root, Path::new(rel)).unwrap();
    let buffer = loaded(&root.join(rel));
    let metadata = fs::metadata(root.join(rel)).unwrap();
    assert_eq!(file.rel, PathBuf::from(rel));
    assert_eq!(file.len, metadata.len());
    assert_eq!(file.modified, Some(metadata.modified().unwrap()));
    assert!(!file.truncated);
    file.symbols
        .into_iter()
        .map(|symbol| {
            assert_eq!(
                buffer.text().slice_to_string(symbol.name_range.clone()),
                symbol.name,
                "{rel}: the loaded text at {symbol:?}'s name range"
            );
            let row = buffer.text().offset_to_point(symbol.name_range.start).row;
            assert_eq!(symbol.row, row, "{rel}: {symbol:?}'s row");
            assert!(symbol.range.start <= symbol.name_range.start);
            assert!(symbol.name_range.end <= symbol.range.end);
            (symbol.kind, symbol.name, symbol.container)
        })
        .collect()
}

#[test]
fn extracts_definitions_with_loaded_offsets() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("lf.rs"), RUST).unwrap();
    fs::write(dir.path().join("crlf.rs"), RUST.replace('\n', "\r\n")).unwrap();
    let expected = vec![
        (OutlineKind::Struct, "Foo".to_owned(), None),
        (OutlineKind::Field, "bar".to_owned(), Some("Foo".to_owned())),
        (
            OutlineKind::Method,
            "get".to_owned(),
            Some("impl Foo".to_owned()),
        ),
        (OutlineKind::Function, "helper".to_owned(), None),
    ];
    assert_eq!(checked(dir.path(), "lf.rs"), expected);
    assert_eq!(checked(dir.path(), "crlf.rs"), expected);
    let crlf = extract(dir.path(), Path::new("crlf.rs")).unwrap();
    assert_eq!(crlf.language, "rust");
    assert_eq!(
        crlf.len,
        RUST.len() as u64 + RUST.matches('\n').count() as u64
    );
}

#[test]
fn skips_what_it_cannot_index() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let skipped = |rel: &str| extract(root, Path::new(rel)).unwrap_err();

    fs::write(root.join("notes.md"), "# Title\n").unwrap();
    fs::write(root.join("notes.txt"), "fn not_code() {}\n").unwrap();
    fs::write(root.join("Noextension"), "fn not_code() {}\n").unwrap();
    assert_eq!(skipped("notes.md"), Skipped::Unsupported);
    assert_eq!(skipped("notes.txt"), Skipped::Unsupported);
    assert_eq!(skipped("Noextension"), Skipped::Unsupported);

    // Sparse: only the length matters, as nothing is read.
    let limit = language_by_name("rust").unwrap().parse_limit();
    let large = fs::File::create(root.join("large.rs")).unwrap();
    large.set_len(limit as u64 + 1).unwrap();
    assert_eq!(skipped("large.rs"), Skipped::TooLarge);

    let mut binary = b"fn before() {}\n".to_vec();
    binary.push(0);
    binary.extend_from_slice(b"\nfn after() {}\n");
    fs::write(root.join("binary.rs"), binary).unwrap();
    assert_eq!(skipped("binary.rs"), Skipped::Binary);

    assert_eq!(
        skipped("missing.rs"),
        Skipped::Unreadable(ErrorKind::NotFound)
    );

    // More than the 64 KiB the loader's detection looks at, so it takes the
    // file for UTF-8 and decodes it lossily. Two invalid bytes after it become
    // two U+FFFDs, of 3 bytes each, so the loaded text's offsets are 4 bytes
    // past the file's.
    let filler = "plain ascii filler\n".repeat(6_000);
    assert!(filler.len() > 64 * 1024);
    let mut lossy = filler.clone().into_bytes();
    lossy.extend(b"// \xff\xfe\nfn after_lossy() {}\n");
    let detected = Buffer::from_reader_detecting(lossy.as_slice()).unwrap();
    assert_eq!(detected.encoding().label(), "UTF-8");
    fs::write(root.join("lossy.rs"), lossy).unwrap();
    let file = extract(root, Path::new("lossy.rs")).unwrap();
    let [symbol] = &file.symbols[..] else {
        panic!("one symbol in {file:?}");
    };
    assert_eq!(symbol.name, "after_lossy");
    assert_eq!(symbol.name_range, filler.len() + 13..filler.len() + 24);
    assert_eq!(symbol.row, 6_001);
    assert_eq!(checked(root, "lossy.rs").len(), 1);
}

#[test]
fn a_directory_swapped_for_a_symlink_is_not_followed() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("project");
    let outside = dir.path().join("outside");
    for parent in [root.join("src/a"), root.join("ignored"), outside.clone()] {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(root.join("src/a/x.rs"), "fn inside() {}\n").unwrap();
    fs::write(root.join("ignored/x.rs"), "fn ignored() {}\n").unwrap();
    fs::write(outside.join("x.rs"), "fn outside() {}\n").unwrap();
    let rel = Path::new("src/a/x.rs");
    assert_eq!(extract(&root, rel).unwrap().symbols[0].name, "inside");

    // The listed path's directory, swapped for a symlink out of the project
    // and then for one to an ignored directory inside it.
    fs::rename(root.join("src/a"), root.join("src/a.old")).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("src/a")).unwrap();
    assert_eq!(
        extract(&root, rel).unwrap_err(),
        Skipped::Unreadable(ErrorKind::NotFound)
    );
    fs::remove_file(root.join("src/a")).unwrap();
    std::os::unix::fs::symlink(root.join("ignored"), root.join("src/a")).unwrap();
    assert_eq!(
        extract(&root, rel).unwrap_err(),
        Skipped::Unreadable(ErrorKind::NotFound)
    );
    assert_eq!(
        extract(&root, Path::new("../outside/x.rs")).unwrap_err(),
        Skipped::Unreadable(ErrorKind::NotFound)
    );

    // A root reached through a symlink is fine: it's resolved first.
    fs::remove_file(root.join("src/a")).unwrap();
    fs::rename(root.join("src/a.old"), root.join("src/a")).unwrap();
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&root, &link).unwrap();
    assert_eq!(extract(&link, rel).unwrap().symbols[0].name, "inside");
}
