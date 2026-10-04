//! Buildings from OpenStreetMap footprints (R45). The map rarely says more than the outline
//! and sometimes a height, so what a building is gets guessed from where it stands and its
//! size: churches from places of worship, halls on industrial land, chalets in the mountains,
//! farmhouses as large buildings in the countryside, blocks from their height or size in
//! towns, sheds from their size, and houses otherwise. Each kind gets its own proportions,
//! roof, materials and details.

mod parts;
mod shape;

use torqa_osm::Building;

use crate::{HeightGrid, MeshData, hash};
use parts::{Builder, Paint, Pitch, RoofPaint, Style};
pub(crate) use shape::{Point, centroid, contains, footprint, signed_area, triangulate};
use shape::{Rect, distance};

/// Height of a storey; rows of windows repeat at it.
const STOREY: f64 = 3.0;
/// Walls of whole storeys end this far above the last one, clear of its windows.
const EAVES_MARGIN: f64 = 0.4;
/// Walls reach this far below the lowest ground point, so slopes never show a gap.
const FOUNDATION: f64 = 1.0;
/// Footprints filling this much of the rectangle around them are built as that rectangle,
/// which can carry gable and hipped roofs; the difference does not show from the road.
const RECTANGULAR: f64 = 0.8;
/// Churches keep their nave shape even with a choir or porch.
const RECTANGULAR_CHURCH: f64 = 0.7;
/// Above this elevation chalets start to replace houses; above the second, all are chalets.
const CHALETS_FROM: f64 = 700.0;
const CHALETS_ONLY: f64 = 1100.0;
/// A church point mapped on church grounds rather than the church marks the largest
/// building this close.
const CHURCH_REACH: f64 = 30.0;

/// Plaster (sRGB): cream, beige, sand, white, light grey, ochre, pale yellow, salmon, sage.
const PLASTER: [[f32; 3]; 9] = [
    [0.93, 0.89, 0.80],
    [0.88, 0.83, 0.74],
    [0.84, 0.76, 0.63],
    [0.94, 0.93, 0.90],
    [0.78, 0.78, 0.76],
    [0.86, 0.72, 0.55],
    [0.93, 0.87, 0.66],
    [0.90, 0.74, 0.66],
    [0.80, 0.84, 0.78],
];
/// Plaster of blocks, churches and masonry ground floors: white, light grey, off-white,
/// pale beige.
const LIGHT_PLASTER: [[f32; 3]; 4] = [
    [0.95, 0.95, 0.93],
    [0.84, 0.84, 0.83],
    [0.92, 0.90, 0.85],
    [0.88, 0.85, 0.78],
];
/// Wood: dark brown, brown, weathered grey-brown, honey.
const WOOD: [[f32; 3]; 4] = [
    [0.36, 0.23, 0.13],
    [0.47, 0.31, 0.18],
    [0.42, 0.36, 0.30],
    [0.62, 0.44, 0.25],
];
/// Roof tiles: terracotta, brown, red-brown, anthracite, slate.
const TILES: [[f32; 3]; 5] = [
    [0.60, 0.29, 0.20],
    [0.44, 0.27, 0.20],
    [0.52, 0.24, 0.18],
    [0.24, 0.25, 0.27],
    [0.36, 0.37, 0.40],
];
/// Mountain roofs: dark grey, slate, brown shingles.
const MOUNTAIN_ROOFS: [[f32; 3]; 3] = [[0.22, 0.22, 0.23], [0.34, 0.35, 0.37], [0.36, 0.28, 0.22]];
/// Metal cladding: light grey, silver, white, blue-grey, beige, dark grey.
const CLADDING: [[f32; 3]; 6] = [
    [0.70, 0.71, 0.72],
    [0.80, 0.81, 0.82],
    [0.90, 0.90, 0.89],
    [0.45, 0.52, 0.60],
    [0.78, 0.74, 0.66],
    [0.38, 0.39, 0.40],
];
/// Metal roofs: grey, dark grey, light grey.
const SHEET: [[f32; 3]; 3] = [[0.50, 0.51, 0.52], [0.30, 0.31, 0.32], [0.66, 0.67, 0.68]];
/// Flat roofs: gravel, membrane, light gravel.
const FLAT: [[f32; 3]; 3] = [[0.45, 0.44, 0.42], [0.35, 0.35, 0.36], [0.55, 0.54, 0.50]];
/// Spires: slate, copper green, red tiles.
const SPIRES: [[f32; 3]; 3] = [[0.28, 0.29, 0.32], [0.36, 0.56, 0.48], [0.55, 0.26, 0.19]];
/// Window frames: white; stone around church openings; chimney caps.
const WHITE: [f32; 3] = [0.93, 0.93, 0.90];
const STONE: [f32; 3] = [0.72, 0.71, 0.68];
const SOOT: [f32; 3] = [0.20, 0.20, 0.21];
const BRICK: [f32; 3] = [0.55, 0.27, 0.20];

/// What a building is, as far as the map lets us tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A family house: plastered walls, a pitched roof.
    House,
    /// A mountain house: timber over a masonry ground floor, a shallow roof with deep eaves.
    Chalet,
    /// A large rural building under a big, steep roof reaching low.
    Farmhouse,
    /// Shed, garage or barn too small to live in: no windows.
    Shed,
    /// Apartments or offices: several storeys, mostly flat roofs.
    Block,
    /// Factory, warehouse or store: low, wide, clad in metal.
    Hall,
    /// A church with a tower, or a chapel with a turret on its roof.
    Church,
}

/// Where a building stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Setting {
    /// In a residential area.
    Town,
    /// On industrial, commercial or retail land.
    Industrial,
    /// Anywhere else.
    Countryside,
}

/// A building with what is known about it, ready to be built.
pub(crate) struct Plot<'a> {
    pub(crate) building: &'a Building,
    /// Footprint in metres east/north, counter-clockwise.
    pub(crate) footprint: Vec<Point>,
    pub(crate) setting: Setting,
    /// A church point lies in or by it.
    pub(crate) church: bool,
}

/// Marks the plots that are churches: the one each church point lies in, or for points
/// mapped on church grounds, the largest building near it.
pub(crate) fn mark_churches(plots: &mut [Plot], churches: &[Point]) {
    for &point in churches {
        let inside = plots.iter().position(|p| contains(&p.footprint, point));
        let nearby = || {
            plots
                .iter()
                .enumerate()
                .filter(|(_, p)| {
                    p.footprint
                        .iter()
                        .any(|&corner| distance(corner, point) < CHURCH_REACH)
                })
                .max_by(|(_, a), (_, b)| {
                    signed_area(&a.footprint).total_cmp(&signed_area(&b.footprint))
                })
                .map(|(i, _)| i)
        };
        if let Some(i) = inside.or_else(nearby) {
            plots[i].church = true;
        }
    }
}

/// Adds a building standing on `heights` to a chunk mesh centred at `origin`.
pub(crate) fn add(mesh: &mut MeshData, plot: &Plot, heights: &HeightGrid, origin: [f64; 3]) {
    let footprint = &plot.footprint;
    if footprint.len() < 3 {
        return;
    }
    // Floors are level: the ground floor is at the highest ground, and on slopes the walls
    // go down to the lowest like a basement.
    let grounds: Vec<f64> = footprint.iter().map(|&(e, n)| heights.at(e, n)).collect();
    let ground = grounds.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let footing = grounds.iter().copied().fold(f64::INFINITY, f64::min) - FOUNDATION;
    let area = signed_area(footprint);
    let dice = Dice(plot.building.id);
    let kind = kind(plot, area, ground, &dice);
    let fill = if kind == Kind::Church {
        RECTANGULAR_CHURCH
    } else {
        RECTANGULAR
    };
    let rect = Rect::around(footprint).filter(|r| area / r.area() >= fill);
    let mut builder = Builder {
        mesh,
        origin,
        ground,
        footing,
    };
    if kind == Kind::Church {
        church(&mut builder, plot, area, rect, &dice);
    } else {
        let design = design(kind, plot.building, rect.is_some(), &dice);
        build(&mut builder, footprint, rect, &design, plot.building, &dice);
    }
}

/// What a building is, from its surroundings, size, mapped height and elevation.
fn kind(plot: &Plot, area: f64, ground: f64, dice: &Dice) -> Kind {
    if plot.church {
        return Kind::Church;
    }
    let height = mapped_height(plot.building);
    if area < 30.0 && height.is_none_or(|h| h < 5.0) {
        return Kind::Shed;
    }
    if height.is_some_and(|h| h >= 12.0) {
        return Kind::Block;
    }
    let alpine = dice.roll(0) < smoothstep(CHALETS_FROM, CHALETS_ONLY, ground);
    match plot.setting {
        Setting::Industrial if area < 80.0 => Kind::Shed,
        Setting::Industrial => Kind::Hall,
        Setting::Town if area > 400.0 => Kind::Block,
        Setting::Countryside if area > 1500.0 => Kind::Hall,
        _ if alpine => Kind::Chalet,
        Setting::Countryside if area > 220.0 => Kind::Farmhouse,
        _ => Kind::House,
    }
}

/// Mapped heights reach the top of the roof.
fn mapped_height(building: &Building) -> Option<f64> {
    building.height.or_else(|| building.levels.map(storeys))
}

/// Wall height of `count` storeys.
fn storeys(count: f64) -> f64 {
    count.max(1.0) * STOREY + EAVES_MARGIN
}

/// The wall height of whole storeys nearest to `height`, so windows never meet the eaves.
fn whole_storeys(height: f64) -> f64 {
    storeys(((height - EAVES_MARGIN) / STOREY).round())
}

fn smoothstep(from: f64, to: f64, x: f64) -> f64 {
    let t = ((x - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Pseudo-random but stable choices for one building.
struct Dice(i64);

impl Dice {
    /// A value in [0, 1); each `salt` gives an independent one.
    fn roll(&self, salt: i64) -> f64 {
        hash(self.0 ^ salt.wrapping_mul(0x5851_F42D_4C95_7F2D))
    }

    fn pick<T: Copy>(&self, salt: i64, options: &[T]) -> T {
        // `roll` lies in [0, 1), so the index is a small non-negative number.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        #[allow(clippy::cast_precision_loss)]
        let index = (self.roll(salt) * options.len() as f64) as usize;
        options[index.min(options.len() - 1)]
    }

    /// A colour varied a little, so neighbours painted alike still differ.
    fn tint(&self, rgb: [f32; 3]) -> [f32; 3] {
        #[allow(clippy::cast_possible_truncation)] // a factor near 1
        let scale = (0.94 + 0.12 * self.roll(99)) as f32;
        rgb.map(|c| (c * scale).min(1.0))
    }
}

/// Roof shapes.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Roof {
    Gable,
    Hipped,
    Flat { parapet: f64 },
}

/// How a building is built.
struct Design {
    kind: Kind,
    /// Wall height when the map gives none.
    walls: f64,
    /// Whether the walls have rows of windows, which then need whole storeys.
    windows: bool,
    /// Roof over rectangular footprints; others get a hipped band or a flat roof.
    roof: Roof,
    pitch_degrees: f64,
    /// Wide buildings get flatter roofs so they rise no more than this.
    max_rise: f64,
    overhang: f64,
    verge: f64,
    wall: Paint,
    /// A masonry ground floor below timber walls.
    base: Option<Paint>,
    roof_paint: RoofPaint,
    chimney: bool,
    /// Frame colour of windows in the gables.
    gable_windows: Option<[f32; 3]>,
    balcony: bool,
}

fn design(kind: Kind, building: &Building, rectangular: bool, dice: &Dice) -> Design {
    let recipe = Recipe {
        kind,
        building,
        rectangular,
        dice,
    };
    match kind {
        Kind::House => recipe.house(),
        Kind::Chalet => recipe.chalet(),
        Kind::Farmhouse => recipe.farmhouse(),
        Kind::Shed => recipe.shed(),
        Kind::Block => recipe.block(),
        // `church` builds churches; none get here.
        Kind::Hall | Kind::Church => recipe.hall(),
    }
}

/// What goes into one building's design.
struct Recipe<'a> {
    kind: Kind,
    building: &'a Building,
    rectangular: bool,
    dice: &'a Dice,
}

impl Recipe<'_> {
    fn paint(&self, palette: &[[f32; 3]], salt: i64, style: Style) -> Paint {
        Paint::new(self.dice.tint(self.dice.pick(salt, palette)), style)
    }

    /// The main walls' paint: the mapped colour if there is one.
    fn facade(&self, palette: &[[f32; 3]], style: Style) -> Paint {
        let rgb = self
            .building
            .color
            .unwrap_or_else(|| self.dice.pick(1, palette));
        Paint::new(self.dice.tint(rgb), style)
    }

    /// Overhang varies with each building, at the eaves and gables alike.
    fn overhang(&self, least: f64, spread: f64) -> f64 {
        least + spread * self.dice.roll(5)
    }

    fn house(&self) -> Design {
        let wall = self.facade(&PLASTER, Style::Plaster);
        let wood = self.paint(&WOOD, 2, Style::Boards);
        Design {
            kind: self.kind,
            walls: storeys(match self.dice.roll(3) {
                r if r < 0.25 => 1.0,
                r if r < 0.85 => 2.0,
                _ => 3.0,
            }),
            windows: true,
            roof: if self.dice.roll(4) < 0.75 {
                Roof::Gable
            } else {
                Roof::Hipped
            },
            pitch_degrees: 35.0 + 10.0 * self.dice.roll(6),
            max_rise: 6.0,
            overhang: self.overhang(0.5, 0.2),
            verge: self.overhang(0.4, 0.2),
            wall,
            base: None,
            roof_paint: RoofPaint {
                top: if self.dice.roll(7) < 0.85 {
                    self.paint(&TILES, 8, Style::Tiles)
                } else {
                    self.paint(&SHEET, 8, Style::Sheet)
                },
                under: wood,
                gables: if self.dice.roll(9) < 0.3 {
                    wood
                } else {
                    wall.with(Style::Blank)
                },
            },
            chimney: self.dice.roll(10) < 0.6,
            gable_windows: Some(WHITE),
            balcony: false,
        }
    }

    fn chalet(&self) -> Design {
        let wood = self.facade(&WOOD, Style::Timber);
        Design {
            kind: self.kind,
            walls: storeys(if self.dice.roll(3) < 0.65 { 2.0 } else { 3.0 }),
            windows: true,
            roof: Roof::Gable,
            pitch_degrees: 20.0 + 6.0 * self.dice.roll(6),
            max_rise: 5.0,
            overhang: self.overhang(1.2, 0.4),
            verge: self.overhang(1.4, 0.4),
            wall: wood,
            base: Some(self.paint(&LIGHT_PLASTER, 2, Style::Plaster)),
            roof_paint: RoofPaint {
                top: if self.dice.roll(7) < 0.7 {
                    self.paint(&MOUNTAIN_ROOFS, 8, Style::Tiles)
                } else {
                    self.paint(&SHEET, 8, Style::Sheet)
                },
                under: wood.with(Style::Boards),
                gables: wood.with(Style::Boards),
            },
            chimney: self.dice.roll(10) < 0.5,
            gable_windows: Some(WHITE),
            balcony: true,
        }
    }

    fn farmhouse(&self) -> Design {
        let wood = self.facade(&WOOD, Style::Timber);
        Design {
            kind: self.kind,
            walls: storeys(if self.dice.roll(3) < 0.6 { 1.0 } else { 2.0 }),
            windows: true,
            roof: if self.dice.roll(4) < 0.6 {
                Roof::Hipped
            } else {
                Roof::Gable
            },
            pitch_degrees: 40.0 + 8.0 * self.dice.roll(6),
            max_rise: 9.0,
            overhang: self.overhang(0.9, 0.3),
            verge: self.overhang(0.9, 0.3),
            wall: wood,
            base: Some(self.paint(&LIGHT_PLASTER, 2, Style::Plaster)),
            roof_paint: RoofPaint {
                top: self.paint(&TILES[..3], 8, Style::Tiles),
                under: wood.with(Style::Boards),
                gables: wood.with(Style::Boards),
            },
            chimney: self.dice.roll(10) < 0.4,
            gable_windows: Some(WHITE),
            balcony: false,
        }
    }

    fn shed(&self) -> Design {
        let wall = if self.dice.roll(2) < 0.6 {
            self.facade(&WOOD, Style::Boards)
        } else {
            self.facade(&PLASTER, Style::Blank)
        };
        Design {
            kind: self.kind,
            walls: 2.6,
            windows: false,
            roof: if self.rectangular {
                Roof::Gable
            } else {
                Roof::Flat { parapet: 0.0 }
            },
            pitch_degrees: 18.0 + 10.0 * self.dice.roll(6),
            max_rise: 2.5,
            overhang: 0.3,
            verge: 0.3,
            wall,
            base: None,
            roof_paint: RoofPaint {
                top: if self.dice.roll(7) < 0.5 {
                    self.paint(&SHEET, 8, Style::Sheet)
                } else {
                    self.paint(&TILES, 8, Style::Tiles)
                },
                under: wall,
                gables: wall,
            },
            chimney: false,
            gable_windows: None,
            balcony: false,
        }
    }

    fn block(&self) -> Design {
        let wall = self.facade(&LIGHT_PLASTER, Style::Plaster);
        let walls = storeys(4.0 + (self.dice.roll(3) * 3.0).floor());
        // Tall blocks are modern and flat-roofed; lower ones are as often hipped.
        let tall = mapped_height(self.building).unwrap_or(walls) >= 15.0;
        let flat = tall || !self.rectangular || self.dice.roll(4) < 0.6;
        Design {
            kind: self.kind,
            walls,
            windows: true,
            roof: if flat {
                Roof::Flat { parapet: 0.8 }
            } else {
                Roof::Hipped
            },
            pitch_degrees: 22.0 + 6.0 * self.dice.roll(6),
            max_rise: 4.0,
            overhang: 0.6,
            verge: 0.6,
            wall,
            base: None,
            roof_paint: RoofPaint {
                top: if flat {
                    self.paint(&FLAT, 8, Style::Flat)
                } else {
                    self.paint(&TILES, 8, Style::Tiles)
                },
                under: wall.with(Style::Blank),
                gables: wall.with(Style::Blank),
            },
            chimney: false,
            gable_windows: None,
            balcony: false,
        }
    }

    fn hall(&self) -> Design {
        let wall = self.facade(&CLADDING, Style::Cladding);
        Design {
            kind: self.kind,
            walls: 6.0 + 3.0 * self.dice.roll(3),
            windows: false,
            roof: if self.rectangular && self.dice.roll(4) < 0.45 {
                Roof::Gable
            } else {
                Roof::Flat { parapet: 0.5 }
            },
            pitch_degrees: 8.0 + 6.0 * self.dice.roll(6),
            max_rise: 3.0,
            overhang: 0.4,
            verge: 0.4,
            wall,
            base: None,
            roof_paint: RoofPaint {
                top: self.paint(&SHEET, 8, Style::Sheet),
                under: wall.with(Style::Blank),
                gables: wall,
            },
            chimney: false,
            gable_windows: None,
            balcony: false,
        }
    }
}

/// Walls, roof and details of every kind but churches.
fn build(
    b: &mut Builder,
    footprint: &[Point],
    rect: Option<Rect>,
    design: &Design,
    building: &Building,
    dice: &Dice,
) {
    let outline = rect.map_or_else(|| footprint.to_vec(), |r| r.corners());
    let parapet = match design.roof {
        Roof::Flat { parapet } => Some(parapet),
        Roof::Gable | Roof::Hipped => None,
    };
    let mut angle = design.pitch_degrees.to_radians();
    let rise = match (parapet, rect) {
        (Some(_), _) => 0.0,
        (None, Some(rect)) => {
            angle = angle.min((design.max_rise / rect.half_width).atan());
            rect.half_width * angle.tan()
        }
        (None, None) => 2.0,
    };
    let walls = wall_height(building, rise, design);
    let ground = b.ground;
    let eaves = ground + walls;
    let wall_top = eaves + parapet.unwrap_or(0.0);
    let bottom = b.footing;
    match design.base {
        Some(base) if ground + STOREY < wall_top => {
            b.walls(&outline, bottom, ground + STOREY, base);
            b.walls(&outline, ground + STOREY, wall_top, design.wall);
        }
        Some(base) => b.walls(&outline, bottom, wall_top, base),
        None => b.walls(&outline, bottom, wall_top, design.wall),
    }

    let pitch = Pitch {
        eaves,
        angle,
        overhang: design.overhang,
        verge: design.verge,
    };
    let paint = design.roof_paint;
    let top = match (design.roof, rect) {
        (Roof::Flat { parapet }, _) => {
            b.flat_roof(&outline, eaves, parapet, paint.top, paint.gables);
            eaves
        }
        (Roof::Gable, Some(rect)) => b.gable_roof(&rect, pitch, paint),
        (Roof::Hipped, Some(rect)) => b.hipped_roof(&rect, pitch, paint),
        (_, None) => b.hip_band_roof(&outline, pitch, paint).unwrap_or_else(|| {
            b.flat_roof(
                &outline,
                eaves,
                0.0,
                paint.top.with(Style::Flat),
                paint.gables,
            );
            eaves
        }),
    };

    let Some(rect) = rect else {
        return;
    };
    if design.chimney && parapet.is_none() {
        chimney(b, &rect, design, pitch, top, dice);
    }
    if design.roof == Roof::Gable {
        if let Some(frame) = design.gable_windows {
            gable_windows(b, &rect, pitch, frame);
        }
        if design.balcony {
            balcony(b, &rect, walls, design.wall.rgb, dice);
        }
    }
    if design.kind == Kind::Block && parapet.is_some() && dice.roll(11) < 0.6 {
        // Stairs and lift machinery on the roof.
        let housing = Rect {
            centre: rect.point((dice.roll(12) - 0.5) * rect.half_length, 0.0),
            half_length: (rect.half_length * 0.3).min(2.0),
            half_width: (rect.half_width * 0.4).min(1.5),
            ..rect
        };
        b.cuboid(
            &housing,
            (eaves, eaves + 2.6),
            design.wall.with(Style::Blank),
            Paint::new(FLAT[1], Style::Flat),
            false,
        );
    }
}

/// Height of the walls above the ground, from the map if it knows (counting half of a
/// roof rising `rise` into a mapped height), else the design's.
fn wall_height(building: &Building, rise: f64, design: &Design) -> f64 {
    let walls = match (building.levels, building.height) {
        (Some(levels), _) => storeys(levels),
        (None, Some(height)) => height - rise / 2.0,
        (None, None) => design.walls,
    };
    if design.windows {
        whole_storeys(walls)
    } else {
        walls.max(2.2)
    }
}

fn chimney(b: &mut Builder, rect: &Rect, design: &Design, pitch: Pitch, top: f64, dice: &Dice) {
    let half = 0.3;
    let along = (dice.roll(20) - 0.5) * rect.half_length;
    let across = if dice.roll(21) < 0.5 { -0.4 } else { 0.4 } * rect.half_width;
    // How far in from the eaves the chimney stands, which sets the roof's height there.
    let inset = match design.roof {
        Roof::Hipped => (rect.half_width - across.abs()).min(rect.half_length - along.abs()),
        Roof::Gable | Roof::Flat { .. } => rect.half_width - across.abs(),
    };
    let slope = pitch.angle.tan();
    let roof = pitch.eaves + (inset - half) * slope;
    let stack = Rect::square(rect.point(along, across), rect.axis, half);
    let sides = if dice.roll(22) < 0.3 {
        Paint::new(BRICK, Style::Blank)
    } else {
        design.wall.with(Style::Blank)
    };
    b.cuboid(
        &stack,
        (roof - 0.1, top + 0.6),
        sides,
        Paint::new(SOOT, Style::Flat),
        false,
    );
}

/// One window, or two in wide gables, where they fit under the roof.
fn gable_windows(b: &mut Builder, rect: &Rect, pitch: Pitch, frame: [f32; 3]) {
    let (sill, lintel) = (0.5, 1.6);
    let rise = rect.half_width * pitch.angle.tan();
    if rise < lintel + 0.4 {
        return;
    }
    // Half the gable's width at the windows' top.
    let room = rect.half_width * (1.0 - lintel / rise);
    let positions: &[f64] = if room >= 2.0 {
        &[-1.2, 1.2]
    } else if room >= 0.75 {
        &[0.0]
    } else {
        &[]
    };
    for end in [-1.0, 1.0] {
        let out = (rect.axis.0 * end, rect.axis.1 * end);
        for &across in positions {
            b.window(
                rect.point(end * rect.half_length, across),
                out,
                0.45,
                (pitch.eaves + sill, pitch.eaves + lintel),
                frame,
            );
        }
    }
}

/// A wooden balcony along one gable, at the floor of the top storey.
fn balcony(b: &mut Builder, rect: &Rect, walls: f64, wood: [f32; 3], dice: &Dice) {
    let span = rect.half_width - 0.5;
    if span < 2.0 {
        return;
    }
    let top_storey = ((walls - EAVES_MARGIN) / STOREY).round() - 1.0;
    if top_storey < 1.0 {
        return;
    }
    let floor = b.ground + top_storey * STOREY;
    let end = if dice.roll(30) < 0.5 { -1.0 } else { 1.0 };
    let depth = 1.2;
    let slab = Rect {
        centre: rect.point(end * (rect.half_length + depth / 2.0), 0.0),
        axis: rect.across(),
        half_length: span,
        half_width: depth / 2.0,
    };
    let railing = Rect {
        centre: rect.point(end * (rect.half_length + depth - 0.04), 0.0),
        half_width: 0.04,
        ..slab
    };
    let boards = Paint::new(wood, Style::Boards);
    b.cuboid(&slab, (floor - 0.2, floor), boards, boards, true);
    b.cuboid(&railing, (floor, floor + 1.0), boards, boards, false);
}

/// A church: a high nave under a steep roof, and a tower with a spire at one end, or a
/// turret on the roof of a chapel.
fn church(b: &mut Builder, plot: &Plot, area: f64, rect: Option<Rect>, dice: &Dice) {
    let building = plot.building;
    let chapel = area < 150.0;
    let wall = Paint::new(
        dice.tint(
            building
                .color
                .unwrap_or_else(|| dice.pick(1, &LIGHT_PLASTER)),
        ),
        Style::Church,
    );
    let roof = Paint::new(
        dice.pick(2, &[TILES[3], TILES[4], TILES[2], TILES[1]]),
        Style::Tiles,
    );
    let blank = wall.with(Style::Blank);
    let paint = RoofPaint {
        top: roof,
        under: blank,
        gables: blank,
    };
    let outline = rect.map_or_else(|| plot.footprint.clone(), |r| r.corners());
    let mut angle = (48.0 + 7.0 * dice.roll(3)).to_radians();
    let rise = match rect {
        Some(rect) => {
            angle = angle.min((12.0 / rect.half_width).atan());
            rect.half_width * angle.tan()
        }
        None => 2.0,
    };
    // Mapped heights of churches are mostly the tower's; only low ones can be the nave's.
    let walls = match building.height {
        Some(height) if height < 20.0 => height - rise / 2.0,
        _ => 0.45 * area.sqrt(),
    }
    .clamp(if chapel { 6.5 } else { 7.5 }, 14.0);
    let ground = b.ground;
    let eaves = ground + walls;
    b.walls(&outline, b.footing, eaves, wall);
    let pitch = Pitch {
        eaves,
        angle,
        overhang: 0.4,
        verge: 0.3,
    };
    let ridge = match rect {
        Some(rect) => b.gable_roof(&rect, pitch, paint),
        None => b.hip_band_roof(&outline, pitch, paint).unwrap_or_else(|| {
            b.flat_roof(&outline, eaves, 0.0, roof.with(Style::Flat), blank);
            eaves
        }),
    };

    // Towers stand at one end of the nave, also when the outline is irregular.
    let Some(nave) = rect.or_else(|| Rect::around(&plot.footprint)) else {
        return;
    };
    let steeple = Steeple {
        nave,
        end: if dice.roll(4) < 0.5 { -1.0 } else { 1.0 },
        ridge,
        wall,
        paint: RoofPaint {
            top: Paint::new(dice.pick(5, &SPIRES), Style::Tiles),
            under: blank,
            gables: blank,
        },
    };
    if chapel {
        steeple.turret(b, angle, roof);
    } else {
        steeple.tower(b, building.height, dice);
    }
}

/// Where a church's tower or turret goes and how it looks.
struct Steeple {
    nave: Rect,
    /// Which end of the nave: −1 or 1 along its axis.
    end: f64,
    /// Height of the nave's ridge.
    ridge: f64,
    wall: Paint,
    paint: RoofPaint,
}

impl Steeple {
    /// A small turret with a needle on the ridge of a chapel whose roof slopes at `angle`.
    fn turret(&self, b: &mut Builder, angle: f64, roof: Paint) {
        let half = 0.7;
        let turret = Rect::square(
            self.nave
                .point(self.end * (self.nave.half_length - 1.6).max(0.0), 0.0),
            self.nave.axis,
            half,
        );
        let base = self.ridge - (half + 0.3) * angle.tan();
        let top = self.ridge + 1.8;
        b.cuboid(&turret, (base, top), self.paint.under, roof, false);
        let needle = Pitch {
            eaves: top,
            angle: (2.6 / half).atan(),
            overhang: 0.05,
            verge: 0.0,
        };
        b.hipped_roof(&turret, needle, self.paint);
    }

    /// A bell tower at the end of the nave, or beside it there, under a needle spire, a
    /// saddle roof or a pyramid; as high as mapped if the map knows.
    fn tower(&self, b: &mut Builder, mapped: Option<f64>, dice: &Dice) {
        let nave = &self.nave;
        let half = (nave.half_width * 0.55).clamp(2.25, 4.0);
        let side = dice.roll(6);
        let across = if side < 0.7 {
            0.0
        } else {
            (if side < 0.85 { -1.0 } else { 1.0 }) * (nave.half_width - half).max(0.0)
        };
        let tower = Rect::square(
            nave.point(self.end * (nave.half_length - half).max(0.0), across),
            nave.axis,
            half,
        );
        let style = dice.roll(7);
        let (spire_angle, spire_rise) = if style < 0.6 {
            // A needle.
            let rise = half * (3.6 + 1.6 * dice.roll(8));
            ((rise / half).atan(), rise)
        } else if style < 0.85 {
            // A saddle roof across the nave.
            (60f64.to_radians(), half * 60f64.to_radians().tan())
        } else {
            // A pyramid.
            (55f64.to_radians(), half * 55f64.to_radians().tan())
        };
        let top = match mapped {
            Some(height) if height >= 20.0 => b.ground + height - spire_rise,
            _ => self.ridge + 2.0 * half + 5.0 * dice.roll(9),
        }
        .max(self.ridge + 3.0);
        b.walls(&tower.corners(), b.footing, top, self.wall);
        belfry(b, &tower, top);
        let pitch = Pitch {
            eaves: top,
            angle: spire_angle,
            overhang: if style < 0.6 { 0.05 } else { 0.3 },
            verge: 0.3,
        };
        if (0.6..0.85).contains(&style) {
            b.gable_roof(&tower.turned(), pitch, self.paint);
        } else {
            b.hipped_roof(&tower, pitch, self.paint);
        }
    }
}

/// Sound openings on every side of a bell tower, below its roof.
fn belfry(b: &mut Builder, tower: &Rect, top: f64) {
    let half = tower.half_length;
    let positions: &[f64] = if half >= 2.6 { &[-0.8, 0.8] } else { &[0.0] };
    let across = tower.across();
    for out in [
        tower.axis,
        across,
        (-tower.axis.0, -tower.axis.1),
        (-across.0, -across.1),
    ] {
        let along = (-out.1, out.0);
        for &offset in positions {
            let centre = (
                tower.centre.0 + out.0 * half + along.0 * offset,
                tower.centre.1 + out.1 * half + along.1 * offset,
            );
            b.window(centre, out, 0.32, (top - 3.4, top - 1.0), STONE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plot(building: &Building, setting: Setting, church: bool) -> Plot<'_> {
        Plot {
            building,
            footprint: Vec::new(),
            setting,
            church,
        }
    }

    fn untagged(id: i64) -> Building {
        Building {
            id,
            outline: Vec::new(),
            height: None,
            levels: None,
            color: None,
        }
    }

    /// Kinds of 200 buildings with different ids.
    fn kinds(setting: Setting, area: f64, ground: f64) -> Vec<Kind> {
        (0..200)
            .map(|id| {
                let building = untagged(id);
                kind(&plot(&building, setting, false), area, ground, &Dice(id))
            })
            .collect()
    }

    #[test]
    fn mountain_houses_are_chalets() {
        assert!(
            kinds(Setting::Countryside, 120.0, 1300.0)
                .iter()
                .all(|&k| k == Kind::Chalet)
        );
        assert!(
            kinds(Setting::Countryside, 120.0, 400.0)
                .iter()
                .all(|&k| k == Kind::House)
        );
        // In between, both.
        let mixed = kinds(Setting::Countryside, 120.0, 900.0);
        assert!(mixed.contains(&Kind::Chalet) && mixed.contains(&Kind::House));
    }

    #[test]
    fn size_and_surroundings_set_the_kind() {
        let all = |kinds: Vec<Kind>, expected: Kind| kinds.iter().all(|&k| k == expected);

        assert!(all(
            kinds(Setting::Countryside, 600.0, 450.0),
            Kind::Farmhouse
        ));
        assert!(all(kinds(Setting::Countryside, 20.0, 450.0), Kind::Shed));
        assert!(all(kinds(Setting::Industrial, 900.0, 450.0), Kind::Hall));
        assert!(all(kinds(Setting::Town, 900.0, 450.0), Kind::Block));
        assert!(all(kinds(Setting::Town, 140.0, 450.0), Kind::House));
    }

    #[test]
    fn tall_buildings_are_blocks_and_marked_ones_churches() {
        let mut tall = untagged(1);
        tall.height = Some(24.0);
        let house = untagged(2);

        let dice = Dice(1);
        assert_eq!(
            kind(
                &plot(&tall, Setting::Countryside, false),
                300.0,
                450.0,
                &dice
            ),
            Kind::Block
        );
        assert_eq!(
            kind(&plot(&house, Setting::Town, true), 300.0, 450.0, &dice),
            Kind::Church
        );
    }

    #[test]
    fn mapped_heights_are_rounded_to_whole_storeys() {
        // A 2-storey house mapped 8 m high: 6.4 m of walls under a 3.2 m roof.
        assert!((whole_storeys(8.0 - 3.2 / 2.0) - 6.4).abs() < 1e-9);
        // Too low for a storey still gets one.
        assert!((whole_storeys(1.5) - 3.4).abs() < 1e-9);
    }

    #[test]
    fn church_points_mark_the_building_they_lie_in_or_the_largest_nearby() {
        let square = |e: f64, n: f64, half: f64| {
            vec![
                (e - half, n - half),
                (e + half, n - half),
                (e + half, n + half),
                (e - half, n + half),
            ]
        };
        let (shed, church, house) = (untagged(1), untagged(2), untagged(3));
        let mut plots = vec![
            Plot {
                footprint: square(0.0, 0.0, 2.0),
                ..plot(&shed, Setting::Town, false)
            },
            Plot {
                footprint: square(20.0, 0.0, 10.0),
                ..plot(&church, Setting::Town, false)
            },
            Plot {
                footprint: square(200.0, 0.0, 5.0),
                ..plot(&house, Setting::Town, false)
            },
        ];

        // In the churchyard, between the shed and the church.
        mark_churches(&mut plots, &[(5.0, 3.0)]);
        assert!(!plots[0].church && plots[1].church && !plots[2].church);

        // On the house itself.
        mark_churches(&mut plots, &[(201.0, 1.0)]);
        assert!(plots[2].church);
    }
}
