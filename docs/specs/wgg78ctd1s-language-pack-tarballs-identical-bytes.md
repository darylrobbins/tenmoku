---
task_uid: "01m796k9zx88562awgg78ctd1s"
task: "T1761"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "same_inputs_give_identical_tarballs prints tar listings of both archives and the first differing offset on mismatch"
    required: true
    manual: false
  - id: "C2"
    text: "Linux and Mac tarball bytes and sha256 are unchanged by the script edit (before/after comparison in the finish notes)"
    required: true
    manual: false
---

# Language pack tarballs: identical bytes on Windows too, and say what differs when they aren't

Windows CI: `same_inputs_give_identical_tarballs` (crates/fastedit/tests/language_pack_per_pack.rs:363) finds
Tenmoku-language-pack-jvm.tar.gz differing between two package runs (umask 022 / UTC vs 077 / Pacific/Auckland,
1.1 s apart). scripts/package-language-pack.sh normalises with find|sort, ustar, fixed owner, and times and modes set
in the stage (lines ~129-146). On Windows the script runs under Git-bash `sh` with whichever `tar` is first on PATH
(System32 bsdtar vs Git's GNU tar); mode bits (chmod no-ops on NTFS / umask mapping), mtime precision or
`find` order of the jvm pack's extra files are the suspects. The other pack (no --pack) passed, so look at what the
jvm pack stages that the default doesn't.

Step 1 (diagnostic, also lands on its own): make the test's mismatch message print `tar -tvzf` listings of both
archives and the first differing byte offset, so the next Windows CI run says what differs. Step 2: fix the script
so mode, mtime and order are normalised explicitly on every OS (e.g. `touch -d`/`-t` and chmod set per file
with `find ... -exec`, and with GNU tar `--mtime`/`--mode` where available) without changing output on Linux/Mac:
the per-pack golden hashes in docs/ and the sha256 checks must still hold there.

Can only be proven on Windows CI; Linux/Mac must stay byte-identical (run the test, plus compare the tarball sha
before/after on Linux). If step 1 shows the cause is outside the script (a staged file differs), respec scope.

## Implementation notes

- GNU tar branch now adds `--mtime='2000-01-01 00:00:00 UTC' --sort=name`. No `--mode`: GNU tar's applies to files and directories alike and `X` still honours incoming execute bits, so the stage's chmod stays the mode source.
- C2 evidence (Linux x86_64, GNU tar 1.35, host neon): base (4c2adf0) vs candidate script on one fixture, default and `--pack jvm` runs. Both archives byte-identical (`cmp`), sha256 `0283807d5889368faee44687eaab59cdffc000556134144da3ababb7a91b326f` for all four files. Mac (bsdtar) identical too (branch untouched).
- The Windows fix is unproven until a Windows CI run; the test's mismatch message now prints both listings and the first differing offset.
