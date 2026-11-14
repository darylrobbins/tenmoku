use super::*;

const CONFIG: &str = "[*]\nindent_style = tab\n";

fn style(dir: &Path) -> Option<IndentKind> {
    editorconfig_for(&dir.join("a.txt")).indent_style
}

#[cfg(unix)]
#[test]
fn a_fifo_is_skipped_and_returns() {
    let dir = tempfile::tempdir().unwrap();
    let fifo = dir.path().join(".editorconfig");
    let status = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap();
    assert!(status.success());
    let (tx, rx) = std::sync::mpsc::channel();
    let path = dir.path().to_path_buf();
    std::thread::spawn(move || {
        let _ = tx.send(style(&path));
    });
    let got = rx
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("blocked on a FIFO");
    assert_eq!(got, None);
}

#[cfg(unix)]
#[test]
fn a_symlink_to_a_regular_file_applies() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("real.cfg");
    std::fs::write(&real, CONFIG).unwrap();
    std::os::unix::fs::symlink(&real, dir.path().join(".editorconfig")).unwrap();
    assert_eq!(style(dir.path()), Some(IndentKind::Tab));
}

#[test]
fn a_two_mib_file_is_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let text = format!("{CONFIG}# {}\n", "x".repeat(2 * 1024 * 1024));
    std::fs::write(dir.path().join(".editorconfig"), text).unwrap();
    assert_eq!(style(dir.path()), None);
}

#[test]
fn a_directory_named_editorconfig_is_skipped() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".editorconfig")).unwrap();
    assert_eq!(style(dir.path()), None);
}
