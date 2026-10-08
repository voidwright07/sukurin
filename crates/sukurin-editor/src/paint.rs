//! Paints the annotation pipeline's [`Scene`] with GPUI. Fills are
//! tessellated once in canvas space and only transformed per frame; image
//! patches become cached GPU images.

use crate::viewport::Viewport;
use gpui_kit::*;
use shattr_annotate::{Prim, RectF, Scene};
use shattr_core::Bitmap;
use std::sync::Arc;

pub enum PaintPrim {
    Fill { path: Path<Pixels>, color: Hsla },
    Image { dest: RectF, image: Arc<RenderImage> },
}

fn hsla(c: shattr_core::Rgba) -> Hsla {
    gpui_kit::Rgba { r: c.r as f32 / 255., g: c.g as f32 / 255., b: c.b as f32 / 255., a: c.a as f32 / 255. }.into()
}

#[derive(Clone, Copy, PartialEq)]
struct Variant {
    opacity_milli: u16,
    size: Option<(u32, u32)>,
}

struct Entry {
    bitmap: Bitmap,
    variant: Variant,
    image: Arc<RenderImage>,
    live: bool,
}

/// GPU images keyed by the bitmap buffer they came from. Images no longer
/// referenced by any scene are queued for release from the sprite atlas.
#[derive(Default)]
pub struct ImageCache {
    entries: Vec<Entry>,
    retired: Vec<Arc<RenderImage>>,
}

impl ImageCache {
    pub fn begin(&mut self) {
        for e in &mut self.entries {
            e.live = false;
        }
    }

    /// Retires every entry not requested since [`ImageCache::begin`].
    pub fn end(&mut self) {
        let (live, dead): (Vec<_>, Vec<_>) = std::mem::take(&mut self.entries).into_iter().partition(|e| e.live);
        self.entries = live;
        self.retired.extend(dead.into_iter().map(|e| e.image));
    }

    /// The GPU image for `bitmap` faded to `opacity` and, if asked,
    /// nearest-enlarged to `upscale_to`. Marks the entry live.
    pub fn image(&mut self, bitmap: &Bitmap, opacity: f32, upscale_to: Option<(u32, u32)>) -> Arc<RenderImage> {
        let variant = Variant { opacity_milli: (opacity.clamp(0., 1.) * 1000.).round() as u16, size: upscale_to };
        if let Some(e) = self.entries.iter_mut().find(|e| e.variant == variant && e.bitmap.shares_buffer_with(bitmap)) {
            e.live = true;
            return e.image.clone();
        }
        let mut b = match upscale_to {
            Some((w, h)) => nearest(bitmap, w, h),
            None => bitmap.clone(),
        };
        if variant.opacity_milli < 1000 {
            b = with_opacity(&b, opacity);
        }
        let image = shattr_ui::render_image(&b);
        self.entries.push(Entry { bitmap: bitmap.clone(), variant, image: image.clone(), live: true });
        image
    }

    pub fn release(&mut self, window: &mut Window) {
        for image in self.retired.drain(..) {
            let _ = window.drop_image(image);
        }
    }
}

const MAX_UPSCALE: u32 = 4096;

/// Converts a scene for painting. Nearest-sampled patches (pixelation) are
/// enlarged on the CPU, because GPUI samples every image bilinearly.
pub fn prepare(scene: &Scene, images: &mut ImageCache) -> Vec<PaintPrim> {
    let mut out = Vec::with_capacity(scene.len());
    for prim in scene {
        match prim {
            Prim::Fill { polygons, color } => {
                let mut b = PathBuilder::fill().with_style(PathStyle::Fill(FillOptions::non_zero()));
                let mut any = false;
                for poly in polygons.iter().filter(|p| p.len() >= 3) {
                    let pts: Vec<Point<Pixels>> = poly.iter().map(|p| point(px(p.x), px(p.y))).collect();
                    b.add_polygon(&pts, true);
                    any = true;
                }
                if !any || color.a == 0 {
                    continue;
                }
                match b.build() {
                    Ok(path) => out.push(PaintPrim::Fill { path, color: hsla(*color) }),
                    Err(e) => log::warn!("tessellation failed: {e}"),
                }
            }
            Prim::Image { dest, bitmap, opacity, nearest } => {
                if dest.w <= 0. || dest.h <= 0. || *opacity <= 0. {
                    continue;
                }
                let upscale = (*nearest && (dest.w > bitmap.width() as f32 || dest.h > bitmap.height() as f32)).then(|| {
                    ((dest.w.round() as u32).clamp(1, MAX_UPSCALE), (dest.h.round() as u32).clamp(1, MAX_UPSCALE))
                });
                let image = images.image(bitmap, *opacity, upscale);
                out.push(PaintPrim::Image { dest: *dest, image });
            }
        }
    }
    out
}

pub fn view_bounds(vp: &Viewport, origin: Point<Pixels>, r: RectF) -> Bounds<Pixels> {
    let v = vp.rect_to_view(r);
    Bounds::new(point(origin.x + px(v.x), origin.y + px(v.y)), size(px(v.w), px(v.h)))
}

/// Paints prims transformed by `vp`, with `origin` the window position of
/// the view's (0, 0).
pub fn paint(prims: &[PaintPrim], vp: &Viewport, origin: Point<Pixels>, window: &mut Window) {
    let (ox, oy, z) = (origin.x.as_f32() + vp.origin.x, origin.y.as_f32() + vp.origin.y, vp.zoom);
    let map = |p: Point<Pixels>| point(px(ox + p.x.as_f32() * z), px(oy + p.y.as_f32() * z));
    for prim in prims {
        match prim {
            PaintPrim::Fill { path, color } => {
                let mut p = path.clone();
                for v in &mut p.vertices {
                    v.xy_position = map(v.xy_position);
                }
                p.bounds = Bounds::new(map(path.bounds.origin), size(path.bounds.size.width * z, path.bounds.size.height * z));
                window.paint_path(p, *color);
            }
            PaintPrim::Image { dest, image } => {
                let b = view_bounds(vp, origin, *dest);
                let _ = window.paint_image(b, b, Corners::default(), image.clone(), 0, false);
            }
        }
    }
}

pub fn nearest(b: &Bitmap, w: u32, h: u32) -> Bitmap {
    let (sw, sh) = (b.width() as usize, b.height() as usize);
    let src = b.bgra();
    let mut out = Vec::with_capacity(w as usize * h as usize * 4);
    let xs: Vec<usize> = (0..w as usize).map(|x| (x * sw / w as usize).min(sw - 1)).collect();
    for y in 0..h as usize {
        let row = (y * sh / h as usize).min(sh - 1) * sw * 4;
        for &x in &xs {
            out.extend_from_slice(&src[row + x * 4..row + x * 4 + 4]);
        }
    }
    Bitmap::from_bgra(w, h, out)
}

fn with_opacity(b: &Bitmap, opacity: f32) -> Bitmap {
    let mut v = b.bgra().to_vec();
    for p in v.as_chunks_mut::<4>().0 {
        p[3] = (p[3] as f32 * opacity).round() as u8;
    }
    Bitmap::from_bgra(b.width(), b.height(), v)
}

#[cfg(test)]
mod tests {
    use super::nearest;
    use shattr_core::Bitmap;

    #[test]
    fn nearest_upscale_repeats_source_pixels_in_blocks() {
        let b = Bitmap::from_bgra(2, 1, vec![0, 0, 0, 255, 200, 200, 200, 255]);
        let n = nearest(&b, 4, 2);
        assert_eq!((n.width(), n.height()), (4, 2));
        let row: Vec<u8> = (0..4).map(|x| n.pixel(x, 1).unwrap().r).collect();
        assert_eq!(row, vec![0, 0, 200, 200]);
    }
}
