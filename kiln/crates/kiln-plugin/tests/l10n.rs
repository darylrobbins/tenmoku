// SPDX-License-Identifier: Apache-2.0

//! kiln-plugin's catalogue: the literal lint (kiln-l10n), no hard-coded
//! user-facing text in this crate's non-test source outside
//! `l10n-allow.toml`, and every key it uses exists; then, in en-US, each
//! message as the text it replaced, byte for byte, and the errors and
//! prompt text that pick their message by kind.

use std::time::Duration;

use kiln_command::capability::{Capability, FsScope, Target};
use kiln_core::ApiVersion;
use kiln_l10n::{set_locales, tr};
use kiln_plugin::{
    compat::Incompatible,
    grants::{CheckTimedOut, SuspendReason, Suspended},
    manifest::WasmPathError,
    process::SpawnError,
    runner::{CallError, Diagnostic, Reason},
    warnings::{Warning, describe},
};

#[test]
fn ui_text_is_in_the_catalogue() {
    kiln_l10n::lint::literals(env!("CARGO_MANIFEST_DIR"))
        .allow_list("l10n-allow.toml")
        .check();
}

#[test]
fn keys_used_here_exist() {
    kiln_l10n::lint::keys(env!("CARGO_MANIFEST_DIR"), &kiln_plugin::l10n::RESOURCES).check();
}

/// Each message with arguments, formatted as kiln-plugin formats it,
/// against the text it wrote before it was a message.
#[test]
fn en_us_messages_are_unchanged() {
    set_locales(&[]);
    let messages = [
        (
            tr!(
                "kiln-plugin-grants-io",
                path = "/s/plugin-grants.toml",
                reason = "denied"
            ),
            "/s/plugin-grants.toml: denied",
        ),
        (
            tr!(
                "kiln-plugin-grants-unreadable",
                path = "/s/g.toml",
                reason = "bad key"
            ),
            "/s/g.toml isn't a grants file this app understands: bad key",
        ),
        (
            tr!(
                "kiln-plugin-grants-wildcard-not-accepted",
                grants = "Grants([Net(Any)])"
            ),
            "granting Grants([Net(Any)]) needs its own acceptance of the wildcard",
        ),
        (
            tr!("kiln-plugin-grants-program-not-resolved", program = "rg"),
            "process.spawn:rg has no program resolved for it",
        ),
        (
            tr!("kiln-plugin-grants-program-suspended", program = "rg"),
            "process.spawn:rg is suspended until it is granted again",
        ),
        (
            tr!(
                "kiln-plugin-grants-program-unbound",
                program = "rg",
                reason = "gone"
            ),
            "process.spawn:rg can't be bound to a program: gone",
        ),
        (
            tr!("kiln-plugin-program-not-a-name", name = "\"..\""),
            "\"..\" is not a program name: it can't be empty, `.`, `..` or hold a `/`",
        ),
        (
            tr!("kiln-plugin-program-not-found", name = "rg"),
            "rg isn't an executable file in any absolute PATH directory",
        ),
        (
            tr!(
                "kiln-plugin-program-unreadable",
                path = "/bin/rg",
                reason = "denied"
            ),
            "can't read /bin/rg: denied",
        ),
        (
            tr!("kiln-plugin-program-not-executable"),
            "not an executable file",
        ),
        (
            tr!(
                "kiln-plugin-spawn-failed",
                path = "/bin/rg",
                reason = "denied"
            ),
            "can't run /bin/rg: denied",
        ),
        (
            tr!("kiln-plugin-manifest-workspace", id = "acme.lint"),
            "a workspace extension cannot install a plugin (acme.lint)",
        ),
        (
            tr!("kiln-plugin-manifest-more-than-one", id = "acme.lint"),
            "acme.lint: [contributes.plugin] must be a single table",
        ),
        (
            tr!("kiln-plugin-manifest-missing-wasm", id = "acme.lint"),
            "acme.lint: [contributes.plugin] needs a `wasm` string",
        ),
        (
            tr!(
                "kiln-plugin-manifest-capabilities-not-strings",
                id = "acme.lint"
            ),
            "acme.lint: `capabilities` must be an array of strings",
        ),
        (
            tr!(
                "kiln-plugin-manifest-signature-not-string",
                id = "acme.lint"
            ),
            "acme.lint: `signature` must be a string",
        ),
        (
            tr!(
                "kiln-plugin-manifest-capability",
                id = "acme.lint",
                reason = "unknown"
            ),
            "acme.lint: unknown",
        ),
        (
            tr!(
                "kiln-plugin-manifest-wasm-path",
                id = "acme.lint",
                path = "\"../x.wasm\"",
                reason = "is not a file"
            ),
            "acme.lint: wasm path \"../x.wasm\": is not a file",
        ),
        (
            tr!("kiln-plugin-no-engine", reason = "no ticker"),
            "no plugin engine: no ticker",
        ),
        (
            tr!("kiln-plugin-no-deadline-thread", reason = "EAGAIN"),
            "no thread to enforce plugins' deadlines: EAGAIN",
        ),
        (
            tr!(
                "kiln-plugin-too-large",
                plugin = "acme.lint",
                bytes = 9000usize,
                limit = 8000usize
            ),
            "plugin acme.lint: the component is 9000 bytes, over the 8000-byte limit",
        ),
        (
            tr!(
                "kiln-plugin-invalid",
                plugin = "acme.lint",
                reason = "bad magic"
            ),
            "plugin acme.lint: not a component this host runs: bad magic",
        ),
        (
            tr!(
                "kiln-plugin-no-thread",
                plugin = "acme.lint",
                reason = "EAGAIN"
            ),
            "plugin acme.lint: no thread to run it on: EAGAIN",
        ),
        (
            tr!(
                "kiln-plugin-init-failed",
                plugin = "acme.lint",
                reason = "Failed(\"x\")"
            ),
            "plugin acme.lint: init failed: Failed(\"x\")",
        ),
        (
            tr!("kiln-plugin-thread-stopped-loading"),
            "its thread stopped while loading it",
        ),
        (tr!("kiln-plugin-thread-stopped"), "its thread stopped"),
        (
            tr!(
                "kiln-plugin-no-export",
                export = "kiln:extension/command-handler@1.0.0#run-command"
            ),
            "no export kiln:extension/command-handler@1.0.0#run-command",
        ),
        (
            tr!("kiln-plugin-call-deadline", deadline = "2s"),
            "a call ran past its 2s deadline",
        ),
        (
            tr!("kiln-plugin-already-loaded", plugin = "acme.lint"),
            "plugin acme.lint is already loaded",
        ),
        (
            tr!("kiln-plugin-unloaded-with-requests", plugin = "acme.lint"),
            "plugin acme.lint was unloaded with an invocation still out",
        ),
        (
            tr!("kiln-plugin-was-unloaded", plugin = "acme.lint"),
            "plugin acme.lint was unloaded",
        ),
        (
            tr!("kiln-plugin-command-not-run", command = "acme.lint.run"),
            "acme.lint.run was not run",
        ),
        (
            tr!("kiln-plugin-command-builtin", id = "editor.save"),
            "editor.save is a built-in command",
        ),
        (
            tr!(
                "kiln-plugin-command-taken",
                id = "acme.lint.run",
                owner = "acme.other"
            ),
            "acme.lint.run is already registered by plugin acme.other",
        ),
        (
            tr!("kiln-plugin-namespace-reserved", namespace = "editor"),
            "the editor namespace is reserved",
        ),
        (
            tr!(
                "kiln-plugin-command-plugin-unloaded",
                id = "acme.lint.run",
                plugin = "acme.lint"
            ),
            "acme.lint.run: plugin acme.lint was unloaded",
        ),
        (
            tr!(
                "kiln-plugin-command-plugin-invoking",
                id = "acme.lint.run",
                plugin = "acme.lint"
            ),
            "acme.lint.run: plugin acme.lint is waiting on a command it invoked",
        ),
        (
            tr!(
                "kiln-plugin-command-returned",
                id = "acme.lint.run",
                plugin = "acme.lint",
                error = "Failed(\"x\")"
            ),
            "acme.lint.run: plugin acme.lint returned Failed(\"x\")",
        ),
        (
            tr!(
                "kiln-plugin-command-answered",
                id = "acme.lint.run",
                plugin = "acme.lint",
                reply = "HandleEvents"
            ),
            "acme.lint.run: plugin acme.lint answered HandleEvents",
        ),
        (
            tr!(
                "kiln-plugin-command-failed",
                id = "acme.lint.run",
                reason = "trapped"
            ),
            "acme.lint.run: trapped",
        ),
        (
            tr!(
                "kiln-plugin-needs-capability",
                plugin = "acme.lint",
                capability = "commands",
                action = "register"
            ),
            "plugin acme.lint needs the commands capability to register commands",
        ),
        (
            tr!(
                "kiln-plugin-needs-capability",
                plugin = "acme.lint",
                capability = "commands",
                action = "invoke"
            ),
            "plugin acme.lint needs the commands capability to invoke commands",
        ),
        (
            tr!("kiln-plugin-register-after-init", plugin = "acme.lint"),
            "plugin acme.lint registers commands only during init",
        ),
        (
            tr!(
                "kiln-plugin-command-outside-namespace",
                id = "other.run",
                plugin = "acme.lint"
            ),
            "other.run is outside plugin acme.lint's namespace: its commands are acme.lint.<name>",
        ),
        (
            tr!(
                "kiln-plugin-command-args-schema",
                id = "acme.lint.run",
                reason = "EOF"
            ),
            "acme.lint.run: args-schema: EOF",
        ),
        (
            tr!(
                "kiln-plugin-command-when",
                id = "acme.lint.run",
                reason = "EOF"
            ),
            "acme.lint.run: when: EOF",
        ),
        (
            tr!(
                "kiln-plugin-command-args",
                id = "acme.lint.run",
                reason = "EOF"
            ),
            "acme.lint.run: args: EOF",
        ),
        (
            tr!("kiln-plugin-command-deadline", id = "editor.save"),
            "editor.save was not answered before the call's deadline",
        ),
        (tr!("kiln-plugin-net-none"), "the host has no network"),
        (
            tr!("kiln-plugin-net-timed-out", url = "https://a.test/"),
            "https://a.test/ timed out",
        ),
        (
            tr!(
                "kiln-plugin-net-wrong-peer",
                url = "https://a.test/",
                peer = "b.test",
                host = "a.test"
            ),
            "https://a.test/ connected to b.test, not a.test",
        ),
        (
            tr!(
                "kiln-plugin-net-too-large",
                url = "https://a.test/",
                limit = 1024usize
            ),
            "https://a.test/'s response is over 1024 bytes",
        ),
        (
            tr!(
                "kiln-plugin-net-too-many-redirects",
                url = "https://a.test/",
                limit = 5usize
            ),
            "https://a.test/ redirected more than 5 times",
        ),
        (
            tr!(
                "kiln-plugin-net-redirect-not-https",
                url = "https://a.test/",
                next = "http://a.test/"
            ),
            "https://a.test/ redirected to http://a.test/, which isn't https",
        ),
        (
            tr!(
                "kiln-plugin-net-not-a-url",
                url = "\"x\"",
                reason = "relative URL without a base"
            ),
            "\"x\" isn't a URL: relative URL without a base",
        ),
        (
            tr!("kiln-plugin-net-user-name", url = "\"https://u@a.test/\""),
            "\"https://u@a.test/\" has a user name, which isn't sent",
        ),
        (
            tr!("kiln-plugin-net-empty-label", url = "\"https://a..test/\""),
            "\"https://a..test/\" has an empty label in its host",
        ),
        (
            tr!("kiln-plugin-net-no-host", url = "\"data:x\""),
            "\"data:x\" has no host",
        ),
        (
            tr!("kiln-plugin-net-not-https", url = "\"http://a.test/\""),
            "\"http://a.test/\" isn't https (http only to 127.0.0.1 or [::1])",
        ),
        (
            tr!(
                "kiln-plugin-net-bad-host",
                url = "\"https://a.test./\"",
                reason = "invalid domain character"
            ),
            "\"https://a.test./\" invalid domain character",
        ),
        (
            tr!(
                "kiln-plugin-net-not-granted",
                url = "https://a.test/",
                host = "a.test"
            ),
            "https://a.test/: no net:a.test grant reaches its host",
        ),
        (
            tr!("kiln-plugin-net-locations", status = 302u16),
            "a 302 response has more than one Location",
        ),
        (
            tr!(
                "kiln-plugin-net-bad-redirect",
                url = "https://a.test/",
                location = "\"//\"",
                reason = "\"//\" has no host"
            ),
            "https://a.test/ redirected to \"//\": \"//\" has no host",
        ),
        (
            tr!("kiln-plugin-secret-name", max = 64usize),
            "a secret's name is 1 to 64 characters of a-z, 0-9, _ and -",
        ),
        (
            tr!("kiln-plugin-and-list", items = "a, b", last = "c"),
            "a, b and c",
        ),
    ];
    for (message, english) in messages {
        assert_eq!(message, english);
    }
    for (action, english) in [
        ("lock", "couldn't lock the secrets index /s/i.json: denied"),
        (
            "parse",
            "couldn't parse the secrets index /s/i.json: denied",
        ),
        ("read", "couldn't read the secrets index /s/i.json: denied"),
        (
            "serialize",
            "couldn't serialize the secrets index /s/i.json: denied",
        ),
        (
            "write",
            "couldn't write the secrets index /s/i.json: denied",
        ),
    ] {
        let message = tr!(
            "kiln-plugin-secrets-index",
            action = action,
            path = "/s/i.json",
            reason = "denied"
        );
        assert_eq!(message, english);
    }
}

/// The errors and prompt text that pick their message by kind, as they
/// read before they were messages.
#[test]
fn en_us_errors_and_prompt_text_are_unchanged() {
    set_locales(&[]);
    let v = |major, minor| ApiVersion { major, minor };
    assert_eq!(
        Incompatible::NeedsNewerHost {
            needed: v(1, 2),
            host: v(1, 0)
        }
        .to_string(),
        "plugin needs API 1.2, but this host only provides 1.0"
    );
    assert_eq!(
        Incompatible::UnsupportedMajor {
            needed: v(2, 0),
            host: v(1, 0)
        }
        .to_string(),
        "plugin needs API 2.0, but this host (1.0) does not support that major"
    );
    assert_eq!(
        CheckTimedOut.to_string(),
        "checking the program ran past its deadline"
    );
    assert_eq!(SuspendReason::Changed.to_string(), "the program changed");
    assert_eq!(SuspendReason::Missing.to_string(), "the program is gone");
    let suspended = Suspended {
        reason: SuspendReason::Changed,
    };
    assert_eq!(
        suspended.to_string(),
        "the program's grant is suspended: the program changed"
    );
    assert_eq!(
        SpawnError::NotGranted {
            program: "rg".into()
        }
        .to_string(),
        "process.spawn:rg is not granted"
    );
    assert_eq!(
        SpawnError::Suspended {
            program: "rg".into(),
            suspended
        }
        .to_string(),
        "process.spawn:rg: the program's grant is suspended: the program changed"
    );
    assert_eq!(
        SpawnError::TimedOut {
            program: "rg".into(),
            deadline: Duration::from_millis(300)
        }
        .to_string(),
        "rg ran past its 300ms deadline and was killed"
    );
    for (reason, english) in [
        (WasmPathError::Empty, "the path is empty"),
        (
            WasmPathError::Absolute,
            "must be relative to the extension's directory",
        ),
        (
            WasmPathError::Outside,
            "is outside the extension's directory",
        ),
        (
            WasmPathError::DirUnreadable,
            "the extension's directory is unreadable",
        ),
        (WasmPathError::Missing, "no such file"),
        (WasmPathError::NotAFile, "is not a file"),
    ] {
        assert_eq!(reason.to_string(), english);
    }
    for (reason, english) in [
        (Reason::Trap, "trapped"),
        (Reason::Timeout, "timed out"),
        (Reason::Memory, "out of memory"),
        (Reason::Protocol, "protocol error"),
    ] {
        assert_eq!(reason.to_string(), english);
        let diagnostic = Diagnostic {
            plugin: "acme.lint".into(),
            reason,
            message: "boom".into(),
        };
        assert_eq!(
            diagnostic.to_string(),
            format!("plugin acme.lint unloaded, {english}: boom")
        );
        assert_eq!(
            CallError::Failed(diagnostic).to_string(),
            format!("plugin acme.lint unloaded, {english}: boom")
        );
    }
    assert_eq!(CallError::Unloaded.to_string(), "the plugin is unloaded");

    let described = [
        (Capability::BufferRead, "read the text of your open files"),
        (Capability::BufferWrite, "edit your open files"),
        (Capability::Commands, "add commands, and run other commands"),
        (
            Capability::Decorations,
            "mark up text and the gutter in your files",
        ),
        (Capability::Status, "show items in the status bar"),
        (
            Capability::FsRead(FsScope::Workspace),
            "read any file in your workspace folders",
        ),
        (
            Capability::FsWrite(FsScope::Workspace),
            "write any file in your workspace folders",
        ),
        (
            Capability::ProcessSpawn(Target::Named("rg".into())),
            "run the program rg, outside the sandbox",
        ),
        (
            Capability::ProcessSpawn(Target::Any),
            "run any program, outside the sandbox",
        ),
        (
            Capability::Net(Target::Named("a.test".into())),
            "connect to a.test",
        ),
        (
            Capability::Net(Target::Any),
            "connect to any host on the internet",
        ),
    ];
    for (capability, english) in described {
        assert_eq!(describe(&capability), english);
    }
    let warnings = [
        (
            Warning::SendsCode {
                hosts: vec!["a.test".into(), "b.test".into(), "c.test".into()],
            },
            "It can read your code and send it to a.test, b.test and c.test.",
        ),
        (
            Warning::SendsCode {
                hosts: vec!["a.test".into()],
            },
            "It can read your code and send it to a.test.",
        ),
        (
            Warning::SendsAnywhere,
            "Extreme: with net:* it can send your code anywhere, to any host on the internet.",
        ),
        (
            Warning::OutsideSandbox {
                programs: vec!["rg".into(), "fd".into()],
            },
            "It runs rg and fd outside the sandbox, with your permissions.",
        ),
        (
            Warning::NearWildcard {
                program: "sh".into(),
            },
            "sh is near-wildcard: it can run any code, so granting it is almost process.spawn:*.",
        ),
        (
            Warning::RunsAnyProgram,
            "Extreme: with process.spawn:* it can run any program as you, outside the sandbox.",
        ),
    ];
    for (warning, english) in warnings {
        assert_eq!(warning.to_string(), english);
    }
}
