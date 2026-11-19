---
task_uid: "01m7rf6ht0wvgpgm5kanm4h828"
task: "T430"
rev: 2
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "GH CI windows Test passes fastedit::language_pack_per_pack same_inputs_give_identical_tarballs (fails at 8ba6f008c: first differing offset 12, entries drwxr-xr-x/-rw-r--r-- under umask 022 vs drwx------/-rw------- under umask 077)"
    required: true
    manual: false
  - id: "C2"
    text: "The BSD tar branch is untouched, and with GNU tar a stage already at 755/644 gives the same archive bytes as before: language_pack_per_pack passes on the gate and on Linux CI"
    required: true
    manual: false
---

# Language pack tarballs differ by umask on Windows: chmod does not stick under Git for Windows, so GNU tar archives 0700/0600 modes; set the modes in tar (Mac T1969)

# Language pack: modes follow the umask on Windows

GH CI 44405849017 on 8ba6f008c, windows Test: same_inputs_give_identical_tarballs (language_pack_per_pack.rs:395) finds Tenmoku-language-pack-jvm.tar.gz different between the umask 022 and umask 077 runs. `tar -tvzf` lists the first with 755/644 entries and the second with 700/600.

Cause: scripts/package-language-pack.sh (lines 138-139) sets the stage's modes with `chmod 755/644`. Under Git for Windows (NTFS, no POSIX modes) chmod doesn't stick, so GNU tar archives whatever modes the umask produced. The comment at :143 rules out `--mode` because a single numeric mode can't give dirs 755 and files 644.

Fix: in the GNU tar branch, add a symbolic `--mode='a=rX,u+w'` (X: execute only for directories or files that already have an x bit). That gives 755 dirs and 644 files whatever the stage's bits. Keep the chmod calls for BSD tar. On Linux the stage is already 755/644, so the bytes don't change. The regression test already exists and fails only on Windows, so there's no `--regression-test`; evidence comes from GH CI windows after landing.
