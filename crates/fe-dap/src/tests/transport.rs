// SPDX-License-Identifier: Apache-2.0

//! Content-Length framing (§2): headers split across reads, several
//! messages in one read, the body limit, ignored and differently cased
//! headers, and one write per message.

use std::io;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use futures::{AsyncWrite, AsyncWriteExt as _};
use serde_json::json;

use crate::fake::{PipeReader, PipeWriter, pipe};
use crate::protocol::{EventMessage, Message, event};
use crate::transport::{FrameReader, FrameWriter, MAX_BODY, ProtocolError, TransportError};

/// A reader fed `chunks`, each arriving as one read (or less).
fn reader_of(chunks: &[&[u8]]) -> FrameReader<PipeReader> {
    let (reader, mut writer) = pipe();
    async_io::block_on(async {
        for chunk in chunks {
            writer.write_all(chunk).await.unwrap();
        }
        writer.close().await.unwrap();
    });
    FrameReader::new(reader)
}

fn read_all(mut reader: FrameReader<PipeReader>) -> Result<Vec<Vec<u8>>, TransportError> {
    async_io::block_on(async {
        let mut bodies = Vec::new();
        while let Some(body) = reader.read_body().await? {
            bodies.push(body);
        }
        Ok(bodies)
    })
}

#[test]
fn a_header_split_across_reads() {
    let reader = reader_of(&[
        b"Content-Len",
        b"gth: ",
        b"9\r",
        b"\n\r",
        b"\n{\"seq\":",
        b"1}",
    ]);
    assert_eq!(read_all(reader).unwrap(), vec![b"{\"seq\":1}".to_vec()]);
}

#[test]
fn a_header_split_one_byte_at_a_time() {
    let frame = b"Content-Length: 2\r\n\r\n{}";
    let chunks: Vec<&[u8]> = frame.chunks(1).collect();
    assert_eq!(read_all(reader_of(&chunks)).unwrap(), vec![b"{}".to_vec()]);
}

#[test]
fn two_messages_in_one_read() {
    let reader = reader_of(&[b"Content-Length: 2\r\n\r\n{}Content-Length: 7\r\n\r\n[1,2,3]"]);
    assert_eq!(
        read_all(reader).unwrap(),
        vec![b"{}".to_vec(), b"[1,2,3]".to_vec()]
    );
}

#[test]
fn a_message_and_a_half_then_the_rest() {
    let reader = reader_of(&[
        b"Content-Length: 2\r\n\r\n{}Content-Length: 4\r\n\r\nab",
        b"cd",
    ]);
    assert_eq!(
        read_all(reader).unwrap(),
        vec![b"{}".to_vec(), b"abcd".to_vec()]
    );
}

#[test]
fn an_oversize_body_fails_with_a_protocol_error() {
    let over = MAX_BODY as u64 + 1;
    let header = format!("Content-Length: {over}\r\n\r\n");
    // No body follows: the limit fails it before anything is read or
    // allocated for one.
    let err = read_all(reader_of(&[header.as_bytes()])).unwrap_err();
    assert!(
        matches!(
            err,
            TransportError::Protocol(ProtocolError::TooLarge { length }) if length == over
        ),
        "{err:?}"
    );
    let text = err.to_string();
    assert_eq!(
        text,
        kiln_l10n::tr!(
            "dap-error-too-large",
            bytes = over.to_string(),
            megabytes = 64_usize
        )
        .to_string()
    );
}

#[test]
fn a_body_at_the_limit_is_not_refused_by_its_header() {
    let header = format!("Content-Length: {MAX_BODY}\r\n\r\n");
    // The stream ends short of the body: an EOF, not a protocol error.
    let err = read_all(reader_of(&[header.as_bytes(), b"{}"])).unwrap_err();
    assert!(
        matches!(&err, TransportError::Io(io) if io.kind() == io::ErrorKind::UnexpectedEof),
        "{err:?}"
    );
}

#[test]
fn a_header_section_that_never_ends_fails() {
    let endless = vec![b'x'; 9 * 1024];
    let err = read_all(reader_of(&[b"X-Long: ", &endless])).unwrap_err();
    assert!(
        matches!(err, TransportError::Protocol(ProtocolError::HeaderTooLong)),
        "{err:?}"
    );
}

#[test]
fn an_unknown_header_is_ignored() {
    let reader = reader_of(&[
        b"Content-Type: application/vscode-jsonrpc; charset=utf-8\r\nContent-Length: 2\r\nX-Other: 99\r\n\r\n{}",
    ]);
    assert_eq!(read_all(reader).unwrap(), vec![b"{}".to_vec()]);
}

#[test]
fn content_length_is_case_insensitive() {
    for header in [
        "content-length",
        "CONTENT-LENGTH",
        "Content-length",
        "cOnTeNt-LeNgTh",
    ] {
        let frame = format!("{header}: 2\r\n\r\n{{}}");
        assert_eq!(
            read_all(reader_of(&[frame.as_bytes()])).unwrap(),
            vec![b"{}".to_vec()],
            "{header}"
        );
    }
    // Space around the value is allowed too.
    let frame = b"Content-Length:2 \r\n\r\n{}";
    assert_eq!(read_all(reader_of(&[frame])).unwrap(), vec![b"{}".to_vec()]);
}

#[test]
fn a_missing_or_bad_content_length_fails() {
    let err = read_all(reader_of(&[b"Content-Type: x\r\n\r\n{}"])).unwrap_err();
    assert!(
        matches!(
            err,
            TransportError::Protocol(ProtocolError::NoContentLength)
        ),
        "{err:?}"
    );
    let err = read_all(reader_of(&[b"Content-Length: two\r\n\r\n{}"])).unwrap_err();
    assert!(
        matches!(err, TransportError::Protocol(ProtocolError::BadHeader)),
        "{err:?}"
    );
    let err = read_all(reader_of(&[b"no colon here\r\n\r\n{}"])).unwrap_err();
    assert!(
        matches!(err, TransportError::Protocol(ProtocolError::BadHeader)),
        "{err:?}"
    );
}

#[test]
fn the_stream_ending_between_messages_is_clean_and_inside_one_is_not() {
    assert_eq!(read_all(reader_of(&[])).unwrap(), Vec::<Vec<u8>>::new());
    let err = read_all(reader_of(&[b"Content-Length: 2\r\n"])).unwrap_err();
    assert!(
        matches!(&err, TransportError::Io(io) if io.kind() == io::ErrorKind::UnexpectedEof),
        "{err:?}"
    );
}

#[test]
fn a_body_that_isnt_a_message_is_a_protocol_error() {
    let mut reader = reader_of(&[b"Content-Length: 12\r\n\r\n{\"seq\":true}"]);
    let err = async_io::block_on(reader.read()).unwrap_err();
    assert!(
        matches!(err, TransportError::Protocol(ProtocolError::BadMessage(_))),
        "{err:?}"
    );
}

/// Records each write and flush it is given.
#[derive(Clone, Default)]
struct Calls(Arc<Mutex<Vec<String>>>);

impl AsyncWrite for Calls {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.0
            .lock()
            .unwrap()
            .push(format!("write {}", String::from_utf8_lossy(buf)));
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.0.lock().unwrap().push("flush".into());
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

#[test]
fn a_message_is_one_write_then_a_flush() {
    let calls = Calls::default();
    let mut writer = FrameWriter::new(calls.clone());
    async_io::block_on(writer.write_body(b"{\"seq\":1}")).unwrap();
    assert_eq!(
        *calls.0.lock().unwrap(),
        vec![
            "write Content-Length: 9\r\n\r\n{\"seq\":1}".to_owned(),
            "flush".to_owned()
        ]
    );
}

#[test]
fn a_written_message_reads_back() {
    let (reader, writer): (PipeReader, PipeWriter) = pipe();
    let mut writer = FrameWriter::new(writer);
    let mut reader = FrameReader::new(reader);
    let sent = Message::Event(EventMessage::new::<event::Initialized>(
        4,
        &Default::default(),
    ));
    async_io::block_on(async {
        writer.write(&sent).await.unwrap();
        writer.close().await.unwrap();
        assert_eq!(reader.read().await.unwrap(), Some(sent));
        assert_eq!(reader.read().await.unwrap(), None);
    });
    // As the specification shows it on the wire.
    let wire = serde_json::to_value(Message::Event(EventMessage::new::<event::Initialized>(
        4,
        &Default::default(),
    )))
    .unwrap();
    assert_eq!(
        wire,
        json!({"seq": 4, "type": "event", "event": "initialized", "body": {}})
    );
}

#[test]
fn a_read_cancelled_while_waiting_resumes_where_it_was() {
    use futures::FutureExt as _;

    let (pipe_reader, mut writer) = pipe();
    let mut reader = FrameReader::new(pipe_reader);
    let mut write = |bytes: &[u8]| async_io::block_on(writer.write_all(bytes)).unwrap();
    // Cancelled waiting inside the header, then inside the body.
    write(b"Content-Length: ");
    assert!(reader.read_body().now_or_never().is_none());
    write(b"9\r\n\r\n{\"seq\"");
    assert!(reader.read_body().now_or_never().is_none());
    assert!(reader.read_body().now_or_never().is_none());
    write(b":1}");
    let body = reader.read_body().now_or_never().expect("pending").unwrap();
    assert_eq!(body, Some(b"{\"seq\":1}".to_vec()));
    // Cancelled with nothing buffered, and the next message is whole.
    assert!(reader.read_body().now_or_never().is_none());
    write(b"Content-Length: 2\r\n\r\n{}");
    let body = reader.read_body().now_or_never().expect("pending").unwrap();
    assert_eq!(body, Some(b"{}".to_vec()));
}
