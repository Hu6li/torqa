//! The smooth curve through a route's points: the road is drawn along it and riders follow it,
//! so bends look like a road's, not a polygon's, and the rider stays on the road drawn.

/// The point `u` (0–1) of the way from `p[1]` to `p[2]` on a centripetal Catmull-Rom spline
/// through four points (flat coordinates in metres).
#[must_use]
pub fn catmull_rom(p: [(f64, f64); 4], u: f64) -> (f64, f64) {
    let knot = |a: (f64, f64), b: (f64, f64)| (b.0 - a.0).hypot(b.1 - a.1).sqrt().max(1e-6);
    let t1 = knot(p[0], p[1]);
    let t2 = t1 + knot(p[1], p[2]);
    let t3 = t2 + knot(p[2], p[3]);
    let t = t1 + (t2 - t1) * u;
    let mix = |a: (f64, f64), b: (f64, f64), from: f64, to: f64| {
        let w = (t - from) / (to - from);
        (a.0 + (b.0 - a.0) * w, a.1 + (b.1 - a.1) * w)
    };
    let a1 = mix(p[0], p[1], 0.0, t1);
    let a2 = mix(p[1], p[2], t1, t2);
    let a3 = mix(p[2], p[3], t2, t3);
    let b1 = mix(a1, a2, 0.0, t2);
    let b2 = mix(a2, a3, t1, t3);
    mix(b1, b2, t1, t2)
}
