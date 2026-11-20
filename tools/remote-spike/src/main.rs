use remote_spike::bench::{self, Opts};
use std::io;
use std::path::PathBuf;

const USAGE: &str = "usage:
  remote-spike server                  speak the protocol on stdin/stdout
  remote-spike bench [--latency 0,5,25] [--window BYTES] [--dir PATH] [--quick]
                     [--capped MS,MBITS]
                     [--ssh HOST --remote-bin PATH]
  remote-spike fixture --dir PATH      make the 50k-file tree and 1 MiB file
  remote-spike codec                   compare encodings";

fn main() -> io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("server") => {
            let build = std::env::var("SPIKE_SERVER_BUILD").unwrap_or_else(|_| bench::BUILD.into());
            remote_spike::server::serve(io::stdin().lock(), io::stdout(), &build)
        }
        Some("fixture") => {
            // Makes the bench fixture on a host, for `bench --ssh`.
            let dir = args.get(2).filter(|_| args[1] == "--dir").expect(USAGE);
            let (tree, big) = bench::fixture(std::path::Path::new(dir))?;
            println!("{} {}", tree.display(), big.display());
            Ok(())
        }
        Some("codec") => {
            remote_spike::codec::run();
            Ok(())
        }
        Some("bench") => {
            let mut o = Opts {
                latencies_ms: vec![0, 5, 25],
                window: 1280 * 1024,
                dir: std::env::temp_dir().join("remote-spike-fixture"),
                ssh: None,
                quick: false,
                capped: (25, 100.0),
            };
            let mut host = None;
            let mut it = args[1..].iter();
            while let Some(a) = it.next() {
                let mut val = || it.next().cloned().expect(USAGE);
                match a.as_str() {
                    "--latency" => {
                        o.latencies_ms = val().split(',').map(|s| s.parse().expect(USAGE)).collect()
                    }
                    "--window" => o.window = val().parse().expect(USAGE),
                    "--dir" => o.dir = PathBuf::from(val()),
                    "--quick" => o.quick = true,
                    "--capped" => {
                        let v = val();
                        let (l, m) = v.split_once(',').expect(USAGE);
                        o.capped = (l.parse().expect(USAGE), m.parse().expect(USAGE));
                    }
                    "--ssh" => host = Some(val()),
                    "--remote-bin" => o.ssh = Some((host.clone().expect(USAGE), val())),
                    _ => panic!("{USAGE}"),
                }
            }
            bench::run(&o)
        }
        _ => {
            eprintln!("{USAGE}");
            std::process::exit(2)
        }
    }
}
