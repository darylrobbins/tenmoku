---
task_uid: "01m7qpes9a9wg1ve9hdw5fk2dd"
task: "T1955"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "`tests::pdf_notice::a_pdf_opens_in_the_notice_not_hex_or_text`: a file with `%PDF-1.7` then NUL-heavy bytes, one with `%PDF-` after 100 bytes of junk, and a `.pdf` whose body is ASCII all open as `Opened::Pdf` from the tree, quick open and `Pane::open`, and a PDF past the large-file threshold does too without being read beyond the probe (`loader::test_support::lower_threshold`)."
    required: true
    manual: false
  - id: "C2"
    text: "`tests::pdf_notice::other_files_open_as_before`: a text file, a binary non-PDF, an image, an SVG and a BOM file open as they did (image_viewer_open_tests, hex_viewer tests, large_file_tests pass)."
    required: true
    manual: false
  - id: "C3"
    text: "`tests::pdf_notice::open_in_default_app_passes_the_path_to_the_seam` and `::open_as_hex_replaces_the_notice_in_place`: the button and the command each do their job; the seam records the exact path; the hex tab then shows the file."
    required: true
    manual: false
  - id: "C4"
    text: "`tests::pdf_notice::a_saved_layout_reopens_the_notice` and `::a_missing_pdf_is_not_restored`."
    required: true
    manual: false
  - id: "C5"
    text: "`cargo tree --workspace` gains no PDF crate and `git diff` touches no Cargo.toml dependency table (state it in the finish note); `scripts/check-no-gpui.sh` and `cargo deny check`'s set are unchanged."
    required: true
    manual: false
  - id: "C6"
    text: "A UI audit surface `pdf_notice` (audit/pdf_notice.rs, mod line and SURFACES entry in audit_scenes.rs, a Captures row in docs/decisions/25w38k498b-ui-polish-audit.md): `cargo run -p fe-workbench --example screenshots -- audit \u003cdir\u003e` exits 0 and writes pdf_notice_dark.png and pdf_notice_light.png (Mac)."
    required: true
    manual: false
  - id: "C7"
    text: "The l10n lint and `cargo clippy --workspace` pass; every string is in a .ftl file; pdf_notice.rs's module doc states \"detection only, no rendering, the fallback for the PDF plugin\"."
    required: true
    manual: false
---

# PDFs open in a native 'This is a PDF' view (Open in Default App, Open as Hex) instead of hex or text; detection only, no rendering (core slot for the PDF plugin)

## Ask

The user asked: "Can we package a pdf viewer as a plugin that we prompt them to download when opening a pdf?" and then: "the PDF viewer is big enough and niche enough that it makes more sense to package as a plugin rather than as part of the core and make it more bloated." This card is the core half: Tenmoku detects a PDF and says so. It does not render one. The plugin half is the design card "Custom viewers from plugins" (docs/decisions/<lane>-plugin-viewers.md).

## Problem

A PDF opens today as garbage: `loader::open` (crates/fe-workbench/src/loader.rs:67) sends it to the read-only hex viewer when its first 8 KiB (`BINARY_PROBE`) holds a NUL (`is_binary`, :108), and as a text buffer when it doesn't (a PDF of ASCII-85 streams, or a small one). Neither says what the file is.

## Change

- **Detect.** A file is a PDF when its first 1024 bytes hold `%PDF-` (the spec lets the header sit past byte 0) or, failing that, when its extension is `pdf` (any case) and it is not otherwise text-sized garbage: extension alone is enough, magic alone is enough. Add `Opened::Pdf` and `loader::is_pdf(path)`, checked in `open` after `is_image` and before `is_binary` and the large-file path. A PDF of any size opens in this view, and its bytes are never read past the probe.
- **View.** A small pane tab, `PdfNotice` (new `crates/fe-gpui/src/pdf_notice.rs`, modelled on `ImageViewer` / `HexViewer`, PaneItem KIND `fastedit.pdf_notice`), centred in the editor background: the file name, the size, the line "This is a PDF. Tenmoku can't show PDFs yet." and two buttons, **Open in Default App** and **Open as Hex**. No new theme tokens. Never dirty; read-only status.
- **Open in Default App** calls GPUI's `App::open_with_system(path)` (the OS handler: `open`, `xdg-open`, ShellExecute), through a seam like `path_commands::RevealSeam` so tests record the path instead of launching anything. No OS cfg is needed, so none is added. Command `pdf.open_in_default_app` (context `PdfNotice`).
- **Open as Hex** reuses `hex.open_as_hex`: widen its context to `editor || ImageViewer || PdfNotice` (workbench_commands.rs:1967) so it replaces the tab in place. `hex.open_as_text` from the hex tab still works.
- **The slot for the plugin.** Document in `pdf_notice.rs` that this view is the fallback and the place the install prompt and the plugin's view later plug in (the design card says how). Do not build that here.
- **No PDF parsing or rendering dependency may be added to core**: no Cargo.toml in the workspace gains a PDF crate (pdfium, mupdf, lopdf, hayro, pdf-rs and so on). PDF rendering ships only as a plugin, so core stays small. The `%PDF-` check is five bytes.
- **Session restore.** serialize keeps the path; restore opens it again, a missing file is not restored.
- Strings are Fluent (`crates/fe-gpui/l10n/en-US/pdf.ftl`, titles in commands.ftl), `debug_selector` ids use `verbatim!`, and the l10n lint passes.

## Scope

crates/fe-workbench/src/loader.rs, crates/fe-workbench/src/pane.rs, crates/fe-workbench/src/workbench.rs, crates/fe-workbench/src/workbench_commands.rs, crates/fe-workbench/src/lib.rs, crates/fastedit/src/main.rs (the `Opened` match at :110), crates/fe-gpui/src/pdf_notice.rs, crates/fe-gpui/src/pdf_notice/**, crates/fe-gpui/src/lib.rs, crates/fe-gpui/src/commands.rs, crates/fe-gpui/l10n/en-US/pdf.ftl, crates/fe-gpui/l10n/en-US/commands.ftl (or the file the hex titles live in), crates/fe-workbench/src/tests/pdf_notice.rs, crates/fe-workbench/src/tests/mod.rs, crates/fe-workbench/examples/screenshots/audit/pdf_notice.rs, crates/fe-workbench/examples/screenshots/audit_scenes.rs, docs/decisions/25w38k498b-ui-polish-audit.md, docs/decisions/*-pdf-notice.md. New tests go in the new files.

## Criteria

- C1 `tests::pdf_notice::a_pdf_opens_in_the_notice_not_hex_or_text`: a file with `%PDF-1.7` then NUL-heavy bytes, one with `%PDF-` after 100 bytes of junk, and a `.pdf` whose body is ASCII all open as `Opened::Pdf` from the tree, quick open and `Pane::open`, and a PDF past the large-file threshold does too without being read beyond the probe (`loader::test_support::lower_threshold`).
- C2 `tests::pdf_notice::other_files_open_as_before`: a text file, a binary non-PDF, an image, an SVG and a BOM file open as they did (image_viewer_open_tests, hex_viewer tests, large_file_tests pass).
- C3 `tests::pdf_notice::open_in_default_app_passes_the_path_to_the_seam` and `::open_as_hex_replaces_the_notice_in_place`: the button and the command each do their job; the seam records the exact path; the hex tab then shows the file.
- C4 `tests::pdf_notice::a_saved_layout_reopens_the_notice` and `::a_missing_pdf_is_not_restored`.
- C5 `cargo tree --workspace` gains no PDF crate and `git diff` touches no Cargo.toml dependency table (state it in the finish note); `scripts/check-no-gpui.sh` and `cargo deny check`'s set are unchanged.
- C6 A UI audit surface `pdf_notice` (audit/pdf_notice.rs, mod line and SURFACES entry in audit_scenes.rs, a Captures row in docs/decisions/25w38k498b-ui-polish-audit.md): `cargo run -p fe-workbench --example screenshots -- audit <dir>` exits 0 and writes pdf_notice_dark.png and pdf_notice_light.png (Mac).
- C7 The l10n lint and `cargo clippy --workspace` pass; every string is in a .ftl file; pdf_notice.rs's module doc states "detection only, no rendering, the fallback for the PDF plugin".

## Out of scope

Rendering, text extraction or thumbnails of PDFs; the install prompt; plugins (card "Custom viewers from plugins" and its breakdown); a Reveal in Finder change; any dependency.

## Conflicts

None with running tasks. Ready cards that touch neighbouring files: T1860 (lib.rs, audit dir) and T1834 (lib.rs); disjoint hunks.
