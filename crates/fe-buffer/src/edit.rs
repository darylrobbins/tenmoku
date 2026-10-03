use std::{ops::Range, sync::Arc};

use fe_text::Bias;

/// One replacement within an applied batch: `old` in the coordinates before the
/// batch, `new` in the coordinates after it. Within a batch, edits are sorted
/// and their `old` ranges don't overlap.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Edit {
    pub old: Range<usize>,
    pub new: Range<usize>,
}

/// Maps `offset` through one batch of edits.
///
/// Offsets outside every edit shift by the length change of the edits before
/// them. For an offset touching or inside an edit:
/// - text inserted exactly at the offset: `Left` stays before it, `Right` moves after it;
/// - at the start of a replaced range: stays at the start;
/// - at the end of a replaced range: moves to the end of the new text;
/// - strictly inside a replaced range: `Left` goes to its start, `Right` to its end.
pub(crate) fn map_offset(edits: &[Edit], offset: usize, bias: Bias) -> usize {
    let ix = edits.partition_point(|edit| edit.old.end < offset);
    map_offset_at(edits, ix, offset, bias)
}

/// Maps ascending `offsets` (all with the same bias) through one batch in a
/// single pass. Mapping preserves their order.
pub(crate) fn map_sorted(edits: &[Edit], offsets: &mut [usize], bias: Bias) {
    let mut ix = 0;
    for offset in offsets {
        while ix < edits.len() && edits[ix].old.end < *offset {
            ix += 1;
        }
        *offset = map_offset_at(edits, ix, *offset, bias);
    }
}

/// `ix` is the first edit whose old range ends at or after `offset`.
fn map_offset_at(edits: &[Edit], mut ix: usize, offset: usize, bias: Bias) -> usize {
    // A replacement ending at `offset` may be followed by an edit starting
    // there; the later edit decides (its start is this one's new end).
    while ix + 1 < edits.len()
        && edits[ix].old.start < offset
        && edits[ix].old.end == offset
        && edits[ix + 1].old.start == offset
    {
        ix += 1;
    }
    let Some(edit) = edits.get(ix) else {
        return match edits.last() {
            Some(last) => offset + last.new.end - last.old.end,
            None => offset,
        };
    };
    if offset < edit.old.start {
        return offset + edit.new.start - edit.old.start;
    }
    let side = if edit.old.is_empty() {
        bias
    } else if offset == edit.old.start {
        Bias::Left
    } else if offset == edit.old.end {
        Bias::Right
    } else {
        bias
    };
    match side {
        Bias::Left => edit.new.start,
        Bias::Right => edit.new.end,
    }
}

/// Sorts edits and merges any that overlap, plus insertions at the same
/// offset, so they can be applied as one batch. Overlapping edits keep their
/// texts in the order given. No-op edits are dropped.
pub(crate) fn normalize(mut edits: Vec<(Range<usize>, Arc<str>)>) -> Vec<(Range<usize>, Arc<str>)> {
    edits.sort_by_key(|(range, _)| (range.start, range.end));
    let mut merged: Vec<(Range<usize>, Arc<str>)> = Vec::with_capacity(edits.len());
    for (range, text) in edits {
        if let Some((last_range, last_text)) = merged.last_mut() {
            let overlaps = range.start < last_range.end;
            let same_insertion = last_range.start == last_range.end
                && range.is_empty()
                && range.start == last_range.start;
            if overlaps || same_insertion {
                last_range.end = last_range.end.max(range.end);
                if !text.is_empty() {
                    *last_text = format!("{last_text}{text}").into();
                }
                continue;
            }
        }
        merged.push((range, text));
    }
    merged.retain(|(range, text)| !(range.is_empty() && text.is_empty()));
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(old: Range<usize>, new: Range<usize>) -> Edit {
        Edit { old, new }
    }

    #[test]
    fn insertion_uses_bias() {
        let edits = [edit(2..2, 2..5)];
        assert_eq!(map_offset(&edits, 1, Bias::Right), 1);
        assert_eq!(map_offset(&edits, 2, Bias::Left), 2);
        assert_eq!(map_offset(&edits, 2, Bias::Right), 5);
        assert_eq!(map_offset(&edits, 3, Bias::Left), 6);
    }

    #[test]
    fn replacement_boundaries_stay_outside() {
        // Replace 2..4 with 3 bytes.
        let edits = [edit(2..4, 2..5)];
        assert_eq!(map_offset(&edits, 2, Bias::Right), 2);
        assert_eq!(map_offset(&edits, 4, Bias::Left), 5);
        assert_eq!(map_offset(&edits, 3, Bias::Left), 2);
        assert_eq!(map_offset(&edits, 3, Bias::Right), 5);
        assert_eq!(map_offset(&edits, 6, Bias::Left), 7);
    }

    #[test]
    fn touching_edits_defer_to_the_later_one() {
        // Delete 0..2, then insert 3 bytes at 2.
        let edits = [edit(0..2, 0..0), edit(2..2, 0..3)];
        assert_eq!(map_offset(&edits, 2, Bias::Left), 0);
        assert_eq!(map_offset(&edits, 2, Bias::Right), 3);
    }

    #[test]
    fn sorted_mapping_matches_single_mapping() {
        let edits = [
            edit(0..0, 0..2),
            edit(3..5, 5..5),
            edit(5..5, 5..6),
            edit(8..9, 9..12),
        ];
        for bias in [Bias::Left, Bias::Right] {
            let mut offsets: Vec<usize> = (0..12).collect();
            let expected: Vec<usize> = offsets
                .iter()
                .map(|&offset| map_offset(&edits, offset, bias))
                .collect();
            map_sorted(&edits, &mut offsets, bias);
            assert_eq!(offsets, expected);
        }
    }

    #[test]
    fn normalize_merges() {
        let a: Arc<str> = "a".into();
        let b: Arc<str> = "b".into();
        let merged = normalize(vec![
            (5..5, b.clone()),
            (1..3, a.clone()),
            (5..5, a.clone()),
            (2..4, b.clone()),
            (7..7, "".into()),
        ]);
        assert_eq!(merged, vec![(1..4, "ab".into()), (5..5, "ba".into())]);
    }
}
