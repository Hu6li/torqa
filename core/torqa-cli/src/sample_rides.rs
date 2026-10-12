//! Made-up rides for screenshots and for trying out the analysis across rides (R62): a season
//! of workouts written to the rider's history like their own rides. The rider builds up, takes
//! a week off and comes back with short, hard efforts, so a recent period differs from the
//! rider's best: the top end is back, the threshold not yet.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use clap::Args;
use torqa_app::paths;
use torqa_domain::profile::Profile;
use torqa_domain::recording::Sample;
use torqa_domain::units::{BeatsPerMinute, Meters, MetersPerSecond, Rpm, Watts};
use torqa_session::analysis::{analyse, summarize};
use torqa_storage::profiles;
use torqa_storage::rides::{self, RideRecord};

const DAY: Duration = Duration::from_hours(24);

#[derive(Args)]
pub struct SampleRidesArgs {
    /// How many weeks of rides, up to today.
    #[arg(long, default_value_t = 16)]
    weeks: u32,
    /// The data directory to write to (default: Torqa's).
    #[arg(long)]
    data_dir: Option<PathBuf>,
}

/// A workout of the made-up rider's week: its name and its power as shares of FTP, in
/// stretches of seconds.
struct Session {
    name: &'static str,
    stretches: Vec<(u32, f64)>,
}

/// Writes the rides of `args.weeks` weeks, three a week but for the week off six weeks ago, to
/// the active rider's history (a new default rider if there is none). Rides of the same day are
/// written again, not added.
pub fn write(args: &SampleRidesArgs) -> Result<()> {
    let data_dir = args.data_dir.clone().unwrap_or_else(paths::data_dir);
    let (id, profile) = rider(&data_dir)?;
    let dir = profiles::rides_dir(&data_dir, &id);
    std::fs::create_dir_all(&dir).with_context(|| format!("cannot create {}", dir.display()))?;
    let now = SystemTime::now();
    let today = now
        .duration_since(UNIX_EPOCH)
        .map(|d| UNIX_EPOCH + DAY * u32::try_from(d.as_secs() / DAY.as_secs()).unwrap_or(0))
        .context("the clock is before 1970")?;
    let mut noise = Noise(0x5eed_cafe);
    let mut count = 0;
    // The week off, after which the rider comes back.
    let comeback = args.weeks.saturating_sub(6);
    for week in (0..args.weeks).filter(|&week| week != comeback) {
        let sessions = if week < comeback {
            // FTP grows from 215 W to 250 W while building up.
            let ftp = 215.0 + 35.0 * f64::from(week) / f64::from(comeback.max(1));
            let intervals = if week.is_multiple_of(2) {
                vo2max(1.12 + 0.01 * f64::from(week))
            } else {
                threshold()
            };
            let weekend = if week % 3 == 2 {
                sprints(3.4)
            } else {
                long_ride()
            };
            [(ftp, intervals), (ftp, endurance()), (ftp, weekend)]
        } else {
            // Back after the week off at a lower FTP, with harder short efforts.
            let ftp = 232.0;
            let weekend = if week % 2 == 1 {
                sprints(3.65)
            } else {
                long_ride()
            };
            [(ftp, vo2max(1.27)), (ftp, endurance()), (ftp, weekend)]
        };
        for (weekday, (ftp, session)) in [1, 3, 5].into_iter().zip(sessions) {
            let start = today - DAY * (7 * (args.weeks - week)) + DAY * weekday + DAY / 4;
            if start > now {
                continue;
            }
            let samples = ride(&session, ftp, &mut noise);
            let fit = dir.join(paths::activity_file_name(start));
            std::fs::write(&fit, torqa_storage::encode_fit(start, &samples)?)
                .with_context(|| format!("cannot write {}", fit.display()))?;
            let record = RideRecord {
                route: session.name.to_owned(),
                start,
                summary: summarize(&samples, Watts(ftp.round())),
                route_key: None,
                route_time: None,
                climbs: Vec::new(),
                name: None,
                ftp_estimate: None,
                rider_mass: Some(profile.rider_mass),
                simulated: false,
                analysis: Some(analyse(&samples)),
                uploads: std::collections::BTreeMap::new(),
            };
            rides::save(&fit, &record)?;
            count += 1;
        }
    }
    println!("{count} rides written to {}", dir.display());
    Ok(())
}

/// The rider Torqa starts with: the active one, else the first, else a new default rider.
fn rider(data_dir: &Path) -> Result<(String, Profile)> {
    let listed = profiles::list(data_dir);
    if let Some(found) = profiles::active(data_dir)
        .and_then(|id| listed.iter().find(|p| p.id == id).cloned())
        .or_else(|| listed.into_iter().next())
    {
        return Ok((found.id, found.profile));
    }
    let profile = Profile::default();
    let id = profiles::new_id(data_dir, &profile.name);
    profiles::save(data_dir, &id, &profile)?;
    profiles::set_active(data_dir, &id)?;
    Ok((id, profile))
}

fn vo2max(share: f64) -> Session {
    Session {
        name: "VO2max 5×5 min",
        stretches: [(900, 0.6)]
            .into_iter()
            .chain([(300, share), (300, 0.5)].repeat(5))
            .chain([(600, 0.55)])
            .collect(),
    }
}

fn threshold() -> Session {
    Session {
        name: "Threshold 2×20 min",
        stretches: vec![
            (900, 0.6),
            (1200, 0.97),
            (300, 0.5),
            (1200, 0.97),
            (600, 0.55),
        ],
    }
}

fn endurance() -> Session {
    Session {
        name: "Endurance 1 h",
        stretches: vec![(3600, 0.66)],
    }
}

fn sprints(share: f64) -> Session {
    Session {
        name: "Sprints 6×15 s",
        stretches: [(1200, 0.62)]
            .into_iter()
            .chain([(15, share), (165, 0.5)].repeat(6))
            .chain([(1200, 0.68)])
            .collect(),
    }
}

fn long_ride() -> Session {
    Session {
        name: "Long ride 2 h",
        stretches: vec![(2400, 0.68), (1200, 0.85), (3600, 0.68)],
    }
}

/// A second-by-second ride of `session` at `ftp`: power a little uneven, heart rate following
/// it with a lag, speed as on a flat road.
fn ride(session: &Session, ftp: f64, noise: &mut Noise) -> Vec<Sample> {
    let mut samples = Vec::new();
    let mut distance = 0.0;
    let mut heart_rate = 95.0;
    for &(seconds, share) in &session.stretches {
        for _ in 0..seconds {
            let power = (ftp * share * (0.94 + 0.12 * noise.next())).round();
            heart_rate += (95.0 + 75.0 * share.min(1.15) - heart_rate) / 40.0;
            // Speed on a flat road where drag takes the power: P = 0.18 · v³.
            let speed = (power / 0.18).cbrt();
            distance += speed;
            samples.push(Sample {
                elapsed: Duration::from_secs(samples.len() as u64),
                location: None,
                distance: Meters(distance),
                speed: MetersPerSecond(speed),
                power: Some(Watts(power)),
                cadence: Some(Rpm((88.0 + 6.0 * noise.next()).round())),
                heart_rate: Some(BeatsPerMinute(heart_rate.round())),
            });
        }
    }
    samples
}

/// Repeatable unevenness (xorshift), so the same rides come out every time.
struct Noise(u64);

impl Noise {
    /// The next number from 0 to 1.
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        #[allow(clippy::cast_precision_loss)] // 53 bits fit an f64 exactly
        let fraction = (self.0 >> 11) as f64 / (1_u64 << 53) as f64;
        fraction
    }
}
