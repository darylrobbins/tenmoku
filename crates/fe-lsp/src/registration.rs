//! Dynamic registration (docs/decisions/lsp-client.md §3.6).
//!
//! fe-lsp offers no dynamic registration (every `dynamicRegistration` in
//! its capabilities is false), but servers send some anyway, and a
//! successful reply means the registration is installed. So
//! `client/registerCapability` accepts only methods whose effect fe-lsp
//! implements: in M4 exactly `workspace/didChangeConfiguration`, which is
//! sent whenever settings change, whatever is registered. Any other method
//! refuses the whole request (registration is all or nothing) with
//! `RequestFailed`, naming the methods, and records nothing.
//! `client/unregisterCapability` removes the ids it names; an unknown id
//! is a no-op.

use std::collections::BTreeMap;

use async_lsp::{ErrorCode, ResponseError};
use lsp_types::notification::{DidChangeConfiguration, Notification};
use lsp_types::{RegistrationParams, UnregistrationParams};

/// The methods a server may register.
const SUPPORTED: [&str; 1] = [DidChangeConfiguration::METHOD];

/// A session's accepted registrations: method by registration id.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Registrations(BTreeMap<String, String>);

impl Registrations {
    pub(crate) fn register(&mut self, params: RegistrationParams) -> Result<(), ResponseError> {
        let unsupported: Vec<&str> = params
            .registrations
            .iter()
            .map(|registration| registration.method.as_str())
            .filter(|method| !SUPPORTED.contains(method))
            .collect();
        if !unsupported.is_empty() {
            return Err(ResponseError::new(
                ErrorCode::REQUEST_FAILED,
                format!("fastedit can't register {}", unsupported.join(", ")),
            ));
        }
        for registration in params.registrations {
            self.0.insert(registration.id, registration.method);
        }
        Ok(())
    }

    pub(crate) fn unregister(&mut self, params: UnregistrationParams) {
        for unregistration in params.unregisterations {
            self.0.remove(&unregistration.id);
        }
    }

    /// The registration ids, in order.
    #[cfg(test)]
    pub(crate) fn ids(&self) -> Vec<String> {
        self.0.keys().cloned().collect()
    }
}
