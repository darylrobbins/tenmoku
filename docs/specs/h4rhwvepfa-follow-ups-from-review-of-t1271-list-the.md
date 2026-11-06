---
task_uid: "01m6pr8vewwq7gwrh4rhwvepfa"
task: "T1388"
rev: 1
type: "docs"
complexity: "xs"
criteria:
  - id: "C1"
    text: "README.md lists only the languages the default build bundles, matching fe-syntax default features, and names kotlin, csharp, scala, swift, cpp, sql, php and ruby as grammars downloaded on first open"
    required: true
    manual: false
  - id: "C2"
    text: "README.md has no instruction to symlink or otherwise install the tenmoku executable onto PATH, and still states its location inside the bundle"
    required: true
    manual: false
---

# Follow-ups from review of T1271: list the README's bundled and downloadable languages accurately (C++ and C# are downloads), and drop its CLI symlink step

Follow-ups from the codex review of T1271 (approved with two medium findings, review hzgqvp on candidate wvx2wg). Both must be fixed before the v0.1.0 tag.

F1 (README.md:13-14): the Features list overstates the default release. C++ and C# are not bundled: fe-syntax's default build leaves them out, and they are among the eight grammars that `extensions/grammars/pack.toml` downloads on first open (kotlin, csharp, scala, swift, cpp, sql, php, ruby). Fix: list only the languages the default build bundles (check fe-syntax's default features rather than guessing), then name the eight as WebAssembly grammars downloaded the first time a file of that language opens. Don't say "and more" or "other grammars" beyond what exists. The grammars-1 release that serves them is being published separately, before the tag, so describe them as available.

F2 (README.md:58 onward): the README tells readers to symlink the executable onto their PATH. The spec of T1271 forbade a CLI installation step. Fix: remove the symlink instructions and the `ln -s` block, and keep only the sentence giving the executable's location inside the bundle (`Tenmoku.app/Contents/MacOS/tenmoku`).

README.md only.
