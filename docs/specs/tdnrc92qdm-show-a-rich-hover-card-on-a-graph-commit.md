---
task_uid: "01m67vc6j845662ytdnrc92qdm"
task: "T259"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-workbench test git_log_card_tests::contents: the card shows an initials circle (first letters of the author's first and last name, a stable colour from the email, no network request), author, '\u003cago\u003e (\u003cabsolute date\u003e)', the full message, and 'N files changed, X insertions(+), Y deletions(-)' with X green and Y red, singular 'file' for 1"
    required: true
    manual: false
  - id: "C2"
    text: "git_log_card_tests::trailers_and_refs: Co-Authored-By and Signed-off-by lines show as rows with the co-author named; the commit's ref pills and its short sha show in the card"
    required: true
    manual: false
  - id: "C3"
    text: "git_log_card_tests::copy: clicking the copy button puts the full id on the clipboard and the icon flips to a check for a moment"
    required: true
    manual: false
  - id: "C4"
    text: "git_log_card_tests::web_link: with an origin on github.com an 'Open on GitHub' link shows and opens commit_web_url; with another host or no remote no link shows (provider seam from T1215)"
    required: true
    manual: false
  - id: "C5"
    text: "git_log_card_tests::rows_lose_metadata: a commit row shows the graph, ref pills and summary only; the muted age and author text is gone (rows' texts() has one cell). git_log_tests.rs and git_log_message_tests.rs are updated for it: every assertion on the muted '\u003cauthor\u003e, \u003cago\u003e' text, incl. a_commit_rows_muted_text_is_its_age, is deleted or replaced with this card's test"
    required: true
    manual: false
  - id: "C6"
    text: "cargo test -p fe-workbench passes; Gravatar or any fetch is NOT added; clippy -D warnings clean"
    required: true
    manual: false
---

# Show a rich hover card on a Graph commit with an initials avatar, author and dates, full message, trailers, files-changed totals, ref pills, a copy-sha button and a web link where the remote has one, and drop the trailing age and author text from the rows (Mac T1221)

## Problem
The user: "Nice attractive hover for the details", then "when we improve the hover, can we also stop showing the metadata in the graph view?" and "initials, gravatar is later and requires explicit permission". The T809 popover (CommitPopover, git_log.rs) is plain text. commit_rows::texts and the row's trailing `when` div carry the age and author today; the card now carries them, so the rows drop them.

## Implementation notes
- `CommitCard` (git_log.rs) replaces `CommitPopover`. Its totals (`Repo::commit_stats`) and web page (`Repo::commit_web_url`) are read on the background executor each time it is shown, so nothing blocks the hover and a changed remote is picked up. The avatar is drawn locally: initials, and a hue hashed from the lowercased email with FNV-1a. Nothing is fetched.
- Trailers: only `Co-authored-by` and `Signed-off-by` lines (any case) in the body's last paragraph are taken out of the message and shown as rows. The expanded commit's body rows still list them.
- `commit_rows::texts` keeps both cells for the History panel. The Graph draws the first cell alone, and `GitLogPanel::rows()` reports that one cell.
- T258 F2: commit, body and note rows now have the same cell structure as file rows: a clipped lead, then the letter's width (`LETTER_WIDTH`) left empty. So lanes clip at the same x in every row. This is covered by `git_log_tests::rails_clip_alike_when_narrow`.
