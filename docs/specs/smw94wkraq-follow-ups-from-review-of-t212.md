---
task_uid: "01m6d7ewmnybzf2dsmw94wkraq"
task: "T408"
rev: 3
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T212

Findings of the review of T212 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-vim/src/folds.rs:124: All fold-creation forms dispatch through vim.operate, but EditorView::refuses_vim permits only Yank in read-only views. Consequently zfj, 3zF and visual zf report E21 and create no fold in a read-only diff pane, although they change no text. The search-operator and :normal read-only classifiers also treat Fold as editing.
  fix: Recognize Operator::Fold as non-editing throughout the host's operator classifications, including direct dispatch, search submission and :normal; add read-only-view regression coverage.
  instances: crates/fe-vim/src/folds.rs:122, crates/fe-vim/src/folds.rs:124, crates/fe-vim/src/folds.rs:126

- F2 medium crates/fe-vim/src/folds.rs:392: area.start can be a visual position past a nonempty row's last character. Assigning it directly after visual::exit undoes that function's normal-mode caret normalization. For example, on "a\nlongline\nend" from (1, 5), vkzf creates fold 0..=1 but leaves the normal-mode caret at (0, 1), on the newline instead of the header's last character.
  fix: Normalize area.start through Cursor::at before assigning the normal-mode caret, and add a visual-fold test whose upper endpoint is past a short row's last character.
  instances: crates/fe-vim/src/folds.rs:392
