//! docs/vim.md's ex command lists, through the ex line as typed (as
//! `ex_day_one.rs` runs it): every command its tables name is one fe-vim
//! takes, by its shortest and its full name, and every name its
//! "Unsupported ex commands" list gives is E492, or the refusal the list
//! quotes beside it.

use std::{fs, path::Path};

use fe_vim::HeadlessVim;

fn doc() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/vim.md");
    fs::read_to_string(&path).unwrap()
}

/// The lines under the heading `heading`, up to the next heading of its
/// level or above.
fn section<'a>(doc: &'a str, heading: &str) -> Vec<&'a str> {
    let level = heading.find(' ').unwrap();
    doc.lines()
        .skip_while(|line| *line != heading)
        .skip(1)
        .take_while(|line| {
            let hashes = line.len() - line.trim_start_matches('#').len();
            !(1..=level).contains(&hashes) || !line[hashes..].starts_with(' ')
        })
        .collect()
}

/// The spans between backticks in `text`, in order.
fn spans(text: &str) -> Vec<&str> {
    text.split('`').skip(1).step_by(2).collect()
}

/// What `:{line}<CR>` leaves as the message, if anything.
fn run(line: &str) -> Option<String> {
    let mut vim = HeadlessVim::new("one\ntwo\n", (0, 0));
    vim.feed(&format!(":{line}<CR>")).unwrap();
    vim.state().message().map(String::from)
}

/// The supported commands: the `:name[rest]` spans in the first cell of
/// each row of the tables under "Ex commands", as their shortest and full
/// names.
fn supported(doc: &str) -> Vec<(String, String)> {
    let mut names = Vec::new();
    for line in section(doc, "## Ex commands") {
        let Some(row) = line.strip_prefix('|') else {
            continue;
        };
        let first = row.split(" | ").next().unwrap();
        for span in spans(first) {
            let Some(form) = span.strip_prefix(':') else {
                continue;
            };
            let short: String = form.split('[').next().unwrap().to_string();
            let full: String = form.chars().filter(|ch| !matches!(ch, '[' | ']')).collect();
            names.push((short, full));
        }
    }
    names
}

#[test]
fn listed_supported_commands_parse() {
    let names = supported(&doc());
    assert!(names.len() > 50, "only {} supported commands", names.len());
    for (short, full) in names {
        for name in [&short, &full] {
            let message = run(name).unwrap_or_default();
            assert!(!message.starts_with("E492"), ":{name}: {message}");
        }
    }
}

#[test]
fn unsupported_commands_are_refused() {
    let doc = doc();
    let lines: Vec<&str> = section(&doc, "### Unsupported ex commands")
        .into_iter()
        .filter(|line| line.starts_with("- `"))
        .collect();
    assert!(
        lines.len() > 20,
        "only {} unsupported commands",
        lines.len()
    );
    for line in lines {
        let name = spans(line)[0];
        let message = run(name).unwrap_or_else(|| panic!(":{name} gave no message"));
        match line.split_once("refused: ") {
            Some((_, refusal)) => {
                let refusal = spans(refusal)[0];
                assert!(message.starts_with(refusal), ":{name}: {message}");
            }
            None => assert_eq!(
                message,
                format!("E492: Not an editor command: {name}"),
                ":{name}"
            ),
        }
    }
}
