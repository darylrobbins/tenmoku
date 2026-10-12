//! `ScipIndex`: a SCIP index decoded into per-file occurrences and
//! per-symbol sites. The indexes are built and encoded here, so no binary
//! fixture is checked in.

use std::{io, path::Path, sync::Arc};

use fe_nav::{ScipEncoding, ScipError, ScipIndex, ScipSite};
use fe_text::Text;
use protobuf::{EnumOrUnknown, Message};
use scip::types::{
    Document, Index, Occurrence, PositionEncoding, SymbolInformation, SymbolRole,
    symbol_information::Kind,
};

const FOO: &str = "rust-analyzer cargo demo 0.1.0 foo().";
const LOCAL: &str = "local 0";
const DEF: i32 = SymbolRole::Definition as i32;

fn occ(range: &[i32], symbol: &str, roles: i32) -> Occurrence {
    Occurrence {
        range: range.to_vec(),
        symbol: symbol.to_owned(),
        symbol_roles: roles,
        ..Default::default()
    }
}

fn doc(rel: &str, encoding: PositionEncoding, occurrences: Vec<Occurrence>) -> Document {
    Document {
        language: "rust".to_owned(),
        relative_path: rel.to_owned(),
        position_encoding: EnumOrUnknown::new(encoding),
        occurrences,
        ..Default::default()
    }
}

fn encode(documents: Vec<Document>) -> Vec<u8> {
    Index {
        documents,
        ..Default::default()
    }
    .write_to_bytes()
    .unwrap()
}

fn decode(documents: Vec<Document>) -> ScipIndex {
    ScipIndex::from_bytes(&encode(documents)).unwrap()
}

fn site(rel: &str, start: (u32, u32), end: (u32, u32), encoding: ScipEncoding) -> ScipSite {
    ScipSite {
        rel: Arc::from(Path::new(rel)),
        start,
        end,
        encoding,
    }
}

#[test]
fn definitions_and_references_span_documents_and_locals_stay_per_document() {
    // a.rs:            b.rs:
    //   fn foo() {}      fn main() {
    //   fn bar() {         let x = 1;
    //     let x = 1;       foo(x);
    //     foo();         }
    //     x
    //   }
    let mut a = doc(
        "src/a.rs",
        PositionEncoding::UTF8CodeUnitOffsetFromLineStart,
        vec![
            occ(&[3, 4, 7], FOO, 0),
            occ(&[0, 3, 6], FOO, DEF),
            occ(&[2, 8, 9], LOCAL, DEF),
            occ(&[4, 4, 5], LOCAL, 0),
        ],
    );
    a.symbols.push(SymbolInformation {
        symbol: FOO.to_owned(),
        display_name: "foo".to_owned(),
        kind: EnumOrUnknown::new(Kind::Function),
        ..Default::default()
    });
    a.symbols.push(SymbolInformation {
        symbol: LOCAL.to_owned(),
        display_name: "x".to_owned(),
        ..Default::default()
    });
    let b = doc(
        "src/b.rs",
        PositionEncoding::UTF8CodeUnitOffsetFromLineStart,
        vec![
            occ(&[1, 8, 9], LOCAL, DEF),
            occ(&[2, 4, 7], FOO, 0),
            occ(&[2, 8, 9], LOCAL, 0),
        ],
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index.scip");
    std::fs::write(&path, encode(vec![a, b])).unwrap();
    let index = ScipIndex::load(&path).unwrap();

    let utf8 = ScipEncoding::Utf8;
    assert_eq!(
        index.documents().collect::<Vec<_>>(),
        [Path::new("src/a.rs"), Path::new("src/b.rs")]
    );
    assert_eq!(index.language(Path::new("src/a.rs")), Some("rust"));
    assert_eq!(index.language(Path::new("src/c.rs")), None);

    assert_eq!(
        index.definitions(FOO),
        [site("src/a.rs", (0, 3), (0, 6), utf8)]
    );
    assert_eq!(
        index.references(FOO),
        [
            site("src/a.rs", (3, 4), (3, 7), utf8),
            site("src/b.rs", (2, 4), (2, 7), utf8),
        ]
    );
    let info = index.symbol_info(FOO).unwrap();
    assert_eq!(
        (info.display_name.as_str(), info.kind),
        ("foo", Kind::Function)
    );

    // Locals are never global, and each document's `local 0` is its own.
    assert!(index.definitions(LOCAL).is_empty());
    assert!(index.references(LOCAL).is_empty());
    assert!(index.symbol_info(LOCAL).is_none());
    assert_eq!(
        index.local_definitions(Path::new("src/a.rs"), 0, LOCAL),
        [site("src/a.rs", (2, 8), (2, 9), utf8)]
    );
    assert_eq!(
        index.local_references(Path::new("src/a.rs"), 0, LOCAL),
        [site("src/a.rs", (4, 4), (4, 5), utf8)]
    );
    assert_eq!(
        index.local_definitions(Path::new("src/b.rs"), 0, LOCAL),
        [site("src/b.rs", (1, 8), (1, 9), utf8)]
    );
    assert_eq!(
        index.local_references(Path::new("src/b.rs"), 0, LOCAL),
        [site("src/b.rs", (2, 8), (2, 9), utf8)]
    );

    let text = Text::new("fn foo() {}\nfn bar() {\n    let x = 1;\n    foo();\n    x\n}\n");
    let x = text.row_start(4) + 4;
    let found = index
        .occurrence_at(Path::new("src/a.rs"), &text, x)
        .unwrap();
    assert_eq!(&*found.symbol, LOCAL);
    assert_eq!(found.site, site("src/a.rs", (4, 4), (4, 5), utf8));
    assert_eq!(found.range, x..x + 1);
    assert!(!found.definition);
    assert_eq!(
        index.definitions_of(&found),
        [site("src/a.rs", (2, 8), (2, 9), utf8)]
    );
    let found = index
        .occurrence_at(Path::new("src/a.rs"), &text, 4)
        .unwrap();
    assert_eq!(index.references_of(&found), index.references(FOO));
    assert_eq!(
        (&*found.symbol, found.range, found.definition),
        (FOO, 3..6, true)
    );
    assert!(
        index
            .occurrence_at(Path::new("src/a.rs"), &text, 1)
            .is_none()
    );
    assert!(
        index
            .occurrence_at(Path::new("src/c.rs"), &text, 4)
            .is_none()
    );
}

/// The line `é 😀 target = target;`: the first `target` starts at byte 8,
/// UTF-16 unit 5 and UTF-32 unit 4; the second is 9 bytes (and units) on.
const LINE: &str = "é 😀 target = target;";

#[test]
fn occurrence_at_reads_each_position_encoding() {
    let text = Text::new(&format!("{LINE}\nnext\n"));
    for (encoding, start) in [
        (PositionEncoding::UTF8CodeUnitOffsetFromLineStart, 8),
        (PositionEncoding::UTF16CodeUnitOffsetFromLineStart, 5),
        (PositionEncoding::UTF32CodeUnitOffsetFromLineStart, 4),
        (PositionEncoding::UnspecifiedPositionEncoding, 5),
    ] {
        let rel = Path::new("a.rs");
        let index = decode(vec![doc(
            "a.rs",
            encoding,
            vec![
                // A 3-element range and a 4-element one on the same line.
                occ(&[0, start, start + 6], FOO, DEF),
                occ(&[0, start + 9, 0, start + 15], "b.", 0),
                // Other lengths are skipped.
                occ(&[0, 0], "two.", 0),
                occ(&[0, 0, 0, 1, 2], "five.", 0),
            ],
        )]);
        let found = index.occurrence_at(rel, &text, 10).unwrap();
        assert_eq!(
            (&*found.symbol, found.range.clone()),
            (FOO, 8..14),
            "{encoding:?}"
        );
        assert!(found.definition);
        // At its end, the occurrence is still found.
        assert_eq!(index.occurrence_at(rel, &text, 14).unwrap().range, 8..14);
        let found = index.occurrence_at(rel, &text, 17).unwrap();
        assert_eq!(
            (&*found.symbol, found.range),
            ("b.", 17..23),
            "{encoding:?}"
        );
        assert_eq!(found.site.start, (0, start as u32 + 9));
        assert!(index.occurrence_at(rel, &text, 0).is_none());
        assert!(index.references("two.").is_empty());
        assert!(index.references("five.").is_empty());
    }
}

#[test]
fn occurrence_at_prefers_the_innermost() {
    let text = Text::new("call(arg)\nend ab\n");
    let rel = Path::new("a.rs");
    let index = decode(vec![doc(
        "a.rs",
        PositionEncoding::UTF8CodeUnitOffsetFromLineStart,
        vec![
            occ(&[0, 0, 1, 3], "outer.", 0),
            occ(&[0, 0, 4], "call.", 0),
            occ(&[0, 5, 8], "arg.", 0),
            occ(&[1, 4, 5], "a.", 0),
            occ(&[1, 5, 6], "b.", 0),
        ],
    )]);
    let at = |offset| index.occurrence_at(rel, &text, offset).unwrap().symbol;
    assert_eq!(&*at(2), "call.");
    // Ending at an offset counts, and `call` is inside `outer`.
    assert_eq!(&*at(4), "call.");
    assert_eq!(&*at(5), "arg.");
    assert_eq!(&*at(8), "arg.");
    // On the next line only the multi-line occurrence is there.
    assert_eq!(&*at(11), "outer.");
    // Between two adjacent ones, the one it's in rather than at the end of.
    assert_eq!(&*at(15), "b.");
}

#[test]
fn a_file_split_across_documents_is_merged() {
    // Each of `a.rs`'s two documents has its own `local 0`.
    let index = decode(vec![
        doc(
            "a.rs",
            PositionEncoding::UTF8CodeUnitOffsetFromLineStart,
            vec![
                occ(&[2, 0, 3], FOO, 0),
                occ(&[3, 0, 1], LOCAL, DEF),
                occ(&[4, 0, 1], LOCAL, 0),
            ],
        ),
        doc(
            "b.rs",
            PositionEncoding::UTF8CodeUnitOffsetFromLineStart,
            vec![occ(&[0, 0, 3], FOO, 0)],
        ),
        doc(
            "a.rs",
            PositionEncoding::UTF16CodeUnitOffsetFromLineStart,
            vec![
                occ(&[0, 0, 3], FOO, DEF),
                occ(&[1, 0, 3], FOO, 0),
                occ(&[5, 0, 1], LOCAL, DEF),
                occ(&[6, 0, 1], LOCAL, 0),
            ],
        ),
    ]);
    let utf8 = ScipEncoding::Utf8;
    let utf16 = ScipEncoding::Utf16;
    assert_eq!(index.documents().count(), 2);
    assert_eq!(
        index.definitions(FOO),
        [site("a.rs", (0, 0), (0, 3), utf16)]
    );
    assert_eq!(
        index.references(FOO),
        [
            site("a.rs", (1, 0), (1, 3), utf16),
            site("a.rs", (2, 0), (2, 3), utf8),
            site("b.rs", (0, 0), (0, 3), utf8),
        ]
    );

    let rel = Path::new("a.rs");
    let text = Text::new("foo\nfoo\nfoo\nx\nx\ny\ny\n");
    let first = index.occurrence_at(rel, &text, 14).unwrap();
    assert_eq!((&*first.symbol, first.scope), (LOCAL, 0));
    assert_eq!(
        index.definitions_of(&first),
        [site("a.rs", (3, 0), (3, 1), utf8)]
    );
    assert_eq!(
        index.references_of(&first),
        [site("a.rs", (4, 0), (4, 1), utf8)]
    );
    let second = index.occurrence_at(rel, &text, 18).unwrap();
    assert_eq!((&*second.symbol, second.scope), (LOCAL, 1));
    assert_eq!(
        index.definitions_of(&second),
        [site("a.rs", (5, 0), (5, 1), utf16)]
    );
    assert_eq!(
        index.references_of(&second),
        [site("a.rs", (6, 0), (6, 1), utf16)]
    );
    assert!(index.local_definitions(rel, 2, LOCAL).is_empty());
}

#[test]
fn byte_range_is_none_off_the_text() {
    let text = Text::new(&format!("{LINE}\nnext"));
    let utf8 = ScipEncoding::Utf8;
    let utf16 = ScipEncoding::Utf16;
    let utf32 = ScipEncoding::Utf32;
    assert_eq!(
        site("a", (0, 8), (0, 14), utf8).byte_range(&text),
        Some(8..14)
    );
    assert_eq!(
        site("a", (0, 5), (1, 4), utf16).byte_range(&text),
        Some(8..29)
    );
    assert_eq!(
        site("a", (1, 0), (1, 4), utf32).byte_range(&text),
        Some(25..29)
    );

    // A line past the end of the text.
    assert_eq!(site("a", (2, 0), (2, 1), utf8).byte_range(&text), None);
    assert_eq!(site("a", (0, 0), (2, 0), utf16).byte_range(&text), None);
    // An offset past the end of its line (`next` is 4 long).
    assert_eq!(site("a", (1, 0), (1, 5), utf8).byte_range(&text), None);
    assert_eq!(site("a", (1, 5), (1, 5), utf16).byte_range(&text), None);
    assert_eq!(site("a", (0, 0), (0, 21), utf32).byte_range(&text), None);
    // An offset splitting a char: inside `é`'s bytes, `😀`'s surrogates.
    assert_eq!(site("a", (0, 1), (0, 2), utf8).byte_range(&text), None);
    assert_eq!(site("a", (0, 3), (0, 5), utf16).byte_range(&text), None);
    assert_eq!(site("a", (0, 4), (0, 6), utf8).byte_range(&text), None);
}

#[test]
fn bad_indexes_are_errors() {
    let bytes = encode(vec![doc(
        "a.rs",
        PositionEncoding::UTF8CodeUnitOffsetFromLineStart,
        vec![occ(&[0, 0, 3], FOO, DEF)],
    )]);
    // One document, so every cut is inside it.
    for len in 1..bytes.len() {
        assert!(
            matches!(
                ScipIndex::from_bytes(&bytes[..len]),
                Err(ScipError::Decode(_))
            ),
            "cut at {len}"
        );
    }

    // An empty unknown group (field 4) doesn't hide a cut document after it.
    let group_then_cut = [0x23, 0x24, 0x12, 0x05, 0x0a, 0x01, 0x61];
    assert!(matches!(
        ScipIndex::from_bytes(&group_then_cut),
        Err(ScipError::Decode(_))
    ));
    let group_then_whole = [&[0x23, 0x24][..], &bytes].concat();
    assert_eq!(
        ScipIndex::from_bytes(&group_then_whole)
            .unwrap()
            .definitions(FOO)
            .len(),
        1
    );
    // A group the input ends inside of.
    assert!(matches!(
        ScipIndex::from_bytes(&[&[0x23][..], &bytes].concat()),
        Err(ScipError::Decode(_))
    ));

    let mut state = 0x9e37_79b9_7f4a_7c15u64;
    let mut random = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state as u8
    };
    for _ in 0..64 {
        let bytes: Vec<u8> = (0..512).map(|_| random()).collect();
        assert!(matches!(
            ScipIndex::from_bytes(&bytes),
            Err(ScipError::Decode(_))
        ));
    }

    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        ScipIndex::load(dir.path().join("index.scip")).unwrap_err(),
        ScipError::Io(io::ErrorKind::NotFound)
    );
}
