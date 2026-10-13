//! `net-ping`: two computers connect as a group ride would (ADR 0015) and ping each other, to
//! try punching through real routers before riding.

use std::time::Duration;

use anyhow::{Result, bail};
use clap::{Args, Subcommand};
use tokio::sync::mpsc;
use tokio::time::{Instant, timeout};
use torqa_net::{
    ConnectOptions, Connection, Course, Hint, Host, HostEvent, Invite, JoinError, Joiner, Reply,
};

#[derive(Args)]
pub(crate) struct NetPingArgs {
    #[command(subcommand)]
    role: Role,
    #[command(flatten)]
    net: NetArgs,
}

/// Which addresses to offer a group ride.
#[derive(Args)]
pub(crate) struct NetArgs {
    /// Ask no STUN server, as with the setting switched off.
    #[arg(long, global = true)]
    no_stun: bool,
    /// Ask these STUN servers (`host:port`, repeatable) instead of the default ones.
    #[arg(long, global = true)]
    stun: Vec<String>,
    /// Do not ask the router for a port.
    #[arg(long, global = true)]
    no_router: bool,
    /// Also offer the loopback address, to try both ends on one computer.
    #[arg(long, global = true)]
    loopback: bool,
}

#[derive(Subcommand)]
enum Role {
    /// Host: print an invite, then take replies pasted on the input, one per line.
    Host,
    /// Join from an invite: print the reply for the host, connect and ping ten times.
    Join {
        /// The host's invite.
        invite: String,
    },
}

impl NetArgs {
    pub(crate) fn options(&self) -> ConnectOptions {
        let mut options = ConnectOptions::default();
        if self.no_stun {
            options.gather.stun_servers.clear();
        } else if !self.stun.is_empty() {
            options.gather.stun_servers.clone_from(&self.stun);
        }
        options.gather.router = !self.no_router;
        options.gather.loopback = self.loopback;
        options
    }
}

pub(crate) async fn run(args: &NetPingArgs) -> Result<()> {
    let options = args.net.options();
    match &args.role {
        Role::Host => host(&options).await,
        Role::Join { invite } => join(invite, &options).await,
    }
}

async fn host(options: &ConnectOptions) -> Result<()> {
    let mut host = Host::open(options).await?;
    let course = Course {
        name: "net-ping".into(),
        route_hash: [0; 32],
    };
    let invite = host.invite("torqa-cli", course);
    print_candidates(&invite.candidates);
    println!(
        "\nInvite — pass it on to whoever joins:\n\n{}\n",
        invite.to_code()
    );
    println!("Paste each reply and press Enter; Ctrl+C to stop.");
    let mut lines = stdin_lines();
    loop {
        tokio::select! {
            line = lines.recv() => {
                let Some(line) = line else { break };
                if line.trim().is_empty() {
                    continue;
                }
                match Reply::from_code(&line) {
                    Ok(reply) => {
                        host.admit(&reply);
                        println!("Punching towards them for {} s…", options.window.as_secs());
                    }
                    Err(e) => println!("{e}"),
                }
            }
            event = host.next() => match event {
                Some(HostEvent::Joined { connection, .. }) => {
                    println!("Joined from {}", connection.remote_address());
                    tokio::spawn(echo(connection));
                }
                Some(HostEvent::GaveUp { hint, .. }) => println!("Gave up: {}", hint_text(hint)),
                None => break,
            },
        }
    }
    host.close().await;
    Ok(())
}

async fn join(code: &str, options: &ConnectOptions) -> Result<()> {
    let invite = Invite::from_code(code)?;
    let joiner = Joiner::open(invite, options).await?;
    let reply = joiner.reply();
    print_candidates(&reply.candidates);
    println!(
        "\nReply — pass it on to the host within {} s:\n\n{}\n",
        options.window.as_secs(),
        reply.to_code()
    );
    let connection = match joiner.connect().await {
        Ok(connection) => connection,
        Err(JoinError::Unreachable(hint)) => {
            bail!("The host could not be reached: {}", hint_text(hint))
        }
        Err(e) => bail!("{e}"),
    };
    println!("Connected to {}", connection.remote_address());
    for i in 0..10u8 {
        let sent = Instant::now();
        connection.send_datagram(vec![i].into())?;
        match timeout(Duration::from_secs(2), connection.read_datagram()).await {
            Ok(Ok(echo)) if echo[..] == [i] => {
                println!("ping {i}: {} ms", sent.elapsed().as_millis());
            }
            Ok(Ok(_)) => println!("ping {i}: unexpected answer"),
            Ok(Err(e)) => bail!("connection lost: {e}"),
            Err(_) => println!("ping {i}: no answer"),
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    joiner.close().await;
    Ok(())
}

async fn echo(connection: Connection) {
    while let Ok(datagram) = connection.read_datagram().await {
        if connection.send_datagram(datagram).is_err() {
            break;
        }
    }
    println!("{} left", connection.remote_address());
}

pub(crate) fn print_candidates(candidates: &[torqa_net::Candidate]) {
    println!("Offering:");
    for c in candidates {
        println!("  {:<8} {}", format!("{:?}", c.kind), c.addr);
    }
}

pub(crate) fn hint_text(hint: Hint) -> &'static str {
    match hint {
        Hint::NoPublicAddress => {
            "one side has no public address (no STUN answer, no router port, no IPv6)"
        }
        Hint::BothSymmetric => "both routers pick a new port per destination; punching cannot work",
        Hint::OneSymmetric => {
            "one router picks a new port per destination; try IPv6 or UPnP on the router"
        }
        Hint::Blocked => "a firewall may block UDP, or a code was passed on too late",
    }
}

/// Lines typed or pasted on the input, read on a thread of their own: the standard input
/// blocks.
fn stdin_lines() -> mpsc::UnboundedReceiver<String> {
    let (tx, rx) = mpsc::unbounded_channel();
    std::thread::spawn(move || {
        for line in std::io::stdin().lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    rx
}
