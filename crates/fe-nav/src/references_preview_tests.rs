//! The preview of a hit in the request's own file fills its window from the
//! before side when the line ends soon after the hit.

use super::super::preview;

#[test]
fn a_hit_near_a_long_lines_end_fills_the_window() {
    let line = format!("{} x", "a".repeat(600));
    let (window, range) = preview(&line, 0, 601..602);
    assert_eq!(window.len(), 512);
    assert!(window.ends_with(" x"));
    assert_eq!(&window[range.unwrap()], "x");

    let line = format!("{} x", "é".repeat(600));
    let hit = line.len() - 1..line.len();
    let (window, range) = preview(&line, 0, hit);
    assert!(matches!(window.len(), 511 | 512), "{}", window.len());
    assert!(window.ends_with(" x"));
    assert_eq!(&window[range.unwrap()], "x");

    // A hit in the middle of a long line, or at its start, keeps its window.
    let line = "b".repeat(1000);
    let (window, range) = preview(&line, 0, 300..301);
    assert_eq!(window.len(), 512);
    assert_eq!(range, Some(255..256));
    let (window, range) = preview(&line, 0, 0..1);
    assert_eq!(window.len(), 512);
    assert_eq!(range, Some(0..1));
}
