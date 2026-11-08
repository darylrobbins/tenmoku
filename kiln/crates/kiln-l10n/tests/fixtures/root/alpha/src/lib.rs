// SPDX-License-Identifier: Apache-2.0
// A fixture crate for the root catalogue check; never compiled.

mod l10n;

pub fn labels(id: &str) {
    let _ = tr!("alpha-used");
    let key = "alpha-literal";
    let _ = tr_or(&format!("alpha-dynamic-{id}"), id);
    let _ = key;
}

#[cfg(test)]
mod tests {
    fn only_in_tests() {
        let _ = tr!("alpha-test-only");
    }
}
