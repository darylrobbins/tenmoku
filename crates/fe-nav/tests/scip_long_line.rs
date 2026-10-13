//! SCIP UTF-16 columns resolve through the rope's metric, not a row walk.

use std::{path::Path, time::Instant};

use fe_nav::ScipIndex;
use fe_text::Text;
use protobuf::{EnumOrUnknown, Message};
use scip::types::{Document, Index, Occurrence, PositionEncoding};

fn index(text_occurrences: Vec<Occurrence>) -> ScipIndex {
    let document = Document {
        language: "javascript".to_owned(),
        relative_path: "a.js".to_owned(),
        position_encoding: EnumOrUnknown::new(PositionEncoding::UTF16CodeUnitOffsetFromLineStart),
        occurrences: text_occurrences,
        ..Default::default()
    };
    let bytes = Index {
        documents: vec![document],
        ..Default::default()
    }
    .write_to_bytes()
    .unwrap();
    ScipIndex::from_bytes(&bytes).unwrap()
}

fn occ(range: [i32; 3], symbol: &str) -> Occurrence {
    Occurrence {
        range: range.to_vec(),
        symbol: symbol.to_owned(),
        ..Default::default()
    }
}

#[test]
fn utf16_occurrences_on_a_long_row_resolve_quickly() {
    let n = 20_000;
    let text = Text::new(&"a;".repeat(n));
    let occurrences = (0..n as i32)
        .map(|i| occ([0, 2 * i, 2 * i + 1], &format!("s{i}")))
        .collect();
    let index = index(occurrences);
    let start = Instant::now();
    let found = index
        .occurrence_at(Path::new("a.js"), &text, 2 * (n - 1))
        .expect("the last occurrence");
    assert!(start.elapsed().as_secs() < 5, "took {:?}", start.elapsed());
    assert_eq!(&*found.symbol, format!("s{}", n - 1));
}

#[test]
fn utf16_columns_that_split_a_surrogate_pair_are_refused() {
    let text = Text::new("😀x");
    let index = index(vec![occ([0, 0, 2], "s1"), occ([0, 1, 3], "s2")]);
    let at = |offset| index.occurrence_at(Path::new("a.js"), &text, offset);
    assert_eq!(&*at(0).unwrap().symbol, "s1");
    for offset in 0..=text.len() {
        if let Some(found) = at(offset) {
            assert_ne!(&*found.symbol, "s2", "offset {offset}");
        }
    }
}
