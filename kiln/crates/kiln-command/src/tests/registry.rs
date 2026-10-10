// SPDX-License-Identifier: LicenseRef-Proprietary

use std::{cell::RefCell, rc::Rc};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::*;

/// Commands that append to a log of strings.
struct Log;

impl CommandTarget for Log {
    type Tx = usize;
    type Target<'a> = Vec<String>;
}

#[derive(Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
struct Say {
    text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    times: Option<u32>,
}

fn registry() -> Registry<Log> {
    let mut registry = Registry::new();
    registry
        .register("test.clear", "Clear", |log: &mut Vec<String>, _: &Actor| {
            log.clear();
            Ok(Dispatched::new(Effect::Changed))
        })
        .unwrap()
        .context(ContextPredicate::parse("log && !read_only").unwrap())
        .mutates_buffer(true);
    registry
        .register_with_args(
            "test.say",
            "Say",
            |log: &mut Vec<String>, actor: &Actor, say: Say| {
                for _ in 0..say.times.unwrap_or(1) {
                    log.push(format!("{actor:?}: {}", say.text));
                }
                Ok(Dispatched::new(Effect::Changed).with_transactions(vec![log.len()]))
            },
        )
        .unwrap()
        .description("Appends text");
    registry
        .register("test.fail", "Fail", |_: &mut Vec<String>, _: &Actor| {
            Err(CommandError::new("no"))
        })
        .unwrap();
    registry
}

fn invocation(command: &str) -> Invocation {
    Invocation::of(command)
        .unwrap()
        .context(ContextStack::new().with(ContextFrame::new("log")))
}

#[test]
fn ids_args_and_metadata() {
    for bad in [
        "Editor.x",
        "editor",
        "editor..x",
        "editor.",
        ".x",
        "editor.1x",
        "editor.move-left",
    ] {
        assert!(CommandId::parse(bad).is_err(), "{bad} parsed");
    }
    let id = CommandId::parse("editor.move_left").unwrap();
    assert_eq!(
        (id.as_str(), id.namespace()),
        ("editor.move_left", "editor")
    );
    assert!(CommandId::parse("a.b2.c_d").is_ok());

    let mut registry = registry();
    let duplicate = registry.register("test.clear", "Again", |_: &mut Vec<String>, _: &Actor| {
        Ok(Dispatched::new(Effect::Nothing))
    });
    assert!(matches!(duplicate, Err(RegisterError::Duplicate(_))));
    assert!(matches!(
        registry.register("Test.x", "Bad", |_: &mut Vec<String>, _: &Actor| Ok(
            Dispatched::new(Effect::Nothing)
        )),
        Err(RegisterError::InvalidId(_))
    ));

    let commands: Vec<_> = registry.commands().collect();
    let ids: Vec<_> = commands.iter().map(|meta| meta.id.as_str()).collect();
    assert_eq!(ids, ["test.clear", "test.say", "test.fail"]);
    let (clear, say) = (commands[0], commands[1]);
    assert_eq!(clear.title, "Clear");
    assert_eq!(
        clear.context.as_ref().unwrap().to_string(),
        "log && !read_only"
    );
    assert!(clear.mutates_buffer);
    assert_eq!(clear.args_schema, None);
    assert_eq!(
        (say.title.as_str(), say.description.as_str()),
        ("Say", "Appends text")
    );
    assert_eq!(say.context, None);
    assert!(!say.mutates_buffer);
    let schema = say.args_schema.as_ref().unwrap();
    assert_eq!(schema["type"], "object");
    assert!(schema["properties"]["text"].is_object(), "{schema}");
    assert!(schema["properties"]["times"].is_object(), "{schema}");
    assert_eq!(schema["required"], json!(["text"]));

    // validate() checks without running; dispatch() reports the same.
    let mut log = vec!["kept".to_string()];
    let unknown = invocation("test.nope");
    assert!(matches!(
        registry.validate(&unknown),
        Err(DispatchError::UnknownCommand(_))
    ));
    let bad_args = [
        invocation("test.say"),
        invocation("test.say").args(json!({"text": 1})),
        invocation("test.clear").args(json!({"x": 1})),
    ];
    for bad in &bad_args {
        assert!(
            matches!(registry.validate(bad), Err(DispatchError::BadArgs { .. })),
            "{bad:?}"
        );
        assert!(matches!(
            registry.dispatch(&mut log, bad),
            Err(DispatchError::BadArgs { .. })
        ));
    }
    let read_only = invocation("test.clear")
        .context(ContextStack::new().with(ContextFrame::new("log").flag("read_only")));
    assert!(matches!(
        registry.validate(&read_only),
        Err(DispatchError::NotInContext { .. })
    ));
    assert!(matches!(
        registry.dispatch(&mut log, &read_only),
        Err(DispatchError::NotInContext { .. })
    ));
    assert!(matches!(
        registry.validate(&Invocation::of("test.clear").unwrap()),
        Err(DispatchError::NotInContext { .. })
    ));
    assert_eq!(log, ["kept"]);

    let clear = invocation("test.clear");
    assert_eq!(registry.validate(&clear).unwrap().id.as_str(), "test.clear");
    assert_eq!(
        registry.dispatch(&mut log, &clear).unwrap().effect,
        Effect::Changed
    );
    assert!(log.is_empty());
    let say = invocation("test.say").args(json!({"text": "hi", "times": 2}));
    let dispatched = registry.dispatch(&mut log, &say).unwrap();
    assert_eq!(dispatched.transactions, [2]);
    assert_eq!(log, ["User: hi", "User: hi"]);
    assert!(matches!(
        registry.dispatch(&mut log, &invocation("test.fail")),
        Err(DispatchError::Failed { error, .. }) if error.0 == "no"
    ));
}

#[test]
fn invocations_round_trip() {
    let invocation = Invocation::of("test.say")
        .unwrap()
        .args(
            serde_json::to_value(Say {
                text: "hi".into(),
                times: Some(3),
            })
            .unwrap(),
        )
        .actor(Actor::Ai {
            agent: "claude".into(),
            session: "s1".into(),
        })
        .source(Source::Script)
        .target(Target::View(7))
        .context(
            ContextStack::new()
                .with(ContextFrame::new("workbench"))
                .with(
                    ContextFrame::new("editor")
                        .flag("read_only")
                        .set("vim.mode", "normal"),
                ),
        );
    let line = serde_json::to_string(&invocation).unwrap();
    assert!(!line.contains('\n'));
    assert_eq!(
        serde_json::from_str::<Invocation>(&line).unwrap(),
        invocation
    );
    assert!(
        line.contains(r#""actor":{"ai":{"agent":"claude","session":"s1"}}"#),
        "{line}"
    );
    assert!(line.contains(r#""target":{"view":7}"#), "{line}");

    // Args are a table, so they're TOML too.
    let table = toml::to_string(&invocation.args).unwrap();
    assert_eq!(toml::from_str::<Value>(&table).unwrap(), invocation.args);
    let say: Say = toml::from_str("text = \"hi\"").unwrap();
    assert_eq!(
        say,
        Say {
            text: "hi".into(),
            times: None
        }
    );
    assert!(toml::to_string(&serde_json::to_value(&say).unwrap()).is_ok());

    let no_args = Invocation::of("test.clear").unwrap();
    let line = serde_json::to_string(&no_args).unwrap();
    assert_eq!(
        line,
        r#"{"command":"test.clear","actor":"user","source":"test","target":"focused","context":[]}"#
    );
    assert_eq!(serde_json::from_str::<Invocation>(&line).unwrap(), no_args);
    // The spec's example line, with defaults filled in where it leaves fields out.
    let example: Invocation = serde_json::from_str(
        r#"{"command":"editor.move","args":{"motion":"word_left","extend":true,"count":3},"actor":"user","source":"keyboard","target":"focused","context":[{"name":"editor"}]}"#,
    )
    .unwrap();
    assert_eq!(example.source, Source::Keyboard);
    assert_eq!(example.context.frames()[0], ContextFrame::new("editor"));
    let minimal: Invocation = serde_json::from_str(r#"{"command":"app.quit"}"#).unwrap();
    assert_eq!(minimal, Invocation::of("app.quit").unwrap());
    assert!(serde_json::from_str::<Invocation>(r#"{"command":"Quit"}"#).is_err());

    let mut registry = registry();
    let seen = Rc::new(RefCell::new(Vec::new()));
    registry.observe({
        let seen = seen.clone();
        move |invocation| seen.borrow_mut().push(invocation.clone())
    });
    let mut log = Vec::new();
    let ok = self::invocation("test.say").args(json!({"text": "a"}));
    let failed = [
        self::invocation("test.say"),
        self::invocation("test.fail"),
        self::invocation("test.nope"),
        Invocation::of("test.clear").unwrap(),
    ];
    registry.dispatch(&mut log, &ok).unwrap();
    for failed in &failed {
        assert!(registry.dispatch(&mut log, failed).is_err());
    }
    let clear = self::invocation("test.clear");
    registry.dispatch(&mut log, &clear).unwrap();
    assert_eq!(*seen.borrow(), [ok, clear]);
}

#[test]
fn args_that_are_not_a_null_free_table_run_nothing() {
    let mut registry = registry();
    let seen = Rc::new(RefCell::new(Vec::new()));
    registry.observe({
        let seen = seen.clone();
        move |invocation| seen.borrow_mut().push(invocation.clone())
    });
    let mut log = vec!["kept".to_string()];
    for args in [
        // Serde reads these into `Say` on its own.
        json!(["hi"]),
        json!(["hi", 2]),
        json!({"text": "hi", "times": null}),
        json!({"text": "hi", "ignored": null}),
        json!({"text": "hi", "ignored": [1, null]}),
        json!({"text": "hi", "ignored": {"deep": null}}),
        json!("hi"),
        json!(1),
    ] {
        let bad = invocation("test.say").args(args);
        assert!(
            matches!(registry.validate(&bad), Err(DispatchError::BadArgs { .. })),
            "{bad:?}"
        );
        assert!(
            matches!(
                registry.dispatch(&mut log, &bad),
                Err(DispatchError::BadArgs { .. })
            ),
            "{bad:?}"
        );
    }
    assert_eq!(log, ["kept"]);
    assert!(seen.borrow().is_empty());

    // Null args are still an empty table, and a table without nulls runs.
    let ok = invocation("test.say").args(json!({"text": "hi", "ignored": [1]}));
    registry.dispatch(&mut log, &ok).unwrap();
    assert_eq!(*seen.borrow(), [ok]);
}
