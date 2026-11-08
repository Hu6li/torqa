//! Coalesced organizer-link round trips, independent of ride-state and control queues.

use std::time::Duration;

use tokio::sync::watch;
use tokio::time::Instant;

use crate::state::MAX_RIDERS;

pub(super) type Values = [Option<Duration>; MAX_RIDERS];
pub(super) const EVERY: Duration = Duration::from_secs(1);
pub(super) const MAX: Duration = Duration::from_secs(60);
const FRESH: Duration = Duration::from_secs(3);

#[derive(Clone)]
pub(super) struct Latency(watch::Sender<(Values, Instant)>);

impl Latency {
    pub fn new() -> Self {
        Self(watch::channel(([None; MAX_RIDERS], Instant::now())).0)
    }

    pub fn update(&self, values: Values) {
        self.0.send_replace((values, Instant::now()));
    }

    pub fn clear(&self, id: crate::state::RiderId) {
        self.0
            .send_modify(|(values, _)| values[usize::from(id.value())] = None);
    }

    pub fn values(&self) -> Values {
        let (values, at) = *self.0.borrow();
        if at.elapsed() < FRESH {
            values
        } else {
            [None; MAX_RIDERS]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn round_trips_coalesce_and_expire_without_new_measurements() {
        let latency = Latency::new();
        assert_eq!(latency.values(), [None; MAX_RIDERS]);
        let mut values = [None; MAX_RIDERS];
        values[1] = Some(Duration::from_millis(120));
        latency.update(values);
        values[1] = Some(Duration::from_millis(80));
        values[2] = Some(Duration::from_millis(250));
        latency.update(values);
        assert_eq!(latency.clone().values(), values);
        tokio::time::advance(FRESH / 2).await;
        latency.clear(crate::state::RiderId::new(1).unwrap());
        assert_eq!(latency.values()[1], None);
        assert_eq!(latency.values()[2], values[2]);
        tokio::time::advance(FRESH / 2).await;
        assert_eq!(latency.values(), [None; MAX_RIDERS]);
        latency.update(values);
        assert_eq!(latency.values(), values);
        latency.update([None; MAX_RIDERS]);
        assert_eq!(latency.values(), [None; MAX_RIDERS]);
    }
}
