//! A small subset of the HTML canvas 2D API on top of tiny-skia, enough for player.rs.
//! Coordinates are CSS pixels of the 1200x630 design, as in player.js.
use std::collections::HashMap;
use tiny_skia::{
    Color, FillRule, LineCap, Paint, PathBuilder, Pixmap, Rect, Shader, Stroke, Transform,
};
use ttf_parser::{Face, GlyphId};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Font {
    Sans = 0,
    SansBold = 1,
    Mono = 2,
    MonoBold = 3,
}

// DejaVu subsets (Basic Latin, Latin-1, en/em dash, ellipsis), GPOS kerning kept, hinting dropped.
// Chromium resolves `system-ui` / `sans-serif` / `monospace` to DejaVu on the reference machine.
static FONT_DATA: [&[u8]; 4] = [
    include_bytes!("../fonts/DejaVuSans.subset.ttf"),
    include_bytes!("../fonts/DejaVuSans-Bold.subset.ttf"),
    include_bytes!("../fonts/DejaVuSansMono.subset.ttf"),
    include_bytes!("../fonts/DejaVuSansMono-Bold.subset.ttf"),
];

#[derive(Clone, Copy)]
pub struct Rgb(pub u8, pub u8, pub u8);

pub const fn hex(v: u32) -> Rgb {
    Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

#[derive(Clone, Copy)]
enum Seg {
    M(f32, f32),
    L(f32, f32),
    Q(f32, f32, f32, f32),
    C(f32, f32, f32, f32, f32, f32),
    Z,
}

struct Outline(Vec<Seg>);
impl ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.push(Seg::M(x, y))
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.push(Seg::L(x, y))
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.0.push(Seg::Q(x1, y1, x, y))
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.0.push(Seg::C(x1, y1, x2, y2, x, y))
    }
    fn close(&mut self) {
        self.0.push(Seg::Z)
    }
}

struct Glyph {
    id: GlyphId,
    adv: f32, // font units
    outline: Vec<Seg>,
}

pub struct Canvas {
    pub pm: Pixmap,
    /// true: rectangles and line segments go through the span fillers below and text through a glyph
    /// coverage cache; tiny-skia only rasterises glyph masks and circles.
    /// false: every primitive is a tiny-skia path (reference behaviour, slower).
    pub fast: bool,
    /// seconds spent per primitive kind: clear, rects, stroked lines, stroked rects, circles, text
    pub prof: [f64; 6],
    pub prof_on: bool,
    faces: Vec<Face<'static>>,
    glyphs: HashMap<(Font, char), Glyph>,
    kerns: HashMap<(Font, u16, u16), f32>,
    masks: HashMap<(Font, char, u32, u8), GlyphMask>,
    cov: Vec<u16>,
}

fn paint(c: Rgb, alpha: f32) -> Paint<'static> {
    Paint {
        shader: Shader::SolidColor(Color::from_rgba8(c.0, c.1, c.2, (alpha.clamp(0.0, 1.0) * 255.0).round() as u8)),
        anti_alias: true,
        ..Paint::default()
    }
}

impl Canvas {
    pub fn new(w: u32, h: u32) -> Canvas {
        Canvas {
            pm: Pixmap::new(w, h).expect("pixmap"),
            fast: true,
            prof: [0.0; 6],
            prof_on: false,
            faces: FONT_DATA.iter().map(|d| Face::parse(d, 0).expect("font")).collect(),
            glyphs: HashMap::new(),
            kerns: HashMap::new(),
            masks: HashMap::new(),
            cov: Vec::new(),
        }
    }

    /// RGBA8, opaque after a full frame (premultiplied == straight).
    pub fn data(&self) -> &[u8] {
        self.pm.data()
    }

    pub fn clear(&mut self, c: Rgb) {
        if !self.prof_on {
            return self.clear_inner(c);
        }
        let t = now();
        let r = self.clear_inner(c);
        self.prof[0] += since(t);
        r
    }

    fn clear_inner(&mut self, c: Rgb) {
        self.pm.fill(Color::from_rgba8(c.0, c.1, c.2, 255));
    }

    /// ctx.fillRect: negative sizes are normalised, empty rectangles draw nothing.
    pub fn fill_rect(&mut self, x: f64, y: f64, w: f64, h: f64, c: Rgb, alpha: f64) {
        if !self.prof_on {
            return self.fill_rect_inner(x, y, w, h, c, alpha);
        }
        let t = now();
        let r = self.fill_rect_inner(x, y, w, h, c, alpha);
        self.prof[1] += since(t);
        r
    }

    fn fill_rect_inner(&mut self, mut x: f64, mut y: f64, mut w: f64, mut h: f64, c: Rgb, alpha: f64) {
        if w < 0.0 {
            x += w;
            w = -w;
        }
        if h < 0.0 {
            y += h;
            h = -h;
        }
        if !(w > 0.0 && h > 0.0) || alpha <= 0.0 {
            return;
        }
        if self.fast {
            self.fill_rect_fast(x as f32, y as f32, (x + w) as f32, (y + h) as f32, c, alpha as f32);
        } else if let Some(r) = Rect::from_xywh(x as f32, y as f32, w as f32, h as f32) {
            self.pm.fill_rect(r, &paint(c, alpha as f32), Transform::identity(), None);
        }
    }

    /// Anti-aliased axis-aligned rectangle, source-over, coverage = covered pixel area.
    fn fill_rect_fast(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, c: Rgb, alpha: f32) {
        let (pw, ph) = (self.pm.width() as usize, self.pm.height() as usize);
        let data = self.pm.data_mut();
        let src = [c.0, c.1, c.2, 255];
        let a = alpha.clamp(0.0, 1.0) * 65536.0;
        let mut sb = SpanBlender::new(src);
        raster_rect(pw, ph, x0, y0, x1, y1, |off, len, cov| sb.run(&mut data[off * 4..(off + len) * 4], (a * cov) as u32));
    }

    /// Stroke of one path, where overlapping segments must not blend twice (ctx.stroke() fills the
    /// union). Axis-aligned segments are OR-ed into a 4x4 sub-sample bit mask per pixel and
    /// composited once; the rare diagonal ones are blended directly.
    fn stroke_union_fast(&mut self, segs: &[f32], width: f32, ext: f32, c: Rgb, alpha: f32) {
        let (pw, ph) = (self.pm.width() as usize, self.pm.height() as usize);
        if self.cov.len() != pw * ph {
            self.cov = vec![0u16; pw * ph];
        }
        let cov = &mut self.cov;
        let hw = width / 2.0;
        let (mut lo, mut hi) = (usize::MAX, 0usize);
        let (sw, sh) = ((pw * 4) as f32, (ph * 4) as f32);
        // sub-columns a..b of a pixel as a 4-bit mask
        let bits = |a: usize, b: usize| -> u16 { ((1u16 << b) - 1) & !((1u16 << a) - 1) };
        let mut diagonal = false;
        for s in segs.chunks_exact(4) {
            let (x0, y0, x1, y1) = if s[1] == s[3] {
                (s[0].min(s[2]) - ext, s[1] - hw, s[0].max(s[2]) + ext, s[1] + hw)
            } else if s[0] == s[2] {
                (s[0] - hw, s[1].min(s[3]) - ext, s[0] + hw, s[1].max(s[3]) + ext)
            } else {
                diagonal = true;
                continue;
            };
            // sample centres at (i + 0.5) / 4
            let sx0 = (x0 * 4.0 - 0.5).ceil().clamp(0.0, sw) as usize;
            let sx1 = (x1 * 4.0 - 0.5).ceil().clamp(0.0, sw) as usize;
            let sy0 = (y0 * 4.0 - 0.5).ceil().clamp(0.0, sh) as usize;
            let sy1 = (y1 * 4.0 - 0.5).ceil().clamp(0.0, sh) as usize;
            if sx1 <= sx0 || sy1 <= sy0 {
                continue;
            }
            let (px0, px1) = (sx0 / 4, (sx1 - 1) / 4);
            for py in sy0 / 4..=(sy1 - 1) / 4 {
                let (r0, r1) = (sy0.max(py * 4) - py * 4, sy1.min(py * 4 + 4) - py * 4);
                let mut rows = 0u16;
                for r in r0..r1 {
                    rows |= 1 << (4 * r);
                }
                let row = &mut cov[py * pw..(py + 1) * pw];
                if px0 == px1 {
                    row[px0] |= rows * bits(sx0 - px0 * 4, sx1 - px0 * 4);
                } else {
                    row[px0] |= rows * bits(sx0 - px0 * 4, 4);
                    for m in &mut row[px0 + 1..px1] {
                        *m |= rows * 0xf;
                    }
                    row[px1] |= rows * bits(0, sx1 - px1 * 4);
                }
                lo = lo.min(py * pw + px0);
                hi = hi.max(py * pw + px1 + 1);
            }
        }
        let data = self.pm.data_mut();
        let src = [c.0, c.1, c.2, 255];
        let a = alpha.clamp(0.0, 1.0) * 65536.0;
        if lo < hi {
            let a16 = a as u32 / 16;
            for (m, px) in cov[lo..hi].iter_mut().zip(data[lo * 4..hi * 4].chunks_exact_mut(4)) {
                if *m != 0 {
                    blend(px, src, a16 * m.count_ones());
                    *m = 0;
                }
            }
        }
        if diagonal {
            let mut sb = SpanBlender::new(src);
            for s in segs.chunks_exact(4) {
                if s[0] != s[2] && s[1] != s[3] {
                    raster_line(pw, ph, s[0], s[1], s[2], s[3], width, ext, &mut |off, len, cv| {
                        sb.run(&mut data[off * 4..(off + len) * 4], (a * cv) as u32)
                    });
                }
            }
        }
    }

    /// One stroked path made of independent line segments: [x1, y1, x2, y2, ...].
    pub fn stroke_lines(&mut self, segs: &[f32], c: Rgb, alpha: f64, width: f64, square_cap: bool) {
        if !self.prof_on {
            return self.stroke_lines_inner(segs, c, alpha, width, square_cap);
        }
        let t = now();
        let r = self.stroke_lines_inner(segs, c, alpha, width, square_cap);
        self.prof[2] += since(t);
        r
    }

    fn stroke_lines_inner(&mut self, segs: &[f32], c: Rgb, alpha: f64, width: f64, square_cap: bool) {
        if segs.is_empty() || alpha <= 0.0 {
            return;
        }
        if self.fast {
            let ext = if square_cap { width as f32 / 2.0 } else { 0.0 };
            if width > 1.0 {
                self.stroke_union_fast(segs, width as f32, ext, c, alpha as f32);
            } else {
                // hairlines: Skia blits every segment on its own, so overlaps accumulate
                let (pw, ph) = (self.pm.width() as usize, self.pm.height() as usize);
                let data = self.pm.data_mut();
                let src = [c.0, c.1, c.2, 255];
                let a = (alpha as f32).clamp(0.0, 1.0) * 65536.0;
                let mut sb = SpanBlender::new(src);
                for s in segs.chunks_exact(4) {
                    raster_line(pw, ph, s[0], s[1], s[2], s[3], width as f32, ext, &mut |off, len, cov| {
                        sb.run(&mut data[off * 4..(off + len) * 4], (a * cov) as u32)
                    });
                }
            }
            return;
        }
        let mut pb = PathBuilder::with_capacity(segs.len() / 2, segs.len() / 2);
        for s in segs.chunks_exact(4) {
            pb.move_to(s[0], s[1]);
            pb.line_to(s[2], s[3]);
        }
        if let Some(path) = pb.finish() {
            let stroke = Stroke {
                width: width as f32,
                line_cap: if square_cap { LineCap::Square } else { LineCap::Butt },
                ..Stroke::default()
            };
            self.pm.stroke_path(&path, &paint(c, alpha as f32), &stroke, Transform::identity(), None);
        }
    }

    /// One stroked path made of rectangles: [x, y, w, h, ...].
    pub fn stroke_rects(&mut self, rects: &[f32], c: Rgb, alpha: f64, width: f64) {
        if !self.prof_on {
            return self.stroke_rects_inner(rects, c, alpha, width);
        }
        let t = now();
        let r = self.stroke_rects_inner(rects, c, alpha, width);
        self.prof[3] += since(t);
        r
    }

    fn stroke_rects_inner(&mut self, rects: &[f32], c: Rgb, alpha: f64, width: f64) {
        if rects.is_empty() || alpha <= 0.0 {
            return;
        }
        if self.fast {
            // mitred outline = outer rectangle minus inner rectangle, as four bands
            let hw = width as f32 / 2.0;
            for r in rects.chunks_exact(4) {
                let (x0, x1) = (r[0].min(r[0] + r[2]), r[0].max(r[0] + r[2]));
                let (y0, y1) = (r[1].min(r[1] + r[3]), r[1].max(r[1] + r[3]));
                let a = alpha as f32;
                if x1 - x0 <= width as f32 || y1 - y0 <= width as f32 {
                    self.fill_rect_fast(x0 - hw, y0 - hw, x1 + hw, y1 + hw, c, a);
                    continue;
                }
                self.fill_rect_fast(x0 - hw, y0 - hw, x1 + hw, y0 + hw, c, a);
                self.fill_rect_fast(x0 - hw, y1 - hw, x1 + hw, y1 + hw, c, a);
                self.fill_rect_fast(x0 - hw, y0 + hw, x0 + hw, y1 - hw, c, a);
                self.fill_rect_fast(x1 - hw, y0 + hw, x1 + hw, y1 - hw, c, a);
            }
            return;
        }
        let mut pb = PathBuilder::with_capacity(rects.len() * 5 / 4, rects.len());
        for r in rects.chunks_exact(4) {
            pb.move_to(r[0], r[1]);
            pb.line_to(r[0] + r[2], r[1]);
            pb.line_to(r[0] + r[2], r[1] + r[3]);
            pb.line_to(r[0], r[1] + r[3]);
            pb.close();
        }
        if let Some(path) = pb.finish() {
            let stroke = Stroke { width: width as f32, ..Stroke::default() };
            self.pm.stroke_path(&path, &paint(c, alpha as f32), &stroke, Transform::identity(), None);
        }
    }

    pub fn stroke_rect(&mut self, x: f64, y: f64, w: f64, h: f64, c: Rgb, alpha: f64, width: f64) {
        self.stroke_rects(&[x as f32, y as f32, w as f32, h as f32], c, alpha, width);
    }

    pub fn fill_circle(&mut self, cx: f64, cy: f64, r: f64, c: Rgb) {
        if !self.prof_on {
            return self.fill_circle_inner(cx, cy, r, c);
        }
        let t = now();
        let r = self.fill_circle_inner(cx, cy, r, c);
        self.prof[4] += since(t);
        r
    }

    fn fill_circle_inner(&mut self, cx: f64, cy: f64, r: f64, c: Rgb) {
        let mut pb = PathBuilder::new();
        pb.push_circle(cx as f32, cy as f32, r as f32);
        if let Some(path) = pb.finish() {
            self.pm.fill_path(&path, &paint(c, 1.0), FillRule::Winding, Transform::identity(), None);
        }
    }

    fn glyph(&mut self, font: Font, ch: char) -> &Glyph {
        let face = &self.faces[font as usize];
        self.glyphs.entry((font, ch)).or_insert_with(|| load_glyph(face, ch))
    }

    /// Blit one glyph from the coverage cache: 4 sub-pixel x positions, whole-pixel baseline.
    fn blit_glyph(&mut self, font: Font, ch: char, size: f32, x: f32, y: f32, c: Rgb, alpha: f32) {
        let q = (x * 4.0).round() as i32;
        let (ix, sub) = (q.div_euclid(4), q.rem_euclid(4) as u8);
        let iy = y.round() as i32;
        let key = (font, ch, size.to_bits(), sub);
        if !self.masks.contains_key(&key) {
            let m = self.render_glyph(font, ch, size, sub as f32 / 4.0);
            self.masks.insert(key, m);
        }
        let m = &self.masks[&key];
        let (pw, ph) = (self.pm.width() as i32, self.pm.height() as i32);
        let a = (alpha.clamp(0.0, 1.0) * 65536.0) as u32;
        let stride = pw as usize * 4;
        let data = self.pm.data_mut();
        let src = [c.0, c.1, c.2, 255];
        for gy in 0..m.h {
            let py = iy + m.top + gy as i32;
            if py < 0 || py >= ph {
                continue;
            }
            for gx in 0..m.w {
                let px = ix + m.left + gx as i32;
                let cov = m.cov[gy * m.w + gx] as u32;
                if cov == 0 || px < 0 || px >= pw {
                    continue;
                }
                let o = py as usize * stride + px as usize * 4;
                blend(&mut data[o..o + 4], src, (cov * a + 127) / 255);
            }
        }
    }

    fn render_glyph(&mut self, font: Font, ch: char, size: f32, sub: f32) -> GlyphMask {
        let face = &self.faces[font as usize];
        let s = size / face.units_per_em() as f32;
        let g = self.glyphs.entry((font, ch)).or_insert_with(|| load_glyph(face, ch));
        let Some(bb) = face.glyph_bounding_box(g.id) else {
            return GlyphMask { left: 0, top: 0, w: 0, h: 0, cov: Vec::new() };
        };
        let left = (bb.x_min as f32 * s + sub).floor() as i32 - 1;
        let right = (bb.x_max as f32 * s + sub).ceil() as i32 + 1;
        let top = (-(bb.y_max as f32) * s).floor() as i32 - 1;
        let bottom = (-(bb.y_min as f32) * s).ceil() as i32 + 1;
        let (w, h) = ((right - left) as usize, (bottom - top) as usize);
        let (ox, oy) = (sub - left as f32, -top as f32);
        let mut pb = PathBuilder::new();
        push_outline(&mut pb, &g.outline, ox, oy, s);
        let mut cov = vec![0u8; w * h];
        if let (Some(path), Some(mut mask)) = (pb.finish(), tiny_skia::Mask::new(w as u32, h as u32)) {
            mask.fill_path(&path, FillRule::Winding, true, Transform::identity());
            cov.copy_from_slice(mask.data());
        }
        GlyphMask { left, top, w, h, cov }
    }

    /// GPOS pair kerning (x advance of the first glyph), font units.
    fn kern(&mut self, font: Font, a: GlyphId, b: GlyphId) -> f32 {
        let face = &self.faces[font as usize];
        *self.kerns.entry((font, a.0, b.0)).or_insert_with(|| gpos_kern(face, a, b))
    }

    /// Glyph ids and pen positions (px, relative to the origin) plus the total advance.
    fn layout(&mut self, text: &str, font: Font, size: f64) -> (Vec<(char, f32)>, f64) {
        let scale = size as f32 / self.faces[font as usize].units_per_em() as f32;
        let mut out = Vec::with_capacity(text.len());
        let mut pen = 0f32;
        let mut prev: Option<GlyphId> = None;
        for ch in text.chars() {
            let id = self.glyph(font, ch).id;
            if let Some(p) = prev {
                pen += self.kern(font, p, id) * scale;
            }
            out.push((ch, pen));
            pen += self.glyph(font, ch).adv * scale;
            prev = Some(id);
        }
        (out, pen as f64)
    }

    pub fn measure(&mut self, text: &str, font: Font, size: f64) -> f64 {
        self.layout(text, font, size).1
    }

    /// ctx.fillText with textBaseline = alphabetic, textAlign = left. Returns the advance.
    pub fn fill_text(&mut self, text: &str, x: f64, y: f64, font: Font, size: f64, c: Rgb, alpha: f64) -> f64 {
        if !self.prof_on {
            return self.fill_text_inner(text, x, y, font, size, c, alpha);
        }
        let t = now();
        let r = self.fill_text_inner(text, x, y, font, size, c, alpha);
        self.prof[5] += since(t);
        r
    }

    fn fill_text_inner(&mut self, text: &str, x: f64, y: f64, font: Font, size: f64, c: Rgb, alpha: f64) -> f64 {
        let (run, adv) = self.layout(text, font, size);
        let s = size as f32 / self.faces[font as usize].units_per_em() as f32;
        let (x, y) = (x as f32, y as f32);
        if self.fast {
            for &(ch, pen) in &run {
                self.blit_glyph(font, ch, size as f32, x + pen, y, c, alpha as f32);
            }
            return adv;
        }
        let mut pb = PathBuilder::new();
        for (ch, pen) in run {
            let ox = x + pen;
            push_outline(&mut pb, &self.glyphs[&(font, ch)].outline, ox, y, s);
        }
        if let Some(path) = pb.finish() {
            self.pm.fill_path(&path, &paint(c, alpha as f32), FillRule::Winding, Transform::identity(), None);
        }
        adv
    }
}

fn load_glyph(face: &Face, ch: char) -> Glyph {
    let id = face.glyph_index(ch).unwrap_or(GlyphId(0));
    let mut o = Outline(Vec::new());
    face.outline_glyph(id, &mut o);
    Glyph { id, adv: face.glyph_hor_advance(id).unwrap_or(0) as f32, outline: o.0 }
}

fn push_outline(pb: &mut PathBuilder, outline: &[Seg], ox: f32, y: f32, s: f32) {
    for seg in outline {
        match *seg {
            Seg::M(a, b) => pb.move_to(ox + a * s, y - b * s),
            Seg::L(a, b) => pb.line_to(ox + a * s, y - b * s),
            Seg::Q(a, b, e, f) => pb.quad_to(ox + a * s, y - b * s, ox + e * s, y - f * s),
            Seg::C(a, b, e, f, g, h) => pb.cubic_to(ox + a * s, y - b * s, ox + e * s, y - f * s, ox + g * s, y - h * s),
            Seg::Z => pb.close(),
        }
    }
}

/// dst = src * k/65536 + dst * (1 - k/65536), k in 0..=65536
#[inline(always)]
fn blend(px: &mut [u8], src: [u8; 4], k: u32) {
    let k = k.min(65536);
    let ik = 65536 - k;
    px[0] = ((src[0] as u32 * k + px[0] as u32 * ik + 32768) >> 16) as u8;
    px[1] = ((src[1] as u32 * k + px[1] as u32 * ik + 32768) >> 16) as u8;
    px[2] = ((src[2] as u32 * k + px[2] as u32 * ik + 32768) >> 16) as u8;
    px[3] = ((src[3] as u32 * k + px[3] as u32 * ik + 32768) >> 16) as u8;
}

/// Constant-coverage runs of one colour. Uses the legacy Skia 8-bit formula (premultiplied source,
/// destination scaled by 256 - alpha256 and truncated), which is what the browser output shows:
/// stacks of faint rectangles come out slightly darker than exact arithmetic.
struct SpanBlender {
    src: [u8; 4],
    k: u32,
    ik: u16,
    pre: [u16; 16],
}

impl SpanBlender {
    fn new(src: [u8; 4]) -> SpanBlender {
        SpanBlender { src, k: u32::MAX, ik: 0, pre: [0; 16] }
    }

    /// k = alpha * coverage in 0..=65536
    #[inline(always)]
    fn run(&mut self, span: &mut [u8], k: u32) {
        if k >= 65536 {
            for px in span.chunks_exact_mut(4) {
                px.copy_from_slice(&self.src);
            }
            return;
        }
        if k != self.k {
            let a = (k * 255 + 32768) >> 16; // 0..255
            let d255 = |x: u32| ((x + 128 + ((x + 128) >> 8)) >> 8) as u16; // round(x / 255)
            let pre = [d255(self.src[0] as u32 * a), d255(self.src[1] as u32 * a), d255(self.src[2] as u32 * a), a as u16];
            self.pre = std::array::from_fn(|i| pre[i % 4]);
            self.ik = (256 - (a + (a >> 7))) as u16;
            self.k = k;
        }
        let (pre, ik) = (&self.pre, self.ik);
        // 16 bytes at a time so the loop vectorises
        let mut it = span.chunks_exact_mut(16);
        for ch in &mut it {
            for i in 0..16 {
                ch[i] = (pre[i] + ((ch[i] as u16 * ik) >> 8)) as u8;
            }
        }
        for (i, b) in it.into_remainder().iter_mut().enumerate() {
            *b = (pre[i] + ((*b as u16 * ik) >> 8)) as u8;
        }
    }
}

/// Area coverage of an axis-aligned rectangle, clipped to pw x ph. Calls
/// `span(first pixel index, pixel count, coverage 0..1)` for runs of equal coverage.
#[inline(always)]
fn raster_rect(pw: usize, ph: usize, x0: f32, y0: f32, x1: f32, y1: f32, mut span: impl FnMut(usize, usize, f32)) {
    let x0 = x0.max(0.0);
    let y0 = y0.max(0.0);
    let x1 = x1.min(pw as f32);
    let y1 = y1.min(ph as f32);
    if !(x1 > x0 && y1 > y0) {
        return;
    }
    let (ix0, ix1) = (x0 as usize, (x1.ceil() as usize).min(pw));
    let (iy0, iy1) = (y0 as usize, (y1.ceil() as usize).min(ph));
    // horizontal coverage: partial left pixel, full interior, partial right pixel
    let (cl, cr, mid0, mid1);
    if ix1 - ix0 == 1 {
        (cl, cr, mid0, mid1) = (x1 - x0, 0.0, ix0 + 1, ix0 + 1);
    } else {
        let l = (ix0 + 1) as f32 - x0;
        let r = x1 - (ix1 - 1) as f32;
        (cl, cr) = (if l < 1.0 { l } else { 0.0 }, if r < 1.0 { r } else { 0.0 });
        (mid0, mid1) = (if l < 1.0 { ix0 + 1 } else { ix0 }, if r < 1.0 { ix1 - 1 } else { ix1 });
    }
    for iy in iy0..iy1 {
        let cy = (y1.min(iy as f32 + 1.0) - y0.max(iy as f32)).max(0.0);
        let row = iy * pw;
        if cl > 0.0 {
            span(row + ix0, 1, cy * cl);
        }
        if mid1 > mid0 {
            span(row + mid0, mid1 - mid0, cy);
        }
        if cr > 0.0 {
            span(row + ix1 - 1, 1, cy * cr);
        }
    }
}

/// Line of the given width: axis-aligned segments are rectangles, the others are swept column by
/// column (or row by row) with area coverage. `ext` lengthens both ends (square cap).
#[inline(always)]
fn raster_line(pw: usize, ph: usize, mut x0: f32, mut y0: f32, mut x1: f32, mut y1: f32, width: f32, ext: f32, span: &mut impl FnMut(usize, usize, f32)) {
    let hw = width / 2.0;
    if y0 == y1 {
        return raster_rect(pw, ph, x0.min(x1) - ext, y0 - hw, x0.max(x1) + ext, y0 + hw, span);
    }
    if x0 == x1 {
        return raster_rect(pw, ph, x0 - hw, y0.min(y1) - ext, x0 + hw, y0.max(y1) + ext, span);
    }
    let steep = (y1 - y0).abs() > (x1 - x0).abs();
    if steep {
        (x0, y0, x1, y1) = (y0, x0, y1, x1); // sweep along y: swap axes
    }
    if x0 > x1 {
        (x0, y0, x1, y1) = (x1, y1, x0, y0);
    }
    let m = (y1 - y0) / (x1 - x0);
    let len = (1.0 + m * m).sqrt();
    if ext > 0.0 {
        let e = ext / len;
        x0 -= e;
        y0 -= e * m;
        x1 += e;
    }
    // Skia's anti-aliased hairlines cover exactly one pixel across the minor axis, whatever the slope
    let hv = if width <= 1.0 { hw } else { hw * len };
    let (umax, vmax) = if steep { (ph as i32, pw as i32) } else { (pw as i32, ph as i32) };
    let (iu0, iu1) = ((x0.floor() as i32).max(0), (x1.ceil() as i32).min(umax));
    for iu in iu0..iu1 {
        let cu = (x1.min(iu as f32 + 1.0) - x0.max(iu as f32)).max(0.0);
        let uc = (iu as f32 + 0.5).clamp(x0, x1);
        let vc = y0 + m * (uc - x0);
        let (top, bot) = (vc - hv, vc + hv);
        let (iv0, iv1) = ((top.floor() as i32).max(0), (bot.ceil() as i32).min(vmax));
        for iv in iv0..iv1 {
            let cv = (bot.min(iv as f32 + 1.0) - top.max(iv as f32)).max(0.0);
            let off = if steep { iu as usize * pw + iv as usize } else { iv as usize * pw + iu as usize };
            span(off, 1, cu * cv);
        }
    }
}

struct GlyphMask {
    left: i32,
    top: i32,
    w: usize,
    h: usize,
    cov: Vec<u8>,
}

fn gpos_kern(face: &Face, a: GlyphId, b: GlyphId) -> f32 {
    use ttf_parser::gpos::{PairAdjustment, PositioningSubtable};
    let Some(gpos) = face.tables().gpos else { return 0.0 };
    for lookup in gpos.lookups {
        for i in 0..lookup.subtables.len() {
            let Some(PositioningSubtable::Pair(pair)) = lookup.subtables.get::<PositioningSubtable>(i) else {
                continue;
            };
            match pair {
                PairAdjustment::Format1 { coverage, sets } => {
                    if let Some(idx) = coverage.get(a) {
                        if let Some((v1, _)) = sets.get(idx).and_then(|s| s.get(b)) {
                            return v1.x_advance as f32;
                        }
                    }
                }
                PairAdjustment::Format2 { coverage, classes, matrix } => {
                    if coverage.get(a).is_some() {
                        let cls = (classes.0.get(a), classes.1.get(b));
                        if let Some((v1, _)) = matrix.get(cls) {
                            if v1.x_advance != 0 {
                                return v1.x_advance as f32;
                            }
                        }
                    }
                }
            }
        }
    }
    0.0
}

#[cfg(not(target_arch = "wasm32"))]
fn now() -> std::time::Instant {
    std::time::Instant::now()
}
#[cfg(not(target_arch = "wasm32"))]
fn since(t: std::time::Instant) -> f64 {
    t.elapsed().as_secs_f64()
}
#[cfg(target_arch = "wasm32")]
fn now() {}
#[cfg(target_arch = "wasm32")]
fn since(_: ()) -> f64 {
    0.0
}
