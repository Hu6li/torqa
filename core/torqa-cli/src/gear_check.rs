//! Checking virtual gears on a real trainer (R9, ADR 0003): ride a virtual road in a gear and
//! compare what the trainer brakes with what the gear should feel like.

use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use clap::Args;
use torqa_devices::DeviceEvent;
use torqa_domain::shifting::{ButtonMap, Shift};
use torqa_domain::telemetry::{SimulationParameters, Telemetry, TrainerControl};
use torqa_domain::units::{GradePercent, Kilograms, MetersPerSecond, Rpm, Watts};
use torqa_physics::{GEARS, Motion, RiderSetup, VirtualGears};

use crate::devices::{DeviceArgs, Devices, field, next_event, stdin_lines};

const GRAVITY: f64 = 9.81;
const TICK: Duration = Duration::from_millis(250);
/// Below this cadence ratios say nothing: the rider is coasting or just starting.
const MIN_CADENCE: f64 = 20.0;
const HELP: &str = "Commands: u / d shift up and down | <gear 1–24> | g <grade %> | q";

#[derive(Args)]
pub(crate) struct GearCheckArgs {
    #[command(flatten)]
    pub(crate) devices: DeviceArgs,
    /// Chainring x cog teeth on the trainer (e.g. 50x14): the real gear the virtual ones scale.
    #[arg(long, value_parser = crate::parse_gears, default_value = "50x14")]
    gears: (u8, u8),
    /// Rider plus bike mass in kg.
    #[arg(long, default_value_t = 83.0)]
    mass: f64,
    /// Wheel circumference in metres, for turning cadence into speed (700x25c: 2.105).
    #[arg(long, default_value_t = 2.105)]
    wheel: f64,
}

pub(crate) async fn run(args: &GearCheckArgs, devices: &mut Devices) -> Result<()> {
    let (chainring, cog) = args.gears;
    let check = GearCheck {
        gears: VirtualGears::new(chainring, cog),
        real_ratio: f64::from(chainring) / f64::from(cog),
        setup: RiderSetup {
            mass: Kilograms(args.mass),
            ..RiderSetup::default()
        },
        wheel: args.wheel,
    };
    let buttons = ButtonMap::shifting(args.devices.up_channel, args.devices.down_channel);
    let mut gear = check.gears.neutral();
    let mut grade = GradePercent(0.0);
    let mut motion = Motion::default();
    let mut latest = Telemetry::default();
    let mut second = Average::default();
    let mut input = stdin_lines();
    let mut ticker = tokio::time::interval(TICK);
    let mut display = tokio::time::interval(Duration::from_secs(1));
    let mut last_tick = Instant::now();

    println!("{HELP}");
    println!(
        "Real gear {chainring}x{cog} = {:.2}. Ride steadily for a few seconds after each shift.",
        check.real_ratio
    );
    devices
        .trainer
        .control(TrainerControl::Simulation(check.sent(grade, gear)))
        .await?;
    loop {
        let before = (gear, grade);
        tokio::select! {
            event = devices.trainer.next_event() => {
                match event.context("trainer driver stopped")? {
                    DeviceEvent::Connected => println!("{}: connected", devices.trainer.name()),
                    DeviceEvent::Disconnected => {
                        println!("{}: disconnected, reconnecting…", devices.trainer.name());
                        latest = Telemetry::default();
                    }
                    DeviceEvent::Telemetry(telemetry) => {
                        latest.merge(&telemetry);
                        second.add(&telemetry);
                    }
                    DeviceEvent::Buttons(_) => {}
                }
            }
            event = next_event(devices.controller.as_mut()) => {
                match event.context("controller driver stopped")? {
                    (_, DeviceEvent::Buttons(presses)) => {
                        for action in buttons.actions(&presses) {
                            for &shift in action.shifts() {
                                gear = shifted(gear, shift);
                            }
                        }
                    }
                    (name, DeviceEvent::Connected) => println!("{name}: connected"),
                    (name, DeviceEvent::Disconnected) => println!("{name}: disconnected"),
                    (_, DeviceEvent::Telemetry(_)) => {}
                }
            }
            _ = ticker.tick() => {
                let now = Instant::now();
                let power = latest.power.unwrap_or(Watts(0.0));
                motion.step(&check.setup, power, grade, MetersPerSecond(0.0), now - last_tick);
                last_tick = now;
            }
            _ = display.tick() => {
                let reading = std::mem::take(&mut second).reading();
                println!("{}", check.line(gear, grade, &reading, motion.speed()));
            }
            line = input.recv() => {
                let Some(line) = line else { break };
                match parse_input(&line) {
                    Ok(Input::Shift(shift)) => gear = shifted(gear, shift),
                    Ok(Input::Grade(new)) => grade = new,
                    Ok(Input::Quit) => break,
                    Ok(Input::Nothing) => {}
                    Err(message) => eprintln!("{message}\n{HELP}"),
                }
            }
            _ = tokio::signal::ctrl_c() => break,
        }
        if (gear, grade) != before {
            devices
                .trainer
                .control(TrainerControl::Simulation(check.sent(grade, gear)))
                .await?;
        }
    }
    Ok(())
}

/// The gears under test and how to judge a reading.
struct GearCheck {
    gears: VirtualGears,
    real_ratio: f64,
    setup: RiderSetup,
    wheel: f64,
}

/// What a second of riding in a gear says about it.
#[derive(Debug, PartialEq)]
struct Verdict {
    /// The gear the trainer turns in, from its speed and the cadence: should be the real one.
    felt_ratio: Option<f64>,
    /// What the trainer should brake at its speed with the parameters sent for the gear.
    expected: Option<Watts>,
    /// What it would brake if it ignored the gear: the road's parameters at that speed.
    ungeared: Option<Watts>,
    /// The gear Torqa's virtual speed rides in at this cadence: settles at the virtual gear's
    /// ratio when the trainer brakes as expected.
    virtual_ratio: Option<f64>,
}

impl GearCheck {
    /// The road as the trainer simulates it, without the gear.
    fn road(&self, grade: GradePercent) -> SimulationParameters {
        self.setup.simulation_parameters(grade)
    }

    /// What is sent to the trainer in `gear`.
    fn sent(&self, grade: GradePercent, gear: usize) -> SimulationParameters {
        self.gears.in_gear(self.road(grade), gear)
    }

    fn judge(
        &self,
        grade: GradePercent,
        gear: usize,
        reading: &Reading,
        virtual_speed: MetersPerSecond,
    ) -> Verdict {
        // Wheel speed per unit of gear ratio at this cadence.
        let per_ratio = reading
            .cadence
            .filter(|c| c.0 >= MIN_CADENCE)
            .map(|c| c.0 / 60.0 * self.wheel);
        let trainer = reading.speed.map(|v| v.0);
        let power = |parameters: SimulationParameters| {
            trainer.map(|v| Watts((brake_force(&parameters, self.setup.mass, v) * v).max(0.0)))
        };
        Verdict {
            felt_ratio: trainer.zip(per_ratio).map(|(v, per)| v / per),
            expected: power(self.sent(grade, gear)),
            ungeared: power(self.road(grade)),
            virtual_ratio: per_ratio.map(|per| virtual_speed.0 / per),
        }
    }

    fn line(
        &self,
        gear: usize,
        grade: GradePercent,
        reading: &Reading,
        virtual_speed: MetersPerSecond,
    ) -> String {
        let sent = self.sent(grade, gear);
        let verdict = self.judge(grade, gear, reading, virtual_speed);
        let ratio = |r: Option<f64>| r.map_or_else(|| "--".to_owned(), |r| format!("{r:.2}"));
        let watts = |w: Option<Watts>| w.map_or_else(|| "--".to_owned(), |w| format!("{:.0}", w.0));
        format!(
            "gear {:>2} {:.2}  sent {:+.1} % Crr {:.4} Cw {:.2} | {} {} {} | felt {} (real {:.2}) | \
             expect {} W, ungeared {} W | virtual {:.1} km/h = {}",
            gear + 1,
            self.gears.ratio(gear),
            sent.grade.0,
            sent.crr,
            sent.cw.0,
            field(reading.power.map(|p| p.0), "W", 0),
            field(reading.cadence.map(|c| c.0), "rpm", 0),
            field(
                reading.speed.map(MetersPerSecond::as_kilometers_per_hour),
                "km/h",
                1
            ),
            ratio(verdict.felt_ratio),
            self.real_ratio,
            watts(verdict.expected),
            watts(verdict.ungeared),
            virtual_speed.as_kilometers_per_hour(),
            ratio(verdict.virtual_ratio),
        )
    }
}

/// The force an FTMS trainer brakes with in slope simulation at wheel speed `v`.
fn brake_force(p: &SimulationParameters, mass: Kilograms, v: f64) -> f64 {
    let theta = (p.grade.0 / 100.0).atan();
    let air = v + p.wind_speed.0;
    mass.0 * GRAVITY * (theta.sin() + p.crr * theta.cos()) + p.cw.0 * air * air.abs()
}

/// Telemetry averaged over a second, each field over the packets that carried it.
#[derive(Debug, Default)]
struct Average {
    power: (f64, u32),
    cadence: (f64, u32),
    speed: (f64, u32),
}

/// A second of riding.
#[derive(Debug, Default, PartialEq)]
struct Reading {
    power: Option<Watts>,
    cadence: Option<Rpm>,
    speed: Option<MetersPerSecond>,
}

impl Average {
    fn add(&mut self, telemetry: &Telemetry) {
        let add = |sum: &mut (f64, u32), value: Option<f64>| {
            if let Some(value) = value {
                sum.0 += value;
                sum.1 += 1;
            }
        };
        add(&mut self.power, telemetry.power.map(|p| p.0));
        add(&mut self.cadence, telemetry.cadence.map(|c| c.0));
        add(&mut self.speed, telemetry.speed.map(|s| s.0));
    }

    fn reading(&self) -> Reading {
        let mean = |(sum, n): (f64, u32)| (n > 0).then(|| sum / f64::from(n));
        Reading {
            power: mean(self.power).map(Watts),
            cadence: mean(self.cadence).map(Rpm),
            speed: mean(self.speed).map(MetersPerSecond),
        }
    }
}

fn shifted(gear: usize, shift: Shift) -> usize {
    match shift {
        Shift::Up => gear + 1,
        Shift::Down => gear.saturating_sub(1),
        Shift::To(number) => number.saturating_sub(1),
    }
    .min(GEARS - 1)
}

#[derive(Debug, PartialEq)]
enum Input {
    Shift(Shift),
    Grade(GradePercent),
    Quit,
    Nothing,
}

fn parse_input(line: &str) -> Result<Input, String> {
    let mut parts = line.split_whitespace();
    let Some(command) = parts.next() else {
        return Ok(Input::Nothing);
    };
    Ok(match command {
        "q" => Input::Quit,
        "u" | "+" => Input::Shift(Shift::Up),
        "d" | "-" => Input::Shift(Shift::Down),
        "g" => Input::Grade(GradePercent(
            parts
                .next()
                .ok_or("missing grade for \"g\"")?
                .parse()
                .map_err(|_| "grade must be a number")?,
        )),
        number => match number.parse::<usize>() {
            Ok(gear @ 1..=GEARS) => Input::Shift(Shift::To(gear)),
            _ => return Err(format!("unknown command {number:?}")),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check() -> GearCheck {
        GearCheck {
            gears: VirtualGears::new(50, 14),
            real_ratio: 50.0 / 14.0,
            setup: RiderSetup::default(),
            wheel: 2.105,
        }
    }

    /// 90 rpm on the real gear, as the trainer would report it.
    fn reading(power: f64) -> Reading {
        Reading {
            power: Some(Watts(power)),
            cadence: Some(Rpm(90.0)),
            speed: Some(MetersPerSecond(1.5 * 2.105 * 50.0 / 14.0)),
        }
    }

    #[test]
    fn the_hardest_gear_expects_the_power_of_its_road_speed() {
        let check = check();
        let hardest = GEARS - 1;
        // 90 rpm in 5.5 is 62.5 km/h: the road's force there, at the pedals' speed times 5.5.
        let road_speed = 1.5 * 2.105 * 5.5;
        let road = brake_force(&check.road(GradePercent(0.0)), Kilograms(83.0), road_speed);

        let verdict = check.judge(
            GradePercent(0.0),
            hardest,
            &reading(400.0),
            MetersPerSecond(road_speed),
        );

        let expected = verdict.expected.unwrap().0;
        assert!((expected - road * road_speed).abs() < 1e-6, "{expected} W");
        assert!((1050.0..1100.0).contains(&expected), "{expected} W");
        assert!(verdict.ungeared.unwrap().0 < 350.0, "{verdict:?}");
        assert!((verdict.felt_ratio.unwrap() - 50.0 / 14.0).abs() < 1e-9);
        assert!((verdict.virtual_ratio.unwrap() - 5.5).abs() < 1e-9);
    }

    #[test]
    fn the_real_gear_expects_what_the_road_brakes() {
        let check = check();
        let neutral = check.gears.neutral();

        let verdict = check.judge(
            GradePercent(5.0),
            neutral,
            &reading(300.0),
            MetersPerSecond(5.0),
        );

        // The neutral gear is within half a step (about 4.5 %) of the real one.
        let (expected, ungeared) = (verdict.expected.unwrap().0, verdict.ungeared.unwrap().0);
        assert!((expected / ungeared - 1.0).abs() < 0.1, "{verdict:?}");
    }

    #[test]
    fn coasting_or_missing_speed_says_nothing() {
        let check = check();
        let coasting = Reading {
            cadence: Some(Rpm(0.0)),
            ..reading(0.0)
        };
        let no_speed = Reading {
            speed: None,
            ..reading(200.0)
        };

        let verdict = check.judge(GradePercent(0.0), 5, &coasting, MetersPerSecond(8.0));
        assert_eq!((verdict.felt_ratio, verdict.virtual_ratio), (None, None));
        let verdict = check.judge(GradePercent(0.0), 5, &no_speed, MetersPerSecond(8.0));
        assert_eq!(verdict.felt_ratio, None);
        assert_eq!(verdict.expected, None);
        assert!(verdict.virtual_ratio.is_some());
    }

    #[test]
    fn descents_expect_no_negative_power() {
        let verdict = check().judge(
            GradePercent(-10.0),
            GEARS - 1,
            &reading(0.0),
            MetersPerSecond(10.0),
        );

        assert_eq!(verdict.expected, Some(Watts(0.0)));
    }

    #[test]
    fn averages_each_field_over_the_packets_carrying_it() {
        let mut average = Average::default();
        average.add(&Telemetry {
            power: Some(Watts(200.0)),
            cadence: Some(Rpm(80.0)),
            ..Telemetry::default()
        });
        average.add(&Telemetry {
            power: Some(Watts(300.0)),
            ..Telemetry::default()
        });

        assert_eq!(
            average.reading(),
            Reading {
                power: Some(Watts(250.0)),
                cadence: Some(Rpm(80.0)),
                speed: None,
            }
        );
    }

    #[test]
    fn parses_commands() {
        assert_eq!(parse_input("u"), Ok(Input::Shift(Shift::Up)));
        assert_eq!(parse_input(" - "), Ok(Input::Shift(Shift::Down)));
        assert_eq!(parse_input("24"), Ok(Input::Shift(Shift::To(24))));
        assert_eq!(parse_input("g -4.5"), Ok(Input::Grade(GradePercent(-4.5))));
        assert_eq!(parse_input("q"), Ok(Input::Quit));
        assert_eq!(parse_input(""), Ok(Input::Nothing));
        assert!(parse_input("25").is_err());
        assert!(parse_input("0").is_err());
        assert!(parse_input("g").is_err());
        assert!(parse_input("g steep").is_err());
    }

    #[test]
    fn shifts_stay_within_the_gears() {
        assert_eq!(shifted(GEARS - 1, Shift::Up), GEARS - 1);
        assert_eq!(shifted(0, Shift::Down), 0);
        assert_eq!(shifted(3, Shift::To(1)), 0);
        assert_eq!(shifted(3, Shift::Up), 4);
    }
}
