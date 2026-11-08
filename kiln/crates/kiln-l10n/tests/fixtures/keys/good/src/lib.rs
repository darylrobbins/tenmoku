// SPDX-License-Identifier: Apache-2.0
// A fixture crate for the key check; never compiled.

pub fn labels(count: usize) {
    let _ = tr!("app-own");
    let _ = kiln_l10n::text!("lib-public", count = count);
    let _ = tr_or("lib-public", fallback);
    let _ = format!("{}", tr!("app-own"));
}

#[cfg(test)]
mod tests {
    fn only_in_tests() {
        let _ = tr!("not-a-key-anywhere");
    }
}
