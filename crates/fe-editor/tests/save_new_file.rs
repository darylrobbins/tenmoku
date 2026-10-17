//! A new file is saved with the umask's permissions, and on a volume with no
//! exclusive rename.

use fe_buffer::Buffer;
use fe_editor::EditorModel;

fn save_new(path: &std::path::Path, text: &str) {
    let mut model = EditorModel::new(Buffer::new(text), Some(path.to_path_buf()));
    model.buffer_mut().set_disk_stamp(None);
    model.save().unwrap();
}

#[cfg(unix)]
#[test]
fn a_new_file_gets_the_permissions_the_umask_gives() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let reference = dir.path().join("reference.txt");
    std::fs::write(&reference, "x").unwrap();
    let path = dir.path().join("new.txt");
    save_new(&path, "hello\n");
    let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&path), mode(&reference));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello\n");
}

#[cfg(target_os = "macos")]
struct Detach(String);

#[cfg(target_os = "macos")]
impl Drop for Detach {
    fn drop(&mut self) {
        let _ = std::process::Command::new("/usr/bin/hdiutil")
            .args(["detach", "-force", &self.0])
            .output();
    }
}

#[cfg(target_os = "macos")]
#[test]
fn a_new_file_saves_on_an_exfat_volume() {
    use std::process::Command;
    let dir = tempfile::tempdir().unwrap();
    let image = dir.path().join("disk.dmg");
    let mount = dir.path().join("mnt");
    std::fs::create_dir(&mount).unwrap();
    let made = Command::new("/usr/bin/hdiutil")
        .args([
            "create", "-quiet", "-size", "4m", "-fs", "ExFAT", "-layout", "NONE",
        ])
        .arg(&image)
        .output()
        .unwrap();
    assert!(made.status.success(), "{made:?}");
    let attached = Command::new("/usr/bin/hdiutil")
        .args(["attach", "-nobrowse", "-mountpoint"])
        .arg(&mount)
        .arg(&image)
        .output()
        .unwrap();
    assert!(attached.status.success(), "{attached:?}");
    let stdout = String::from_utf8_lossy(&attached.stdout);
    let device = stdout
        .split_whitespace()
        .next()
        .unwrap_or_else(|| panic!("no device in {stdout:?}"))
        .to_string();
    let _detach = Detach(device);

    let path = mount.join("new.txt");
    save_new(&path, "on exfat\n");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "on exfat\n");
}
