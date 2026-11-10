//! Minimal GDSII stream reader: just what the precheck needs.
//! Keeps the hierarchy (cells, shapes per layer, references) and can flatten a layer.

use crate::geom::{Poly, Pt};
use std::collections::HashMap;

pub type LayerKey = (i16, i16);

#[derive(Default, Clone)]
pub struct Trans {
    pub mirror_x: bool, // reflect about x axis before rotation
    pub rot: u8,        // multiples of 90 degrees
    pub mag: f64,
    pub dx: i32,
    pub dy: i32,
}

impl Trans {
    pub fn apply(&self, p: Pt) -> Pt {
        let (mut x, mut y) = (p.x as i64, p.y as i64);
        if self.mirror_x {
            y = -y;
        }
        if self.mag != 1.0 {
            x = (x as f64 * self.mag).round() as i64;
            y = (y as f64 * self.mag).round() as i64;
        }
        let (x, y) = match self.rot & 3 {
            0 => (x, y),
            1 => (-y, x),
            2 => (-x, -y),
            _ => (y, -x),
        };
        Pt::new((x + self.dx as i64) as i32, (y + self.dy as i64) as i32)
    }
    /// self after `inner` (inner applied first)
    pub fn concat(&self, inner: &Trans) -> Trans {
        let o = self.apply(Pt::new(inner.dx, inner.dy));
        let rot = if self.mirror_x { (self.rot + 4 - inner.rot) & 3 } else { (self.rot + inner.rot) & 3 };
        Trans { mirror_x: self.mirror_x ^ inner.mirror_x, rot, mag: self.mag * inner.mag, dx: o.x, dy: o.y }
    }
    pub fn identity() -> Trans {
        Trans { mirror_x: false, rot: 0, mag: 1.0, dx: 0, dy: 0 }
    }
}

pub struct Shape {
    pub layer: LayerKey,
    pub pts: Vec<Pt>, // boundary/box: closed ring without the repeated last point; path: centre line
    pub path: Option<(i32, i16)>, // (width, pathtype)
}

pub struct Text {
    pub layer: LayerKey,
    pub pos: Pt,
    pub string: String,
}

pub struct Ref {
    pub cell: String,
    pub trans: Trans,
    pub cols: i32,
    pub rows: i32,
    pub col_step: (i32, i32),
    pub row_step: (i32, i32),
}

#[derive(Default)]
pub struct Cell {
    pub name: String,
    pub shapes: Vec<Shape>,
    pub texts: Vec<Text>,
    pub refs: Vec<Ref>,
}

#[derive(Default)]
pub struct Library {
    pub dbu_um: f64,
    pub cells: Vec<Cell>,
    pub index: HashMap<String, usize>,
}

fn gds_real8(b: &[u8]) -> f64 {
    let sign = if b[0] & 0x80 != 0 { -1.0 } else { 1.0 };
    let exp = (b[0] & 0x7f) as i32 - 64;
    let mut mant: u64 = 0;
    for &x in &b[1..8] {
        mant = (mant << 8) | x as u64;
    }
    sign * (mant as f64) / 2f64.powi(56) * 16f64.powi(exp)
}

fn i16s(b: &[u8]) -> i16 {
    i16::from_be_bytes([b[0], b[1]])
}
fn i32s(b: &[u8]) -> i32 {
    i32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

pub fn read(data: &[u8]) -> Result<Library, String> {
    let mut lib = Library::default();
    let mut pos = 0usize;
    let mut cell: Option<Cell> = None;
    // element state
    #[derive(PartialEq)]
    enum El {
        None,
        Boundary,
        Path,
        Sref,
        Aref,
        Text,
        Box,
        Other,
    }
    let mut el = El::None;
    let (mut layer, mut dtype) = (0i16, 0i16);
    let mut xy: Vec<Pt> = Vec::new();
    let mut sname = String::new();
    let mut string = String::new();
    let mut tr = Trans::identity();
    let (mut width, mut pathtype) = (0i32, 0i16);
    let (mut cols, mut rows) = (1i32, 1i32);
    while pos + 4 <= data.len() {
        let len = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
        if len < 4 {
            break;
        }
        let rec = data[pos + 2];
        let body = &data[pos + 4..pos + len];
        pos += len;
        match rec {
            0x03 => lib.dbu_um = gds_real8(&body[0..8]), // UNITS: user units per dbu
            0x05 => cell = Some(Cell::default()),         // BGNSTR
            0x06 => {
                let s = String::from_utf8_lossy(body).trim_end_matches('\0').to_string();
                if let Some(c) = cell.as_mut() {
                    c.name = s;
                }
            }
            0x07 => {
                let c = cell.take().ok_or("ENDSTR without BGNSTR")?;
                lib.index.insert(c.name.clone(), lib.cells.len());
                lib.cells.push(c);
            }
            0x08 | 0x09 | 0x0a | 0x0b | 0x0c | 0x2d | 0x15 => {
                el = match rec {
                    0x08 => El::Boundary,
                    0x09 => El::Path,
                    0x0a => El::Sref,
                    0x0b => El::Aref,
                    0x0c => El::Text,
                    0x2d => El::Box,
                    _ => El::Other,
                };
                layer = 0;
                dtype = 0;
                xy.clear();
                tr = Trans::identity();
                width = 0;
                pathtype = 0;
                cols = 1;
                rows = 1;
                string.clear();
            }
            0x0d => layer = i16s(body),
            0x0e | 0x16 | 0x2e => dtype = i16s(body), // DATATYPE / TEXTTYPE / BOXTYPE
            0x0f => width = i32s(body),
            0x21 => pathtype = i16s(body),
            0x10 => {
                xy.clear();
                for c in body.chunks_exact(8) {
                    xy.push(Pt::new(i32s(&c[0..4]), i32s(&c[4..8])));
                }
            }
            0x12 => sname = String::from_utf8_lossy(body).trim_end_matches('\0').to_string(),
            0x13 => {
                cols = i16s(&body[0..2]) as i32;
                rows = i16s(&body[2..4]) as i32;
            }
            0x19 => string = String::from_utf8_lossy(body).trim_end_matches('\0').to_string(),
            0x1a => tr.mirror_x = body[0] & 0x80 != 0,
            0x1b => tr.mag = gds_real8(body),
            0x1c => {
                let a = gds_real8(body);
                let q = (a / 90.0).round();
                if (a - q * 90.0).abs() > 1e-9 {
                    return Err(format!("non-orthogonal reference angle {a}"));
                }
                tr.rot = (q as i64).rem_euclid(4) as u8;
            }
            0x11 => {
                let c = cell.as_mut().ok_or("element outside structure")?;
                match el {
                    El::Boundary | El::Box => {
                        if xy.len() > 1 && xy.first() == xy.last() {
                            xy.pop();
                        }
                        c.shapes.push(Shape { layer: (layer, dtype), pts: xy.clone(), path: None });
                    }
                    El::Path => c.shapes.push(Shape { layer: (layer, dtype), pts: xy.clone(), path: Some((width, pathtype)) }),
                    El::Text => c.texts.push(Text { layer: (layer, dtype), pos: xy[0], string: string.clone() }),
                    El::Sref => {
                        tr.dx = xy[0].x;
                        tr.dy = xy[0].y;
                        c.refs.push(Ref { cell: sname.clone(), trans: tr.clone(), cols: 1, rows: 1, col_step: (0, 0), row_step: (0, 0) });
                    }
                    El::Aref => {
                        tr.dx = xy[0].x;
                        tr.dy = xy[0].y;
                        let cs = ((xy[1].x - xy[0].x) / cols.max(1), (xy[1].y - xy[0].y) / cols.max(1));
                        let rs = ((xy[2].x - xy[0].x) / rows.max(1), (xy[2].y - xy[0].y) / rows.max(1));
                        c.refs.push(Ref { cell: sname.clone(), trans: tr.clone(), cols, rows, col_step: cs, row_step: rs });
                    }
                    _ => {}
                }
                el = El::None;
            }
            0x04 => break, // ENDLIB
            _ => {}
        }
    }
    Ok(lib)
}

/// Polygon outline of a path (pathtype 0 = flush, 2 = extended by half width; 1 treated as 2).
pub fn path_to_poly(pts: &[Pt], width: i32, pathtype: i16) -> Option<Vec<Pt>> {
    if pts.len() < 2 || width <= 0 {
        return None;
    }
    let h = width as f64 / 2.0;
    let ext = if pathtype == 0 { 0.0 } else { h };
    let n = pts.len();
    let mut left = Vec::with_capacity(n);
    let mut right = Vec::with_capacity(n);
    let dir = |a: Pt, b: Pt| {
        let (dx, dy) = ((b.x - a.x) as f64, (b.y - a.y) as f64);
        let l = (dx * dx + dy * dy).sqrt();
        (dx / l, dy / l)
    };
    for i in 0..n {
        let p = (pts[i].x as f64, pts[i].y as f64);
        if i == 0 || i == n - 1 {
            let d = if i == 0 { dir(pts[0], pts[1]) } else { dir(pts[n - 2], pts[n - 1]) };
            let s = if i == 0 { -ext } else { ext };
            let c = (p.0 + d.0 * s, p.1 + d.1 * s);
            left.push((c.0 - d.1 * h, c.1 + d.0 * h));
            right.push((c.0 + d.1 * h, c.1 - d.0 * h));
        } else {
            let d1 = dir(pts[i - 1], pts[i]);
            let d2 = dir(pts[i], pts[i + 1]);
            // miter join
            let (nx, ny) = (-(d1.1 + d2.1), d1.0 + d2.0);
            let nl = (nx * nx + ny * ny).sqrt();
            let (nx, ny) = (nx / nl, ny / nl);
            let cosh = nx * -d1.1 + ny * d1.0;
            let m = h / cosh;
            left.push((p.0 + nx * m, p.1 + ny * m));
            right.push((p.0 - nx * m, p.1 - ny * m));
        }
    }
    let mut out: Vec<Pt> = left.iter().map(|p| Pt::new(p.0.round() as i32, p.1.round() as i32)).collect();
    out.extend(right.iter().rev().map(|p| Pt::new(p.0.round() as i32, p.1.round() as i32)));
    Some(out)
}

impl Library {
    pub fn top_cells(&self) -> Vec<&Cell> {
        let mut used = std::collections::HashSet::new();
        for c in &self.cells {
            for r in &c.refs {
                used.insert(r.cell.as_str());
            }
        }
        self.cells.iter().filter(|c| !used.contains(c.name.as_str())).collect()
    }

    /// Flatten all polygons of all layers below `top` into per-layer raw polygon lists.
    pub fn flatten(&self, top: &str) -> HashMap<LayerKey, Vec<Poly>> {
        let mut out: HashMap<LayerKey, Vec<Poly>> = HashMap::new();
        let mut stack: Vec<(usize, Trans)> = vec![(self.index[top], Trans::identity())];
        while let Some((ci, t)) = stack.pop() {
            let c = &self.cells[ci];
            for s in &c.shapes {
                let ring = match s.path {
                    None => Some(s.pts.clone()),
                    Some((w, pt)) => path_to_poly(&s.pts, w, pt),
                };
                if let Some(r) = ring {
                    if r.len() >= 3 {
                        let pts: Vec<Pt> = r.iter().map(|p| t.apply(*p)).collect();
                        out.entry(s.layer).or_default().push(Poly::from_ring(pts));
                    }
                }
            }
            for r in &c.refs {
                let Some(&ri) = self.index.get(&r.cell) else { continue };
                for i in 0..r.cols.max(1) {
                    for j in 0..r.rows.max(1) {
                        let mut inner = r.trans.clone();
                        inner.dx += i * r.col_step.0 + j * r.row_step.0;
                        inner.dy += i * r.col_step.1 + j * r.row_step.1;
                        stack.push((ri, t.concat(&inner)));
                    }
                }
            }
        }
        out
    }

    /// Bounding box of everything below `top` (polygons and paths), without flattening.
    pub fn bbox(&self, top: &str) -> crate::geom::BBox {
        fn rec(lib: &Library, ci: usize, memo: &mut Vec<Option<crate::geom::BBox>>) -> crate::geom::BBox {
            if let Some(b) = memo[ci] {
                return b;
            }
            let c = &lib.cells[ci];
            let mut bb = crate::geom::BBox::empty();
            for s in &c.shapes {
                let ring = match s.path {
                    None => Some(s.pts.clone()),
                    Some((w, pt)) => path_to_poly(&s.pts, w, pt),
                };
                for p in ring.iter().flatten() {
                    bb.add(*p);
                }
            }
            for r in &c.refs {
                let Some(&ri) = lib.index.get(&r.cell) else { continue };
                let cb = rec(lib, ri, memo);
                if cb.is_empty() {
                    continue;
                }
                for (i, j) in [(0, 0), (r.cols.max(1) - 1, 0), (0, r.rows.max(1) - 1), (r.cols.max(1) - 1, r.rows.max(1) - 1)] {
                    let mut t = r.trans.clone();
                    t.dx += i * r.col_step.0 + j * r.row_step.0;
                    t.dy += i * r.col_step.1 + j * r.row_step.1;
                    for p in [Pt::new(cb.x1, cb.y1), Pt::new(cb.x1, cb.y2), Pt::new(cb.x2, cb.y1), Pt::new(cb.x2, cb.y2)] {
                        bb.add(t.apply(p));
                    }
                }
            }
            memo[ci] = Some(bb);
            bb
        }
        let mut memo = vec![None; self.cells.len()];
        rec(self, self.index[top], &mut memo)
    }

    /// All texts below `top`, flattened: (layer, position, string)
    pub fn flat_texts(&self, top: &str) -> Vec<(LayerKey, Pt, String)> {
        let mut out = Vec::new();
        let mut stack: Vec<(usize, Trans)> = vec![(self.index[top], Trans::identity())];
        while let Some((ci, t)) = stack.pop() {
            let c = &self.cells[ci];
            for x in &c.texts {
                out.push((x.layer, t.apply(x.pos), x.string.clone()));
            }
            for r in &c.refs {
                let Some(&ri) = self.index.get(&r.cell) else { continue };
                for i in 0..r.cols.max(1) {
                    for j in 0..r.rows.max(1) {
                        let mut inner = r.trans.clone();
                        inner.dx += i * r.col_step.0 + j * r.row_step.0;
                        inner.dy += i * r.col_step.1 + j * r.row_step.1;
                        stack.push((ri, t.concat(&inner)));
                    }
                }
            }
        }
        out
    }
}
