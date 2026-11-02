//! Rider-selected seasons, independent of the calendar and the ride's weather (#350).

/// The season used to present a 3D ride. Older settings and unknown future seasons use summer.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Season {
    /// Green foliage and meadows, the original appearance.
    #[default]
    Summer,
    /// Meadow flowers and blossom among fresh green broadleaves.
    Spring,
    /// Dormant vegetation and snow where the route's climate allows it.
    Winter,
    /// Red, orange and gold broadleaf foliage.
    Autumn,
}

impl Season {
    /// Seasons available in the ride options, with the default first.
    pub const ALL: &[Self] = &[Self::Summer, Self::Spring, Self::Autumn, Self::Winter];

    /// Stable, untranslated name used by settings and the presentation boundary.
    #[must_use]
    pub const fn name(self) -> &'static str {
        // i18n-begin
        match self {
            Self::Summer => "Summer",
            Self::Spring => "Spring",
            Self::Winter => "Winter",
            Self::Autumn => "Autumn",
        }
        // i18n-end
    }

    /// Share of the course's flower clumps kept as flowers; the rest become grass.
    #[must_use]
    pub const fn flower_share(self) -> f64 {
        match self {
            Self::Summer => 0.1,
            Self::Spring => 1.0,
            Self::Autumn | Self::Winter => 0.0,
        }
    }

    /// Looks up a season without accepting an unknown setting.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|season| season.name() == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summer_is_the_default_and_names_round_trip() {
        assert_eq!(Season::default(), Season::Summer);
        for season in Season::ALL {
            assert_eq!(Season::from_name(season.name()), Some(*season));
        }
        assert_eq!(Season::from_name("unknown"), None);
    }
}
