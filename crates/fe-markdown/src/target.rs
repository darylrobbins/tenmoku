//! Where links and images point, as VS Code's Markdown preview opens and
//! loads them.

use std::{
    borrow::Cow,
    path::{Component, Path, PathBuf},
};

use percent_encoding::percent_decode_str;

/// What clicking a link does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkTarget {
    /// A heading of this document: the fragment, percent-decoded. See
    /// [`crate::Document::heading`].
    Anchor(String),
    /// An `http:`, `https:` or `mailto:` URL, for the browser or mail app.
    Web(String),
    /// A file, and the 0-based line a `#L12` fragment names.
    File { path: PathBuf, line: Option<u32> },
    /// A scheme the preview doesn't open, such as `javascript:` or `data:`.
    Unsupported,
}

/// Whether an image is shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImageSource {
    /// A file inside the workspace root or the document's folder.
    Local(PathBuf),
    Blocked(BlockReason),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BlockReason {
    /// A remote image, or any URL but a `file:` one. They are always
    /// blocked; there's no setting to allow them yet.
    Remote,
    /// A local file outside the workspace root and the document's folder.
    Outside,
}

/// Where the link `dest` in the document at `doc` points. Relative paths are
/// resolved against the document's folder, and those starting with `/`
/// against `root`, the workspace folder, or the document's folder when no
/// folder is open.
pub fn link_target(dest: &str, doc: &Path, root: Option<&Path>) -> LinkTarget {
    let dest = dest.trim();
    if let Some(fragment) = dest.strip_prefix('#') {
        return LinkTarget::Anchor(decode(fragment).into_owned());
    }
    match scheme(dest) {
        Some(scheme) if matches!(scheme.as_str(), "http" | "https" | "mailto") => {
            LinkTarget::Web(dest.to_owned())
        }
        Some(scheme) if scheme == "file" => {
            file_link(Local::FileUrl(file_url_path(dest)), dest, doc, root)
        }
        Some(_) => LinkTarget::Unsupported,
        None => file_link(Local::Link(dest), dest, doc, root),
    }
}

fn file_link(path: Local<'_>, dest: &str, doc: &Path, root: Option<&Path>) -> LinkTarget {
    let line = dest
        .split_once('#')
        .and_then(|(_, fragment)| line_fragment(fragment));
    LinkTarget::File {
        path: path.resolve(doc, root),
        line,
    }
}

/// Whether the image `dest` in the document at `doc` is shown. Only a local
/// file inside `root` or the document's folder is, as VS Code's preview
/// limits local resources to those folders; it resolves paths as
/// [`link_target`] does.
pub fn image_source(dest: &str, doc: &Path, root: Option<&Path>) -> ImageSource {
    let dest = dest.trim();
    let path = match scheme(dest) {
        None => Local::Link(dest),
        Some(scheme) if scheme == "file" => Local::FileUrl(file_url_path(dest)),
        Some(_) => return ImageSource::Blocked(BlockReason::Remote),
    };
    let Some(path) = canonical(&path.resolve(doc, root)) else {
        return ImageSource::Blocked(BlockReason::Outside);
    };
    // A folder that can't be resolved allows nothing.
    let folders = [root, doc.parent()]
        .into_iter()
        .flatten()
        .filter_map(canonical);
    let inside = folders.into_iter().any(|folder| path.starts_with(folder));
    if inside {
        ImageSource::Local(path)
    } else {
        ImageSource::Blocked(BlockReason::Outside)
    }
}

/// The URL scheme `dest` starts with, lowercased.
fn scheme(dest: &str) -> Option<String> {
    let (scheme, _) = dest.split_once(':')?;
    let mut chars = scheme.chars();
    let valid = chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    valid.then(|| scheme.to_ascii_lowercase())
}

/// A `file:` URL's path, without its host: `file:///a/b` is `/a/b`.
fn file_url_path(url: &str) -> &str {
    let rest = &url["file:".len()..];
    match rest.strip_prefix("//") {
        Some(authority) => authority.find('/').map_or("", |slash| &authority[slash..]),
        None => rest,
    }
}

/// Where a link's path points: a relative path or `file:` URL, without
/// its query and fragment, percent-decoded.
enum Local<'a> {
    /// A path as written in the link: one starting with `/` is rooted at the
    /// workspace, anything else at the document's folder.
    Link(&'a str),
    /// A `file:` URL's path, which is absolute.
    FileUrl(&'a str),
}

impl Local<'_> {
    fn resolve(&self, doc: &Path, root: Option<&Path>) -> PathBuf {
        let (Local::Link(path) | Local::FileUrl(path)) = *self;
        let path = path.split(['?', '#']).next().unwrap_or_default();
        let path = decode(path);
        let folder = doc.parent().unwrap_or(Path::new(""));
        let resolved = match (self, path.strip_prefix('/')) {
            // An empty path is the document itself, as in `?query` or `#L3`.
            _ if path.is_empty() => doc.to_path_buf(),
            (Local::Link(_), Some(rooted)) => root.unwrap_or(folder).join(rooted),
            _ => folder.join(path.as_ref()),
        };
        normalize(&resolved)
    }
}

/// `path` made absolute, with its symlinks resolved as far as it exists, so
/// a link out of a folder isn't inside it, and lexically normalised past
/// that. An empty path, such as a bare file name's folder, is the current
/// directory. `None` if there's no current directory to resolve against.
fn canonical(path: &Path) -> Option<PathBuf> {
    let path = if path.as_os_str().is_empty() {
        Path::new(".")
    } else {
        path
    };
    let path = normalize(&std::path::absolute(path).ok()?);
    let mut existing = path.as_path();
    loop {
        if let Ok(canonical) = existing.canonicalize() {
            let rest = path.strip_prefix(existing).expect("an ancestor");
            return Some(if rest.as_os_str().is_empty() {
                canonical
            } else {
                canonical.join(rest)
            });
        }
        match existing.parent() {
            Some(parent) => existing = parent,
            None => return Some(path),
        }
    }
}

/// `path` without `.` components, and with each `..` taking off the
/// component before it. A `..` at the root stays there.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match out.components().next_back() {
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                Some(Component::RootDir | Component::Prefix(_)) => {}
                _ => out.push(component),
            },
            component => out.push(component),
        }
    }
    out
}

fn decode(text: &str) -> Cow<'_, str> {
    percent_decode_str(text).decode_utf8_lossy()
}

/// The 0-based line `L12`, `L12C3` or `L12,3` names, ignoring case.
fn line_fragment(fragment: &str) -> Option<u32> {
    let rest = fragment.strip_prefix(['L', 'l'])?;
    let digits = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    let after = &rest[digits..];
    let column = after
        .strip_prefix([',', 'C', 'c'])
        .is_some_and(|column| !column.is_empty() && column.bytes().all(|b| b.is_ascii_digit()));
    if !(after.is_empty() || column) {
        return None;
    }
    let line: u32 = rest[..digits].parse().ok()?;
    Some(line.saturating_sub(1))
}
