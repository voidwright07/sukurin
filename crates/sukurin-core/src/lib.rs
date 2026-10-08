//! Types shared by every Shattr crate. All geometry here is in physical
//! (device) pixels; logical points only exist at the UI layer.

pub mod stitch;

use std::sync::Arc;
use std::time::SystemTime;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PointPx {
    pub x: i32,
    pub y: i32,
}

impl PointPx {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct RectPx {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl RectPx {
    pub const fn new(x: i32, y: i32, w: u32, h: u32) -> Self {
        Self { x, y, w, h }
    }

    /// The rectangle spanned by two corners, in either order. Both corner
    /// pixels are included.
    pub fn from_corners(a: PointPx, b: PointPx) -> Self {
        let (x0, x1) = (a.x.min(b.x), a.x.max(b.x));
        let (y0, y1) = (a.y.min(b.y), a.y.max(b.y));
        Self::new(x0, y0, (x1 - x0 + 1) as u32, (y1 - y0 + 1) as u32)
    }

    pub fn right(&self) -> i32 {
        self.x + self.w as i32
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.h as i32
    }

    pub fn is_empty(&self) -> bool {
        self.w == 0 || self.h == 0
    }

    pub fn contains(&self, p: PointPx) -> bool {
        p.x >= self.x && p.y >= self.y && p.x < self.right() && p.y < self.bottom()
    }

    pub fn intersect(&self, other: &RectPx) -> Option<RectPx> {
        let x0 = self.x.max(other.x);
        let y0 = self.y.max(other.y);
        let x1 = self.right().min(other.right());
        let y1 = self.bottom().min(other.bottom());
        (x1 > x0 && y1 > y0).then(|| RectPx::new(x0, y0, (x1 - x0) as u32, (y1 - y0) as u32))
    }

    pub fn translate(&self, dx: i32, dy: i32) -> RectPx {
        RectPx::new(self.x + dx, self.y + dy, self.w, self.h)
    }
}

/// An immutable, tightly packed BGRA8 image. BGRA is what both the capture
/// backends produce and the GPU renderer consumes, so the hot path never
/// swizzles.
#[derive(Clone)]
pub struct Bitmap {
    width: u32,
    height: u32,
    bgra: Arc<[u8]>,
}

impl PartialEq for Bitmap {
    fn eq(&self, other: &Self) -> bool {
        self.width == other.width
            && self.height == other.height
            && (Arc::ptr_eq(&self.bgra, &other.bgra) || self.bgra == other.bgra)
    }
}

impl std::fmt::Debug for Bitmap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Bitmap({}x{})", self.width, self.height)
    }
}

impl Bitmap {
    /// Panics when `bgra.len() != width * height * 4`.
    pub fn from_bgra(width: u32, height: u32, bgra: impl Into<Arc<[u8]>>) -> Self {
        let bgra = bgra.into();
        assert_eq!(bgra.len(), width as usize * height as usize * 4, "bitmap size mismatch");
        Self { width, height, bgra }
    }

    pub fn from_rgba(width: u32, height: u32, mut rgba: Vec<u8>) -> Self {
        swap_red_blue(&mut rgba);
        Self::from_bgra(width, height, rgba)
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn bounds(&self) -> RectPx {
        RectPx::new(0, 0, self.width, self.height)
    }

    pub fn bgra(&self) -> &[u8] {
        &self.bgra
    }

    pub fn shares_buffer_with(&self, other: &Bitmap) -> bool {
        Arc::ptr_eq(&self.bgra, &other.bgra)
    }

    pub fn pixel(&self, x: u32, y: u32) -> Option<Rgba> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = (y as usize * self.width as usize + x as usize) * 4;
        let p = &self.bgra[i..i + 4];
        Some(Rgba { r: p[2], g: p[1], b: p[0], a: p[3] })
    }

    /// Copies out `rect`, clipped to the bitmap. Returns `None` when nothing
    /// of `rect` lies inside.
    pub fn crop(&self, rect: RectPx) -> Option<Bitmap> {
        let r = rect.intersect(&self.bounds())?;
        if r == self.bounds() {
            return Some(self.clone());
        }
        let stride = self.width as usize * 4;
        let row = r.w as usize * 4;
        let mut out = Vec::with_capacity(row * r.h as usize);
        for y in r.y as usize..r.bottom() as usize {
            let start = y * stride + r.x as usize * 4;
            out.extend_from_slice(&self.bgra[start..start + row]);
        }
        Some(Bitmap::from_bgra(r.w, r.h, out))
    }

    pub fn to_rgba(&self) -> Vec<u8> {
        let mut v = self.bgra.to_vec();
        swap_red_blue(&mut v);
        v
    }

    /// Shrinks `rect` past margins of one flat colour, so a loose drag around
    /// a panel snaps to the panel. A margin must be at least two lines thick:
    /// a single flat line at the edge is usually the panel's own border, and
    /// trimming it would also eat the flat fill behind it.
    pub fn trim_flat_margins(&self, rect: RectPx, tolerance: u8) -> RectPx {
        let Some(r) = rect.intersect(&self.bounds()) else {
            return rect;
        };
        let close = |a: Rgba, b: Rgba| {
            a.r.abs_diff(b.r) <= tolerance && a.g.abs_diff(b.g) <= tolerance && a.b.abs_diff(b.b) <= tolerance
        };
        // Colour of a line if every pixel on it matches, else None.
        let row = |y: u32, x0: u32, x1: u32| {
            let first = self.pixel(x0, y).unwrap();
            (x0..=x1).all(|x| close(self.pixel(x, y).unwrap(), first)).then_some(first)
        };
        let col = |x: u32, y0: u32, y1: u32| {
            let first = self.pixel(x, y0).unwrap();
            (y0..=y1).all(|y| close(self.pixel(x, y).unwrap(), first)).then_some(first)
        };
        // Length of the run of flat lines matching the outermost one.
        let margin = |lines: &mut dyn Iterator<Item = Option<Rgba>>| {
            let Some(Some(edge)) = lines.next() else { return 0 };
            1 + lines.take_while(|l| l.is_some_and(|c| close(c, edge))).count() as u32
        };
        let (mut x0, mut y0) = (r.x as u32, r.y as u32);
        let (mut x1, mut y1) = (r.right() as u32 - 1, r.bottom() as u32 - 1);
        let top = margin(&mut (y0..=y1).map(|y| row(y, x0, x1)));
        let bottom = margin(&mut (y0..=y1).rev().map(|y| row(y, x0, x1)));
        if top >= 2 {
            y0 += top;
        }
        if bottom >= 2 {
            y1 = y1.saturating_sub(bottom);
        }
        if y0 >= y1 {
            return r;
        }
        let left = margin(&mut (x0..=x1).map(|x| col(x, y0, y1)));
        let right = margin(&mut (x0..=x1).rev().map(|x| col(x, y0, y1)));
        if left >= 2 {
            x0 += left;
        }
        if right >= 2 {
            x1 = x1.saturating_sub(right);
        }
        if x0 >= x1 {
            return r;
        }
        RectPx::new(x0 as i32, y0 as i32, x1 - x0 + 1, y1 - y0 + 1)
    }

    /// Area-averaged downscale to fit within `max_w` x `max_h`, keeping the
    /// aspect ratio. Never upscales.
    pub fn downscale_to_fit(&self, max_w: u32, max_h: u32) -> Bitmap {
        let k = (max_w as f64 / self.width as f64).min(max_h as f64 / self.height as f64);
        if k >= 1.0 {
            return self.clone();
        }
        let w = ((self.width as f64 * k).round() as u32).max(1);
        let h = ((self.height as f64 * k).round() as u32).max(1);
        let mut out = Vec::with_capacity(w as usize * h as usize * 4);
        for oy in 0..h {
            let y0 = (oy as u64 * self.height as u64 / h as u64) as u32;
            let y1 = (((oy + 1) as u64 * self.height as u64 / h as u64) as u32).max(y0 + 1);
            for ox in 0..w {
                let x0 = (ox as u64 * self.width as u64 / w as u64) as u32;
                let x1 = (((ox + 1) as u64 * self.width as u64 / w as u64) as u32).max(x0 + 1);
                let mut acc = [0u32; 4];
                for y in y0..y1 {
                    let row = (y as usize * self.width as usize + x0 as usize) * 4;
                    for p in self.bgra[row..row + (x1 - x0) as usize * 4].as_chunks::<4>().0 {
                        for c in 0..4 {
                            acc[c] += p[c] as u32;
                        }
                    }
                }
                let n = (x1 - x0) * (y1 - y0);
                out.extend(acc.map(|v| (v / n) as u8));
            }
        }
        Bitmap::from_bgra(w, h, out)
    }

    /// Mean colour over `rect` (clipped), alpha ignored.
    pub fn average(&self, rect: RectPx) -> Option<Rgba> {
        let r = rect.intersect(&self.bounds())?;
        let (mut sr, mut sg, mut sb) = (0u64, 0u64, 0u64);
        for y in r.y as u32..r.bottom() as u32 {
            for x in r.x as u32..r.right() as u32 {
                let p = self.pixel(x, y).unwrap();
                sr += p.r as u64;
                sg += p.g as u64;
                sb += p.b as u64;
            }
        }
        let n = r.w as u64 * r.h as u64;
        Some(Rgba::opaque((sr / n) as u8, (sg / n) as u8, (sb / n) as u8))
    }
}

fn swap_red_blue(buf: &mut [u8]) {
    for p in buf.as_chunks_mut::<4>().0 {
        p.swap(0, 2);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const fn opaque(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub fn hex(&self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }

    pub fn u32_rgb(&self) -> u32 {
        (self.r as u32) << 16 | (self.g as u32) << 8 | self.b as u32
    }
}

/// One physical display as the capture backend sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct Display {
    /// Position and size on the virtual desktop, physical pixels.
    pub rect: RectPx,
    /// Physical pixels per logical point (1.5 at 150% scaling).
    pub scale: f32,
    pub name: String,
}

/// A whole-display grab, before the user has picked a region.
#[derive(Clone, Debug)]
pub struct Frame {
    pub display: Display,
    pub bitmap: Bitmap,
    pub taken_at: SystemTime,
}

/// A finished capture: the pixels the user chose, plus where they came from.
#[derive(Clone, Debug)]
pub struct Shot {
    pub bitmap: Bitmap,
    pub scale: f32,
    /// Region on the virtual desktop, physical pixels.
    pub source_rect: RectPx,
    pub taken_at: SystemTime,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checker(w: u32, h: u32) -> Bitmap {
        let mut v = Vec::new();
        for y in 0..h {
            for x in 0..w {
                v.extend_from_slice(&[x as u8, y as u8, 0, 255]);
            }
        }
        Bitmap::from_bgra(w, h, v)
    }

    #[test]
    fn rect_from_corners_includes_both_corner_pixels() {
        let r = RectPx::from_corners(PointPx::new(10, 5), PointPx::new(3, 8));
        assert_eq!(r, RectPx::new(3, 5, 8, 4));
    }

    #[test]
    fn crop_copies_the_requested_pixels_and_clips() {
        let b = checker(10, 10);
        let c = b.crop(RectPx::new(8, 7, 5, 5)).unwrap();
        assert_eq!((c.width(), c.height()), (2, 3));
        assert_eq!(c.pixel(0, 0).unwrap().b, 8);
        assert_eq!(c.pixel(1, 2).unwrap().g, 9);
        assert!(b.crop(RectPx::new(20, 20, 3, 3)).is_none());
    }

    #[test]
    fn pixel_reads_bgra_as_rgb() {
        let b = Bitmap::from_bgra(1, 1, vec![0xEB, 0x63, 0x25, 0xFF]);
        assert_eq!(b.pixel(0, 0).unwrap().hex(), "#2563EB");
    }

    fn panel_on_background() -> Bitmap {
        let (w, h) = (40u32, 30u32);
        let mut v = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let inside = (10..30).contains(&x) && (8..20).contains(&y);
                let border = inside && (x == 10 || x == 29 || y == 8 || y == 19);
                let c = if border { [0xE5, 0xE7, 0xEB] } else if inside { [255, 255, 255] } else { [0xF7, 0xF8, 0xFA] };
                let text = inside && !border && y == 12 && (12..20).contains(&x);
                let c = if text { [0x11, 0x18, 0x27] } else { c };
                v.extend_from_slice(&[c[2], c[1], c[0], 255]);
            }
        }
        Bitmap::from_bgra(w, h, v)
    }

    #[test]
    fn trim_snaps_a_loose_selection_to_the_panel_border() {
        let b = panel_on_background();
        let loose = RectPx::new(4, 3, 32, 22);
        assert_eq!(b.trim_flat_margins(loose, 4), RectPx::new(10, 8, 20, 12));
    }

    #[test]
    fn trim_keeps_a_selection_that_is_already_tight() {
        let b = panel_on_background();
        let tight = RectPx::new(10, 8, 20, 12);
        assert_eq!(b.trim_flat_margins(tight, 4), tight);
    }

    #[test]
    fn trim_of_a_flat_region_returns_it_unchanged() {
        let b = panel_on_background();
        let flat = RectPx::new(0, 0, 8, 8);
        assert_eq!(b.trim_flat_margins(flat, 4), flat);
    }

    #[test]
    fn downscale_fits_the_box_keeps_aspect_and_averages() {
        let mut v = Vec::new();
        for _y in 0..4 {
            for x in 0..8 {
                let c = if x % 2 == 0 { 0 } else { 200 };
                v.extend_from_slice(&[c, c, c, 255]);
            }
        }
        let b = Bitmap::from_bgra(8, 4, v);
        let d = b.downscale_to_fit(4, 4);
        assert_eq!((d.width(), d.height()), (4, 2));
        assert_eq!(d.pixel(1, 1).unwrap(), Rgba::opaque(100, 100, 100));
        assert!(b.downscale_to_fit(100, 100).shares_buffer_with(&b));
    }

    #[test]
    fn average_is_the_mean_of_the_region() {
        let b = Bitmap::from_bgra(2, 1, vec![0, 0, 0, 255, 200, 100, 50, 255]);
        assert_eq!(b.average(b.bounds()).unwrap(), Rgba::opaque(25, 50, 100));
    }
}
