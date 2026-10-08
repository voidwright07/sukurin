//! Rasterises a scene with tiny-skia and frames it with the backdrop.

use shattr_core::{Bitmap, Rgba};
use tiny_skia::{
    Color, FillRule, FilterQuality, GradientStop, IntSize, LinearGradient, Mask, Paint, Pattern, Pixmap, PixmapPaint,
    Point, SpreadMode, Transform,
};

use crate::geom;
use crate::{Backdrop, Prim, RectF};

fn color(c: Rgba) -> Color {
    Color::from_rgba8(c.r, c.g, c.b, c.a)
}

pub(crate) fn to_pixmap(b: &Bitmap) -> Option<Pixmap> {
    let mut data = Vec::with_capacity(b.bgra().len());
    for p in b.bgra().as_chunks::<4>().0 {
        let a = p[3];
        if a == 255 {
            data.extend_from_slice(&[p[2], p[1], p[0], 255]);
        } else {
            let m = |v: u8| ((v as u32 * a as u32 + 127) / 255) as u8;
            data.extend_from_slice(&[m(p[2]), m(p[1]), m(p[0]), a]);
        }
    }
    Pixmap::from_vec(data, IntSize::from_wh(b.width(), b.height())?)
}

pub(crate) fn to_bitmap(pm: &Pixmap) -> Bitmap {
    let mut out = Vec::with_capacity(pm.data().len());
    for p in pm.data().as_chunks::<4>().0 {
        let a = p[3];
        match a {
            0 => out.extend_from_slice(&[0, 0, 0, 0]),
            255 => out.extend_from_slice(&[p[2], p[1], p[0], 255]),
            _ => {
                let d = |v: u8| ((v as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8;
                out.extend_from_slice(&[d(p[2]), d(p[1]), d(p[0]), a]);
            }
        }
    }
    Bitmap::from_bgra(pm.width(), pm.height(), out)
}

/// Draws `prims` onto `pm`, whose top-left sits at canvas point (`ox`, `oy`).
pub(crate) fn draw(pm: &mut Pixmap, prims: &[Prim], ox: f32, oy: f32) {
    let ts = Transform::from_translate(-ox, -oy);
    for prim in prims {
        match prim {
            Prim::Fill { polygons, color: c } => {
                let Some(path) = geom::polys_path(polygons) else { continue };
                let mut paint = Paint::default();
                paint.set_color(color(*c));
                paint.anti_alias = true;
                pm.fill_path(&path, &paint, FillRule::Winding, ts, None);
            }
            Prim::Image { dest, bitmap, opacity, nearest } => draw_image(pm, *dest, bitmap, *opacity, *nearest, ox, oy),
        }
    }
}

fn draw_image(pm: &mut Pixmap, dest: RectF, bitmap: &Bitmap, opacity: f32, nearest: bool, ox: f32, oy: f32) {
    if opacity <= 0.0 || bitmap.width() == 0 || bitmap.height() == 0 {
        return;
    }
    let Some(src) = to_pixmap(bitmap) else { return };
    let (sx, sy) = (dest.w / bitmap.width() as f32, dest.h / bitmap.height() as f32);
    let (dx, dy) = (dest.x - ox, dest.y - oy);
    let aligned = sx == 1.0 && sy == 1.0 && dx.fract() == 0.0 && dy.fract() == 0.0;
    if aligned {
        let paint = PixmapPaint { opacity: opacity.min(1.0), ..PixmapPaint::default() };
        pm.draw_pixmap(dx as i32, dy as i32, src.as_ref(), &paint, Transform::identity(), None);
        return;
    }
    let quality = if nearest { FilterQuality::Nearest } else { FilterQuality::Bilinear };
    let shader = Pattern::new(src.as_ref(), SpreadMode::Pad, quality, opacity.min(1.0), Transform::from_row(sx, 0.0, 0.0, sy, dx, dy));
    let paint = Paint { shader, anti_alias: true, ..Paint::default() };
    if let Some(r) = RectF::new(dx, dy, dest.w, dest.h).to_skia() {
        pm.fill_rect(r, &paint, Transform::identity(), None);
    }
}

/// Places `content` on the backdrop: gradient, padding, aspect, a soft
/// shadow, and rounded corners.
pub(crate) fn frame(content: &Pixmap, b: &Backdrop) -> Option<Pixmap> {
    let (cw, ch) = (content.width(), content.height());
    let pad = b.padding.max(0.0).round() as u32;
    let (mut w, mut h) = (cw + 2 * pad, ch + 2 * pad);
    if let Some((aw, ah)) = b.aspect.filter(|(a, b)| *a > 0 && *b > 0) {
        let want = aw as f64 / ah as f64;
        if (w as f64 / h as f64) < want {
            w = (h as f64 * want).round() as u32;
        } else {
            h = (w as f64 / want).round() as u32;
        }
    }
    let (ox, oy) = ((w - cw) / 2, (h - ch) / 2);
    let mut out = Pixmap::new(w, h)?;
    let full = tiny_skia::Rect::from_xywh(0.0, 0.0, w as f32, h as f32)?;
    let shader = LinearGradient::new(
        Point::from_xy(0.0, 0.0),
        Point::from_xy(w as f32, h as f32),
        vec![GradientStop::new(0.0, color(b.from)), GradientStop::new(1.0, color(b.to))],
        SpreadMode::Pad,
        Transform::identity(),
    )
    .unwrap_or(tiny_skia::Shader::SolidColor(color(b.from)));
    out.fill_rect(full, &Paint { shader, ..Paint::default() }, Transform::identity(), None);

    let content_rect = RectF::new(ox as f32, oy as f32, cw as f32, ch as f32);
    let radius = b.radius.max(0.0);
    if b.shadow > 0.0 {
        let blur = b.shadow;
        let shifted = RectF::new(content_rect.x, content_rect.y + blur * 0.35, content_rect.w, content_rect.h);
        let mut mask = Mask::new(w, h)?;
        if let Some(p) = geom::rounded_rect_path(shifted, radius) {
            mask.fill_path(&p, FillRule::Winding, true, Transform::identity());
        }
        let blurred = blur_mask(mask.data(), w as usize, h as usize, (blur / 2.0).round().max(1.0) as usize);
        let mask = Mask::from_vec(blurred, IntSize::from_wh(w, h)?)?;
        let mut paint = Paint::default();
        paint.set_color(Color::from_rgba8(0, 0, 0, 110));
        out.fill_rect(full, &paint, Transform::identity(), Some(&mask));
    }

    let mut clip = Mask::new(w, h)?;
    if let Some(p) = geom::rounded_rect_path(content_rect, radius) {
        clip.fill_path(&p, FillRule::Winding, true, Transform::identity());
    }
    out.draw_pixmap(ox as i32, oy as i32, content.as_ref(), &PixmapPaint::default(), Transform::identity(), Some(&clip));
    Some(out)
}

/// Three box passes per axis over a coverage mask.
fn blur_mask(src: &[u8], w: usize, h: usize, r: usize) -> Vec<u8> {
    let mut a: Vec<u32> = src.iter().map(|&v| v as u32).collect();
    let mut tmp = vec![0u32; w.max(h)];
    for _ in 0..3 {
        for y in 0..h {
            box_pass(&mut a[y * w..(y + 1) * w], 1, w, r, &mut tmp);
        }
        for x in 0..w {
            box_pass(&mut a[x..], w, h, r, &mut tmp);
        }
    }
    a.into_iter().map(|v| v.min(255) as u8).collect()
}

fn box_pass(data: &mut [u32], stride: usize, n: usize, r: usize, tmp: &mut [u32]) {
    let ri = r as isize;
    let get = |data: &[u32], i: isize| data[i.clamp(0, n as isize - 1) as usize * stride];
    let mut acc: u32 = (-ri..=ri).map(|i| get(data, i)).sum();
    let div = (2 * r + 1) as u32;
    for i in 0..n as isize {
        tmp[i as usize] = (acc + div / 2) / div;
        acc = acc + get(data, i + ri + 1) - get(data, i - ri);
    }
    for i in 0..n {
        data[i * stride] = tmp[i];
    }
}
