//! What the memory report reads of a buffer's undo history.

use super::ai;
use crate::*;

#[test]
fn history_total_is_the_sum_over_origins() {
    let mut buffer = Buffer::new("");
    assert_eq!(buffer.history_bytes_total(), 0);
    let origins = [Origin::User, ai(), Origin::Lsp, Origin::Format];
    for (ix, origin) in origins.iter().enumerate() {
        let end = buffer.len();
        let text = "x".repeat(ix + 1);
        buffer
            .edit([(end..end, text.as_str())], origin.clone())
            .unwrap();
    }
    buffer.undo(|origin| *origin == Origin::Lsp).unwrap();
    let sum: usize = origins
        .iter()
        .map(|origin| buffer.history_bytes(origin))
        .sum();
    assert!(
        origins
            .iter()
            .all(|origin| buffer.history_bytes(origin) > 0)
    );
    assert_eq!(buffer.history_bytes_total(), sum);
}
