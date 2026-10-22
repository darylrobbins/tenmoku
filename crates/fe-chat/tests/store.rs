use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use fe_chat::{BlockKind, ChatEvent, ChatId, ChatSettings, ChatStore, Conversation, StopReason};
use serde_json::json;

/// A fresh directory under the test target dir.
fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("store-{name}"));
    fs::remove_dir_all(&dir).ok();
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn chat(title_line: &str) -> Conversation {
    let mut chat = Conversation::new(&ChatSettings::default());
    chat.begin_turn(
        vec![json!({"type": "text", "text": title_line})],
        Vec::new(),
    );
    let thinking = json!({"type": "thinking", "thinking": "Hmm.", "signature": "EqQB=="});
    let text = json!({"type": "text", "text": "Done."});
    for event in [
        ChatEvent::BlockStarted {
            index: 0,
            kind: BlockKind::Thinking,
        },
        ChatEvent::BlockFinished {
            index: 0,
            block: thinking,
        },
        ChatEvent::BlockStarted {
            index: 1,
            kind: BlockKind::Text,
        },
        ChatEvent::BlockFinished {
            index: 1,
            block: text,
        },
        ChatEvent::Finished {
            stop_reason: StopReason::EndTurn,
            stop_details: None,
        },
    ] {
        chat.apply(event);
    }
    chat
}

fn age(path: &std::path::Path, seconds: u64) {
    let file = fs::File::options().write(true).open(path).unwrap();
    file.set_modified(SystemTime::now() - Duration::from_secs(seconds))
        .unwrap();
}

#[test]
fn chats_save_atomically_and_list_newest_first() {
    let root = scratch("chats");
    let dir = root.join("chats").join("folder");
    let store = ChatStore::new(&dir);
    let (old, new) = (chat("old question"), chat("new question"));

    store.save(&old).unwrap();
    store.save(&new).unwrap();
    age(&dir.join(format!("{}.json", old.id)), 100);

    assert!(!dir.join(format!("{}.json.tmp", old.id)).exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = |path: PathBuf| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(dir.clone()), 0o700);
        assert_eq!(mode(dir.join(format!("{}.json", new.id))), 0o600);
    }

    let loaded = store.load(new.id).unwrap();
    assert_eq!(loaded, new);
    assert_eq!(
        serde_json::to_vec(&loaded).unwrap(),
        serde_json::to_vec(&new).unwrap()
    );
    for (a, b) in loaded.turns[0].reply.iter().zip(&new.turns[0].reply) {
        assert_eq!(
            serde_json::to_string(&a.block).unwrap(),
            serde_json::to_string(&b.block).unwrap()
        );
    }

    let listed = store.list();
    let ids: Vec<ChatId> = listed.iter().map(|summary| summary.id).collect();
    assert_eq!(ids, [new.id, old.id]);
    assert_eq!(listed[0].title, "new question");

    // Saving again replaces the file.
    store.save(&new).unwrap();
    assert_eq!(store.list().len(), 2);

    store.delete(old.id).unwrap();
    assert!(!dir.join(format!("{}.json", old.id)).exists());
    assert_eq!(store.list().len(), 1);
    store.delete(old.id).unwrap();
}

#[test]
fn bad_chat_files_are_skipped_not_deleted() {
    let dir = scratch("bad");
    let store = ChatStore::new(&dir);
    let good = chat("fine");
    store.save(&good).unwrap();

    let garbled = ChatId::new();
    let garbled_file = dir.join(format!("{garbled}.json"));
    fs::write(&garbled_file, "{ not json").unwrap();
    let newer = ChatId::new();
    let newer_file = dir.join(format!("{newer}.json"));
    fs::write(
        &newer_file,
        r#"{"version": 2, "conversation": {"future": true}}"#,
    )
    .unwrap();

    let ids: Vec<ChatId> = store.list().iter().map(|summary| summary.id).collect();
    assert_eq!(ids, [good.id]);
    assert!(garbled_file.exists());
    assert!(newer_file.exists());

    for (id, file) in [(garbled, &garbled_file), (newer, &newer_file)] {
        let error = store.load(id).unwrap_err().to_string();
        assert!(error.contains(&file.display().to_string()), "{error}");
    }
}

#[test]
fn concurrent_saves_of_one_chat_leave_one_complete_file() {
    let dir = scratch("concurrent");
    let chat = chat("shared");
    let threads: Vec<_> = (0..4)
        .map(|_| {
            let (store, chat) = (ChatStore::new(&dir), chat.clone());
            std::thread::spawn(move || {
                for _ in 0..25 {
                    store.save(&chat).unwrap();
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    let store = ChatStore::new(&dir);
    assert_eq!(store.load(chat.id).unwrap(), chat);
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
}

#[test]
fn saves_are_compare_and_swap_on_the_revision() {
    let dir = scratch("revision");
    let store = ChatStore::new(&dir);
    let mut chat = chat("first");

    // A new chat is at revision 0; each accepted save bumps it.
    assert_eq!(store.save_if(&chat, 0).unwrap(), 1);
    assert_eq!(store.load_revision(chat.id).unwrap().1, 1);
    assert_eq!(store.save_if(&chat, 1).unwrap(), 2);

    // Written from an older revision: refused, and the file is as it was.
    let mut other = chat.clone();
    other.title = "other branch".to_owned();
    let error = store.save_if(&other, 1).unwrap_err();
    assert!(matches!(
        error,
        fe_chat::StoreError::Diverged { current: 2, .. }
    ));
    chat.title = "first".to_owned();
    assert_eq!(store.load(chat.id).unwrap().title, "first");
    assert_eq!(store.load_revision(chat.id).unwrap().1, 2);

    // An unconditional save goes on from the current revision.
    store.save(&other).unwrap();
    assert_eq!(store.load_revision(chat.id).unwrap().1, 3);
}
