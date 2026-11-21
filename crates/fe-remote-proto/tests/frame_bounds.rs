//! The header's layout and the decoder's bounds (D § 2.1).

use fe_remote_proto::{
    CHUNK, FrameError, HEADER, Header, Kind, MAX_BODY, Status, decode_all, decode_header, encode,
    encode_header, read_frame,
};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

/// Records the largest allocation made on each thread, so a test can show
/// that a decode never asked for a body the header announced.
struct Largest;

thread_local! {
    static LARGEST: Cell<usize> = const { Cell::new(0) };
}

unsafe impl GlobalAlloc for Largest {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = LARGEST.try_with(|l| l.set(l.get().max(layout.size())));
        // SAFETY: the caller's contract for `alloc` is passed through.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the caller's contract for `dealloc` is passed through.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOC: Largest = Largest;

fn largest_allocation_in(f: impl FnOnce()) -> usize {
    LARGEST.with(|l| l.set(0));
    f();
    LARGEST.with(|l| l.get())
}

fn header(len: u32, kind: u8, flags: u8) -> [u8; HEADER] {
    let mut h = [0u8; HEADER];
    h[0..4].copy_from_slice(&len.to_le_bytes());
    h[4] = kind;
    h[5] = flags;
    h
}

#[test]
fn the_header_is_twelve_bytes_in_the_decisions_layout() {
    let h = encode_header(&Header {
        kind: Kind::End,
        chan: 0x0201,
        id: 0x0605_0403,
        body_len: 3,
    })
    .unwrap();
    assert_eq!(HEADER, 12);
    assert_eq!(
        h,
        [
            11,
            0,
            0,
            0,
            Kind::End as u8,
            0,
            0x01,
            0x02,
            0x03,
            0x04,
            0x05,
            0x06
        ]
    );
    assert_eq!(
        decode_header(&h).unwrap(),
        Header {
            kind: Kind::End,
            chan: 0x0201,
            id: 0x0605_0403,
            body_len: 3
        }
    );
}

#[test]
fn the_bounds_are_one_mebibyte_and_sixty_four_kibibytes() {
    assert_eq!(MAX_BODY, 1024 * 1024);
    assert_eq!(CHUNK, 64 * 1024);
}

#[test]
fn a_body_of_exactly_max_body_is_accepted() {
    let h = header(8 + MAX_BODY as u32, Kind::Data as u8, 0);
    assert_eq!(decode_header(&h).unwrap().body_len, MAX_BODY);
}

#[test]
fn a_length_over_the_bound_is_refused() {
    for len in [8 + MAX_BODY as u32 + 1, u32::MAX] {
        let h = header(len, Kind::Data as u8, 0);
        assert_eq!(decode_header(&h), Err(FrameError::TooLong(len)));
    }
}

#[test]
fn a_length_over_the_bound_is_refused_before_allocating() {
    let mut stream = header(u32::MAX, Kind::Data as u8, 0).to_vec();
    stream.extend_from_slice(&[0; 64]);
    let largest = largest_allocation_in(|| {
        let err = read_frame(&mut stream.as_slice()).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert!(decode_all(&stream).is_err());
    });
    assert!(largest < MAX_BODY, "allocated {largest} bytes");
}

#[test]
fn an_encoder_refuses_a_body_over_the_bound() {
    let body = vec![0u8; MAX_BODY + 1];
    assert!(matches!(
        fe_remote_proto::encode_raw(Kind::Data, 64, 1, &body),
        Err(FrameError::BodyTooLong(_))
    ));
}

#[test]
fn an_unknown_kind_is_refused() {
    for kind in [0, 11, 0xff] {
        assert_eq!(
            decode_header(&header(8, kind, 0)),
            Err(FrameError::BadKind(kind))
        );
    }
}

#[test]
fn non_zero_flags_are_refused() {
    let h = header(8, Kind::Ping as u8, 0x80);
    assert_eq!(decode_header(&h), Err(FrameError::BadFlags(0x80)));
}

#[test]
fn a_length_shorter_than_the_header_is_refused() {
    for len in 0..8 {
        assert_eq!(
            decode_header(&header(len, Kind::Ping as u8, 0)),
            Err(FrameError::TooShort(len))
        );
    }
}

#[test]
fn a_truncated_frame_is_refused() {
    let frame = encode(Kind::End, 1, 7, &Status::Ok).unwrap();
    for cut in 1..frame.len() {
        assert!(
            matches!(decode_all(&frame[..cut]), Err(FrameError::Truncated { .. })),
            "cut at {cut}"
        );
    }
}

#[test]
fn bytes_after_a_body_are_refused() {
    let mut frame = encode(Kind::End, 1, 7, &Status::Ok).unwrap();
    frame.push(0);
    frame[0] += 1;
    assert!(matches!(
        decode_all(&frame),
        Err(FrameError::Trailing { extra: 1, .. })
    ));
}

#[test]
fn a_control_kind_off_channel_zero_is_refused() {
    let frame = encode(Kind::Ping, 1, 7, &()).unwrap();
    assert!(matches!(
        decode_all(&frame),
        Err(FrameError::BadChannel {
            kind: Kind::Ping,
            chan: 1
        })
    ));
}

#[test]
fn a_request_on_a_channel_without_ops_is_refused() {
    let frame = encode(Kind::Request, 4, 7, &()).unwrap();
    assert!(matches!(
        decode_all(&frame),
        Err(FrameError::BadChannel {
            kind: Kind::Request,
            chan: 4
        })
    ));
}

#[test]
fn data_on_a_dynamic_channel_is_opaque() {
    let frame = fe_remote_proto::encode_raw(Kind::Data, 64, 7, b"Content-Length: 2").unwrap();
    assert_eq!(decode_all(&frame).unwrap()[0].body_len, 17);
}
