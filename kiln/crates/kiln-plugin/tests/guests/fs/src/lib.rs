// SPDX-License-Identifier: Apache-2.0

//! A guest that does through `std` what a plugin would, and reports what happened: `tests/fs.rs`
//! runs it under the WASI context `kiln_plugin::fs::configure` builds.

use std::io::{Read as _, Write as _};

wit_bindgen::generate!({
    inline: "
        package kiln-test:fs;

        // Export names are core symbols too: `read` or `write` would replace libc's own.
        world fs-guest {
            /// The file at `path`, or why it can't be read.
            export read-file: func(path: string) -> result<string, string>;
            /// Writes `contents` to `path`, creating or truncating it.
            export write-file: func(path: string, contents: string) -> result<_, string>;
            /// The environment the guest sees.
            export environment: func() -> list<tuple<string, string>>;
            /// The arguments the guest sees.
            export arguments: func() -> list<string>;
            /// Writes `marker` to stdout and stderr, then reads stdin to its end and returns it.
            export stdio: func(marker: string) -> result<list<u8>, string>;
        }
    ",
});

struct FsGuest;

impl Guest for FsGuest {
    fn read_file(path: String) -> Result<String, String> {
        std::fs::read_to_string(path).map_err(|error| error.to_string())
    }

    fn write_file(path: String, contents: String) -> Result<(), String> {
        std::fs::write(path, contents).map_err(|error| error.to_string())
    }

    fn environment() -> Vec<(String, String)> {
        std::env::vars().collect()
    }

    fn arguments() -> Vec<String> {
        std::env::args().collect()
    }

    fn stdio(marker: String) -> Result<Vec<u8>, String> {
        let failed = |error: std::io::Error| error.to_string();
        std::io::stdout()
            .write_all(marker.as_bytes())
            .map_err(failed)?;
        std::io::stdout().flush().map_err(failed)?;
        std::io::stderr()
            .write_all(marker.as_bytes())
            .map_err(failed)?;
        let mut stdin = Vec::new();
        std::io::stdin().read_to_end(&mut stdin).map_err(failed)?;
        Ok(stdin)
    }
}

export!(FsGuest);
