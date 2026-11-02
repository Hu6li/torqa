//! Quinn's drivers belong to the local session, including during cancelled handshakes.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use quinn::{AsyncTimer, AsyncUdpSocket, Runtime, TokioRuntime};
use tokio::task::JoinSet;

// Includes endpoint drivers and connections draining after failed attempts. Refuse more
// work rather than retaining arbitrarily many driver tasks under repeated handshakes.
const MAX_DRIVERS: usize = 128;

#[derive(Debug, Default)]
pub(crate) struct OwnedRuntime(Mutex<Drivers>);

#[derive(Debug, Default)]
struct Drivers {
    closed: bool,
    tasks: JoinSet<()>,
}

impl OwnedRuntime {
    pub(crate) fn shutdown(&self) {
        let mut drivers = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        drivers.closed = true;
        let mut tasks = std::mem::take(&mut drivers.tasks);
        // Flush a close packet without letting normal QUIC draining retain cancelled
        // sockets. Dropping the JoinSet aborts drivers still alive after this grace period.
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let _ = tokio::time::timeout(std::time::Duration::from_millis(100), async {
                    while tasks.join_next().await.is_some() {}
                })
                .await;
            });
        }
    }
}

impl Runtime for OwnedRuntime {
    fn new_timer(&self, time: std::time::Instant) -> Pin<Box<dyn AsyncTimer>> {
        TokioRuntime.new_timer(time)
    }

    fn spawn(&self, future: Pin<Box<dyn Future<Output = ()> + Send>>) {
        let mut drivers = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while drivers.tasks.try_join_next().is_some() {}
        if !drivers.closed && drivers.tasks.len() < MAX_DRIVERS {
            drivers.tasks.spawn(future);
        }
    }

    fn wrap_udp_socket(
        &self,
        socket: std::net::UdpSocket,
    ) -> std::io::Result<Arc<dyn AsyncUdpSocket>> {
        TokioRuntime.wrap_udp_socket(socket)
    }

    fn now(&self) -> std::time::Instant {
        TokioRuntime.now()
    }
}
