//! The scenes and their capture, with the Metal renderer: macOS only.

use std::{
    collections::VecDeque,
    process::ExitCode,
    sync::{Arc, Mutex},
};

use fe_chat::{
    BlockKind, ChatEvent, ChatProvider, ChatRequest, ChatSettings, ChatStream, Conversation,
    StopReason,
};
use fe_chat_view::{ChatOptions, ChatView};
use fe_gpui::{EditorPalette, EditorSettings, set_editor_palette};
use futures::StreamExt as _;
use gpui::{AppContext as _, Pixels, Size, VisualTestAppContext, WindowHandle, px, size};
use kiln_platform::screenshots::{Goldens, Result, RgbaImage, Settle, exit_code};
use kiln_ui::{Theme, UiTheme, set_ui_theme};
use serde_json::json;

/// The window size of every scene, in logical pixels: a chat panel's.
const SCENE_SIZE: Size<Pixels> = size(px(520.), px(860.));

/// Pinned so goldens don't depend on which fonts are installed (TESTING.md).
const FONT_FAMILY: &str = "Menlo";

const MODEL: &str = "claude-opus-5-5";

/// The first reply: a heading, a list, inline code and a block for a file.
const REPLY: &str = r#"## Doubling

Add a helper next to `triple`:

- it takes a `u32`
- it returns **twice** that

```rust path=src/lib.rs
/// Twice `x`.
pub fn double(x: u32) -> u32 {
    x * 2 // never overflows here
}
```

Then call `double(21)`."#;

/// A reply whose block for `deny.toml` is two SEARCH/REPLACE sections,
/// drawn as a diff.
const DIFF_REPLY: &str = r#"Make the bans strict and set a licence threshold:

```toml path=deny.toml
<<<<<<< SEARCH
[bans]
multiple-versions = "warn"
=======
[bans]
multiple-versions = "deny"
wildcards = "deny"
>>>>>>> REPLACE

<<<<<<< SEARCH
[licenses]
=======
[licenses]
confidence-threshold = 0.9
>>>>>>> REPLACE
```

Then run `cargo deny check`."#;

const THINKING: &str = "The user wants a *small* helper; a `u32` function is enough.";

struct Scene {
    name: &'static str,
    chat: Chat,
    palette: &'static EditorPalette,
    ui: UiTheme,
}

/// What a scene's chat holds.
#[derive(Clone, Copy)]
enum Chat {
    /// Three turns: answered with thinking, refused, and streaming.
    Transcript,
    /// One finished reply with an edit drawn as a diff.
    Diff,
}

fn scenes() -> Vec<Scene> {
    vec![
        Scene {
            name: "chat_dark",
            chat: Chat::Transcript,
            palette: EditorPalette::fastedit_dark(),
            ui: UiTheme::default_dark(),
        },
        Scene {
            name: "chat_light",
            chat: Chat::Transcript,
            palette: EditorPalette::fastedit_light(),
            ui: Theme::light().ui_theme(),
        },
        Scene {
            name: "chat_diff_dark",
            chat: Chat::Diff,
            palette: EditorPalette::fastedit_dark(),
            ui: UiTheme::default_dark(),
        },
        Scene {
            name: "chat_diff_light",
            chat: Chat::Diff,
            palette: EditorPalette::fastedit_light(),
            ui: Theme::light().ui_theme(),
        },
    ]
}

/// Plays one scripted reply per request; a reply with no `Finished` goes
/// on streaming for good.
struct Script(Mutex<VecDeque<Vec<ChatEvent>>>);

impl ChatProvider for Script {
    fn name(&self) -> &str {
        "script"
    }

    fn stream(&self, _: ChatRequest) -> ChatStream {
        let events = self.0.lock().expect("the script").pop_front();
        let events = events.unwrap_or_default();
        Box::pin(futures::stream::iter(events).chain(futures::stream::pending()))
    }
}

fn started() -> ChatEvent {
    ChatEvent::Started {
        model: MODEL.to_owned(),
        message_id: "msg".to_owned(),
    }
}

fn text(index: usize, text: &str) -> [ChatEvent; 2] {
    [
        ChatEvent::BlockStarted {
            index,
            kind: BlockKind::Text,
        },
        ChatEvent::TextDelta {
            index,
            text: text.to_owned(),
        },
    ]
}

/// The three turns: answered with thinking, refused, and streaming.
fn script() -> Script {
    let mut answered = vec![
        started(),
        ChatEvent::BlockStarted {
            index: 0,
            kind: BlockKind::Thinking,
        },
        ChatEvent::ThinkingDelta {
            index: 0,
            text: THINKING.to_owned(),
        },
        ChatEvent::BlockFinished {
            index: 0,
            block: json!({"type": "thinking", "thinking": THINKING, "signature": "sig"}),
        },
    ];
    answered.extend(text(1, REPLY));
    answered.extend([
        ChatEvent::BlockFinished {
            index: 1,
            block: json!({"type": "text", "text": REPLY}),
        },
        ChatEvent::Finished {
            stop_reason: StopReason::EndTurn,
            stop_details: None,
        },
    ]);
    let refused = vec![
        started(),
        ChatEvent::Finished {
            stop_reason: StopReason::Refusal,
            stop_details: Some(json!({"type": "refusal", "category": "cyber"})),
        },
    ];
    let mut streaming = vec![started()];
    streaming.extend(text(
        0,
        "Reading `Cargo.toml` first:\n\n```toml\n[package]\nname = \"demo\"\n",
    ));
    Script(Mutex::new(VecDeque::from([answered, refused, streaming])))
}

/// The one turn: [`DIFF_REPLY`], finished.
fn diff_script() -> Script {
    let mut answered = vec![started()];
    answered.extend(text(0, DIFF_REPLY));
    answered.extend([
        ChatEvent::BlockFinished {
            index: 0,
            block: json!({"type": "text", "text": DIFF_REPLY}),
        },
        ChatEvent::Finished {
            stop_reason: StopReason::EndTurn,
            stop_details: None,
        },
    ]);
    Script(Mutex::new(VecDeque::from([answered])))
}

/// Sends `question` from the composer, as Send does.
fn send(
    cx: &mut VisualTestAppContext,
    window: WindowHandle<ChatView>,
    question: &str,
) -> Result<()> {
    window.update(cx, |view, window, cx| {
        view.set_composer_text(question, cx);
        kiln::with_cx(window, cx, |cx| view.send_message(cx));
    })?;
    cx.run_until_parked();
    Ok(())
}

/// Renders the chat in `scene`'s colours, off screen, once its replies and
/// highlights have landed.
fn capture(cx: &mut VisualTestAppContext, scene: &Scene) -> Result<RgbaImage> {
    cx.update(|cx| {
        set_ui_theme(cx, scene.ui);
        set_editor_palette(cx, Arc::new(scene.palette.clone()));
    });
    let window = cx.open_offscreen_window(SCENE_SIZE, |window, cx| {
        let conversation = Conversation::new(&ChatSettings {
            model: MODEL.to_owned(),
            show_thinking: true,
            ..ChatSettings::default()
        });
        let provider: Arc<dyn ChatProvider> = Arc::new(match scene.chat {
            Chat::Transcript => script(),
            Chat::Diff => diff_script(),
        });
        let settings = EditorSettings {
            font_family: Some(FONT_FAMILY.to_owned()),
            ..EditorSettings::default()
        };
        cx.new(|cx| {
            let view = ChatView::new(provider, conversation, ChatOptions::default(), window, cx);
            view.composer()
                .update(cx, |composer, cx| composer.apply_settings(&settings, cx));
            view
        })
    })?;
    match scene.chat {
        Chat::Transcript => {
            send(cx, window, "How do I double a number?")?;
            send(cx, window, "And how do I get into my neighbour's router?")?;
            send(cx, window, "Add it to the crate")?;
            window.update(cx, |view, _, cx| view.set_thinking_open(0, true, cx))?;
        }
        Chat::Diff => send(cx, window, "Tighten deny.toml")?,
    }
    let view = window.update(cx, |_, _, cx| cx.entity())?;
    let mut settle = Settle::new();
    settle.observe(&view, cx);
    Ok(settle.capture(cx, window.into())?.0)
}

/// Renders every scene and compares it with its golden, or rewrites the
/// goldens. Returns whether all scenes matched.
fn check_scenes(cx: &mut VisualTestAppContext) -> Result<bool> {
    let goldens = Goldens::new(env!("CARGO_MANIFEST_DIR"), "examples/screenshots/golden");
    let mut passed = true;
    for scene in scenes() {
        let image = capture(cx, &scene)?;
        passed &= goldens.check(scene.name, &image)?;
    }
    Ok(passed)
}

pub(crate) fn main() -> ExitCode {
    let mut cx = VisualTestAppContext::new(gpui_platform::current_platform(false));
    cx.update(|cx| {
        fe_gpui::init(cx);
        fe_chat_view::init(cx);
    });
    exit_code(check_scenes(&mut cx))
}
