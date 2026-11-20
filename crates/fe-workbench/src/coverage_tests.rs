//! The coverage model (`coverage.rs`): lcov and Cobertura read into lines,
//! hostile and unsupported input refused, paths matched under symlinks, the
//! report found in its places, and followed as it changes.

use std::{
    fs,
    io::{self, Cursor, Read},
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};

use fe_project::FsChange;

use crate::coverage::{
    CoverageError, CoverageReport, CoverageSource, Limits, LineCover, MAX_FILES, MAX_GLOB_MATCHES,
    MAX_LINE_RECORDS, MAX_XML_BYTES, POLL_INTERVAL, discover, discover_limited, find_report,
    glob_match, is_foreign_absolute, load_report, normalize, parse_report,
};

/// A project folder, and its canonical path, which reports' paths resolve
/// to.
struct Project {
    _dir: tempfile::TempDir,
    root: PathBuf,
    canonical: PathBuf,
}

fn project() -> Project {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("project");
    fs::create_dir(&root).unwrap();
    let canonical = dunce::canonicalize(&root).unwrap();
    Project {
        _dir: dir,
        root,
        canonical,
    }
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn set_mtime(path: &Path, time: SystemTime) {
    let file = fs::File::options().write(true).open(path).unwrap();
    file.set_modified(time).unwrap();
}

fn parse(text: &str, root: &Path) -> Result<CoverageReport, CoverageError> {
    parse_report(Cursor::new(text.as_bytes()), root, root, &Limits::DEFAULT)
}

fn parse_limited(reader: impl Read, root: &Path, limits: Limits) -> CoverageError {
    parse_report(reader, root, root, &limits).unwrap_err()
}

/// Every mark of a file, by line.
fn marks(report: &CoverageReport, path: &Path) -> Vec<(u32, LineCover)> {
    let file = report.file(path).unwrap_or_else(|| {
        panic!(
            "no {}: {:?}",
            path.display(),
            report.files.keys().collect::<Vec<_>>()
        )
    });
    file.lines
        .iter()
        .map(|(&line, &mark)| (line, mark))
        .collect()
}

#[test]
fn lcov_lines_branches_and_duplicate_sf() {
    let project = project();
    let text = "TN:unit\r\n\
        SF:src/a.rs\r\n\
        FN:1,main\r\n\
        FNDA:1,main\r\n\
        FNF:1\r\n\
        DA:1,3\r\n\
        DA:2,0\r\n\
        DA:3,5,abc123\r\n\
        BRDA:3,0,0,2\r\n\
        BRDA:3,0,1,-\r\n\
        DA:4,1\r\n\
        BRDA:4,0,0,1\r\n\
        BRDA:4,0,1,7\r\n\
        DA:6,1\r\n\
        BRDA:6,0,0,1\r\n\
        BRDA:6,0,1,0\r\n\
        BRDA:9,0,0,0\r\n\
        BRF:7\r\n\
        BRH:4\r\n\
        LF:5\r\n\
        LH:4\r\n\
        XYZ:something new\r\n\
        end_of_record\r\n\
        TN:integration\r\n\
        SF:src/b.rs\r\n\
        DA:10,0\r\n\
        end_of_record\r\n\
        SF:src/a.rs\r\n\
        DA:2,4\r\n\
        DA:7,0\r\n\
        DA:6,2\r\n\
        BRDA:6,0,1,3\r\n\
        BRDA:6,0,0,0\r\n\
        end_of_record\r\n";
    let report = parse(text, &project.root).unwrap();
    assert_eq!(report.files.len(), 2, "{:?}", report.files.keys());
    let a = project.canonical.join("src/a.rs");
    assert_eq!(
        marks(&report, &a),
        vec![
            (1, LineCover::Covered),
            // Uncovered in the first record, covered in the second.
            (2, LineCover::Covered),
            // A branch taken, one never (`-`).
            (3, LineCover::Partial),
            (4, LineCover::Covered),
            // Branch 1 missed in the first record, taken in the second.
            (6, LineCover::Covered),
            (7, LineCover::Uncovered),
        ],
    );
    // A BRDA on a line without DA isn't instrumented.
    assert_eq!(report.mark(&a, 9), None);
    assert_eq!(report.mark(&a, 5), None);
    let b = project.canonical.join("src/b.rs");
    assert_eq!(marks(&report, &b), vec![(10, LineCover::Uncovered)]);

    // A branch counted 0 on a covered line is partial too, and LF line
    // ends, a BOM and blank lines read the same.
    let text = "\u{feff}\nSF:src/c.rs\nDA:1,1\nBRDA:1,0,0,0\nBRDA:1,0,1,1\n\nDA:2,1\nend_of_record";
    let report = parse(text, &project.root).unwrap();
    let c = project.canonical.join("src/c.rs");
    assert_eq!(
        marks(&report, &c),
        vec![(1, LineCover::Partial), (2, LineCover::Covered)]
    );

    // Hits add, without overflowing.
    let text = format!(
        "SF:x.rs\nDA:1,{max}\nend_of_record\nSF:x.rs\nDA:1,{max}\nend_of_record\n",
        max = u64::MAX
    );
    let report = parse(&text, &project.root).unwrap();
    assert_eq!(
        report.mark(&project.canonical.join("x.rs"), 1),
        Some(LineCover::Covered)
    );
    // Line 0, some tools' placeholder, has no mark but is no error.
    let report = parse("SF:x.rs\nDA:0,1\nDA:1,0\nend_of_record\n", &project.root).unwrap();
    assert_eq!(
        marks(&report, &project.canonical.join("x.rs")),
        vec![(1, LineCover::Uncovered)]
    );

    // A report caught halfway through its write, or broken, is refused:
    // a record that doesn't parse, or one never ended.
    let malformed = |line| CoverageError::LcovMalformed { line };
    let refused = [
        ("SF:a\nDA:1,", malformed(2)),
        ("SF:a\nDA:1,1\nDA:x,1\nend_of_record\n", malformed(3)),
        ("SF:a\nDA:1,1\nBRDA:1,0,0\nend_of_record\n", malformed(3)),
        ("SF:a\nBRDA:1,0,0,yes\nend_of_record\n", malformed(2)),
        ("TN:\nDA:1,1\n", malformed(2)),
        ("SF:\nDA:1,1\nend_of_record\n", malformed(1)),
        // A file's record opened before the last one ended.
        ("SF:a\nDA:1,1\nSF:b\nDA:2,0\nend_of_record\n", malformed(3)),
        // Every BRDA field is there.
        ("SF:a\nDA:1,1\nBRDA:1,,0,1\nend_of_record\n", malformed(3)),
        ("SF:a\nDA:1,1\nBRDA:1,0, ,1\nend_of_record\n", malformed(3)),
        ("SF:a\nDA:1,1\nBRDA:1,0,0,\nend_of_record\n", malformed(3)),
        ("SF:a\nDA:1,1\n", CoverageError::Truncated),
        (
            "SF:a\nDA:1,1\nend_of_record\nSF:b\n",
            CoverageError::Truncated,
        ),
    ];
    for (text, expected) in refused {
        let error = parse(text, &project.root).unwrap_err();
        assert_eq!(error, expected, "{text:?}");
        assert!(!error.to_string().is_empty());
    }
}

#[test]
fn cobertura_sources_and_condition_coverage() {
    let project = project();
    let root = &project.root;
    // pkg/mod.py is under the second source (the first doesn't have it)
    // and at the root too: the source wins. top.py is only at the root.
    write(&root.join("lib/pkg/mod.py"), "");
    write(&root.join("pkg/mod.py"), "");
    write(&root.join("top.py"), "");
    let text = format!(
        r#"<?xml version="1.0" ?>
<!-- Generated by coverage.py: https://coverage.readthedocs.io/ -->
<!-- Based on https://raw.githubusercontent.com/cobertura/web/master/htdocs/xml/coverage-04.dtd -->
<coverage version="7.4.0" timestamp="1" lines-valid="9" lines-covered="5" line-rate="0.55" branches-covered="3" branches-valid="6" branch-rate="0.5" complexity="0">
	<sources>
		<source>{root}/empty</source>
		<source>
			{root}/lib
		</source>
	</sources>
	<packages>
		<package name="pkg" line-rate="0.5" branch-rate="0.5" complexity="0">
			<classes>
				<class name="mod.py" filename="pkg/mod.py" complexity="0" line-rate="0.5" branch-rate="0.5">
					<methods>
						<method name="f" signature="()V">
							<lines>
								<line number="3" hits="2" branch="true" condition-coverage="100% (2/2)"/>
								<line number="8" hits="1"/>
							</lines>
						</method>
					</methods>
					<lines>
						<line number="1" hits="1"/>
						<line number="2" hits="0"/>
						<line number="3" hits="2" branch="true" condition-coverage="50% (1/2)" missing-branches="7"/>
						<line number="4" hits="0" branch="true" condition-coverage="0% (0/2)"/>
						<line number="5" hits="1" branch="true" condition-coverage="100% (2/2)"/>
						<line number="6" hits="3" branch="true" condition-coverage="75%"/>
						<line number="7" hits='1' branch='false'/>
					</lines>
				</class>
				<class name="top.py" filename="top.py" complexity="0" line-rate="1" branch-rate="1">
					<methods/>
					<lines>
						<line number="1" hits="4"/>
					</lines>
				</class>
				<class name="esc" filename="caf&#233; &amp; co.py">
					<lines><line number="2" hits="0"/></lines>
				</class>
			</classes>
		</package>
	</packages>
</coverage>
"#,
        root = root.display(),
    );
    let report = parse(&text, root).unwrap();
    let module = project.canonical.join("lib/pkg/mod.py");
    assert_eq!(
        marks(&report, &module),
        vec![
            (1, LineCover::Covered),
            (2, LineCover::Uncovered),
            // The class's 50% wins over its method's copy at 100%.
            (3, LineCover::Partial),
            (4, LineCover::Uncovered),
            (5, LineCover::Covered),
            (6, LineCover::Partial),
            (7, LineCover::Covered),
        ],
    );
    // A line only a method lists isn't the class's: no mark.
    assert_eq!(report.mark(&module, 8), None);
    assert!(report.file(&project.canonical.join("pkg/mod.py")).is_none());
    let top = project.canonical.join("top.py");
    assert_eq!(marks(&report, &top), vec![(1, LineCover::Covered)]);
    // Not on disk anywhere: against the first source.
    let escaped = project.canonical.join("empty/café & co.py");
    assert_eq!(marks(&report, &escaped), vec![(2, LineCover::Uncovered)]);

    // No sources: the root, then the report's folder.
    let text = r#"<coverage><packages><package><classes>
        <class filename="top.py"><lines><line number="9" hits="0"/></lines></class>
        </classes></package></packages></coverage>"#;
    let report = parse(text, root).unwrap();
    assert_eq!(report.mark(&top, 9), Some(LineCover::Uncovered));
    write(&root.join("out/only_here.py"), "");
    let text = r#"<coverage><packages><package><classes>
        <class filename="only_here.py"><lines><line number="1" hits="1"/></lines></class>
        </classes></package></packages></coverage>"#;
    let report =
        parse_report(Cursor::new(text), &root.join("out"), root, &Limits::DEFAULT).unwrap();
    let only = project.canonical.join("out/only_here.py");
    assert_eq!(report.mark(&only, 1), Some(LineCover::Covered));
}

/// An endless reader: `head` once, then `body(n)` for n = 0, 1, 2, ….
struct Endless<F> {
    pending: Vec<u8>,
    next: usize,
    body: F,
    /// Bytes handed out.
    read: usize,
}

impl<F: FnMut(usize) -> Vec<u8>> Endless<F> {
    fn new(head: &str, body: F) -> Self {
        Self {
            pending: head.as_bytes().to_vec(),
            next: 0,
            body,
            read: 0,
        }
    }
}

impl<F: FnMut(usize) -> Vec<u8>> Read for Endless<F> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        while self.pending.is_empty() {
            self.pending = (self.body)(self.next);
            self.next += 1;
        }
        let n = buf.len().min(self.pending.len());
        buf[..n].copy_from_slice(&self.pending[..n]);
        self.pending.drain(..n);
        self.read += n;
        Ok(n)
    }
}

#[test]
fn unsupported_and_hostile_input() {
    let project = project();
    let root = &project.root;
    let unsupported = |text: &str, first_line: &str| {
        let error = parse(text, root).unwrap_err();
        assert_eq!(
            error,
            CoverageError::Unsupported {
                first_line: first_line.to_owned()
            },
            "{text}"
        );
        let message = error.to_string();
        assert!(message.contains("Unsupported coverage report"), "{message}");
        assert!(message.contains(first_line), "{message}");
    };
    // JaCoCo, with its DOCTYPE and without.
    unsupported(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <!DOCTYPE report PUBLIC \"-//JACOCO//DTD Report 1.1//EN\" \"report.dtd\">\n\
         <report name=\"app\"><package name=\"a\"/></report>",
        "<!DOCTYPE report PUBLIC \"-//JACOCO//DTD Report 1.1//EN\" \"report.dtd\">",
    );
    unsupported(
        "<?xml version=\"1.0\"?>\n<report name=\"app\">\n<counter type=\"LINE\"/></report>",
        "<report name=\"app\">",
    );
    // gcov's text.
    unsupported(
        "        -:    0:Source:main.c\n        -:    0:Graph:main.gcno\n        1:    1:int x;\n",
        "-:    0:Source:main.c",
    );
    // Clover, under a `<coverage>` root.
    unsupported(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<coverage generated=\"1\" clover=\"3.2.0\">\n<project/></coverage>",
        "<coverage generated=\"1\" clover=\"3.2.0\">",
    );
    unsupported(
        "<coverage generated=\"1\">\n<project timestamp=\"1\"/></coverage>",
        "<project timestamp=\"1\"/></coverage>",
    );
    // A long first line is cut short.
    let long = format!("{}\n", "x".repeat(500));
    let error = parse(&long, root).unwrap_err();
    let CoverageError::Unsupported { first_line } = error else {
        panic!("{error:?}")
    };
    assert_eq!(first_line.chars().count(), 81, "{first_line}");
    assert_eq!(parse(" \r\n\t\n", root).unwrap_err(), CoverageError::Empty);

    // DOCTYPEs and entities, before the root and inside it.
    let refused = [
        "<?xml version=\"1.0\"?>\n<!DOCTYPE coverage [<!ENTITY lol \"lol\"><!ENTITY lol2 \"&lol;&lol;\">]>\n<coverage>&lol2;</coverage>",
        "<!DOCTYPE coverage SYSTEM \"file:///etc/passwd\"><coverage/>",
        "<coverage><!ENTITY x \"y\"></coverage>",
    ];
    for text in refused {
        let error = parse(text, root).unwrap_err();
        assert_eq!(error, CoverageError::Doctype, "{text}");
        assert!(error.to_string().contains("DOCTYPE"), "{error}");
    }
    let entity = |text: &str, name: &str| {
        let error = parse(text, root).unwrap_err();
        assert_eq!(
            error,
            CoverageError::Entity {
                name: name.to_owned()
            },
            "{text}"
        );
    };
    entity(
        "<coverage><sources><source>&xxe;</source></sources></coverage>",
        "xxe",
    );
    entity(
        "<coverage><packages><package><classes><class filename=\"&lol;.py\"/></classes></package></packages></coverage>",
        "lol",
    );
    entity("<coverage>&amp &lt;</coverage>", "amp");

    // Truncated, mismatched or nested past the cap: malformed.
    let malformed = [
        "<coverage><packages><package>",
        "<coverage><packages></package></coverage>",
        "<coverage><class filename=\"a></coverage>",
    ];
    for text in malformed {
        assert!(
            matches!(parse(text, root), Err(CoverageError::Malformed { .. })),
            "{text}"
        );
    }
    let deep = format!("<coverage>{}", "<a>".repeat(100));
    assert!(matches!(
        parse(&deep, root),
        Err(CoverageError::Malformed { .. })
    ));

    // The caps.
    assert_eq!(MAX_FILES, 200_000);
    assert_eq!(MAX_LINE_RECORDS, 20_000_000);
    assert_eq!(MAX_XML_BYTES, 64 * 1024 * 1024);
    let defaults = Limits::DEFAULT;
    assert_eq!(
        (defaults.files, defaults.line_records, defaults.xml_bytes),
        (MAX_FILES, MAX_LINE_RECORDS, MAX_XML_BYTES),
    );
    let small = Limits {
        files: 3,
        line_records: 10,
        xml_bytes: 4096,
        text_line: 1024,
    };
    // At the caps loads; past them doesn't.
    let at_cap =
        "SF:a\nDA:1,1\nend_of_record\nSF:b\nDA:1,1\nend_of_record\nSF:c\nDA:1,1\nend_of_record\n";
    assert!(parse_report(Cursor::new(at_cap), root, root, &small).is_ok());
    let error = parse_limited(Cursor::new(format!("{at_cap}SF:d\n")), root, small);
    assert_eq!(error, CoverageError::TooManyFiles { limit: 3 });
    assert!(error.to_string().contains("3"), "{error}");
    let lines: String = (1..=11).map(|n| format!("DA:{n},1\n")).collect();
    let error = parse_limited(Cursor::new(format!("SF:a\n{lines}")), root, small);
    assert_eq!(error, CoverageError::TooManyLines { limit: 10 });

    // Endless input stops at the cap, having read only so far past it.
    let mut files = Endless::new("TN:\n", |n| {
        format!("SF:f{n}\nend_of_record\n").into_bytes()
    });
    let error = parse_report(&mut files, root, root, &small).unwrap_err();
    assert_eq!(error, CoverageError::TooManyFiles { limit: 3 });
    assert!(files.read < 128 << 10, "{}", files.read);
    let mut records = Endless::new("SF:a\n", |n| {
        format!("DA:{},1\nBRDA:1,0,{n},1\n", n + 1).into_bytes()
    });
    let error = parse_report(&mut records, root, root, &small).unwrap_err();
    assert_eq!(error, CoverageError::TooManyLines { limit: 10 });
    assert!(records.read < 128 << 10, "{}", records.read);
    let mut long_line = Endless::new("SF:", |_| vec![b'x'; 100]);
    let error = parse_report(&mut long_line, root, root, &small).unwrap_err();
    assert_eq!(
        error,
        CoverageError::LineTooLong {
            line: 1,
            limit: 1024
        }
    );
    assert!(long_line.read < 128 << 10, "{}", long_line.read);
    let mut xml = Endless::new("<coverage>", |_| b"<!-- padding -->".to_vec());
    let error = parse_report(&mut xml, root, root, &small).unwrap_err();
    assert_eq!(error, CoverageError::TooLarge { limit: 4096 });
    assert!(xml.read <= 4097 + (64 << 10), "{}", xml.read);
    let at_size = format!("<coverage>{}</coverage>", " ".repeat(4096 - 21));
    assert_eq!(at_size.len(), 4096);
    assert!(parse_report(Cursor::new(&at_size), root, root, &small).is_ok());
    let past = format!("{at_size} ");
    let error = parse_limited(Cursor::new(past), root, small);
    assert_eq!(error, CoverageError::TooLarge { limit: 4096 });
    let error = CoverageError::TooLarge {
        limit: MAX_XML_BYTES,
    };
    assert!(error.to_string().contains("64 MiB"), "{error}");
    // Cobertura's files and lines count too.
    let classes: String = (0..4)
        .map(|n| {
            format!(
                "<class filename=\"f{n}.py\"><lines><line number=\"1\" hits=\"1\"/></lines></class>"
            )
        })
        .collect();
    let text = format!(
        "<coverage><packages><package><classes>{classes}</classes></package></packages></coverage>"
    );
    assert_eq!(
        parse_limited(Cursor::new(text), root, small),
        CoverageError::TooManyFiles { limit: 3 }
    );
    let many: String = (1..=11)
        .map(|n| format!("<line number=\"{n}\" hits=\"1\"/>"))
        .collect();
    let text = format!(
        "<coverage><packages><package><classes><class filename=\"a.py\"><lines>{many}</lines></class></classes></package></packages></coverage>"
    );
    assert_eq!(
        parse_limited(Cursor::new(text), root, small),
        CoverageError::TooManyLines { limit: 10 }
    );
}

#[test]
fn paths_resolve_under_symlinked_roots() {
    // Windows paths, at the string level: absolute, so never joined to a
    // base on a system that doesn't know them.
    if !cfg!(windows) {
        assert!(is_foreign_absolute(br"C:\work\src\a.rs"));
        assert!(is_foreign_absolute(b"d:/work/a.rs"));
        assert!(is_foreign_absolute(br"\\server\share\a.rs"));
    }
    assert!(!is_foreign_absolute(b"src/a.rs"));
    assert!(!is_foreign_absolute(b"C:"));
    assert!(!is_foreign_absolute(b"1:/x"));
    assert_eq!(
        normalize(Path::new("/a/b/../c/./d")),
        PathBuf::from("/a/c/d")
    );
    assert_eq!(normalize(Path::new("a/../../b")), PathBuf::from("../b"));
    let project = project();
    let report = parse(
        "SF:C:\\work\\src\\a.rs\nDA:1,1\nend_of_record\n",
        &project.root,
    )
    .unwrap();
    if !cfg!(windows) {
        assert_eq!(
            report.mark(Path::new(r"C:\work\src\a.rs"), 1),
            Some(LineCover::Covered)
        );
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        for name in ["a.rs", "b.rs", "c.rs", "d.rs"] {
            write(&real.join("src").join(name), "");
        }
        symlink("a.rs", real.join("src/alias.rs")).unwrap();
        let link = dir.path().join("link");
        symlink(&real, &link).unwrap();
        let canonical = |name: &str| dunce::canonicalize(real.join("src").join(name)).unwrap();
        // The root as opened, through the link.
        let root = &link;
        let lcov = format!(
            "SF:src/a.rs\nDA:1,1\nend_of_record\n\
             SF:{link}/src/b.rs\nDA:2,0\nend_of_record\n\
             SF:{real}/src/c.rs\nDA:3,1\nend_of_record\n\
             SF:./src/../src/alias.rs\nDA:4,0\nend_of_record\n\
             SF:../outside/x.rs\nDA:5,1\nend_of_record\n\
             SF:/elsewhere/y.rs\nDA:6,1\nend_of_record\n",
            link = link.display(),
            real = real.display(),
        );
        write(&link.join("coverage/lcov.info"), &lcov);
        let report = load_report(&link.join("coverage/lcov.info"), root).unwrap();
        assert_eq!(report.mark(&canonical("a.rs"), 1), Some(LineCover::Covered));
        assert_eq!(
            report.mark(&canonical("b.rs"), 2),
            Some(LineCover::Uncovered)
        );
        assert_eq!(report.mark(&canonical("c.rs"), 3), Some(LineCover::Covered));
        // A symlinked file is its target's.
        assert_eq!(
            report.mark(&canonical("a.rs"), 4),
            Some(LineCover::Uncovered)
        );
        // Outside the root: kept, normalised, never canonicalised.
        let outside = normalize(&link.join("../outside/x.rs"));
        assert_eq!(outside, dir.path().join("outside/x.rs"));
        assert_eq!(report.mark(&outside, 5), Some(LineCover::Covered));
        assert_eq!(
            report.mark(Path::new("/elsewhere/y.rs"), 6),
            Some(LineCover::Covered)
        );
        assert!(report.modified.is_some());

        // Cobertura: a source through the link, a filename relative to it,
        // and absolute ones both ways.
        let xml = format!(
            r#"<?xml version="1.0"?>
<coverage line-rate="1"><sources><source>{link}/src</source></sources><packages><package name="p"><classes>
<class filename="a.rs"><lines><line number="1" hits="1"/></lines></class>
<class filename="{link}/src/b.rs"><lines><line number="2" hits="0"/></lines></class>
<class filename="{real}/src/c.rs"><lines><line number="3" hits="1" branch="true" condition-coverage="50% (1/2)"/></lines></class>
<class filename="src/d.rs"><lines><line number="4" hits="1"/></lines></class>
</classes></package></packages></coverage>"#,
            link = link.display(),
            real = real.display(),
        );
        write(&link.join("coverage.xml"), &xml);
        let report = load_report(&link.join("coverage.xml"), root).unwrap();
        assert_eq!(report.mark(&canonical("a.rs"), 1), Some(LineCover::Covered));
        assert_eq!(
            report.mark(&canonical("b.rs"), 2),
            Some(LineCover::Uncovered)
        );
        assert_eq!(report.mark(&canonical("c.rs"), 3), Some(LineCover::Partial));
        // Not under the source: the root's.
        assert_eq!(report.mark(&canonical("d.rs"), 4), Some(LineCover::Covered));
        assert_eq!(report.files.len(), 4, "{:?}", report.files.keys());
    }
}

#[cfg(unix)]
#[test]
fn a_report_through_a_symlinked_ancestor_of_the_root_resolves_inside() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let t = dir.path();
    write(&t.join("r/real/src/c.rs"), "");
    write(&t.join("r/elsewhere.rs"), "");
    symlink(t.join("r"), t.join("alias")).unwrap();
    symlink(t.join("r/real"), t.join("r/link")).unwrap();
    let root = t.join("r/link");
    let canonical = dunce::canonicalize(t.join("r/real/src/c.rs")).unwrap();
    let alias = t.join("alias");

    let lcov = format!(
        "SF:{a}/real/src/c.rs\nDA:3,1\nend_of_record\n\
         SF:{a}/elsewhere.rs\nDA:1,1\nend_of_record\n",
        a = alias.display()
    );
    write(&root.join("lcov.info"), &lcov);
    let report = load_report(&root.join("lcov.info"), &root).unwrap();
    assert_eq!(report.mark(&canonical, 3), Some(LineCover::Covered));
    // Exists, resolves outside the root: kept as spelled, not canonical.
    let outside = alias.join("elsewhere.rs");
    assert_eq!(report.mark(&outside, 1), Some(LineCover::Covered));
    assert_eq!(report.mark(&t.join("r/elsewhere.rs"), 1), None);

    let xml = format!(
        r#"<?xml version="1.0"?>
<coverage line-rate="1"><packages><package name="p"><classes>
<class filename="{a}/real/src/c.rs"><lines><line number="3" hits="1"/></lines></class>
</classes></package></packages></coverage>"#,
        a = alias.display()
    );
    write(&root.join("coverage.xml"), &xml);
    let report = load_report(&root.join("coverage.xml"), &root).unwrap();
    assert_eq!(report.mark(&canonical, 3), Some(LineCover::Covered));
}

#[test]
fn discovery_order_and_setting() {
    let project = project();
    let root = &project.root;
    let base = SystemTime::now() - Duration::from_secs(3600);
    let at = |path: &str, age: u64| {
        let path = root.join(path);
        write(&path, "SF:a\nend_of_record\n");
        set_mtime(&path, base + Duration::from_secs(age));
        path
    };
    assert_eq!(discover(root, None), Err(CoverageError::NoReport));
    assert_eq!(find_report(root, None), None);

    // Not in a place searched: the tree isn't walked.
    at("sub/deep/lcov.info", 0);
    at("htmlcov/index.html", 0);
    at("src/coverage.xml", 0);
    assert_eq!(discover(root, None), Err(CoverageError::NoReport));

    // Only coverage.py's database.
    at(".coverage", 0);
    let error = discover(root, None).unwrap_err();
    assert_eq!(error, CoverageError::CoverageDatabase);
    assert!(
        error
            .to_string()
            .contains("Run coverage xml or coverage lcov"),
        "{error}"
    );

    // Later folders give way to earlier ones, however new.
    let target_coverage = at("target/coverage/lcov.info", 50);
    assert_eq!(find_report(root, None), Some(target_coverage));
    let llvm_cov_target = at("target/llvm-cov-target/lcov.info", 40);
    assert_eq!(find_report(root, None), Some(llvm_cov_target));
    let llvm_cov = at("target/llvm-cov/lcov.info", 30);
    assert_eq!(find_report(root, None), Some(llvm_cov));
    // In one folder, the newest.
    let coverage_lcov = at("coverage/lcov.info", 10);
    assert_eq!(find_report(root, None), Some(coverage_lcov.clone()));
    let cobertura = at("coverage/cobertura-coverage.xml", 20);
    assert_eq!(find_report(root, None), Some(cobertura));
    let coverage_xml = at("coverage/coverage.xml", 25);
    assert_eq!(find_report(root, None), Some(coverage_xml));
    let root_lcov = at("lcov.info", 5);
    assert_eq!(find_report(root, None), Some(root_lcov.clone()));
    let root_xml = at("coverage.xml", 1);
    assert_eq!(find_report(root, None), Some(root_lcov.clone()));
    set_mtime(&root_xml, base + Duration::from_secs(6));
    assert_eq!(find_report(root, None), Some(root_xml.clone()));
    // Ignored by git is still found.
    write(&root.join(".gitignore"), "coverage.xml\nlcov.info\n");
    assert_eq!(find_report(root, None), Some(root_xml));

    // The setting wins: a path, relative to the root or absolute in it.
    let mine = at("reports/mine.info", 0);
    assert_eq!(
        find_report(root, Some(Path::new("reports/mine.info"))),
        Some(mine.clone())
    );
    assert_eq!(find_report(root, Some(&mine)), Some(mine.clone()));
    let canonical_mine = project.canonical.join("reports/mine.info");
    assert!(find_report(root, Some(&canonical_mine)).is_some());
    // A glob: the newest match, wildcards in folders too.
    let theirs = at("reports/theirs.info", 2);
    at("reports/theirs.txt", 9);
    assert_eq!(
        find_report(root, Some(Path::new("reports/*.info"))),
        Some(theirs)
    );
    let run_b = at("out-b/lcov.info", 3);
    at("out-a/lcov.info", 1);
    at("other/lcov.info", 8);
    assert_eq!(
        find_report(root, Some(Path::new("out-*/lcov.info"))),
        Some(run_b.clone())
    );
    assert_eq!(
        find_report(root, Some(Path::new("out-[ab]/lcov.?nfo"))),
        Some(run_b)
    );
    // Every entry of a folder is looked at, however many; too many
    // matches fail rather than pick from some.
    for n in 0..50 {
        write(&root.join(format!("many/other-{n:02}.txt")), "");
    }
    let last = at("many/zz-last.info", 0);
    let pattern = Path::new("many/*.info");
    assert_eq!(discover_limited(root, Some(pattern), 2), Ok(last));
    at("many/a.info", 0);
    at("many/b.info", 0);
    assert_eq!(
        discover_limited(root, Some(pattern), 2),
        Err(CoverageError::SettingTooBroad {
            setting: "many/*.info".to_owned(),
            limit: 2
        }),
    );
    assert!(discover_limited(root, Some(pattern), 3).is_ok());
    assert_eq!(MAX_GLOB_MATCHES, 10_000);
    // A setting that matches nothing doesn't fall back.
    let error = discover(root, Some(Path::new("nothing/*.info"))).unwrap_err();
    assert_eq!(
        error,
        CoverageError::SettingNoMatch {
            setting: "nothing/*.info".to_owned()
        }
    );
    assert!(error.to_string().contains("nothing/*.info"), "{error}");
    assert!(matches!(
        discover(root, Some(Path::new("reports"))),
        Err(CoverageError::SettingNoMatch { .. })
    ));
    // Nothing outside the root is read.
    let outside = project._dir.path().join("outside.info");
    write(&outside, "SF:a\n");
    assert_eq!(
        discover(root, Some(&outside)),
        Err(CoverageError::SettingOutsideRoot {
            setting: outside.display().to_string()
        }),
    );
    assert!(matches!(
        discover(root, Some(Path::new("../outside.info"))),
        Err(CoverageError::SettingOutsideRoot { .. })
    ));
    #[cfg(unix)]
    {
        // A report symlinked from outside isn't followed.
        let fresh = project_with_link(&outside);
        assert_eq!(discover(&fresh.root, None), Err(CoverageError::NoReport));
    }

    let glob = |pattern: &str, name: &str| {
        glob_match(
            &pattern.chars().collect::<Vec<_>>(),
            &name.chars().collect::<Vec<_>>(),
        )
    };
    assert!(glob("*.info", "lcov.info"));
    assert!(glob("*", ""));
    assert!(glob("a*b*c", "aXbYbZc"));
    assert!(!glob("a*b*c", "aXbYbZ"));
    assert!(glob("[!x]?", "ab"));
    assert!(!glob("[!a]?", "ab"));
    assert!(glob("[a-c]", "b"));
    assert!(glob("[]]", "]"));
    assert!(glob("[", "["));
    assert!(!glob("?", ""));
}

/// A fresh project whose `lcov.info` links to `target`.
#[cfg(unix)]
fn project_with_link(target: &Path) -> Project {
    let fresh = project();
    std::os::unix::fs::symlink(target, fresh.root.join("lcov.info")).unwrap();
    fresh
}

#[test]
fn reload_on_change_keeps_old_report_on_bad_parse() {
    let project = project();
    let root = &project.root;
    let report_path = root.join("lcov.info");
    let source_file = project.canonical.join("src/a.rs");
    let written = SystemTime::now() - Duration::from_secs(600);
    write(&report_path, "SF:src/a.rs\nDA:1,1\nDA:2,0\nend_of_record\n");
    set_mtime(&report_path, written);

    let mut source = CoverageSource::new(root.clone(), None);
    let t0 = Instant::now();
    assert!(!source.poll(t0));
    assert!(source.is_loading());
    assert_eq!(
        (
            source.test.discoveries,
            source.test.stats,
            source.test.loads
        ),
        (1, 1, 1)
    );
    assert_eq!(source.report_path(), Some(report_path.as_path()));
    assert!(source.wait_for_load());
    let parsed_on = source.test.loaded_on.unwrap();
    assert_ne!(
        parsed_on,
        std::thread::current().id(),
        "parsed on the main thread"
    );
    let mark = |source: &CoverageSource, line| source.report().unwrap().mark(&source_file, line);
    assert_eq!(mark(&source, 1), Some(LineCover::Covered));
    assert_eq!(source.error(), None);

    // Not due: nothing is read. Due, but unchanged: one stat, no load.
    assert!(!source.poll(t0 + POLL_INTERVAL / 2));
    assert_eq!((source.test.stats, source.test.loads), (1, 1));
    assert!(!source.poll(t0 + POLL_INTERVAL));
    assert_eq!((source.test.stats, source.test.loads), (2, 1));

    // A bad write: the old report stays, with the error.
    fs::write(&report_path, "garbage, not lcov\n").unwrap();
    set_mtime(&report_path, written + Duration::from_secs(10));
    source.poll(t0 + POLL_INTERVAL * 2);
    assert_eq!(source.test.loads, 2);
    assert!(source.wait_for_load());
    assert_eq!(
        source.error(),
        Some(&CoverageError::Unsupported {
            first_line: "garbage, not lcov".to_owned()
        })
    );
    assert_eq!(mark(&source, 1), Some(LineCover::Covered));
    // The failed file isn't parsed again until it changes.
    source.poll(t0 + POLL_INTERVAL * 3);
    assert_eq!(source.test.loads, 2);

    // Caught halfway through a write: refused, the old report kept.
    for (n, (partial, expected)) in [
        (
            "SF:src/a.rs\nDA:1,0\nDA:2,",
            CoverageError::LcovMalformed { line: 3 },
        ),
        ("SF:src/a.rs\nDA:1,0\n", CoverageError::Truncated),
        (
            "SF:src/a.rs\nDA:1,0\nSF:src/b.rs\nDA:2,0\nend_of_record\n",
            CoverageError::LcovMalformed { line: 3 },
        ),
        (
            "SF:src/a.rs\nDA:1,0\nBRDA:1,,0,1\nend_of_record\n",
            CoverageError::LcovMalformed { line: 3 },
        ),
    ]
    .into_iter()
    .enumerate()
    {
        fs::write(&report_path, partial).unwrap();
        set_mtime(&report_path, written + Duration::from_secs(5 + n as u64));
        source.poll(t0 + POLL_INTERVAL * (4 + n as u32));
        assert!(source.wait_for_load());
        assert_eq!(source.error(), Some(&expected));
        assert_eq!(mark(&source, 1), Some(LineCover::Covered));
    }

    // Only the length changes: a reload, which lands.
    fs::write(
        &report_path,
        "SF:src/a.rs\nDA:1,0\nDA:2,0\nDA:3,4\nend_of_record\n",
    )
    .unwrap();
    set_mtime(&report_path, written + Duration::from_secs(10));
    source.poll(t0 + POLL_INTERVAL * 10);
    assert_eq!(source.test.loads, 7);
    assert!(source.wait_for_load());
    assert_eq!(source.error(), None);
    assert_eq!(mark(&source, 1), Some(LineCover::Uncovered));
    assert_eq!(mark(&source, 3), Some(LineCover::Covered));

    // Stale only for a file saved after the report was written.
    let report_time = written + Duration::from_secs(10);
    let state = |modified| {
        source
            .file_state(&source_file, Some(modified))
            .unwrap()
            .stale
    };
    assert!(state(report_time + Duration::from_secs(1)));
    assert!(!state(report_time));
    assert!(!state(report_time - Duration::from_secs(1)));
    assert!(!source.file_state(&source_file, None).unwrap().stale);

    // With a live watcher, only the watcher's word is acted on.
    source.set_watching(true);
    let stats = source.test.stats;
    assert!(!source.poll(t0 + POLL_INTERVAL * 100));
    source.fs_changed(&[FsChange::Path(root.join("src/a.rs"))]);
    source.poll(t0 + POLL_INTERVAL * 101);
    assert_eq!(source.test.stats, stats);
    fs::write(&report_path, "SF:src/a.rs\nDA:1,9\nend_of_record\n").unwrap();
    source.fs_changed(&[FsChange::Path(report_path.clone())]);
    source.poll(t0 + POLL_INTERVAL * 101);
    assert_eq!(source.test.stats, stats + 1);
    assert!(source.wait_for_load());
    assert_eq!(mark(&source, 1), Some(LineCover::Covered));

    // A newer report in an earlier place, heard of: followed.
    let first = root.join("coverage.xml");
    write(
        &first,
        r#"<coverage><packages><package><classes><class filename="src/a.rs"><lines><line number="7" hits="0"/></lines></class></classes></package></packages></coverage>"#,
    );
    set_mtime(&first, SystemTime::now() + Duration::from_secs(60));
    source.fs_changed(&[FsChange::Path(first.clone())]);
    source.poll(t0 + POLL_INTERVAL * 102);
    assert_eq!(source.report_path(), Some(first.as_path()));
    assert!(source.wait_for_load());
    assert_eq!(mark(&source, 7), Some(LineCover::Uncovered));

    // The followed report turned older than another in its folder: the
    // watcher's word on it looks again, and the other is followed.
    set_mtime(&first, SystemTime::now() - Duration::from_secs(3600));
    source.fs_changed(&[FsChange::Path(first.clone())]);
    source.poll(t0 + POLL_INTERVAL * 102);
    assert_eq!(source.report_path(), Some(report_path.as_path()));
    assert!(source.wait_for_load());
    assert_eq!(mark(&source, 1), Some(LineCover::Covered));
    assert_eq!(mark(&source, 7), None);

    // Gone: its marks go, and the error says why.
    fs::remove_file(&first).unwrap();
    fs::remove_file(&report_path).unwrap();
    source.fs_changed(&[FsChange::Path(first)]);
    assert!(source.poll(t0 + POLL_INTERVAL * 103));
    source.poll(t0 + POLL_INTERVAL * 103);
    assert!(source.report().is_none());
    assert_eq!(source.error(), Some(&CoverageError::NoReport));
}

#[test]
fn reload_follows_setting_changes_and_moved_folders() {
    let project = project();
    let root = &project.root;
    let source_file = project.canonical.join("src/a.rs");
    let t0 = Instant::now();

    // A setting changed while the old report loads: the old load's answer
    // never lands, so a failed new one leaves no stale marks.
    write(
        &root.join("lcov.info"),
        "SF:src/a.rs\nDA:1,1\nend_of_record\n",
    );
    write(&root.join("bad.info"), "not a report\n");
    let mut source = CoverageSource::new(root.clone(), None);
    source.poll(t0);
    assert!(source.is_loading());
    source.set_setting(Some(PathBuf::from("bad.info")));
    assert!(!source.is_loading());
    source.poll(t0);
    assert_eq!(source.report_path(), Some(root.join("bad.info").as_path()));
    assert!(source.wait_for_load());
    std::thread::sleep(Duration::from_millis(50));
    assert!(!source.poll(t0));
    assert!(source.report().is_none());
    assert_eq!(
        source.error(),
        Some(&CoverageError::Unsupported {
            first_line: "not a report".to_owned()
        })
    );
    // And back: the first report loads again.
    source.set_setting(None);
    source.poll(t0);
    assert!(source.wait_for_load());
    assert_eq!(
        source.report().unwrap().mark(&source_file, 1),
        Some(LineCover::Covered)
    );
    fs::remove_file(root.join("lcov.info")).unwrap();

    // A folder of reports moved in whole: the watcher names only the
    // folder, and that's enough to find it.
    let mut source = CoverageSource::new(root.clone(), None);
    source.set_watching(true);
    source.poll(t0);
    assert_eq!(source.error(), Some(&CoverageError::NoReport));
    write(
        &root.join("stage/lcov.info"),
        "SF:src/a.rs\nDA:2,0\nend_of_record\n",
    );
    fs::rename(root.join("stage"), root.join("coverage")).unwrap();
    // Unrelated folders and files don't look again.
    let discoveries = source.test.discoveries;
    source.fs_changed(&[
        FsChange::Path(root.join("src")),
        FsChange::Path(root.join("target/debug/build")),
        FsChange::Path(root.join("coverage/index.html")),
    ]);
    source.poll(t0);
    assert_eq!(source.test.discoveries, discoveries);
    source.fs_changed(&[FsChange::Path(root.join("coverage"))]);
    source.poll(t0);
    assert_eq!(
        source.report_path(),
        Some(root.join("coverage/lcov.info").as_path())
    );
    assert!(source.wait_for_load());
    assert_eq!(
        source.report().unwrap().mark(&source_file, 2),
        Some(LineCover::Uncovered)
    );

    // The same under a glob setting's folders, named by the canonical root.
    let mut source = CoverageSource::new(root.clone(), Some(PathBuf::from("runs/*/lcov.info")));
    source.set_watching(true);
    source.poll(t0);
    assert!(matches!(
        source.error(),
        Some(CoverageError::SettingNoMatch { .. })
    ));
    write(
        &root.join("stage/lcov.info"),
        "SF:src/a.rs\nDA:3,1\nend_of_record\n",
    );
    fs::create_dir(root.join("runs")).unwrap();
    fs::rename(root.join("stage"), root.join("runs/r1")).unwrap();
    let discoveries = source.test.discoveries;
    source.fs_changed(&[FsChange::Path(root.join("coverage/lcov.info.tmp"))]);
    source.poll(t0);
    assert_eq!(source.test.discoveries, discoveries);
    source.fs_changed(&[FsChange::Path(project.canonical.join("runs/r1"))]);
    source.poll(t0);
    assert_eq!(
        source.report_path(),
        Some(root.join("runs/r1/lcov.info").as_path())
    );
    assert!(source.wait_for_load());
    assert_eq!(
        source.report().unwrap().mark(&source_file, 3),
        Some(LineCover::Covered)
    );

    // Watchers name paths through symlinks resolved: a searched folder
    // linked to another in the project is watched at its target.
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let project = self::project();
        let root = &project.root;
        let canonical = &project.canonical;
        let source_file = canonical.join("src/a.rs");
        let base = SystemTime::now() - Duration::from_secs(600);
        let report = |path: &Path, line: u32, age: u64| {
            write(path, &format!("SF:src/a.rs\nDA:{line},1\nend_of_record\n"));
            set_mtime(path, base + Duration::from_secs(age));
        };
        fs::create_dir(root.join("actual")).unwrap();
        symlink("actual", root.join("coverage")).unwrap();
        report(&root.join("actual/lcov.info"), 1, 0);
        let mut source = CoverageSource::new(root.clone(), None);
        source.set_watching(true);
        source.poll(t0);
        assert_eq!(
            source.report_path(),
            Some(root.join("coverage/lcov.info").as_path())
        );
        assert!(source.wait_for_load());
        // Created, named at the link's target.
        let xml = "<coverage><packages><package><classes><class filename=\"src/a.rs\"><lines><line number=\"2\" hits=\"0\"/></lines></class></classes></package></packages></coverage>";
        write(&root.join("actual/coverage.xml"), xml);
        set_mtime(
            &root.join("actual/coverage.xml"),
            base + Duration::from_secs(10),
        );
        source.fs_changed(&[FsChange::Path(canonical.join("actual/coverage.xml"))]);
        source.poll(t0);
        assert_eq!(
            source.report_path(),
            Some(root.join("coverage/coverage.xml").as_path())
        );
        assert!(source.wait_for_load());
        assert_eq!(
            source.report().unwrap().mark(&source_file, 2),
            Some(LineCover::Uncovered)
        );
        // Reordered, named at the link's target.
        set_mtime(
            &root.join("actual/lcov.info"),
            base + Duration::from_secs(20),
        );
        source.fs_changed(&[FsChange::Path(canonical.join("actual/lcov.info"))]);
        source.poll(t0);
        assert_eq!(
            source.report_path(),
            Some(root.join("coverage/lcov.info").as_path())
        );
        assert!(source.wait_for_load());
        assert_eq!(
            source.report().unwrap().mark(&source_file, 1),
            Some(LineCover::Covered)
        );
        // The link's target folder itself replaced.
        fs::remove_file(root.join("actual/lcov.info")).unwrap();
        fs::remove_file(root.join("actual/coverage.xml")).unwrap();
        source.fs_changed(&[FsChange::Path(canonical.join("actual"))]);
        source.poll(t0);
        assert_eq!(source.report_path(), None);

        // A glob setting: a matched folder linked elsewhere in the project.
        let mut source = CoverageSource::new(root.clone(), Some(PathBuf::from("runs/*/lcov.info")));
        source.set_watching(true);
        fs::create_dir_all(root.join("data/r1")).unwrap();
        fs::create_dir_all(root.join("data/r2")).unwrap();
        fs::create_dir(root.join("runs")).unwrap();
        symlink("../data/r1", root.join("runs/r1")).unwrap();
        symlink("../data/r2", root.join("runs/r2")).unwrap();
        source.poll(t0);
        assert!(matches!(
            source.error(),
            Some(CoverageError::SettingNoMatch { .. })
        ));
        report(&root.join("data/r1/lcov.info"), 4, 0);
        source.fs_changed(&[FsChange::Path(canonical.join("data/r1/lcov.info"))]);
        source.poll(t0);
        assert_eq!(
            source.report_path(),
            Some(root.join("runs/r1/lcov.info").as_path())
        );
        assert!(source.wait_for_load());
        report(&root.join("data/r2/lcov.info"), 5, 10);
        source.fs_changed(&[FsChange::Path(canonical.join("data/r2/lcov.info"))]);
        source.poll(t0);
        assert_eq!(
            source.report_path(),
            Some(root.join("runs/r2/lcov.info").as_path())
        );
        assert!(source.wait_for_load());
        assert_eq!(
            source.report().unwrap().mark(&source_file, 5),
            Some(LineCover::Covered)
        );
    }
}
