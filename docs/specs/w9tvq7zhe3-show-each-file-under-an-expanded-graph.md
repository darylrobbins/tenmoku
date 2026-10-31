---
task_uid: "01m67vc6dj7k50n7w9tvq7zhe3"
task: "T258"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-workbench test git_log_file_rows_tests::icon_name_dir: a file row's cells are the file_icon for its name (through the icon-set seam), the name, then the dimmed directory"
    required: true
    manual: false
  - id: "C2"
    text: "git_log_file_rows_tests::letter_right: the M/A/D/R letter is the last cell, flex-none and right-aligned, coloured modified amber, added green, deleted red, renamed blue (theme tokens, not literals); the old left-hand letter is gone"
    required: true
    manual: false
  - id: "C3"
    text: "git_log_file_rows_tests::counts: with commit_stats landed, '+3 -1' shows muted in green/red before the letter; a binary file shows none; counts load on the background executor and the row shows no counts until they arrive"
    required: true
    manual: false
  - id: "C4"
    text: "git_log_file_rows_tests::narrow: a long name ellipsizes before the directory, the directory before the letter, and the letter is never clipped"
    required: true
    manual: false
  - id: "C5"
    text: "git_log_rail_tests, git_log_rail_painted_tests, git_log_open_tests and git_log_tests pass, updated only for the moved letter; cargo test -p fe-workbench passes"
    required: true
    manual: false
---

# Show each file under an expanded Graph commit with its file-type icon, name, dimmed directory and a coloured status letter at the right edge with its +/- counts (Mac T1220)

## Problem
The user: "File icons, M/A/D on the right side". Today `file_cell` (git_log.rs ~:1156) renders `<letter> name dir` with the letter first and no icon. The rail through expanded rows is already drawn (rail tests), keep it unbroken.
