# Fixtures for the XCUITest suite

Each folder is a fixture that a test declares in `../TenmokuE2E/Fixtures.swift`.
`scripts/e2e-macos.sh` copies its `project/` to the test's scratch project, once per test
and iteration, and a `settings/<name>.toml` the test declares to the scratch config's
`settings.toml`. The tests check the copy against these files, byte for byte, before they
launch the app, so a set is always fresh. `--fixture <name>` runs the tests that declare
`<name>`.

- `plain`: an empty `notes.txt`.
- `two-files`: `a.txt` and `b.txt`, one line each.
- `encodings`: `windows-1252.txt` (no BOM, guessed), `utf16le-bom.txt`, `utf8-bom.txt`,
  `crlf.txt` (UTF-8, CRLF line endings). Binary: edit them with a script, not an editor.
- `markdown`: `notes.md` and `notes.txt`.
- `vim`: `notes.txt` (`alpha`, `beta`); `settings/off.toml` and
  `settings/unnamedplus.toml` turn vim on with each `vim.clipboard` value.
- `recent`: `notes.txt` and `quiz.txt` (a name no scratch path's letters spell, for the
  Open Recent query).
- `lines`: `lines.txt`, 40 numbered lines.
