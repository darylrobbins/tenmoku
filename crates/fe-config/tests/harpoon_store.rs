//! Harpoon's file marks: slots, caps, reordering, and per-project
//! persistence under `<state_dir>/harpoon/`.

use std::fs;
use std::path::{Path, PathBuf};

use fe_config::MarkList;
use fe_config::harpoon::{self, Added, MAX_MARKS};

fn p(s: &str) -> PathBuf {
    PathBuf::from(s)
}

#[test]
fn add_dedupes_caps_and_keeps_order() {
    let mut list = MarkList::default();
    assert_eq!(list.add(&p("src/a.rs")), Added::New(0));
    assert_eq!(list.add(&p("src/b.rs")), Added::New(1));
    // The same path twice keeps one slot, where it was.
    assert_eq!(list.add(&p("src/a.rs")), Added::Already(0));
    assert_eq!(list.files(), [p("src/a.rs"), p("src/b.rs")]);

    // Up to nine; a tenth is refused.
    for i in 2..MAX_MARKS {
        assert_eq!(list.add(&p(&format!("f{i}"))), Added::New(i));
    }
    assert_eq!(list.len(), 9);
    assert_eq!(list.add(&p("tenth")), Added::Full);
    assert_eq!(list.len(), 9);
    assert_eq!(list.position(&p("tenth")), None);
    // Already there still says where, full or not.
    assert_eq!(list.add(&p("src/b.rs")), Added::Already(1));

    // Only paths under the root.
    let mut small = MarkList::default();
    for outside in ["/etc/passwd", "../up", "a/../../b", "", "./a"] {
        assert_eq!(small.add(&p(outside)), Added::Invalid, "{outside:?}");
    }
    assert!(small.is_empty());

    // move_to reorders, the ones between shifting.
    assert!(list.move_to(0, 2));
    assert_eq!(&list.files()[..3], [p("src/b.rs"), p("f2"), p("src/a.rs")]);
    assert!(list.move_to(2, 0));
    assert_eq!(&list.files()[..3], [p("src/a.rs"), p("src/b.rs"), p("f2")]);
    assert!(!list.move_to(0, 9), "out of range changes nothing");
    assert!(!list.move_to(9, 0));
    assert_eq!(list.get(0), Some(Path::new("src/a.rs")));
    assert_eq!(list.get(9), None);

    // remove closes the gap.
    assert_eq!(list.remove(1), Some(p("src/b.rs")));
    assert_eq!(list.len(), 8);
    assert_eq!(&list.files()[..3], [p("src/a.rs"), p("f2"), p("f3")]);
    assert_eq!(list.get(1), Some(Path::new("f2")));
    assert_eq!(list.remove(8), None);
    // With a slot free, a new file goes last.
    assert_eq!(list.add(&p("tenth")), Added::New(8));
}

#[test]
fn lists_are_per_project_and_survive_reload() {
    let state = tempfile::tempdir().unwrap();
    let projects = tempfile::tempdir().unwrap();
    let (one, two) = (projects.path().join("one"), projects.path().join("two"));
    fs::create_dir_all(&one).unwrap();
    fs::create_dir_all(&two).unwrap();

    let one_file = harpoon::marks_file(state.path(), &one);
    let two_file = harpoon::marks_file(state.path(), &two);
    assert_ne!(one_file, two_file);
    assert_eq!(
        one_file.parent(),
        Some(state.path().join("harpoon").as_path())
    );
    // The key is stable and follows the canonical root.
    assert_eq!(harpoon::project_key(&one), harpoon::project_key(&one));
    let dotted = two.join("..").join("one");
    assert_eq!(harpoon::marks_file(state.path(), &dotted), one_file);

    let add = |file: &Path, path: &str| {
        harpoon::update(file, |list| {
            let added = list.add(&p(path));
            (matches!(added, Added::New(_)), added)
        })
        .unwrap()
    };
    add(&one_file, "b.rs");
    add(&one_file, "a.rs");
    add(&one_file, "c.rs");
    add(&two_file, "z.rs");
    harpoon::update(&one_file, |list| (list.move_to(2, 0), ())).unwrap();

    // save then load returns the same order, each project its own.
    let (loaded, error) = MarkList::load(&one_file);
    assert!(error.is_none(), "{error:?}");
    assert_eq!(loaded.files(), [p("c.rs"), p("b.rs"), p("a.rs")]);
    let (loaded, _) = MarkList::load(&two_file);
    assert_eq!(loaded.files(), [p("z.rs")]);

    let saved = MarkList::from_files([p("x"), p("y")]);
    let elsewhere = state.path().join("direct.toml");
    saved.save(&elsewhere).unwrap();
    let (loaded, error) = MarkList::load(&elsewhere);
    assert!(error.is_none(), "{error:?}");
    assert_eq!(loaded, saved);

    // A missing file is an empty list, without error.
    let (missing, error) = MarkList::load(&state.path().join("none.toml"));
    assert!(missing.is_empty());
    assert!(error.is_none());

    // A corrupt file loads empty, and stays on disk as it is.
    let corrupt = "files = [not toml";
    fs::write(&one_file, corrupt).unwrap();
    let (loaded, error) = MarkList::load(&one_file);
    assert!(loaded.is_empty());
    assert_eq!(error.unwrap().kind(), std::io::ErrorKind::InvalidData);
    // A read through `update` that changes nothing leaves it too.
    let (read, ()) = harpoon::update(&one_file, |_| (false, ())).unwrap();
    assert!(read.is_empty());
    assert_eq!(fs::read_to_string(&one_file).unwrap(), corrupt);
    // The next change replaces it.
    add(&one_file, "fresh.rs");
    let (loaded, error) = MarkList::load(&one_file);
    assert!(error.is_none(), "{error:?}");
    assert_eq!(loaded.files(), [p("fresh.rs")]);

    // Entries a hand edit put outside the root, or repeated, are dropped.
    fs::write(
        &two_file,
        "files = [\"a\", \"../out\", \"/abs\", \"a\", \"b\"]\n",
    )
    .unwrap();
    assert_eq!(MarkList::load(&two_file).0.files(), [p("a"), p("b")]);
}
