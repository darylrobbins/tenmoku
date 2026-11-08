//! `textDocument/onTypeFormatting`: the client capability, a server's
//! static trigger characters and its dynamic registrations, and the request
//! with the typed character and the editor's options.

use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use fe_buffer::Buffer;
use futures::FutureExt;
use lsp_types::request::{RegisterCapability, UnregisterCapability};
use lsp_types::{
    DocumentOnTypeFormattingOptions, PositionEncodingKind, Registration, RegistrationParams,
    ServerCapabilities, Unregistration, UnregistrationParams, Url,
};
use serde_json::{Value, json};

use super::FormatOptions;
use crate::fake::{
    FakeServer, InitializeReply, Record, Reply, Script, Scripted, initialize_result,
};
use crate::{Clock, LspManager, ManualClock, ServerId, ServerSpec, SystemClock};

const ROOT: &str = crate::fake::FAKE_ROOT;
const ON_TYPE: &str = "textDocument/onTypeFormatting";

fn answer<T>(reply: impl Future<Output = T>) -> T {
    let reply = reply.fuse();
    let timeout = SystemClock.sleep(Duration::from_secs(10)).fuse();
    futures::pin_mut!(reply, timeout);
    async_io::block_on(async {
        futures::select_biased! {
            answer = reply => answer,
            () = timeout => panic!("no answer in 10 s"),
        }
    })
}

/// The requests of `method` the fake got.
fn requests(fake: &FakeServer, method: &str) -> Vec<Value> {
    (fake.records().into_iter())
        .filter_map(|record| match record {
            Record::Request {
                method: m, params, ..
            } if m == method => Some(params),
            _ => None,
        })
        .collect()
}

struct Started {
    fake: FakeServer,
    manager: LspManager,
    server: ServerId,
    buffer: Buffer,
    uri: Url,
}

/// A server on the fake in UTF-16, with `provider` as its static on-type
/// formatting provider, with a buffer of `text` open as `a.rs` and synced.
fn started(provider: Option<DocumentOnTypeFormattingOptions>, text: &str) -> Started {
    let mut init = initialize_result(Some(PositionEncodingKind::UTF16));
    init.capabilities = ServerCapabilities {
        document_on_type_formatting_provider: provider,
        ..init.capabilities
    };
    let fake = FakeServer::new(Script {
        initialize: InitializeReply::Result(Box::new(init)),
        replies: HashMap::new(),
        ..Script::default()
    });
    let clock = Arc::new(ManualClock::new());
    let mut manager = LspManager::with_clock(fake.launcher(), clock, |_| {});
    let server = manager
        .start(ServerSpec::new("fake", "fake-server"), Path::new(ROOT))
        .unwrap();
    fake.wait_initialized();
    let mut buffer = Buffer::new(text);
    let path = PathBuf::from(ROOT).join("a.rs");
    let subscription = buffer.subscribe();
    manager
        .open_on(
            server,
            buffer.id(),
            buffer.text().clone(),
            buffer.version(),
            subscription,
            &path,
            "rust",
        )
        .unwrap();
    let uri = Url::from_file_path(&path).unwrap();
    fake.wait_for_document(&uri, "didOpen", |r| r.is_some_and(|r| r.open));
    Started {
        fake,
        manager,
        server,
        buffer,
        uri,
    }
}

fn register(fake: &FakeServer, id: &str, options: Option<Value>) -> Result<Value, i32> {
    fake.request::<RegisterCapability>(RegistrationParams {
        registrations: vec![Registration {
            id: id.into(),
            method: ON_TYPE.into(),
            register_options: options,
        }],
    })
    .map_err(|refused| refused.code.unwrap_or_default())
}

fn unregister(fake: &FakeServer, id: &str) {
    let done = fake.request::<UnregisterCapability>(UnregistrationParams {
        unregisterations: vec![Unregistration {
            id: id.into(),
            method: ON_TYPE.into(),
        }],
    });
    assert_eq!(done, Ok(Value::Null));
}

fn edit(start: (u32, u32), end: (u32, u32), text: &str) -> Value {
    json!({
        "range": {
            "start": { "line": start.0, "character": start.1 },
            "end": { "line": end.0, "character": end.1 },
        },
        "newText": text,
    })
}

#[test]
fn dynamic_registration_is_accepted_with_trigger_characters() {
    let s = started(None, "fn main() {\n    }\n");
    let buffer = s.buffer.id();

    // The client says it takes the registration.
    let initialize = requests(&s.fake, "initialize").pop().unwrap();
    assert_eq!(
        initialize["capabilities"]["textDocument"]["onTypeFormatting"]["dynamicRegistration"],
        json!(true)
    );
    // No static provider, no registration: no triggers, and nothing sent.
    assert!(s.manager.on_type_formatting_triggers(buffer).is_empty());
    let version = s.buffer.version();
    let none = answer(
        s.manager
            .on_type_formatting(buffer, version, 16, '}', OPTIONS),
    );
    assert_eq!(none, Ok(None));

    // Accepted and recorded, with its characters, each once.
    let options = json!({
        "documentSelector": [{ "language": "rust" }],
        "firstTriggerCharacter": "}",
        "moreTriggerCharacter": [";", "\n", "}"],
    });
    assert_eq!(register(&s.fake, "rust", Some(options)), Ok(Value::Null));
    assert_eq!(s.manager.registrations(s.server), ["rust"]);
    assert_eq!(
        s.manager.on_type_formatting_triggers(buffer),
        ['}', ';', '\n']
    );

    // A selector that doesn't take the document adds nothing; a null one,
    // or a pattern that matches its path, takes every character it names.
    let other = json!({
        "documentSelector": [{ "language": "python" }, { "pattern": "**/*.py" }],
        "firstTriggerCharacter": ")",
    });
    assert_eq!(register(&s.fake, "python", Some(other)), Ok(Value::Null));
    let any = json!({ "documentSelector": null, "firstTriggerCharacter": "]" });
    assert_eq!(register(&s.fake, "any", Some(any)), Ok(Value::Null));
    let pattern = json!({
        "documentSelector": [{ "scheme": "file", "pattern": "**/*.rs" }],
        "firstTriggerCharacter": ",",
    });
    assert_eq!(register(&s.fake, "pattern", Some(pattern)), Ok(Value::Null));
    assert_eq!(
        s.manager.on_type_formatting_triggers(buffer),
        [']', ',', '}', ';', '\n']
    );

    // Missing or unreadable options, or a glob that doesn't build, are
    // refused, and nothing more is recorded.
    let bad_glob = json!({
        "documentSelector": [{ "pattern": "[" }],
        "firstTriggerCharacter": "{",
    });
    for options in [
        None,
        Some(json!({ "moreTriggerCharacter": ["{"] })),
        Some(bad_glob),
    ] {
        assert_eq!(register(&s.fake, "bad", options), Err(-32803));
    }
    assert_eq!(
        s.manager.registrations(s.server),
        ["any", "pattern", "python", "rust"]
    );

    // A registered character asks the server.
    s.fake.set_reply(
        ON_TYPE,
        Scripted::now(Reply::Result(json!([edit((1, 0), (1, 4), "")]))),
    );
    let edits = answer(
        s.manager
            .on_type_formatting(buffer, version, 17, '}', OPTIONS),
    );
    let row = "fn main() {\n".len();
    assert_eq!(edits, Ok(Some(vec![(row..row + 4, String::new())])));
    assert_eq!(requests(&s.fake, ON_TYPE).len(), 1);

    // Unregistered, its characters go.
    for id in ["rust", "any", "pattern"] {
        unregister(&s.fake, id);
    }
    assert!(s.manager.on_type_formatting_triggers(buffer).is_empty());
}

const OPTIONS: FormatOptions = FormatOptions {
    tab_size: 2,
    insert_spaces: false,
};

#[test]
fn request_carries_position_char_and_options() {
    // A crab before the caret: two UTF-16 units, four bytes.
    let text = "fn 🦀() {\n        x;\n        }\n";
    let provider = DocumentOnTypeFormattingOptions {
        first_trigger_character: "}".into(),
        more_trigger_character: Some(vec![";".into()]),
    };
    let s = started(Some(provider), text);
    let (buffer, version) = (s.buffer.id(), s.buffer.version());
    assert_eq!(s.manager.on_type_formatting_triggers(buffer), ['}', ';']);

    // The server reindents both rows.
    s.fake.set_reply(
        ON_TYPE,
        Scripted::now(Reply::Result(json!([
            edit((2, 0), (2, 8), ""),
            edit((1, 0), (1, 8), "\t"),
        ]))),
    );
    let caret = text.rfind('}').unwrap() + 1;
    let options = FormatOptions {
        tab_size: 3,
        insert_spaces: false,
    };
    let edits = answer(
        s.manager
            .on_type_formatting(buffer, version, caret, '}', options),
    );
    let second = text.find("        x").unwrap();
    let third = text.find("        }").unwrap();
    assert_eq!(
        edits,
        Ok(Some(vec![
            (second..second + 8, "\t".to_owned()),
            (third..third + 8, String::new()),
        ]))
    );
    let sent = requests(&s.fake, ON_TYPE).pop().unwrap();
    assert_eq!(sent["textDocument"]["uri"], json!(s.uri));
    assert_eq!(sent["position"], json!({ "line": 2, "character": 9 }));
    assert_eq!(sent["ch"], json!("}"));
    assert_eq!(sent["options"]["tabSize"], json!(3));
    assert_eq!(sent["options"]["insertSpaces"], json!(false));

    // In UTF-16 past the crab, with spaces.
    s.fake
        .set_reply(ON_TYPE, Scripted::now(Reply::Result(Value::Null)));
    let paren = text.find('(').unwrap();
    let options = FormatOptions {
        tab_size: 4,
        insert_spaces: true,
    };
    let edits = answer(
        s.manager
            .on_type_formatting(buffer, version, paren, ';', options),
    );
    assert_eq!(edits, Ok(Some(Vec::new())));
    let sent = requests(&s.fake, ON_TYPE).pop().unwrap();
    assert_eq!(sent["position"], json!({ "line": 0, "character": 5 }));
    assert_eq!(sent["ch"], json!(";"));
    assert_eq!(sent["options"]["tabSize"], json!(4));
    assert_eq!(sent["options"]["insertSpaces"], json!(true));

    // Another character sends nothing.
    let none = answer(
        s.manager
            .on_type_formatting(buffer, version, caret, ')', options),
    );
    assert_eq!(none, Ok(None));
    assert_eq!(requests(&s.fake, ON_TYPE).len(), 2);
}
