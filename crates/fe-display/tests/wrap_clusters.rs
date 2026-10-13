//! Soft wrap breaks between whole grapheme clusters, measures them with
//! `CharWidth::cluster_width`, and never breaks at a no-break space.

use fe_buffer::Buffer;
use fe_display::{CharWidth, DisplayMap, Monospace};

const FAMILY: &str = "👨\u{200d}👩\u{200d}👧";

/// Every char and every cluster one wide.
struct Cells;

impl CharWidth for Cells {
    fn char_width(&mut self, _: char) -> f32 {
        1.
    }

    fn cluster_width(&mut self, _: &str) -> f32 {
        1.
    }
}

fn wrapped(text: &str, width: f32, widths: &mut dyn CharWidth) -> Vec<String> {
    let mut map = DisplayMap::new(Buffer::new(text).current(), 4);
    map.set_wrap_width(Some(width), widths);
    while map.wrap_pending(0, usize::MAX, widths) {}
    (0..map.row_count())
        .map(|r| text[map.row(r).range].to_string())
        .collect()
}

#[test]
fn soft_wrap_keeps_grapheme_clusters_whole() {
    let text = format!("abcd{FAMILY}x");
    assert_eq!(
        wrapped(&text, 5., &mut Cells),
        [format!("abcd{FAMILY}"), "x".into()]
    );
    assert_eq!(
        wrapped("abcde\u{301}", 5., &mut Monospace),
        ["abcd", "e\u{301}"]
    );
}

#[test]
fn soft_wrap_measures_clusters_with_cluster_width() {
    assert_eq!(wrapped("abcde\u{301}", 5., &mut Cells), ["abcde\u{301}"]);
}

#[test]
fn soft_wrap_never_breaks_at_a_no_break_space() {
    assert_eq!(
        wrapped("x 10\u{a0}kg", 6., &mut Monospace),
        ["x ", "10\u{a0}kg"]
    );
}

#[test]
fn soft_wrap_still_breaks_after_spaces() {
    assert_eq!(wrapped("ab cd ef", 5., &mut Monospace), ["ab cd ", "ef"]);
}
