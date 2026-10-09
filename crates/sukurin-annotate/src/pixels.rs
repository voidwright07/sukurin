//! Effects that read the capture's pixels: redactions and the magnifier.

use std::collections::HashMap;

use sukurin_core::{Bitmap, RectPx};

use crate::{Document, Prim, Pt, RectF, RedactMode};

/// Straight-alpha RGBA pixels of a canvas region.
struct Img {
    w: usize,
    h: usize,
    px: Vec<[u8; 4]>,
}

impl Img {
    fn at(&self, x: usize, y: usize) -> [u8; 4] {
        self.px[y * self.w + x]
    }

    fn into_bitmap(self) -> Bitmap {
        Bitmap::from_rgba(self.w as u32, self.h as u32, self.px.into_iter().flatten().collect())
    }

    fn crop(&self, x: usize, y: usize, w: usize, h: usize) -> Img {
        let mut px = Vec::with_capacity(w * h);
        for row in y..y + h {
            px.extend_from_slice(&self.px[row * self.w + x..row * self.w + x + w]);
        }
        Img { w, h, px }
    }
}

fn canvas_rect(doc: &Document) -> RectPx {
    snap(doc.canvas_bounds())
}

fn snap(r: RectF) -> RectPx {
    let x0 = r.x.floor();
    let y0 = r.y.floor();
    let x1 = r.right().ceil();
    let y1 = r.bottom().ceil();
    RectPx::new(x0 as i32, y0 as i32, (x1 - x0).max(0.0) as u32, (y1 - y0).max(0.0) as u32)
}

/// The layers composited over `r`; uncovered pixels are transparent.
fn sample(doc: &Document, r: RectPx) -> Img {
    let (w, h) = (r.w as usize, r.h as usize);
    let mut px = vec![[0u8; 4]; w * h];
    for layer in &doc.layers {
        let b = &layer.bitmap;
        let lr = RectPx::new(layer.origin.x.round() as i32, layer.origin.y.round() as i32, b.width(), b.height());
        let Some(i) = r.intersect(&lr) else { continue };
        let src = b.bgra();
        for y in i.y..i.bottom() {
            let sy = (y - lr.y) as usize;
            let dy = (y - r.y) as usize;
            for x in i.x..i.right() {
                let s = (sy * b.width() as usize + (x - lr.x) as usize) * 4;
                let p = [src[s + 2], src[s + 1], src[s], src[s + 3]];
                if p[3] > 0 {
                    px[dy * w + (x - r.x) as usize] = p;
                }
            }
        }
    }
    Img { w, h, px }
}

pub(crate) fn redact(doc: &Document, rect: RectF, mode: RedactMode, block: f32) -> Option<Prim> {
    let canvas = canvas_rect(doc);
    let r = snap(rect).intersect(&canvas)?;
    let block = block.max(2.0);
    let img = match mode {
        RedactMode::Pixelate => pixelate(sample(doc, r), block.round() as usize),
        RedactMode::Blur => {
            let rad = block.round() as u32;
            let outer = grow(r, rad * 3).intersect(&canvas)?;
            let blurred = blur(sample(doc, outer), rad as usize);
            blurred.crop((r.x - outer.x) as usize, (r.y - outer.y) as usize, r.w as usize, r.h as usize)
        }
        RedactMode::Erase => {
            let outer = grow(r, 1).intersect(&canvas)?;
            erase(&sample(doc, outer), (r.x - outer.x) as usize, (r.y - outer.y) as usize, r.w as usize, r.h as usize)
        }
        RedactMode::EraseText => erase_text(sample(doc, r)),
    };
    Some(Prim::Image {
        dest: RectF::new(r.x as f32, r.y as f32, r.w as f32, r.h as f32),
        bitmap: img.into_bitmap(),
        opacity: 1.0,
        nearest: mode == RedactMode::Pixelate,
    })
}

fn grow(r: RectPx, d: u32) -> RectPx {
    RectPx::new(r.x - d as i32, r.y - d as i32, r.w + 2 * d, r.h + 2 * d)
}

fn pixelate(mut img: Img, block: usize) -> Img {
    for by in (0..img.h).step_by(block) {
        for bx in (0..img.w).step_by(block) {
            let (x1, y1) = ((bx + block).min(img.w), (by + block).min(img.h));
            let mut acc = [0u32; 4];
            for y in by..y1 {
                for x in bx..x1 {
                    for (a, v) in acc.iter_mut().zip(img.at(x, y)) {
                        *a += v as u32;
                    }
                }
            }
            let n = ((x1 - bx) * (y1 - by)) as u32;
            let avg = acc.map(|v| ((v + n / 2) / n) as u8);
            for y in by..y1 {
                img.px[y * img.w + bx..y * img.w + x1].fill(avg);
            }
        }
    }
    img
}

/// Three box passes per axis approximate a Gaussian with sigma near `r`.
fn blur(mut img: Img, r: usize) -> Img {
    let (w, h) = (img.w, img.h);
    let mut src = Vec::with_capacity(w.max(h));
    let mut dst = vec![[0u8; 4]; w.max(h)];
    for _ in 0..3 {
        for y in 0..h {
            box_line(&img.px[y * w..(y + 1) * w], &mut dst[..w], r);
            img.px[y * w..(y + 1) * w].copy_from_slice(&dst[..w]);
        }
        for x in 0..w {
            src.clear();
            src.extend((0..h).map(|y| img.px[y * w + x]));
            box_line(&src, &mut dst[..h], r);
            for (y, p) in dst[..h].iter().enumerate() {
                img.px[y * w + x] = *p;
            }
        }
    }
    img
}

/// Box filter of radius `r` with edge clamping; `dst` matches `src` in length.
fn box_line(src: &[[u8; 4]], dst: &mut [[u8; 4]], r: usize) {
    let n = src.len();
    if n == 0 {
        return;
    }
    let ri = r as isize;
    let get = |i: isize| src[i.clamp(0, n as isize - 1) as usize];
    let mut acc = [0u32; 4];
    for i in -ri..=ri {
        for (a, v) in acc.iter_mut().zip(get(i)) {
            *a += v as u32;
        }
    }
    let div = (2 * r + 1) as u32;
    for (i, out) in dst.iter_mut().enumerate() {
        *out = acc.map(|v| ((v + div / 2) / div) as u8);
        let (add, sub) = (get(i as isize + ri + 1), get(i as isize - ri));
        for c in 0..4 {
            acc[c] = acc[c] + add[c] as u32 - sub[c] as u32;
        }
    }
}

fn lerp4(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    [0, 1, 2, 3].map(|c| a[c] + (b[c] - a[c]) * t)
}

/// Rebuilds `w`x`h` at (`x`,`y`) inside `img` from the ring of pixels
/// around it: each pixel averages a left-right and a top-bottom
/// interpolation of the (lightly smoothed) border.
fn erase(img: &Img, x: usize, y: usize, w: usize, h: usize) -> Img {
    let f = |p: [u8; 4]| p.map(|v| v as f32);
    let lx = x.saturating_sub(1);
    let rx = (x + w).min(img.w - 1);
    let ty = y.saturating_sub(1);
    let by = (y + h).min(img.h - 1);
    let smooth = |v: Vec<[f32; 4]>| -> Vec<[f32; 4]> {
        (0..v.len())
            .map(|i| {
                let (a, b) = (i.saturating_sub(2), (i + 3).min(v.len()));
                let mut s = [0.0; 4];
                for p in &v[a..b] {
                    for (acc, x) in s.iter_mut().zip(p) {
                        *acc += x;
                    }
                }
                s.map(|t| t / (b - a) as f32)
            })
            .collect()
    };
    let left = smooth((y..y + h).map(|yy| f(img.at(lx, yy))).collect());
    let right = smooth((y..y + h).map(|yy| f(img.at(rx, yy))).collect());
    let top = smooth((x..x + w).map(|xx| f(img.at(xx, ty))).collect());
    let bottom = smooth((x..x + w).map(|xx| f(img.at(xx, by))).collect());
    let span_x = (rx as f32 - lx as f32).max(1.0);
    let span_y = (by as f32 - ty as f32).max(1.0);
    let mut px = Vec::with_capacity(w * h);
    for j in 0..h {
        let ty_t = ((y + j) as f32 - ty as f32) / span_y;
        for i in 0..w {
            let tx = ((x + i) as f32 - lx as f32) / span_x;
            let row = lerp4(left[j], right[j], tx);
            let col = lerp4(top[i], bottom[i], ty_t);
            px.push([0, 1, 2, 3].map(|c| ((row[c] + col[c]) * 0.5).round().clamp(0.0, 255.0) as u8));
        }
    }
    Img { w, h, px }
}

fn diff(a: [u8; 4], b: [u8; 4]) -> u8 {
    a[0].abs_diff(b[0]).max(a[1].abs_diff(b[1])).max(a[2].abs_diff(b[2]))
}

const CELL: usize = 12;
/// Cells on each side of a cell that vote on its background.
const WINDOW: usize = 2;

/// Local background per cell, voted over the surrounding window: the
/// dominant colour bin, then the exact modal colour of that bin when it
/// clearly leads (flat fills come back exact), else the bin's mean (so
/// gradients come back smooth).
fn background_cells(img: &Img) -> (usize, Vec<[u8; 4]>) {
    let cw = img.w.div_ceil(CELL);
    let ch = img.h.div_ceil(CELL);
    let mut hist: Vec<HashMap<[u8; 4], u32>> = vec![HashMap::new(); cw * ch];
    for y in 0..img.h {
        for x in 0..img.w {
            *hist[(y / CELL) * cw + x / CELL].entry(img.at(x, y)).or_insert(0) += 1;
        }
    }
    let bin = |p: [u8; 4]| ((p[0] as u16 >> 4) << 8) | ((p[1] as u16 >> 4) << 4) | (p[2] as u16 >> 4);
    let mut out = Vec::with_capacity(cw * ch);
    let mut merged: HashMap<[u8; 4], u32> = HashMap::new();
    let mut bins: HashMap<u16, (u32, [u64; 4], [u8; 4], u32)> = HashMap::new();
    for cy in 0..ch {
        for cx in 0..cw {
            merged.clear();
            for ny in cy.saturating_sub(WINDOW)..(cy + WINDOW + 1).min(ch) {
                for nx in cx.saturating_sub(WINDOW)..(cx + WINDOW + 1).min(cw) {
                    for (p, n) in &hist[ny * cw + nx] {
                        *merged.entry(*p).or_insert(0) += n;
                    }
                }
            }
            bins.clear();
            for (&p, &n) in &merged {
                let e = bins.entry(bin(p)).or_insert((0, [0; 4], p, 0));
                e.0 += n;
                for (sum, v) in e.1.iter_mut().zip(p) {
                    *sum += v as u64 * n as u64;
                }
                if n > e.3 || (n == e.3 && p > e.2) {
                    (e.2, e.3) = (p, n);
                }
            }
            let (_, &(n, sum, mode, mode_n)) = bins.iter().max_by_key(|(k, (n, ..))| (*n, **k)).expect("cell has pixels");
            let n64 = n as u64;
            out.push(if mode_n * 10 >= n * 3 { mode } else { sum.map(|v| ((v + n64 / 2) / n64) as u8) });
        }
    }
    (cw, out)
}

/// Replaces glyph-like marks with the local background. Marks are connected
/// groups of pixels that differ strongly from the background; a group is
/// kept when it looks like UI rather than text: long and thin (borders,
/// dividers), large, or a dense solid block.
fn erase_text(mut img: Img) -> Img {
    let (w, h) = (img.w, img.h);
    let (cw, cells) = background_cells(&img);
    let bg = |x: usize, y: usize| cells[(y / CELL) * cw + x / CELL];
    let strong: Vec<bool> = (0..w * h).map(|i| diff(img.px[i], bg(i % w, i / w)) > 48).collect();

    let mut remove = vec![false; w * h];
    let mut seen = vec![false; w * h];
    let mut stack = Vec::new();
    let mut comp = Vec::new();
    for start in 0..w * h {
        if !strong[start] || seen[start] {
            continue;
        }
        comp.clear();
        stack.push(start);
        seen[start] = true;
        let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
        while let Some(i) = stack.pop() {
            comp.push(i);
            let (x, y) = (i % w, i / w);
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
            for ny in y.saturating_sub(1)..(y + 2).min(h) {
                for nx in x.saturating_sub(1)..(x + 2).min(w) {
                    let j = ny * w + nx;
                    if strong[j] && !seen[j] {
                        seen[j] = true;
                        stack.push(j);
                    }
                }
            }
        }
        let (cw_, ch_) = (x1 - x0 + 1, y1 - y0 + 1);
        let long = cw_.max(ch_);
        let short = cw_.min(ch_);
        let density = comp.len() as f32 / (cw_ * ch_) as f32;
        let line_like = short <= 3 && long >= 20;
        let large = long > 72;
        let solid = short >= 14 && density > 0.72;
        if !(line_like || large || solid) {
            for &i in &comp {
                remove[i] = true;
            }
        }
    }

    for _ in 0..2 {
        let grown: Vec<bool> = (0..w * h)
            .map(|i| {
                if remove[i] {
                    return true;
                }
                let (x, y) = (i % w, i / w);
                if diff(img.px[i], bg(x, y)) <= 4 {
                    return false;
                }
                (y.saturating_sub(1)..(y + 2).min(h))
                    .any(|ny| (x.saturating_sub(1)..(x + 2).min(w)).any(|nx| remove[ny * w + nx]))
            })
            .collect();
        remove = grown;
    }

    for (i, r) in remove.iter().enumerate() {
        if *r {
            img.px[i] = bg(i % w, i / w);
        }
    }
    img
}

/// The source region enlarged into a disc of radius `r` around `center`,
/// nearest-neighbour, with an antialiased circular alpha edge. Rendered at
/// output resolution so the edge stays smooth while the pixels stay blocky.
pub(crate) fn magnify(doc: &Document, source: RectF, center: Pt, r: f32) -> Option<Prim> {
    if r < 1.0 || source.w < 1.0 || source.h < 1.0 {
        return None;
    }
    let x0 = (center.x - r).floor();
    let y0 = (center.y - r).floor();
    let n = ((center.x + r).ceil() - x0).max((center.y + r).ceil() - y0) as usize;
    let k = source.w.min(source.h) / (2.0 * r);
    let sc = source.center();
    let src_rect = snap(RectF::new(sc.x - r * k - 1.0, sc.y - r * k - 1.0, 2.0 * r * k + 2.0, 2.0 * r * k + 2.0));
    let src = sample(doc, src_rect);
    let mut px = vec![[0u8; 4]; n * n];
    for j in 0..n {
        for i in 0..n {
            let p = Pt::new(x0 + i as f32 + 0.5, y0 + j as f32 + 0.5);
            let cover = (r - p.dist(center) + 0.5).clamp(0.0, 1.0);
            if cover <= 0.0 {
                continue;
            }
            let sx = ((sc.x + (p.x - center.x) * k).floor() as i32 - src_rect.x).clamp(0, src.w as i32 - 1);
            let sy = ((sc.y + (p.y - center.y) * k).floor() as i32 - src_rect.y).clamp(0, src.h as i32 - 1);
            let mut c = src.at(sx as usize, sy as usize);
            c[3] = (c[3] as f32 * cover).round() as u8;
            px[j * n + i] = c;
        }
    }
    Some(Prim::Image {
        dest: RectF::new(x0, y0, n as f32, n as f32),
        bitmap: Img { w: n, h: n, px }.into_bitmap(),
        opacity: 1.0,
        nearest: true,
    })
}
