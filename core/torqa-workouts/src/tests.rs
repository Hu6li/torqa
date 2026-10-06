use std::io::Cursor;
use std::time::Duration;

use embedded_io_adapters::std::FromStd;
use rustyfit::Encoder;
use rustyfit::profile::{mesgdef, typedef};
use rustyfit::proto::{FIT, Message};
use torqa_domain::units::{Rpm, Watts};
use torqa_domain::workout::{Cue, Intensity, Plan, Step, Target, WorkoutParser};

use super::*;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("torqa-workouts-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn ftp_shares(plan: &Plan) -> Vec<(u64, Option<(f64, f64)>)> {
    plan.steps
        .iter()
        .map(|step| {
            let power = match step.target {
                Target::Power {
                    from: Intensity::Ftp(a),
                    to: Intensity::Ftp(b),
                } => Some(((a * 100.0).round(), (b * 100.0).round())),
                Target::Power { .. } => panic!("watts in {step:?}"),
                Target::Free => None,
            };
            (step.duration.as_secs(), power)
        })
        .collect()
}

#[test]
fn zwo_steps_ramps_intervals_and_free_rides() {
    let xml = r#"<workout_file>
        <name>Mixed</name><description>All kinds</description><sportType>bike</sportType>
        <workout>
            <Warmup Duration="600" PowerLow="0.75" PowerHigh="0.25"/>
            <SteadyState Duration="300" PowerLow="0.8" PowerHigh="0.9" Cadence="95">
                <textevent timeoffset="10" message="Settle in"/>
            </SteadyState>
            <IntervalsT Repeat="2" OnDuration="60" OffDuration="30" PowerOnLow="1.1" PowerOnHigh="1.3" OffPower="0.5" CadenceResting="85">
                <TextEvent timeoffset="0" mssage="Go!"></TextEvent>
            </IntervalsT>
            <Ramp Duration="120" PowerLow="0.9" PowerHigh="0.6"/>
            <Freeride Duration="180"/>
            <Cooldown Duration="300" PowerLow="0.3" PowerHigh="0.6"/>
        </workout></workout_file>"#;

    let plan = ZwoParser.parse(xml.as_bytes(), "file").unwrap();

    assert_eq!(
        (plan.name.as_str(), plan.description.as_str()),
        ("Mixed", "All kinds")
    );
    assert_eq!(
        ftp_shares(&plan),
        [
            // A warm-up rises and a cool-down falls, whichever value comes first.
            (600, Some((25.0, 75.0))),
            (300, Some((85.0, 85.0))),
            (60, Some((120.0, 120.0))),
            (30, Some((50.0, 50.0))),
            (60, Some((120.0, 120.0))),
            (30, Some((50.0, 50.0))),
            // A ramp goes in the order given.
            (120, Some((90.0, 60.0))),
            (180, None),
            (300, Some((60.0, 30.0))),
        ]
    );
    assert_eq!(plan.steps[1].cadence, Some(Rpm(95.0)));
    assert_eq!(
        plan.steps[3].cadence,
        Some(Rpm(85.0)),
        "resting cadence when off"
    );
    let cues: Vec<(u64, &str)> = plan
        .cues
        .iter()
        .map(|c| (c.at.as_secs(), c.text.as_str()))
        .collect();
    assert_eq!(cues, [(610, "Settle in"), (900, "Go!")]);
}

#[test]
fn zwo_files_that_are_not_cycling_workouts_are_refused() {
    let run = r#"<workout_file><sportType>run</sportType><workout>
        <SteadyState Duration="60" Power="1"/></workout></workout_file>"#;
    let distance = r#"<workout_file durationType="distance"><workout>
        <SteadyState Duration="1000" Power="1"/></workout></workout_file>"#;
    let gpx = r"<gpx><trk/></gpx>";

    for xml in [run, distance, gpx, "not xml <"] {
        assert!(ZwoParser.parse(xml.as_bytes(), "x").is_err(), "{xml}");
    }
    let unnamed = r#"<workout_file><workout><FreeRide Duration="60"/></workout></workout_file>"#;
    assert_eq!(
        ZwoParser
            .parse(unnamed.as_bytes(), "file name")
            .unwrap()
            .name,
        "file name"
    );
}

#[test]
fn erg_points_join_into_steps_in_watts_and_mrc_in_percent() {
    let erg = "[COURSE HEADER]\nVERSION = 2\nUNITS = ENGLISH\nDESCRIPTION = Over-unders\n\
        FILE NAME = OU\nMINUTES WATTS\n[END COURSE HEADER]\n[COURSE DATA]\n\
        0.00\t100\n10.00\t200\n10.00\t250\n12.00\t250\n12.00\t150\n15.00\t150\n\
        [END COURSE DATA]\n[COURSE TEXT]\n600\tHard now\t10\n[END COURSE TEXT]\n";

    let plan = ErgParser.parse(erg.as_bytes(), "x").unwrap();

    assert_eq!(
        (plan.name.as_str(), plan.description.as_str()),
        ("OU", "Over-unders")
    );
    assert_eq!(plan.duration(), Duration::from_mins(15));
    assert_eq!(plan.steps.len(), 3, "points at the same minute are steps");
    let watts = |s: u32, t: f64| {
        plan.at(Duration::from_secs_f64(f64::from(s) + t), Watts(999.0))
            .unwrap()
            .power
            .unwrap()
            .0
    };
    assert!(
        (watts(300, 0.0) - 150.0).abs() < 1e-9,
        "a ramp from 100 to 200 W"
    );
    assert!(
        (watts(600, 0.0) - 250.0).abs() < 1e-9,
        "watts whatever the FTP"
    );
    assert_eq!(plan.cues[0].at, Duration::from_secs(600));
    assert_eq!(plan.cues[0].text, "Hard now");

    let mrc = erg.replace("MINUTES WATTS", "MINUTES PERCENT");
    let percent = ErgParser.parse(mrc.as_bytes(), "x").unwrap();
    let at = percent.at(Duration::from_secs(700), Watts(200.0)).unwrap();
    assert!((at.power.unwrap().0 - 500.0).abs() < 1e-9, "250 % of 200 W");

    let headless = "[COURSE DATA]\n0\t100\n10\t100\n[END COURSE DATA]\n";
    assert!(ErgParser.parse(headless.as_bytes(), "x").is_err());
}

/// A FIT workout file of `steps`, as Garmin Connect writes them.
fn fit_workout(name: &str, steps: Vec<mesgdef::WorkoutStep>) -> Vec<u8> {
    let mut file_id = mesgdef::FileId::new();
    file_id.r#type = typedef::File::WORKOUT;
    let mut workout = mesgdef::Workout::new();
    workout.wkt_name = name.to_owned();
    workout.sport = typedef::Sport::CYCLING;
    workout.num_valid_steps = u16::try_from(steps.len()).unwrap();
    let mut messages = vec![Message::from(file_id), Message::from(workout)];
    messages.extend(steps.into_iter().map(Message::from));
    let mut fit = FIT {
        messages,
        ..Default::default()
    };
    let mut buffer = Cursor::new(Vec::new());
    Encoder::new()
        .encode(FromStd::new(&mut buffer), &mut fit)
        .unwrap();
    buffer.into_inner()
}

fn fit_step(
    seconds: u32,
    target: typedef::WktStepTarget,
    low: u32,
    high: u32,
) -> mesgdef::WorkoutStep {
    let mut step = mesgdef::WorkoutStep::new();
    step.duration_type = typedef::WktStepDuration::TIME;
    step.duration_value = seconds * 1000;
    step.target_type = target;
    step.custom_target_value_low = low;
    step.custom_target_value_high = high;
    step
}

#[test]
fn fit_workouts_hold_the_middle_of_their_power_ranges_and_repeat_blocks() {
    let mut warm_up = fit_step(600, typedef::WktStepTarget::POWER, 50, 60);
    warm_up.wkt_step_name = "Warm up".to_owned();
    let mut repeat = mesgdef::WorkoutStep::new();
    repeat.duration_type = typedef::WktStepDuration::REPEAT_UNTIL_STEPS_CMPLT;
    repeat.duration_value = 1;
    repeat.target_value = 3;
    let mut zone = fit_step(300, typedef::WktStepTarget::POWER, 0, 0);
    zone.target_value = 2;
    let bytes = fit_workout(
        "Garmin intervals",
        vec![
            warm_up,
            fit_step(60, typedef::WktStepTarget::POWER, 1280, 1320),
            fit_step(120, typedef::WktStepTarget::OPEN, 0, 0),
            repeat,
            fit_step(180, typedef::WktStepTarget::CADENCE, 85, 95),
            zone,
        ],
    );

    let plan = FitParser.parse(&bytes, "x").unwrap();

    assert_eq!(plan.name, "Garmin intervals");
    let durations: Vec<u64> = plan.steps.iter().map(|s| s.duration.as_secs()).collect();
    assert_eq!(durations, [600, 60, 120, 60, 120, 60, 120, 180, 300]);
    assert_eq!(
        plan.steps[0].target,
        Target::steady(Intensity::Ftp(0.55)),
        "the middle of 50–60 % of FTP"
    );
    assert_eq!(
        plan.steps[5].target,
        Target::steady(Intensity::Watts(Watts(300.0))),
        "the middle of 280–320 W, three times"
    );
    assert_eq!(plan.steps[6].target, Target::Free);
    assert_eq!(
        (plan.steps[7].target, plan.steps[7].cadence),
        (Target::Free, Some(Rpm(90.0)))
    );
    assert_eq!(
        plan.steps[8].target,
        Target::steady(Intensity::Ftp(0.65)),
        "zone 2"
    );
    assert_eq!(plan.cues[0].text, "Warm up");
}

#[test]
fn fit_files_other_than_time_based_workouts_are_refused() {
    let mut by_distance = fit_step(0, typedef::WktStepTarget::OPEN, 0, 0);
    by_distance.duration_type = typedef::WktStepDuration::DISTANCE;
    let distance = fit_workout("x", vec![by_distance]);
    let error = FitParser.parse(&distance, "x").unwrap_err();
    assert!(error.0.contains("distance"), "{error}");

    let mut file_id = mesgdef::FileId::new();
    file_id.r#type = typedef::File::ACTIVITY;
    let mut fit = FIT {
        messages: vec![Message::from(file_id)],
        ..Default::default()
    };
    let mut buffer = Cursor::new(Vec::new());
    Encoder::new()
        .encode(FromStd::new(&mut buffer), &mut fit)
        .unwrap();
    assert!(
        FitParser.parse(&buffer.into_inner(), "x").is_err(),
        "a ride is no workout"
    );
}

#[test]
fn the_built_in_workouts_are_all_there() {
    let builtins = builtins();

    assert_eq!(builtins.len(), BUILTINS.len(), "every built-in parses");
    let sweet_spot = load("builtin:sweet-spot-3x10").unwrap();
    assert_eq!(sweet_spot.duration(), Duration::from_mins(58));
    assert!(builtins.iter().all(|b| !b.plan.description.is_empty()));
    assert!(load("builtin:nothing").is_err());
}

#[test]
fn imported_workouts_join_the_library_after_the_built_ins() {
    let dir = temp_dir("library");
    let downloads = temp_dir("downloads");
    let zwo = downloads.join("Tempo.zwo");
    std::fs::write(
        &zwo,
        r#"<workout_file><name>Tempo</name><workout><SteadyState Duration="1200" Power="0.8"/></workout></workout_file>"#,
    )
    .unwrap();
    let broken = downloads.join("broken.zwo");
    std::fs::write(&broken, "<workout_file><workout/></workout_file>").unwrap();

    let first = import(&dir, &zwo).unwrap();
    let second = import(&dir, &zwo).unwrap();
    assert!(import(&dir, &broken).is_err());
    assert!(import(&dir, &downloads.join("ride.gpx")).is_err());

    assert_ne!(first, second, "a copy of its own");
    let library = library(&dir);
    assert_eq!(
        library.len(),
        BUILTINS.len() + 2,
        "the broken one is not copied"
    );
    assert!(
        library[..BUILTINS.len()]
            .iter()
            .all(|e| e.id.starts_with(BUILTIN))
    );
    let added = &library[BUILTINS.len()];
    assert_eq!(added.plan.name, "Tempo");
    assert_eq!(load(&added.id).unwrap(), added.plan);
}

#[test]
fn zones_without_power_hold_their_middle() {
    assert!((zone_share(1.0) - 0.45).abs() < 1e-9);
    assert!((zone_share(4.0) - 0.98).abs() < 1e-9);
    assert!((zone_share(9.0) - 1.6).abs() < 1e-9);
}

fn edited() -> Plan {
    Plan {
        name: "Over & under <3>".to_owned(),
        description: "Hard \"but\" fair".to_owned(),
        steps: vec![
            Step {
                duration: Duration::from_mins(5),
                target: Target::steady(Intensity::Ftp(0.5)),
                cadence: None,
            },
            Step {
                duration: Duration::from_secs(90),
                target: Target::Power {
                    from: Intensity::Ftp(0.5),
                    to: Intensity::Ftp(0.8),
                },
                cadence: Some(Rpm(95.0)),
            },
            Step {
                duration: Duration::from_mins(1),
                target: Target::steady(Intensity::Watts(Watts(300.0))),
                cadence: None,
            },
            Step {
                duration: Duration::from_mins(2),
                target: Target::Free,
                cadence: None,
            },
        ],
        cues: vec![
            Cue {
                at: Duration::from_mins(5),
                text: "Ramp up".to_owned(),
            },
            Cue {
                at: Duration::from_secs(400),
                text: "Last bit & go".to_owned(),
            },
        ],
    }
}

#[test]
fn written_zwo_files_read_back_as_the_workout() {
    let plan = edited();

    let xml = write_zwo(&plan, Watts(250.0));
    let back = ZwoParser.parse(xml.as_bytes(), "x").unwrap();

    let mut expected = plan.clone();
    // Watts are written as a share of the rider's FTP.
    expected.steps[2].target = Target::steady(Intensity::Ftp(1.2));
    assert_eq!(back, expected, "{xml}");
}

#[test]
fn saved_workouts_replace_only_their_own_zwo_file() {
    let dir = temp_dir("save");
    let mut plan = edited();
    plan.name = "Tempo/Sweet spot".to_owned();

    let first = save(&dir, &plan, Watts(250.0), None).unwrap();
    plan.description = "Changed".to_owned();
    let again = save(&dir, &plan, Watts(250.0), Some(&first)).unwrap();
    let copy = save(&dir, &plan, Watts(250.0), Some("builtin:recovery-30")).unwrap();

    assert_eq!(again, first, "the edited file is replaced");
    assert!(first.ends_with("Tempo_Sweet spot.zwo"), "{first}");
    assert_ne!(copy, first, "an edited built-in becomes a file of its own");
    assert_eq!(load(&first).unwrap().description, "Changed");
    let empty = Plan {
        steps: Vec::new(),
        ..plan
    };
    assert!(save(&dir, &empty, Watts(250.0), None).is_err());

    delete(&dir, &copy).unwrap();
    assert!(!Path::new(&copy).exists());
    assert!(delete(&dir, "builtin:recovery-30").is_err());
    assert!(
        delete(&dir, "/etc/hosts").is_err(),
        "only files of the library"
    );
}
