// SPDX-License-Identifier: Apache-2.0

//! The keymap store of the config service that started, reachable from
//! the app with its default layers: what an app that edits the user's
//! keymap reads it on. They follow a change to the defaults.

use std::path::{Path, PathBuf};

use gpui::TestAppContext;
use kiln_core::{KeymapLayer, KeymapStore};
use kiln_platform::config::{self, ConfigService, batch_channel};

fn layer(path: &str, keys: &str, command: &str) -> KeymapLayer {
    let text = format!("[[bindings]]\nkeys = \"{keys}\"\ncommand = \"{command}\"\n");
    let (layer, diagnostics) = KeymapLayer::parse(Path::new(path), &text, None);
    assert_eq!(diagnostics, []);
    layer.unwrap()
}

#[gpui::test]
fn the_started_service_has_the_store_with_its_defaults(cx: &mut TestAppContext) {
    assert!(cx.update(|cx| config::keymap_store(cx).is_none()));
    let framework = layer("framework.toml", "secondary-shift-p", "palette.toggle");
    let app = layer("app.toml", "secondary-s", "pad.save");
    let user = PathBuf::from("/nonexistent/keymap.toml");
    let (_sender, batches) = batch_channel();
    cx.update(|cx| {
        let store = KeymapStore::with_defaults(
            vec![framework.clone(), app.clone()],
            Vec::new(),
            Some(user),
            None,
        );
        ConfigService::new()
            .keymap(store)
            .start_with(batches, cx)
            .detach();
    });
    cx.run_until_parked();
    let store = cx.update(|cx| config::keymap_store(cx).expect("the store"));
    assert_eq!(store.defaults(), [framework.clone(), app.clone()]);
    assert_eq!(store.keymap().bindings().len(), 2);

    // New defaults, as when commands are registered later: the store the
    // app reaches has them.
    let more = layer("more.toml", "secondary-j", "pad.join");
    cx.update(|cx| {
        let layers = vec![framework.clone(), app.clone(), more.clone()];
        store.update(cx, |store| store.set_defaults(layers, Vec::new(), None));
    });
    let again = cx.update(|cx| config::keymap_store(cx).unwrap());
    assert_eq!(again.defaults(), [framework, app, more]);
}
