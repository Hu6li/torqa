use super::*;

fn rider(distance: f64, speed: f64) -> Rider {
    Rider {
        generation: 1,
        distance: Meters(distance),
        speed: MetersPerSecond(speed),
        power: Watts(200.0),
        cadence: Rpm(85.0),
        age: Duration::ZERO,
    }
}

fn peers(members: &[Rider]) -> [Option<Rider>; MAX_RIDERS] {
    std::array::from_fn(|id| members.get(id).copied())
}

fn band(members: &[Option<Rider>; MAX_RIDERS], local: Rider) -> f64 {
    assistance(members, local.distance, local.speed, true, false, true)
        .grade_offset()
        .0
}

#[test]
fn shared_reference_supports_the_back_brakes_the_front_and_leaves_the_centre_alone() {
    let pack = [rider(80.0, 10.0), rider(100.0, 10.0), rider(120.0, 10.0)];
    for local in 0..pack.len() {
        let mut others = peers(&pack);
        others[local] = None;
        let (center, speed, _) =
            reference(&others, pack[local].distance, pack[local].speed).unwrap();
        assert_eq!(center, Meters(100.0), "every rider gets the same reference");
        assert_eq!(speed, MetersPerSecond(10.0));
        let expected = [-0.6, 0.0, 0.6][local];
        assert!((band(&others, pack[local]) - expected).abs() < 1e-9);
        others.rotate_left(3);
        assert!((band(&others, pack[local]) - expected).abs() < 1e-9);
    }
    let back = rider(0.0, 10.0);
    let front = rider(80.0, 10.0);
    let back_band = band(&peers(&[front]), back);
    let front_band = band(&peers(&[back]), front);
    assert!(back_band < 0.0 && front_band > 0.0);
    assert!(
        (back_band + front_band).abs() < 1e-9,
        "two riders share their midpoint"
    );
    assert_eq!(band(&[None; MAX_RIDERS], back), 0.0);
}

#[test]
fn one_sprint_has_bounded_influence_and_the_local_sprinter_is_treated_equally() {
    let group = [rider(100.0, 10.0), rider(100.0, 10.0)];
    for sprint in [rider(200.0, 20.0), rider(1_000_000.0, 150.0)] {
        let others = peers(&[group[1], sprint]);
        let (center, pace, _) = reference(&others, group[0].distance, group[0].speed).unwrap();
        assert!((center.0 - (100.0 + 25.0 / 3.0)).abs() < 1e-9);
        assert!((pace.0 - (10.0 + 1.0 / 3.0)).abs() < 1e-9);
        assert!(band(&others, group[0]).abs() < 0.6);
        let (own_center, own_pace, _) =
            reference(&peers(&group), sprint.distance, sprint.speed).unwrap();
        assert!((own_center.0 - center.0).abs() < 1e-9);
        assert!((own_pace.0 - pace.0).abs() < 1e-9);
        assert!(band(&peers(&group), sprint) > 0.0);
    }
}

#[test]
fn stopped_riders_do_not_anchor_but_moving_coasting_riders_still_participate() {
    let local = rider(100.0, 10.0);
    let moving = rider(120.0, 10.0);
    let baseline = band(&peers(&[moving]), local);
    for speed in [0.0, 0.5] {
        let stopped = rider(0.0, speed);
        assert_eq!(band(&peers(&[moving, stopped]), local), baseline);
        assert_eq!(band(&peers(&[stopped]), local), 0.0);
    }
    let coasting = Rider {
        power: Watts(0.0),
        cadence: Rpm(0.0),
        ..moving
    };
    assert_eq!(band(&peers(&[coasting]), local), baseline);
    assert!(
        band(&peers(&[moving]), rider(0.0, 0.0)) < 0.0,
        "pedaling can restart a stopped rider"
    );
    assert_eq!(
        assistance(
            &peers(&[moving]),
            local.distance,
            local.speed,
            false,
            true,
            true
        ),
        GroupEffect::default(),
        "local coasting never supplies propulsion"
    );
}

#[test]
fn stale_riders_lose_influence_even_with_a_fresh_peer_and_expire_completely() {
    let local = rider(100.0, 10.0);
    let moving = rider(102.0, 10.0);
    let outlier = Rider {
        age: Duration::from_millis(1999),
        ..rider(1_000_000.0, 150.0)
    };
    let with_stale = peers(&[moving, outlier, outlier]);
    let (center, pace, _) = reference(&with_stale, local.distance, local.speed).unwrap();
    assert!(
        (center.0 - 101.0).abs() < 0.1,
        "low-weight outliers cannot choose the median"
    );
    assert!((pace.0 - 10.0).abs() < 0.01);
    assert!(band(&with_stale, local).abs() < 0.01);
    assert!(band(&peers(&[outlier]), local).abs() < 0.002);
    let expired = Rider {
        age: STALE,
        ..outlier
    };
    assert_eq!(
        band(&peers(&[moving, expired]), local),
        band(&peers(&[moving]), local)
    );
    assert_eq!(band(&peers(&[expired]), local), 0.0);
}

#[test]
fn relative_speed_can_brake_while_behind_and_nearby_equal_speed_riders_are_unaffected() {
    let peer = rider(110.0, 10.0);
    assert!(band(&peers(&[peer]), rider(100.0, 14.0)) > 0.0);
    for gap in [-8.0, 0.0, 8.0] {
        assert_eq!(band(&peers(&[peer]), rider(110.0 + gap, 10.0)), 0.0);
    }
}
