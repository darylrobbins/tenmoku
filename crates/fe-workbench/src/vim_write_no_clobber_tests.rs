//! Past `:w {path}`'s and `:sav`'s check for an existing file, the write
//! itself refuses one: `E13`, with the file left as it was.

use std::path::Path;

use gpui::TestAppContext;

use super::super::vim_host_tests::{active, dirty, ex, message, window, write};

const E13: &str = "E13: File exists (add ! to override)";

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

#[gpui::test]
fn the_write_refuses_a_file_that_exists(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, &["a.txt"]);
    let (workbench, mut vcx) = window(cx, root, &["a.txt"]);
    let cx = &mut vcx;
    let a = active(&workbench, cx);
    cx.simulate_keystrokes("x");
    let other = root.join("other.txt");
    std::fs::write(&other, "theirs\n").unwrap();

    // Save As, past the check the command makes first.
    cx.update(|window, cx| {
        workbench.update(cx, |w, cx| {
            w.save_editor_as_with(&a, other.clone(), false, window, cx)
        })
    });
    cx.run_until_parked();
    assert_eq!(message(&workbench, cx).as_deref(), Some(E13));
    assert_eq!(read(&other), "theirs\n");
    assert!(dirty(&a, cx));

    // A copy, likewise.
    let copy = a.update(cx, |a, cx| a.write_copy(other.clone(), false, cx));
    cx.run_until_parked();
    assert!(cx.foreground_executor().block_test(copy).is_err());
    assert_eq!(read(&other), "theirs\n");

    // With ! they overwrite.
    ex("sav! other.txt", cx);
    assert_eq!(read(&other), "ne\n");
    assert!(!dirty(&a, cx));
}

#[gpui::test]
fn w_bang_overwrites_a_copy(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, &["a.txt"]);
    let (workbench, mut vcx) = window(cx, root, &["a.txt"]);
    let cx = &mut vcx;
    cx.simulate_keystrokes("x");
    let other = root.join("other.txt");
    std::fs::write(&other, "theirs\n").unwrap();
    ex("w other.txt", cx);
    assert_eq!(message(&workbench, cx).as_deref(), Some(E13));
    assert_eq!(read(&other), "theirs\n");
    ex("w! other.txt", cx);
    assert_eq!(read(&other), "ne\n");
}

#[gpui::test]
#[cfg(unix)]
fn a_dangling_symlink_is_an_existing_file(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, &["a.txt"]);
    let (workbench, mut vcx) = window(cx, root, &["a.txt"]);
    let cx = &mut vcx;
    cx.simulate_keystrokes("x");
    let link = root.join("link.txt");
    std::os::unix::fs::symlink(root.join("missing.txt"), &link).unwrap();
    for line in ["w link.txt", "sav link.txt"] {
        ex(line, cx);
        assert_eq!(message(&workbench, cx).as_deref(), Some(E13), "{line}");
        assert!(!root.join("missing.txt").exists(), "{line}");
    }
}

/// Runs `hook` once, between the host's check for the destination and the
/// write.
fn after_check(hook: impl FnOnce(&Path) + 'static) {
    super::AFTER_CHECK.with_borrow_mut(|slot| *slot = Some(Box::new(hook)));
}

#[gpui::test]
fn a_file_created_after_the_check_gives_e13(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, &["a.txt"]);
    let (workbench, mut vcx) = window(cx, root, &["a.txt"]);
    let cx = &mut vcx;
    let a = active(&workbench, cx);
    cx.simulate_keystrokes("x");
    let other = root.join("other.txt");
    for line in ["w other.txt", "sav other.txt"] {
        after_check(|path| std::fs::write(path, "theirs\n").unwrap());
        ex(line, cx);
        assert_eq!(message(&workbench, cx).as_deref(), Some(E13), "{line}");
        assert_eq!(read(&other), "theirs\n", "{line}");
        assert!(dirty(&a, cx), "{line}");
        std::fs::remove_file(&other).unwrap();
    }

    // With !, they overwrite.
    after_check(|path| std::fs::write(path, "theirs\n").unwrap());
    ex("w! other.txt", cx);
    assert_eq!(read(&other), "ne\n");
    assert!(dirty(&a, cx));
    after_check(|path| std::fs::write(path, "theirs\n").unwrap());
    ex("sav! other.txt", cx);
    assert_eq!(read(&other), "ne\n");
    assert!(!dirty(&a, cx));
}

#[gpui::test]
fn a_copys_destination_is_held_while_it_is_written(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, &["a.txt"]);
    let (workbench, mut vcx) = window(cx, root, &["a.txt"]);
    let cx = &mut vcx;
    cx.simulate_keystrokes("x");
    let held = std::rc::Rc::new(std::cell::Cell::new(false));
    let seen = held.clone();
    after_check(move |path| seen.set(crate::open::reserved_paths().iter().any(|p| p == path)));
    ex("w other.txt", cx);
    assert!(held.get(), "held during the write");
    assert!(crate::open::reserved_paths().is_empty(), "let go after");
    assert_eq!(read(&root.join("other.txt")), "ne\n");
    drop(workbench);
}

#[cfg(unix)]
#[gpui::test]
#[cfg(unix)]
fn a_dangling_symlink_created_after_the_check_gives_e13(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, &["a.txt"]);
    let (workbench, mut vcx) = window(cx, root, &["a.txt"]);
    let cx = &mut vcx;
    let a = active(&workbench, cx);
    cx.simulate_keystrokes("x");
    let referent = root.join("referent.txt");
    for line in ["w link.txt", "sav link.txt"] {
        let target = referent.clone();
        after_check(move |path| std::os::unix::fs::symlink(&target, path).unwrap());
        ex(line, cx);
        assert_eq!(message(&workbench, cx).as_deref(), Some(E13), "{line}");
        assert!(!referent.exists(), "{line}");
        assert!(dirty(&a, cx), "{line}");
        std::fs::remove_file(root.join("link.txt")).unwrap();
    }
}

#[gpui::test]
fn a_copy_refuses_a_destination_another_buffer_holds(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, &["a.txt"]);
    let (workbench, mut vcx) = window(cx, root, &["a.txt"]);
    let cx = &mut vcx;
    cx.simulate_keystrokes("x");
    let other = root.join("other.txt");
    let held = fe_buffer::Buffer::new("");
    let reservation =
        crate::open::reserve(held.id(), other.clone(), crate::open::file_identity(&other));
    ex("w! other.txt", cx);
    let shown = message(&workbench, cx);
    assert!(
        shown
            .as_deref()
            .is_some_and(|m| m.contains("being saved by another tab")),
        "{shown:?}"
    );
    assert!(!other.exists());
    drop(reservation);
    ex("w other.txt", cx);
    assert_eq!(read(&other), "ne\n");
}
