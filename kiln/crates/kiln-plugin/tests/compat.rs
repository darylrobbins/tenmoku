// SPDX-License-Identifier: Apache-2.0

//! Which plugins load on which host, and an adapter serving an older minor.

use kiln_core::ApiVersion;
use kiln_plugin::compat::{Adapter, Compat, HOST_API, Incompatible, link_adapters};
use wasmtime::{
    Config, Engine, Store,
    component::{Component, Linker, Val},
};

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
