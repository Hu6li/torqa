use super::*;

fn shelter(gap: f64) -> f64 {
    let mut riders = [None; MAX_RIDERS];
    riders[1] = Some(Rider {
        generation: 1,
        distance: Meters(100.0 + gap),
        speed: MetersPerSecond(10.0),
        power: Watts(200.0),
        cadence: Rpm(85.0),
        age: Duration::ZERO,
    });
    (1.0 - assistance(
        &riders,
        Meters(100.0),
        MetersPerSecond(10.0),
        true,
        true,
        false,
    )
    .drag_share())
        / 0.3
}

#[test]
fn abreast_overlapping_and_out_of_range_riders_supply_no_drafting() {
    for gap in [-5.0, -0.01, 0.0, 0.01, 0.49, 1.0, 15.0, 30.0] {
        assert_eq!(shelter(gap), 0.0, "gap {gap} m");
    }
    assert!(
        shelter(3.0) > 0.99,
        "a rider clearly ahead supplies full shelter"
    );
}

#[test]
fn close_overtakes_and_range_boundaries_change_drafting_continuously() {
    let mut previous = 0.0;
    for mm in -1000..=16000 {
        let gap = f64::from(mm) / 1000.0;
        let current = shelter(gap);
        assert!((0.0..=1.000_000_001).contains(&current));
        assert!(
            (current - previous).abs() < 0.005,
            "gap {gap}: {previous} -> {current}"
        );
        previous = current;
    }
}

#[test]
fn live_prediction_does_not_extend_buffered_playbacks_missing_data_prediction() {
    let start = Instant::now();
    let mut riders = Riders::default();
    riders.observe(
        1,
        1,
        Sample {
            elapsed: Duration::ZERO,
            distance: Meters(100.0),
            speed: MetersPerSecond(10.0),
            power: Watts(200.0),
            cadence: Rpm(85.0),
        },
        start,
    );
    let elapsed = Duration::from_millis(1200);
    assert_eq!(
        riders.view(start + elapsed, elapsed)[1].unwrap().distance,
        Meters(103.0)
    );
    assert_eq!(
        riders.current(start + elapsed, elapsed)[1]
            .unwrap()
            .distance,
        Meters(108.0)
    );
    assert!(riders.current(start + STALE, STALE)[1].is_none());
}

#[test]
fn healthy_delivery_jitter_does_not_change_live_gaps_or_assistance() {
    for gap in [0.0, 3.0, 75.0] {
        let start = Instant::now();
        let mut riders = Riders::default();
        let mut packets = Vec::new();
        for sequence in 0..=50_u64 {
            if sequence % 9 == 3 {
                continue;
            }
            let ms = sequence * 200;
            let delay = [90, 330, 160, 510, 110][usize::try_from(sequence % 5).unwrap()];
            packets.push((
                ms + delay,
                Sample {
                    elapsed: Duration::from_millis(ms),
                    distance: Meters(100.0 + 10.0 * Duration::from_millis(ms).as_secs_f64() + gap),
                    speed: MetersPerSecond(10.0),
                    power: Watts(200.0),
                    cadence: Rpm(85.0),
                },
            ));
        }
        packets.sort_by_key(|p| p.0);
        packets.reverse();
        let mut previous: Option<GroupEffect> = None;
        for ms in (0..=9500).step_by(10) {
            let elapsed = Duration::from_millis(ms);
            let now = start + elapsed;
            while packets.last().is_some_and(|p| p.0 <= ms) {
                let (arrival, sample) = packets.pop().unwrap();
                riders.observe(1, 1, sample, start + Duration::from_millis(arrival));
            }
            if ms < 1000 {
                continue;
            }
            let current = riders.current(now, elapsed);
            let local = Meters(100.0 + 10.0 * elapsed.as_secs_f64());
            assert!(
                (current[1].unwrap().distance.0 - local.0 - gap).abs() < 1e-7,
                "gap {gap}, time {ms}: healthy delivery must not stop the live prediction"
            );
            let effect = assistance(&current, local, MetersPerSecond(10.0), true, true, true);
            if let Some(last) = previous {
                assert!((effect.drag_share() - last.drag_share()).abs() < 1e-7);
                assert!((effect.grade_offset().0 - last.grade_offset().0).abs() < 1e-7);
            }
            previous = Some(effect);
        }
    }
}
