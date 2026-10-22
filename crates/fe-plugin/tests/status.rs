//! The `fastedit:editor/status` imports, called as a plugin's store calls them: what the host
//! keeps for the status bar, and that clicking an item runs its command as the plugin, authorized
//! by the plugin's grants.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use fe_plugin::HostState;
use fe_plugin::fastedit::editor::status::{Host as _, Item};
use fe_plugin::status::{Alignment, MAX_ITEMS_PER_PLUGIN, State, StatusItem, StatusItems};
use kiln_command::capability::{Capability, Grants};
use kiln_command::{
    Actor, CommandId, CommandTarget, DispatchError, Dispatched, Effect, Invocation, PluginAccess,
    PluginDenied, PluginPolicy, Registry, Source,
};
use kiln_plugin::bindings::Error;

const PLUGIN: &str = "acme";

fn grants(grants: &[&str]) -> Grants {
    grants
        .iter()
        .map(|capability| Capability::parse(capability).unwrap())
        .collect()
}

/// Plugin `plugin`'s store state, holding `granted`, on `items`.
fn plugin(plugin: &str, granted: &[&str], items: &StatusItems) -> HostState {
    HostState {
        status: State::new(plugin, &grants(granted), items),
        ..HostState::default()
    }
}

fn item(id: &str, text: &str, command: Option<&str>) -> Item {
    Item {
        id: id.into(),
        text: text.into(),
        tooltip: None,
        command: command.map(Into::into),
        alignment: Alignment::Left,
    }
}

#[test]
fn items_hold_text_tooltip_and_command() {
    let items = StatusItems::new();
    let mut state = plugin(PLUGIN, &["status"], &items);
    let mut branch = item("branch", "main", Some("acme.show_branch"));
    branch.tooltip = Some("The current branch".into());
    state.set_item(branch).unwrap();
    let expected = StatusItem {
        plugin: PLUGIN.into(),
        id: "branch".into(),
        text: "main".into(),
        tooltip: Some("The current branch".into()),
        command: Some(CommandId::parse("acme.show_branch").unwrap()),
        alignment: Alignment::Left,
    };
    assert_eq!(items.items(), std::slice::from_ref(&expected));
    assert_eq!(items.item(PLUGIN, "branch"), Some(expected));

    // Setting the id again replaces it, in place.
    state.set_item(item("lines", "12 lines", None)).unwrap();
    state.set_item(item("branch", "feature", None)).unwrap();
    let shown: Vec<_> = items.items().into_iter().map(|item| item.text).collect();
    assert_eq!(shown, ["feature", "12 lines"]);
    assert_eq!(items.item(PLUGIN, "branch").unwrap().command, None);

    state.remove_item("lines".into()).unwrap();
    let missing = state.remove_item("lines".into());
    assert!(matches!(missing, Err(Error::NotFound(_))), "{missing:?}");
    let bad = state.set_item(item("bad", "bad", Some("not a command")));
    assert!(matches!(bad, Err(Error::InvalidArgument(_))), "{bad:?}");

    // Each import needs `status`.
    let mut denied = plugin("other", &["commands", "decorations"], &items);
    for result in [
        denied.set_item(item("x", "x", None)),
        denied.remove_item("x".into()),
    ] {
        assert!(
            matches!(&result, Err(Error::Denied(why)) if why.contains("status")),
            "{result:?}"
        );
    }
    assert!(items.items().iter().all(|item| item.plugin == PLUGIN));

    // A plugin shows only so many.
    for index in 1..MAX_ITEMS_PER_PLUGIN {
        state.set_item(item(&index.to_string(), "n", None)).unwrap();
    }
    let over = state.set_item(item("one more", "n", None));
    assert!(matches!(over, Err(Error::Failed(_))), "{over:?}");

    // Unloading clears them, and the unloaded store sets nothing more.
    items.unload(PLUGIN);
    assert!(items.items().is_empty());
    let after = state.set_item(item("branch", "main", None));
    assert!(matches!(after, Err(Error::Denied(_))), "{after:?}");
    assert!(items.items().is_empty());
}

/// Commands that log what ran, and as whom.
struct Logged;

impl CommandTarget for Logged {
    type Tx = ();
    type Target<'a> = Vec<String>;
}

/// Grants `acme` `commands` and `status`, and `bare` only `status`.
struct Policy;

impl PluginPolicy for Policy {
    fn grants(&self, plugin: &str) -> Option<Grants> {
        match plugin {
            PLUGIN => Some(grants(&["commands", "status"])),
            "bare" => Some(grants(&["status"])),
            _ => None,
        }
    }

    fn workspace_roots(&self) -> Vec<PathBuf> {
        Vec::new()
    }
}

#[test]
fn activating_an_item_dispatches_as_the_plugin() {
    // `test.open` is open to plugins with `commands`, `test.write` to those with `buffer.write`
    // too, and `test.secret` to none.
    let mut registry = Registry::<Logged>::new();
    for id in ["test.open", "test.write", "test.secret"] {
        registry
            .register(id, id, move |log: &mut Vec<String>, actor: &Actor| {
                log.push(format!("{actor:?} ran {id}"));
                Ok(Dispatched::new(Effect::Notify))
            })
            .unwrap();
    }
    registry
        .allow_plugins("test.open", PluginAccess::default())
        .unwrap();
    registry
        .allow_plugins(
            "test.write",
            PluginAccess {
                capabilities: vec![Capability::BufferWrite],
                path_args: Vec::new(),
            },
        )
        .unwrap();
    registry.set_plugin_policy(Arc::new(Policy));
    let observed = Rc::new(RefCell::new(Vec::new()));
    let observer = Rc::clone(&observed);
    registry.observe(move |invocation| observer.borrow_mut().push(invocation.clone()));

    let items = StatusItems::new();
    let mut state = plugin(PLUGIN, &["status"], &items);
    for (id, command) in [
        ("open", Some("test.open")),
        ("write", Some("test.write")),
        ("secret", Some("test.secret")),
        ("plain", None),
    ] {
        state.set_item(item(id, id, command)).unwrap();
    }

    let mut log = Vec::new();
    let opened = items.activate(PLUGIN, "open", &registry, &mut log);
    assert!(matches!(opened, Some(Ok(_))), "{opened:?}");
    assert_eq!(log, [format!("Host({PLUGIN:?}) ran test.open")]);
    let expected = Invocation::of("test.open")
        .unwrap()
        .actor(Actor::Host(PLUGIN.into()))
        .source(Source::Mouse);
    assert_eq!(*observed.borrow(), [expected]);

    // An item can't run what its plugin couldn't.
    log.clear();
    let write = items.activate(PLUGIN, "write", &registry, &mut log);
    assert!(
        matches!(
            write,
            Some(Err(DispatchError::PluginDenied(
                PluginDenied::MissingCapability {
                    capability: Capability::BufferWrite,
                    ..
                }
            )))
        ),
        "{write:?}"
    );
    let secret = items.activate(PLUGIN, "secret", &registry, &mut log);
    assert!(
        matches!(
            secret,
            Some(Err(DispatchError::PluginDenied(
                PluginDenied::Undeclared { .. }
            )))
        ),
        "{secret:?}"
    );
    // Nor one whose plugin holds no `commands`.
    let mut bare = plugin("bare", &["status"], &items);
    bare.set_item(item("open", "open", Some("test.open")))
        .unwrap();
    let bare = items.activate("bare", "open", &registry, &mut log);
    assert!(
        matches!(
            bare,
            Some(Err(DispatchError::PluginDenied(
                PluginDenied::MissingCapability {
                    capability: Capability::Commands,
                    ..
                }
            )))
        ),
        "{bare:?}"
    );
    assert!(log.is_empty(), "{log:?}");
    assert_eq!(observed.borrow().len(), 1);

    // An item with no command, or none at all, dispatches nothing.
    assert!(
        items
            .activate(PLUGIN, "plain", &registry, &mut log)
            .is_none()
    );
    assert!(
        items
            .activate(PLUGIN, "gone", &registry, &mut log)
            .is_none()
    );
    items.unload(PLUGIN);
    assert!(
        items
            .activate(PLUGIN, "open", &registry, &mut log)
            .is_none()
    );
}
