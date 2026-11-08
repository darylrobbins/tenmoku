#![cfg(target_os = "macos")]

use std::{fs, path::Path, process::Command};

fn xattr(args: &[&str], path: &Path) -> String {
    let out = Command::new("/usr/bin/xattr")
        .args(args)
        .arg(path)
        .output()
        .unwrap();
    assert!(out.status.success(), "xattr {args:?} {path:?}: {out:?}");
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

#[test]
fn duplicate_keeps_extended_attributes() {
    let tmp = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(tmp.path()).unwrap();
    fs::write(root.join("a.txt"), "a").unwrap();
    fs::create_dir(root.join("d")).unwrap();
    fs::write(root.join("d/b.txt"), "b").unwrap();
    for p in ["a.txt", "d", "d/b.txt"] {
        xattr(&["-w", "com.example.fastedit", "value"], &root.join(p));
    }

    let file = fe_project::duplicate(&root, &root.join("a.txt")).unwrap();
    let dir = fe_project::duplicate(&root, &root.join("d")).unwrap();
    assert!(file.ends_with("a copy.txt") && dir.ends_with("d copy"));
    for p in [file, dir.clone(), dir.join("b.txt")] {
        assert_eq!(xattr(&["-p", "com.example.fastedit"], &p), "value", "{p:?}");
    }
}
