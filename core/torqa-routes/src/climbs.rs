//! Climbs along a route (R27), found automatically from the elevation profile and categorised
//! like popular platforms do: by length × average gradient.

use torqa_domain::units::{GradePercent, Meters};

use crate::RoutePoint;

/// A climb starts once the road has risen this much above the lowest point before it.
const START_RISE_M: f64 = 5.0;
/// Short dips inside a climb are tolerated up to this drop, or a fifth of the gain so far.
const DIP_TOLERANCE_M: f64 = 10.0;
const DIP_SHARE: f64 = 0.2;
const MIN_LENGTH_M: f64 = 300.0;
const MIN_GRADE: f64 = 3.0;
/// Length in metres × average gradient in percent, from which a rise counts as a climb.
const MIN_SCORE: f64 = 3_000.0;

/// How hard a climb is, from length × average gradient (Cat 4 is the easiest category).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ClimbCategory {
    /// A noticeable rise below Cat 4.
    Uncategorized,
    /// Score from 8 000.
    Cat4,
    /// Score from 16 000.
    Cat3,
    /// Score from 32 000.
    Cat2,
    /// Score from 64 000.
    Cat1,
    /// Hors catégorie: score from 80 000.
    Hc,
}

impl ClimbCategory {
    fn from_score(score: f64) -> Self {
        match score {
            s if s >= 80_000.0 => Self::Hc,
            s if s >= 64_000.0 => Self::Cat1,
            s if s >= 32_000.0 => Self::Cat2,
            s if s >= 16_000.0 => Self::Cat3,
            s if s >= 8_000.0 => Self::Cat4,
            _ => Self::Uncategorized,
        }
    }

    /// Short label, e.g. `Cat 3` or `HC`.
    #[must_use]
    pub fn label(self) -> &'static str {
        // i18n-begin: translated by the front end.
        match self {
            Self::Uncategorized => "Climb",
            Self::Cat4 => "Cat 4",
            Self::Cat3 => "Cat 3",
            Self::Cat2 => "Cat 2",
            Self::Cat1 => "Cat 1",
            Self::Hc => "HC",
        }
        // i18n-end
    }
}

/// One climb of a route.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Climb {
    /// Distance from the route start to the foot of the climb.
    pub start: Meters,
    /// Distance from the route start to the top.
    pub end: Meters,
    /// Height gained from foot to top.
    pub gain: Meters,
    /// Gain over length.
    pub average_grade: GradePercent,
    /// Category.
    pub category: ClimbCategory,
}

impl Climb {
    /// Length from foot to top.
    #[must_use]
    pub fn length(&self) -> Meters {
        Meters(self.end.0 - self.start.0)
    }
}

/// The climbs along evenly spaced, smoothed route points, in order.
#[must_use]
pub fn detect(points: &[RoutePoint]) -> Vec<Climb> {
    let elevation = |i: usize| points[i].elevation.0;
    let mut climbs = Vec::new();
    // The foot moves along flat ground, so a flat approach is not part of the climb.
    let mut low = 0;
    let mut top: Option<(usize, usize)> = None; // (foot, highest point so far)
    for i in 1..points.len() {
        match top {
            None => {
                if elevation(i) <= elevation(low) {
                    low = i;
                } else if elevation(i) - elevation(low) >= START_RISE_M {
                    top = Some((low, i));
                }
            }
            Some((foot, high)) => {
                if elevation(i) > elevation(high) {
                    top = Some((foot, i));
                } else {
                    let gain = elevation(high) - elevation(foot);
                    if elevation(high) - elevation(i) > DIP_TOLERANCE_M.max(gain * DIP_SHARE) {
                        climbs.extend(qualify(points, foot, high));
                        top = None;
                        low = i;
                    }
                }
            }
        }
    }
    if let Some((foot, high)) = top {
        climbs.extend(qualify(points, foot, high));
    }
    climbs
}

fn qualify(points: &[RoutePoint], foot: usize, top: usize) -> Option<Climb> {
    let length = points[top].distance.0 - points[foot].distance.0;
    let gain = points[top].elevation.0 - points[foot].elevation.0;
    if length < MIN_LENGTH_M {
        return None;
    }
    let grade = gain / length * 100.0;
    let score = length * grade;
    (grade >= MIN_GRADE && score >= MIN_SCORE).then(|| Climb {
        start: points[foot].distance,
        end: points[top].distance,
        gain: Meters(gain),
        average_grade: GradePercent(grade),
        category: ClimbCategory::from_score(score),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Surface;

    /// Points every 10 m following `profile`: (length m, grade %) sections.
    fn route(profile: &[(f64, f64)]) -> Vec<RoutePoint> {
        let mut points = vec![RoutePoint {
            lat: 46.0,
            lon: 7.0,
            elevation: Meters(500.0),
            distance: Meters(0.0),
            surface: Surface::Ground,
        }];
        for &(length, grade) in profile {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // small, positive
            let steps = (length / 10.0).round() as usize;
            for _ in 0..steps {
                let last = points[points.len() - 1];
                points.push(RoutePoint {
                    elevation: Meters(last.elevation.0 + grade / 10.0),
                    distance: Meters(last.distance.0 + 10.0),
                    ..last
                });
            }
        }
        points
    }

    #[test]
    fn finds_a_climb_between_flats() {
        let climbs = detect(&route(&[(1000.0, 0.0), (2000.0, 6.0), (1000.0, 0.0)]));

        assert_eq!(climbs.len(), 1);
        let climb = climbs[0];
        assert!((climb.start.0 - 1000.0).abs() < 1.0, "{climb:?}");
        assert!((climb.end.0 - 3000.0).abs() < 1.0, "{climb:?}");
        assert!((climb.gain.0 - 120.0).abs() < 1e-6);
        assert!((climb.average_grade.0 - 6.0).abs() < 1e-6);
        // 2000 m × 6 % = 12 000.
        assert_eq!(climb.category, ClimbCategory::Cat4);
    }

    #[test]
    fn a_short_dip_does_not_split_a_climb_but_a_descent_does() {
        let dip = detect(&route(&[(1000.0, 7.0), (50.0, -8.0), (1000.0, 7.0)]));
        let descent = detect(&route(&[(1000.0, 7.0), (1000.0, -5.0), (1000.0, 7.0)]));

        assert_eq!(dip.len(), 1, "{dip:?}");
        assert_eq!(descent.len(), 2, "{descent:?}");
    }

    #[test]
    fn ignores_gentle_and_short_rises() {
        assert_eq!(detect(&route(&[(3000.0, 2.0)])), []);
        assert_eq!(detect(&route(&[(200.0, 10.0)])), []);
    }

    #[test]
    fn categorises_by_length_times_gradient() {
        let alpe = detect(&route(&[(13_800.0, 8.1)]));
        let ramp = detect(&route(&[(600.0, 6.0)]));

        assert_eq!(alpe[0].category, ClimbCategory::Hc);
        assert_eq!(ramp[0].category, ClimbCategory::Uncategorized);
        assert_eq!(ClimbCategory::Cat2.label(), "Cat 2");
    }
}
