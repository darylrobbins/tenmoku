//! `fe-remote-proto-peer [--build BUILD] [--window BYTES]`: a mux server on
//! stdin and stdout running `fe_remote_proto::testing::serve`, for tests of
//! the child-stdio link. `--build` makes it answer as another build.

use fe_remote_proto::{Config, Link, Mux, testing};
use std::io;

fn main() {
    let mut config = Config {
        // The client's heartbeat is the one under test.
        heartbeat: None,
        ..Config::default()
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let value = args.next().expect("a value after each flag");
        match arg.as_str() {
            "--build" => config.build = value,
            "--window" => config.window = value.parse().expect("a window in bytes"),
            _ => panic!("unknown flag {arg}"),
        }
    }
    let link = Link::new(io::stdin(), io::stdout());
    // A refused client is a normal exit: the client sees the reason.
    if let Ok((mux, _)) = Mux::server(link, config, testing::accept_any) {
        testing::serve(&mux);
    }
}
