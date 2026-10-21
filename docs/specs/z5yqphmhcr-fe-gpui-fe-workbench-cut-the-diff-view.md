---
task_uid: "01m5fzyafwf1kbgsz5yqphmhcr"
task: "T1071"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "diff_view::tests::large_sides_diff_off_the_main_thread (14 s), diff_view::diff_view_compare_tests::editable_side_compares (12 s) and diff_view::tests::too_large_to_compare (10 s) each take under 4 s in a debug build on an idle machine (cargo nextest run -p fe-gpui diff_view), still asserting the over-ASYNC_BYTES and over-MAX_DIFF_BYTES paths (the byte-size asserts stay)"
    required: true
    manual: false
  - id: "C2"
    text: "tests::each_frame_wraps_at_most_one_slice (11 s) takes under 4 s (cargo nextest run -p fe-gpui each_frame_wraps)"
    required: true
    manual: false
  - id: "C3"
    text: "tests::project_panel::only_visible_rows_are_built (22 s) takes under 6 s (cargo nextest run -p fe-workbench project_panel::only_visible_rows) with a fixture of at most 20,000 files"
    required: true
    manual: false
  - id: "C4"
    text: "find_replace_interrupted::replace_on_an_expensive_row_steps_only_as_far_as_it_needs (16 s) takes under 6 s (cargo nextest run -p fe-workbench find_replace_interrupted)"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-gpui and cargo nextest run -p fe-workbench pass"
    required: true
    manual: false
---

# fe-gpui, fe-workbench: cut the diff_view, each_frame_wraps, only_visible_rows and find_replace_interrupted GPUI tests (10 to 22 s)

Test-only speed-up, user-approved 2026-10-21. Mac-only (GPUI TestAppContext). Keep what each proves.

- large_sides_diff_off_the_main_thread / too_large_to_compare / editable_side_compares: long_texts(150_000) and long_texts(800_000) and texts(250_000) are sized just over ASYNC_BYTES (1 MiB), MAX_DIFF_BYTES and 1.5 x ASYNC_BYTES. Size each text by bytes: build the same row pattern but with fewer, longer rows (e.g. rows of 200 chars) so the byte thresholds are still crossed with 5 to 10x fewer rows; the byte-size assertions already guard the choice. Beware the imara Histogram diff is quadratic on many interleaved changes: keep the changes at the same few rows (5..6, 20..) and not more.
- each_frame_wraps_at_most_one_slice: 1,000,000 non-ASCII rows -> 100,000 (about 0.8 MB, still over 5 x WRAP_SLICE_BYTES so frames repeat); keep the frames loop.
- only_visible_rows_are_built: fixture::generate(dir, 1, 100_000) -> 20_000 files; update the 100_002 count and the file_name(99_999) assertion from the constant. The test needs far more rows than fit (fit < 1000), not 100k.
- replace_on_an_expensive_row_steps_only_as_far_as_it_needs: SETUP_LIMIT/SLOW_ROW size the row; shrink SLOW_ROW only if the FEW_CALLS (WANTED_CALLS = 5) versus MANY_A ratio assertion still has a >= 10x gap; otherwise report what dominates (serially(SETUP_LIMIT) waiting) and stop.
Mac-only: fe-gpui and the workbench tests need GPUI/Metal.
