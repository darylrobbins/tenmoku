//! What a plugin implements, and the macros around it.

use crate::Error;
use crate::events::Event;

/// A plugin: one value, made with [`Default`] before its first call and kept until it unloads.
/// Every method has a default, so a plugin writes only what it uses. Export it with
/// [`export_plugin!`](crate::export_plugin).
pub trait Plugin: Default + 'static {
    /// Called once after the plugin loads, before anything else: the one time it can register
    /// commands. An error unloads it.
    fn init(&mut self) -> Result<(), Error> {
        Ok(())
    }

    /// Called once before the host drops the plugin.
    fn shutdown(&mut self) {}

    /// Runs one of the plugin's registered commands, named by its whole id, with JSON arguments
    /// (empty for none).
    fn run_command(&mut self, id: &str, args: &str) -> Result<(), Error> {
        let _ = args;
        Err(Error::not_found(id))
    }

    /// Takes the next events, in order, of the kinds passed to
    /// [`events::subscribe`](crate::events::subscribe).
    fn handle_events(&mut self, events: Vec<Event>) {
        let _ = events;
    }
}

/// Each case of the WIT `error`, from anything that makes its message.
impl Error {
    pub fn unimplemented(message: impl Into<String>) -> Self {
        Self::Unimplemented(message.into())
    }

    pub fn denied(message: impl Into<String>) -> Self {
        Self::Denied(message.into())
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound(message.into())
    }

    pub fn invalid_argument(message: impl Into<String>) -> Self {
        Self::InvalidArgument(message.into())
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::Conflict(message.into())
    }

    pub fn failed(message: impl Into<String>) -> Self {
        Self::Failed(message.into())
    }
}

/// Exports a [`Plugin`] as the component's `init`, `shutdown`, `run-command` and `handle-events`.
/// A plugin calls it once, at the top level of its crate.
#[macro_export]
macro_rules! export_plugin {
    ($plugin:ty) => {
        const _: () = {
            ::std::thread_local! {
                static PLUGIN: ::core::cell::RefCell<$plugin> =
                    ::core::cell::RefCell::new(<$plugin as ::core::default::Default>::default());
            }

            struct Exports;

            impl $crate::Guest for Exports {
                fn init() -> ::core::result::Result<(), $crate::Error> {
                    PLUGIN.with_borrow_mut(|plugin| $crate::Plugin::init(plugin))
                }

                fn shutdown() {
                    PLUGIN.with_borrow_mut(|plugin| $crate::Plugin::shutdown(plugin))
                }
            }

            impl $crate::exports::kiln::extension::command_handler::Guest for Exports {
                fn run_command(
                    id: ::std::string::String,
                    args: ::std::string::String,
                ) -> ::core::result::Result<(), $crate::Error> {
                    PLUGIN.with_borrow_mut(|plugin| $crate::Plugin::run_command(plugin, &id, &args))
                }
            }

            impl $crate::exports::kiln::extension::event_handler::Guest for Exports {
                fn handle_events(events: ::std::vec::Vec<$crate::events::Event>) {
                    PLUGIN.with_borrow_mut(|plugin| $crate::Plugin::handle_events(plugin, events))
                }
            }

            $crate::export! { Exports with_types_in $crate }
        };
    };
}

/// Logs at [`Level::Info`](crate::log::Level::Info), under the plugin's name, with `format!`'s
/// arguments.
#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => {
        $crate::log::log($crate::log::Level::Info, &::std::format!($($arg)*))
    };
}

/// Logs at [`Level::Warn`](crate::log::Level::Warn), under the plugin's name, with `format!`'s
/// arguments.
#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => {
        $crate::log::log($crate::log::Level::Warn, &::std::format!($($arg)*))
    };
}

/// Logs at [`Level::Error`](crate::log::Level::Error), under the plugin's name, with `format!`'s
/// arguments.
#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => {
        $crate::log::log($crate::log::Level::Error, &::std::format!($($arg)*))
    };
}
