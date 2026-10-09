//! Canvas placement inside the canvas area. Canvas space is the document's
//! physical pixels; view space is the canvas area's logical pixels. "100%"
//! means one image pixel per device pixel, so percentages go through the
//! window's scale factor.

use sukurin_annotate::{Pt, RectF};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    /// View pixels per canvas pixel.
    pub zoom: f32,
    /// View position of canvas (0, 0).
    pub origin: Pt,
}

pub const MIN_PERCENT: f32 = 5.;
pub const MAX_PERCENT: f32 = 6400.;
const STEPS: [f32; 16] = [10., 25., 33., 50., 67., 75., 100., 150., 200., 300., 400., 600., 800., 1200., 1600., 3200.];

impl Viewport {
    /// Centres `content` in an area of `w` x `h` view pixels, shrinking it to
    /// fit inside `margin` but never enlarging past 100%.
    pub fn fit(content: RectF, w: f32, h: f32, margin: f32, scale_factor: f32) -> Self {
        let avail_w = (w - 2. * margin).max(1.);
        let avail_h = (h - 2. * margin).max(1.);
        let actual = 1. / scale_factor;
        let zoom = (avail_w / content.w.max(1.)).min(avail_h / content.h.max(1.)).min(actual);
        let origin = Pt::new(
            (w - content.w * zoom) / 2. - content.x * zoom,
            (h - content.h * zoom) / 2. - content.y * zoom,
        );
        Self { zoom, origin }
    }

    pub fn to_view(self, p: Pt) -> Pt {
        Pt::new(self.origin.x + p.x * self.zoom, self.origin.y + p.y * self.zoom)
    }

    pub fn to_canvas(self, v: Pt) -> Pt {
        Pt::new((v.x - self.origin.x) / self.zoom, (v.y - self.origin.y) / self.zoom)
    }

    pub fn rect_to_view(self, r: RectF) -> RectF {
        let o = self.to_view(Pt::new(r.x, r.y));
        RectF::new(o.x, o.y, r.w * self.zoom, r.h * self.zoom)
    }

    /// Zooms to `zoom`, keeping the canvas point under `anchor` (view space) fixed.
    pub fn zoom_at(&self, zoom: f32, anchor: Pt, scale_factor: f32) -> Self {
        let zoom = clamp_zoom(zoom, scale_factor);
        let c = self.to_canvas(anchor);
        Self { zoom, origin: Pt::new(anchor.x - c.x * zoom, anchor.y - c.y * zoom) }
    }

    pub fn pan(&self, dx: f32, dy: f32) -> Self {
        Self { zoom: self.zoom, origin: Pt::new(self.origin.x + dx, self.origin.y + dy) }
    }

    pub fn percent(&self, scale_factor: f32) -> f32 {
        self.zoom * scale_factor * 100.
    }
}

pub fn clamp_zoom(zoom: f32, scale_factor: f32) -> f32 {
    zoom.clamp(MIN_PERCENT / 100. / scale_factor, MAX_PERCENT / 100. / scale_factor)
}

/// The next preset zoom (in view pixels per canvas pixel) above or below the
/// current one.
pub fn step(zoom: f32, scale_factor: f32, up: bool) -> f32 {
    let pct = zoom * scale_factor * 100.;
    let next = if up {
        STEPS.iter().copied().find(|s| *s > pct + 0.5).unwrap_or(MAX_PERCENT)
    } else {
        STEPS.iter().rev().copied().find(|s| *s < pct - 0.5).unwrap_or(MIN_PERCENT)
    };
    next / 100. / scale_factor
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_centres_and_never_enlarges_past_actual_size() {
        let small = Viewport::fit(RectF::new(0., 0., 100., 50.), 1000., 800., 32., 1.5);
        assert_eq!(small.zoom, 1. / 1.5);
        let v = small.to_view(Pt::new(50., 25.));
        assert!((v.x - 500.).abs() < 1e-3 && (v.y - 400.).abs() < 1e-3);

        let big = Viewport::fit(RectF::new(0., 0., 2560., 1600.), 1396., 794., 32., 1.5);
        let r = big.rect_to_view(RectF::new(0., 0., 2560., 1600.));
        assert!(r.x >= 31.9 && r.y >= 31.9 && r.x + r.w <= 1364.1 && r.y + r.h <= 762.1);
        assert!((r.y - 32.).abs() < 1e-3, "height is the binding side: {r:?}");
    }

    #[test]
    fn zoom_at_keeps_the_anchor_still() {
        let v = Viewport { zoom: 0.5, origin: Pt::new(10., 20.) };
        let anchor = Pt::new(300., 200.);
        let before = v.to_canvas(anchor);
        let z = v.zoom_at(2.0, anchor, 1.);
        let after = z.to_canvas(anchor);
        assert!((before.x - after.x).abs() < 1e-3 && (before.y - after.y).abs() < 1e-3);
        assert_eq!(z.zoom, 2.0);
    }

    #[test]
    fn view_and_canvas_round_trip() {
        let v = Viewport { zoom: 1.75, origin: Pt::new(-40., 12.5) };
        let p = Pt::new(123.25, 456.5);
        let back = v.to_canvas(v.to_view(p));
        assert!((back.x - p.x).abs() < 1e-3 && (back.y - p.y).abs() < 1e-3);
    }

    #[test]
    fn steps_walk_the_preset_ladder_in_device_percent() {
        let s = 1.5;
        let z70 = 0.70 / s;
        assert!((step(z70, s, true) * s * 100. - 75.).abs() < 1e-3);
        assert!((step(z70, s, false) * s * 100. - 67.).abs() < 1e-3);
        let z100 = 1. / s;
        assert!((step(z100, s, true) * s * 100. - 150.).abs() < 1e-3);
        assert_eq!(clamp_zoom(1000., s) * s * 100., MAX_PERCENT);
    }
}
