//! A plugin whose commands call the `fastedit:editor/buffer` imports and report what came back:
//! `tests/editor.rs` runs each command and reads the plugin's log. A Rust guest rather than WAT,
//! since the imports take and return lists of records.
//!
//! The commands' arguments are words, not JSON, and a failed import is the command's error as
//! the host returned it:
//!
//! - `read`, `<buffer> [<start> <end>]`: logs `<version> <text>`.
//! - `edit`, `<buffer> <version>`, then one line per edit, `<start> <end> <text>`: logs the new
//!   version.
//! - `propose`, `<buffer> <version> <title>`, then the edits as for `edit`.

wit_bindgen::generate!({
    world: "fastedit:editor/editor-extension",
    path: ["../../../../../kiln/crates/kiln-plugin/wit", "../../../wit"],
    // kiln:extension is read from the first path; generate it here rather than map it elsewhere.
    generate_all,
});

use fastedit::editor::buffer::{self, ProposedChange, Range, TextEdit};
use kiln::extension::log::{Level, log};

struct EditorGuest;

impl Guest for EditorGuest {
    fn init() -> Result<(), Error> {
        Ok(())
    }

    fn shutdown() {}
}

impl exports::kiln::extension::command_handler::Guest for EditorGuest {
    fn run_command(id: String, args: String) -> Result<(), Error> {
        let mut lines = args.lines();
        let mut head = lines.next().unwrap_or_default().splitn(3, ' ');
        match id.as_str() {
            "read" => {
                let buffer = number(head.next())?;
                let range = match head.next() {
                    None => None,
                    Some(start) => Some(Range {
                        start: number(Some(start))?,
                        end: number(head.next())?,
                    }),
                };
                let snapshot = buffer::read(buffer, range)?;
                log(Level::Info, &format!("{} {}", snapshot.version, snapshot.text));
            }
            "edit" => {
                let buffer = number(head.next())?;
                let version = number(head.next())?;
                let version = buffer::edit(buffer, version, &edits(lines)?)?;
                log(Level::Info, &version.to_string());
            }
            "propose" => {
                buffer::propose(&ProposedChange {
                    buffer: number(head.next())?,
                    version: number(head.next())?,
                    title: head.next().unwrap_or_default().to_owned(),
                    edits: edits(lines)?,
                })?;
            }
            _ => return Err(Error::NotFound(id)),
        }
        Ok(())
    }
}

impl exports::kiln::extension::event_handler::Guest for EditorGuest {
    fn handle_events(_events: Vec<kiln::extension::events::Event>) {}
}

fn number(word: Option<&str>) -> Result<u64, Error> {
    let word = word.unwrap_or_default();
    word.parse()
        .map_err(|_| Error::InvalidArgument(format!("{word:?} isn't a number")))
}

/// One edit per line: `<start> <end> <text>`.
fn edits<'a>(lines: impl Iterator<Item = &'a str>) -> Result<Vec<TextEdit>, Error> {
    lines
        .map(|line| {
            let mut words = line.splitn(3, ' ');
            Ok(TextEdit {
                range: Range {
                    start: number(words.next())?,
                    end: number(words.next())?,
                },
                text: words.next().unwrap_or_default().to_owned(),
            })
        })
        .collect()
}

export!(EditorGuest);
