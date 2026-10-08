use std::ops::{Add, Mul, Sub};

use tiny_skia::{LineCap, LineJoin, Path, PathBuilder, PathSegment, Stroke, StrokeDash};

use crate::{Pt, RectF};

/// One closed contour; the closing edge is implicit.
pub(crate) type Poly = Vec<Pt>;

/// Maximum distance between a curve and its flattened chords, in pixels.
const TOLERANCE: f32 = 0.25;

impl Add for Pt {
    type Output = Pt;
    fn add(self, o: Pt) -> Pt {
        Pt::new(self.x + o.x, self.y + o.y)
    }
}

impl Sub for Pt {
    type Output = Pt;
    fn sub(self, o: Pt) -> Pt {
        Pt::new(self.x - o.x, self.y - o.y)
    }
}

impl Mul<f32> for Pt {
    type Output = Pt;
    fn mul(self, k: f32) -> Pt {
        Pt::new(self.x * k, self.y * k)
    }
}

impl Pt {
    pub(crate) fn len(self) -> f32 {
        self.x.hypot(self.y)
    }

    pub(crate) fn dist(self, o: Pt) -> f32 {
        (self - o).len()
    }

    pub(crate) fn norm(self) -> Pt {
        let l = self.len();
        if l > 1e-6 { self * (1.0 / l) } else { Pt::new(1.0, 0.0) }
    }

    /// Rotated 90 degrees counter-clockwise on screen (y down).
    pub(crate) fn perp(self) -> Pt {
        Pt::new(self.y, -self.x)
    }

    pub(crate) fn lerp(self, o: Pt, t: f32) -> Pt {
        self + (o - self) * t
    }
}

impl RectF {
    pub(crate) fn right(&self) -> f32 {
        self.x + self.w
    }

    pub(crate) fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub(crate) fn center(&self) -> Pt {
        Pt::new(self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    pub(crate) fn inflate(&self, d: f32) -> RectF {
        self.pad(d, d)
    }

    pub(crate) fn pad(&self, x: f32, y: f32) -> RectF {
        RectF::new(self.x - x, self.y - y, self.w + 2.0 * x, self.h + 2.0 * y)
    }

    /// The same rectangle with non-negative width and height.
    pub(crate) fn normalized(&self) -> RectF {
        RectF::from_points(Pt::new(self.x, self.y), Pt::new(self.right(), self.bottom()))
    }

    pub(crate) fn intersect(&self, o: &RectF) -> Option<RectF> {
        let (x0, y0) = (self.x.max(o.x), self.y.max(o.y));
        let (x1, y1) = (self.right().min(o.right()), self.bottom().min(o.bottom()));
        (x1 > x0 && y1 > y0).then(|| RectF::new(x0, y0, x1 - x0, y1 - y0))
    }

    pub(crate) fn union(&self, o: &RectF) -> RectF {
        let x0 = self.x.min(o.x);
        let y0 = self.y.min(o.y);
        RectF::new(x0, y0, self.right().max(o.right()) - x0, self.bottom().max(o.bottom()) - y0)
    }

    pub(crate) fn to_skia(self) -> Option<tiny_skia::Rect> {
        tiny_skia::Rect::from_xywh(self.x, self.y, self.w, self.h)
    }
}

fn pt(p: tiny_skia::Point) -> Pt {
    Pt::new(p.x, p.y)
}

fn quad_at(a: Pt, b: Pt, c: Pt, t: f32) -> Pt {
    let u = 1.0 - t;
    a * (u * u) + b * (2.0 * u * t) + c * (t * t)
}

fn cubic_at(a: Pt, b: Pt, c: Pt, d: Pt, t: f32) -> Pt {
    let u = 1.0 - t;
    a * (u * u * u) + b * (3.0 * u * u * t) + c * (3.0 * u * t * t) + d * (t * t * t)
}

fn segments_for(second_difference: f32, k: f32) -> usize {
    ((second_difference * k / TOLERANCE).sqrt().ceil() as usize).clamp(1, 96)
}

/// Closes every contour of `path` and flattens its curves to chords.
pub(crate) fn flatten(path: &Path) -> Vec<Poly> {
    let mut out = Vec::new();
    let mut cur: Poly = Vec::new();
    let mut last = Pt::default();
    for seg in path.segments() {
        match seg {
            PathSegment::MoveTo(p) => {
                finish(&mut out, &mut cur);
                last = pt(p);
                cur.push(last);
            }
            PathSegment::LineTo(p) => {
                last = pt(p);
                cur.push(last);
            }
            PathSegment::QuadTo(c, p) => {
                let (c, p) = (pt(c), pt(p));
                let n = segments_for((last - c * 2.0 + p).len(), 0.25);
                cur.extend((1..=n).map(|i| quad_at(last, c, p, i as f32 / n as f32)));
                last = p;
            }
            PathSegment::CubicTo(c1, c2, p) => {
                let (c1, c2, p) = (pt(c1), pt(c2), pt(p));
                let dd = (last - c1 * 2.0 + c2).len().max((c1 - c2 * 2.0 + p).len());
                let n = segments_for(dd, 0.75);
                cur.extend((1..=n).map(|i| cubic_at(last, c1, c2, p, i as f32 / n as f32)));
                last = p;
            }
            PathSegment::Close => finish(&mut out, &mut cur),
        }
    }
    finish(&mut out, &mut cur);
    out
}

fn finish(out: &mut Vec<Poly>, cur: &mut Poly) {
    cur.dedup_by(|a, b| (a.x - b.x).abs() < 1e-4 && (a.y - b.y).abs() < 1e-4);
    if cur.len() > 1 && cur[0].dist(cur[cur.len() - 1]) < 1e-4 {
        cur.pop();
    }
    if cur.len() >= 3 {
        out.push(std::mem::take(cur));
    } else {
        cur.clear();
    }
}

pub(crate) fn signed_area(poly: &[Pt]) -> f32 {
    let mut a = 0.0;
    let n = poly.len();
    for i in 0..n {
        let (p, q) = (poly[i], poly[(i + 1) % n]);
        a += p.x * q.y - q.x * p.y;
    }
    a * 0.5
}

/// Flips the whole set so its net area is positive, keeping holes opposite
/// to their outlines. Shapes that share one non-zero fill must agree on
/// direction or their overlaps would cancel into holes.
pub(crate) fn orient(mut polys: Vec<Poly>, positive: bool) -> Vec<Poly> {
    let total: f32 = polys.iter().map(|p| signed_area(p)).sum();
    if (total > 0.0) != positive {
        for p in &mut polys {
            p.reverse();
        }
    }
    polys
}

#[derive(Clone, Copy)]
pub(crate) struct Pen {
    pub width: f32,
    pub cap: LineCap,
    pub join: LineJoin,
}

impl Pen {
    pub(crate) fn round(width: f32) -> Self {
        Self { width, cap: LineCap::Round, join: LineJoin::Round }
    }

    pub(crate) fn butt(width: f32) -> Self {
        Self { width, cap: LineCap::Butt, join: LineJoin::Round }
    }
}

/// Outline of `path` stroked with `pen`, optionally dashed, wound positive.
pub(crate) fn stroke(path: &Path, pen: Pen, dash: Option<StrokeDash>) -> Vec<Poly> {
    let dashed;
    let path = match dash.and_then(|d| path.dash(&d, 1.0)) {
        Some(d) => {
            dashed = d;
            &dashed
        }
        None => path,
    };
    let s = Stroke { width: pen.width.max(0.01), miter_limit: 4.0, line_cap: pen.cap, line_join: pen.join, dash: None };
    match path.stroke(&s, 1.0) {
        Some(outline) => orient(flatten(&outline), true),
        None => Vec::new(),
    }
}

pub(crate) fn fill(path: &Path) -> Vec<Poly> {
    orient(flatten(path), true)
}

pub(crate) fn polyline_path(pts: &[Pt], closed: bool) -> Option<Path> {
    let mut pb = PathBuilder::with_capacity(pts.len() + 1, pts.len());
    let (first, rest) = pts.split_first()?;
    pb.move_to(first.x, first.y);
    if rest.is_empty() {
        pb.line_to(first.x + 0.01, first.y);
    }
    for p in rest {
        pb.line_to(p.x, p.y);
    }
    if closed {
        pb.close();
    }
    pb.finish()
}

pub(crate) fn polys_path(polys: &[Poly]) -> Option<Path> {
    let mut pb = PathBuilder::new();
    for poly in polys {
        let Some((first, rest)) = poly.split_first() else { continue };
        pb.move_to(first.x, first.y);
        for p in rest {
            pb.line_to(p.x, p.y);
        }
        pb.close();
    }
    pb.finish()
}

/// Rounded rectangle from four cubic quarter-arcs, clockwise on screen.
pub(crate) fn rounded_rect(pb: &mut PathBuilder, r: RectF, radius: f32) {
    let rad = radius.clamp(0.0, r.w.min(r.h) / 2.0);
    let (x0, y0, x1, y1) = (r.x, r.y, r.right(), r.bottom());
    if rad <= 0.01 {
        pb.move_to(x0, y0);
        pb.line_to(x1, y0);
        pb.line_to(x1, y1);
        pb.line_to(x0, y1);
        pb.close();
        return;
    }
    let k = rad * (1.0 - 0.552_284_8);
    pb.move_to(x0 + rad, y0);
    pb.line_to(x1 - rad, y0);
    pb.cubic_to(x1 - k, y0, x1, y0 + k, x1, y0 + rad);
    pb.line_to(x1, y1 - rad);
    pb.cubic_to(x1, y1 - k, x1 - k, y1, x1 - rad, y1);
    pb.line_to(x0 + rad, y1);
    pb.cubic_to(x0 + k, y1, x0, y1 - k, x0, y1 - rad);
    pb.line_to(x0, y0 + rad);
    pb.cubic_to(x0, y0 + k, x0 + k, y0, x0 + rad, y0);
    pb.close();
}

pub(crate) fn rounded_rect_path(r: RectF, radius: f32) -> Option<Path> {
    let mut pb = PathBuilder::new();
    rounded_rect(&mut pb, r, radius);
    pb.finish()
}

pub(crate) fn oval_path(r: RectF) -> Option<Path> {
    PathBuilder::from_oval(r.to_skia()?)
}

/// Ellipse inscribed in `r` as a dense polygon, clockwise on screen.
pub(crate) fn ellipse_poly(r: RectF) -> Poly {
    let c = r.center();
    let (rx, ry) = (r.w / 2.0, r.h / 2.0);
    let n = (((rx + ry) * std::f32::consts::PI / 3.0) as usize).clamp(24, 360);
    (0..n)
        .map(|i| {
            let t = i as f32 / n as f32 * std::f32::consts::TAU;
            Pt::new(c.x + rx * t.cos(), c.y + ry * t.sin())
        })
        .collect()
}

pub(crate) fn bounds_of<'a>(polys: impl IntoIterator<Item = &'a Poly>) -> Option<RectF> {
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for p in polys.into_iter().flatten() {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    (x1 >= x0).then(|| RectF::new(x0, y0, x1 - x0, y1 - y0))
}

fn winding(polys: &[Poly], p: Pt) -> i32 {
    let mut w = 0;
    for poly in polys {
        let n = poly.len();
        for i in 0..n {
            let (a, b) = (poly[i], poly[(i + 1) % n]);
            let side = (b.x - a.x) * (p.y - a.y) - (p.x - a.x) * (b.y - a.y);
            if a.y <= p.y {
                if b.y > p.y && side > 0.0 {
                    w += 1;
                }
            } else if b.y <= p.y && side < 0.0 {
                w -= 1;
            }
        }
    }
    w
}

pub(crate) fn segment_distance(p: Pt, a: Pt, b: Pt) -> f32 {
    let ab = b - a;
    let l2 = ab.x * ab.x + ab.y * ab.y;
    let t = if l2 > 0.0 { (((p - a).x * ab.x + (p - a).y * ab.y) / l2).clamp(0.0, 1.0) } else { 0.0 };
    p.dist(a + ab * t)
}

pub(crate) fn outline_distance(polys: &[Poly], p: Pt) -> f32 {
    let mut d = f32::MAX;
    for poly in polys {
        let n = poly.len();
        for i in 0..n {
            d = d.min(segment_distance(p, poly[i], poly[(i + 1) % n]));
        }
    }
    d
}

/// Whether `p` is inside the non-zero fill of `polys` or within `tol` of it.
pub(crate) fn fill_hit(polys: &[Poly], p: Pt, tol: f32) -> bool {
    let Some(b) = bounds_of(polys) else { return false };
    if !b.inflate(tol).contains(p) {
        return false;
    }
    winding(polys, p) != 0 || outline_distance(polys, p) <= tol
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flattened_circle_stays_within_tolerance() {
        let path = PathBuilder::from_circle(50.0, 50.0, 40.0).unwrap();
        let polys = flatten(&path);
        assert_eq!(polys.len(), 1);
        for p in &polys[0] {
            assert!((p.dist(Pt::new(50.0, 50.0)) - 40.0).abs() <= TOLERANCE + 0.05);
        }
        for w in polys[0].windows(2) {
            let mid = w[0].lerp(w[1], 0.5);
            assert!(40.0 - mid.dist(Pt::new(50.0, 50.0)) <= TOLERANCE + 0.05);
        }
    }

    #[test]
    fn rect_stroke_is_a_ring_under_nonzero() {
        let path = rounded_rect_path(RectF::new(0.0, 0.0, 100.0, 50.0), 0.0).unwrap();
        let ring = stroke(&path, Pen::butt(4.0), None);
        assert!(fill_hit(&ring, Pt::new(0.0, 25.0), 0.0));
        assert!(!fill_hit(&ring, Pt::new(50.0, 25.0), 0.0));
    }
}
