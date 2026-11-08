//! NOTICE lists every source THIRD_PARTY.md's "Code copied or adapted",
//! "Icons" and "Fonts" tables name, and each license those rows carry has a
//! text to ship.
//!
//! The checks are functions over `(path, text)` pairs, run over the
//! repository and over in-memory fixtures.

use std::collections::BTreeMap;
use std::path::Path;

/// Repository path (with `/`) to the file's text.
type Files = BTreeMap<String, String>;

/// A THIRD_PARTY.md, the NOTICE beside it, and the directory they are in
/// ("" or "kiln/").
struct Pair<'a> {
    prefix: &'a str,
}

/// Lucide's license, beside the icons, for the Icons table's rows.
const ISC_TEXT: &str = "kiln/crates/kiln-ui/assets/icons/LICENSE";
/// Tabler Icons' license, beside its icons.
const TABLER_TEXT: &str = "kiln/crates/kiln-ui/assets/tabler-icons/LICENSE";
/// Material Icon Theme's license, beside its icons.
const MATERIAL_TEXT: &str = "kiln/crates/kiln-ui/assets/file-icons/LICENSE";

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/fastedit is two levels below the repository root")
}

/// One row of a checked table.
struct Row {
    table: String,
    path: String,
    source: String,
    license: String,
}

/// Splits a markdown table row into trimmed cells.
fn cells(line: &str) -> Vec<&str> {
    line.trim()
        .trim_matches('|')
        .split('|')
        .map(str::trim)
        .collect()
}

/// The rows of the "Code copied or adapted…", "Icons" and "Fonts" tables.
fn rows(md: &str) -> Vec<Row> {
    let mut out = Vec::new();
    let mut table = None;
    let mut seen_header = false;
    for line in md.lines() {
        if let Some(h) = line.strip_prefix("## ") {
            table = (h.starts_with("Code copied or adapted") || h == "Icons" || h == "Fonts")
                .then(|| h.to_string());
            seen_header = false;
            continue;
        }
        let Some(table) = &table else { continue };
        if !line.trim_start().starts_with('|') {
            continue;
        }
        let c = cells(line);
        if !seen_header {
            // The header row, then the `---` separator.
            if c.first() == Some(&"Our path") {
                seen_header = true;
            }
            continue;
        }
        if c.len() < 4 || c[0].starts_with("---") {
            continue;
        }
        // Source is column 1; License is column 3 in all three tables.
        out.push(Row {
            table: table.clone(),
            path: c[0].trim_matches('`').to_string(),
            source: c[1].to_string(),
            license: c[3].to_string(),
        });
    }
    out
}

/// The Source cell's first markdown link's text, else its first word.
fn source_name(cell: &str) -> String {
    if cell.starts_with('[')
        && let Some(end) = cell.find("](")
    {
        return cell[1..end].to_string();
    }
    cell.split_whitespace().next().unwrap_or("").to_string()
}

/// Names a NOTICE may use for a source name, lower-cased.
fn aliases(name: &str) -> Vec<String> {
    let name = name.to_lowercase();
    let mut v = vec![name.clone()];
    if name == "lucide-icons/lucide" {
        v.push("lucide".into());
    }
    v
}

/// The license identifiers in a License cell.
fn license_ids(cell: &str) -> Vec<&'static str> {
    ["Apache-2.0", "MIT", "ISC", "OFL-1.1"]
        .into_iter()
        .filter(|id| cell.contains(id))
        .collect()
}

fn check_pair(files: &Files, pair: &Pair) -> Vec<String> {
    let mut failures = Vec::new();
    let tp = format!("{}THIRD_PARTY.md", pair.prefix);
    let notice_path = format!("{}NOTICE", pair.prefix);
    let Some(md) = files.get(&tp) else {
        return vec![format!("{tp}: missing")];
    };
    let notice = files.get(&notice_path).map(|t| t.to_lowercase());
    for row in rows(md) {
        let name = source_name(&row.source);
        let named = notice
            .as_ref()
            .is_some_and(|n| aliases(&name).iter().any(|a| n.contains(a)));
        if !named {
            failures.push(format!(
                "{tp}, \"{}\" table, row {}: {notice_path} doesn't name the source `{name}` \
                 (rule: every copied source is in NOTICE)",
                row.table, row.path
            ));
        }
        for id in license_ids(&row.license) {
            // An icon set's LICENSE beside it carries its notices: Lucide's
            // ISC and Feather's MIT, Tabler's MIT, Material Icon Theme's MIT.
            let text = if row.table == "Icons" && id != "Apache-2.0" {
                format!("{}{}LICENSE", pair.prefix, row.path)
            } else if row.table == "Fonts" && id == "OFL-1.1" {
                // A font family's OFL.txt beside its files, as Google Fonts
                // ships it.
                format!("{}{}OFL.txt", pair.prefix, row.path)
            } else {
                format!("{}LICENSES/{id}.txt", pair.prefix)
            };
            if files.get(&text).is_none_or(|t| t.trim().is_empty()) {
                failures.push(format!(
                    "{tp}, \"{}\" table, row {}: no license text `{text}` for {id} \
                     (rule: every license a row names ships its text)",
                    row.table, row.path
                ));
            }
        }
    }
    failures
}

fn check_all(files: &Files) -> Vec<String> {
    [Pair { prefix: "" }, Pair { prefix: "kiln/" }]
        .iter()
        .flat_map(|p| check_pair(files, p))
        .collect()
}

fn repo_files() -> Files {
    let root = repo_root();
    let mut files = Files::new();
    for p in [
        "THIRD_PARTY.md",
        "NOTICE",
        "kiln/THIRD_PARTY.md",
        "kiln/NOTICE",
        "LICENSES/Apache-2.0.txt",
        "LICENSES/MIT.txt",
        "kiln/LICENSES/Apache-2.0.txt",
        ISC_TEXT,
        TABLER_TEXT,
        MATERIAL_TEXT,
    ] {
        match std::fs::read_to_string(root.join(p)) {
            Ok(text) => {
                files.insert(p.to_string(), text);
            }
            // A missing text is a finding; a manifest we can't read is not.
            Err(e) if p.ends_with(".md") || p.ends_with("NOTICE") => {
                panic!("{p}: can't read it: {e}")
            }
            Err(_) => {}
        }
    }
    // Each font family's OFL.txt, beside the files its row names.
    let fonts: Vec<String> = ["", "kiln/"]
        .iter()
        .flat_map(|prefix| {
            let md = &files[&format!("{prefix}THIRD_PARTY.md")];
            rows(md)
                .into_iter()
                .filter(|row| row.table == "Fonts")
                .map(move |row| format!("{prefix}{}OFL.txt", row.path))
        })
        .collect();
    for p in fonts {
        if let Ok(text) = std::fs::read_to_string(root.join(&p)) {
            files.insert(p, text);
        }
    }
    files
}

fn assert_clean(failures: Vec<String>) {
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

#[test]
fn sources_are_in_notice() {
    let files = repo_files();
    let failures: Vec<String> = check_all(&files)
        .into_iter()
        .filter(|f| !f.contains("no license text"))
        .collect();
    assert_clean(failures);
}

#[test]
fn license_texts_ship() {
    let files = repo_files();
    let failures: Vec<String> = check_all(&files)
        .into_iter()
        .filter(|f| !f.contains("doesn't name"))
        .collect();
    assert_clean(failures);
}

fn fixture(notice: &str, license: &str, texts: &[&str]) -> Files {
    let md = format!(
        "## Code copied or adapted\n\n| Our path | Source | Commit | License |\n|---|---|---|---|\n\
         | `a/b.rs` | [foo](https://x) `f.rs` | abc | {license} |\n"
    );
    let mut f = Files::new();
    f.insert("THIRD_PARTY.md".into(), md);
    f.insert("NOTICE".into(), notice.into());
    f.insert("kiln/THIRD_PARTY.md".into(), String::new());
    for t in texts {
        f.insert(format!("LICENSES/{t}.txt"), "text".into());
    }
    f
}

#[test]
fn checks_catch_gaps() {
    let pair = Pair { prefix: "" };

    let ok = fixture("Foo\n", "Apache-2.0", &["Apache-2.0"]);
    assert_eq!(check_pair(&ok, &pair), Vec::<String>::new());

    let unnamed = check_pair(&fixture("Bar\n", "MIT", &["MIT"]), &pair);
    assert_eq!(unnamed.len(), 1, "{unnamed:?}");
    for part in ["Code copied or adapted", "a/b.rs", "doesn't name", "NOTICE"] {
        assert!(unnamed[0].contains(part), "{part}: {unnamed:?}");
    }

    let no_text = check_pair(&fixture("foo\n", "MIT", &[]), &pair);
    assert_eq!(no_text.len(), 1, "{no_text:?}");
    for part in [
        "Code copied or adapted",
        "a/b.rs",
        "no license text",
        "LICENSES/MIT.txt",
    ] {
        assert!(no_text[0].contains(part), "{part}: {no_text:?}");
    }

    let multi = check_pair(
        &fixture("foo\n", "Apache-2.0 (some files: MIT)", &["Apache-2.0"]),
        &pair,
    );
    assert_eq!(multi.len(), 1, "{multi:?}");
    assert!(multi[0].contains("LICENSES/MIT.txt") && multi[0].contains("a/b.rs"));

    let missing_manifest = check_pair(&Files::new(), &pair);
    assert_eq!(missing_manifest.len(), 1, "{missing_manifest:?}");
    assert!(missing_manifest[0].contains("THIRD_PARTY.md: missing"));

    // Lucide's LICENSE covers ISC and MIT in the Icons table, not Apache-2.0.
    let md = "## Icons\n\n| Our path | Source | Tag | License | Files |\n|---|---|---|---|---|\n\
              | `icons/` | [lucide-icons/lucide](https://x) | 1 | ISC (some: Apache-2.0) | `a.svg` |\n";
    let mut icons = Files::new();
    icons.insert("THIRD_PARTY.md".into(), md.into());
    icons.insert("NOTICE".into(), "Lucide\n".into());
    icons.insert("icons/LICENSE".into(), "text".into());
    let icons = check_pair(&icons, &pair);
    assert_eq!(icons.len(), 1, "{icons:?}");
    for part in [
        "Icons",
        "icons/",
        "no license text",
        "LICENSES/Apache-2.0.txt",
    ] {
        assert!(icons[0].contains(part), "{part}: {icons:?}");
    }
}

#[test]
fn font_rows_need_their_ofl_txt() {
    let pair = Pair { prefix: "" };
    let md = "## Fonts\n\n| Our path | Source | Commit | License | Files |\n|---|---|---|---|---|\n\
              | `fonts/sans/` | [Some Sans](https://x) | abc | OFL-1.1 | `SomeSans.ttf`, `OFL.txt` |\n";
    let mut fonts = Files::new();
    fonts.insert("THIRD_PARTY.md".into(), md.into());
    fonts.insert("NOTICE".into(), "Some Sans font\n".into());
    fonts.insert("fonts/sans/OFL.txt".into(), "text".into());
    assert_eq!(check_pair(&fonts, &pair), Vec::<String>::new());

    // A LICENSES/OFL-1.1.txt doesn't stand in for the one beside the files.
    fonts.remove("fonts/sans/OFL.txt");
    fonts.insert("LICENSES/OFL-1.1.txt".into(), "text".into());
    let missing = check_pair(&fonts, &pair);
    assert_eq!(missing.len(), 1, "{missing:?}");
    for part in [
        "Fonts",
        "fonts/sans/",
        "no license text",
        "fonts/sans/OFL.txt",
        "OFL-1.1",
    ] {
        assert!(missing[0].contains(part), "{part}: {missing:?}");
    }

    let unnamed = {
        let mut f = fonts.clone();
        f.insert("fonts/sans/OFL.txt".into(), "text".into());
        f.insert("NOTICE".into(), "Other\n".into());
        check_pair(&f, &pair)
    };
    assert_eq!(unnamed.len(), 1, "{unnamed:?}");
    assert!(
        unnamed[0].contains("doesn't name the source `Some Sans`"),
        "{unnamed:?}"
    );
}

#[test]
fn repository_fonts_table_is_checked() {
    // The Fonts table is read, and each of its rows has its OFL.txt: an empty
    // table would pass the checks above vacuously.
    let files = repo_files();
    let fonts: Vec<Row> = rows(&files["THIRD_PARTY.md"])
        .into_iter()
        .filter(|row| row.table == "Fonts")
        .collect();
    assert_eq!(fonts.len(), 3, "the Fonts table's rows");
    for row in &fonts {
        assert!(
            license_ids(&row.license).contains(&"OFL-1.1"),
            "{}",
            row.path
        );
        let text = format!("{}OFL.txt", row.path);
        assert!(
            files
                .get(&text)
                .is_some_and(|t| t.contains("SIL Open Font License")),
            "{text}: not read, or not the OFL"
        );
    }
}

#[test]
fn repository_icons_table_is_checked() {
    // Both copies of the Icons table carry Lucide's, Tabler's and Material
    // Icon Theme's rows, each with its own LICENSE beside the icons.
    let files = repo_files();
    for prefix in ["", "kiln/"] {
        let icons: Vec<Row> = rows(&files[&format!("{prefix}THIRD_PARTY.md")])
            .into_iter()
            .filter(|row| row.table == "Icons")
            .collect();
        let sources: Vec<String> = icons.iter().map(|r| source_name(&r.source)).collect();
        assert_eq!(
            sources,
            [
                "lucide-icons/lucide",
                "tabler/tabler-icons",
                "material-extensions/vscode-material-icon-theme"
            ],
            "{prefix}THIRD_PARTY.md"
        );
    }
    assert!(
        files
            .get(TABLER_TEXT)
            .is_some_and(|t| t.contains("MIT License") && t.contains("Paweł Kuna")),
        "{TABLER_TEXT}: not read, or not Tabler's MIT"
    );
}
