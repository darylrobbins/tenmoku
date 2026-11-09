//! Streaming edits: an AI's output written into a target range as it arrives,
//! chunk by chunk, while the user keeps editing.
//!
//! The whole stream is one transaction under the AI's origin. The first chunk
//! replaces the target range in a transaction of its own, set aside from any
//! the caller has open, and every later chunk joins it through
//! [`Buffer::edit_in`], so the user's edits in between stay in their own
//! transactions. The user's undo (origin `Origin::User`) never touches the
//! stream, and one "undo AI" (`buffer.undo(|o| o.is_ai())`) reverts all of it
//! and restores the old target text; redo brings it back.
//!
//! Cancelling mid-stream keeps what landed (decided 2026-10-11): a host stops
//! a stream with [`EditStream::finish`], or by dropping it, and the text
//! written so far stays as that one AI transaction.

use std::{ops::Range, sync::Arc};

use crate::{Anchor, Buffer, BufferId, BufferSnapshot, EditError, Origin, TransactionId, Version};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StreamError {
    /// The target text changed before the first chunk landed; nothing was
    /// applied.
    Conflict,
    /// The stream's transaction was undone, or is no longer in the history.
    Undone,
}

impl std::fmt::Display for StreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&match self {
            Self::Conflict => kiln_l10n::tr!("buffer-stream-conflict"),
            Self::Undone => kiln_l10n::tr!("buffer-stream-undone"),
        })
    }
}

impl std::error::Error for StreamError {}

/// An AI's edit of one range, written as it arrives; see the module docs.
///
/// The region is held by two [`Anchor`]s: the start moves after text
/// inserted at it and the end stays before text inserted at it, as
/// [`Buffer::edit_from`] maps a range. So text the user types at either edge
/// stays outside the region, and later chunks go before text typed at its
/// end. Text the user changes inside the region stays, and later chunks
/// still go to its end.
///
/// The anchors keep the buffer's edit log alive from [`EditStream::version`]
/// on: [`EditStream::refresh`] them now and then, and keep
/// [`Buffer::compact_log`]'s floor at or below that version.
#[derive(Clone, Debug)]
pub struct EditStream {
    buffer: BufferId,
    origin: Origin,
    start: Anchor,
    end: Anchor,
    /// The target's text, until the first chunk lands.
    old_text: Option<Arc<str>>,
    transaction: Option<TransactionId>,
    /// A chunk ended in `\r`, held back in case the next starts with `\n`.
    held_cr: bool,
    /// What killed the stream; every later push returns it.
    error: Option<StreamError>,
}

impl EditStream {
    /// A stream that will replace `range` (clipped) of `buffer`'s text. It
    /// doesn't edit.
    pub fn begin(buffer: &Buffer, range: Range<usize>, origin: Origin) -> Self {
        let snapshot = buffer.current();
        let text = snapshot.text();
        let (start, end) = (range.start.min(range.end), range.start.max(range.end));
        let range = text.clip_offset(start, fe_text::Bias::Left)
            ..text.clip_offset(end, fe_text::Bias::Right);
        Self {
            buffer: snapshot.id(),
            origin,
            start: snapshot.anchor_after(range.start),
            end: snapshot.anchor_before(range.end),
            old_text: Some(text.slice_to_string(range).into()),
            transaction: None,
            held_cr: false,
            error: None,
        }
    }

    /// Writes `chunk` at the end of the region. The first non-empty push
    /// replaces the target range, if its text is still the one
    /// [`EditStream::begin`] saw.
    ///
    /// # Errors
    /// [`StreamError::Conflict`] if the target text changed before the first
    /// chunk, checked on the first non-empty push even if its text is held
    /// back; [`StreamError::Undone`] if the stream's transaction was undone,
    /// trimmed or compacted away, checked on every push, empty ones too.
    /// Either way nothing is applied and the stream is dead: every later push
    /// returns the same error, even once the transaction is redone.
    ///
    /// # Panics
    /// If `buffer` isn't the stream's buffer, or its log was compacted above
    /// [`EditStream::version`].
    pub fn push(&mut self, buffer: &mut Buffer, chunk: &str) -> Result<(), StreamError> {
        self.check_buffer(buffer.current());
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        self.validate(buffer, !chunk.is_empty())?;
        let mut text = String::with_capacity(chunk.len() + 1);
        if self.held_cr {
            text.push('\r');
        }
        text.push_str(chunk);
        let held_cr = text.ends_with('\r');
        if held_cr {
            text.pop();
        }
        if !text.is_empty() {
            self.write(buffer, text)?;
        }
        self.held_cr = held_cr;
        Ok(())
    }

    /// Flushes a held `\r` and ends the stream. Returns its transaction, if
    /// anything landed; the text written so far stays.
    pub fn finish(mut self, buffer: &mut Buffer) -> Option<TransactionId> {
        if self.held_cr && self.error.is_none() {
            self.held_cr = false;
            // A failure kills the stream, and it ends here anyway.
            let _ = self.write(buffer, "\r".into());
        }
        self.transaction
    }

    /// The target range before the first chunk lands, then the region
    /// written so far.
    ///
    /// # Panics
    /// If `snapshot` belongs to another buffer, is older than the anchors
    /// (see [`EditStream::refresh`]), or its log was compacted above
    /// [`EditStream::version`].
    pub fn range(&self, snapshot: &BufferSnapshot) -> Range<usize> {
        self.check_buffer(snapshot);
        snapshot.resolve_range(&(self.start..self.end))
    }

    /// The stream's transaction, once a chunk has changed the text.
    pub fn transaction(&self) -> Option<TransactionId> {
        self.transaction
    }

    /// The origin the stream writes under.
    pub fn origin(&self) -> &Origin {
        &self.origin
    }

    /// Rebases the anchors onto `snapshot`, so resolving them stays cheap and
    /// the buffer's log can be compacted up to `snapshot`'s version.
    ///
    /// # Panics
    /// As [`EditStream::range`].
    pub fn refresh(&mut self, snapshot: &BufferSnapshot) {
        self.check_buffer(snapshot);
        self.start = snapshot.refresh(&self.start);
        self.end = snapshot.refresh(&self.end);
    }

    /// The oldest version the anchors use. The buffer's owner must keep
    /// [`Buffer::compact_log`]'s floor at or below it.
    pub fn version(&self) -> Version {
        self.start.version().min(self.end.version())
    }

    /// Checks, before any `\r` is held back, that the stream can still
    /// write: that the target text is in place, on an input that would start
    /// the stream (`writes`), and that the stream's transaction is still
    /// there, on every push. Kills the stream on failure.
    fn validate(&mut self, buffer: &mut Buffer, writes: bool) -> Result<(), StreamError> {
        if let Some(old_text) = &self.old_text {
            if writes {
                let target = self.range(buffer.current());
                if !buffer.text().range_eq(target, old_text) {
                    return Err(self.kill(StreamError::Conflict));
                }
            }
        } else if let Some(transaction) = self.transaction {
            // An empty edit changes nothing, and fails only if the
            // transaction is gone.
            let at = buffer.len();
            if buffer.edit_in(transaction, [(at..at, "")]).is_err() {
                return Err(self.kill(StreamError::Undone));
            }
        }
        Ok(())
    }

    /// Writes non-empty `text`: over the target range on the first write,
    /// else at the region's end. Kills the stream on failure.
    fn write(&mut self, buffer: &mut Buffer, text: String) -> Result<(), StreamError> {
        let region = self.range(buffer.current());
        let len = buffer_len(&text);
        let at = if let Some(old_text) = &self.old_text {
            if !buffer.text().range_eq(region.clone(), old_text) {
                return Err(self.kill(StreamError::Conflict));
            }
            let origin = self.origin.clone();
            let transaction = buffer
                .with_transaction_aside(|buffer| buffer.edit([(region.clone(), text)], origin));
            self.transaction = transaction;
            self.old_text = None;
            region.start
        } else {
            let at = region.end;
            match self.transaction {
                Some(transaction) => match buffer.edit_in(transaction, [(at..at, text)]) {
                    Ok(_) => {}
                    Err(EditError::NoSuchTransaction(_)) => {
                        return Err(self.kill(StreamError::Undone));
                    }
                    Err(error) => {
                        unreachable!("edit_in fails only for a gone transaction: {error}")
                    }
                },
                // The first chunk equalled the old text, so nothing has
                // landed yet: this chunk starts the transaction.
                None => {
                    let origin = self.origin.clone();
                    self.transaction = buffer
                        .with_transaction_aside(|buffer| buffer.edit([(at..at, text)], origin));
                }
            }
            at
        };
        let current = buffer.current();
        self.start = current.anchor_after(region.start);
        self.end = current.anchor_before(at + len);
        Ok(())
    }

    fn kill(&mut self, error: StreamError) -> StreamError {
        self.error = Some(error.clone());
        self.held_cr = false;
        error
    }

    fn check_buffer(&self, snapshot: &BufferSnapshot) {
        assert_eq!(
            snapshot.id(),
            self.buffer,
            "the snapshot belongs to another buffer than the stream"
        );
    }
}

/// The length `text` has once [`Buffer::edit`] has converted its line endings
/// to `\n`.
fn buffer_len(text: &str) -> usize {
    text.len() - text.matches("\r\n").count()
}
