// SPDX-License-Identifier: LicenseRef-Proprietary

//! [`ColouredIcons`]: a curated subset of Material Icon Theme's icons, in
//! their own colours, for files and folders.
//!
//! The SVGs are upstream's, unmodified, in `assets/file-icons/` with its MIT
//! `LICENSE` (THIRD_PARTY.md lists them). [`Assets`](crate::Assets) serves
//! them at `kiln-ui/file-icons/<stem>.svg`; [`IconRef::element`] draws them
//! as images.
//!
//! Which file gets which icon is data, in the shape of Lucide's tables:
//! [`FILE_NAMES`], [`EXTENSIONS`] and [`FOLDER_NAMES`] map a name to an
//! icon's file stem. A folder stem's open icon is `<stem>-open`. A stem with
//! a `<stem>_light` file, for an icon that would vanish on a light
//! background, draws that under a light theme.

use std::{collections::HashMap, sync::LazyLock};

use crate::{
    Appearance, IconRef, IconSet,
    file_icons::{MAX_KEY, by_name, lookup},
    theme::active_appearance,
};

macro_rules! files {
    ($($stem:literal,)*) => {
        /// Every bundled file: its stem, asset path and SVG.
        pub(crate) const FILES: &[(&str, &str, &[u8])] = &[$((
            $stem,
            concat!("kiln-ui/file-icons/", $stem, ".svg"),
            include_bytes!(concat!("../assets/file-icons/", $stem, ".svg")),
        ),)*];
    };
}

files! {
    "audio",
    "bun",
    "bun_light",
    "c",
    "certificate",
    "changelog",
    "cmake",
    "console",
    "cpp",
    "csharp",
    "css",
    "dart",
    "database",
    "diff",
    "docker",
    "document",
    "editorconfig",
    "elixir",
    "erlang",
    "exe",
    "file",
    "folder",
    "folder-benchmark",
    "folder-benchmark-open",
    "folder-config",
    "folder-config-open",
    "folder-css",
    "folder-css-open",
    "folder-dist",
    "folder-dist-open",
    "folder-docs",
    "folder-docs-open",
    "folder-examples",
    "folder-examples-open",
    "folder-font",
    "folder-font-open",
    "folder-git",
    "folder-github",
    "folder-github-open",
    "folder-git-open",
    "folder-i18n",
    "folder-i18n-open",
    "folder-images",
    "folder-images-open",
    "folder-include",
    "folder-include-open",
    "folder-lib",
    "folder-lib-open",
    "folder-mock",
    "folder-mock-open",
    "folder-node",
    "folder-node-open",
    "folder-open",
    "folder-packages",
    "folder-packages-open",
    "folder-public",
    "folder-public-open",
    "folder-resource",
    "folder-resource-open",
    "folder-rust",
    "folder-rust-open",
    "folder-scripts",
    "folder-scripts-open",
    "folder-src",
    "folder-src-open",
    "folder-target",
    "folder-target-open",
    "folder-temp",
    "folder-temp-open",
    "folder-test",
    "folder-test-open",
    "folder-theme",
    "folder-theme-open",
    "folder-tools",
    "folder-tools-open",
    "folder-utils",
    "folder-utils-open",
    "folder-vscode",
    "folder-vscode-open",
    "font",
    "gemfile",
    "git",
    "go",
    "go-mod",
    "gradle",
    "h",
    "haskell",
    "hpp",
    "html",
    "image",
    "java",
    "javascript",
    "json",
    "julia",
    "just",
    "key",
    "kotlin",
    "less",
    "license",
    "lock",
    "log",
    "lua",
    "makefile",
    "markdown",
    "maven",
    "nix",
    "nodejs",
    "npm",
    "ocaml",
    "pdf",
    "php",
    "pnpm",
    "pnpm_light",
    "powershell",
    "python",
    "python-misc",
    "r",
    "react",
    "react_ts",
    "readme",
    "ruby",
    "rust",
    "sass",
    "scala",
    "settings",
    "svelte",
    "svg",
    "swift",
    "table",
    "tex",
    "toml",
    "toml_light",
    "tune",
    "typescript",
    "typescript-def",
    "video",
    "vue",
    "xml",
    "yaml",
    "yarn",
    "zig",
    "zip",
}

/// A file no table names.
pub(crate) const DEFAULT_FILE: &str = "file";
/// A file whose name starts with `.env`.
pub(crate) const ENV_FILE: &str = "tune";
/// A folder [`FOLDER_NAMES`] doesn't name.
pub(crate) const DEFAULT_FOLDER: &str = "folder";
/// Appended to a folder stem for its open icon.
pub(crate) const OPEN: &str = "-open";
/// Appended to a stem for its icon on a light background.
pub(crate) const LIGHT: &str = "_light";

/// Exact file names, ASCII lowercase, to stems. Checked before any
/// extension.
pub(crate) const FILE_NAMES: &[(&str, &str)] = &[
    (".editorconfig", "editorconfig"),
    (".ecrc", "editorconfig"),
    ("makefile", "makefile"),
    ("gnumakefile", "makefile"),
    ("kbuild", "makefile"),
    ("justfile", "just"),
    (".justfile", "just"),
    ("cmakelists.txt", "cmake"),
    ("cmakecache.txt", "cmake"),
    ("cmakepresets.json", "cmake"),
    ("dockerfile", "docker"),
    ("containerfile", "docker"),
    (".dockerignore", "docker"),
    (".containerignore", "docker"),
    ("docker-compose.yml", "docker"),
    ("docker-compose.yaml", "docker"),
    ("compose.yml", "docker"),
    ("compose.yaml", "docker"),
    ("cargo.toml", "rust"),
    ("cargo.lock", "lock"),
    ("package.json", "nodejs"),
    ("package-lock.json", "nodejs"),
    (".nvmrc", "nodejs"),
    (".node-version", "nodejs"),
    (".npmrc", "npm"),
    (".npmignore", "npm"),
    ("pnpm-lock.yaml", "pnpm"),
    ("pnpm-workspace.yaml", "pnpm"),
    (".pnpmfile.cjs", "pnpm"),
    ("yarn.lock", "yarn"),
    (".yarnrc", "yarn"),
    (".yarnrc.yml", "yarn"),
    (".yarnrc.yaml", "yarn"),
    ("bun.lock", "bun"),
    ("bun.lockb", "bun"),
    ("bunfig.toml", "bun"),
    (".bun-version", "bun"),
    ("go.mod", "go-mod"),
    ("go.sum", "go-mod"),
    ("go.work", "go-mod"),
    ("go.work.sum", "go-mod"),
    ("pyproject.toml", "python-misc"),
    ("requirements.txt", "python-misc"),
    ("pipfile", "python-misc"),
    (".python-version", "python-misc"),
    ("py.typed", "python-misc"),
    ("pylintrc", "python-misc"),
    (".pylintrc", "python-misc"),
    (".coveragerc", "python-misc"),
    ("gemfile", "gemfile"),
    ("rakefile", "ruby"),
    ("podfile", "ruby"),
    ("brewfile", "ruby"),
    ("guardfile", "ruby"),
    ("capfile", "ruby"),
    (".ruby-version", "ruby"),
    ("pom.xml", "maven"),
    ("gradle.properties", "gradle"),
    ("gradlew", "gradle"),
    ("gradlew.bat", "gradle"),
    (".gitignore", "git"),
    (".gitattributes", "git"),
    (".gitmodules", "git"),
    (".gitconfig", "git"),
    (".gitkeep", "git"),
    (".git-blame-ignore-revs", "git"),
    ("commit_editmsg", "git"),
    ("merge_msg", "git"),
    ("tag_editmsg", "git"),
    ("git-rebase-todo", "git"),
    (".bashrc", "console"),
    (".bash_profile", "console"),
    (".bash_login", "console"),
    (".bash_logout", "console"),
    (".bash_aliases", "console"),
    (".profile", "console"),
    (".zshrc", "console"),
    (".zshenv", "console"),
    (".zprofile", "console"),
    (".zlogin", "console"),
    (".zlogout", "console"),
    (".envrc", "console"),
    ("pkgbuild", "console"),
    ("apkbuild", "console"),
    (".babelrc", "json"),
    (".bowerrc", "json"),
    (".eslintrc", "json"),
    (".hintrc", "json"),
    (".jscsrc", "json"),
    (".jsfmtrc", "json"),
    (".jshintrc", "json"),
    (".jslintrc", "json"),
    (".swcrc", "json"),
    (".vuerc", "json"),
    (".watchmanconfig", "json"),
    ("composer.lock", "json"),
    ("license", "license"),
    ("license.md", "license"),
    ("license.txt", "license"),
    ("license-mit", "license"),
    ("license-apache", "license"),
    ("licence", "license"),
    ("licence.md", "license"),
    ("licence.txt", "license"),
    ("copying", "license"),
    ("copying.md", "license"),
    ("copying.txt", "license"),
    ("copyright", "license"),
    ("notice", "license"),
    ("notice.md", "license"),
    ("notice.txt", "license"),
    ("readme", "readme"),
    ("readme.md", "readme"),
    ("readme.txt", "readme"),
    ("readme.rst", "readme"),
    ("changelog", "changelog"),
    ("changelog.md", "changelog"),
    ("changelog.txt", "changelog"),
    ("changes", "changelog"),
    ("changes.md", "changelog"),
];

/// Extensions without the dot, ASCII lowercase, to stems. A two-part key
/// (`d.ts`) is tried before the last extension alone.
pub(crate) const EXTENSIONS: &[(&str, &str)] = &[
    ("rs", "rust"),
    ("ron", "rust"),
    ("py", "python"),
    ("pyi", "python"),
    ("pyw", "python"),
    ("pyc", "python-misc"),
    ("whl", "python-misc"),
    ("js", "javascript"),
    ("mjs", "javascript"),
    ("cjs", "javascript"),
    ("jsx", "react"),
    ("ts", "typescript"),
    ("mts", "typescript"),
    ("cts", "typescript"),
    ("tsx", "react_ts"),
    ("d.ts", "typescript-def"),
    ("d.mts", "typescript-def"),
    ("d.cts", "typescript-def"),
    ("json", "json"),
    ("jsonc", "json"),
    ("json5", "json"),
    ("jsonl", "json"),
    ("ndjson", "json"),
    ("webmanifest", "json"),
    ("har", "json"),
    ("jsonld", "json"),
    ("geojson", "json"),
    ("ipynb", "json"),
    ("bowerrc", "json"),
    ("jscsrc", "json"),
    ("jslintrc", "json"),
    ("vuerc", "json"),
    ("eslintrc", "json"),
    ("jsfmtrc", "json"),
    ("jshintrc", "json"),
    ("swcrc", "json"),
    ("hintrc", "json"),
    ("babelrc", "json"),
    ("code-workspace", "json"),
    ("code-snippets", "json"),
    ("toml", "toml"),
    ("go", "go"),
    ("c", "c"),
    ("h", "h"),
    ("cc", "cpp"),
    ("cpp", "cpp"),
    ("cxx", "cpp"),
    ("c++", "cpp"),
    ("hh", "hpp"),
    ("hpp", "hpp"),
    ("hxx", "hpp"),
    ("h++", "hpp"),
    ("inl", "hpp"),
    ("sh", "console"),
    ("bash", "console"),
    ("zsh", "console"),
    ("ksh", "console"),
    ("fish", "console"),
    ("csh", "console"),
    ("nu", "console"),
    ("bat", "console"),
    ("cmd", "console"),
    ("ps1", "powershell"),
    ("psm1", "powershell"),
    ("psd1", "powershell"),
    ("html", "html"),
    ("htm", "html"),
    ("xhtml", "html"),
    ("css", "css"),
    ("scss", "sass"),
    ("sass", "sass"),
    ("less", "less"),
    ("yaml", "yaml"),
    ("yml", "yaml"),
    ("md", "markdown"),
    ("markdown", "markdown"),
    ("rst", "markdown"),
    ("java", "java"),
    ("rb", "ruby"),
    ("erb", "ruby"),
    ("gemspec", "ruby"),
    ("rake", "ruby"),
    ("ru", "ruby"),
    ("podspec", "ruby"),
    ("lua", "lua"),
    ("php", "php"),
    ("cs", "csharp"),
    ("csx", "csharp"),
    ("scala", "scala"),
    ("sc", "scala"),
    ("xml", "xml"),
    ("plist", "xml"),
    ("xsd", "xml"),
    ("xsl", "xml"),
    ("xslt", "xml"),
    ("svg", "svg"),
    ("swift", "swift"),
    ("kt", "kotlin"),
    ("kts", "kotlin"),
    ("gradle", "gradle"),
    ("gradle.kts", "gradle"),
    ("sql", "database"),
    ("sqlite", "database"),
    ("sqlite3", "database"),
    ("db", "database"),
    ("dockerfile", "docker"),
    ("containerfile", "docker"),
    ("mk", "makefile"),
    ("mak", "makefile"),
    ("cmake", "cmake"),
    ("gitignore", "git"),
    ("ini", "settings"),
    ("cfg", "settings"),
    ("conf", "settings"),
    ("config", "settings"),
    ("properties", "settings"),
    ("env", "tune"),
    ("just", "just"),
    ("diff", "diff"),
    ("patch", "diff"),
    ("rej", "diff"),
    ("csv", "table"),
    ("tsv", "table"),
    ("tab", "table"),
    ("psv", "table"),
    ("xlsx", "table"),
    ("xls", "table"),
    ("ods", "table"),
    ("zig", "zig"),
    ("zon", "zig"),
    ("ex", "elixir"),
    ("exs", "elixir"),
    ("heex", "elixir"),
    ("erl", "erlang"),
    ("hrl", "erlang"),
    ("hs", "haskell"),
    ("lhs", "haskell"),
    ("ml", "ocaml"),
    ("mli", "ocaml"),
    ("dart", "dart"),
    ("r", "r"),
    ("rmd", "r"),
    ("jl", "julia"),
    ("vue", "vue"),
    ("svelte", "svelte"),
    ("nix", "nix"),
    ("tex", "tex"),
    ("latex", "tex"),
    ("ltx", "tex"),
    ("sty", "tex"),
    ("lock", "lock"),
    ("png", "image"),
    ("jpg", "image"),
    ("jpeg", "image"),
    ("gif", "image"),
    ("webp", "image"),
    ("bmp", "image"),
    ("ico", "image"),
    ("tif", "image"),
    ("tiff", "image"),
    ("avif", "image"),
    ("heic", "image"),
    ("icns", "image"),
    ("woff", "font"),
    ("woff2", "font"),
    ("ttf", "font"),
    ("otf", "font"),
    ("eot", "font"),
    ("ttc", "font"),
    ("zip", "zip"),
    ("tar", "zip"),
    ("gz", "zip"),
    ("tgz", "zip"),
    ("xz", "zip"),
    ("bz2", "zip"),
    ("zst", "zip"),
    ("7z", "zip"),
    ("rar", "zip"),
    ("deb", "zip"),
    ("rpm", "zip"),
    ("tar.gz", "zip"),
    ("tar.xz", "zip"),
    ("tar.bz2", "zip"),
    ("tar.zst", "zip"),
    ("txt", "document"),
    ("pem", "key"),
    ("key", "key"),
    ("pub", "key"),
    ("asc", "key"),
    ("gpg", "key"),
    ("crt", "certificate"),
    ("cer", "certificate"),
    ("cert", "certificate"),
    ("p12", "certificate"),
    ("pfx", "certificate"),
    ("mp4", "video"),
    ("m4v", "video"),
    ("mov", "video"),
    ("webm", "video"),
    ("mkv", "video"),
    ("avi", "video"),
    ("mp3", "audio"),
    ("wav", "audio"),
    ("flac", "audio"),
    ("ogg", "audio"),
    ("opus", "audio"),
    ("m4a", "audio"),
    ("aac", "audio"),
    ("pdf", "pdf"),
    ("log", "log"),
    ("exe", "exe"),
    ("msi", "exe"),
];

/// Folder names, ASCII lowercase, to closed folder stems. A name with a
/// leading `.` or `_`, or a trailing `_` (`.github`, `__tests__`), is
/// looked up without them if it isn't here as it is.
pub(crate) const FOLDER_NAMES: &[(&str, &str)] = &[
    ("src", "folder-src"),
    ("srcs", "folder-src"),
    ("source", "folder-src"),
    ("sources", "folder-src"),
    ("code", "folder-src"),
    ("test", "folder-test"),
    ("tests", "folder-test"),
    ("testing", "folder-test"),
    ("spec", "folder-test"),
    ("specs", "folder-test"),
    ("snapshots", "folder-test"),
    ("testdata", "folder-test"),
    ("doc", "folder-docs"),
    ("docs", "folder-docs"),
    ("documentation", "folder-docs"),
    ("wiki", "folder-docs"),
    ("cfg", "folder-config"),
    ("conf", "folder-config"),
    ("config", "folder-config"),
    ("configs", "folder-config"),
    ("configuration", "folder-config"),
    ("settings", "folder-config"),
    ("options", "folder-config"),
    ("prefs", "folder-config"),
    ("meta-inf", "folder-config"),
    ("images", "folder-images"),
    ("image", "folder-images"),
    ("img", "folder-images"),
    ("imgs", "folder-images"),
    ("icons", "folder-images"),
    ("icon", "folder-images"),
    ("figures", "folder-images"),
    ("screenshots", "folder-images"),
    ("pics", "folder-images"),
    ("pictures", "folder-images"),
    ("photos", "folder-images"),
    ("assets", "folder-resource"),
    ("asset", "folder-resource"),
    ("resources", "folder-resource"),
    ("resource", "folder-resource"),
    ("res", "folder-resource"),
    ("static", "folder-resource"),
    ("script", "folder-scripts"),
    ("scripts", "folder-scripts"),
    ("xtask", "folder-scripts"),
    ("git", "folder-git"),
    ("githooks", "folder-git"),
    ("patches", "folder-git"),
    ("submodules", "folder-git"),
    ("github", "folder-github"),
    ("node_modules", "folder-node"),
    ("node", "folder-node"),
    ("nodejs", "folder-node"),
    ("dist", "folder-dist"),
    ("out", "folder-dist"),
    ("output", "folder-dist"),
    ("build", "folder-dist"),
    ("builds", "folder-dist"),
    ("release", "folder-dist"),
    ("bin", "folder-dist"),
    ("built", "folder-dist"),
    ("compiled", "folder-dist"),
    ("target", "folder-target"),
    ("lib", "folder-lib"),
    ("libs", "folder-lib"),
    ("library", "folder-lib"),
    ("vendor", "folder-lib"),
    ("vendors", "folder-lib"),
    ("third_party", "folder-lib"),
    ("third-party", "folder-lib"),
    ("thirdparty", "folder-lib"),
    ("external", "folder-lib"),
    ("crates", "folder-lib"),
    ("include", "folder-include"),
    ("includes", "folder-include"),
    ("inc", "folder-include"),
    ("example", "folder-examples"),
    ("examples", "folder-examples"),
    ("demo", "folder-examples"),
    ("demos", "folder-examples"),
    ("sample", "folder-examples"),
    ("samples", "folder-examples"),
    ("tools", "folder-tools"),
    ("tooling", "folder-tools"),
    ("devtools", "folder-tools"),
    ("bench", "folder-benchmark"),
    ("benches", "folder-benchmark"),
    ("benchmark", "folder-benchmark"),
    ("benchmarks", "folder-benchmark"),
    ("perf", "folder-benchmark"),
    ("public", "folder-public"),
    ("www", "folder-public"),
    ("web", "folder-public"),
    ("util", "folder-utils"),
    ("utils", "folder-utils"),
    ("utility", "folder-utils"),
    ("utilities", "folder-utils"),
    ("font", "folder-font"),
    ("fonts", "folder-font"),
    ("css", "folder-css"),
    ("style", "folder-css"),
    ("styles", "folder-css"),
    ("stylesheets", "folder-css"),
    ("theme", "folder-theme"),
    ("themes", "folder-theme"),
    ("colors", "folder-theme"),
    ("colours", "folder-theme"),
    ("vscode", "folder-vscode"),
    ("temp", "folder-temp"),
    ("tmp", "folder-temp"),
    ("cache", "folder-temp"),
    ("package", "folder-packages"),
    ("packages", "folder-packages"),
    ("pkg", "folder-packages"),
    ("pkgs", "folder-packages"),
    ("mock", "folder-mock"),
    ("mocks", "folder-mock"),
    ("fixture", "folder-mock"),
    ("fixtures", "folder-mock"),
    ("i18n", "folder-i18n"),
    ("l10n", "folder-i18n"),
    ("locale", "folder-i18n"),
    ("locales", "folder-i18n"),
    ("lang", "folder-i18n"),
    ("translations", "folder-i18n"),
    ("rust", "folder-rust"),
    ("cargo", "folder-rust"),
];

static NAME_MAP: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    FILE_NAMES
        .iter()
        .map(|&(name, stem)| (name, path(stem)))
        .collect()
});
static EXTENSION_MAP: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    EXTENSIONS
        .iter()
        .map(|&(ext, stem)| (ext, path(stem)))
        .collect()
});
/// Folder names to their closed and open paths.
static FOLDER_MAP: LazyLock<HashMap<&'static str, [&'static str; 2]>> = LazyLock::new(|| {
    (FOLDER_NAMES.iter())
        .map(|&(name, stem)| (name, folder_paths(stem)))
        .collect()
});
static DEFAULT_FOLDER_PATHS: LazyLock<[&'static str; 2]> =
    LazyLock::new(|| folder_paths(DEFAULT_FOLDER));
/// Bundled files' paths by stem.
static BY_STEM: LazyLock<HashMap<&'static str, &'static str>> =
    LazyLock::new(|| FILES.iter().map(|&(stem, path, _)| (stem, path)).collect());
/// Bundled files by asset path.
static BY_PATH: LazyLock<HashMap<&'static str, &'static [u8]>> =
    LazyLock::new(|| FILES.iter().map(|&(_, path, svg)| (path, svg)).collect());
/// Paths to their light variants', for the icons that have one.
static LIGHT_PATHS: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    (FILES.iter())
        .filter_map(|&(stem, light, _)| Some((path(stem.strip_suffix(LIGHT)?), light)))
        .collect()
});

/// The bundled file at `path`, if there's one.
pub(crate) fn bundled(path: &str) -> Option<&'static [u8]> {
    BY_PATH.get(path).copied()
}

/// The asset path of the bundled file `stem`; the default file's if there's
/// none, which a test rules out.
pub(crate) fn path(stem: &str) -> &'static str {
    BY_STEM.get(stem).copied().unwrap_or(DEFAULT_PATH)
}

/// [`DEFAULT_FILE`]'s path.
const DEFAULT_PATH: &str = "kiln-ui/file-icons/file.svg";

/// A folder stem's closed and open paths.
fn folder_paths(stem: &str) -> [&'static str; 2] {
    [path(stem), path(&format!("{stem}{OPEN}"))]
}

/// Material Icon Theme's icons, in their own colours: a curated subset
/// with an icon per language kiln's hosts highlight, per well-known file
/// (`Makefile`, `Cargo.toml`, `.editorconfig`, …) and per well-known folder
/// (`src`, `tests`, `.git`, `node_modules`, …), open and closed. An icon
/// that would vanish on a light background has a light variant, drawn
/// while the active theme is light.
///
/// Not installed by default: install it with
/// [`set_icon_set`](crate::set_icon_set).
#[derive(Clone, Copy, Debug, Default)]
pub struct ColouredIcons;

impl ColouredIcons {
    /// The asset path of the icon for a file called `name` under a theme of
    /// `appearance`.
    pub fn file_path(name: &str, appearance: Appearance) -> &'static str {
        let path = by_name(
            &NAME_MAP,
            &EXTENSION_MAP,
            path(ENV_FILE),
            path(DEFAULT_FILE),
            name,
        );
        themed(path, appearance)
    }

    /// The asset path of the icon for a folder called `name`, open or
    /// closed, under a theme of `appearance`. Named as [`FOLDER_NAMES`]
    /// says, ASCII case-insensitively, else the plain folder's.
    pub fn folder_path(name: &str, expanded: bool, appearance: Appearance) -> &'static str {
        let mut buf = [0u8; MAX_KEY];
        let trimmed = name.trim_start_matches(['.', '_']).trim_end_matches('_');
        let paths = lookup(&FOLDER_MAP, name, &mut buf)
            .or_else(|| lookup(&FOLDER_MAP, trimmed, &mut buf))
            .unwrap_or(*DEFAULT_FOLDER_PATHS);
        themed(paths[usize::from(expanded)], appearance)
    }
}

/// `path`, or its light variant under a light theme if it has one.
fn themed(path: &'static str, appearance: Appearance) -> &'static str {
    match appearance {
        Appearance::Light => LIGHT_PATHS.get(path).copied().unwrap_or(path),
        Appearance::Dark => path,
    }
}

impl IconSet for ColouredIcons {
    fn file(&self, name: &str) -> IconRef {
        IconRef::Coloured(Self::file_path(name, active_appearance()).into())
    }

    fn folder(&self, expanded: bool) -> IconRef {
        self.folder_named("", expanded)
    }

    fn folder_named(&self, name: &str, expanded: bool) -> IconRef {
        IconRef::Coloured(Self::folder_path(name, expanded, active_appearance()).into())
    }
}
