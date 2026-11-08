---
task_uid: "01m6wj0vmfqmpavnnx19fszt91"
task: "T1612"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "scripts/test-bundle-macos.sh builds a bundle and checks that Contents/Resources/Licenses/ holds LICENSE, NOTICE, LICENSES/Apache-2.0.txt, LICENSES/MIT.txt, THIRD_PARTY.md, both icon-set licences, the tree-sitter licence and the licence of every grammar the app ships, each identical to its source"
    required: true
    manual: false
  - id: "C2"
    text: "The bundle is still signed after the copy (codesign --verify --deep --strict passes in the bundle test), and the Sparkle and font licence files are unchanged"
    required: true
    manual: false
---

# Ship every licence text the app carries (LICENSE, NOTICE, LICENSES/, THIRD_PARTY.md, icon and grammar licences) inside Tenmoku.app (T1511 F1 follow-up)

Follow-up of T1511's review finding F1 (the pre-existing part of it).

`scripts/bundle-macos.sh` copies no licence text into Tenmoku.app except Sparkle's (`Sparkle-LICENSE.txt`, line ~199).
T1511 adds the bundled fonts' OFL.txt. Everything else the app carries still ships without its notice:
- Tenmoku's own `LICENSE`, `NOTICE`, `LICENSES/Apache-2.0.txt` and `LICENSES/MIT.txt`;
- `THIRD_PARTY.md`;
- the icon sets' licences: `kiln/crates/kiln-ui/assets/icons/LICENSE` and `kiln/crates/kiln-ui/assets/file-icons/LICENSE`;
- the vendored tree-sitter runtime (`vendor/tree-sitter/LICENSE`) and any bundled grammar's LICENSE under
  `extensions/grammars/*/LICENSE` whose grammar is compiled into, or shipped with, the app.

Copy them into `Contents/Resources/Licenses/` before signing, under names that say what each covers (for example
`icons-LICENSE.txt`, `grammar-<name>-LICENSE.txt`), and have `scripts/test-bundle-macos.sh` check that every one is
present and matches its source byte for byte. Keep Sparkle's and T1511's font files where they are. `release.yml`
stays unchanged.
