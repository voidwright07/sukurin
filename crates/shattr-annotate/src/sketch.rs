//! Deterministic hand-drawn wobble. Every random number derives from the
//! annotation id, so a shape redraws identically while it is being edited.

use crate::Pt;

fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

pub(crate) fn seed(id: u64, part: u64) -> u64 {
    mix(mix(id) ^ part.wrapping_mul(0xD6E8_FEB8_6659_FD93))
}

/// Uniform in [-1, 1].
pub(crate) fn unit(seed: u64, i: u64) -> f32 {
    (mix(seed ^ mix(i)) >> 40) as f32 / (1u64 << 23) as f32 - 1.0
}

/// Smooth value noise in [-1, 1] with features one unit apart.
fn noise(seed: u64, x: f32) -> f32 {
    let i = x.floor();
    let t = x - i;
    let t = t * t * (3.0 - 2.0 * t);
    let a = unit(seed, i as i64 as u64);
    let b = unit(seed, (i as i64 + 1) as u64);
    a + (b - a) * t
}

/// Wavelength of the wobble along the stroke, in pixels.
const WAVELENGTH: f32 = 70.0;

/// Resamples `pts` every few pixels and pushes each sample sideways by low
/// frequency noise. `taper_end` pins the last point so arrow shafts stay
/// under their heads.
pub(crate) fn wobble(pts: &[Pt], seed: u64, amp: f32, taper_end: bool) -> Vec<Pt> {
    let total: f32 = pts.windows(2).map(|w| w[0].dist(w[1])).sum();
    if pts.len() < 2 || total < 1.0 {
        return pts.to_vec();
    }
    let step = 3.0;
    let n = ((total / step).ceil() as usize).max(2);
    let mut out = Vec::with_capacity(n + 1);
    let mut seg = 0;
    let mut seg_start = 0.0;
    for k in 0..=n {
        let s = total * k as f32 / n as f32;
        while seg + 2 < pts.len() && seg_start + pts[seg].dist(pts[seg + 1]) < s {
            seg_start += pts[seg].dist(pts[seg + 1]);
            seg += 1;
        }
        let (a, b) = (pts[seg], pts[seg + 1]);
        let l = a.dist(b).max(1e-6);
        let p = a.lerp(b, ((s - seg_start) / l).clamp(0.0, 1.0));
        let normal = (b - a).norm().perp();
        let mut off = noise(seed, s / WAVELENGTH) * amp;
        if taper_end {
            off *= ((total - s) / (WAVELENGTH * 0.5)).min(1.0);
        }
        out.push(p + normal * off);
    }
    out
}

/// A straight stroke the way a marker draws it: slightly bowed, ends
/// overshooting a little, wavering along the way.
pub(crate) fn line(a: Pt, b: Pt, seed: u64, amp: f32) -> Vec<Pt> {
    let len = a.dist(b);
    let dir = (b - a).norm();
    let n = dir.perp();
    let over = amp * 2.0;
    let a2 = a - dir * (over * (0.6 + 0.4 * unit(seed, 1))) + n * (amp * unit(seed, 2));
    let b2 = b + dir * (over * (0.6 + 0.4 * unit(seed, 3))) + n * (amp * unit(seed, 4));
    let bow = (len * 0.015).min(amp * 2.5) * unit(seed, 5);
    let steps = ((len / 6.0) as usize).clamp(4, 400);
    let pts: Vec<Pt> = (0..=steps)
        .map(|i| {
            let t = i as f32 / steps as f32;
            a2.lerp(b2, t) + n * (bow * 4.0 * t * (1.0 - t))
        })
        .collect();
    wobble(&pts, seed ^ 0xA5A5, amp * 0.6, false)
}
