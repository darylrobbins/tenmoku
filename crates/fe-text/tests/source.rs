//! `TextSource` through a trait object answers like `Text` itself.

use std::sync::Arc;

use fe_text::{Bias, Point, Text, TextSource};
use proptest::prelude::*;
use rand::{Rng, SeedableRng, rngs::StdRng};

fn chunks_joined(source: &dyn TextSource, range: std::ops::Range<usize>) -> String {
    source
        .chunks_in_range(range)
        .map(|c| c.into_owned())
        .collect()
}

proptest! {
    #[test]
    fn text_through_the_trait_matches_text(
        raw in "[a-zé世😀\n]{0,60}",
        seed in any::<u64>(),
    ) {
        let text = Text::new(&raw);
        let source: &dyn TextSource = &text;

        prop_assert_eq!(source.len(), text.len());
        prop_assert_eq!(source.is_empty(), text.is_empty());
        prop_assert_eq!(source.row_count(), text.row_count());
        prop_assert_eq!(source.max_point(), text.max_point());
        for row in 0..text.row_count() + 1 {
            prop_assert_eq!(source.row_start(row), text.row_start(row));
            prop_assert_eq!(source.row_len(row), text.row_len(row));
            prop_assert_eq!(source.row_to_string(row), text.row_to_string(row));
            let start = source.row_start(row);
            let end = start + source.row_len(row) as usize;
            prop_assert_eq!(source.row_to_string(row), chunks_joined(source, start..end));
            for column in 0..=text.row_len(row.min(text.row_count() - 1)) + 2 {
                let point = Point::new(row, column);
                prop_assert_eq!(source.point_to_offset(point), text.point_to_offset(point));
            }
        }
        for offset in 0..=text.len() {
            prop_assert_eq!(source.offset_to_point(offset), text.offset_to_point(offset));
            prop_assert_eq!(source.is_char_boundary(offset), text.is_char_boundary(offset));
            if text.is_char_boundary(offset) {
                prop_assert_eq!(source.char_at(offset), text.char_at(offset));
            }
            for bias in [Bias::Left, Bias::Right] {
                prop_assert_eq!(
                    source.clip_offset(offset, bias),
                    text.clip_offset(offset, bias)
                );
            }
        }

        let mut rng = StdRng::seed_from_u64(seed);
        for _ in 0..200 {
            let a = text.clip_offset(rng.random_range(0..=text.len()), Bias::Left);
            let b = text.clip_offset(rng.random_range(0..=text.len()), Bias::Left);
            let range = a.min(b)..a.max(b);
            let expected: String = text.chunks_in_range(range.clone()).collect();
            prop_assert_eq!(chunks_joined(source, range.clone()), expected.clone());
            prop_assert_eq!(source.slice_to_string(range.clone()), expected.clone());
            prop_assert_eq!(text.slice_to_string(range), expected);
        }
    }
}

#[test]
fn shared_handle_is_send_sync() {
    let text = Text::new("one\ntwo\nthree");
    let handle: Arc<dyn TextSource> = text.share();
    let (len, rows) = std::thread::spawn(move || (handle.len(), handle.row_count()))
        .join()
        .unwrap();
    assert_eq!((len, rows), (13, 3));
}
