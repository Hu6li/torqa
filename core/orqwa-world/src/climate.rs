//! The climate a ride passes through, which sets what grows and how houses are built (#136):
//! palms, banana plants and flat-roofed or low red-tiled houses in the (sub)tropics, the
//! temperate world everywhere else.

/// Up to this latitude (north or south) the lowlands count as tropical: the tropics proper end
/// at 23.4°, but palms and houses built for the heat reach further, to Ishigaki (24.3° N),
/// Okinawa (26.5° N), Taiwan and southern Florida.
const TROPICAL_LATITUDE: f64 = 27.0;
/// Above this elevation even the tropics are temperate: highland forests, no palms.
const TROPICAL_CEILING: f64 = 1200.0;

/// Seasonal material limits derived from the course's climate, without changing its geometry.
#[derive(Debug, Clone, Copy)]
pub struct SeasonClimate {
    /// Altitude in metres at which ground starts to carry snow.
    pub snow_line: f64,
    /// Lower limit for seasonal broadleaf changes; tropical lowlands stay evergreen.
    pub deciduous_from: f64,
}

/// Material limits for the selected season and route latitude (degrees north or south).
#[must_use]
pub fn season_climate(latitude: f64, season: orqwa_domain::season::Season) -> SeasonClimate {
    let tropical = Climate::at(latitude) == Climate::Tropical;
    let snow_line = match season {
        orqwa_domain::season::Season::Winter => {
            let latitude = latitude.abs();
            if tropical {
                4200.0
            } else if latitude < 40.0 {
                1800.0 - (latitude - TROPICAL_LATITUDE) / 13.0 * 1200.0
            } else {
                (600.0 - (latitude - 40.0) * 40.0).max(0.0)
            }
        }
        orqwa_domain::season::Season::Summer
        | orqwa_domain::season::Season::Spring
        | orqwa_domain::season::Season::Autumn => {
            if tropical {
                4500.0
            } else {
                2400.0
            }
        }
    };
    SeasonClimate {
        snow_line,
        deciduous_from: if tropical {
            TROPICAL_CEILING
        } else {
            -10_000.0
        },
    }
}

/// The climate of a ride's region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Climate {
    /// Conifers and broadleaf trees, houses with pitched roofs, chalets in the mountains.
    Temperate,
    /// Palms, banana plants and tropical shrubs, houses built for the heat.
    Tropical,
}

impl Climate {
    /// The climate at `latitude` (degrees); a ride stays within one.
    pub(crate) fn at(latitude: f64) -> Self {
        if latitude.abs() < TROPICAL_LATITUDE {
            Self::Tropical
        } else {
            Self::Temperate
        }
    }

    /// Whether the ground at `elevation` metres is tropical: in the tropics and below their
    /// highlands.
    pub(crate) fn tropical_at(self, elevation: f64) -> bool {
        self == Self::Tropical && elevation < TROPICAL_CEILING
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn winter_snow_is_climate_dependent_and_the_same_in_both_hemispheres() {
        use orqwa_domain::season::Season;
        for latitude in [1.0, 24.0, 32.0, 47.0, 60.0] {
            let north = season_climate(latitude, Season::Winter);
            let south = season_climate(-latitude, Season::Winter);
            assert_eq!(north.snow_line, south.snow_line);
            assert!(north.snow_line < season_climate(latitude, Season::Summer).snow_line);
        }
        assert!(season_climate(1.0, Season::Winter).snow_line > 4000.0);
        assert!(season_climate(32.0, Season::Winter).snow_line > 1000.0);
        assert!(season_climate(47.0, Season::Winter).snow_line < 500.0);
    }

    #[test]
    fn summer_keeps_tropical_lowlands_evergreen_and_snow_high_above_them() {
        use orqwa_domain::season::Season;
        for latitude in [-24.0, 1.0, 26.5] {
            let climate = season_climate(latitude, Season::Summer);
            assert_eq!(climate.deciduous_from, TROPICAL_CEILING);
            assert!(climate.snow_line > 4000.0);
        }
        let climate = season_climate(47.0, Season::Summer);
        assert_eq!(climate.snow_line, 2400.0);
        assert!(climate.deciduous_from < 0.0);
    }

    #[test]
    fn the_subtropical_lowlands_are_tropical() {
        // Ishigaki, Okinawa, Hawaii, Singapore, and Rio de Janeiro in the south.
        for latitude in [24.34, 26.2, 21.3, 1.35, -22.9] {
            assert!(Climate::at(latitude).tropical_at(10.0), "{latitude}");
        }
        // Bern, Brittany, Tokyo, Tasmania.
        for latitude in [46.95, 48.4, 35.7, -42.9] {
            assert_eq!(Climate::at(latitude), Climate::Temperate, "{latitude}");
        }
        // Highlands in the tropics are not.
        assert!(!Climate::at(4.6).tropical_at(2600.0));
    }
}
