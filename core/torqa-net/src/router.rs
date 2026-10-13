//! A port the rider's router forwards to the ride's socket, asked for by PCP or NAT-PMP
//! (`crab_nat`) and `UPnP` (`igd-next`) at once. Many routers have neither switched on; then a
//! ride relies on punching alone.

use std::net::{IpAddr, SocketAddr};
use std::num::NonZeroU16;
use std::time::Duration;

use igd_next::PortMappingProtocol;
use igd_next::aio::Gateway;
use igd_next::aio::tokio::Tokio;
use tokio::time::timeout;

/// How long a mapping is asked for. Longer than most rides; the router drops it by itself
/// afterwards should Torqa not get to release it.
const LEASE: Duration = Duration::from_hours(4);

/// How often a mapping is renewed: well within its lease.
pub const RENEW_EVERY: Duration = Duration::from_hours(1);

fn lease_seconds() -> u32 {
    u32::try_from(LEASE.as_secs()).unwrap_or(u32::MAX)
}

/// Shown in the router's list of forwarded ports.
const DESCRIPTION: &str = "Torqa group ride";

/// A UDP port the router forwards to the ride's socket.
#[derive(Debug)]
pub struct RouterMapping {
    external: SocketAddr,
    local: SocketAddr,
    via: Via,
}

#[derive(Debug)]
enum Via {
    Pcp(crab_nat::PortMapping),
    Upnp(Gateway<Tokio>),
}

impl RouterMapping {
    /// The public address others reach the socket at.
    #[must_use]
    pub fn external(&self) -> SocketAddr {
        self.external
    }

    /// Asks the router to keep forwarding the port for another lease; `false` if it would not.
    pub async fn renew(&mut self) -> bool {
        let renewed = match &mut self.via {
            Via::Pcp(mapping) => mapping.renew().await.map_err(|e| e.to_string()),
            Via::Upnp(gateway) => gateway
                .add_port(
                    PortMappingProtocol::UDP,
                    self.external.port(),
                    self.local,
                    lease_seconds(),
                    DESCRIPTION,
                )
                .await
                .map_err(|e| e.to_string()),
        };
        renewed
            .inspect_err(|e| tracing::debug!("Router did not renew {}: {e}", self.external))
            .is_ok()
    }

    /// Asks the router to stop forwarding the port. Best effort: the lease ends anyway.
    pub async fn release(self) {
        let released = match self.via {
            Via::Pcp(mapping) => mapping.try_drop().await.map_err(|(e, _)| e.to_string()),
            Via::Upnp(gateway) => gateway
                .remove_port(PortMappingProtocol::UDP, self.external.port())
                .await
                .map_err(|e| e.to_string()),
        };
        if let Err(e) = released {
            tracing::debug!("Router kept forwarding {}: {e}", self.external);
        }
    }
}

/// Asks the router of the default network for a port forwarded to `local` (the computer's
/// private IPv4 address and the socket's port), by PCP or NAT-PMP and by `UPnP` at once, within
/// `wait`. A router that grants both keeps the PCP one.
pub async fn map_port(local: SocketAddr, wait: Duration) -> Option<RouterMapping> {
    let (pcp, upnp) = tokio::join!(timeout(wait, by_pcp(local)), timeout(wait, by_upnp(local)));
    let mut found = Vec::new();
    for mapping in [pcp.ok().flatten(), upnp.ok().flatten()]
        .into_iter()
        .flatten()
    {
        // A router behind another one (carrier-grade NAT) forwards from an address nobody
        // outside can reach.
        if crate::candidates::is_private(mapping.external.ip()) {
            tracing::debug!("Router is behind another NAT: {}", mapping.external);
            mapping.release().await;
        } else {
            found.push(mapping);
        }
    }
    let mut found = found.into_iter();
    match (found.next(), found.next()) {
        (Some(pcp), Some(upnp)) => {
            upnp.release().await;
            Some(pcp)
        }
        (first, _) => first,
    }
}

async fn by_pcp(local: SocketAddr) -> Option<RouterMapping> {
    let gateway = match netdev::get_default_gateway() {
        Ok(gateway) => *gateway.ipv4.first()?,
        Err(e) => {
            tracing::debug!("No default gateway: {e}");
            return None;
        }
    };
    let options = crab_nat::PortMappingOptions {
        external_port: NonZeroU16::new(local.port()),
        lifetime_seconds: Some(lease_seconds()),
        timeout_config: None,
    };
    let mapping = crab_nat::PortMapping::new(
        gateway.into(),
        local.ip(),
        crab_nat::InternetProtocol::Udp,
        NonZeroU16::new(local.port())?,
        options,
    )
    .await
    .inspect_err(|e| tracing::debug!("No PCP or NAT-PMP mapping from {gateway}: {e}"))
    .ok()?;
    let ip = match mapping.mapping_type() {
        crab_nat::PortMappingType::Pcp { external_ip, .. } => external_ip,
        crab_nat::PortMappingType::NatPmp => {
            match crab_nat::natpmp::external_address(gateway.into(), None).await {
                Ok(ip) => IpAddr::V4(ip),
                Err(e) => {
                    tracing::debug!("NAT-PMP mapping without its address: {e}");
                    let _ = mapping.try_drop().await;
                    return None;
                }
            }
        }
    };
    Some(RouterMapping {
        external: SocketAddr::new(ip, mapping.external_port().get()),
        local,
        via: Via::Pcp(mapping),
    })
}

async fn by_upnp(local: SocketAddr) -> Option<RouterMapping> {
    let mut options = igd_next::SearchOptions::default();
    options.bind_addr = SocketAddr::new(local.ip(), 0);
    let gateway = igd_next::aio::tokio::search_gateway(options)
        .await
        .inspect_err(|e| tracing::debug!("No UPnP router: {e}"))
        .ok()?;
    let external = gateway
        .get_any_address(
            PortMappingProtocol::UDP,
            local,
            lease_seconds(),
            DESCRIPTION,
        )
        .await
        .inspect_err(|e| tracing::debug!("No UPnP mapping from {}: {e}", gateway.addr))
        .ok()?;
    Some(RouterMapping {
        external,
        local,
        via: Via::Upnp(gateway),
    })
}
