//! A stand-in agent host for checking an AI's streamed edit by hand: typing
//! while it streams, cmd-z skipping its text, and "undo AI" taking it all
//! back.
//!
//! ```text
//! cargo run -p fe-gpui --example stream_demo -- <file> <source-file>
//! ```
//!
//! Opens `<file>` in a window and reads one-letter commands from stdin:
//!
//! - `s` streams `<source-file>`'s text into the editor at the primary
//!   selection, 16 bytes every 60 ms, as one AI-origin undo group;
//! - `c` stops the stream, keeping what landed;
//! - `u` undoes the last AI transaction, `r` redoes it;
//! - `q` quits.
//!
//! Each chunk, stream error, undo or redo and buffer edit is printed.

use std::{
    io::BufRead as _,
    process::ExitCode,
    sync::mpsc::{self, Receiver, TryRecvError},
    time::Duration,
};

use fe_buffer::{BufferEvent, EditStream, Origin, Subscription};
use fe_editor::EditorModel;
use fe_gpui::{EditorTheme, EditorView};
use gpui::{App, AppContext as _, AsyncApp, Focusable as _, WeakEntity, WindowOptions};

const USAGE: &str = "usage: stream_demo <file> <source-file>";
const CHUNK_BYTES: usize = 16;
const CHUNK_EVERY: Duration = Duration::from_millis(60);
const POLL_EVERY: Duration = Duration::from_millis(20);

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [file, source] = args.as_slice() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let (mut model, source) = match (EditorModel::open(file), std::fs::read_to_string(source)) {
        (Ok(model), Ok(source)) => (model, source),
        (model, source) => {
            if let Err(error) = model {
                eprintln!("{file}: {error}");
            }
            if let Err(error) = source {
                eprintln!("{}: {error}", args[1]);
            }
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };

    let (sender, commands) = mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            for command in line.chars().filter(|c| !c.is_whitespace()) {
                if sender.send(command).is_err() {
                    return;
                }
            }
        }
        sender.send('q').ok();
    });

    gpui_platform::application().run(move |cx: &mut App| {
        fe_gpui::init(cx);
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let events = model.buffer_mut().subscribe();
        let window = cx
            .open_window(WindowOptions::default(), move |window, cx| {
                let view = cx.new(|cx| {
                    model.set_soft_wrap(false);
                    EditorView::new(model, EditorTheme::default_dark(cx), cx)
                });
                window.focus(&view.focus_handle(cx), cx);
                view
            })
            .expect("the window opens");
        let view = window.root(cx).expect("the window is open");
        let weak = view.downgrade();
        cx.spawn(async move |cx| host(weak, source, commands, events, cx).await)
            .detach();
        cx.activate(true);
    });
    ExitCode::SUCCESS
}

/// The text still to push, and the stream it goes into.
struct Streaming {
    stream: EditStream,
    rest: String,
}

/// Runs the agent host: polls the commands, pushes the next chunk when it's
/// due and prints the buffer's events.
async fn host(
    view: WeakEntity<EditorView>,
    source: String,
    commands: Receiver<char>,
    events: Subscription,
    cx: &mut AsyncApp,
) {
    let mut streaming: Option<Streaming> = None;
    let mut streams = 0;
    let mut since_chunk = CHUNK_EVERY;
    loop {
        let command = match commands.try_recv() {
            Ok(command) => Some(command),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some('q'),
        };
        let chunk_due = since_chunk >= CHUNK_EVERY;
        let step = view.update(cx, |view, cx| {
            let quit = step(
                view,
                command,
                chunk_due,
                &source,
                &mut streams,
                &mut streaming,
            );
            for event in events.drain() {
                if let BufferEvent::Edited { origin, kind, .. } = event {
                    println!("edited: {origin:?} {kind:?}");
                }
            }
            cx.notify();
            quit
        });
        if chunk_due {
            since_chunk = Duration::ZERO;
        }
        if step.unwrap_or(true) {
            cx.update(|cx| cx.quit());
            return;
        }
        cx.background_executor().timer(POLL_EVERY).await;
        since_chunk += POLL_EVERY;
    }
}

/// Handles `command`, then pushes a chunk if one is `chunk_due`. Returns
/// whether to quit.
fn step(
    view: &mut EditorView,
    command: Option<char>,
    chunk_due: bool,
    source: &str,
    streams: &mut usize,
    streaming: &mut Option<Streaming>,
) -> bool {
    match command {
        Some('s') if streaming.is_some() => println!("already streaming"),
        Some('s') => {
            *streams += 1;
            let range = view.model().primary_selection().range;
            let origin = Origin::Ai {
                agent: "demo".into(),
                session: format!("stream-{streams}").into(),
            };
            let stream = EditStream::begin(&view.model().buffer(), range.clone(), origin);
            println!("stream {streams} started over {range:?}");
            *streaming = Some(Streaming {
                stream,
                rest: source.to_owned(),
            });
        }
        Some('c') => match streaming.take() {
            Some(Streaming { stream, .. }) => {
                let transaction = stream.finish(&mut view.model_mut().buffer_mut());
                println!("stream stopped: {transaction:?}");
            }
            None => println!("not streaming"),
        },
        Some(command @ ('u' | 'r')) => {
            let mut buffer = view.model_mut().buffer_mut();
            let (name, result) = if command == 'u' {
                ("undo AI", buffer.undo(Origin::is_ai))
            } else {
                ("redo AI", buffer.redo(Origin::is_ai))
            };
            match result {
                Ok(transaction) => println!("{name}: {transaction:?}"),
                Err(error) => println!("{name}: {error:?}"),
            }
        }
        Some('q') => return true,
        Some(other) => println!("unknown command {other:?}: s, c, u, r or q"),
        None => {}
    }
    if chunk_due && let Some(current) = streaming {
        push_chunk(view, current);
        if current.rest.is_empty() {
            let finished = streaming.take().expect("streaming");
            let transaction = finished.stream.finish(&mut view.model_mut().buffer_mut());
            println!("stream done: {transaction:?}");
        }
    }
    false
}

/// Pushes the next chunk of `current`, printing the region it grew to or
/// the error that killed the stream.
fn push_chunk(view: &mut EditorView, current: &mut Streaming) {
    let mut at = CHUNK_BYTES.min(current.rest.len());
    while !current.rest.is_char_boundary(at) {
        at += 1;
    }
    let chunk: String = current.rest.drain(..at).collect();
    let mut buffer = view.model_mut().buffer_mut();
    match current.stream.push(&mut buffer, &chunk) {
        Ok(()) => {
            current.stream.refresh(buffer.current());
            println!(
                "chunk {chunk:?}: region {:?}",
                current.stream.range(buffer.current())
            );
        }
        Err(error) => {
            println!("stream error: {error}");
            current.rest.clear();
        }
    }
}
