//! `net-check`: the addresses this computer could offer a group ride (ADR 0015), to see how a
//! network fares before riders try it together.

use std::time::Duration;

use anyhow::Result;
use clap::Args;
use tokio::net::UdpSocket;
use torqa_net::{GatherOptions, NatBehavior, gather};

#[derive(Args)]
pub(crate) struct NetCheckArgs {
    /// Ask no STUN server, as with the setting switched off.
    #[arg(long)]
    no_stun: bool,
    /// Do not ask the router for a port.
    #[arg(long)]
    no_router: bool,
    /// Keep a port the router forwards instead of releasing it at the end, e.g. to look it up
    /// in the router.
    #[arg(long)]
    keep_mapping: bool,
}

pub(crate) async fn run(args: &NetCheckArgs) -> Result<()> {
    let mut options = GatherOptions::default();
    if args.no_stun {
        options.stun_servers.clear();
    }
    options.router = !args.no_router;
    options.timeout = Duration::from_secs(5);

    for bind in ["0.0.0.0:0", "[::]:0"] {
        let socket = match UdpSocket::bind(bind).await {
            Ok(socket) => socket,
            Err(e) => {
                println!("{bind}: no socket ({e})");
                continue;
            }
        };
        let gathered = gather(&socket, &options).await;
        let family = if bind.starts_with('[') {
            "IPv6"
        } else {
            "IPv4"
        };
        println!("{family} ({}):", socket.local_addr()?);
        for c in &gathered.candidates {
            println!("  {:<8} {}", format!("{:?}", c.kind), c.addr);
        }
        if gathered.candidates.is_empty() {
            println!("  no addresses");
        }
        let nat = match gathered.nat {
            NatBehavior::EndpointIndependent => "one port for every destination: punching works",
            NatBehavior::Symmetric => {
                "a port per destination: punching works only with riders whose router keeps one"
            }
            NatBehavior::Unknown => "one STUN server answered: unknown",
            NatBehavior::NoAnswer => "no STUN server answered",
        };
        println!("  NAT: {nat}");
        if let Some(mapping) = gathered.mapping {
            if args.keep_mapping {
                println!("  forwarded port kept: {}", mapping.external());
            } else {
                mapping.release().await;
            }
        }
    }
    Ok(())
}
