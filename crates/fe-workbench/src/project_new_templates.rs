//! The typed files the project panel's New menu offers
//! ([`super::new_menu`]): each one's extension and the tiny body a new file
//! of it starts with, and the project markers at the root that put a
//! language's entries first.
//!
//! The table is data: [`TEMPLATES`] for the types, [`MARKERS`] for what
//! detects them, [`FALLBACK`] for those always offered after Markdown.

use gpui::SharedString;
use kiln_l10n::{tr, verbatim};

/// A type of file the New menu makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum FileType {
    Markdown,
    Rust,
    TypeScript,
    JavaScript,
    Python,
    Go,
    Java,
    Html,
    Css,
    Json,
    Shell,
}

use FileType::*;

/// A [`FileType`]'s new file: its extension, without the dot, and what it
/// starts with.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Template {
    pub(crate) file_type: FileType,
    pub(crate) extension: &'static str,
    pub(crate) body: &'static str,
}

/// Every type's template.
pub(crate) const TEMPLATES: &[Template] = &[
    Template {
        file_type: Markdown,
        extension: "md",
        body: "",
    },
    Template {
        file_type: Rust,
        extension: "rs",
        body: "",
    },
    Template {
        file_type: TypeScript,
        extension: "ts",
        body: "",
    },
    Template {
        file_type: JavaScript,
        extension: "js",
        body: "",
    },
    Template {
        file_type: Python,
        extension: "py",
        body: "",
    },
    Template {
        file_type: Go,
        extension: "go",
        body: verbatim!("package main\n"),
    },
    Template {
        file_type: Java,
        extension: "java",
        body: "",
    },
    Template {
        file_type: Html,
        extension: "html",
        body: verbatim!(
            "<!doctype html>\n\
               <html>\n\
               <head>\n  \
               <meta charset=\"utf-8\">\n  \
               <title></title>\n\
               </head>\n\
               <body>\n\
               </body>\n\
               </html>\n"
        ),
    },
    Template {
        file_type: Css,
        extension: "css",
        body: "",
    },
    Template {
        file_type: Json,
        extension: "json",
        body: "{}\n",
    },
    Template {
        file_type: Shell,
        extension: "sh",
        body: verbatim!("#!/bin/sh\n"),
    },
];

/// A file at the project's root and the types it puts first, in this
/// order.
pub(crate) const MARKERS: &[(&str, &[FileType])] = &[
    (verbatim!("Cargo.toml"), &[Rust]),
    ("package.json", &[TypeScript, JavaScript]),
    ("tsconfig.json", &[TypeScript, JavaScript]),
    ("pyproject.toml", &[Python]),
    ("requirements.txt", &[Python]),
    ("go.mod", &[Go]),
    ("pom.xml", &[Java]),
    ("build.gradle", &[Java]),
    ("build.gradle.kts", &[Java]),
];

/// The most types [`MARKERS`] put first.
pub(crate) const MAX_DETECTED: usize = 6;

/// The types offered after Markdown whatever the project is.
pub(crate) const FALLBACK: &[FileType] = &[Html, Css, Json, Shell];

impl FileType {
    /// Its template.
    pub(crate) fn template(self) -> &'static Template {
        (TEMPLATES.iter())
            .find(|template| template.file_type == self)
            .expect("every type has a template")
    }

    /// What the New menu calls it, in the active locale.
    pub(crate) fn title(self) -> SharedString {
        match self {
            Markdown => tr!("workbench-project-new-markdown"),
            Rust => tr!("workbench-project-new-rust"),
            TypeScript => tr!("workbench-project-new-typescript"),
            JavaScript => tr!("workbench-project-new-javascript"),
            Python => tr!("workbench-project-new-python"),
            Go => tr!("workbench-project-new-go"),
            Java => tr!("workbench-project-new-java"),
            Html => tr!("workbench-project-new-html"),
            Css => tr!("workbench-project-new-css"),
            Json => tr!("workbench-project-new-json"),
            Shell => tr!("workbench-project-new-shell"),
        }
    }
}

/// The typed entries of the New menu, for a root whose entries `has_entry`
/// answers by name: the types its markers detect (at most
/// [`MAX_DETECTED`], each once, in [`MARKERS`]' order), then Markdown, then
/// [`FALLBACK`].
pub(crate) fn typed_entries(has_entry: impl Fn(&str) -> bool) -> Vec<FileType> {
    let mut detected: Vec<FileType> = Vec::new();
    for (marker, types) in MARKERS {
        if has_entry(marker) {
            for &file_type in *types {
                if !detected.contains(&file_type) && detected.len() < MAX_DETECTED {
                    detected.push(file_type);
                }
            }
        }
    }
    for file_type in std::iter::once(Markdown).chain(FALLBACK.iter().copied()) {
        if !detected.contains(&file_type) {
            detected.push(file_type);
        }
    }
    detected
}

/// Whether `name` ends in `template`'s extension, in any case: a new file
/// gets its body only then, so a name typed with another extension starts
/// empty.
pub(crate) fn names_its_type(name: &str, template: &Template) -> bool {
    std::path::Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(template.extension))
}
