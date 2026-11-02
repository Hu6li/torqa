//! Explicit two-person connection diagnostics; no trainer or activity data is exchanged.

use std::time::Duration;

use anyhow::{Context, Result, ensure};
use clap::{Args, Subcommand};
use orqwa_net::code::{Invite, Reply};
use orqwa_net::link::{ConnectOptions, Connection, Host, HostEvent, Joiner};
use tokio::task::JoinSet;
use tokio::time::{sleep, timeout};

use crate::network::{self, Input, NetworkArgs};

#[derive(Args)]
pub(crate) struct NetPingArgs {
    #[command(flatten)]
    network: NetworkArgs,
    #[command(subcommand)]
    role: Role,
}

#[derive(Subcommand)]
enum Role {
    /// Create an invite, then paste one private reply for each admitted person.
    Host,
    /// Create a reply to this invite; send it privately to the host within one minute.
    Join { invite: String },
}

pub(crate) async fn run(args: NetPingArgs) -> Result<()> {
    let options = ConnectOptions {
        gather: args.network.options(),
        ..ConnectOptions::default()
    };
    let route = orqwa_net::crypto::route_hash(b"orqwa-net-ping/1");
    match args.role {
        Role::Host => {
            let mut host = Host::open("CLI host", "Connection check", route, &options).await?;
            println!("Invite: {}", host.invite().to_code(options.gather.policy)?);
            println!(
                "Share codes privately with the intended person. Paste replies here; q quits."
            );
            let mut input = network::input();
            let mut tasks = JoinSet::new();
            loop {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => break,
                    line = input.recv() => match line {
                        None => break,
                        Some(Input::Line(line)) if line == "q" => break,
                        Some(Input::Line(line)) => match Reply::from_code(&line, options.gather.policy)
                            .map_err(anyhow::Error::from).and_then(|reply| host.admit(&reply).map_err(Into::into)) {
                                Ok(()) => println!("Reply admitted for at most one minute."),
                                Err(error) => eprintln!("Reply refused: {error}"),
                            },
                        Some(Input::Invalid) => eprintln!("Input refused: invalid text or more than 2048 bytes."),
                    },
                    event = host.next() => match event {
                        Some(HostEvent::Joined(connection)) if tasks.len() < 16 => { tasks.spawn(echo(connection)); },
                        Some(HostEvent::Joined(connection)) => connection.close(),
                        Some(HostEvent::Expired) => println!("A reply expired; ask for a fresh reply."),
                        None => break,
                    },
                    result = tasks.join_next(), if !tasks.is_empty() => {
                        if let Some(Ok(Err(error))) = result { eprintln!("Connection check ended: {error}"); }
                    },
                }
            }
            tasks.abort_all();
            host.close().await;
        }
        Role::Join { invite } => {
            let invite = Invite::from_code(&invite, options.gather.policy)?;
            let joiner = Joiner::open(invite, route, &options).await?;
            println!("Reply: {}", joiner.reply().to_code(options.gather.policy)?);
            println!("Send the reply privately to the host now; it expires within one minute.");
            let connection = tokio::select! {
                result = joiner.connect() => result?,
                _ = tokio::signal::ctrl_c() => return Ok(()),
            };
            tokio::select! {
                result = ping(&connection) => result?,
                _ = tokio::signal::ctrl_c() => {},
            }
            connection.close();
        }
    }
    Ok(())
}

async fn echo(connection: Connection) -> Result<()> {
    for sequence in 0..10 {
        let bytes = timeout(Duration::from_secs(2), connection.read_datagram()).await??;
        ensure!(bytes == [sequence], "invalid ping sequence");
        connection.send_datagram(&bytes)?;
    }
    println!("Connection check: ten pings echoed.");
    let _ = timeout(Duration::from_secs(2), connection.closed()).await;
    Ok(())
}

async fn ping(connection: &Connection) -> Result<()> {
    for sequence in 0..10 {
        connection.send_datagram(&[sequence])?;
        let bytes = timeout(Duration::from_secs(2), connection.read_datagram())
            .await
            .context("ping timed out")??;
        ensure!(bytes == [sequence], "invalid ping response");
        println!(
            "Ping {}: {:.1} ms",
            sequence + 1,
            connection.rtt().as_secs_f64() * 1000.0
        );
        sleep(Duration::from_millis(200)).await;
    }
    Ok(())
}
