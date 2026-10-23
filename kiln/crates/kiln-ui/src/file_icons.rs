// SPDX-License-Identifier: LicenseRef-Proprietary

//! The bundled icon set and the file-name → icon mapping.
//!
//! Icons are Lucide's, unmodified (see `assets/icons/LICENSE` and
//! THIRD_PARTY.md). They're monochrome strokes, so GPUI's `svg()` draws them as
//! an alpha mask tinted with the element's text colour.
//!
//! The mapping is data: [`FILE_NAMES`] and [`EXTENSIONS`], built once into hash
//! maps. A themeable mapping would load over these defaults.

use std::{collections::HashMap, sync::LazyLock};

macro_rules! icons {
    ($($variant:ident => $name:literal,)*) => {
        /// An icon bundled with kiln-ui; draw it with `svg().path(icon.path())`
        /// once the app's asset source serves [`crate::Assets`].
        #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
        pub enum Icon {
            $($variant,)*
        }

        impl Icon {
            /// Every bundled icon.
            pub const ALL: &[Icon] = &[$(Icon::$variant,)*];

            /// Lucide's name for the icon, e.g. `"file-code"`.
            pub fn name(self) -> &'static str {
                match self {
                    $(Icon::$variant => $name,)*
                }
            }

            /// The asset path, e.g. `"kiln-ui/icons/file-code.svg"`.
            pub fn path(self) -> &'static str {
                match self {
                    $(Icon::$variant => concat!("kiln-ui/icons/", $name, ".svg"),)*
                }
            }

            /// The SVG file, compiled in.
            pub(crate) fn svg(self) -> &'static [u8] {
                match self {
                    $(Icon::$variant => include_bytes!(concat!("../assets/icons/", $name, ".svg")),)*
                }
            }
        }
    };
}

icons! {
    ChevronRight => "chevron-right",
    Folder => "folder",
    FolderOpen => "folder-open",
    File => "file",
    FileCode => "file-code",
    FileBraces => "file-braces",
    FileText => "file-text",
    FileCog => "file-cog",
    Package => "package",
    FileLock => "file-lock",
    GitBranch => "git-branch",
    GitGraph => "git-graph",
    Container => "container",
    FileTerminal => "file-terminal",
    FileKey => "file-key",
    FileImage => "file-image",
    FileArchive => "file-archive",
    FileSpreadsheet => "file-spreadsheet",
    Database => "database",
    FileDiff => "file-diff",
    FileVideoCamera => "file-video-camera",
    FileMusic => "file-music",
    Scale => "scale",
    FileSymlink => "file-symlink",
    // The activity rail's; not mapped from names.
    Files => "files",
    Search => "search",
    ListTree => "list-tree",
    // The pane toolbar's; not mapped from names.
    ArrowLeft => "arrow-left",
    ArrowRight => "arrow-right",
    Columns2 => "columns-2",
    Rows2 => "rows-2",
    Ellipsis => "ellipsis",
    // The tab bar's Markdown preview buttons; not mapped from names.
    Eye => "eye",
    Code => "code",
    PanelRightOpen => "panel-right-open",
    // The window buttons drawn under client-side decorations; not mapped
    // from names.
    Minus => "minus",
    Square => "square",
    Copy => "copy",
    X => "x",
    // The checkbox's mark.
    Check => "check",
    // The status bar's; not mapped from names.
    CircleX => "circle-x",
    TriangleAlert => "triangle-alert",
    Server => "server",
    Lock => "lock",
    // The outline's symbol kinds; not mapped from names.
    Box => "box",
    Component => "component",
    Braces => "braces",
    List => "list",
    Plug => "plug",
    Link => "link",
    Type => "type",
    SquareFunction => "square-function",
    Hash => "hash",
    Asterisk => "asterisk",
    Table => "table",
    Heading => "heading",
    CircleDot => "circle-dot",
    Diamond => "diamond",
    Variable => "variable",
    // The AI chat panel's; not mapped from names.
    MessageSquare => "message-square",
    // The search view's results toolbar and file groups; not mapped from
    // names.
    ChevronDown => "chevron-down",
    RefreshCw => "refresh-cw",
    ListX => "list-x",
    FoldVertical => "fold-vertical",
    UnfoldVertical => "unfold-vertical",
}

/// Exact file names, ASCII lowercase. Checked before any extension.
pub(crate) const FILE_NAMES: &[(&str, Icon)] = &[
    (".editorconfig", Icon::FileCog),
    ("makefile", Icon::FileCog),
    ("justfile", Icon::FileCog),
    ("cmakelists.txt", Icon::FileCog),
    ("cargo.toml", Icon::Package),
    ("package.json", Icon::Package),
    ("pyproject.toml", Icon::Package),
    ("go.mod", Icon::Package),
    ("gemfile", Icon::Package),
    ("pom.xml", Icon::Package),
    ("build.gradle", Icon::Package),
    ("build.gradle.kts", Icon::Package),
    ("package-lock.json", Icon::FileLock),
    ("pnpm-lock.yaml", Icon::FileLock),
    ("go.sum", Icon::FileLock),
    (".gitignore", Icon::GitBranch),
    (".gitattributes", Icon::GitBranch),
    (".gitmodules", Icon::GitBranch),
    (".git-blame-ignore-revs", Icon::GitBranch),
    ("dockerfile", Icon::Container),
    ("containerfile", Icon::Container),
    (".dockerignore", Icon::Container),
    ("license", Icon::Scale),
    ("license-mit", Icon::Scale),
    ("license-apache", Icon::Scale),
    ("licence", Icon::Scale),
    ("copying", Icon::Scale),
    ("notice", Icon::Scale),
];

/// Extensions without the dot, ASCII lowercase. A two-part key (`tar.gz`) is
/// tried before the last extension alone.
pub(crate) const EXTENSIONS: &[(&str, Icon)] = &[
    ("rs", Icon::FileCode),
    ("c", Icon::FileCode),
    ("h", Icon::FileCode),
    ("cc", Icon::FileCode),
    ("cpp", Icon::FileCode),
    ("hpp", Icon::FileCode),
    ("go", Icon::FileCode),
    ("py", Icon::FileCode),
    ("rb", Icon::FileCode),
    ("java", Icon::FileCode),
    ("kt", Icon::FileCode),
    ("kts", Icon::FileCode),
    ("scala", Icon::FileCode),
    ("swift", Icon::FileCode),
    ("php", Icon::FileCode),
    ("cs", Icon::FileCode),
    ("lua", Icon::FileCode),
    ("js", Icon::FileCode),
    ("mjs", Icon::FileCode),
    ("cjs", Icon::FileCode),
    ("jsx", Icon::FileCode),
    ("ts", Icon::FileCode),
    ("tsx", Icon::FileCode),
    ("html", Icon::FileCode),
    ("htm", Icon::FileCode),
    ("css", Icon::FileCode),
    ("scss", Icon::FileCode),
    ("xml", Icon::FileCode),
    ("vue", Icon::FileCode),
    ("svelte", Icon::FileCode),
    ("zig", Icon::FileCode),
    ("ex", Icon::FileCode),
    ("exs", Icon::FileCode),
    ("erl", Icon::FileCode),
    ("hs", Icon::FileCode),
    ("ml", Icon::FileCode),
    ("dart", Icon::FileCode),
    ("r", Icon::FileCode),
    ("jl", Icon::FileCode),
    ("json", Icon::FileBraces),
    ("jsonc", Icon::FileBraces),
    ("json5", Icon::FileBraces),
    ("md", Icon::FileText),
    ("markdown", Icon::FileText),
    ("txt", Icon::FileText),
    ("rst", Icon::FileText),
    ("adoc", Icon::FileText),
    ("org", Icon::FileText),
    ("toml", Icon::FileCog),
    ("yaml", Icon::FileCog),
    ("yml", Icon::FileCog),
    ("ini", Icon::FileCog),
    ("cfg", Icon::FileCog),
    ("conf", Icon::FileCog),
    ("properties", Icon::FileCog),
    ("lock", Icon::FileLock),
    ("dockerfile", Icon::Container),
    ("sh", Icon::FileTerminal),
    ("bash", Icon::FileTerminal),
    ("zsh", Icon::FileTerminal),
    ("fish", Icon::FileTerminal),
    ("ps1", Icon::FileTerminal),
    ("bat", Icon::FileTerminal),
    ("cmd", Icon::FileTerminal),
    ("pem", Icon::FileKey),
    ("key", Icon::FileKey),
    ("crt", Icon::FileKey),
    ("p12", Icon::FileKey),
    ("png", Icon::FileImage),
    ("jpg", Icon::FileImage),
    ("jpeg", Icon::FileImage),
    ("gif", Icon::FileImage),
    ("webp", Icon::FileImage),
    ("bmp", Icon::FileImage),
    ("ico", Icon::FileImage),
    ("svg", Icon::FileImage),
    ("tiff", Icon::FileImage),
    ("avif", Icon::FileImage),
    ("zip", Icon::FileArchive),
    ("tar", Icon::FileArchive),
    ("gz", Icon::FileArchive),
    ("tgz", Icon::FileArchive),
    ("xz", Icon::FileArchive),
    ("bz2", Icon::FileArchive),
    ("zst", Icon::FileArchive),
    ("7z", Icon::FileArchive),
    ("rar", Icon::FileArchive),
    ("tar.gz", Icon::FileArchive),
    ("tar.xz", Icon::FileArchive),
    ("csv", Icon::FileSpreadsheet),
    ("tsv", Icon::FileSpreadsheet),
    ("xlsx", Icon::FileSpreadsheet),
    ("xls", Icon::FileSpreadsheet),
    ("ods", Icon::FileSpreadsheet),
    ("sql", Icon::Database),
    ("sqlite", Icon::Database),
    ("db", Icon::Database),
    ("diff", Icon::FileDiff),
    ("patch", Icon::FileDiff),
    ("mp4", Icon::FileVideoCamera),
    ("mov", Icon::FileVideoCamera),
    ("webm", Icon::FileVideoCamera),
    ("mkv", Icon::FileVideoCamera),
    ("avi", Icon::FileVideoCamera),
    ("mp3", Icon::FileMusic),
    ("wav", Icon::FileMusic),
    ("flac", Icon::FileMusic),
    ("ogg", Icon::FileMusic),
    ("m4a", Icon::FileMusic),
];

/// Longest name or extension that is lowercased and looked up; longer ones
/// can't be in the tables.
const MAX_KEY: usize = 64;

static NAME_MAP: LazyLock<HashMap<&'static str, Icon>> =
    LazyLock::new(|| FILE_NAMES.iter().copied().collect());
static EXTENSION_MAP: LazyLock<HashMap<&'static str, Icon>> =
    LazyLock::new(|| EXTENSIONS.iter().copied().collect());

/// The icon for a file called `name` (a file name, not a path).
///
/// In order: the exact name, ASCII case-insensitively; a name starting with
/// `.env`; the last two extensions (`tar.gz`); the last extension; else
/// [`Icon::File`]. A leading dot doesn't start an extension, so `.gitignore`
/// is a name. At most three hash probes, and no allocation: names and
/// extensions longer than 64 bytes skip their lookup.
pub fn file_icon(name: &str) -> Icon {
    let mut buf = [0u8; MAX_KEY];
    if let Some(icon) = lookup(&NAME_MAP, name, &mut buf) {
        return icon;
    }
    if name
        .as_bytes()
        .get(..4)
        .is_some_and(|start| start.eq_ignore_ascii_case(b".env"))
    {
        return Icon::FileKey;
    }
    // Skip a leading dot, so it can't start an extension.
    let stem_start = usize::from(name.starts_with('.'));
    let Some(last_dot) = name[stem_start..].rfind('.').map(|i| i + stem_start) else {
        return Icon::File;
    };
    if let Some(prev_dot) = name[stem_start..last_dot]
        .rfind('.')
        .map(|i| i + stem_start)
        && let Some(icon) = lookup(&EXTENSION_MAP, &name[prev_dot + 1..], &mut buf)
    {
        return icon;
    }
    lookup(&EXTENSION_MAP, &name[last_dot + 1..], &mut buf).unwrap_or(Icon::File)
}

/// The icon for a folder row.
pub fn folder_icon(expanded: bool) -> Icon {
    if expanded {
        Icon::FolderOpen
    } else {
        Icon::Folder
    }
}

/// Looks `key` up ASCII-lowercased through `buf`; `None` if it's empty, too
/// long, or absent.
fn lookup(map: &HashMap<&'static str, Icon>, key: &str, buf: &mut [u8; MAX_KEY]) -> Option<Icon> {
    if key.is_empty() || key.len() > MAX_KEY {
        return None;
    }
    let lower = &mut buf[..key.len()];
    lower.copy_from_slice(key.as_bytes());
    lower.make_ascii_lowercase();
    // Lowercasing ASCII bytes keeps UTF-8 valid, so this never fails.
    let lower = std::str::from_utf8(lower).ok()?;
    map.get(lower).copied()
}
