//! Images: local files drawn scaled down to the column, remote ones and
//! files outside the workspace blocked, never fetched.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use fe_markdown::{BlockReason, ImageSource, image_source};
use gpui::{
    AnyElement, App, Asset, Image, ImageCacheError, ImageFormat, InteractiveElement as _,
    IntoElement, ObjectFit, ParentElement as _, RenderImage, SMOOTH_SVG_SCALE_FACTOR, SharedString,
    Styled as _, StyledImage as _, Window, div, img, prelude::FluentBuilder as _, px,
};
use usvg::roxmltree;

use crate::view::RenderCx;

/// The placeholder's padding, both sides, and its border.
const BLOCKED_PAD_X_PX: f32 = 12.;
const BLOCKED_PAD_Y_PX: f32 = 8.;
const BLOCKED_BORDER_PX: f32 = 1.;

/// Where an image destination resolved to.
#[derive(Clone)]
pub(crate) enum Resolved {
    Local(LocalImage),
    Blocked(BlockReason),
}

/// A local image file and the folders the files it links to must be in.
#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) struct LocalImage {
    path: PathBuf,
    folders: Arc<[PathBuf]>,
}

/// A loaded image, and how many of its pixels a logical pixel is.
#[derive(Clone)]
pub(crate) struct Loaded {
    image: Arc<RenderImage>,
    scale: f32,
}

/// Why a local image isn't drawn.
#[derive(Clone)]
pub(crate) enum LoadError {
    /// An SVG linking to a file outside the folders.
    Blocked,
    Failed(ImageCacheError),
}

/// Loads a [`LocalImage`] as GPUI's own loader loads a path, but from the
/// bytes it checked: an SVG is drawn only if every file it links to is in
/// the folders, as GPUI's SVG parser would read them. An animated WebP
/// shows its first frame, as GPUI draws image bytes.
enum LocalImageLoader {}

impl Asset for LocalImageLoader {
    type Source = LocalImage;
    type Output = Result<Loaded, LoadError>;

    fn load(
        source: Self::Source,
        cx: &mut App,
    ) -> impl Future<Output = Self::Output> + Send + 'static {
        let renderer = cx.svg_renderer();
        async move {
            let failed = |err: ImageCacheError| LoadError::Failed(err);
            let bytes = std::fs::read(&source.path).map_err(|err| failed(err.into()))?;
            match raster_format(&bytes) {
                Some(format) => Ok(Loaded {
                    image: (Image::from_bytes(format, bytes).to_image_data(renderer))
                        .map_err(|err| failed(err.into()))?,
                    scale: 1.,
                }),
                None => {
                    check_svg_links(&bytes, &source.folders)?;
                    Ok(Loaded {
                        image: (renderer.render_single_frame(&bytes, 1.0))
                            .map_err(|err| failed(err.into()))?,
                        // GPUI draws an SVG at this many pixels a point.
                        scale: SMOOTH_SVG_SCALE_FACTOR,
                    })
                }
            }
        }
    }
}

/// The raster format of `bytes`, by their magic number, or `None` for
/// anything else, which GPUI takes for an SVG.
fn raster_format(bytes: &[u8]) -> Option<ImageFormat> {
    use image::ImageFormat as Raster;
    Some(match image::guess_format(bytes).ok()? {
        Raster::Png => ImageFormat::Png,
        Raster::Jpeg => ImageFormat::Jpeg,
        Raster::WebP => ImageFormat::Webp,
        Raster::Gif => ImageFormat::Gif,
        Raster::Bmp => ImageFormat::Bmp,
        Raster::Tiff => ImageFormat::Tiff,
        Raster::Ico => ImageFormat::Ico,
        Raster::Pnm => ImageFormat::Pnm,
        _ => return None,
    })
}

/// Checks that the SVG `bytes` link to no file outside `folders`.
///
/// GPUI's SVG parser, usvg, reads the file an `<image>` or `<feImage>`
/// `href` names, unless it's a data URL: an absolute path as it is, a
/// relative one against the process's working directory. SVGs it reads
/// that way, or from data URLs, read no files. Whether it reads an
/// element's `href` can turn on how the SVG draws, on its geometry and
/// fonts, so this checks every `href` in the document, read with the XML
/// parser and options usvg reads it with, as a path. An existing file
/// outside the folders blocks the SVG, and bytes this can't read aren't
/// drawn.
fn check_svg_links(bytes: &[u8], folders: &[PathBuf]) -> Result<(), LoadError> {
    let unreadable = |why: &str| LoadError::Failed(ImageCacheError::Asset(why.to_owned().into()));
    // As `usvg::Tree::from_data` tells compressed SVG.
    let data = if bytes.starts_with(&[0x1f, 0x8b]) {
        usvg::decompress_svgz(bytes).map_err(|_| unreadable("can't decompress the SVG"))?
    } else {
        bytes.to_vec()
    };
    let text = std::str::from_utf8(&data).map_err(|_| unreadable("the SVG isn't UTF-8"))?;
    let options = roxmltree::ParsingOptions {
        allow_dtd: true,
        ..Default::default()
    };
    let document = roxmltree::Document::parse_with_options(text, options)
        .map_err(|_| unreadable("can't parse the SVG"))?;
    let hrefs = (document.descendants())
        .flat_map(|node| node.attributes())
        .filter(|attr| attr.name() == "href");
    for href in hrefs {
        // A file that isn't there isn't read.
        if let Ok(path) = Path::new(href.value()).canonicalize()
            && !folders.iter().any(|folder| path.starts_with(folder))
        {
            return Err(LoadError::Blocked);
        }
    }
    Ok(())
}

/// Where `dest` resolves to, once per destination and document.
fn resolve(dest: &str, cx: &mut RenderCx) -> Resolved {
    let (doc_path, root) = (cx.doc_path, cx.root);
    (cx.images.entry(dest.to_owned()))
        .or_insert_with(|| match image_source(dest, doc_path, root) {
            ImageSource::Local(path) => {
                let folders = [root, doc_path.parent()]
                    .into_iter()
                    .flatten()
                    .filter_map(|folder| folder.canonicalize().ok())
                    .collect();
                Resolved::Local(LocalImage { path, folders })
            }
            ImageSource::Blocked(reason) => Resolved::Blocked(reason),
        })
        .clone()
}

/// The local file `resolved` names, or why it's blocked: it's remote,
/// outside the folders, or an SVG that loaded linking to a file outside.
fn blocked_why(
    resolved: &Resolved,
    window: &mut Window,
    cx: &mut App,
) -> Result<LocalImage, &'static str> {
    match resolved {
        Resolved::Blocked(BlockReason::Remote) => Err("Remote image blocked"),
        Resolved::Blocked(BlockReason::Outside) => Err("Image outside the workspace blocked"),
        Resolved::Local(local) => match window.use_asset::<LocalImageLoader>(local, cx) {
            Some(Err(LoadError::Blocked)) => {
                Err("Image linking files outside the workspace blocked")
            }
            _ => Ok(local.clone()),
        },
    }
}

/// The image `dest` with `alt` text: the file, while it loads or if it
/// can't be read its alt text muted, or a placeholder saying why it's
/// blocked. Its `debug_selector` is `md-image-<n>`, or `md-image-blocked-<n>`
/// when blocked (and its alt text's `md-image-blocked-<n>-alt`), `n` being
/// its place among the document's images. Its alt text while it loads is
/// `md-image-<n>-loading`, and if it can't be read `md-image-<n>-failed`.
pub(crate) fn render(dest: &str, alt: &str, cx: &mut RenderCx) -> AnyElement {
    let n = cx.next_image();
    let style = cx.style;
    if cx.compact {
        // Never loaded: the alt text stands in.
        return div()
            .debug_selector(|| format!("md-image-blocked-{n}-alt"))
            .text_color(style.muted)
            .child(alt.to_owned())
            .into_any_element();
    }
    let resolved = resolve(dest, cx);
    let local = match blocked_why(&resolved, cx.window, cx.app) {
        Ok(local) => local,
        Err(why) => {
            return div()
                .debug_selector(|| format!("md-image-blocked-{n}"))
                .flex()
                .flex_col()
                .self_start()
                .max_w_full()
                .px(px(BLOCKED_PAD_X_PX))
                .py(px(BLOCKED_PAD_Y_PX))
                .border(px(BLOCKED_BORDER_PX))
                .border_color(style.border)
                .rounded(px(4.))
                .when(!alt.is_empty(), |this| {
                    this.child(
                        div()
                            .debug_selector(|| format!("md-image-blocked-{n}-alt"))
                            .self_start()
                            .text_color(style.foreground)
                            .child(alt.to_owned()),
                    )
                })
                .child(div().text_color(style.muted).child(why))
                .into_any_element();
        }
    };
    let muted = style.muted;
    let alt_text = |state: &'static str| {
        let alt = SharedString::from(alt.to_owned());
        move || {
            div()
                .debug_selector(|| format!("md-image-{n}-{state}"))
                .text_color(muted)
                .child(alt.clone())
                .into_any_element()
        }
    };
    let source = move |window: &mut Window, cx: &mut App| {
        let loaded = window.use_asset::<LocalImageLoader>(&local, cx)?;
        Some(loaded.map(|loaded| loaded.image).map_err(|err| match err {
            LoadError::Failed(err) => err,
            LoadError::Blocked => ImageCacheError::Asset("blocked".into()),
        }))
    };
    // An image keeps the state that shows its loading text only with an id.
    img(source)
        .id(("md-image", n))
        .debug_selector(|| format!("md-image-{n}"))
        .max_w_full()
        .object_fit(ObjectFit::ScaleDown)
        .with_loading(alt_text("loading"))
        .with_fallback(alt_text("failed"))
        .into_any_element()
}

/// How wide [`render`] draws `dest` before the column narrows it: the
/// file's width once loaded, else its alt text's or its placeholder's.
pub(crate) fn width(dest: &str, alt: &str, cx: &mut RenderCx) -> f32 {
    if cx.compact {
        return cx.text_width(alt);
    }
    let resolved = resolve(dest, cx);
    match blocked_why(&resolved, cx.window, cx.app) {
        Ok(local) => match cx.window.use_asset::<LocalImageLoader>(&local, cx.app) {
            Some(Ok(loaded)) => loaded.image.size(0).width.0 as f32 / loaded.scale,
            _ => cx.text_width(alt),
        },
        Err(why) => {
            // A pixel more than its widest text: drawn, a text wraps when
            // its glyphs' widths add up to a hair more than its box's.
            let text = cx.text_width(alt).max(cx.text_width(why)) + 1.;
            text + 2. * (BLOCKED_PAD_X_PX + BLOCKED_BORDER_PX)
        }
    }
}
