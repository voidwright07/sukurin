//! Pure geometry for drawing and editing shapes: drag constraints, handle
//! placement, and the transforms that move and resize a shape.

use sukurin_annotate::{ArrowKind, Axis, Pt, RectF, Shape};

/// Snaps `to` so the segment from `from` lies on a multiple of 15°.
pub fn snap_angle(from: Pt, to: Pt) -> Pt {
    let (dx, dy) = (to.x - from.x, to.y - from.y);
    let len = dx.hypot(dy);
    let step = 15f32.to_radians();
    let a = (dy.atan2(dx) / step).round() * step;
    Pt::new(from.x + len * a.cos(), from.y + len * a.sin())
}

/// The rectangle a drag from `anchor` to `p` describes. `square` forces equal
/// sides, `centre` grows it around the anchor.
pub fn drag_rect(anchor: Pt, p: Pt, square: bool, centre: bool) -> RectF {
    let (mut dx, mut dy) = (p.x - anchor.x, p.y - anchor.y);
    if square {
        let s = dx.abs().max(dy.abs());
        dx = s.copysign(if dx == 0. { 1. } else { dx });
        dy = s.copysign(if dy == 0. { 1. } else { dy });
    }
    if centre {
        RectF::from_points(Pt::new(anchor.x - dx, anchor.y - dy), Pt::new(anchor.x + dx, anchor.y + dy))
    } else {
        RectF::from_points(anchor, Pt::new(anchor.x + dx, anchor.y + dy))
    }
}

/// A two-point segment drawn from `anchor` to `p`; `centre` mirrors it.
pub fn drag_segment(anchor: Pt, p: Pt, snap: bool, centre: bool) -> (Pt, Pt) {
    let p = if snap { snap_angle(anchor, p) } else { p };
    if centre { (Pt::new(2. * anchor.x - p.x, 2. * anchor.y - p.y), p) } else { (anchor, p) }
}

fn map_pt(p: Pt, from: RectF, to: RectF) -> Pt {
    let sx = if from.w.abs() < 1e-6 { 1. } else { to.w / from.w };
    let sy = if from.h.abs() < 1e-6 { 1. } else { to.h / from.h };
    Pt::new(to.x + (p.x - from.x) * sx, to.y + (p.y - from.y) * sy)
}

fn map_rect(r: RectF, from: RectF, to: RectF) -> RectF {
    let a = map_pt(Pt::new(r.x, r.y), from, to);
    let b = map_pt(Pt::new(r.x + r.w, r.y + r.h), from, to);
    RectF::from_points(a, b)
}

/// Every point of `shape` mapped by the affine transform taking `from` onto `to`.
pub fn map_shape(shape: &Shape, from: RectF, to: RectF) -> Shape {
    let m = |p: Pt| map_pt(p, from, to);
    let mr = |r: RectF| map_rect(r, from, to);
    match shape {
        Shape::Arrow { from: a, to: b, control, kind } => {
            Shape::Arrow { from: m(*a), to: m(*b), control: control.map(m), kind: *kind }
        }
        Shape::Line { from: a, to: b } => Shape::Line { from: m(*a), to: m(*b) },
        Shape::Rect { rect } => Shape::Rect { rect: mr(*rect) },
        Shape::Oval { rect } => Shape::Oval { rect: mr(*rect) },
        Shape::Freehand { points } => Shape::Freehand { points: points.iter().copied().map(m).collect() },
        Shape::Highlighter { points } => Shape::Highlighter { points: points.iter().copied().map(m).collect() },
        Shape::Text { origin, text } => Shape::Text { origin: m(*origin), text: text.clone() },
        Shape::Step { center, number } => Shape::Step { center: m(*center), number: *number },
        Shape::Spotlight { rect, oval } => Shape::Spotlight { rect: mr(*rect), oval: *oval },
        Shape::Redact { rect, mode, block } => Shape::Redact { rect: mr(*rect), mode: *mode, block: *block },
        Shape::Magnifier { source, callout } => Shape::Magnifier { source: mr(*source), callout: mr(*callout) },
        Shape::Measure { from: a, to: b } => Shape::Measure { from: m(*a), to: m(*b) },
        Shape::Guide { axis, at } => {
            let p = match axis {
                Axis::Horizontal => m(Pt::new(0., *at)).y,
                Axis::Vertical => m(Pt::new(*at, 0.)).x,
            };
            Shape::Guide { axis: *axis, at: p }
        }
        Shape::Overlay { rect, image, opacity } => Shape::Overlay { rect: mr(*rect), image: image.clone(), opacity: *opacity },
    }
}

pub fn translate(shape: &Shape, dx: f32, dy: f32) -> Shape {
    let from = RectF::new(0., 0., 1., 1.);
    map_shape(shape, from, RectF::new(dx, dy, 1., 1.))
}

/// Uniform scale about the canvas origin, for resizing the whole document.
pub fn scale(shape: &Shape, k: f32) -> Shape {
    map_shape(shape, RectF::new(0., 0., 1., 1.), RectF::new(0., 0., k, k))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    /// A point on the box: grid (0..=2, 0..=2), centre excluded.
    Box(u8, u8),
    From,
    To,
    /// The arrow's bend handle, drawn on the curve's midpoint.
    Bend,
}

pub const BOX: [(u8, u8); 8] = [(0, 0), (1, 0), (2, 0), (2, 1), (2, 2), (1, 2), (0, 2), (0, 1)];

/// The editable frame of a box-like shape, before stroke width.
pub fn frame(shape: &Shape) -> Option<RectF> {
    match shape {
        Shape::Rect { rect } | Shape::Oval { rect } | Shape::Spotlight { rect, .. } | Shape::Redact { rect, .. } => {
            Some(*rect)
        }
        Shape::Overlay { rect, .. } => Some(*rect),
        Shape::Magnifier { callout, .. } => Some(*callout),
        Shape::Freehand { points } | Shape::Highlighter { points } => points_bounds(points),
        _ => None,
    }
}

pub fn points_bounds(points: &[Pt]) -> Option<RectF> {
    let first = points.first()?;
    let (mut x0, mut y0, mut x1, mut y1) = (first.x, first.y, first.x, first.y);
    for p in points {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    Some(RectF::new(x0, y0, x1 - x0, y1 - y0))
}

fn box_point(r: RectF, (gx, gy): (u8, u8)) -> Pt {
    Pt::new(r.x + r.w * gx as f32 / 2., r.y + r.h * gy as f32 / 2.)
}

/// The quadratic control point an arrow is drawn with. Curved arrows bow to
/// the left of their direction by a fifth of their length.
pub fn arrow_control(from: Pt, to: Pt, control: Option<Pt>, kind: ArrowKind) -> Option<Pt> {
    match kind {
        ArrowKind::Straight => None,
        ArrowKind::Bendable => control.or(Some(Pt::new((from.x + to.x) / 2., (from.y + to.y) / 2.))),
        ArrowKind::Curved => control.or_else(|| {
            let (dx, dy) = (to.x - from.x, to.y - from.y);
            Some(Pt::new((from.x + to.x) / 2. + dy * 0.2, (from.y + to.y) / 2. - dx * 0.2))
        }),
    }
}

pub fn quad_mid(a: Pt, c: Pt, b: Pt) -> Pt {
    Pt::new(0.25 * a.x + 0.5 * c.x + 0.25 * b.x, 0.25 * a.y + 0.5 * c.y + 0.25 * b.y)
}

/// The control point that puts the curve's midpoint at `m`.
pub fn control_through(a: Pt, m: Pt, b: Pt) -> Pt {
    Pt::new(2. * m.x - (a.x + b.x) / 2., 2. * m.y - (a.y + b.y) / 2.)
}

pub fn handles(shape: &Shape) -> Vec<(Handle, Pt)> {
    match shape {
        Shape::Arrow { from, to, control, kind } => {
            let c = arrow_control(*from, *to, *control, *kind)
                .unwrap_or(Pt::new((from.x + to.x) / 2., (from.y + to.y) / 2.));
            vec![(Handle::From, *from), (Handle::To, *to), (Handle::Bend, quad_mid(*from, c, *to))]
        }
        Shape::Line { from, to } | Shape::Measure { from, to } => vec![(Handle::From, *from), (Handle::To, *to)],
        s => match frame(s) {
            Some(r) => BOX.iter().map(|g| (Handle::Box(g.0, g.1), box_point(r, *g))).collect(),
            None => Vec::new(),
        },
    }
}

/// `orig` with `handle` dragged to `p`. `constrain` snaps segments to 15°
/// and keeps a box's aspect ratio.
pub fn drag_handle(orig: &Shape, handle: Handle, p: Pt, constrain: bool) -> Shape {
    match (orig, handle) {
        (Shape::Arrow { from, to, .. }, Handle::Bend) => {
            let c = control_through(*from, p, *to);
            Shape::Arrow { from: *from, to: *to, control: Some(c), kind: ArrowKind::Bendable }
        }
        (Shape::Arrow { from, to, control, kind }, Handle::From | Handle::To) => {
            let (a, b) = if handle == Handle::From { (*to, *from) } else { (*from, *to) };
            let moved = if constrain { snap_angle(a, p) } else { p };
            let (nf, nt) = if handle == Handle::From { (moved, *to) } else { (*from, moved) };
            let control = control.map(|c| {
                let m = quad_mid(*from, c, *to);
                let shifted = Pt::new(m.x + (moved.x - b.x) / 2., m.y + (moved.y - b.y) / 2.);
                control_through(nf, shifted, nt)
            });
            Shape::Arrow { from: nf, to: nt, control, kind: *kind }
        }
        (Shape::Line { from, to }, Handle::From | Handle::To) => {
            let (nf, nt) = segment_end(*from, *to, handle, p, constrain);
            Shape::Line { from: nf, to: nt }
        }
        (Shape::Measure { from, to }, Handle::From | Handle::To) => {
            let (nf, nt) = segment_end(*from, *to, handle, p, constrain);
            Shape::Measure { from: nf, to: nt }
        }
        (s, Handle::Box(gx, gy)) => match frame(s) {
            Some(r) => with_frame(s, r, resize_box(r, (gx, gy), p, constrain)),
            None => s.clone(),
        },
        (s, _) => s.clone(),
    }
}

/// `shape` with its editable frame `from` replaced by `to`.
fn with_frame(shape: &Shape, from: RectF, to: RectF) -> Shape {
    match shape {
        Shape::Rect { .. } => Shape::Rect { rect: to },
        Shape::Oval { .. } => Shape::Oval { rect: to },
        Shape::Spotlight { oval, .. } => Shape::Spotlight { rect: to, oval: *oval },
        Shape::Redact { mode, block, .. } => Shape::Redact { rect: to, mode: *mode, block: *block },
        Shape::Overlay { image, opacity, .. } => Shape::Overlay { rect: to, image: image.clone(), opacity: *opacity },
        Shape::Magnifier { source, .. } => Shape::Magnifier { source: *source, callout: to },
        s => map_shape(s, from, to),
    }
}

fn segment_end(from: Pt, to: Pt, handle: Handle, p: Pt, constrain: bool) -> (Pt, Pt) {
    if handle == Handle::From {
        (if constrain { snap_angle(to, p) } else { p }, to)
    } else {
        (from, if constrain { snap_angle(from, p) } else { p })
    }
}

/// `r` with the handle at grid `g` moved to `p`; the opposite side stays put.
pub fn resize_box(r: RectF, (gx, gy): (u8, u8), p: Pt, keep_aspect: bool) -> RectF {
    let (mut x0, mut y0, mut x1, mut y1) = (r.x, r.y, r.x + r.w, r.y + r.h);
    match gx {
        0 => x0 = p.x,
        2 => x1 = p.x,
        _ => {}
    }
    match gy {
        0 => y0 = p.y,
        2 => y1 = p.y,
        _ => {}
    }
    if keep_aspect && gx != 1 && gy != 1 && r.w > 0. && r.h > 0. {
        let aspect = r.w / r.h;
        let (w, h) = ((x1 - x0).abs(), (y1 - y0).abs());
        let (w, h) = if w / aspect > h { (w, w / aspect) } else { (h * aspect, h) };
        if gx == 0 {
            x0 = x1 - w;
        } else {
            x1 = x0 + w;
        }
        if gy == 0 {
            y0 = y1 - h;
        } else {
            y1 = y0 + h;
        }
    }
    RectF::from_points(Pt::new(x0, y0), Pt::new(x1, y1))
}

/// Constrains a free rectangle to an aspect ratio, anchored at its origin.
pub fn with_aspect(r: RectF, aspect: Option<(u32, u32)>) -> RectF {
    match aspect {
        Some((w, h)) if r.w > 0. && r.h > 0. => {
            let a = w as f32 / h as f32;
            if r.w / r.h > a { RectF::new(r.x, r.y, r.h * a, r.h) } else { RectF::new(r.x, r.y, r.w, r.w / a) }
        }
        _ => r,
    }
}

pub fn intersect(a: RectF, b: RectF) -> Option<RectF> {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = (a.x + a.w).min(b.x + b.w);
    let y1 = (a.y + a.h).min(b.y + b.h);
    (x1 > x0 && y1 > y0).then(|| RectF::new(x0, y0, x1 - x0, y1 - y0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Pt, b: Pt) -> bool {
        (a.x - b.x).abs() < 1e-3 && (a.y - b.y).abs() < 1e-3
    }

    #[test]
    fn snap_angle_lands_on_15_degree_multiples_and_keeps_length() {
        let p = snap_angle(Pt::new(0., 0.), Pt::new(100., 8.));
        assert!(close(p, Pt::new(100.32, 0.)) || (p.y.abs() < 1e-3 && (p.x - 100.32).abs() < 0.01));
        let q = snap_angle(Pt::new(0., 0.), Pt::new(50., 52.));
        assert!((q.x - q.y).abs() < 1e-3, "45° stays 45°: {q:?}");
    }

    #[test]
    fn drag_rect_square_and_from_centre() {
        let r = drag_rect(Pt::new(10., 10.), Pt::new(40., 20.), true, false);
        assert_eq!(r, RectF::new(10., 10., 30., 30.));
        let c = drag_rect(Pt::new(50., 50.), Pt::new(60., 55.), false, true);
        assert_eq!(c, RectF::new(40., 45., 20., 10.));
        let up_left = drag_rect(Pt::new(50., 50.), Pt::new(45., 30.), true, false);
        assert_eq!(up_left, RectF::new(30., 30., 20., 20.));
    }

    #[test]
    fn box_handles_resize_from_the_opposite_side() {
        let s = Shape::Rect { rect: RectF::new(10., 10., 100., 50.) };
        let hs = handles(&s);
        assert_eq!(hs.len(), 8);
        assert!(hs.iter().any(|(h, p)| *h == Handle::Box(2, 2) && close(*p, Pt::new(110., 60.))));
        let r = drag_handle(&s, Handle::Box(2, 2), Pt::new(210., 110.), false);
        assert_eq!(r, Shape::Rect { rect: RectF::new(10., 10., 200., 100.) });
        let n = drag_handle(&s, Handle::Box(1, 0), Pt::new(999., 0.), false);
        assert_eq!(n, Shape::Rect { rect: RectF::new(10., 0., 100., 60.) }, "edge handles move one side only");
    }

    #[test]
    fn freehand_scales_with_its_box() {
        let s = Shape::Freehand { points: vec![Pt::new(0., 0.), Pt::new(10., 10.)] };
        let r = drag_handle(&s, Handle::Box(2, 2), Pt::new(20., 30.), false);
        assert_eq!(r, Shape::Freehand { points: vec![Pt::new(0., 0.), Pt::new(20., 30.)] });
    }

    #[test]
    fn bend_handle_puts_the_curve_midpoint_under_the_cursor() {
        let s = Shape::Arrow { from: Pt::new(0., 0.), to: Pt::new(100., 0.), control: None, kind: ArrowKind::Straight };
        let bent = drag_handle(&s, Handle::Bend, Pt::new(50., 40.), false);
        let Shape::Arrow { from, to, control: Some(c), kind } = bent else { panic!("bent arrow has a control") };
        assert_eq!(kind, ArrowKind::Bendable);
        assert!(close(quad_mid(from, c, to), Pt::new(50., 40.)));
        let bend = handles(&Shape::Arrow { from, to, control: Some(c), kind }).into_iter().find(|h| h.0 == Handle::Bend);
        assert!(close(bend.unwrap().1, Pt::new(50., 40.)));
    }

    #[test]
    fn translate_moves_every_point() {
        let s = Shape::Magnifier { source: RectF::new(0., 0., 10., 10.), callout: RectF::new(20., 20., 40., 40.) };
        assert_eq!(
            translate(&s, 5., -5.),
            Shape::Magnifier { source: RectF::new(5., -5., 10., 10.), callout: RectF::new(25., 15., 40., 40.) }
        );
        let g = Shape::Guide { axis: Axis::Vertical, at: 30. };
        assert_eq!(translate(&g, 5., 100.), Shape::Guide { axis: Axis::Vertical, at: 35. });
    }

    #[test]
    fn aspect_constraint_trims_the_long_side() {
        assert_eq!(with_aspect(RectF::new(0., 0., 400., 100.), Some((1, 1))), RectF::new(0., 0., 100., 100.));
        assert_eq!(with_aspect(RectF::new(0., 0., 160., 400.), Some((16, 9))), RectF::new(0., 0., 160., 90.));
    }
}
