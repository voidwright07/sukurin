//! The annotation document and its one geometry pipeline.
//!
//! Every visible mark is reduced to a [`Scene`]: filled polygons and image
//! patches in canvas pixels. The GPUI editor paints that scene live and
//! [`export`] rasterises the very same scene with tiny-skia, so what the user
//! sees is what gets copied, saved, or sent to a model.
//!
//! Canvas space is physical pixels of the combined capture layers, origin at
//! the top-left of the first layer. Crop and backdrop are applied on export.

mod geom;
mod pixels;
mod raster;
mod shapes;
mod sketch;
mod text;

use shattr_core::{Bitmap, Rgba};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pt {
    pub x: f32,
    pub y: f32,
}

impl Pt {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RectF {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl RectF {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    /// Normalised rectangle between two corners in any order.
    pub fn from_points(a: Pt, b: Pt) -> Self {
        Self::new(a.x.min(b.x), a.y.min(b.y), (a.x - b.x).abs(), (a.y - b.y).abs())
    }

    pub fn contains(&self, p: Pt) -> bool {
        p.x >= self.x && p.y >= self.y && p.x <= self.x + self.w && p.y <= self.y + self.h
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dash {
    Solid,
    Dashed,
    Dotted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fill {
    None,
    /// The stroke colour at low alpha.
    Tint,
    Solid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArrowKind {
    Straight,
    /// One quadratic curve; the control point follows the endpoints.
    Curved,
    /// A quadratic curve whose control point the user drags freely.
    Bendable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RedactMode {
    Pixelate,
    Blur,
    /// Fill from the surrounding background.
    Erase,
    /// Remove glyph-like pixels only, keeping backgrounds and borders.
    EraseText,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextBackground {
    None,
    Pill,
    Outline,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Horizontal,
    Vertical,
}

/// Appearance shared by every shape; each shape reads the fields it uses.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    pub color: Rgba,
    /// Stroke width in canvas pixels.
    pub width: f32,
    pub dash: Dash,
    pub fill: Fill,
    /// 0.0 to 1.0, multiplies the whole mark.
    pub opacity: f32,
    /// Sketchy, slightly wobbly geometry for shapes and arrows. Deterministic
    /// per annotation id so it doesn't shimmer while editing.
    pub hand_drawn: bool,
    /// Text and step-counter size in canvas pixels.
    pub font_size: f32,
    pub bold: bool,
    pub text_background: TextBackground,
    pub corner_radius: f32,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            color: Rgba::opaque(0xF0, 0x44, 0x3A),
            width: 4.0,
            dash: Dash::Solid,
            fill: Fill::None,
            opacity: 1.0,
            hand_drawn: false,
            font_size: 28.0,
            bold: true,
            text_background: TextBackground::Pill,
            corner_radius: 6.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    Arrow { from: Pt, to: Pt, control: Option<Pt>, kind: ArrowKind },
    Line { from: Pt, to: Pt },
    Rect { rect: RectF },
    Oval { rect: RectF },
    Freehand { points: Vec<Pt> },
    Highlighter { points: Vec<Pt> },
    /// `origin` is the top-left of the first line's box; a pill background
    /// pads outward from it.
    Text { origin: Pt, text: String },
    Step { center: Pt, number: u32 },
    /// Dims everything outside `rect`.
    Spotlight { rect: RectF, oval: bool },
    Redact { rect: RectF, mode: RedactMode, block: f32 },
    /// Shows `source` enlarged inside the circle inscribed in `callout`.
    Magnifier { source: RectF, callout: RectF },
    /// A distance readout between two points, labelled in logical points.
    Measure { from: Pt, to: Pt },
    Guide { axis: Axis, at: f32 },
    Overlay { rect: RectF, image: Bitmap, opacity: f32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AnnotationId(pub u64);

#[derive(Clone, Debug, PartialEq)]
pub struct Annotation {
    pub id: AnnotationId,
    pub shape: Shape,
    pub style: Style,
}

/// One capture placed on the canvas. Combining captures adds layers.
#[derive(Clone, Debug)]
pub struct Layer {
    pub bitmap: Bitmap,
    pub origin: Pt,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Backdrop {
    /// Top-left and bottom-right colours of a linear gradient; equal for flat.
    pub from: Rgba,
    pub to: Rgba,
    pub padding: f32,
    pub shadow: f32,
    pub radius: f32,
    /// Pads the framed output out to this aspect ratio, if set.
    pub aspect: Option<(u32, u32)>,
}

#[derive(Clone, Debug)]
pub struct Document {
    pub layers: Vec<Layer>,
    /// Physical pixels per logical point of the capture (1.5 at 150%).
    pub scale: f32,
    pub annotations: Vec<Annotation>,
    pub crop: Option<RectF>,
    pub backdrop: Option<Backdrop>,
    next_id: u64,
}

/// Something drawable, in canvas pixels, back to front.
#[derive(Clone, Debug)]
pub enum Prim {
    /// Polygons filled together under the non-zero rule (holes wind the
    /// other way). Strokes, dashes, arrowheads and glyphs all arrive here
    /// already outlined and flattened.
    Fill { polygons: Vec<Vec<Pt>>, color: Rgba },
    /// A pixel patch: redactions, magnifier contents, overlays.
    Image { dest: RectF, bitmap: Bitmap, opacity: f32, nearest: bool },
}

pub type Scene = Vec<Prim>;

impl Document {
    pub fn new(base: Bitmap, scale: f32) -> Self {
        Self {
            layers: vec![Layer { bitmap: base, origin: Pt::default() }],
            scale,
            annotations: Vec::new(),
            crop: None,
            backdrop: None,
            next_id: 1,
        }
    }

    pub fn add(&mut self, shape: Shape, style: Style) -> AnnotationId {
        let id = AnnotationId(self.next_id);
        self.next_id += 1;
        self.annotations.push(Annotation { id, shape, style });
        id
    }

    pub fn get(&self, id: AnnotationId) -> Option<&Annotation> {
        self.annotations.iter().find(|a| a.id == id)
    }

    pub fn get_mut(&mut self, id: AnnotationId) -> Option<&mut Annotation> {
        self.annotations.iter_mut().find(|a| a.id == id)
    }

    pub fn remove(&mut self, id: AnnotationId) -> Option<Annotation> {
        let i = self.annotations.iter().position(|a| a.id == id)?;
        Some(self.annotations.remove(i))
    }

    /// The next free step number (one more than the highest present).
    pub fn next_step(&self) -> u32 {
        let highest = self.annotations.iter().filter_map(|a| match a.shape {
            Shape::Step { number, .. } => Some(number),
            _ => None,
        });
        highest.max().map_or(1, |n| n + 1)
    }

    /// Bounding box of all layers.
    pub fn canvas_bounds(&self) -> RectF {
        self.layers
            .iter()
            .map(|l| RectF::new(l.origin.x, l.origin.y, l.bitmap.width() as f32, l.bitmap.height() as f32))
            .reduce(|a, b| a.union(&b))
            .unwrap_or_default()
    }

    /// Topmost annotation whose drawn geometry lies within `tolerance`
    /// canvas pixels of `p`.
    pub fn hit_test(&self, p: Pt, tolerance: f32) -> Option<AnnotationId> {
        self.annotations.iter().rev().find(|a| self.hits(a, p, tolerance)).map(|a| a.id)
    }

    fn hits(&self, a: &Annotation, p: Pt, tol: f32) -> bool {
        match &a.shape {
            Shape::Text { .. }
            | Shape::Step { .. }
            | Shape::Redact { .. }
            | Shape::Overlay { .. }
            | Shape::Magnifier { .. } => self.bounds_of(a).is_some_and(|b| b.inflate(tol).contains(p)),
            Shape::Spotlight { rect, oval } => {
                geom::outline_distance(&shapes::spotlight_hole(rect.normalized(), *oval, &a.style), p) <= tol
            }
            _ => shapes::prims(self, a).iter().any(|prim| match prim {
                Prim::Fill { polygons, .. } => geom::fill_hit(polygons, p, tol),
                Prim::Image { dest, .. } => dest.inflate(tol).contains(p),
            }),
        }
    }

    /// Visual bounds of one annotation, including stroke and arrowheads.
    /// A spotlight reports its hole, the part the user manipulates.
    pub fn bounds(&self, id: AnnotationId) -> Option<RectF> {
        self.bounds_of(self.get(id)?)
    }

    fn bounds_of(&self, a: &Annotation) -> Option<RectF> {
        match &a.shape {
            Shape::Spotlight { rect, .. } => Some(rect.normalized()),
            Shape::Redact { rect, .. } => rect.normalized().intersect(&self.canvas_bounds()),
            Shape::Text { origin, text } => Some(shapes::text_box(*origin, text, &a.style)),
            _ => shapes::prims(self, a)
                .iter()
                .filter_map(|prim| match prim {
                    Prim::Fill { polygons, .. } => geom::bounds_of(polygons),
                    Prim::Image { dest, .. } => Some(*dest),
                })
                .reduce(|a, b| a.union(&b)),
        }
    }
}

/// The capture layers, in canvas space, as image prims.
pub fn layers_scene(doc: &Document) -> Scene {
    doc.layers
        .iter()
        .map(|l| Prim::Image {
            dest: RectF::new(l.origin.x, l.origin.y, l.bitmap.width() as f32, l.bitmap.height() as f32),
            bitmap: l.bitmap.clone(),
            opacity: 1.0,
            nearest: false,
        })
        .collect()
}

/// All annotations, back to front. Redactions read pixels from the layers.
pub fn annotations_scene(doc: &Document) -> Scene {
    doc.annotations.iter().flat_map(|a| shapes::prims(doc, a)).collect()
}

/// One annotation on its own, for live editing without re-deriving the rest.
pub fn annotation_scene(doc: &Document, id: AnnotationId) -> Scene {
    doc.get(id).map(|a| shapes::prims(doc, a)).unwrap_or_default()
}

/// The finished image: layers + annotations, cropped, framed by the
/// backdrop. Must equal what the editor shows, pixel for pixel up to
/// antialiasing.
pub fn export(doc: &Document) -> Bitmap {
    let area = doc.crop.unwrap_or_else(|| doc.canvas_bounds());
    let (x, y) = (area.x.round(), area.y.round());
    let (w, h) = (area.w.round().max(0.0) as u32, area.h.round().max(0.0) as u32);
    let Some(mut pm) = tiny_skia::Pixmap::new(w, h) else {
        return Bitmap::from_bgra(0, 0, Vec::new());
    };
    raster::draw(&mut pm, &layers_scene(doc), x, y);
    raster::draw(&mut pm, &annotations_scene(doc), x, y);
    match doc.backdrop.as_ref().and_then(|b| raster::frame(&pm, b)) {
        Some(framed) => raster::to_bitmap(&framed),
        None => raster::to_bitmap(&pm),
    }
}

/// Undo/redo over whole-document snapshots. Bitmaps are shared, so a
/// snapshot costs roughly the annotation list.
#[derive(Default)]
pub struct History {
    undo: Vec<Document>,
    redo: Vec<Document>,
}

impl History {
    /// Call before mutating `doc`.
    pub fn checkpoint(&mut self, doc: &Document) {
        self.undo.push(doc.clone());
        self.redo.clear();
    }

    pub fn undo(&mut self, doc: &mut Document) -> bool {
        match self.undo.pop() {
            Some(prev) => {
                self.redo.push(std::mem::replace(doc, prev));
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self, doc: &mut Document) -> bool {
        match self.redo.pop() {
            Some(next) => {
                self.undo.push(std::mem::replace(doc, next));
                true
            }
            None => false,
        }
    }
}
