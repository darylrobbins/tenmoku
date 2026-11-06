use std::{
    fs,
    path::{Path, PathBuf},
};

use fe_markdown::{BlockReason, ImageSource, LinkTarget, image_source, link_target};

fn file(path: impl Into<PathBuf>, line: Option<u32>) -> LinkTarget {
    LinkTarget::File {
        path: path.into(),
        line,
    }
}

#[test]
fn link_targets_resolve_as_vs_code_opens_them() {
    let doc = Path::new("/work/docs/README.md");
    let root = Some(Path::new("/work"));
    let target = |dest| link_target(dest, doc, root);

    // Fragments are headings of this document.
    assert_eq!(target("#usage"), LinkTarget::Anchor("usage".into()));
    assert_eq!(
        target("#Usage%20Notes"),
        LinkTarget::Anchor("Usage Notes".into())
    );

    // Web and mail links.
    for dest in [
        "http://a.com",
        "https://a.com/b?c#d",
        "HTTPS://A.COM",
        "mailto:me@a.com",
    ] {
        assert_eq!(target(dest), LinkTarget::Web(dest.into()), "{dest}");
    }

    // Relative paths, against the document's folder.
    assert_eq!(target("other.md"), file("/work/docs/other.md", None));
    assert_eq!(
        target("./sub/a%20b.md?x=1"),
        file("/work/docs/sub/a b.md", None)
    );
    assert_eq!(target("../src/main.rs"), file("/work/src/main.rs", None));
    // `#L12` and `#L12C3` give the 0-based line; other fragments none.
    assert_eq!(
        target("../src/main.rs#L12"),
        file("/work/src/main.rs", Some(11))
    );
    assert_eq!(target("lib.rs#L12C3"), file("/work/docs/lib.rs", Some(11)));
    assert_eq!(target("lib.rs?q#L1"), file("/work/docs/lib.rs", Some(0)));
    assert_eq!(target("lib.rs#intro"), file("/work/docs/lib.rs", None));
    assert_eq!(target("lib.rs#L12x"), file("/work/docs/lib.rs", None));
    // A leading `/` is the workspace root, or with no folder open, the
    // document's folder.
    assert_eq!(target("/CHANGELOG.md"), file("/work/CHANGELOG.md", None));
    assert_eq!(
        link_target("/CHANGELOG.md#L2", doc, None),
        file("/work/docs/CHANGELOG.md", Some(1))
    );

    // `file:` URLs are absolute.
    assert_eq!(target("file:///etc/hosts#L3"), file("/etc/hosts", Some(2)));
    assert_eq!(
        target("file://localhost/a%20b/c.md"),
        file("/a b/c.md", None)
    );

    // Anything else isn't opened.
    for dest in [
        "javascript:alert(1)",
        "JavaScript:alert(1)",
        "data:text/html,<b>x</b>",
        "vscode:extension/rust-lang.rust-analyzer",
        "ftp://a.com/b",
    ] {
        assert_eq!(target(dest), LinkTarget::Unsupported, "{dest}");
    }
}

#[test]
fn only_local_images_inside_the_root_or_document_folder_load() {
    let temp = tempfile::tempdir().unwrap();
    let base = dunce::canonicalize(temp.path()).unwrap();
    let root = base.join("root");
    let docs = root.join("docs");
    let outside = base.join("outside");
    for dir in [&docs, &outside] {
        fs::create_dir_all(dir).unwrap();
    }
    for image in [
        root.join("logo.png"),
        docs.join("pic.png"),
        outside.join("x.png"),
    ] {
        fs::write(image, b"png").unwrap();
    }
    let doc = docs.join("README.md");
    let source = |dest| image_source(dest, &doc, Some(&root));
    let outside_blocked = ImageSource::Blocked(BlockReason::Outside);
    let remote_blocked = ImageSource::Blocked(BlockReason::Remote);

    // Inside the document's folder or the root.
    assert_eq!(source("pic.png"), ImageSource::Local(docs.join("pic.png")));
    assert_eq!(
        source("./pic.png?v=2"),
        ImageSource::Local(docs.join("pic.png"))
    );
    assert_eq!(
        source("../logo.png"),
        ImageSource::Local(root.join("logo.png"))
    );
    assert_eq!(
        source("/logo.png"),
        ImageSource::Local(root.join("logo.png"))
    );
    let url = format!("file://{}", root.join("logo.png").display());
    assert_eq!(source(&url), ImageSource::Local(root.join("logo.png")));
    // A missing file is still local: the preview shows it as broken.
    assert_eq!(source("new.png"), ImageSource::Local(docs.join("new.png")));

    // Outside both.
    assert_eq!(source("../../outside/x.png"), outside_blocked);
    assert_eq!(source("/../outside/x.png"), outside_blocked);
    assert_eq!(source("../../outside/missing.png"), outside_blocked);
    let url = format!("file://{}", outside.join("x.png").display());
    assert_eq!(source(&url), outside_blocked);
    // A symlink inside that points outside is outside.
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(outside.join("x.png"), docs.join("link.png")).unwrap();
        assert_eq!(source("link.png"), outside_blocked);
        // So is a missing file in a linked folder.
        std::os::unix::fs::symlink(&outside, docs.join("away")).unwrap();
        assert_eq!(source("away/missing.png"), outside_blocked);
        assert_eq!(
            source("away/../pic.png"),
            ImageSource::Local(docs.join("pic.png"))
        );
    }

    // Remote images, and every other scheme, are blocked.
    for dest in [
        "http://a.com/x.png",
        "https://a.com/x.png",
        "data:image/png;base64,iVBORw0KGgo=",
        "ftp://a.com/x.png",
    ] {
        assert_eq!(source(dest), remote_blocked, "{dest}");
    }

    // With no folder open, only the document's folder counts.
    assert_eq!(
        image_source("pic.png", &doc, None),
        ImageSource::Local(docs.join("pic.png"))
    );
    assert_eq!(image_source("../logo.png", &doc, None), outside_blocked);
    // A document outside the root may show images from its own folder.
    let elsewhere = outside.join("notes.md");
    assert_eq!(
        image_source("x.png", &elsewhere, Some(&root)),
        ImageSource::Local(outside.join("x.png"))
    );
}

#[cfg(unix)]
#[test]
fn folders_reached_through_symlinks_still_hold_their_images() {
    let temp = tempfile::tempdir().unwrap();
    let real = dunce::canonicalize(temp.path()).unwrap().join("real");
    fs::create_dir_all(&real).unwrap();
    fs::write(real.join("a.png"), b"png").unwrap();
    let linked = temp.path().join("linked");
    std::os::unix::fs::symlink(&real, &linked).unwrap();
    let doc = linked.join("README.md");
    assert_eq!(
        image_source("a.png", &doc, Some(&linked)),
        ImageSource::Local(real.join("a.png"))
    );
    assert_eq!(
        image_source("missing.png", &doc, Some(&linked)),
        ImageSource::Local(real.join("missing.png"))
    );
}

#[test]
fn a_relative_document_path_allows_only_its_own_folder() {
    let temp = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(temp.path()).unwrap();
    let outside = tempfile::tempdir().unwrap();
    let image = dunce::canonicalize(outside.path()).unwrap().join("x.png");
    fs::write(&image, b"png").unwrap();
    let url = format!("file://{}", image.display());
    // A bare file name's folder is the current directory, which holds
    // neither the image nor the root.
    let doc = Path::new("README.md");
    assert_eq!(
        image_source(&url, doc, Some(&root)),
        ImageSource::Blocked(BlockReason::Outside)
    );
    assert_eq!(
        image_source(&url, doc, None),
        ImageSource::Blocked(BlockReason::Outside)
    );
    let cwd = dunce::canonicalize(std::env::current_dir().unwrap()).unwrap();
    assert_eq!(
        image_source("a.png", doc, Some(&root)),
        ImageSource::Local(cwd.join("a.png"))
    );
}
