//! Explicit network choices shared by the developer networking commands.

use anyhow::Result;
use clap::Args;
use orqwa_net::discovery::{self, GatherOptions};
use orqwa_net::policy::AddressPolicy;
use tokio::time::Instant;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Input {
    Line(String),
    Invalid,
}

// A plain thread lets the CLI exit while stdin is idle. Neither the thread nor its
// bounded queue owns any network resource; overlong lines are discarded before allocation.
pub(crate) fn input() -> tokio::sync::mpsc::Receiver<Input> {
    let (send, receive) = tokio::sync::mpsc::channel(8);
    std::thread::spawn(move || {
        let mut stdin = std::io::stdin().lock();
        while let Ok(Some(line)) = read_input(&mut stdin) {
            if send.blocking_send(line).is_err() {
                break;
            }
        }
    });
    receive
}

fn read_input(reader: &mut impl std::io::BufRead) -> std::io::Result<Option<Input>> {
    let mut bytes = Vec::with_capacity(orqwa_net::code::MAX_PASTE_BYTES);
    let mut invalid = false;
    loop {
        let chunk = reader.fill_buf()?;
        if chunk.is_empty() {
            return Ok(if bytes.is_empty() && !invalid {
                None
            } else {
                Some(parse_input(bytes, invalid))
            });
        }
        let end = chunk.iter().position(|b| *b == b'\n');
        let count = end.map_or(chunk.len(), |i| i + 1);
        if bytes.len() + count > orqwa_net::code::MAX_PASTE_BYTES {
            invalid = true;
        }
        if !invalid {
            bytes.extend_from_slice(&chunk[..count]);
        }
        reader.consume(count);
        if end.is_some() {
            return Ok(Some(parse_input(bytes, invalid)));
        }
    }
}

fn parse_input(bytes: Vec<u8>, invalid: bool) -> Input {
    if invalid {
        return Input::Invalid;
    }
    String::from_utf8(bytes).map_or(Input::Invalid, |line| Input::Line(line.trim().to_owned()))
}

#[derive(Args)]
pub(crate) struct NetworkArgs {
    /// Permit private addresses for a deliberate local-network ride.
    #[arg(long, global = true, conflicts_with = "loopback")]
    lan: bool,
    /// Use only this computer's loopback sockets; no STUN or interface discovery.
    #[arg(long,global=true,conflicts_with_all=["lan","interface","stun"])]
    loopback: bool,
    /// OS network interface name; otherwise use the default interface only.
    #[arg(long, global = true)]
    interface: Option<String>,
    /// Ask no public STUN server and send no STUN DNS requests.
    #[arg(long, global = true, conflicts_with = "stun")]
    no_stun: bool,
    /// Use these STUN host:port addresses instead of the defaults (at most four).
    #[arg(long, global = true)]
    stun: Vec<String>,
}

impl NetworkArgs {
    pub(crate) fn options(&self) -> GatherOptions {
        let mut options = GatherOptions {
            policy: AddressPolicy {
                lan: self.lan,
                loopback: self.loopback,
            },
            interface: self.interface.clone(),
            ..GatherOptions::default()
        };
        if self.no_stun || self.loopback {
            options.stun_servers.clear();
        } else if !self.stun.is_empty() {
            options.stun_servers.clone_from(&self.stun);
        }
        options
    }
}

#[derive(Args)]
pub(crate) struct NetCheckArgs {
    #[command(flatten)]
    network: NetworkArgs,
}

pub(crate) async fn check(args: &NetCheckArgs) -> Result<()> {
    let paths = discovery::bind(
        &args.network.options(),
        Instant::now() + discovery::DISCOVERY_WINDOW,
    )
    .await?;
    for path in paths {
        println!("UDP {}", path.socket.local_addr()?);
        if path.candidates.is_empty() {
            println!(
                "  no permitted reachable addresses; try LAN mode, STUN or a public IPv6 interface"
            );
        }
        for candidate in path.candidates {
            println!("  {:?} {}", candidate.kind(), candidate.addr());
        }
        println!("  STUN observation: {:?}", path.nat);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    #[test]
    fn pasted_input_discards_oversized_and_invalid_lines_then_recovers() {
        let mut bytes = vec![b'a'; 100_000];
        bytes.extend_from_slice(b"\n\xff\nq\n");
        let mut reader = std::io::Cursor::new(bytes);
        assert_eq!(
            super::read_input(&mut reader).unwrap(),
            Some(super::Input::Invalid)
        );
        assert_eq!(
            super::read_input(&mut reader).unwrap(),
            Some(super::Input::Invalid)
        );
        assert_eq!(
            super::read_input(&mut reader).unwrap(),
            Some(super::Input::Line("q".into()))
        );
        assert_eq!(super::read_input(&mut reader).unwrap(), None);
    }

    #[test]
    fn local_and_offline_checks_do_not_inherit_public_discovery_servers() {
        for flag in ["--loopback", "--no-stun"] {
            let cli = crate::Cli::try_parse_from(["orqwa-cli", "net-check", flag]).unwrap();
            let crate::Command::NetCheck(args) = cli.command else {
                panic!("net-check");
            };
            assert_eq!(args.network.options().stun_servers, Vec::<String>::new());
        }
        assert!(
            crate::Cli::try_parse_from(["orqwa-cli", "net-check", "--loopback", "--lan"]).is_err()
        );
        assert!(
            crate::Cli::try_parse_from([
                "orqwa-cli",
                "net-check",
                "--no-stun",
                "--stun",
                "1.1.1.1:3478"
            ])
            .is_err()
        );
    }
}
