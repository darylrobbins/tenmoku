use super::*;

/// The positions a text object's span takes: its start and the position its
/// end is taken at. `None` for a failed object.
fn object_ends(text: &Text, span: Span, object: TextObject) -> Option<(usize, usize)> {
    use TextObject::*;
    match span {
        // An empty inside: nothing for `i(` and `i{`, the quotes for `i"`,
        // the position itself for a word object (an empty row).
        Span::Chars(range) if range.is_empty() => match object {
            InnerParen | InnerBrace => None,
            InnerDoubleQuote | InnerSingleQuote => Some((range.start, range.start - 1)),
            _ => Some((range.start, range.start)),
        },
        // `i(` and `i{` end at a row's end when the bracket starts the next
        // row: a visual selection takes that row's end too.
        Span::Chars(range)
            if matches!(object, TextObject::InnerParen | TextObject::InnerBrace)
                && range.end < text.len()
                && range.end == row_end(text, text.offset_to_point(range.end).row) =>
        {
            Some((range.start, range.end))
        }
        Span::Chars(range) => {
            // Ending at a row's start, a word object ends on that (empty)
            // row; any other ends with a line break, at the row's end before.
            let words = matches!(
                object,
                TextObject::InnerWord
                    | TextObject::AWord
                    | TextObject::InnerBigWord
                    | TextObject::ABigWord
            );
            let last = match text.offset_to_point(range.end).column == 0 {
                true if words => range.end,
                true => range.end - 1,
                false => Cursor::at(text, range.end - 1).offset(),
            };
            Some((range.start, last))
        }
        Span::Lines { rows, .. } => {
            Some((text.row_start(*rows.start()), row_end(text, *rows.end())))
        }
        _ => None,
    }
}

/// `vim.select`: a text object typed in a visual mode. Over one position it
/// selects the object there; otherwise a word object adds `count` more from
/// the cursor on, the way it faces, and any other selects the smallest one
/// around the selection that's larger than it. The mode becomes charwise.
pub(crate) fn select_object(
    target: &mut VimTarget<'_>,
    args: SelectArgs,
) -> Result<Dispatched<TransactionId>, CommandError> {
    use TextObject::*;
    let VimTarget { model, state, .. } = target;
    model.sync_display();
    let Some(ends) = ends(model, state) else {
        return effect(false);
    };
    let text = model.text().clone();
    let count = args.count.unwrap_or(1).max(1);
    let object = |offset: usize, count: u32| {
        let span = text_object::select(&text, offset, args.text_object, count);
        object_ends(&text, span, args.text_object)
    };
    let words = matches!(
        args.text_object,
        InnerWord | AWord | InnerBigWord | ABigWord
    );
    let forward = ends.cursor > ends.anchor;
    let big = matches!(args.text_object, InnerBigWord | ABigWord);
    let mut kind = VisualKind::Char;
    let inner = matches!(args.text_object, InnerWord | InnerBigWord);
    // A row's end, or a last row that's empty: `iw` grows back from it.
    let at_end = {
        let at = cursor_at(&text, ends.cursor);
        match at.line.len() {
            0 => at.row > 0 && at.row + 1 == text.row_count(),
            len => at.ix == len,
        }
    };
    let (anchor, cursor) = if ends.anchor == ends.cursor && inner && at_end {
        (
            ends.anchor,
            back_inner_words(&text, ends.cursor, count, big, true),
        )
    } else if ends.anchor == ends.cursor {
        match object(ends.cursor, 1) {
            // More `iw` than one go on as the selection grows.
            Some((start, _))
                if count > 1 && matches!(args.text_object, InnerWord | InnerBigWord) =>
            {
                let end = inner_words(&text, ends.cursor, count, big, false);
                (start, end.unwrap_or_else(|| position(&text, text.len())))
            }
            Some(_) if count > 1 => match object(ends.cursor, count) {
                Some(found) => found,
                None => return effect(false),
            },
            Some(found) => found,
            None => return effect(false),
        }
    } else if !forward && matches!(args.text_object, AWord | ABigWord) {
        (ends.anchor, back_a_words(&text, ends.cursor, count, big))
    } else if !forward && inner {
        // At the start of the text there's nothing to grow onto.
        match back_inner_words(&text, ends.cursor, count, big, false) {
            back if back == ends.cursor => return effect(false),
            back => (ends.anchor, back),
        }
    } else if forward && inner {
        match inner_words(&text, ends.cursor, count, big, true) {
            Some(end) => (ends.anchor, end),
            // Past the end of the text, onto the last row's end.
            None => {
                kind = ends.kind;
                (ends.anchor, position(&text, text.len()))
            }
        }
    } else if words {
        let mut from = cursor_at(&text, ends.cursor);
        let stepped = match forward {
            true => from.forward(),
            false => from.backward(),
        };
        match object(from.offset(), count).filter(|_| stepped) {
            Some((start, _)) if !forward => (ends.anchor, start),
            Some((_, last)) => (ends.anchor, last),
            // Forward past the end of the text, onto the last row's end.
            None if forward => {
                kind = ends.kind;
                (ends.anchor, position(&text, text.len()))
            }
            None => return effect(false),
        }
    } else {
        // The object around the cursor, or if that's the selection, the
        // next one out (for `i"`, with its quotes), if it's larger.
        let covers = |(start, last): (usize, usize)| (start, end_of(&text, last));
        let current = (ends.lo(), end_of(&text, ends.hi()));
        let quotes = !matches!(args.text_object, InnerParen | AParen | InnerBrace | ABrace);
        let found = object(ends.cursor, count).and_then(|found| match covers(found) == current {
            true => object(ends.cursor, count.saturating_add(1)),
            false => (!quotes).then_some(found),
        });
        match found.map(|found| (found, covers(found))) {
            Some((found, (start, end)))
                if start <= current.0 && end >= current.1 && (start, end) != current =>
            {
                found
            }
            _ => return effect(false),
        }
    };
    let selected = Ends {
        kind,
        anchor,
        cursor,
        goal: None,
    };
    select(model, state, selected);
    effect(true)
}

/// `iw` (`iW` if `big`) in a visual mode, `count` items from the cursor at
/// `offset`, returning where the cursor goes, or `None` past the end of the
/// text. An item is a run of one class on a row, or an empty row, which a
/// selection takes up to the next row's start; each after the first (and
/// the first, when the selection `grows`) starts one position on.
fn inner_words(text: &Text, offset: usize, count: u32, big: bool, grows: bool) -> Option<usize> {
    let mut cursor = cursor_at(text, offset);
    for item in 0..count {
        let further = grows || item > 0;
        if further && !cursor.forward() {
            return None;
        }
        if cursor.line.len() == 0 {
            if further && cursor.row + 1 < text.row_count() {
                cursor.enter(cursor.row + 1);
                cursor.ix = 0;
            }
        } else {
            cursor.ix = cursor.ix.min(cursor.line.last());
            let class = cursor.class(big);
            while cursor.class_on_row(cursor.ix + 1, big) == Some(class) {
                cursor.ix += 1;
            }
        }
    }
    Some(cursor.offset())
}

/// `iw` (`iW` if `big`) `count` times back from the cursor at `offset`, as
/// a visual selection grows the way it faces: each item one position back,
/// then to the start of its run on the row. From a row's `end` (or a last row
/// that's empty) the first item is only the position before it.
fn back_inner_words(text: &Text, offset: usize, count: u32, big: bool, end: bool) -> usize {
    let mut cursor = cursor_at(text, offset);
    for item in 0..count {
        if !cursor.backward() {
            break;
        }
        if item > 0 || !end {
            let class = cursor.class_on_row(cursor.ix, big);
            while cursor.ix > 0 && cursor.class_on_row(cursor.ix - 1, big) == class {
                cursor.ix -= 1;
            }
        }
    }
    cursor.offset()
}

/// `aw` (`aW` if `big`) `count` times back from the cursor at `offset`, as a
/// visual selection grows the way it faces: before it, a word and the blanks
/// before that, or blanks and the word before them. Returns where the
/// cursor goes.
fn back_a_words(text: &Text, offset: usize, count: u32, big: bool) -> usize {
    let mut cursor = cursor_at(text, offset);
    let blank = |cursor: &Cursor| cursor.class_on_row(cursor.ix, big) == Some(Class::Blank);
    // Back to the start of the run holding the cursor, on its row.
    let run_start = |cursor: &mut Cursor| {
        let class = cursor.class_on_row(cursor.ix, big);
        while cursor.ix > 0 && cursor.class_on_row(cursor.ix - 1, big) == class {
            cursor.ix -= 1;
        }
    };
    for _ in 0..count {
        if !cursor.backward() {
            break;
        }
        let blanks_first = blank(&cursor);
        if blanks_first {
            run_start(&mut cursor);
            if !cursor.backward() {
                break;
            }
        }
        run_start(&mut cursor);
        if !blanks_first
            && cursor.ix > 0
            && cursor.class_on_row(cursor.ix - 1, big) == Some(Class::Blank)
        {
            cursor.ix -= 1;
            run_start(&mut cursor);
        }
    }
    cursor.offset()
}
