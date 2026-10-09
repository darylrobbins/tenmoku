//! Reads through the chunk cache against the file's own bytes.

use std::fs;
use std::path::{Path, PathBuf};

use fe_fileview::{CACHE_CHUNKS, CHUNK_BYTES, FileView, Rows};
use rand::{Rng, SeedableRng, rngs::StdRng};

const MIB: u64 = 1024 * 1024;

fn write(dir: &tempfile::TempDir, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.path().join(name);
    fs::write(&path, bytes).unwrap();
    path
}

/// Text with 2-, 3- and 4-byte chars, invalid bytes and a lone continuation
/// byte, with some of each laid across every chunk edge.
fn mixed_bytes(len: usize, rng: &mut StdRng) -> Vec<u8> {
    const PIECES: [&[u8]; 9] = [
        b"plain ascii text ",
        "é".as_bytes(),
        "€".as_bytes(),
        "😀".as_bytes(),
        b"\n",
        b"\r\n",
        b"\xff\xfe",
        b"\x80",
        b"\xe2\x82", // a truncated 3-byte char
    ];
    let mut bytes = Vec::with_capacity(len + 16);
    while bytes.len() < len {
        bytes.extend_from_slice(PIECES[rng.random_range(0..PIECES.len())]);
    }
    bytes.truncate(len);
    let chunk = CHUNK_BYTES as usize;
    for (n, edge) in (chunk..len).step_by(chunk).enumerate() {
        let piece = PIECES[1 + n % 3]; // 2, 3 or 4 bytes
        let at = edge - 1 - n % (piece.len() - 1);
        bytes[at..at + piece.len()].copy_from_slice(piece);
        if n % 4 == 3 {
            bytes[edge] = 0xff;
        }
    }
    bytes
}

/// What `FileView::read` promises: back past at most 3 continuation bytes.
fn snap(bytes: &[u8], mut at: usize) -> usize {
    for _ in 0..3 {
        if at == 0 || at >= bytes.len() || bytes[at] & 0xC0 != 0x80 {
            break;
        }
        at -= 1;
    }
    at
}

#[test]
fn reads_match_the_file() {
    let mut rng = StdRng::seed_from_u64(241);
    let len = (8 * MIB) as usize;
    let bytes = mixed_bytes(len, &mut rng);
    let dir = tempfile::tempdir().unwrap();
    let view = FileView::open(write(&dir, "mixed.txt", &bytes)).unwrap();
    assert_eq!(view.len(), len as u64);

    let chunk = CHUNK_BYTES as usize;
    for i in 0..1000 {
        let start = if i % 2 == 0 {
            // Near a chunk edge, so ends straddle it.
            let edge = chunk * rng.random_range(1..len / chunk);
            edge - 4 + rng.random_range(0..8)
        } else {
            rng.random_range(0..=len)
        };
        let end = match i % 3 {
            0 => start + rng.random_range(0..16),
            1 => start + rng.random_range(0..2 * chunk),
            _ => start + rng.random_range(0..64 * 1024),
        }
        .min(len);
        let expected = String::from_utf8_lossy(&bytes[snap(&bytes, start)..snap(&bytes, end)]);
        let read = view.read(start as u64..end as u64).unwrap();
        assert_eq!(read, expected, "range {start}..{end}");
    }

    // Past the end clamps to the recorded length.
    let tail = view.read(len as u64 - 10..len as u64 + 100).unwrap();
    assert_eq!(
        tail,
        String::from_utf8_lossy(&bytes[snap(&bytes, len - 10)..])
    );
    assert_eq!(view.read(len as u64 + 5..len as u64 + 9).unwrap(), "");
}

#[test]
fn a_char_across_a_chunk_edge_decodes_whole() {
    let edge = CHUNK_BYTES as usize;
    let mut bytes = vec![b'a'; edge + 8];
    bytes[edge - 2..edge + 2].copy_from_slice("😀".as_bytes());
    let dir = tempfile::tempdir().unwrap();
    let view = FileView::open(write(&dir, "edge.txt", &bytes)).unwrap();

    assert_eq!(view.read(edge as u64 - 3..edge as u64 + 3).unwrap(), "a😀a");
    // Ends inside the char snap back to its start.
    assert_eq!(view.read(edge as u64 - 1..edge as u64 + 1).unwrap(), "");
    assert_eq!(view.read(edge as u64..edge as u64 + 3).unwrap(), "😀a");
    assert_eq!(view.read(edge as u64 - 3..edge as u64).unwrap(), "a");
}

fn numbered_lines(count: usize, terminator: &str) -> (Vec<String>, Vec<u8>) {
    // Long lines, so the first 60 cross a chunk edge.
    let lines: Vec<String> = (0..count)
        .map(|n| match n % 5 {
            0 => String::new(),
            1 => format!("{n} é😀 {}", "x".repeat(20_000)),
            _ => format!("line {n} {}", "y".repeat(n * 50)),
        })
        .collect();
    let mut bytes = Vec::new();
    for line in &lines {
        bytes.extend_from_slice(line.as_bytes());
        bytes.extend_from_slice(terminator.as_bytes());
    }
    (lines, bytes)
}

fn offset_after(lines: &[String], count: usize, terminator: &str) -> u64 {
    lines[..count]
        .iter()
        .map(|line| (line.len() + terminator.len()) as u64)
        .sum()
}

#[test]
fn first_rows_without_an_index() {
    let dir = tempfile::tempdir().unwrap();
    for (name, terminator) in [("lf.txt", "\n"), ("crlf.txt", "\r\n")] {
        let (lines, bytes) = numbered_lines(150, terminator);
        let view = FileView::open(write(&dir, name, &bytes)).unwrap();
        let end = offset_after(&lines, 60, terminator);
        assert!(
            end > CHUNK_BYTES,
            "{name}: the first screen should cross a chunk edge"
        );

        let first = view.rows_from(0, 60).unwrap();
        assert_eq!(
            first,
            Rows {
                lines: lines[..60].to_vec(),
                end
            },
            "{name}"
        );

        // The end is where the next screen starts; the last screen is short.
        let next = view.rows_from(first.end, 60).unwrap();
        assert_eq!(next.lines, lines[60..120], "{name}");
        let last = view.rows_from(next.end, 60).unwrap();
        assert_eq!(
            last,
            Rows {
                lines: lines[120..].to_vec(),
                end: bytes.len() as u64
            },
            "{name}"
        );
    }
}

#[test]
fn rows_split_a_crlf_across_a_chunk_edge_and_keep_an_unterminated_last_line() {
    let edge = CHUNK_BYTES as usize;
    let mut bytes = vec![b'a'; edge - 1];
    bytes.extend_from_slice(b"\r\n\r\nlast\r");
    let dir = tempfile::tempdir().unwrap();
    let view = FileView::open(write(&dir, "split.txt", &bytes)).unwrap();

    let rows = view.rows_from(0, 10).unwrap();
    let a = "a".repeat(edge - 1);
    assert_eq!(rows.lines, [a.as_str(), "", "last\r"]);
    assert_eq!(rows.end, bytes.len() as u64);
    assert_eq!(
        view.rows_from(0, 0).unwrap(),
        Rows {
            lines: vec![],
            end: 0
        }
    );
    assert_eq!(view.rows_from(edge as u64 + 1, 1).unwrap().lines, [""]);
}

fn write_sized(dir: &Path, len: u64) -> PathBuf {
    let path = dir.join("big.txt");
    let line = b"    let value = compute_something(x)?; // e\n";
    let mut bytes = Vec::with_capacity(len as usize);
    while (bytes.len() as u64) < len {
        bytes.extend_from_slice(line);
    }
    bytes.truncate(len as usize);
    fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn cache_stays_bounded() {
    let cap = CHUNK_BYTES * CACHE_CHUNKS as u64;
    assert_eq!(cap, 16 * MIB);
    let dir = tempfile::tempdir().unwrap();
    let len = 64 * MIB;
    let view = FileView::open(write_sized(dir.path(), len)).unwrap();
    let chunks = len / CHUNK_BYTES;

    for index in 0..chunks {
        let start = index * CHUNK_BYTES;
        view.read(start..start + CHUNK_BYTES).unwrap();
        assert!(
            view.cached_bytes() <= cap,
            "after chunk {index}: {}",
            view.cached_bytes()
        );
    }
    assert_eq!(view.cached_bytes(), cap, "the cache fills up");

    let mut rng = StdRng::seed_from_u64(64);
    for _ in 0..1000 {
        let start = rng.random_range(0..len);
        let end = (start + rng.random_range(0..2 * CHUNK_BYTES)).min(len);
        view.read(start..end).unwrap();
        assert!(view.cached_bytes() <= cap, "{}", view.cached_bytes());
    }
}

#[test]
fn a_view_is_send_and_sync() {
    fn shared<T: Send + Sync>() {}
    shared::<FileView>();
}
