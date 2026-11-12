// SPDX-License-Identifier: Apache-2.0

//! Which plugins load on which host, the world each minor runs against, and an adapter serving
//! an older minor.

use std::sync::{
    Arc,
    mpsc::{self, Receiver},
};

use kiln_command::{Capability, Grants};
use kiln_core::ApiVersion;
use kiln_plugin::{
    bindings::HostState,
    commands::{CommandHost, Dispatcher, Request, State},
    compat::{Adapter, Compat, HOST_API, Incompatible, World, link_adapters, world_for},
    engine::PluginEngine,
    limits::Limits,
    runner::{Call, LoadError, Plugin, Reply},
};
use wasmtime::{
    Config, Engine, Store,
    component::{Component, Linker, Val},
};

/// A component built for API 1.0, checked in as released: it registers `acme.tool.hello` in its
/// `init`, and logs a command's id and arguments when it runs.
const COMMANDS_1_0: &str = include_str!("fixtures/commands.wat");

/// The fixture's plugin id.
const PLUGIN: &str = "acme.tool";

fn v(major: u32, minor: u32) -> ApiVersion {
    ApiVersion { major, minor }
}

fn host(minor: u32) -> Compat {
    Compat {
        host: v(1, minor),
        deprecated: &[],
    }
}

#[test]
fn an_older_minor_loads() {
    assert_eq!(host(1).check(v(1, 0)), Ok(()));
    assert_eq!(host(1).check(v(1, 1)), Ok(()));
    assert_eq!(Compat::CURRENT.check(HOST_API), Ok(()));
}

#[test]
fn a_newer_minor_is_refused_naming_both() {
    let error = host(1).check(v(1, 2)).unwrap_err();
    assert_eq!(
        error,
        Incompatible::NeedsNewerHost {
            needed: v(1, 2),
            host: v(1, 1)
        }
    );
    let message = error.to_string();
    assert!(
        message.contains("1.2") && message.contains("1.1"),
        "{message}"
    );
}

#[test]
fn another_major_is_refused() {
    let error = host(1).check(v(2, 0)).unwrap_err();
    assert_eq!(
        error,
        Incompatible::UnsupportedMajor {
            needed: v(2, 0),
            host: v(1, 1)
        }
    );
    let message = error.to_string();
    assert!(
        message.contains("2.0") && message.contains("1.1"),
        "{message}"
    );
}

#[test]
fn a_deprecated_major_loads_until_its_window_ends() {
    static TABLE: &[(u32, ApiVersion)] = &[(1, ApiVersion { major: 2, minor: 1 })];
    let on = |host| Compat {
        host,
        deprecated: TABLE,
    };
    assert_eq!(on(v(2, 0)).check(v(1, 0)), Ok(()));
    assert_eq!(on(v(2, 1)).check(v(1, 3)), Ok(()));
    assert!(matches!(
        on(v(2, 2)).check(v(1, 0)),
        Err(Incompatible::UnsupportedMajor { .. })
    ));
}

#[test]
fn an_adapter_serves_a_one_zero_plugin_on_a_one_one_host() {
    let compat = host(1);
    let adapters = [Adapter::<Vec<String>> {
        serves: v(1, 0),
        link: |linker| {
            linker
                .instance("kiln:extension/log@1.0.0")?
                .func_new("log", |mut cx, _, args, _| {
                    let [Val::Enum(level), Val::String(message)] = args else {
                        panic!("unexpected arguments {args:?}");
                    };
                    cx.data_mut().push(format!("{level}:{message}"));
                    Ok(())
                })
        },
    }];

    let mut config = Config::new();
    config.wasm_component_model(true);
    let engine = Engine::new(&config).unwrap();
    let component = Component::new(&engine, include_str!("fixtures/log_v1_0.wat")).unwrap();

    let bare = Linker::<Vec<String>>::new(&engine);
    assert!(
        bare.instantiate(Store::new(&engine, vec![]), &component)
            .is_err()
    );

    let mut linker = Linker::<Vec<String>>::new(&engine);
    link_adapters(&compat, &adapters, &mut linker).unwrap();
    let mut store = Store::new(&engine, vec![]);
    let instance = linker.instantiate(&mut store, &component).unwrap();
    instance
        .get_typed_func::<(), ()>(&mut store, "run")
        .unwrap()
        .call(&mut store, ())
        .unwrap();
    assert_eq!(store.data(), &["info:hi".to_string()]);
}

/// [`COMMANDS_1_0`], as built against 1.1's WIT: every interface it imports and exports at
/// `@1.1.0`. 1.1 adds nothing it uses, so it is the same component otherwise.
fn commands_1_1() -> String {
    COMMANDS_1_0.replace("@1.0.0", "@1.1.0")
}

/// Answers no request: the fixture's `hello` asks for none.
struct Unanswered;

impl Dispatcher for Unanswered {
    fn dispatch(&self, _request: Request) {}
}

/// `component` loaded as [`PLUGIN`], with `commands`, as a plugin needing `needed` on
/// `compat`'s host, and what it logs.
fn load(
    component: &str,
    compat: &Compat,
    needed: ApiVersion,
) -> (Result<Plugin, LoadError>, Receiver<String>) {
    let grants: Grants = [Capability::Commands].into_iter().collect();
    let host = CommandHost::new([], Arc::new(Unanswered));
    let (sender, logged) = mpsc::channel();
    let mut state = HostState::new(Box::new(move |_, message| drop(sender.send(message))));
    state.commands = State::new(PLUGIN, &grants, &host).unwrap();
    let plugin = Plugin::load_for(
        PluginEngine::shared().unwrap(),
        None,
        component.as_bytes(),
        PLUGIN,
        &grants,
        Limits::default(),
        state,
        compat,
        needed,
    );
    (plugin, logged)
}

/// Runs the fixture's `acme.tool.hello` and delivers it no events, as a host does.
fn answers(plugin: &mut Plugin, logged: &Receiver<String>) {
    let reply = plugin.call(Call::RunCommand {
        id: "acme.tool.hello".into(),
        args: "{\"n\":1}".into(),
    });
    assert_eq!(reply, Ok(Reply::RunCommand(Ok(()))));
    let lines: Vec<String> = logged.try_iter().collect();
    assert_eq!(lines, ["acme.tool.hello", "{\"n\":1}"]);
    assert_eq!(
        plugin.call(Call::HandleEvents(Vec::new())),
        Ok(Reply::HandleEvents)
    );
}

#[test]
fn one_generated_world_per_supported_minor() {
    assert_eq!(HOST_API, v(1, 1));
    assert_eq!(World::CURRENT.api(), HOST_API);
    assert_eq!(World::ALL.last(), Some(&World::CURRENT));
    for minor in 0..=HOST_API.minor {
        let world = world_for(v(1, minor)).unwrap();
        assert_eq!(world.api(), v(1, minor));
        assert_eq!(
            world.interface("command-handler"),
            format!("kiln:extension/command-handler@1.{minor}.0")
        );
    }
    assert_eq!(world_for(v(1, 0)), Ok(World::V1_0));
    assert_eq!(world_for(v(1, 1)), Ok(World::V1_1));
    assert_eq!(
        world_for(v(1, 2)),
        Err(Incompatible::NeedsNewerHost {
            needed: v(1, 2),
            host: v(1, 1)
        })
    );
    assert_eq!(
        host(0).world_for(v(1, 1)),
        Err(Incompatible::NeedsNewerHost {
            needed: v(1, 1),
            host: v(1, 0)
        })
    );
    // A host said to be newer than this build has no world for what it would admit.
    assert_eq!(
        host(3).world_for(v(1, 2)),
        Err(Incompatible::NeedsNewerHost {
            needed: v(1, 2),
            host: v(1, 1)
        })
    );
}

#[test]
fn compat_1_0_plugin_runs_on_1_1_host() {
    assert!(
        COMMANDS_1_0.contains("kiln:extension/command-handler@1.0.0")
            && !COMMANDS_1_0.contains("@1.1"),
        "the fixture is a 1.0 component"
    );
    assert_eq!(Compat::CURRENT.host, v(1, 1));
    let (plugin, logged) = load(COMMANDS_1_0, &Compat::CURRENT, v(1, 0));
    let mut plugin = plugin.unwrap();
    answers(&mut plugin, &logged);

    // As the current world, too: a 1.0 component has all of 1.1's exports while 1.1 adds none.
    let (plugin, logged) = load(COMMANDS_1_0, &Compat::CURRENT, v(1, 1));
    answers(&mut plugin.unwrap(), &logged);
}

#[test]
fn compat_pinned_1_0_host_refuses_1_1_plugin() {
    let built = commands_1_1();
    // It runs on a 1.1 host.
    let (plugin, logged) = load(&built, &Compat::CURRENT, v(1, 1));
    answers(&mut plugin.unwrap(), &logged);

    let pinned = host(0);
    let (refused, logged) = load(&built, &pinned, v(1, 1));
    let Err(error) = refused else {
        panic!("a host pinned to 1.0 loaded a 1.1 plugin");
    };
    let LoadError::Incompatible { plugin, error: why } = &error else {
        panic!("not refused as incompatible: {error:?}");
    };
    assert_eq!(plugin, PLUGIN);
    assert_eq!(
        *why,
        Incompatible::NeedsNewerHost {
            needed: v(1, 1),
            host: v(1, 0)
        }
    );
    let message = error.to_string();
    assert!(
        message.contains(PLUGIN) && message.contains(&why.to_string()),
        "{message}"
    );
    assert!(
        message.contains("1.1") && message.contains("1.0"),
        "{message}"
    );
    assert_eq!(logged.try_iter().count(), 0, "its init ran");

    // The pinned host still runs a 1.0 plugin.
    let (plugin, logged) = load(COMMANDS_1_0, &pinned, v(1, 0));
    answers(&mut plugin.unwrap(), &logged);
}
