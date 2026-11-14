---
task_uid: "01m7c4m91dd0qayyc19jxmsnvk"
task: "T640"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "editorconfig_special_files_tests: a FIFO .editorconfig is skipped and the call returns (unix); a symlink to a regular file still applies; a 2 MiB file and a directory named .editorconfig are skipped"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-config editorconfig passes"
    required: true
    manual: false
---

# fe-config editorconfig: read .editorconfig only when it is a regular file under 1 MiB, so a FIFO or device named .editorconfig can never block the reader (T1788 F13 follow-up) (Mac T1837)

Follow-up of T1788 review finding F13 (helium T620, medium, pre-existing): `read_file` in crates/fe-config/src/editorconfig.rs (~179) calls `std::fs::read` on `dir/.editorconfig` whatever it is. A FIFO named `.editorconfig` in an opened folder (or any ancestor) blocks the reading thread forever; a device or a huge file reads unbounded.

Fix: open the file, check `metadata()` on the opened handle (follows a symlink to its target, so a symlink to a regular file still works), and read only a regular file, bounded at 1 MiB (an .editorconfig is a few KB). Anything else (FIFO, socket, device, directory, oversize) is skipped with the existing `warn_skipped` and treated as no file. Open the FIFO case without blocking (O_NONBLOCK on unix, then check the type) so the check itself cannot hang.

Tests in a new crates/fe-config/src/editorconfig_special_files_tests.rs declared from editorconfig.rs: a FIFO .editorconfig (unix only, made with mkfifo via libc or nix if already a dependency, else `std::process::Command::new("mkfifo")`) is skipped and returns promptly; a symlink to a regular .editorconfig still applies; a 2 MiB file is skipped; a directory named .editorconfig is skipped.
