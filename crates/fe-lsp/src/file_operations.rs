//! LSP file operations: `workspace/didCreateFiles`, `didRenameFiles` and
//! `didDeleteFiles` to the servers whose `workspace.fileOperations`
//! filters match, and `workspace/willRenameFiles` for the edits a rename
//! needs (imports, `mod` paths).
//!
//! - **Filters.** A path matches a filter if its scheme is `file` (the
//!   only one fe-lsp has), its glob matches (case-folded under
//!   `ignoreCase`) and its kind is the filter's `matches` (unset: both).
//!   The kind is the caller's to read from the filesystem.
//! - **Notifications.** Each server gets one notification with the paths
//!   that match its filters, a rename matching by its old path; a server
//!   with none matching, or no filters for the operation, gets nothing.
//! - **willRename.** Each matching server is asked at once; its edit is
//!   decoded as a `workspace/applyEdit`'s is. A server's error, null
//!   answer or timeout gives nothing for it: the rename goes ahead.

use std::path::{Path, PathBuf};

use futures::FutureExt;
use futures::future::{BoxFuture, join_all};
use lsp_types::request::WillRenameFiles;
use lsp_types::{
    CreateFilesParams, DeleteFilesParams, FileCreate, FileDelete, FileOperationFilter,
    FileOperationPatternKind, FileOperationRegistrationOptions, FileRename, RenameFilesParams,
    ServerCapabilities, Url,
};
use serde_json::{Value, json};

use crate::glob::LspGlob;
use crate::manager::LspManager;
use crate::workspace_edit::FileEdits;

/// Whether `path` (a folder if `folder`) matches any of `options`' filters.
fn matches(options: &FileOperationRegistrationOptions, path: &Path, folder: bool) -> bool {
    options
        .filters
        .iter()
        .any(|filter| filter_matches(filter, path, folder))
}

fn filter_matches(filter: &FileOperationFilter, path: &Path, folder: bool) -> bool {
    if filter
        .scheme
        .as_deref()
        .is_some_and(|scheme| scheme != "file")
    {
        return false;
    }
    let pattern = &filter.pattern;
    match pattern.matches {
        Some(FileOperationPatternKind::File) if folder => return false,
        Some(FileOperationPatternKind::Folder) if !folder => return false,
        _ => {}
    }
    let ignore_case = pattern
        .options
        .as_ref()
        .and_then(|options| options.ignore_case)
        == Some(true);
    let glob = if ignore_case {
        pattern.glob.to_lowercase()
    } else {
        pattern.glob.clone()
    };
    let Ok(glob) = LspGlob::new(&lsp_types::GlobPattern::String(glob)) else {
        return false;
    };
    if ignore_case {
        glob.matches(Path::new(&path.to_string_lossy().to_lowercase()))
    } else {
        glob.matches(path)
    }
}

fn uri(path: &Path) -> Option<String> {
    Url::from_file_path(path).ok().map(String::from)
}

/// The options a server registers for an operation.
type Registered = fn(&ServerCapabilities) -> Option<&FileOperationRegistrationOptions>;

fn did_create(capabilities: &ServerCapabilities) -> Option<&FileOperationRegistrationOptions> {
    operations(capabilities)?.did_create.as_ref()
}

fn did_rename(capabilities: &ServerCapabilities) -> Option<&FileOperationRegistrationOptions> {
    operations(capabilities)?.did_rename.as_ref()
}

fn did_delete(capabilities: &ServerCapabilities) -> Option<&FileOperationRegistrationOptions> {
    operations(capabilities)?.did_delete.as_ref()
}

fn will_rename(capabilities: &ServerCapabilities) -> Option<&FileOperationRegistrationOptions> {
    operations(capabilities)?.will_rename.as_ref()
}

fn operations(
    capabilities: &ServerCapabilities,
) -> Option<&lsp_types::WorkspaceFileOperationsServerCapabilities> {
    capabilities.workspace.as_ref()?.file_operations.as_ref()
}

/// The moves `registered` matches by their old paths, as `(old, new)` URIs.
fn matching_moves(
    capabilities: &ServerCapabilities,
    registered: Registered,
    moves: &[(PathBuf, PathBuf, bool)],
) -> Vec<FileRename> {
    let Some(options) = registered(capabilities) else {
        return Vec::new();
    };
    moves
        .iter()
        .filter(|(old, _, folder)| matches(options, old, *folder))
        .filter_map(|(old, new, _)| {
            Some(FileRename {
                old_uri: uri(old)?,
                new_uri: uri(new)?,
            })
        })
        .collect()
}

/// The paths `registered` matches, as URIs.
fn matching_paths(
    capabilities: &ServerCapabilities,
    registered: Registered,
    paths: &[(PathBuf, bool)],
) -> Vec<String> {
    let Some(options) = registered(capabilities) else {
        return Vec::new();
    };
    paths
        .iter()
        .filter(|(path, folder)| matches(options, path, *folder))
        .filter_map(|(path, _)| uri(path))
        .collect()
}

impl LspManager {
    /// Tells each server whose `didCreate` filters match some of `paths`
    /// (each with whether it's a folder) that they were created.
    pub fn did_create_files(&self, paths: Vec<(PathBuf, bool)>) {
        self.notify_sessions("workspace/didCreateFiles", |_, capabilities| {
            let files = matching_paths(capabilities, did_create, &paths);
            let files: Vec<_> = files.into_iter().map(|uri| FileCreate { uri }).collect();
            (!files.is_empty()).then(|| json!(CreateFilesParams { files }))
        });
    }

    /// Tells each server whose `didDelete` filters match some of `paths`
    /// that they were deleted.
    pub fn did_delete_files(&self, paths: Vec<(PathBuf, bool)>) {
        self.notify_sessions("workspace/didDeleteFiles", |_, capabilities| {
            let files = matching_paths(capabilities, did_delete, &paths);
            let files: Vec<_> = files.into_iter().map(|uri| FileDelete { uri }).collect();
            (!files.is_empty()).then(|| json!(DeleteFilesParams { files }))
        });
    }

    /// Tells each server whose `didRename` filters match the old path of
    /// some of `moves` (old, new, whether a folder) that they were renamed.
    pub fn did_rename_files(&self, moves: Vec<(PathBuf, PathBuf, bool)>) {
        self.notify_sessions("workspace/didRenameFiles", |_, capabilities| {
            let files = matching_moves(capabilities, did_rename, &moves);
            (!files.is_empty()).then(|| -> Value { json!(RenameFilesParams { files }) })
        });
    }

    /// Asks each server whose `willRename` filters match some of `moves`,
    /// at once, what edits the rename needs, decoded as a
    /// `workspace/applyEdit`'s are, in server order. A server's error,
    /// null answer, refused edit or timeout (`lsp.timeouts.references`)
    /// gives nothing for it. Dropping the future cancels the requests.
    pub fn will_rename_files(
        &self,
        moves: Vec<(PathBuf, PathBuf, bool)>,
    ) -> impl Future<Output = Vec<FileEdits>> + Send + 'static + use<> {
        let mut asked: Vec<BoxFuture<'static, Vec<FileEdits>>> = Vec::new();
        for id in self.server_ids() {
            let Some(capabilities) = self.capabilities(id) else {
                continue;
            };
            let files = matching_moves(&capabilities, will_rename, &moves);
            if files.is_empty() {
                continue;
            }
            let reply =
                self.request_with_documents::<WillRenameFiles>(id, RenameFilesParams { files });
            asked.push(
                reply
                    .map(|answer| {
                        let (reply, opened) = answer.ok()?;
                        opened.decode_workspace_edit(reply.result?).ok()
                    })
                    .map(Option::unwrap_or_default)
                    .boxed(),
            );
        }
        join_all(asked).map(|edits| edits.into_iter().flatten().collect())
    }
}
