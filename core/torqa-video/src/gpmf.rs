//! GoPro's GPMF metadata (as GoPro documents it in gopro/gpmf-parser): the GPS positions a
//! camera records next to the video, used to sync a ride to the footage (R17).
//!
//! GPMF is a tree of key-length-value entries: a four-letter key, a type letter (0 for nested
//! entries), the size of one sample and the number of samples, then the data padded to four
//! bytes; numbers are big-endian. GPS sits in `DEVC` → `STRM` with `GPS5` (lat, lon, altitude,
//! 2D speed, 3D speed) on older cameras or `GPS9` (adding date, time, DOP and fix) from the
//! HERO11, each scaled by the stream's `SCAL` divisors.

/// A position the camera recorded.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GpsPoint {
    /// Latitude in degrees (WGS84).
    pub lat: f64,
    /// Longitude in degrees (WGS84).
    pub lon: f64,
    /// Altitude in metres.
    pub altitude: f64,
    /// Ground speed in metres per second.
    pub speed: f64,
}

/// The GPS positions in one GPMF payload, in recording order. Payloads without GPS (other
/// sensors) give none; positions without a fix are left out.
#[must_use]
pub fn gps_points(payload: &[u8]) -> Vec<GpsPoint> {
    let mut points = Vec::new();
    for (key, entry) in entries(payload) {
        if key == *b"DEVC" {
            for (key, stream) in entries(entry.data) {
                if key == *b"STRM" {
                    points.extend(stream_points(stream.data));
                }
            }
        }
    }
    points
}

/// One key-length-value entry.
struct Entry<'a> {
    kind: u8,
    sample_size: usize,
    repeat: usize,
    data: &'a [u8],
}

/// The entries at one level of a GPMF tree; stops at malformed data.
fn entries(mut data: &[u8]) -> Vec<([u8; 4], Entry<'_>)> {
    let mut found = Vec::new();
    while data.len() >= 8 {
        let key = [data[0], data[1], data[2], data[3]];
        let kind = data[4];
        let sample_size = usize::from(data[5]);
        let repeat = usize::from(u16::from_be_bytes([data[6], data[7]]));
        let length = sample_size * repeat;
        let padded = length.div_ceil(4) * 4;
        if data.len() < 8 + padded {
            break;
        }
        found.push((
            key,
            Entry {
                kind,
                sample_size,
                repeat,
                data: &data[8..8 + length],
            },
        ));
        data = &data[8 + padded..];
    }
    found
}

/// GPS from one sensor stream: `GPS5` or `GPS9` with the stream's `SCAL` and fix.
fn stream_points(stream: &[u8]) -> Vec<GpsPoint> {
    let mut scales: Vec<f64> = Vec::new();
    let mut fix: Option<f64> = None;
    let mut points = Vec::new();
    for (key, entry) in entries(stream) {
        match &key {
            b"SCAL" => scales = numbers(&entry),
            b"GPSF" => fix = numbers(&entry).first().copied(),
            b"GPS5" | b"GPS9" => {
                let gps9 = key == *b"GPS9";
                // A 2D fix (or better) is needed for a usable position; GPS9 carries it per
                // sample, GPS5 per stream (missing means unknown: keep the points).
                if !gps9 && fix.is_some_and(|f| f < 2.0) {
                    continue;
                }
                for sample in entry.data.chunks_exact(entry.sample_size.max(1)) {
                    let values = signed_values(sample, gps9);
                    let value = |i: usize| {
                        let divisor = scales.get(i).or(scales.first()).copied().unwrap_or(1.0);
                        values.get(i).copied().unwrap_or(0.0)
                            / if divisor == 0.0 { 1.0 } else { divisor }
                    };
                    if gps9 && value(8) < 2.0 {
                        continue;
                    }
                    points.push(GpsPoint {
                        lat: value(0),
                        lon: value(1),
                        altitude: value(2),
                        speed: value(3),
                    });
                }
            }
            _ => {}
        }
    }
    points
}

/// A GPS sample's raw numbers: five signed 32-bit values for `GPS5`; for `GPS9` five signed
/// 32-bit values, two more 32-bit ones (days, seconds) and two unsigned 16-bit ones (DOP, fix).
#[allow(clippy::cast_precision_loss)] // raw GPS integers fit f64 exactly
fn signed_values(sample: &[u8], gps9: bool) -> Vec<f64> {
    let int = |at: usize| {
        sample.get(at..at + 4).map_or(0.0, |b| {
            f64::from(i32::from_be_bytes([b[0], b[1], b[2], b[3]]))
        })
    };
    let mut values: Vec<f64> = (0..if gps9 { 7 } else { 5 }).map(|i| int(i * 4)).collect();
    if gps9 {
        for at in [28, 30] {
            values.push(
                sample
                    .get(at..at + 2)
                    .map_or(0.0, |b| f64::from(u16::from_be_bytes([b[0], b[1]]))),
            );
        }
    }
    values
}

/// The numbers of an entry such as `SCAL` or `GPSF`, whatever their integer type.
fn numbers(entry: &Entry<'_>) -> Vec<f64> {
    let width = match entry.kind {
        b'l' | b'L' => 4,
        b's' | b'S' => 2,
        b'b' | b'B' => 1,
        _ => return Vec::new(),
    };
    entry
        .data
        .chunks_exact(width)
        .take(entry.repeat * entry.sample_size / width)
        .map(|b| match (entry.kind, b.len()) {
            (b'l', 4) => f64::from(i32::from_be_bytes([b[0], b[1], b[2], b[3]])),
            (b'L', 4) => f64::from(u32::from_be_bytes([b[0], b[1], b[2], b[3]])),
            (b's', 2) => f64::from(i16::from_be_bytes([b[0], b[1]])),
            (b'S', 2) => f64::from(u16::from_be_bytes([b[0], b[1]])),
            (b'b', 1) => f64::from(i8::from_be_bytes([b[0]])),
            _ => f64::from(b[0]),
        })
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// One GPMF entry, padded to four bytes.
    pub(crate) fn entry(
        key: [u8; 4],
        kind: u8,
        sample_size: u8,
        repeat: u16,
        data: &[u8],
    ) -> Vec<u8> {
        let mut bytes = key.to_vec();
        bytes.push(kind);
        bytes.push(sample_size);
        bytes.extend(repeat.to_be_bytes());
        bytes.extend(data);
        while !bytes.len().is_multiple_of(4) {
            bytes.push(0);
        }
        bytes
    }

    fn be(values: &[i32]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_be_bytes()).collect()
    }

    /// A HERO-style payload: a device with an accelerometer stream and a `GPS5` stream.
    pub(crate) fn gps5_payload(points: &[(f64, f64, f64, f64)], fix: u32) -> Vec<u8> {
        let scales = be(&[10_000_000, 10_000_000, 1000, 1000, 100]);
        let mut samples = Vec::new();
        #[allow(clippy::cast_possible_truncation)]
        for &(lat, lon, alt, speed) in points {
            samples.extend(be(&[
                (lat * 1e7).round() as i32,
                (lon * 1e7).round() as i32,
                (alt * 1000.0).round() as i32,
                (speed * 1000.0).round() as i32,
                (speed * 100.0).round() as i32,
            ]));
        }
        let mut gps = entry(*b"STNM", b'c', 1, 9, b"GPS (Lat.");
        gps.extend(entry(*b"GPSF", b'L', 4, 1, &fix.to_be_bytes()));
        gps.extend(entry(*b"SCAL", b'l', 4, 5, &scales));
        gps.extend(entry(
            *b"GPS5",
            b'l',
            20,
            u16::try_from(points.len()).unwrap(),
            &samples,
        ));
        let accel = entry(*b"ACCL", b's', 6, 2, &[0; 12]);
        let mut device = entry(*b"DVID", b'L', 4, 1, &1u32.to_be_bytes());
        device.extend(entry(
            *b"STRM",
            0,
            1,
            u16::try_from(accel.len()).unwrap(),
            &accel,
        ));
        device.extend(entry(
            *b"STRM",
            0,
            1,
            u16::try_from(gps.len()).unwrap(),
            &gps,
        ));
        entry(
            *b"DEVC",
            0,
            1,
            u16::try_from(device.len()).unwrap(),
            &device,
        )
    }

    #[test]
    fn reads_gps5_positions_scaled() {
        let payload = gps5_payload(
            &[
                (46.948_123, 7.447_456, 540.5, 8.25),
                (46.948_200, 7.447_500, 541.0, 8.5),
            ],
            3,
        );

        let points = gps_points(&payload);

        assert_eq!(points.len(), 2);
        assert!((points[0].lat - 46.948_123).abs() < 1e-7);
        assert!((points[0].lon - 7.447_456).abs() < 1e-7);
        assert!((points[0].altitude - 540.5).abs() < 1e-3);
        assert!((points[1].speed - 8.5).abs() < 1e-3);
    }

    #[test]
    fn positions_without_a_fix_are_left_out() {
        let payload = gps5_payload(&[(46.9, 7.4, 500.0, 0.0)], 0);

        assert_eq!(gps_points(&payload), []);
    }

    #[test]
    fn reads_gps9_with_per_sample_fix() {
        let scales = be(&[10_000_000, 10_000_000, 1000, 1000, 100, 1, 1000, 100, 1]);
        let sample = |lat: i32, fix: u16| {
            let mut bytes = be(&[lat, 74_474_560, 540_500, 8250, 825, 9_407, 25_200_000]);
            bytes.extend(150u16.to_be_bytes());
            bytes.extend(fix.to_be_bytes());
            bytes
        };
        let mut samples = sample(469_481_230, 3);
        samples.extend(sample(469_482_000, 0));
        let mut stream = entry(*b"SCAL", b'l', 4, 9, &scales);
        stream.extend(entry(*b"GPS9", b'?', 32, 2, &samples));
        let device = entry(
            *b"STRM",
            0,
            1,
            u16::try_from(stream.len()).unwrap(),
            &stream,
        );
        let payload = entry(
            *b"DEVC",
            0,
            1,
            u16::try_from(device.len()).unwrap(),
            &device,
        );

        let points = gps_points(&payload);

        // The second sample has no fix.
        assert_eq!(points.len(), 1);
        assert!((points[0].lat - 46.948_123).abs() < 1e-7);
        assert!((points[0].speed - 8.25).abs() < 1e-3);
    }

    #[test]
    fn malformed_payloads_give_nothing_rather_than_panicking() {
        assert_eq!(gps_points(b"DEVC\x00\x01\xff\xff"), []);
        assert_eq!(gps_points(&[]), []);
        assert_eq!(gps_points(b"garbage that is not gpmf at all"), []);
    }
}
