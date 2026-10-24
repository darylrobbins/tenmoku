//! Inline edit's headless core: the request, the reply read as it streams,
//! and the proposal.

use fe_buffer::{Buffer, HunkView, Origin};
use fe_chat::{
    ChatEvent, ChatRequest, ChatSettings, ErrorKind, InlineEditInput, InlineEditSession,
    InlineEditState, InlineEditUpdate, ProviderError, ScriptedProvider, StopReason, inline_request,
    run_inline_edit, to_proposal,
};
use futures::StreamExt;
use futures::executor::block_on;

fn input() -> InlineEditInput {
    InlineEditInput {
        path: "src/lib.rs".into(),
        language: "rust".into(),
        text_before: "fn a() {}\n".into(),
        selection: "fn b() {}\n".into(),
        text_after: "fn c() {}\n".into(),
        instruction: "rename b to bee".into(),
        settings: ChatSettings::default(),
    }
}

fn user_texts(request: &ChatRequest) -> Vec<String> {
    assert_eq!(request.messages.len(), 1);
    request.messages[0]
        .content
        .iter()
        .map(|block| block["text"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn request_holds_selection_between_markers() {
    let request = inline_request(&input());
    let texts = user_texts(&request);
    assert_eq!(
        texts[0],
        "<file path=\"src/lib.rs\" language=\"rust\">\n\
         fn a() {}\n<selection>fn b() {}\n</selection>fn c() {}\n</file>"
    );
    let system = &request.system[0].text;
    assert!(system.contains("only the replacement"), "{system}");
    assert!(system.contains("one fenced code block"), "{system}");
    assert!(system.contains("src/lib.rs (rust)"), "{system}");
    assert_eq!(request.model, ChatSettings::default().model);
    assert_eq!(request.max_tokens, ChatSettings::default().max_tokens);
}

#[test]
fn context_is_cut_at_line_starts() {
    let line = "0123456789\n";
    let mut input = input();
    input.text_before = line.repeat(1_000);
    input.text_after = line.repeat(1_000);
    let context = &user_texts(&inline_request(&input))[0];
    let (before, rest) = context.split_once("<selection>").unwrap();
    let (_, after) = rest.split_once("</selection>").unwrap();
    let before = before.split_once(">\n").unwrap().1;
    let after = after.strip_suffix("</file>").unwrap();
    // 4,000 bytes is 363 whole lines and some more.
    assert_eq!(before.len(), 363 * line.len());
    assert!(before.starts_with(line) && before.ends_with('\n'));
    assert_eq!(after.len(), 363 * line.len());
    assert!(after.starts_with(line) && after.ends_with('\n'));

    // Short text is kept whole; a long line with no break gives nothing.
    input.text_before = "x".repeat(5_000);
    input.text_after = "é".repeat(3_000);
    let context = &user_texts(&inline_request(&input))[0];
    assert!(context.contains(">\n<selection>"), "{context}");
    assert!(context.contains("</selection>\n</file>"), "{context}");
}

#[test]
fn caret_means_insert() {
    let mut input = input();
    input.selection.clear();
    let request = inline_request(&input);
    let context = &user_texts(&request)[0];
    assert!(
        context.contains("fn a() {}\n<caret/>fn c() {}\n"),
        "{context}"
    );
    assert!(!context.contains("<selection>"), "{context}");
    assert!(request.system[0].text.contains("insert"));
}

#[test]
fn instruction_is_the_last_user_text() {
    let texts = user_texts(&inline_request(&input()));
    assert_eq!(texts.last().unwrap(), "rename b to bee");
}

fn delta(text: &str) -> ChatEvent {
    ChatEvent::TextDelta {
        index: 0,
        text: text.into(),
    }
}

fn stop() -> ChatEvent {
    ChatEvent::Finished {
        stop_reason: StopReason::EndTurn,
        stop_details: None,
    }
}

fn played(events: &[ChatEvent]) -> InlineEditSession {
    let mut session = InlineEditSession::new();
    for event in events {
        session.apply(event);
    }
    session
}

#[test]
fn fence_split_across_deltas() {
    let mut session = InlineEditSession::new();
    for piece in ["``", "`rust\nfn bee()", " {}\n``", "`", "\n"] {
        session.apply(&delta(piece));
        assert!(
            !session.text_so_far().contains('`'),
            "{:?}",
            session.text_so_far()
        );
    }
    assert_eq!(session.text_so_far(), "fn bee() {}\n");
    assert_eq!(session.state(), &InlineEditState::Streaming);
    session.apply(&stop());
    assert_eq!(session.state(), &InlineEditState::Done);
    assert_eq!(session.finish().as_deref(), Some("fn bee() {}\n"));
}

#[test]
fn language_tag_ignored() {
    for opening in ["```rust\n", "```rust path=x.rs\n", "```\n", "````\n"] {
        let closing = if opening == "````\n" { "````" } else { "```" };
        let session = played(&[delta(&format!("{opening}a\n{closing}")), stop()]);
        assert_eq!(session.finish().as_deref(), Some("a\n"), "{opening}");
    }
    // The tag arriving a letter at a time never shows in the text.
    let mut session = InlineEditSession::new();
    for piece in ["```", "r", "us", "t", "\n", "x"] {
        session.apply(&delta(piece));
    }
    assert_eq!(session.text_so_far(), "x");
}

#[test]
fn prose_before_and_after_the_fence_ignored() {
    let session = played(&[
        delta("Sure, here you go:\n\n```rust\nlet x = 1;\n```\n"),
        delta("Hope that helps!\n```\nnot this\n```\n"),
        stop(),
    ]);
    assert_eq!(session.finish().as_deref(), Some("let x = 1;\n"));
}

#[test]
fn no_fence_is_no_code() {
    let session = played(&[delta("I can't do that."), stop()]);
    assert_eq!(session.state(), &InlineEditState::NoCode);
    assert_eq!(session.finish(), None);
    assert_eq!(session.text_so_far(), "");

    // A fence the reply never closed is a cut-off reply, not an answer.
    let session = played(&[delta("```rust\nfn half("), stop()]);
    assert_eq!(session.state(), &InlineEditState::NoCode);
    assert_eq!(session.finish(), None);
}

#[test]
fn provider_error_is_failed() {
    let error = ProviderError::new(ErrorKind::Overloaded, "busy");
    let session = played(&[
        delta("```\nabc\n"),
        ChatEvent::Failed(error.clone()),
        delta("```"),
        stop(),
    ]);
    assert_eq!(session.state(), &InlineEditState::Failed(error));
    assert_eq!(session.finish(), None);
}

#[test]
fn partial_reply_text_so_far_grows() {
    let mut session = InlineEditSession::new();
    let mut seen = Vec::new();
    for piece in ["```\n", "one\ntw", "o\n", "three"] {
        session.apply(&delta(piece));
        seen.push(session.text_so_far());
    }
    assert_eq!(seen, ["", "one\ntw", "one\ntwo\n", "one\ntwo\nthree"]);
}

fn scripted(deltas: &[&str]) -> ScriptedProvider {
    let mut events: Vec<ChatEvent> = deltas.iter().map(|text| delta(text)).collect();
    events.push(stop());
    ScriptedProvider::new([events])
}

#[test]
fn scripted_reply_streams_updates() {
    let provider = scripted(&["Here:\n```rust\nfn be", "e() {}\n", "```\n"]);
    let updates: Vec<InlineEditUpdate> = block_on(run_inline_edit(&provider, &input()).collect());
    assert_eq!(
        updates,
        [
            InlineEditUpdate::TextSoFar("fn be".into()),
            InlineEditUpdate::TextSoFar("fn bee() {}\n".into()),
            InlineEditUpdate::Done("fn bee() {}\n".into()),
        ]
    );
    assert_eq!(provider.requests(), [inline_request(&input())]);
    assert_eq!(provider.dropped(), 0);

    let provider = ScriptedProvider::new([vec![delta("no code")]]);
    let updates: Vec<_> = block_on(run_inline_edit(&provider, &input()).collect());
    assert!(matches!(updates.as_slice(), [InlineEditUpdate::Failed(_)]));
    let provider = scripted(&["no code"]);
    let updates: Vec<_> = block_on(run_inline_edit(&provider, &input()).collect());
    assert_eq!(updates, [InlineEditUpdate::NoCode]);
}

#[test]
fn dropping_cancels_the_provider() {
    let provider = scripted(&["```\n", "a\n", "b\n", "```\n"]);
    let mut updates = Box::pin(run_inline_edit(&provider, &input()));
    let first = block_on(updates.next());
    assert_eq!(first, Some(InlineEditUpdate::TextSoFar("a\n".into())));
    assert_eq!(provider.dropped(), 0);
    drop(updates);
    assert_eq!(provider.dropped(), 1);
}

const THREE: &str = "one\ntwo\nthree\nfour\n";

fn hunk_texts(hunks: &[HunkView]) -> Vec<(String, String)> {
    hunks
        .iter()
        .map(|hunk| (hunk.old_text.to_string(), hunk.new_text.to_string()))
        .collect()
}

#[test]
fn replacement_becomes_hunks_for_changed_lines_only() {
    let buffer = Buffer::new(THREE);
    let base = buffer.snapshot();
    let range = 0..THREE.len();
    let proposal = to_proposal(&base, range, "one\nTWO\nthree\nFOUR\n", "s1");
    assert_eq!(
        hunk_texts(&proposal.hunks(&base)),
        [
            ("two\n".into(), "TWO\n".into()),
            ("four\n".into(), "FOUR\n".into())
        ]
    );
    assert_eq!(
        proposal.origin(),
        &Origin::Ai {
            agent: "fastedit.inline_edit".into(),
            session: "s1".into()
        }
    );
}

#[test]
fn identical_replacement_has_no_hunks() {
    let buffer = Buffer::new(THREE);
    let base = buffer.snapshot();
    let proposal = to_proposal(&base, 4..14, "two\nthree\n", "s1");
    assert!(proposal.hunks(&base).is_empty());
}

#[test]
fn insertion_at_caret_is_one_hunk() {
    let buffer = Buffer::new(THREE);
    let base = buffer.snapshot();
    let at = "one\ntwo\n".len();
    let proposal = to_proposal(&base, at..at, "inserted\n", "s1");
    let hunks = proposal.hunks(&base);
    assert_eq!(hunk_texts(&hunks), [("".into(), "inserted\n".into())]);
    assert_eq!(hunks[0].range, at..at);
}

#[test]
fn closing_fence_tolerates_whitespace() {
    for closing in [" ```\n", "``` \n", "   ```\t\n", "```"] {
        let session = played(&[
            delta(&format!(
                "```rust\nx\n{closing}{}",
                if closing.ends_with('\n') {
                    "after\n"
                } else {
                    ""
                }
            )),
            stop(),
        ]);
        assert_eq!(session.finish().as_deref(), Some("x\n"), "{closing:?}");
        assert_eq!(session.text_so_far(), "x\n", "{closing:?}");
    }
    // Split across deltas, the closer never shows in the text.
    let mut session = InlineEditSession::new();
    for piece in ["```\nx\n", " ", "``", "`", " ", "\nprose"] {
        session.apply(&delta(piece));
        assert_eq!(session.text_so_far(), "x\n", "{piece:?}");
    }
    // Four spaces in is body, not a fence.
    let session = played(&[delta("```\nx\n    ```\n```\n"), stop()]);
    assert_eq!(session.finish().as_deref(), Some("x\n    ```\n"));
}

#[test]
fn whitespace_closing_fence_through_the_provider() {
    let provider = scripted(&["```rust\nx\n", " ``", "` \nprose\n"]);
    let updates: Vec<_> = block_on(run_inline_edit(&provider, &input()).collect());
    assert_eq!(updates.last(), Some(&InlineEditUpdate::Done("x\n".into())));
}
