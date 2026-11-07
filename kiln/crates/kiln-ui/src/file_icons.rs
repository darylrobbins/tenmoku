// SPDX-License-Identifier: Apache-2.0

//! The bundled icon set and the file-name → icon mapping.
//!
//! Icons are Lucide's, unmodified (see `assets/icons/LICENSE` and
//! THIRD_PARTY.md). They're monochrome strokes, so GPUI's `svg()` draws them as
//! an alpha mask tinted with the element's text colour. A set may answer a
//! multi-colour SVG instead ([`IconRef::Coloured`]), which
//! [`IconRef::element`] draws as an image in its own colours.
//!
//! The mapping is data: [`FILE_NAMES`] and [`EXTENSIONS`], built once into hash
//! maps.
//!
//! Every surface that shows a file or folder icon asks the installed
//! [`IconSet`] through [`file_icon_ref`], [`folder_icon_ref`] or their `_path`
//! forms, so installing another set with [`set_icon_set`] changes them all
//! together. With none installed the set is [`LucideIcons`], the mapping here.
//! [`ColouredIcons`](crate::ColouredIcons) is the other bundled set, in
//! colour.

use std::{cell::RefCell, collections::HashMap, sync::Arc, sync::LazyLock};

use gpui::{
    AnyElement, App, AssetSource as _, DevicePixels, Global, Hsla, ImageCacheError, IntoElement,
    Pixels, RenderImage, SharedString, Styled as _, SvgSize, Window, img, size, svg,
};

use crate::Assets;

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
    // A pinned tab's glyph; not mapped from names.
    Pin => "pin",
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
    // The Settings tab's Reset to Default; not mapped from names.
    RotateCcw => "rotate-ccw",
    // The Git Log's ref pills: a remote branch and a tag.
    Cloud => "cloud",
    Tag => "tag",
    // The which-key popup's row icons, by command or group; not mapped
    // from names.
    AppWindow => "app-window",
    Bot => "bot",
    Bug => "bug",
    FlaskConical => "flask-conical",
    LogOut => "log-out",
    PanelsTopLeft => "panels-top-left",
    Play => "play",
    Redo => "redo",
    Save => "save",
    Settings => "settings",
    Terminal => "terminal",
    TextWrap => "text-wrap",
    ToggleRight => "toggle-right",
    Undo => "undo",
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
pub(crate) const MAX_KEY: usize = 64;

static NAME_MAP: LazyLock<HashMap<&'static str, Icon>> =
    LazyLock::new(|| FILE_NAMES.iter().copied().collect());
static EXTENSION_MAP: LazyLock<HashMap<&'static str, Icon>> =
    LazyLock::new(|| EXTENSIONS.iter().copied().collect());

/// A file or folder icon as an icon set answers it: one bundled with kiln-ui,
/// or an SVG served by the app's asset source, masked or in its own colours.
/// Tree rows, crumbs and tabs take any of them; [`IconRef::element`] draws
/// one.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum IconRef {
    Bundled(Icon),
    /// An asset path, e.g. `"my-icons/rust.svg"`; drawn as a bundled icon is,
    /// as an alpha mask tinted with the text colour.
    Asset(SharedString),
    /// An asset path to a multi-colour SVG, e.g. `"my-icons/rust.svg"`; drawn
    /// as an image in its own colours, never tinted.
    Coloured(SharedString),
}

impl IconRef {
    /// The asset path: what `svg().path(..)` draws for a bundled or masked
    /// icon, and the coloured SVG's for a coloured one.
    pub fn path(&self) -> SharedString {
        match self {
            IconRef::Bundled(icon) => icon.path().into(),
            IconRef::Asset(path) | IconRef::Coloured(path) => path.clone(),
        }
    }

    /// The bundled icon, if it is one.
    pub fn bundled(&self) -> Option<Icon> {
        match self {
            IconRef::Bundled(icon) => Some(*icon),
            IconRef::Asset(_) | IconRef::Coloured(_) => None,
        }
    }

    /// Whether it draws in its own colours, so a tint can't mute it.
    pub fn is_coloured(&self) -> bool {
        matches!(self, IconRef::Coloured(_))
    }

    /// What debug selectors name it by: a bundled icon's name
    /// (`"file-code"`), else its asset path.
    pub fn name(&self) -> SharedString {
        match self {
            IconRef::Bundled(icon) => icon.name().into(),
            IconRef::Asset(path) | IconRef::Coloured(path) => path.clone(),
        }
    }

    /// The icon, `size` square. A bundled or masked icon is `svg()` tinted
    /// with `tint`. A coloured one is an image, `tint` ignored, its SVG
    /// rasterised once per window scale at exactly `size` in device pixels,
    /// so it's as sharp at 2x as at 1x; it's read from the app's asset
    /// source, else from [`Assets`].
    pub fn element(&self, size: Pixels, tint: Hsla) -> AnyElement {
        match self {
            IconRef::Coloured(path) => {
                let path = path.clone();
                let load = move |window: &mut Window, cx: &mut App| {
                    Some(coloured_image(&path, size, window, cx))
                };
                img(load).size(size).flex_none().into_any_element()
            }
            IconRef::Bundled(_) | IconRef::Asset(_) => svg()
                .path(self.path())
                .size(size)
                .flex_none()
                .text_color(tint)
                .into_any_element(),
        }
    }
}

/// The coloured icons an app has rasterised, by asset path and side in
/// device pixels, failures included so a missing file isn't read each frame.
/// Kept for the app's life, a handful of icons at one or two scales, until
/// [`Assets::register_coloured`] changes what's registered.
#[derive(Default)]
pub(crate) struct ColouredImages {
    pub(crate) images: HashMap<(SharedString, u32), Result<Arc<RenderImage>, ImageCacheError>>,
    /// [`Assets::coloured_revision`] when `images` was emptied last.
    revision: u64,
}

impl Global for ColouredImages {}

/// `path`'s coloured icon at `size` on `window`, from the cache or
/// rasterised into it. The same `Arc` each frame, so the window's atlas
/// keeps one texture for it.
pub(crate) fn coloured_image(
    path: &SharedString,
    size: Pixels,
    window: &mut Window,
    cx: &mut App,
) -> Result<Arc<RenderImage>, ImageCacheError> {
    let side = (f32::from(size) * window.scale_factor()).round().max(1.) as u32;
    let key = (path.clone(), side);
    let revision = Assets::coloured_revision();
    match cx.try_global::<ColouredImages>() {
        Some(cache) if cache.revision == revision => {
            if let Some(image) = cache.images.get(&key) {
                return image.clone();
            }
        }
        // A registration since: any image, or failure, may be stale.
        Some(_) => {
            let cache = cx.global_mut::<ColouredImages>();
            cache.revision = revision;
            let stale: Vec<_> = (cache.images.drain())
                .filter_map(|(_, image)| image.ok())
                .collect();
            // An atlas keeps an image's tile until it's dropped, from every
            // window: this one is out of the app's list while it draws.
            for image in stale {
                cx.drop_image(image, Some(window));
            }
            // So no window shows a dropped tile.
            cx.refresh_windows();
        }
        None => cx.set_global(ColouredImages {
            images: HashMap::new(),
            revision,
        }),
    }
    let image = rasterize_coloured(path, side, cx);
    cx.global_mut::<ColouredImages>()
        .images
        .insert(key, image.clone());
    image
}

/// `path`'s SVG rasterised `side` device pixels square. Read from the app's
/// asset source, else from [`Assets`], so kiln-ui's own coloured icons draw
/// even under a host source that doesn't delegate to it.
pub(crate) fn rasterize_coloured(
    path: &str,
    side: u32,
    cx: &App,
) -> Result<Arc<RenderImage>, ImageCacheError> {
    let bytes = match cx.asset_source().load(path) {
        Ok(Some(bytes)) => bytes,
        _ => Assets.load(path).ok().flatten().ok_or_else(|| {
            ImageCacheError::Asset(format!("coloured icon not found: {path}").into())
        })?,
    };
    let renderer = cx.svg_renderer();
    let svg = renderer.parse_svg(&bytes)?;
    let side = DevicePixels(side as i32);
    Ok(renderer.render_parsed(&svg, SvgSize::ExactSize(size(side, side)))?)
}

impl From<Icon> for IconRef {
    fn from(icon: Icon) -> Self {
        IconRef::Bundled(icon)
    }
}

impl PartialEq<Icon> for IconRef {
    fn eq(&self, icon: &Icon) -> bool {
        self.bundled() == Some(*icon)
    }
}

/// Which icon each file and folder gets.
pub trait IconSet: 'static {
    /// The icon for a file called `name` (a file name, not a path).
    fn file(&self, name: &str) -> IconRef;

    /// The icon for a folder row, open or closed.
    fn folder(&self, expanded: bool) -> IconRef;

    /// The icon for a folder row called `name` (a folder name, not a path),
    /// open or closed. By default [`IconSet::folder`]'s, whatever the name.
    fn folder_named(&self, _name: &str, expanded: bool) -> IconRef {
        self.folder(expanded)
    }
}

/// The default set: Lucide's icons through [`FILE_NAMES`] and [`EXTENSIONS`].
#[derive(Clone, Copy, Debug, Default)]
pub struct LucideIcons;

impl IconSet for LucideIcons {
    fn file(&self, name: &str) -> IconRef {
        lucide_file(name).into()
    }

    fn folder(&self, expanded: bool) -> IconRef {
        lucide_folder(expanded).into()
    }
}

thread_local! {
    /// The installed set. Per thread, as an app's globals are per app: an
    /// app and every window it draws live on its UI thread, and each test's
    /// app on its own. Free functions read it so callers without an `App`
    /// (a `PaneItem::tab_icon`, the gallery) can too.
    static ICON_SET: RefCell<Option<Arc<dyn IconSet>>> = const { RefCell::new(None) };
}

/// Installs `set` for every file and folder icon, and redraws every window
/// so what's on screen changes with it.
pub fn set_icon_set(set: Arc<dyn IconSet>, cx: &mut App) {
    ICON_SET.with(|installed| *installed.borrow_mut() = Some(set));
    cx.refresh_windows();
}

/// Puts [`LucideIcons`] back, redrawing every window.
pub fn reset_icon_set(cx: &mut App) {
    ICON_SET.with(|installed| *installed.borrow_mut() = None);
    cx.refresh_windows();
}

/// The installed set, or [`LucideIcons`] when none is.
pub fn icon_set() -> Arc<dyn IconSet> {
    // Cloned out, so a set may itself ask the free functions.
    ICON_SET
        .with(|installed| installed.borrow().clone())
        .unwrap_or_else(|| Arc::new(LucideIcons))
}

/// The installed set's icon for a file called `name`.
pub fn file_icon_ref(name: &str) -> IconRef {
    icon_set().file(name)
}

/// The installed set's icon for a folder row.
pub fn folder_icon_ref(expanded: bool) -> IconRef {
    icon_set().folder(expanded)
}

/// The installed set's icon for a folder row called `name`, which a set may
/// pick by the name ([`IconSet::folder_named`]).
pub fn folder_icon_ref_named(name: &str, expanded: bool) -> IconRef {
    icon_set().folder_named(name, expanded)
}

/// The asset path of the installed set's icon for a file called `name`, for a
/// surface that draws it itself; [`IconRef::element`] draws a coloured one
/// as it needs drawing.
pub fn file_icon_path(name: &str) -> SharedString {
    file_icon_ref(name).path()
}

/// The asset path of the installed set's icon for a folder row.
pub fn folder_icon_path(expanded: bool) -> SharedString {
    folder_icon_ref(expanded).path()
}

/// The installed set's icon for a file called `name`, where only a bundled
/// [`Icon`] will do, as in the gallery. A set that answers with an asset path
/// gets [`LucideIcons`]' icon here; app surfaces use [`file_icon_ref`].
pub fn file_icon(name: &str) -> Icon {
    file_icon_ref(name)
        .bundled()
        .unwrap_or_else(|| lucide_file(name))
}

/// The installed set's folder icon where only a bundled [`Icon`] will do;
/// see [`file_icon`].
pub fn folder_icon(expanded: bool) -> Icon {
    folder_icon_ref(expanded)
        .bundled()
        .unwrap_or_else(|| lucide_folder(expanded))
}

/// Lucide's icon for a file called `name` (a file name, not a path), by
/// [`by_name`].
fn lucide_file(name: &str) -> Icon {
    by_name(&NAME_MAP, &EXTENSION_MAP, Icon::FileKey, Icon::File, name)
}

/// What `names` and `extensions` give a file called `name` (a file name, not
/// a path).
///
/// In order: the exact name, ASCII case-insensitively; `env` for a name
/// starting with `.env`; the last two extensions (`tar.gz`); the last
/// extension; else `default`. A leading dot doesn't start an extension, so
/// `.gitignore` is a name. At most three hash probes, and no allocation:
/// names and extensions longer than 64 bytes skip their lookup.
pub(crate) fn by_name<V: Copy>(
    names: &HashMap<&'static str, V>,
    extensions: &HashMap<&'static str, V>,
    env: V,
    default: V,
    name: &str,
) -> V {
    let mut buf = [0u8; MAX_KEY];
    if let Some(icon) = lookup(names, name, &mut buf) {
        return icon;
    }
    if name
        .as_bytes()
        .get(..4)
        .is_some_and(|start| start.eq_ignore_ascii_case(b".env"))
    {
        return env;
    }
    // Skip a leading dot, so it can't start an extension.
    let stem_start = usize::from(name.starts_with('.'));
    let Some(last_dot) = name[stem_start..].rfind('.').map(|i| i + stem_start) else {
        return default;
    };
    if let Some(prev_dot) = name[stem_start..last_dot]
        .rfind('.')
        .map(|i| i + stem_start)
        && let Some(icon) = lookup(extensions, &name[prev_dot + 1..], &mut buf)
    {
        return icon;
    }
    lookup(extensions, &name[last_dot + 1..], &mut buf).unwrap_or(default)
}

/// Lucide's icon for a folder row.
fn lucide_folder(expanded: bool) -> Icon {
    if expanded {
        Icon::FolderOpen
    } else {
        Icon::Folder
    }
}

/// Looks `key` up ASCII-lowercased through `buf`; `None` if it's empty, too
/// long, or absent.
pub(crate) fn lookup<V: Copy>(
    map: &HashMap<&'static str, V>,
    key: &str,
    buf: &mut [u8; MAX_KEY],
) -> Option<V> {
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
