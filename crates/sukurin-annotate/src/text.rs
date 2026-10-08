use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use tiny_skia::{Path, PathBuilder, Transform};
use ttf_parser::gpos::{PairAdjustment, PositioningSubtable};
use ttf_parser::{Face, GlyphId, OutlineBuilder, Tag};

use crate::Pt;

static INTER: &[u8] = include_bytes!("../../../assets/fonts/InterVariable.ttf");

/// A glyph in em units, y pointing down, origin on the baseline.
pub(crate) struct Glyph {
    id: GlyphId,
    advance: f32,
    path: Option<Path>,
}

struct Font {
    regular: Face<'static>,
    bold: Face<'static>,
    ascender: f32,
    line_height: f32,
    cap_height: f32,
    em: f32,
    /// Pair-adjustment subtables of each GPOS `kern` lookup.
    kern: Vec<Vec<PairAdjustment<'static>>>,
    glyphs: Mutex<HashMap<(char, bool), Arc<Glyph>>>,
    pairs: Mutex<HashMap<(u16, u16), f32>>,
}

fn kern_lookups(face: &Face<'static>) -> Vec<Vec<PairAdjustment<'static>>> {
    let Some(gpos) = face.tables().gpos else { return Vec::new() };
    let mut indices: Vec<u16> = gpos
        .features
        .into_iter()
        .filter(|f| f.tag == Tag::from_bytes(b"kern"))
        .flat_map(|f| f.lookup_indices)
        .collect();
    indices.sort_unstable();
    indices.dedup();
    indices
        .into_iter()
        .filter_map(|i| gpos.lookups.get(i))
        .map(|lookup| {
            (0..lookup.subtables.len())
                .filter_map(|j| match lookup.subtables.get::<PositioningSubtable>(j) {
                    Some(PositioningSubtable::Pair(p)) => Some(p),
                    _ => None,
                })
                .collect()
        })
        .collect()
}

/// Horizontal adjustment between two glyphs in em units. Variation deltas
/// on kerning values are ignored; the default-instance value is close
/// enough at annotation sizes.
fn kerning(a: GlyphId, b: GlyphId) -> f32 {
    let f = font();
    let mut pairs = f.pairs.lock().unwrap_or_else(|e| e.into_inner());
    *pairs.entry((a.0, b.0)).or_insert_with(|| {
        let mut units = 0i32;
        for lookup in &f.kern {
            for sub in lookup {
                let value = match sub {
                    PairAdjustment::Format1 { coverage, sets } => {
                        coverage.get(a).and_then(|i| sets.get(i)).and_then(|set| set.get(b)).map(|(v, _)| v.x_advance)
                    }
                    PairAdjustment::Format2 { coverage, classes, matrix } => coverage
                        .contains(a)
                        .then(|| matrix.get((classes.0.get(a), classes.1.get(b))))
                        .flatten()
                        .map(|(v, _)| v.x_advance),
                };
                if let Some(v) = value {
                    units += v as i32;
                    break;
                }
            }
        }
        units as f32 / f.em
    })
}

fn font() -> &'static Font {
    static FONT: OnceLock<Font> = OnceLock::new();
    FONT.get_or_init(|| {
        let face = |weight: f32| {
            let mut f = Face::parse(INTER, 0).expect("bundled Inter font parses");
            f.set_variation(Tag::from_bytes(b"wght"), weight);
            f
        };
        let regular = face(400.0);
        let em = regular.units_per_em() as f32;
        let ascender = regular.ascender() as f32 / em;
        let descender = regular.descender() as f32 / em;
        let gap = regular.line_gap() as f32 / em;
        let cap_height = regular.capital_height().map_or(0.727, |c| c as f32 / em);
        Font {
            em,
            kern: kern_lookups(&regular),
            pairs: Mutex::new(HashMap::new()),
            bold: face(700.0),
            regular,
            ascender,
            line_height: ascender - descender + gap,
            cap_height,
            glyphs: Mutex::new(HashMap::new()),
        }
    })
}

struct EmBuilder {
    pb: PathBuilder,
    k: f32,
}

impl OutlineBuilder for EmBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        self.pb.move_to(x * self.k, -y * self.k);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.pb.line_to(x * self.k, -y * self.k);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.pb.quad_to(x1 * self.k, -y1 * self.k, x * self.k, -y * self.k);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let k = self.k;
        self.pb.cubic_to(x1 * k, -y1 * k, x2 * k, -y2 * k, x * k, -y * k);
    }
    fn close(&mut self) {
        self.pb.close();
    }
}

fn glyph(c: char, bold: bool) -> Arc<Glyph> {
    let f = font();
    let mut cache = f.glyphs.lock().unwrap_or_else(|e| e.into_inner());
    cache
        .entry((c, bold))
        .or_insert_with(|| {
            let face = if bold { &f.bold } else { &f.regular };
            let k = 1.0 / face.units_per_em() as f32;
            let id = face.glyph_index(c).unwrap_or(GlyphId(0));
            let mut b = EmBuilder { pb: PathBuilder::new(), k };
            let path = face.outline_glyph(id, &mut b).and_then(|_| b.pb.finish());
            let advance = face.glyph_hor_advance(id).unwrap_or(0) as f32 * k;
            Arc::new(Glyph { id, advance, path })
        })
        .clone()
}

/// Glyphs placed relative to the top-left of the text block.
pub(crate) struct TextLayout {
    placed: Vec<(Arc<Glyph>, Pt)>,
    size: f32,
    pub width: f32,
    pub height: f32,
}

/// Left-aligned lines, `size` pixels per em, one line per '\n'.
pub(crate) fn layout(text: &str, size: f32, bold: bool) -> TextLayout {
    let f = font();
    let line_h = f.line_height * size;
    let mut placed = Vec::new();
    let mut width: f32 = 0.0;
    let mut lines = 0;
    for (i, line) in text.split('\n').enumerate() {
        lines += 1;
        let baseline = f.ascender * size + i as f32 * line_h;
        let mut x = 0.0;
        let mut prev: Option<GlyphId> = None;
        for c in line.chars() {
            let (c, repeat) = match c {
                '\t' => (' ', 4),
                '\r' => continue,
                c => (c, 1),
            };
            let g = glyph(c, bold);
            if let Some(p) = prev {
                x += kerning(p, g.id) * size;
            }
            prev = Some(g.id);
            for _ in 0..repeat {
                let adv = g.advance * size;
                placed.push((g.clone(), Pt::new(x, baseline)));
                x += adv;
            }
        }
        width = width.max(x);
    }
    TextLayout { placed, size, width, height: lines as f32 * line_h }
}

impl TextLayout {
    /// All glyph outlines at `origin` as one path, for filling or stroking.
    pub(crate) fn path(&self, origin: Pt) -> Option<Path> {
        let mut pb = PathBuilder::new();
        for (g, at) in &self.placed {
            let Some(p) = &g.path else { continue };
            let ts = Transform::from_row(self.size, 0.0, 0.0, self.size, origin.x + at.x, origin.y + at.y);
            if let Some(p) = p.clone().transform(ts) {
                pb.push_path(&p);
            }
        }
        pb.finish()
    }
}

/// Offset from the top of a line box down to the point where capitals and
/// digits are vertically centred.
pub(crate) fn cap_center(size: f32) -> f32 {
    let f = font();
    (f.ascender - f.cap_height / 2.0) * size
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bold_glyphs_are_wider_and_lines_stack() {
        let r = layout("Hi", 20.0, false);
        let b = layout("Hi", 20.0, true);
        assert!(b.width > r.width);
        let two = layout("Hi\nthere", 20.0, false);
        assert!((two.height - 2.0 * r.height).abs() < 0.01);
        assert!(r.path(Pt::default()).is_some());
    }

    #[test]
    fn kerning_tucks_classic_pairs() {
        let pair = |s: &str| layout(s, 100.0, false).width;
        let apart = |a: &str, b: &str| pair(a) + pair(b);
        assert!(pair("AV") < apart("A", "V") - 1.0, "AV {} vs {}", pair("AV"), apart("A", "V"));
        assert!(pair("To") < apart("T", "o") - 1.0);
        assert!((pair("HH") - apart("H", "H")).abs() < 0.01);
    }
}
