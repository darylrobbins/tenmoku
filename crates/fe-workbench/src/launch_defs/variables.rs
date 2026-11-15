//! §4's variables, written `${name}` in a configuration's `program`,
//! `args`, `cwd`, `env` values and passthrough strings:
//!
//! - `${workspaceFolder}` and `${workspaceFolderBasename}`;
//! - `${file}`, `${fileDirname}`, `${fileBasename}`,
//!   `${fileBasenameNoExtension}`, `${relativeFile}` and `${lineNumber}`,
//!   from the active editor, and an error when there is none;
//! - `${env:NAME}`, empty when `NAME` isn't set, and `${userHome}`.
//!
//! `${command:…}` and `${input:…}` aren't supported, nor is any other name:
//! a configuration using one is listed but can't start ([`problems`]). A
//! `${` with no `}` after it is just text.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use kiln_l10n::tr;

/// The names the active editor's file gives.
const FILE_VARIABLES: [&str; 6] = [
    "file",
    "fileDirname",
    "fileBasename",
    "fileBasenameNoExtension",
    "relativeFile",
    "lineNumber",
];

/// What the variables stand for when a configuration starts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Values {
    pub workspace_folder: PathBuf,
    pub active: Option<ActiveFile>,
    pub user_home: Option<PathBuf>,
    /// The environment `${env:…}` reads.
    pub env: BTreeMap<String, String>,
}

/// The active editor's file and where its caret is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveFile {
    pub path: PathBuf,
    /// 1-based.
    pub line: usize,
}

/// Each `${…}` in `text`: what's between the braces, and the byte range of
/// the whole reference.
fn references(text: &str) -> Vec<(&str, std::ops::Range<usize>)> {
    let mut found = Vec::new();
    let mut at = 0;
    while let Some(start) = text[at..].find("${").map(|start| at + start) {
        let Some(end) = text[start + 2..].find('}').map(|end| start + 2 + end) else {
            break;
        };
        found.push((&text[start + 2..end], start..end + 1));
        at = end + 1;
    }
    found
}

/// Whether `name` is one this module can expand.
fn supported(name: &str) -> bool {
    matches!(
        name,
        "workspaceFolder" | "workspaceFolderBasename" | "userHome"
    ) || FILE_VARIABLES.contains(&name)
        || name.starts_with("env:")
}

/// Why `text`'s variables stop a configuration starting, in words: one for
/// each `${command:…}`, `${input:…}` or unknown name, in order.
pub fn problems(text: &str) -> Vec<String> {
    references(text)
        .into_iter()
        .filter(|(name, _)| !supported(name))
        .map(|(name, range)| {
            let variable = &text[range];
            if name.starts_with("command:") || name.starts_with("input:") {
                tr!(
                    "workbench-debug-launch-variable-unsupported",
                    variable = variable
                )
            } else {
                tr!(
                    "workbench-debug-launch-variable-unknown",
                    variable = variable
                )
            }
            .into()
        })
        .collect()
}

/// `text` with its variables replaced by `values`.
///
/// # Errors
///
/// In words: a variable [`problems`] reports, one of the active file's with
/// no active editor, or `${userHome}` with no home.
pub fn expand(text: &str, values: &Values) -> Result<String, String> {
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for (name, range) in references(text) {
        out.push_str(&text[at..range.start]);
        let variable = &text[range.clone()];
        out.push_str(&value(name, variable, values)?);
        at = range.end;
    }
    out.push_str(&text[at..]);
    Ok(out)
}

fn value(name: &str, variable: &str, values: &Values) -> Result<String, String> {
    if !supported(name) {
        return Err(problems(variable).concat());
    }
    if let Some(env) = name.strip_prefix("env:") {
        return Ok(values.env.get(env).cloned().unwrap_or_default());
    }
    let text = |path: &Path| path.to_string_lossy().into_owned();
    let folder = &values.workspace_folder;
    match name {
        "workspaceFolder" => return Ok(text(folder)),
        "workspaceFolderBasename" => {
            return Ok(folder
                .file_name()
                .map(|name| text(Path::new(name)))
                .unwrap_or_default());
        }
        "userHome" => {
            return values.user_home.as_deref().map(text).ok_or_else(|| {
                tr!(
                    "workbench-debug-launch-variable-no-home",
                    variable = variable
                )
                .into()
            });
        }
        _ => {}
    }
    let active = values.active.as_ref().ok_or_else(|| -> String {
        tr!(
            "workbench-debug-launch-variable-no-editor",
            variable = variable
        )
        .into()
    })?;
    let file = &active.path;
    let part =
        |part: Option<&std::ffi::OsStr>| part.map(|part| text(Path::new(part))).unwrap_or_default();
    Ok(match name {
        "file" => text(file),
        "fileDirname" => file.parent().map(text).unwrap_or_default(),
        "fileBasename" => part(file.file_name()),
        "fileBasenameNoExtension" => part(file.file_stem()),
        "relativeFile" => text(file.strip_prefix(folder).unwrap_or(file)),
        _ => active.line.to_string(),
    })
}
