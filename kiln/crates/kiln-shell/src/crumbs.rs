// SPDX-License-Identifier: Apache-2.0

//! A pane's breadcrumbs for its active item: the item's file as path
//! crumbs ([`path_crumbs`], from [`PaneItem::path`](crate::PaneItem::path)),
//! relative to the window's root folder, then the item's own crumbs
//! ([`PaneItem::breadcrumbs`](crate::PaneItem::breadcrumbs)), each drawn
//! as kiln-ui's [`Crumb`] ([`ItemCrumb::into_crumb`]).

use std::path::{Component, Path, PathBuf};

use gpui::SharedString;
use kiln_ui::Crumb;

use crate::ItemCrumb;

/// One path crumb: a directory, or the file last.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathCrumb {
    pub label: SharedString,
    /// The directory it stands for, or the file's, for a click to list.
    pub dir: Option<PathBuf>,
    /// It's the file's, the last.
    pub file: bool,
}

/// `path`'s crumbs: a directory per component under `root`, then the file;
/// outside `root`, its parent directory's name, then the file. A document
/// with no path is `untitled`.
pub fn path_crumbs(path: Option<&Path>, root: Option<&Path>) -> Vec<PathCrumb> {
    let Some(path) = path else {
        return vec![PathCrumb {
            label: "untitled".into(),
            dir: None,
            file: true,
        }];
    };
    let parent = path.parent().map(Path::to_owned);
    let (mut dir, dirs): (PathBuf, Vec<_>) =
        match root.and_then(|root| Some((root, path.strip_prefix(root).ok()?))) {
            Some((root, relative)) => {
                let mut components: Vec<_> = relative.components().collect();
                components.pop();
                (root.to_owned(), components)
            }
            None => match parent.as_deref().and_then(Path::file_name) {
                Some(name) => {
                    let above = parent.as_deref().and_then(Path::parent);
                    let above = above.map_or_else(PathBuf::new, Path::to_owned);
                    (above, vec![Component::Normal(name)])
                }
                None => (PathBuf::new(), Vec::new()),
            },
        };
    let mut crumbs: Vec<_> = dirs
        .into_iter()
        .map(|component| {
            dir.push(component);
            PathCrumb {
                label: component.as_os_str().to_string_lossy().into_owned().into(),
                dir: Some(dir.clone()),
                file: false,
            }
        })
        .collect();
    let file = path.file_name().map_or_else(
        || "untitled".into(),
        |name| name.to_string_lossy().into_owned(),
    );
    crumbs.push(PathCrumb {
        label: file.into(),
        dir: parent,
        file: true,
    });
    crumbs
}

impl ItemCrumb {
    /// The crumb to draw, running its click, if any.
    pub fn into_crumb(self) -> Crumb {
        let mut crumb = Crumb::new(self.label, self.kind);
        if let Some(icon) = self.icon {
            crumb = crumb.icon(icon);
        }
        match self.on_click {
            Some(on_click) => crumb.on_click(move |_, window, cx| on_click(window, cx)),
            None => crumb,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{PathCrumb, path_crumbs};

    fn labels(crumbs: &[PathCrumb]) -> Vec<&str> {
        crumbs.iter().map(|crumb| crumb.label.as_ref()).collect()
    }

    #[test]
    fn under_the_root_a_crumb_per_directory() {
        let root = Path::new("/work/proj");
        let crumbs = path_crumbs(Some(Path::new("/work/proj/src/ui/main.rs")), Some(root));
        assert_eq!(labels(&crumbs), ["src", "ui", "main.rs"]);
        assert_eq!(crumbs[0].dir.as_deref(), Some(Path::new("/work/proj/src")));
        assert_eq!(
            crumbs[1].dir.as_deref(),
            Some(Path::new("/work/proj/src/ui"))
        );
        assert_eq!(
            crumbs[2].dir.as_deref(),
            Some(Path::new("/work/proj/src/ui"))
        );
        assert_eq!(
            crumbs.iter().map(|crumb| crumb.file).collect::<Vec<_>>(),
            [false, false, true]
        );
        let top = path_crumbs(Some(Path::new("/work/proj/README.md")), Some(root));
        assert_eq!(labels(&top), ["README.md"]);
    }

    #[test]
    fn outside_the_root_the_parent_then_the_file() {
        let root = Path::new("/work/proj");
        let crumbs = path_crumbs(Some(Path::new("/etc/hosts")), Some(root));
        assert_eq!(labels(&crumbs), ["etc", "hosts"]);
        assert_eq!(crumbs[0].dir.as_deref(), Some(Path::new("/etc")));
        let no_root = path_crumbs(Some(Path::new("/a/b/c.txt")), None);
        assert_eq!(labels(&no_root), ["b", "c.txt"]);
        assert_eq!(no_root[0].dir.as_deref(), Some(Path::new("/a/b")));
    }

    #[test]
    fn no_path_is_untitled() {
        let crumbs = path_crumbs(None, Some(Path::new("/work")));
        assert_eq!(
            crumbs,
            [PathCrumb {
                label: "untitled".into(),
                dir: None,
                file: true,
            }]
        );
    }
}
