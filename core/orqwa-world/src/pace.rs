//! How much of the device a world takes while it is built under a ride (ADR 0040, 2d).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

use orqwa_domain::units::Meters;

/// How many cores a world being built may use, and at what priority, so that a ride drawn
/// meanwhile keeps its frame time and a tablet stays cool (ADR 0040, 2d). While a ride runs,
/// the build takes half the cores at low priority; when its lead over the rider falls below
/// twice the radius the scene uploads chunks within, it takes them all, until the lead has
/// doubled. With no ride running, or the ride paused (also while a rider who caught up waits
/// for it, or a late joiner waits to enter a group ride), it takes every core at the usual
/// priority: nothing moves on screen that it would hold up.
///
/// The lowered threads are never reused at the usual priority: lowering can't be undone
/// without privileges, so the build starts fresh threads for every batch rather than keeping
/// a pool.
///
/// It also turns the build to where a rider enters a ride under way, as a late joiner of a
/// group ride does (ADR 0040, 2d): see [`Pace::turn_to`].
///
/// Shared between the build and whoever knows where the rider is.
#[derive(Debug)]
pub struct Pace {
    /// The rider's distance along the route in metres, as `f64` bits: NaN while no ride runs.
    rider: AtomicU64,
    /// How far around the rider the scene uploads chunks, in metres.
    radius: f64,
    /// Whether the build's lead fell short, so it takes every core.
    hurrying: AtomicBool,
    /// Lowers the calling thread's priority, where the platform's glue can.
    lower: Option<fn()>,
    /// Where the build was last asked to turn to and hasn't yet.
    turn: Mutex<Option<Entry>>,
    /// Cores to build on instead of the device's, and the threads given to each batch.
    #[cfg(test)]
    pub(crate) test: (Option<usize>, std::sync::Mutex<Vec<usize>>),
}

impl Pace {
    /// The pace of a world whose scene uploads chunks within `radius` of the rider. The
    /// build's threads call `lower` as they start while a ride runs.
    #[must_use]
    pub fn new(radius: Meters, lower: Option<fn()>) -> Self {
        Self {
            rider: AtomicU64::new(f64::NAN.to_bits()),
            radius: radius.0,
            hurrying: AtomicBool::new(false),
            lower,
            turn: Mutex::new(None),
            #[cfg(test)]
            test: (None, std::sync::Mutex::new(Vec::new())),
        }
    }

    /// Cores to build on instead of the device's.
    #[allow(clippy::unused_self)] // tests only
    pub(crate) fn cores(&self) -> Option<usize> {
        #[cfg(test)]
        return self.test.0;
        #[cfg(not(test))]
        None
    }

    /// Where the rider is along the route, or `None` while no ride runs or it is paused.
    pub fn ride_at(&self, distance: Option<Meters>) {
        let distance = distance.map_or(f64::NAN, |distance| distance.0);
        self.rider.store(distance.to_bits(), Ordering::Relaxed);
    }

    /// Turns the build to `entry`: once the world has been handed over, the chunks around it
    /// are built next, then those ahead of it, then those behind. A later turn replaces one not
    /// yet taken. What is built stays the same; only its order changes.
    pub fn turn_to(&self, entry: Entry) {
        *self.turn.lock().unwrap_or_else(PoisonError::into_inner) = Some(entry);
    }

    /// The turn asked for since last taken, if any.
    pub(crate) fn take_turn(&self) -> Option<Entry> {
        self.turn
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }

    /// Whether the build takes every core because its lead fell short.
    #[must_use]
    pub fn hurrying(&self) -> bool {
        self.hurrying.load(Ordering::Relaxed)
    }

    /// The workers for the next chunks, of `cores`, when the nearest chunk ahead of the rider
    /// not yet built lies `front` metres along the route (`None`: none is left ahead).
    pub(crate) fn workers(&self, front: Option<f64>, cores: usize) -> Workers {
        let workers = self.pick(front, cores);
        #[cfg(test)]
        self.test
            .1
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(workers.threads);
        workers
    }

    fn pick(&self, front: Option<f64>, cores: usize) -> Workers {
        let rider = f64::from_bits(self.rider.load(Ordering::Relaxed));
        if rider.is_nan() {
            self.hurrying.store(false, Ordering::Relaxed);
            return Workers::new(cores);
        }
        let lead = front.map_or(f64::INFINITY, |front| front - rider);
        let hurrying = if lead < 2.0 * self.radius {
            true
        } else if lead >= 4.0 * self.radius {
            false
        } else {
            self.hurrying.load(Ordering::Relaxed)
        };
        self.hurrying.store(hurrying, Ordering::Relaxed);
        Workers {
            threads: if hurrying { cores } else { (cores / 2).max(1) },
            lower: self.lower,
        }
    }
}

/// Where a ride enters a course, as [`crate::Ahead`] has it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Entry {
    /// How far along the route.
    pub from: Meters,
    /// Where, in metres east/north of the route start.
    pub at: (Meters, Meters),
}

/// The threads a step of the build runs on.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Workers {
    /// How many.
    pub(crate) threads: usize,
    /// Called by each as it starts, to lower its priority.
    pub(crate) lower: Option<fn()>,
}

impl Workers {
    /// `threads` threads at the usual priority.
    pub(crate) fn new(threads: usize) -> Self {
        Self {
            threads,
            lower: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lowered() {}

    #[test]
    fn a_build_takes_every_core_while_no_ride_runs() {
        let pace = Pace::new(Meters(1000.0), Some(lowered));
        let workers = pace.workers(Some(500.0), 8);
        assert_eq!(workers.threads, 8);
        assert!(workers.lower.is_none(), "nothing is drawn to give way to");
    }

    #[test]
    fn a_ride_leaves_half_the_cores_at_a_higher_priority_while_the_build_is_well_ahead() {
        let pace = Pace::new(Meters(1000.0), Some(lowered));
        pace.ride_at(Some(Meters(0.0)));
        let workers = pace.workers(Some(5000.0), 8);
        assert_eq!(workers.threads, 4);
        assert!(workers.lower.is_some());
        assert_eq!(pace.workers(None, 8).threads, 4, "nothing left ahead");
        assert_eq!(pace.workers(None, 1).threads, 1, "at least one");
    }

    #[test]
    fn a_build_takes_every_core_once_its_lead_falls_short_until_it_has_doubled() {
        let pace = Pace::new(Meters(1000.0), Some(lowered));
        pace.ride_at(Some(Meters(10_000.0)));
        assert_eq!(pace.workers(Some(13_000.0), 8).threads, 4);
        // Below twice the radius.
        let hurried = pace.workers(Some(11_900.0), 8);
        assert_eq!(hurried.threads, 8);
        assert!(hurried.lower.is_some(), "drawing still comes first");
        assert!(pace.hurrying());
        assert_eq!(
            pace.workers(Some(13_900.0), 8).threads,
            8,
            "not doubled yet"
        );
        assert_eq!(pace.workers(Some(14_000.0), 8).threads, 4, "doubled");
        assert!(!pace.hurrying());
        // The rider caught up.
        pace.ride_at(Some(Meters(14_500.0)));
        assert_eq!(pace.workers(Some(14_000.0), 8).threads, 8);
        // The ride ended.
        pace.ride_at(None);
        assert_eq!(pace.workers(Some(14_000.0), 8).threads, 8);
        assert!(!pace.hurrying());
    }
}
