//! Each annotation reduced to prims.

use sukurin_core::Rgba;
use tiny_skia::{LineCap, LineJoin, PathBuilder, StrokeDash};

use crate::geom::{self, Pen, Poly};
use crate::{ArrowKind, Axis, Dash, Document, Fill, Prim, Pt, RectF, Scene, Shape, Style, TextBackground};
use crate::{Annotation, pixels, sketch, text};

pub(crate) fn prims(doc: &Document, a: &Annotation) -> Scene {
    let s = &a.style;
    let id = a.id.0;
    let mut out = match &a.shape {
        Shape::Arrow { from, to, control, kind } => arrow(id, *from, *to, *control, *kind, s),
        Shape::Line { from, to } => line(id, *from, *to, s),
        Shape::Rect { rect } => rect_shape(id, rect.normalized(), s),
        Shape::Oval { rect } => oval(id, rect.normalized(), s),
        Shape::Freehand { points } => freehand(points, s),
        Shape::Highlighter { points } => highlighter(points, s),
        Shape::Text { origin, text } => text_shape(*origin, text, s),
        Shape::Step { center, number } => step(*center, *number, s),
        Shape::Spotlight { rect, oval } => spotlight(doc, rect.normalized(), *oval, s),
        Shape::Redact { rect, mode, block } => pixels::redact(doc, rect.normalized(), *mode, *block).into_iter().collect(),
        Shape::Magnifier { source, callout } => magnifier(doc, source.normalized(), callout.normalized(), s),
        Shape::Measure { from, to } => measure(*from, *to, doc.scale, s),
        Shape::Guide { axis, at } => guide(doc, *axis, *at, s),
        Shape::Overlay { rect, image, opacity } => vec![Prim::Image {
            dest: rect.normalized(),
            bitmap: image.clone(),
            opacity: opacity.clamp(0.0, 1.0),
            nearest: false,
        }],
    };
    let o = s.opacity.clamp(0.0, 1.0);
    if o < 1.0 {
        for p in &mut out {
            match p {
                Prim::Fill { color, .. } => *color = alpha(*color, o),
                Prim::Image { opacity, .. } => *opacity *= o,
            }
        }
    }
    out
}

pub(crate) fn alpha(c: Rgba, k: f32) -> Rgba {
    Rgba { a: (c.a as f32 * k).round().clamp(0.0, 255.0) as u8, ..c }
}

/// White on saturated and dark colours, near-black on light ones.
fn ink_on(c: Rgba) -> Rgba {
    let lin = |v: u8| {
        let v = v as f32 / 255.0;
        if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    let lum = 0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b);
    if lum > 0.42 { Rgba::opaque(0x16, 0x17, 0x1A) } else { Rgba::opaque(0xFF, 0xFF, 0xFF) }
}

fn fill_prim(polygons: Vec<Poly>, color: Rgba) -> Option<Prim> {
    (!polygons.is_empty()).then_some(Prim::Fill { polygons, color })
}

fn dash(s: &Style) -> Option<StrokeDash> {
    let w = s.width.max(0.5);
    match s.dash {
        Dash::Solid => None,
        Dash::Dashed => StrokeDash::new(vec![w * 2.5 + 3.0, w * 1.75 + 3.0], 0.0),
        Dash::Dotted => StrokeDash::new(vec![0.01, w * 2.0 + 1.0], 0.0),
    }
}

/// Pen for open strokes: round ends, except butt-ended dashes.
fn open_pen(s: &Style, width: f32) -> Pen {
    let cap = if s.dash == Dash::Dashed { LineCap::Butt } else { LineCap::Round };
    Pen { width, cap, join: LineJoin::Round }
}

/// Pen for closed outlines: crisp mitred corners, round dots.
fn closed_pen(s: &Style, width: f32) -> Pen {
    let cap = if s.dash == Dash::Dotted { LineCap::Round } else { LineCap::Butt };
    Pen { width, cap, join: LineJoin::Miter }
}

fn polyline_stroke(pts: &[Pt], pen: Pen, dash: Option<StrokeDash>) -> Vec<Poly> {
    geom::polyline_path(pts, false).map(|p| geom::stroke(&p, pen, dash)).unwrap_or_default()
}

fn hand_amp(s: &Style) -> f32 {
    (s.width * 0.4).max(1.5)
}

fn arc_length(pts: &[Pt]) -> f32 {
    pts.windows(2).map(|w| w[0].dist(w[1])).sum()
}

/// `pts` with `d` pixels of arc length removed from the end; returns the
/// shortened polyline (its last point is the cut).
fn trim_end(pts: &[Pt], d: f32) -> Vec<Pt> {
    let mut left = d;
    let mut out = pts.to_vec();
    while out.len() >= 2 {
        let (a, b) = (out[out.len() - 2], out[out.len() - 1]);
        let l = a.dist(b);
        if l > left {
            *out.last_mut().unwrap() = b.lerp(a, left / l);
            return out;
        }
        left -= l;
        out.pop();
    }
    out
}

fn arrow(id: u64, from: Pt, to: Pt, control: Option<Pt>, kind: ArrowKind, s: &Style) -> Scene {
    let w = s.width.max(0.5);
    let ctrl = match kind {
        ArrowKind::Straight => None,
        ArrowKind::Curved => Some(from.lerp(to, 0.5) + (to - from).perp() * 0.2),
        ArrowKind::Bendable => control,
    };
    let center: Vec<Pt> = match ctrl {
        Some(c) => {
            let n = ((from.dist(c) + c.dist(to)) / 4.0).clamp(8.0, 200.0) as usize;
            (0..=n)
                .map(|i| {
                    let t = i as f32 / n as f32;
                    from.lerp(c, t).lerp(c.lerp(to, t), t)
                })
                .collect()
        }
        None => vec![from, to],
    };
    let len = arc_length(&center);
    if len < 1.0 {
        return Vec::new();
    }
    let head_len = (4.5 * w + 6.0).min(len / 1.4);
    let half = head_len * 0.46;
    let base = *trim_end(&center, head_len).last().unwrap_or(&from);
    let dir = (to - base).norm();
    let n = dir.perp();
    let mut head = [to, to - dir * head_len + n * half, to - dir * (head_len * 0.8), to - dir * head_len - n * half];
    let shaft = trim_end(&center, head_len * 0.55);
    let pen = open_pen(s, w);
    let mut polys = if s.hand_drawn {
        let amp = hand_amp(s);
        for (i, p) in head.iter_mut().enumerate().skip(1) {
            *p = *p + Pt::new(sketch::unit(sketch::seed(id, 40), i as u64), sketch::unit(sketch::seed(id, 41), i as u64)) * (amp * 0.5);
        }
        let mut v = polyline_stroke(&sketch::wobble(&shaft, sketch::seed(id, 1), amp, true), pen, dash(s));
        let second = Pen { width: w * 0.7, ..pen };
        v.extend(polyline_stroke(&sketch::wobble(&shaft, sketch::seed(id, 2), amp * 0.9, true), second, dash(s)));
        v
    } else {
        polyline_stroke(&shaft, pen, dash(s))
    };
    polys.extend(geom::orient(vec![head.to_vec()], true));
    fill_prim(polys, s.color).into_iter().collect()
}

fn line(id: u64, from: Pt, to: Pt, s: &Style) -> Scene {
    let w = s.width.max(0.5);
    let polys = if s.hand_drawn {
        let amp = hand_amp(s);
        let mut v = polyline_stroke(&sketch::line(from, to, sketch::seed(id, 1), amp), open_pen(s, w), dash(s));
        v.extend(polyline_stroke(&sketch::line(from, to, sketch::seed(id, 2), amp), open_pen(s, w * 0.6), dash(s)));
        v
    } else {
        polyline_stroke(&[from, to], open_pen(s, w), dash(s))
    };
    fill_prim(polys, s.color).into_iter().collect()
}

/// Outline plus optional fill. A tint stops at the stroke's inner edge so a
/// translucent mark never stacks over itself; a solid fill absorbs the
/// stroke into one shape.
fn outlined(outline: Vec<Poly>, inner: Vec<Poly>, outer: Vec<Poly>, s: &Style) -> Scene {
    match s.fill {
        Fill::None => fill_prim(outline, s.color).into_iter().collect(),
        Fill::Tint => {
            fill_prim(inner, alpha(s.color, 0.18)).into_iter().chain(fill_prim(outline, s.color)).collect()
        }
        Fill::Solid => fill_prim(outer, s.color).into_iter().collect(),
    }
}

fn rect_shape(id: u64, r: RectF, s: &Style) -> Scene {
    let w = s.width.max(0.5);
    if s.hand_drawn {
        return sketched_closed(id, s, |seed, amp| {
            let j = |k: u64| Pt::new(sketch::unit(seed, k), sketch::unit(seed, k + 100)) * (amp * 0.8);
            let c = [
                Pt::new(r.x, r.y) + j(1),
                Pt::new(r.right(), r.y) + j(2),
                Pt::new(r.right(), r.bottom()) + j(3),
                Pt::new(r.x, r.bottom()) + j(4),
            ];
            let sides = (0..4).map(|i| sketch::line(c[i], c[(i + 1) % 4], seed ^ (i as u64 + 7), amp)).collect();
            (sides, c.to_vec())
        });
    }
    let path = |rr: RectF, rad: f32| geom::rounded_rect_path(rr, rad);
    let rad = s.corner_radius.max(0.0);
    let outline = path(r, rad).map(|p| geom::stroke(&p, closed_pen(s, w), dash(s))).unwrap_or_default();
    let inner_r = r.inflate(-w / 2.0);
    let inner = if inner_r.w > 0.0 && inner_r.h > 0.0 {
        path(inner_r, (rad - w / 2.0).max(0.0)).map(|p| geom::fill(&p)).unwrap_or_default()
    } else {
        Vec::new()
    };
    let outer_rad = if rad > 0.0 { rad + w / 2.0 } else { 0.0 };
    let outer = path(r.inflate(w / 2.0), outer_rad).map(|p| geom::fill(&p)).unwrap_or_default();
    outlined(outline, inner, outer, s)
}

fn oval(id: u64, r: RectF, s: &Style) -> Scene {
    let w = s.width.max(0.5);
    if s.hand_drawn {
        return sketched_closed(id, s, |seed, amp| {
            let c = r.center();
            let rx = r.w / 2.0 + amp * sketch::unit(seed, 1);
            let ry = r.h / 2.0 + amp * sketch::unit(seed, 2);
            let start = sketch::unit(seed, 3) * std::f32::consts::PI;
            let sweep = std::f32::consts::TAU + 0.4 + 0.15 * sketch::unit(seed, 4);
            let n = (((rx + ry) * 3.2 / 3.0) as usize).clamp(24, 400);
            let ring: Vec<Pt> = (0..=n)
                .map(|i| {
                    let t = i as f32 / n as f32;
                    let a = start + sweep * t;
                    let grow = 1.0 + 0.05 * t * sketch::unit(seed, 5).signum();
                    Pt::new(c.x + rx * grow * a.cos(), c.y + ry * grow * a.sin())
                })
                .collect();
            let body = geom::ellipse_poly(RectF::new(c.x - rx, c.y - ry, 2.0 * rx, 2.0 * ry));
            (vec![sketch::wobble(&ring, seed ^ 0x55, amp * 0.6, false)], body)
        });
    }
    let path = |rr: RectF| geom::oval_path(rr);
    let outline = path(r).map(|p| geom::stroke(&p, closed_pen(s, w), dash(s))).unwrap_or_default();
    let inner = path(r.inflate(-w / 2.0)).map(|p| geom::fill(&p)).unwrap_or_default();
    let outer = path(r.inflate(w / 2.0)).map(|p| geom::fill(&p)).unwrap_or_default();
    outlined(outline, inner, outer, s)
}

/// Two marker passes over a closed shape. `pass` returns the stroke
/// polylines and a fill polygon for one seed.
fn sketched_closed(id: u64, s: &Style, pass: impl Fn(u64, f32) -> (Vec<Vec<Pt>>, Vec<Pt>)) -> Scene {
    let w = s.width.max(0.5);
    let amp = hand_amp(s);
    let (first, body) = pass(sketch::seed(id, 1), amp);
    let (second, _) = pass(sketch::seed(id, 2), amp);
    let mut outline = Vec::new();
    for p in &first {
        outline.extend(polyline_stroke(p, open_pen(s, w), dash(s)));
    }
    for p in &second {
        outline.extend(polyline_stroke(p, open_pen(s, w * 0.6), dash(s)));
    }
    let body = geom::orient(vec![body], true);
    match s.fill {
        Fill::None => fill_prim(outline, s.color).into_iter().collect(),
        Fill::Tint => fill_prim(body, alpha(s.color, 0.18)).into_iter().chain(fill_prim(outline, s.color)).collect(),
        Fill::Solid => {
            outline.extend(body);
            fill_prim(outline, s.color).into_iter().collect()
        }
    }
}

/// Catmull-Rom through the input points, after dropping sub-pixel jitter.
fn smooth_path(points: &[Pt]) -> Option<tiny_skia::Path> {
    let mut pts: Vec<Pt> = Vec::with_capacity(points.len());
    for &p in points {
        if pts.last().is_none_or(|q: &Pt| q.dist(p) >= 1.5) {
            pts.push(p);
        }
    }
    if let (Some(&last_in), Some(&last_kept)) = (points.last(), pts.last())
        && last_in != last_kept
    {
        pts.push(last_in);
    }
    if pts.len() < 3 {
        return geom::polyline_path(&pts, false);
    }
    let mut pb = PathBuilder::with_capacity(pts.len() + 1, pts.len() * 3);
    pb.move_to(pts[0].x, pts[0].y);
    for i in 0..pts.len() - 1 {
        let p0 = pts[i.saturating_sub(1)];
        let (p1, p2) = (pts[i], pts[i + 1]);
        let p3 = pts[(i + 2).min(pts.len() - 1)];
        let c1 = p1 + (p2 - p0) * (1.0 / 6.0);
        let c2 = p2 - (p3 - p1) * (1.0 / 6.0);
        pb.cubic_to(c1.x, c1.y, c2.x, c2.y, p2.x, p2.y);
    }
    pb.finish()
}

fn dot(p: Pt, d: f32) -> Vec<Poly> {
    vec![geom::ellipse_poly(RectF::new(p.x - d / 2.0, p.y - d / 2.0, d, d))]
}

fn freehand(points: &[Pt], s: &Style) -> Scene {
    let w = s.width.max(0.5);
    let polys = match points {
        [] => Vec::new(),
        [p] => dot(*p, w),
        _ => smooth_path(points).map(|p| geom::stroke(&p, Pen::round(w), dash(s))).unwrap_or_default(),
    };
    fill_prim(polys, s.color).into_iter().collect()
}

fn highlighter(points: &[Pt], s: &Style) -> Scene {
    let w = (s.width * 4.0).max(14.0);
    let pen = Pen { width: w, cap: LineCap::Butt, join: LineJoin::Round };
    let polys = match points {
        [] => Vec::new(),
        [p] => vec![vec![
            Pt::new(p.x - w / 4.0, p.y - w / 2.0),
            Pt::new(p.x + w / 4.0, p.y - w / 2.0),
            Pt::new(p.x + w / 4.0, p.y + w / 2.0),
            Pt::new(p.x - w / 4.0, p.y + w / 2.0),
        ]],
        _ => smooth_path(points).map(|p| geom::stroke(&p, pen, None)).unwrap_or_default(),
    };
    fill_prim(polys, alpha(s.color, 0.4)).into_iter().collect()
}

/// Text on a rounded label in `bg`, ink chosen for contrast.
fn label(text: &str, center: Pt, size: f32, bg: Rgba) -> (RectF, Scene) {
    let lay = text::layout(text, size, true);
    let (pad_x, pad_y) = (size * 0.55, size * 0.28);
    let box_ = RectF::new(center.x - lay.width / 2.0 - pad_x, center.y - lay.height / 2.0 - pad_y, lay.width + 2.0 * pad_x, lay.height + 2.0 * pad_y);
    let origin = Pt::new(box_.x + pad_x, box_.y + pad_y);
    let pill = geom::rounded_rect_path(box_, box_.h / 2.0).map(|p| geom::fill(&p)).unwrap_or_default();
    let glyphs = lay.path(origin).map(|p| geom::fill(&p)).unwrap_or_default();
    (box_, fill_prim(pill, bg).into_iter().chain(fill_prim(glyphs, ink_on(bg))).collect())
}

/// Padding of a text pill around the text block, as fractions of the size.
const PILL_PAD: (f32, f32) = (0.42, 0.16);

pub(crate) fn text_box(origin: Pt, text: &str, s: &Style) -> RectF {
    let lay = text::layout(text, s.font_size, s.bold);
    let r = RectF::new(origin.x, origin.y, lay.width, lay.height);
    match s.text_background {
        TextBackground::Pill => r.pad(s.font_size * PILL_PAD.0, s.font_size * PILL_PAD.1),
        TextBackground::Outline => r.inflate(2.0),
        TextBackground::None => r,
    }
}

fn text_shape(origin: Pt, text: &str, s: &Style) -> Scene {
    let lay = text::layout(text, s.font_size.max(1.0), s.bold);
    let Some(path) = lay.path(origin) else { return Vec::new() };
    let glyphs = geom::fill(&path);
    match s.text_background {
        TextBackground::None => fill_prim(glyphs, s.color).into_iter().collect(),
        TextBackground::Pill => {
            let size = s.font_size;
            let r = RectF::new(origin.x, origin.y, lay.width, lay.height).pad(size * PILL_PAD.0, size * PILL_PAD.1);
            let rad = (r.h / 2.0).min(size * 0.55);
            let pill = geom::rounded_rect_path(r, rad).map(|p| geom::fill(&p)).unwrap_or_default();
            fill_prim(pill, s.color).into_iter().chain(fill_prim(glyphs, ink_on(s.color))).collect()
        }
        TextBackground::Outline => {
            let halo = geom::stroke(&path, Pen::round(4.0), None);
            let halo_color = if ink_on(s.color).r > 128 { Rgba { a: 235, ..Rgba::opaque(0xFA, 0xFA, 0xFA) } } else { Rgba { a: 235, ..Rgba::opaque(0x10, 0x11, 0x14) } };
            fill_prim(halo, halo_color).into_iter().chain(fill_prim(glyphs, s.color)).collect()
        }
    }
}

fn step_diameter(s: &Style) -> f32 {
    (s.font_size * 1.15).max(8.0)
}

fn step(center: Pt, number: u32, s: &Style) -> Scene {
    let d = step_diameter(s);
    let r = d / 2.0;
    let ring_w = (d * 0.07).max(1.5);
    let circle = |rad: f32| PathBuilder::from_circle(center.x, center.y, rad);
    let ring = circle(r + ring_w / 2.0).map(|p| geom::stroke(&p, Pen::butt(ring_w), None)).unwrap_or_default();
    let disc = circle(r).map(|p| geom::fill(&p)).unwrap_or_default();
    let digits = number.to_string();
    let mut size = d * 0.56;
    let width = text::layout(&digits, size, true).width;
    if width > d * 0.72 {
        size *= d * 0.72 / width;
    }
    let lay = text::layout(&digits, size, true);
    let origin = Pt::new(center.x - lay.width / 2.0, center.y - text::cap_center(size));
    let glyphs = lay.path(origin).map(|p| geom::fill(&p)).unwrap_or_default();
    fill_prim(ring, Rgba::opaque(255, 255, 255))
        .into_iter()
        .chain(fill_prim(disc, s.color))
        .chain(fill_prim(glyphs, ink_on(s.color)))
        .collect()
}

pub(crate) fn spotlight_hole(rect: RectF, oval: bool, s: &Style) -> Vec<Poly> {
    if oval {
        vec![geom::ellipse_poly(rect)]
    } else {
        geom::rounded_rect_path(rect, s.corner_radius).map(|p| geom::flatten(&p)).unwrap_or_default()
    }
}

fn spotlight(doc: &Document, rect: RectF, oval: bool, s: &Style) -> Scene {
    let c = doc.canvas_bounds().union(&rect);
    let mut polys = vec![vec![Pt::new(c.x, c.y), Pt::new(c.right(), c.y), Pt::new(c.right(), c.bottom()), Pt::new(c.x, c.bottom())]];
    polys = geom::orient(polys, true);
    polys.extend(geom::orient(spotlight_hole(rect, oval, s), false));
    fill_prim(polys, Rgba { r: 0, g: 0, b: 0, a: 153 }).into_iter().collect()
}

/// Distance in logical points; an almost axis-aligned measurement reports
/// the axis distance so a slightly-off drag still reads a clean number.
pub(crate) fn measure_label(from: Pt, to: Pt, scale: f32) -> String {
    let d = to - from;
    let angle = d.y.abs().atan2(d.x.abs()).to_degrees();
    let px = if angle < 3.0 {
        d.x.abs()
    } else if angle > 87.0 {
        d.y.abs()
    } else {
        d.len()
    };
    format!("{} pt", (px / scale.max(0.01)).round() as i64)
}

fn measure(from: Pt, to: Pt, scale: f32, s: &Style) -> Scene {
    let lw = (s.width * 0.5).max(1.5);
    let dir = (to - from).norm();
    let n = dir.perp();
    let tick = 5.0 + lw * 2.0;
    let pen = Pen::butt(lw);
    let mut polys = polyline_stroke(&[from, to], pen, None);
    for p in [from, to] {
        polys.extend(polyline_stroke(&[p + n * tick, p - n * tick], pen, None));
    }
    let size = (s.font_size * 0.5).max(12.0);
    let text = measure_label(from, to, scale);
    let mid = from.lerp(to, 0.5);
    let (probe, _) = label(&text, mid, size, s.color);
    let len = from.dist(to);
    let along = (probe.w * dir.x.abs() + probe.h * dir.y.abs()) + 2.0 * tick;
    let across = probe.w * n.x.abs() + probe.h * n.y.abs();
    let side = if n.y > 0.0 { -1.0 } else { 1.0 };
    let center = if len < along { mid + n * (side * (across / 2.0 + tick + 2.0)) } else { mid };
    let (_, label_prims) = label(&text, center, size, s.color);
    fill_prim(polys, s.color).into_iter().chain(label_prims).collect()
}

fn guide(doc: &Document, axis: Axis, at: f32, s: &Style) -> Scene {
    let c = doc.canvas_bounds();
    let v = at.floor() + 0.5;
    let (a, b) = match axis {
        Axis::Horizontal => (Pt::new(c.x, v), Pt::new(c.right(), v)),
        Axis::Vertical => (Pt::new(v, c.y), Pt::new(v, c.bottom())),
    };
    let polys = polyline_stroke(&[a, b], Pen::butt(1.0), StrokeDash::new(vec![6.0, 4.0], 0.0));
    fill_prim(polys, s.color).into_iter().collect()
}

fn magnifier(doc: &Document, source: RectF, callout: RectF, s: &Style) -> Scene {
    let c = callout.center();
    let r = callout.w.min(callout.h) / 2.0;
    let mut lines = Vec::new();
    let thin = (s.width * 0.4).max(1.5);
    lines.extend(geom::rounded_rect_path(source, 2.0).map(|p| geom::stroke(&p, Pen::butt(thin), None)).unwrap_or_default());
    let sc = source.center();
    if sc.dist(c) > r + 1.0 {
        let dir = (c - sc).norm();
        let tx = if dir.x.abs() > 1e-6 { (source.w / 2.0) / dir.x.abs() } else { f32::MAX };
        let ty = if dir.y.abs() > 1e-6 { (source.h / 2.0) / dir.y.abs() } else { f32::MAX };
        let start = sc + dir * tx.min(ty);
        let end = c - dir * r;
        if start.dist(sc) < end.dist(sc) {
            lines.extend(polyline_stroke(&[start, end], Pen::round(thin), None));
        }
    }
    let ring_w = (s.width * 0.75).max(2.0);
    let ring = PathBuilder::from_circle(c.x, c.y, r).map(|p| geom::stroke(&p, Pen::butt(ring_w), None)).unwrap_or_default();
    fill_prim(lines, s.color)
        .into_iter()
        .chain(pixels::magnify(doc, source, c, r))
        .chain(fill_prim(ring, s.color))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measure_label_reports_logical_points() {
        assert_eq!(measure_label(Pt::new(0.0, 0.0), Pt::new(78.0, 0.0), 1.5), "52 pt");
        assert_eq!(measure_label(Pt::new(10.0, 10.0), Pt::new(13.0, 110.0), 2.0), "50 pt");
        assert_eq!(measure_label(Pt::new(0.0, 0.0), Pt::new(30.0, 40.0), 1.0), "50 pt");
        assert_eq!(measure_label(Pt::new(0.0, 0.0), Pt::new(100.0, 4.0), 1.0), "100 pt");
    }

    #[test]
    fn ink_contrasts_with_its_background() {
        assert_eq!(ink_on(Rgba::opaque(0xF0, 0x44, 0x3A)), Rgba::opaque(255, 255, 255));
        assert_ne!(ink_on(Rgba::opaque(0xFA, 0xCC, 0x15)), Rgba::opaque(255, 255, 255));
    }
}
