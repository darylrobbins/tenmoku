//! Another process truncating, extending and rewriting the file under a view.

use std::fs::{self, File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};

use fe_fileview::{CHUNK_BYTES, FileView, Freshness, ReadError};
use rand::{Rng, SeedableRng, rngs::StdRng};

const MIB: u64 = 1024 * 1024;

#[test]
fn truncation_and_rewrites_never_panic() {
    let mut rng = StdRng::seed_from_u64(4);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("log.txt");
    fs::write(&path, "a log line é😀\n".repeat((MIB / 18) as usize)).unwrap();
    let file = OpenOptions::new().write(true).open(&path).unwrap();

    let mut view = FileView::open(&path).unwrap();
    assert_eq!(view.check(), Freshness::Unchanged);
    let (mut oks, mut changed) = (0, 0);
    for i in 0..250 {
        // A fresh view now and then, so its cache is cold and reads reach the
        // file as it is now.
        if i % 5 == 0 {
            view = FileView::open(&path).unwrap();
        }
        let current = file.metadata().unwrap().len();
        let shortest = current.min(view.len());
        match rng.random_range(0..3) {
            0 if shortest > 0 => {
                file.set_len(rng.random_range(0..shortest)).unwrap();
                assert_eq!(
                    view.check(),
                    Freshness::Changed,
                    "iteration {i}: after a truncation"
                );
            }
            0 | 1 => file.set_len(rng.random_range(current..=2 * MIB)).unwrap(),
            _ => {
                let at = rng.random_range(0..=current);
                let len = rng.random_range(1..64 * 1024);
                let mut bytes = vec![0; len];
                rng.fill(&mut bytes[..]);
                (&file).seek(SeekFrom::Start(at)).unwrap();
                (&file).write_all(&bytes).unwrap();
            }
        }

        let len = view.len();
        for _ in 0..3 {
            let start = rng.random_range(0..=len + 1024);
            let end = start + rng.random_range(0..2 * CHUNK_BYTES);
            let results = [
                view.read(start..end).map(drop),
                view.rows_from(start.min(len), 20).map(drop),
            ];
            for result in results {
                match result {
                    Ok(()) => oks += 1,
                    Err(ReadError::Changed) => changed += 1,
                    Err(error) => panic!("iteration {i}: {start}..{end}: {error}"),
                }
            }
        }
    }
    assert!(oks > 0 && changed > 0, "{oks} reads ok, {changed} changed");
}

#[test]
fn check_sees_a_replaced_file_and_a_deleted_one() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rotated.log");
    fs::write(&path, "same length\n").unwrap();
    let view = FileView::open(&path).unwrap();
    assert_eq!(view.check(), Freshness::Unchanged);

    // Rotated: a new file of the same length and mtime at the same path.
    let next = dir.path().join("next.log");
    fs::write(&next, "same length\n").unwrap();
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    File::options()
        .write(true)
        .open(&next)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    assert_eq!(fs::metadata(&next).unwrap().modified().unwrap(), modified);
    fs::rename(&next, &path).unwrap();
    assert_eq!(view.check(), Freshness::Changed);
    // The view still reads the file it opened.
    assert_eq!(view.read(0..4).unwrap(), "same");

    fs::remove_file(&path).unwrap();
    assert_eq!(view.check(), Freshness::Changed);
}

#[test]
fn a_short_read_inside_the_recorded_length_is_changed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("short.txt");
    fs::write(&path, vec![b'x'; 3 * CHUNK_BYTES as usize]).unwrap();
    let view = FileView::open(&path).unwrap();
    assert_eq!(view.read(0..10).unwrap(), "xxxxxxxxxx");

    let mut file = OpenOptions::new().write(true).open(&path).unwrap();
    file.set_len(CHUNK_BYTES + 100).unwrap();
    // The cached chunk still reads; the cut one and the one past the cut don't.
    assert_eq!(view.read(0..10).unwrap(), "xxxxxxxxxx");
    assert!(matches!(
        view.read(CHUNK_BYTES..CHUNK_BYTES + 10),
        Err(ReadError::Changed)
    ));
    assert!(matches!(
        view.read(2 * CHUNK_BYTES..2 * CHUNK_BYTES + 1),
        Err(ReadError::Changed)
    ));
    assert!(
        matches!(view.rows_from(0, 1), Err(ReadError::Changed)),
        "a line running into the cut"
    );

    // Growing back with other bytes reads as the new bytes, within the old length.
    file.seek(SeekFrom::End(0)).unwrap();
    file.write_all(&vec![b'y'; 2 * CHUNK_BYTES as usize])
        .unwrap();
    assert_eq!(
        view.read(2 * CHUNK_BYTES..2 * CHUNK_BYTES + 3).unwrap(),
        "yyy"
    );
    assert_eq!(view.check(), Freshness::Changed);
}
