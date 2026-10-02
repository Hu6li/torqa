//! Physical quantities as newtypes, so values with different units cannot be mixed up.
//!
//! Each type documents the unit of its inner value. SI units are used wherever one exists;
//! conversion to display units (km/h, imperial) happens only at the presentation edge.

macro_rules! quantity {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
        pub struct $name(pub f64);
    };
}

quantity!(
    /// Power in watts.
    Watts
);
quantity!(
    /// Distance or elevation in metres.
    Meters
);
quantity!(
    /// Mass in kilograms.
    Kilograms
);
quantity!(
    /// Area in square metres, e.g. the drag area `CdA`.
    SquareMeters
);
quantity!(
    /// Density in kilograms per cubic metre, e.g. air density.
    KilogramsPerCubicMeter
);
quantity!(
    /// Cadence in revolutions per minute.
    Rpm
);
quantity!(
    /// Speed in metres per second.
    MetersPerSecond
);
quantity!(
    /// Heart rate in beats per minute.
    BeatsPerMinute
);
quantity!(
    /// Road gradient in percent (rise over run × 100); negative downhill.
    GradePercent
);
quantity!(
    /// Wind resistance coefficient `½·ρ·CdA` in kilograms per metre.
    KilogramsPerMeter
);
quantity!(
    /// A share of a range in percent, 0–100.
    Percent
);

impl MetersPerSecond {
    /// Converts from kilometres per hour.
    #[must_use]
    pub fn from_kilometers_per_hour(kmh: f64) -> Self {
        Self(kmh / 3.6)
    }

    /// Converts to kilometres per hour.
    #[must_use]
    pub fn as_kilometers_per_hour(self) -> f64 {
        self.0 * 3.6
    }
}
