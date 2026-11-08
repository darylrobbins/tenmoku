// SPDX-License-Identifier: Apache-2.0

//! [`TintedIcons`]: Lucide's line icons, each drawn in the theme's colour
//! for its file type or folder role, the `file_icon` token group.
//!
//! A file's icon is [`LucideIcons`](crate::LucideIcons)' (a README's is the
//! Markdown icon); its tint is data, in the shape of Lucide's tables:
//! [`FILE_NAMES`] and [`EXTENSIONS`] map a name to a token, a file neither
//! names takes `file_icon.default`. [`FOLDER_NAMES`] maps a folder's name
//! to its role's token, any other folder `file_icon.folder`. Every
//! `file_icon` token is `list.icon` unless a theme sets it, so under a
//! theme that sets none the set draws as Lucide's does, in one colour.

use std::{collections::HashMap, sync::LazyLock};

use crate::{
    Icon, IconRef, IconSet,
    file_icons::{MAX_KEY, by_name, lookup, lucide_file, lucide_folder},
};

/// Rust sources.
pub(crate) const RUST: &str = "file_icon.rust";
/// TOML files.
pub(crate) const TOML: &str = "file_icon.toml";
/// Markdown and READMEs.
pub(crate) const MARKDOWN: &str = "file_icon.markdown";
/// Other configuration.
pub(crate) const CONFIG: &str = "file_icon.config";
/// Data, tables and lock files.
pub(crate) const DATA: &str = "file_icon.data";
/// Shell scripts and scripting languages.
pub(crate) const SCRIPT: &str = "file_icon.script";
/// The web's languages.
pub(crate) const WEB: &str = "file_icon.web";
/// Images, video and sound.
pub(crate) const IMAGE: &str = "file_icon.image";
/// Any other file.
pub(crate) const DEFAULT: &str = "file_icon.default";
/// Source folders.
pub(crate) const FOLDER_SOURCE: &str = "file_icon.folder_source";
/// Tests folders.
pub(crate) const FOLDER_TESTS: &str = "file_icon.folder_tests";
/// Documentation folders.
pub(crate) const FOLDER_DOCS: &str = "file_icon.folder_docs";
/// Any other folder.
pub(crate) const FOLDER: &str = "file_icon.folder";

/// Exact file names, ASCII lowercase, whose icon isn't Lucide's.
pub(crate) const ICON_NAMES: &[(&str, Icon)] = &[("readme", Icon::FileText)];

/// Exact file names, ASCII lowercase, to tints. Checked before any
/// extension.
pub(crate) const FILE_NAMES: &[(&str, &str)] = &[
    ("readme", MARKDOWN),
    ("readme.txt", MARKDOWN),
    ("readme.rst", MARKDOWN),
    ("readme.adoc", MARKDOWN),
    ("readme.org", MARKDOWN),
    (".editorconfig", CONFIG),
    ("makefile", CONFIG),
    ("justfile", CONFIG),
    ("cmakelists.txt", CONFIG),
    ("gemfile", CONFIG),
    ("go.mod", CONFIG),
    ("pom.xml", CONFIG),
    ("build.gradle", CONFIG),
    ("build.gradle.kts", CONFIG),
    ("dockerfile", CONFIG),
    ("containerfile", CONFIG),
    (".dockerignore", CONFIG),
    (".gitignore", CONFIG),
    (".gitattributes", CONFIG),
    (".gitmodules", CONFIG),
    (".git-blame-ignore-revs", CONFIG),
    ("pnpm-lock.yaml", DATA),
    ("go.sum", DATA),
];

/// Extensions without the dot, ASCII lowercase, to tints. A two-part key
/// is tried before the last extension alone.
pub(crate) const EXTENSIONS: &[(&str, &str)] = &[
    ("rs", RUST),
    ("toml", TOML),
    ("md", MARKDOWN),
    ("markdown", MARKDOWN),
    ("mdx", MARKDOWN),
    ("yaml", CONFIG),
    ("yml", CONFIG),
    ("ini", CONFIG),
    ("cfg", CONFIG),
    ("conf", CONFIG),
    ("properties", CONFIG),
    ("dockerfile", CONFIG),
    ("json", DATA),
    ("jsonc", DATA),
    ("json5", DATA),
    ("xml", DATA),
    ("csv", DATA),
    ("tsv", DATA),
    ("xlsx", DATA),
    ("xls", DATA),
    ("ods", DATA),
    ("sql", DATA),
    ("sqlite", DATA),
    ("db", DATA),
    ("lock", DATA),
    ("sh", SCRIPT),
    ("bash", SCRIPT),
    ("zsh", SCRIPT),
    ("fish", SCRIPT),
    ("ps1", SCRIPT),
    ("bat", SCRIPT),
    ("cmd", SCRIPT),
    ("py", SCRIPT),
    ("rb", SCRIPT),
    ("lua", SCRIPT),
    ("php", SCRIPT),
    ("pl", SCRIPT),
    ("js", WEB),
    ("mjs", WEB),
    ("cjs", WEB),
    ("jsx", WEB),
    ("ts", WEB),
    ("tsx", WEB),
    ("html", WEB),
    ("htm", WEB),
    ("css", WEB),
    ("scss", WEB),
    ("vue", WEB),
    ("svelte", WEB),
    ("png", IMAGE),
    ("jpg", IMAGE),
    ("jpeg", IMAGE),
    ("gif", IMAGE),
    ("webp", IMAGE),
    ("bmp", IMAGE),
    ("ico", IMAGE),
    ("svg", IMAGE),
    ("tiff", IMAGE),
    ("avif", IMAGE),
    ("mp4", IMAGE),
    ("mov", IMAGE),
    ("webm", IMAGE),
    ("mkv", IMAGE),
    ("avi", IMAGE),
    ("mp3", IMAGE),
    ("wav", IMAGE),
    ("flac", IMAGE),
    ("ogg", IMAGE),
    ("m4a", IMAGE),
];

/// Folder names, ASCII lowercase, to their role's tint.
pub(crate) const FOLDER_NAMES: &[(&str, &str)] = &[
    ("src", FOLDER_SOURCE),
    ("source", FOLDER_SOURCE),
    ("sources", FOLDER_SOURCE),
    ("lib", FOLDER_SOURCE),
    ("crates", FOLDER_SOURCE),
    ("packages", FOLDER_SOURCE),
    ("app", FOLDER_SOURCE),
    ("tests", FOLDER_TESTS),
    ("test", FOLDER_TESTS),
    ("__tests__", FOLDER_TESTS),
    ("spec", FOLDER_TESTS),
    ("specs", FOLDER_TESTS),
    ("e2e", FOLDER_TESTS),
    ("benches", FOLDER_TESTS),
    ("docs", FOLDER_DOCS),
    ("doc", FOLDER_DOCS),
    ("documentation", FOLDER_DOCS),
];

static ICON_NAME_MAP: LazyLock<HashMap<&'static str, Icon>> =
    LazyLock::new(|| ICON_NAMES.iter().copied().collect());
static NAME_MAP: LazyLock<HashMap<&'static str, &'static str>> =
    LazyLock::new(|| FILE_NAMES.iter().copied().collect());
static EXTENSION_MAP: LazyLock<HashMap<&'static str, &'static str>> =
    LazyLock::new(|| EXTENSIONS.iter().copied().collect());
static FOLDER_MAP: LazyLock<HashMap<&'static str, &'static str>> =
    LazyLock::new(|| FOLDER_NAMES.iter().copied().collect());

/// Lucide's line icons in the theme's `file_icon` colours: Rust, TOML,
/// Markdown, configuration, data, scripts, the web and media each in their
/// own, folders by role (source, tests, docs).
///
/// Not installed by default: install it with
/// [`set_icon_set`](crate::set_icon_set).
#[derive(Clone, Copy, Debug, Default)]
pub struct TintedIcons;

impl TintedIcons {
    /// The token whose colour a file called `name` is drawn in.
    pub fn file_tint(name: &str) -> &'static str {
        by_name(&NAME_MAP, &EXTENSION_MAP, CONFIG, DEFAULT, name)
    }

    /// The token whose colour a folder called `name` is drawn in.
    pub fn folder_tint(name: &str) -> &'static str {
        lookup(&FOLDER_MAP, name, &mut [0; MAX_KEY]).unwrap_or(FOLDER)
    }
}

impl IconSet for TintedIcons {
    fn file(&self, name: &str) -> IconRef {
        let icon =
            lookup(&ICON_NAME_MAP, name, &mut [0; MAX_KEY]).unwrap_or_else(|| lucide_file(name));
        IconRef::Tinted(icon, Self::file_tint(name))
    }

    fn folder(&self, expanded: bool) -> IconRef {
        IconRef::Tinted(lucide_folder(expanded), FOLDER)
    }

    fn folder_named(&self, name: &str, expanded: bool) -> IconRef {
        IconRef::Tinted(lucide_folder(expanded), Self::folder_tint(name))
    }
}
