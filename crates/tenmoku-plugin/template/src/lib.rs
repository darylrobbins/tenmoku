//! Shows the focused buffer's word count in the status bar, and logs it on a command.

use tenmoku_plugin::buffer;
use tenmoku_plugin::commands::{self, Command};
use tenmoku_plugin::events::{self, Event, EventKinds};
use tenmoku_plugin::status::{self, Alignment, Item};
use tenmoku_plugin::types::BufferId;
use tenmoku_plugin::{Error, Plugin, export_plugin, info, warn};

const COUNT: &str = "example.word_count.count";
const STATUS_ITEM: &str = "words";

#[derive(Default)]
struct WordCount {
    focused: Option<BufferId>,
}

impl WordCount {
    /// Shows the focused buffer's count, or no item when there is nothing to count.
    fn refresh(&self) {
        let shown = self.focused.map(words).and_then(|words| match words {
            Ok(words) => Some(words),
            Err(error) => {
                warn!("couldn't count words: {error:?}");
                None
            }
        });
        let result = match shown {
            Some(words) => status::set_item(&Item {
                id: STATUS_ITEM.to_owned(),
                text: label(words),
                tooltip: None,
                command: Some(COUNT.to_owned()),
                alignment: Alignment::Right,
            }),
            // Not shown is what's wanted, so `not-found` is fine.
            None => status::remove_item(STATUS_ITEM).or_else(|error| match error {
                Error::NotFound(_) => Ok(()),
                error => Err(error),
            }),
        };
        if let Err(error) = result {
            warn!("couldn't update the status bar: {error:?}");
        }
    }
}

impl Plugin for WordCount {
    fn init(&mut self) -> Result<(), Error> {
        commands::register(&Command {
            id: COUNT.to_owned(),
            title: "Word Count: Count Words".to_owned(),
            args_schema: None,
            when: None,
            mutates_buffer: false,
        })?;
        // A host that doesn't implement `subscribe` yet chooses what it delivers itself.
        match events::subscribe(EventKinds::FOCUS_CHANGED | EventKinds::BUFFER_SAVED) {
            Ok(()) | Err(Error::Unimplemented(_)) => Ok(()),
            Err(error) => Err(error),
        }
    }

    fn run_command(&mut self, id: &str, _args: &str) -> Result<(), Error> {
        if id != COUNT {
            return Err(Error::not_found(id));
        }
        let focused = self
            .focused
            .ok_or_else(|| Error::failed("no buffer is focused"))?;
        info!("{}", words(focused)?);
        Ok(())
    }

    fn handle_events(&mut self, events: Vec<Event>) {
        for event in events {
            match event {
                Event::FocusChanged(focused) => {
                    self.focused = focused;
                    self.refresh();
                }
                Event::BufferSaved(saved) if self.focused == Some(saved.buffer) => self.refresh(),
                // A save may be among what the host dropped.
                Event::Lagged(_) => self.refresh(),
                _ => {}
            }
        }
    }
}

/// How many words `buffer` holds now.
fn words(buffer: BufferId) -> Result<usize, Error> {
    let snapshot = buffer::read(buffer, None)?;
    Ok(snapshot.text.split_whitespace().count())
}

fn label(words: usize) -> String {
    match words {
        1 => "1 word".to_owned(),
        words => format!("{words} words"),
    }
}

export_plugin!(WordCount);
