//! An incremental parser for server-sent events (the WHATWG HTML standard's
//! `text/event-stream`), as much of it as the Messages API uses: `event:`
//! and `data:` fields, `:` comments, and LF, CR or CRLF line ends. Bytes
//! may be split anywhere, inside a line, a CRLF or a UTF-8 sequence: a line
//! is decoded only once it is whole.

/// One dispatched event.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SseEvent {
    /// The `event:` field, or `"message"` when there was none.
    pub event: String,
    /// The `data:` fields, joined with `\n`.
    pub data: String,
}

/// Reads events from bytes as they arrive.
#[derive(Debug, Default)]
pub struct SseParser {
    /// The line read so far, undecoded.
    line: Vec<u8>,
    /// Whether the last byte was a CR, so an LF straight after it is part of
    /// the same line end.
    after_cr: bool,
    /// Whether the stream's first line has begun, for the optional BOM.
    started: bool,
    event: String,
    data: String,
    /// Whether a `data:` field was seen since the last dispatch.
    has_data: bool,
}

impl SseParser {
    pub fn new() -> SseParser {
        SseParser::default()
    }

    /// Reads `bytes` and returns the events they complete, in order.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<SseEvent> {
        let mut events = Vec::new();
        for &byte in bytes {
            match byte {
                b'\n' if self.after_cr => self.after_cr = false,
                b'\r' | b'\n' => {
                    self.after_cr = byte == b'\r';
                    self.end_line(&mut events);
                }
                _ => {
                    self.after_cr = false;
                    self.line.push(byte);
                }
            }
        }
        events
    }

    fn end_line(&mut self, events: &mut Vec<SseEvent>) {
        let mut bytes = std::mem::take(&mut self.line);
        if !self.started {
            self.started = true;
            if bytes.starts_with("\u{feff}".as_bytes()) {
                bytes.drain(..3);
            }
        }
        let line = String::from_utf8_lossy(&bytes);
        if line.is_empty() {
            self.dispatch(events);
            return;
        }
        if line.starts_with(':') {
            return;
        }
        let (field, value) = match line.split_once(':') {
            Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
            None => (&*line, ""),
        };
        match field {
            "event" => self.event = value.to_owned(),
            "data" => {
                if self.has_data {
                    self.data.push('\n');
                }
                self.data.push_str(value);
                self.has_data = true;
            }
            // `id` and `retry` mean nothing to a one-shot request, and
            // unknown fields are ignored by the standard.
            _ => {}
        }
    }

    fn dispatch(&mut self, events: &mut Vec<SseEvent>) {
        let event = std::mem::take(&mut self.event);
        let data = std::mem::take(&mut self.data);
        if !std::mem::take(&mut self.has_data) {
            return;
        }
        events.push(SseEvent {
            event: if event.is_empty() {
                "message".to_owned()
            } else {
                event
            },
            data,
        });
    }
}
