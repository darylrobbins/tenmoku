//! **Preview:** this API and the WIT under it can change incompatibly in any release; rebuild
//! plugins against each release.
//!
//! The SDK a Tenmoku plugin builds on.
//!
//! A plugin is a `cdylib` built for `wasm32-wasip2` with an `extension.toml` beside it. It
//! implements [`Plugin`], overriding what it uses, and exports it with [`export_plugin!`]; the
//! imports its manifest's `capabilities` grant are the modules here, such as [`buffer`] and
//! [`status`]. `template/` is one to copy.
//!
//! ```ignore
//! use tenmoku_plugin::{Error, Plugin, export_plugin, info};
//!
//! #[derive(Default)]
//! struct Hello;
//!
//! impl Plugin for Hello {
//!     fn init(&mut self) -> Result<(), Error> {
//!         info!("hello");
//!         Ok(())
//!     }
//! }
//!
//! export_plugin!(Hello);
//! ```

mod plugin;

pub use plugin::Plugin;

mod bindings {
    // In a module of its own: the world's export macro can't be `pub use`d at the root of the crate
    // that `#[macro_export]`s it.
    wit_bindgen::generate!({
        world: "fastedit:editor/editor-extension",
        // kiln:extension is read from its one copy, as the host's bindgen reads it.
        path: ["../../kiln/crates/kiln-plugin/wit", "../fe-plugin/wit"],
        generate_all,
        pub_export_macro: true,
        default_bindings_module: "tenmoku_plugin",
    });
}

pub use bindings::*;
pub use fastedit::editor::{buffer, decorations, status};
pub use kiln::extension::{commands, events, fs, log, net, process, secrets, types};
