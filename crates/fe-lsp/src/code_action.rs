//! `textDocument/codeAction` and `codeAction/resolve`: quick fixes and
//! refactors at a range, whose edits are workspace edits
//! ([`crate::workspace_edit`]) and whose commands the server may carry out
//! through `workspace/applyEdit`. Filled by the code actions card.
