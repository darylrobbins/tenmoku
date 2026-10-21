// SPDX-License-Identifier: LicenseRef-Proprietary

//! Plugin-origin dispatches are authorized against the plugin's grants.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use kiln_command::{
    Actor, Capability, CommandTarget, DispatchError, Dispatched, Effect, Grants, Invocation,
    PluginAccess, PluginDenied, PluginPolicy, Registry,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

/// Commands that append what ran, and as whom, to a log, and can dispatch
/// others through the registry they're in.
struct Logged;

struct Run<'a> {
    registry: &'a Registry<Logged>,
    log: Vec<String>,
}

impl CommandTarget for Logged {
    type Tx = ();
    type Target<'a> = Run<'a>;
}

#[derive(Deserialize, JsonSchema)]
struct OpenArgs {
    /// Defaults outside any workspace, so leaving it out must not pass.
    #[serde(default = "outside_default")]
    path: String,
}

fn outside_default() -> String {
    "/etc/passwd".to_string()
}

#[derive(Deserialize, JsonSchema)]
struct DelegateArgs {
    to: String,
    /// Dispatch as the user rather than as the actor delegating.
    #[serde(default)]
    as_user: bool,
}

fn registry() -> Registry<Logged> {
    let mut registry = Registry::new();
    for id in ["test.read", "test.write", "test.secret"] {
        registry
            .register(id, id, move |run: &mut Run<'_>, actor: &Actor| {
                run.log.push(format!("{actor:?} ran {id}"));
                Ok(Dispatched::new(Effect::Changed))
            })
            .unwrap();
    }
    registry
        .register_with_args(
            "test.open",
            "Open",
            |run: &mut Run<'_>, actor: &Actor, args: OpenArgs| {
                run.log.push(format!("{actor:?} opened {}", args.path));
                Ok(Dispatched::new(Effect::Changed))
            },
        )
        .unwrap();
    registry
        .register_with_args(
            "test.delegate",
            "Delegate",
            |run: &mut Run<'_>, actor: &Actor, args: DelegateArgs| {
                let actor = if args.as_user {
                    Actor::User
                } else {
                    actor.clone()
                };
                let invocation = Invocation::of(&args.to).unwrap().actor(actor);
                let registry = run.registry;
                if let Err(error) = registry.dispatch(run, &invocation) {
                    run.log.push(format!("denied: {error}"));
                }
                Ok(Dispatched::new(Effect::Changed))
            },
        )
        .unwrap();
    registry
        .allow_plugins("test.read", PluginAccess::default())
        .unwrap();
    registry
        .allow_plugins("test.write", access(&["buffer.write"]))
        .unwrap();
    registry
        .allow_plugins(
            "test.open",
            PluginAccess {
                capabilities: vec![cap("fs.read:workspace")],
                path_args: vec!["path"],
            },
        )
        .unwrap();
    registry
        .allow_plugins("test.delegate", PluginAccess::default())
        .unwrap();
    registry
}

fn cap(capability: &str) -> Capability {
    Capability::parse(capability).unwrap()
}

fn access(capabilities: &[&str]) -> PluginAccess {
    PluginAccess {
        capabilities: capabilities.iter().map(|c| cap(c)).collect(),
        path_args: Vec::new(),
    }
}

struct Policy {
    grants: HashMap<String, Grants>,
    roots: Vec<PathBuf>,
}

impl PluginPolicy for Policy {
    fn grants(&self, plugin: &str) -> Option<Grants> {
        self.grants.get(plugin).cloned()
    }

    fn workspace_roots(&self) -> Vec<PathBuf> {
        self.roots.clone()
    }
}

fn policy(plugin: &str, capabilities: &[&str], roots: Vec<PathBuf>) -> Arc<Policy> {
    let grants = capabilities.iter().map(|c| cap(c)).collect();
    Arc::new(Policy {
        grants: HashMap::from([(plugin.to_string(), grants)]),
        roots,
    })
}

const EVERY_CAPABILITY: &[&str] = &[
    "buffer.read",
    "buffer.write",
    "commands",
    "decorations",
    "status",
    "fs.read:workspace",
    "fs.write:workspace",
    "process.spawn:*",
    "net:*",
];

fn plugin(command: &str) -> Invocation {
    Invocation::of(command)
        .unwrap()
        .actor(Actor::Host("acme.tool".into()))
}

/// Dispatches and returns the result and what ran.
fn dispatch(
    registry: &Registry<Logged>,
    invocation: &Invocation,
) -> (Result<Dispatched<()>, DispatchError>, Vec<String>) {
    let mut run = Run {
        registry,
        log: Vec::new(),
    };
    let result = registry.dispatch(&mut run, invocation);
    (result, run.log)
}

fn denied(result: Result<Dispatched<()>, DispatchError>) -> PluginDenied {
    match result {
        Err(DispatchError::PluginDenied(denied)) => denied,
        other => panic!("expected a denial, got {other:?}"),
    }
}

#[test]
fn a_host_actor_is_refused_with_no_policy() {
    let registry = registry();
    let (result, log) = dispatch(&registry, &plugin("test.read"));
    assert!(
        matches!(denied(result), PluginDenied::NoPolicy { .. }),
        "no policy, no plugin dispatch"
    );
    assert!(log.is_empty(), "ran {log:?}");
    assert!(registry.validate(&plugin("test.read")).is_err());
}

#[test]
fn an_undeclared_command_is_refused_to_plugins() {
    let mut registry = registry();
    registry.set_plugin_policy(policy("acme.tool", EVERY_CAPABILITY, Vec::new()));
    let (result, log) = dispatch(&registry, &plugin("test.secret"));
    let denial = denied(result);
    assert!(
        matches!(&denial, PluginDenied::Undeclared { command, .. } if command.as_str() == "test.secret"),
        "{denial:?}"
    );
    assert!(log.is_empty(), "ran {log:?}");
    // A plugin the policy doesn't know is refused too.
    let stranger = plugin("test.read").actor(Actor::Host("other.tool".into()));
    let (result, log) = dispatch(&registry, &stranger);
    assert!(matches!(denied(result), PluginDenied::UnknownPlugin { .. }));
    assert!(log.is_empty());
}

#[test]
fn needs_the_commands_capability_and_the_commands_own() {
    let mut registry = registry();
    let missing = |registry: &Registry<Logged>, command| {
        let (result, log) = dispatch(registry, &plugin(command));
        assert!(log.is_empty(), "ran {log:?}");
        match denied(result) {
            PluginDenied::MissingCapability { capability, .. } => capability,
            other => panic!("expected a missing capability, got {other:?}"),
        }
    };

    registry.set_plugin_policy(policy("acme.tool", &["buffer.write"], Vec::new()));
    assert_eq!(missing(&registry, "test.read"), Capability::Commands);
    assert_eq!(missing(&registry, "test.write"), Capability::Commands);

    registry.set_plugin_policy(policy("acme.tool", &["commands"], Vec::new()));
    assert_eq!(missing(&registry, "test.write"), Capability::BufferWrite);
    let error = dispatch(&registry, &plugin("test.write"))
        .0
        .unwrap_err()
        .to_string();
    assert!(error.contains("buffer.write"), "{error}");
    let (result, log) = dispatch(&registry, &plugin("test.read"));
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(log, ["Host(\"acme.tool\") ran test.read"]);

    registry.set_plugin_policy(policy(
        "acme.tool",
        &["commands", "buffer.write"],
        Vec::new(),
    ));
    let (result, log) = dispatch(&registry, &plugin("test.write"));
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(log, ["Host(\"acme.tool\") ran test.write"]);
}

/// A fresh directory under the target's temp dir, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("plugin_authorize-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn path_args_must_stay_inside_the_workspace() {
    let tmp = TempDir::new("paths");
    let root = tmp.0.join("root");
    let outside = tmp.0.join("outside");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(root.join("src/main.rs"), "").unwrap();
    fs::write(outside.join("secret"), "").unwrap();

    let mut registry = registry();
    registry.set_plugin_policy(policy(
        "acme.tool",
        &["commands", "fs.read:workspace"],
        vec![root.clone()],
    ));
    let open = |path: &Path| {
        let invocation = plugin("test.open").args(json!({ "path": path }));
        dispatch(&registry, &invocation)
    };
    let refused = |path: &Path| {
        let (result, log) = open(path);
        assert!(log.is_empty(), "{} ran {log:?}", path.display());
        assert!(
            matches!(
                denied(result),
                PluginDenied::OutsideWorkspace { arg: "path", .. }
            ),
            "{}",
            path.display()
        );
    };

    let inside = root.join("src/main.rs");
    let (result, log) = open(&inside);
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(log.len(), 1, "{log:?}");
    // Not yet existing, and with a `..` that stays inside: allowed.
    assert!(open(&root.join("src/new/../new.rs")).0.is_ok());

    refused(&root.join("src/../../outside/secret"));
    refused(&root.join("missing/../../outside/secret"));
    refused(&outside.join("secret"));
    refused(Path::new("/"));
    refused(Path::new("src/main.rs"));

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
        refused(&root.join("link/secret"));
        refused(&root.join("link/new_file"));
        // `..` after a symlink leaves where it leads, not the root.
        refused(&root.join("link/../outside/secret"));
        // A dangling symlink leads where creating through it would write.
        std::os::unix::fs::symlink(outside.join("new_file"), root.join("dangling")).unwrap();
        refused(&root.join("dangling"));
        std::os::unix::fs::symlink(outside.join("new_dir"), root.join("dangling_dir")).unwrap();
        refused(&root.join("dangling_dir/file"));
        // Relative targets resolve from the symlink's directory.
        std::os::unix::fs::symlink("../../outside", root.join("src/up")).unwrap();
        refused(&root.join("src/up/secret"));
        std::os::unix::fs::symlink("main.rs", root.join("src/alias")).unwrap();
        assert!(open(&root.join("src/alias")).0.is_ok());
        std::os::unix::fs::symlink("dangling_inside", root.join("src/new_alias")).unwrap();
        assert!(open(&root.join("src/new_alias")).0.is_ok());
        // A loop can't be checked, so it's refused.
        std::os::unix::fs::symlink(root.join("loop"), root.join("loop")).unwrap();
        refused(&root.join("loop/file"));
    }

    // A path arg left out could default to anywhere: refused, not run.
    for args in [json!({}), json!({ "path": null }), serde_json::Value::Null] {
        let (result, log) = dispatch(&registry, &plugin("test.open").args(args.clone()));
        assert!(log.is_empty(), "{args}: ran {log:?}");
        assert!(
            matches!(
                denied(result),
                PluginDenied::MissingPath { arg: "path", .. }
            ),
            "{args}"
        );
    }

    let not_a_string = plugin("test.open").args(json!({ "path": ["a"] }));
    assert!(matches!(
        denied(dispatch(&registry, &not_a_string).0),
        PluginDenied::OutsideWorkspace { .. }
    ));
}

#[test]
fn delegated_dispatch_keeps_the_actor_and_its_limits() {
    let mut registry = registry();
    registry.set_plugin_policy(policy("acme.tool", &["commands"], Vec::new()));
    let delegate = |to: &str, as_user: bool| {
        plugin("test.delegate").args(json!({ "to": to, "as_user": as_user }))
    };

    let (result, log) = dispatch(&registry, &delegate("test.read", false));
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(log, ["Host(\"acme.tool\") ran test.read"]);

    for as_user in [false, true] {
        let (result, log) = dispatch(&registry, &delegate("test.write", as_user));
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(log.len(), 1, "{log:?}");
        assert!(
            log[0].starts_with("denied: ") && log[0].contains("buffer.write"),
            "{log:?}"
        );

        let (result, log) = dispatch(&registry, &delegate("test.secret", as_user));
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(log.len(), 1, "{log:?}");
        assert!(log[0].contains("isn't open to plugins"), "{log:?}");
    }

    // Claiming to be the user inside a plugin's dispatch still runs as the
    // plugin.
    let (_, log) = dispatch(&registry, &delegate("test.read", true));
    assert_eq!(log, ["Host(\"acme.tool\") ran test.read"]);

    // Granted, the inner command runs, as the plugin.
    registry.set_plugin_policy(policy(
        "acme.tool",
        &["commands", "buffer.write"],
        Vec::new(),
    ));
    let (_, log) = dispatch(&registry, &delegate("test.write", false));
    assert_eq!(log, ["Host(\"acme.tool\") ran test.write"]);

    // Once the plugin's dispatch is done, the user is the user again.
    let user = Invocation::of("test.delegate")
        .unwrap()
        .args(json!({ "to": "test.secret" }));
    let (result, log) = dispatch(&registry, &user);
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(log, ["User ran test.secret"]);
}

#[test]
fn user_and_ai_actors_are_unaffected() {
    let actors = [
        Actor::User,
        Actor::Ai {
            agent: "claude".into(),
            session: "s1".into(),
        },
        Actor::Lsp,
        Actor::Format,
    ];
    let run_all = |registry: &Registry<Logged>| {
        let mut runs = Vec::new();
        for actor in &actors {
            for command in ["test.read", "test.write", "test.secret"] {
                let invocation = Invocation::of(command).unwrap().actor(actor.clone());
                let (result, log) = dispatch(registry, &invocation);
                assert!(result.is_ok(), "{actor:?} {command}: {result:?}");
                runs.push(log);
            }
            let outside = Invocation::of("test.open")
                .unwrap()
                .args(json!({ "path": "/outside/the/workspace" }))
                .actor(actor.clone());
            let (result, log) = dispatch(registry, &outside);
            assert!(result.is_ok(), "{actor:?}: {result:?}");
            runs.push(log);
        }
        runs
    };

    let before = run_all(&registry());
    let mut with_policy = registry();
    with_policy.set_plugin_policy(policy("acme.tool", &[], Vec::new()));
    assert_eq!(run_all(&with_policy), before);
    assert_eq!(before.len(), actors.len() * 4);
}
