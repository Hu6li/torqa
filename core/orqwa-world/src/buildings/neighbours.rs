//! Roofs like the neighbours'. Few buildings have their roof mapped (6–8 % in Switzerland and
//! its neighbours, ADR 0041), but enough do for a town's or a village's roofs to show: around
//! Biel's centre most mapped roofs are flat, around Brienz none is. A building whose roof the
//! map leaves out takes one as the mapped roofs around it go, those of buildings its size
//! first; where too few are mapped nearby, its kind's own roof stays.

use std::collections::HashMap;

use orqwa_osm::RoofShape;

use super::shape::{centroid, signed_area};
use super::{Dice, Drawn, Plot, Point};

/// Mapped roofs are looked for within the first of these distances (m) that has enough of
/// buildings the same size...
const REACHES: [f64; 4] = [1000.0, 2000.0, 3000.0, 5000.0];
/// ...this many: a town builds its houses, blocks and sheds differently.
const ENOUGH_ALIKE: u32 = 8;
/// Short of that within the furthest, buildings of other sizes count too, less the more they
/// differ (`ALIKE`), if there are this many mapped roofs at all; else none is guessed.
const ENOUGH: u32 = 10;
/// Footprint areas (m²) dividing the sizes of building whose roofs are told apart: sheds and
/// garages, houses, large houses, blocks and halls.
const SIZES: [f64; 4] = [60.0, 200.0, 600.0, 1500.0];
/// How much a mapped roof counts for a building as many sizes apart.
const ALIKE: [u32; 3] = [4, 2, 1];
/// Cells of the index of mapped roofs, and of the mixes worked out once for all the
/// buildings in one.
const CELL: f64 = 250.0;

/// The mapped roofs in each cell: where, the size of building and the form.
type Mapped = HashMap<(i64, i64), Vec<(Point, usize, usize)>>;
/// A cell and a size of building.
type Key = ((i64, i64), usize);

/// Roofs told apart: flat, gabled (as gambrel and lean-to roofs are drawn) and hipped (as the
/// other pitched ones are).
const FORMS: [RoofShape; 3] = [RoofShape::Flat, RoofShape::Gabled, RoofShape::Hipped];

/// Gives each plot whose roof the map leaves out the roof of its neighbours (`Plot::roof`).
pub(crate) fn roofs_like_neighbours(plots: &mut [Plot]) {
    let mut mapped: Mapped = HashMap::new();
    for plot in plots.iter().filter(|p| drawn(p)) {
        if let Some(form) = plot.building.shape.and_then(|s| s.roof).map(form) {
            let at = centroid(&plot.footprint);
            let entry = (at, size(&plot.footprint), form);
            mapped.entry(cell(at)).or_default().push(entry);
        }
    }
    if mapped.is_empty() {
        return;
    }
    let mut mixes: HashMap<Key, Option<[u32; 3]>> = HashMap::new();
    for plot in plots.iter_mut() {
        if !drawn(plot) || plot.building.shape.and_then(|s| s.roof).is_some() {
            continue;
        }
        let at = centroid(&plot.footprint);
        let key = (cell(at), size(&plot.footprint));
        let mix = *mixes.entry(key).or_insert_with(|| mix(&mapped, key));
        plot.roof = mix.map(|counts| pick(counts, &Dice(plot.building.id)));
    }
}

/// Whether a plot is drawn as a building of its own, whose roof shows.
fn drawn(plot: &Plot) -> bool {
    matches!(plot.drawn, Drawn::Whole | Drawn::Part(_)) && !plot.church && plot.landmark.is_none()
}

/// The roofs mapped around the middle of a cell: those of buildings of `size` within the first
/// reach that has enough of them, else those of all sizes within the furthest, counted by how
/// alike their size is; none if too few are mapped there. Whole counts, so the mix comes out
/// the same whatever order the roofs are found in.
fn mix(mapped: &Mapped, ((x, y), size): Key) -> Option<[u32; 3]> {
    #[allow(clippy::cast_precision_loss)] // cell numbers of local metres
    let middle = ((x as f64 + 0.5) * CELL, (y as f64 + 0.5) * CELL);
    let mut by_size = [[0_u32; 3]; SIZES.len() + 1];
    for reach in REACHES {
        #[allow(clippy::cast_possible_truncation)] // a few cells
        let span = (reach / CELL).ceil() as i64 + 1;
        by_size = [[0; 3]; SIZES.len() + 1];
        for cx in x - span..=x + span {
            for cy in y - span..=y + span {
                for &(at, other, form) in mapped.get(&(cx, cy)).into_iter().flatten() {
                    if (at.0 - middle.0).hypot(at.1 - middle.1) <= reach {
                        by_size[other][form] += 1;
                    }
                }
            }
        }
        if by_size[size].iter().sum::<u32>() >= ENOUGH_ALIKE {
            return Some(by_size[size]);
        }
    }
    let found: u32 = by_size.iter().flatten().sum();
    if found < ENOUGH {
        return None;
    }
    let mut counts = [0_u32; 3];
    for (other, forms) in by_size.iter().enumerate() {
        if let Some(&weight) = ALIKE.get(other.abs_diff(size)) {
            for (count, n) in counts.iter_mut().zip(forms) {
                *count += weight * n;
            }
        }
    }
    (counts.iter().sum::<u32>() > 0).then_some(counts)
}

/// A roof picked from a mix, the same for the same building every time.
fn pick(counts: [u32; 3], dice: &Dice) -> RoofShape {
    let total: u32 = counts.iter().sum();
    let mut left = dice.roll(77) * f64::from(total);
    for (form, count) in FORMS.iter().zip(counts) {
        left -= f64::from(count);
        if left < 0.0 {
            return *form;
        }
    }
    FORMS[counts.iter().rposition(|&c| c > 0).unwrap_or(1)]
}

/// Which of `FORMS` a mapped roof counts as.
fn form(roof: RoofShape) -> usize {
    match roof {
        RoofShape::Flat => 0,
        RoofShape::Gabled | RoofShape::Gambrel | RoofShape::Skillion => 1,
        RoofShape::Hipped
        | RoofShape::HalfHipped
        | RoofShape::Mansard
        | RoofShape::Pyramidal
        | RoofShape::Round => 2,
    }
}

/// The size of building a footprint is, as an index into the sizes `SIZES` divide.
fn size(footprint: &[Point]) -> usize {
    let area = signed_area(footprint).abs();
    SIZES.iter().take_while(|&&s| area >= s).count()
}

#[allow(clippy::cast_possible_truncation)] // local metres stay far below 2^63 cells
fn cell((east, north): Point) -> (i64, i64) {
    ((east / CELL).floor() as i64, (north / CELL).floor() as i64)
}

#[cfg(test)]
mod tests {
    use orqwa_osm::{Building, Shape};

    use super::*;
    use crate::buildings::passage::Cut;
    use crate::buildings::{Region, Setting};
    use crate::climate::Climate;

    fn building(id: i64, roof: Option<RoofShape>) -> Building {
        Building {
            id,
            outline: Vec::new(),
            height: None,
            levels: None,
            color: None,
            min_height: None,
            in_parts: false,
            kind: None,
            shape: roof.map(|roof| Shape {
                roof: Some(roof),
                ..Shape::default()
            }),
        }
    }

    /// A square footprint of `side` metres at (east, north).
    fn plot(building: &Building, (east, north): Point, side: f64) -> Plot<'_> {
        let h = side / 2.0;
        Plot {
            building,
            footprint: vec![
                (east - h, north - h),
                (east + h, north - h),
                (east + h, north + h),
                (east - h, north + h),
            ],
            setting: Setting::Town,
            church: false,
            shop: None,
            purpose: None,
            landmark: None,
            climate: Climate::Temperate,
            region: Region::Generic,
            drawn: Drawn::Whole,
            cut: Cut::Whole,
            roof: None,
        }
    }

    /// The roofs guessed for `count` unmapped houses of `side` metres among `mapped`
    /// buildings (roof, side) laid out on a grid around them.
    #[allow(clippy::cast_possible_wrap, clippy::cast_precision_loss)] // a few test buildings
    fn guesses(mapped: &[(RoofShape, f64)], count: i64, side: f64) -> Vec<Option<RoofShape>> {
        let known: Vec<Building> = (0..mapped.len())
            .map(|i| building(1000 + i as i64, Some(mapped[i].0)))
            .collect();
        let unknown: Vec<Building> = (0..count).map(|id| building(id, None)).collect();
        let mut plots: Vec<Plot> = known
            .iter()
            .zip(mapped)
            .enumerate()
            .map(|(i, (b, &(_, side)))| {
                let at = ((i % 10) as f64 * 40.0, (i / 10) as f64 * 40.0);
                plot(b, at, side)
            })
            .chain(unknown.iter().map(|b| plot(b, (180.0, 600.0), side)))
            .collect();
        roofs_like_neighbours(&mut plots);
        plots[mapped.len()..].iter().map(|p| p.roof).collect()
    }

    #[test]
    fn buildings_take_the_roofs_mapped_around_them() {
        let village = vec![(RoofShape::Gabled, 12.0); 25];
        assert!(
            guesses(&village, 50, 12.0)
                .iter()
                .all(|r| *r == Some(RoofShape::Gabled)),
            "a house in a village of gabled roofs got another"
        );
        let centre = vec![(RoofShape::Flat, 12.0); 25];
        assert!(
            guesses(&centre, 50, 12.0)
                .iter()
                .all(|r| *r == Some(RoofShape::Flat))
        );
        // A mix comes out mixed, as often as mapped.
        let mixed: Vec<_> = [(RoofShape::Flat, 12.0), (RoofShape::Hipped, 12.0)]
            .iter()
            .cycle()
            .take(40)
            .copied()
            .collect();
        let picked = guesses(&mixed, 400, 12.0);
        let flat = picked
            .iter()
            .filter(|r| **r == Some(RoofShape::Flat))
            .count();
        assert!((150..=250).contains(&flat), "{flat} of 400 flat");
    }

    #[test]
    fn buildings_of_their_own_size_count_most() {
        // Large flat-roofed blocks among small gabled houses: a house takes a house's roof
        // more often, a block a block's.
        let town: Vec<_> = (0..40)
            .map(|i| {
                if i % 2 == 0 {
                    (RoofShape::Flat, 40.0)
                } else {
                    (RoofShape::Gabled, 10.0)
                }
            })
            .collect();
        let share = |side| {
            let picked = guesses(&town, 300, side);
            picked
                .iter()
                .filter(|r| **r == Some(RoofShape::Flat))
                .count()
        };
        assert!(
            share(10.0) < 120,
            "houses got flat roofs {} of 300 times",
            share(10.0)
        );
        assert!(
            share(40.0) > 180,
            "blocks got flat roofs {} of 300 times",
            share(40.0)
        );
    }

    #[test]
    fn too_few_mapped_roofs_leave_the_kinds_own() {
        let few = vec![(RoofShape::Flat, 12.0); 6];
        assert!(guesses(&few, 20, 12.0).iter().all(Option::is_none));
    }

    #[test]
    fn guesses_are_the_same_every_time() {
        let mixed: Vec<_> = [(RoofShape::Flat, 12.0), (RoofShape::Gabled, 12.0)]
            .iter()
            .cycle()
            .take(30)
            .copied()
            .collect();
        assert_eq!(guesses(&mixed, 50, 12.0), guesses(&mixed, 50, 12.0));
    }
}
