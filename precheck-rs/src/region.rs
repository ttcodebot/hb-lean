//! Polygon set ("region") operations on flat, merged data.
//! Booleans are delegated to the `i_overlay` crate; selections use a bbox grid.

use crate::geom::*;
use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay::{ContourDirection, IntOverlayOptions, Overlay, ShapeType};
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::core::solver::Solver;
use i_overlay::i_float::int::point::IntPoint;

pub type Region = Vec<Poly>;
type IShape = Vec<Vec<IntPoint>>;

fn to_ishape(p: &Poly) -> IShape {
    let mut s = Vec::with_capacity(1 + p.holes.len());
    s.push(p.hull.iter().map(|q| IntPoint::new(q.x, q.y)).collect());
    for h in &p.holes {
        s.push(h.iter().map(|q| IntPoint::new(q.x, q.y)).collect());
    }
    s
}

fn from_ishapes(shapes: Vec<IShape>) -> Region {
    let mut out = Vec::with_capacity(shapes.len());
    for s in shapes {
        let mut it = s.into_iter();
        let Some(h) = it.next() else { continue };
        let mut hull: Vec<Pt> = h.into_iter().map(|q| Pt::new(q.x, q.y)).collect();
        normalize_ring(&mut hull, true);
        if hull.is_empty() {
            continue;
        }
        let mut holes = Vec::new();
        for c in it {
            let mut r: Vec<Pt> = c.into_iter().map(|q| Pt::new(q.x, q.y)).collect();
            normalize_ring(&mut r, false);
            if !r.is_empty() {
                holes.push(r);
            }
        }
        holes.sort();
        out.push(Poly { hull, holes });
    }
    out
}

fn overlay(subj: &[Poly], clip: &[Poly], rule: OverlayRule) -> Region {
    let cap = subj.iter().chain(clip.iter()).map(|p| p.num_edges()).sum();
    let opts = IntOverlayOptions { preserve_input_collinear: false, output_direction: ContourDirection::Clockwise, preserve_output_collinear: false, min_output_area: 0, ogc: false };
    let solver = Solver { multithreading: None, ..Solver::AUTO };
    let mut ov = Overlay::<i32>::new_custom(cap, opts, solver);
    for p in subj {
        ov.add_shape(&to_ishape(p), ShapeType::Subject);
    }
    for p in clip {
        ov.add_shape(&to_ishape(p), ShapeType::Clip);
    }
    from_ishapes(ov.overlay(rule, FillRule::NonZero))
}

pub fn merge(raw: &[Poly]) -> Region {
    if raw.is_empty() {
        return vec![];
    }
    overlay(raw, &[], OverlayRule::Subject)
}
/// Merge where most inputs do not touch anything else: connected groups (by bbox) are merged
/// with the boolean engine, isolated boxes pass through.
pub fn merge_boxes(raw: &[Poly]) -> Region {
    let grid = PolyGrid::new(raw);
    let mut scratch = Vec::new();
    let mut out = Vec::new();
    let mut rest = Vec::new();
    for (i, p) in raw.iter().enumerate() {
        grid.query(&p.bbox(), &mut scratch);
        if p.is_box() && scratch.iter().all(|&j| j as usize == i) {
            out.push(p.clone());
        } else {
            rest.push(p.clone());
        }
    }
    out.extend(merge(&rest));
    out
}

/// Per-polygon acceleration for box containment tests against polygons with many edges
/// (merged power rails have > 1000 vertices).
struct Accel {
    grid: Option<crate::check::EdgeGrid>,
    bbox: BBox,
}

impl Accel {
    fn new(p: &Poly) -> Accel {
        let grid = if p.num_edges() > 24 { Some(crate::check::EdgeGrid::new(p.edges().map(|e| (e, 0)).collect(), 2000)) } else { None };
        Accel { grid, bbox: p.bbox() }
    }
}

/// true if the box `bx` lies completely inside polygon `p` (boundary contact allowed)
fn poly_contains_box(p: &Poly, acc: &Accel, bx: &BBox, scratch: &mut Vec<u32>) -> bool {
    if p.is_box() {
        return acc.bbox.contains(bx);
    }
    if !acc.bbox.contains(bx) {
        return false;
    }
    let crosses = |e: &Edge| {
        let eb = e.bbox();
        // an edge passing through the open interior of the box; non-manhattan edges near the
        // box are treated conservatively (the boolean engine decides)
        (eb.x1 < bx.x2 && eb.x2 > bx.x1 && eb.y1 < bx.y2 && eb.y2 > bx.y1) || (!e.is_ortho() && eb.touches(bx))
    };
    let (cx, cy) = ((bx.x1 + bx.x2) as i64, (bx.y1 + bx.y2) as i64); // doubled centre
    match &acc.grid {
        None => !p.edges().any(|e| crosses(&e)) && point_in_poly2(p, cx, cy) > 0,
        Some(g) => {
            g.query(bx, scratch);
            if scratch.iter().any(|&i| crosses(&g.edges[i as usize].0)) {
                return false;
            }
            // ray from the centre along the short side of the polygon bbox
            let vertical = acc.bbox.height() <= acc.bbox.width();
            let ray = if vertical { BBox { x1: bx.x1, x2: bx.x2, y1: acc.bbox.y1, y2: bx.y2 } } else { BBox { x1: acc.bbox.x1, x2: bx.x2, y1: bx.y1, y2: bx.y2 } };
            g.query(&ray, scratch);
            let mut wn = 0i32;
            for &i in scratch.iter() {
                let e = &g.edges[i as usize].0;
                // transpose for the vertical ray so the same half-open rule applies
                let (ax, ay, bx_, by, px, py) = if vertical {
                    (2 * e.p1.y as i64, 2 * e.p1.x as i64, 2 * e.p2.y as i64, 2 * e.p2.x as i64, cy, cx)
                } else {
                    (2 * e.p1.x as i64, 2 * e.p1.y as i64, 2 * e.p2.x as i64, 2 * e.p2.y as i64, cx, cy)
                };
                // crossings of the ray going towards -x from (px, py)
                if (ay <= py) != (by <= py) {
                    let v = (bx_ - ax) * (py - ay) - (by - ay) * (px - ax);
                    let up = by > ay;
                    if (up && v > 0) || (!up && v < 0) {
                        wn += if up { 1 } else { -1 };
                    }
                }
            }
            wn != 0
        }
    }
}

/// Split `a` against `b`: (polygons of a fully inside one b polygon, polygons not overlapping any b bbox, rest, b polygons the rest can touch)
fn classify<'a>(a: &'a [Poly], b: &'a [Poly]) -> (Vec<&'a Poly>, Vec<&'a Poly>, Vec<Poly>, Vec<Poly>) {
    let grid = PolyGrid::new(b);
    let acc: Vec<Accel> = b.iter().map(Accel::new).collect();
    let mut scratch = Vec::new();
    let mut scratch2 = Vec::new();
    let (mut inside, mut outside, mut rest) = (Vec::new(), Vec::new(), Vec::new());
    let mut used = vec![false; b.len()];
    for p in a {
        let bb = p.bbox();
        grid.query(&bb, &mut scratch);
        scratch.retain(|&j| b[j as usize].bbox().overlaps(&bb));
        if scratch.is_empty() {
            outside.push(p);
        } else if p.is_box() && scratch.iter().any(|&j| poly_contains_box(&b[j as usize], &acc[j as usize], &bb, &mut scratch2)) {
            inside.push(p);
        } else {
            for &j in &scratch {
                used[j as usize] = true;
            }
            rest.push(p.clone());
        }
    }
    let bsel = b.iter().zip(used).filter(|(_, u)| *u).map(|(p, _)| p.clone()).collect();
    (inside, outside, rest, bsel)
}

pub fn and(a: &[Poly], b: &[Poly]) -> Region {
    if a.is_empty() || b.is_empty() {
        return vec![];
    }
    let (inside, _, rest, bsel) = classify(a, b);
    let mut out: Region = inside.into_iter().cloned().collect();
    if !rest.is_empty() {
        out.extend(overlay(&rest, &bsel, OverlayRule::Intersect));
    }
    out
}
pub fn not(a: &[Poly], b: &[Poly]) -> Region {
    if a.is_empty() {
        return vec![];
    }
    if b.is_empty() {
        return a.to_vec();
    }
    let (_, outside, rest, bsel) = classify(a, b);
    let mut out: Region = outside.into_iter().cloned().collect();
    if !rest.is_empty() {
        out.extend(overlay(&rest, &bsel, OverlayRule::Difference));
    }
    out
}
pub fn or(a: &[Poly], b: &[Poly]) -> Region {
    if a.is_empty() && b.is_empty() {
        return vec![];
    }
    overlay(a, b, OverlayRule::Union)
}

/// bbox grid over polygons
pub struct PolyGrid<'a> {
    pub polys: &'a [Poly],
    boxes: Vec<BBox>,
    cell: i32,
    x0: i32,
    y0: i32,
    nx: i32,
    ny: i32,
    buckets: Vec<Vec<u32>>,
}

impl<'a> PolyGrid<'a> {
    pub fn new(polys: &'a [Poly]) -> PolyGrid<'a> {
        let boxes: Vec<BBox> = polys.iter().map(|p| p.bbox()).collect();
        let mut bb = BBox::empty();
        for b in &boxes {
            bb.join(b);
        }
        if bb.is_empty() {
            bb = BBox { x1: 0, y1: 0, x2: 1, y2: 1 };
        }
        let n = polys.len().max(1) as f64;
        let area = (bb.width() as f64 + 1.0) * (bb.height() as f64 + 1.0);
        let cell = ((area / n).sqrt() * 2.0).clamp(1000.0, 200000.0) as i32;
        let nx = bb.width() / cell + 1;
        let ny = bb.height() / cell + 1;
        let mut buckets = vec![Vec::new(); (nx as usize) * (ny as usize)];
        for (i, b) in boxes.iter().enumerate() {
            for gy in ((b.y1 - bb.y1) / cell)..=((b.y2 - bb.y1) / cell) {
                for gx in ((b.x1 - bb.x1) / cell)..=((b.x2 - bb.x1) / cell) {
                    buckets[(gy * nx + gx) as usize].push(i as u32);
                }
            }
        }
        PolyGrid { polys, boxes, cell, x0: bb.x1, y0: bb.y1, nx, ny, buckets }
    }
    pub fn query(&self, b: &BBox, out: &mut Vec<u32>) {
        out.clear();
        let gx1 = ((b.x1 - self.x0).div_euclid(self.cell)).clamp(0, self.nx - 1);
        let gx2 = ((b.x2 - self.x0).div_euclid(self.cell)).clamp(0, self.nx - 1);
        let gy1 = ((b.y1 - self.y0).div_euclid(self.cell)).clamp(0, self.ny - 1);
        let gy2 = ((b.y2 - self.y0).div_euclid(self.cell)).clamp(0, self.ny - 1);
        for gy in gy1..=gy2 {
            for gx in gx1..=gx2 {
                for &i in &self.buckets[(gy * self.nx + gx) as usize] {
                    if self.boxes[i as usize].touches(b) {
                        out.push(i);
                    }
                }
            }
        }
        if gx1 != gx2 || gy1 != gy2 {
            out.sort_unstable();
            out.dedup();
        }
    }
}

/// point location in doubled coordinates: 1 inside, 0 on boundary, -1 outside
pub fn point_in_poly2(p: &Poly, x2: i64, y2: i64) -> i32 {
    let mut wn = 0i32;
    for e in p.edges() {
        let (ax, ay, bx, by) = (2 * e.p1.x as i64, 2 * e.p1.y as i64, 2 * e.p2.x as i64, 2 * e.p2.y as i64);
        let v = (bx - ax) * (y2 - ay) - (by - ay) * (x2 - ax);
        if v == 0 && x2 >= ax.min(bx) && x2 <= ax.max(bx) && y2 >= ay.min(by) && y2 <= ay.max(by) {
            return 0;
        }
        if ay <= y2 {
            if by > y2 && v > 0 {
                wn += 1;
            }
        } else if by <= y2 && v < 0 {
            wn -= 1;
        }
    }
    if wn != 0 { 1 } else { -1 }
}

/// closed-set interaction (touching counts)
pub fn polys_interact(a: &Poly, b: &Poly) -> bool {
    if a.is_box() && b.is_box() {
        return a.bbox().touches(&b.bbox());
    }
    let bb = b.bbox();
    let ab = a.bbox();
    for ea in a.edges() {
        if !ea.bbox().touches(&bb) {
            continue;
        }
        for eb in b.edges() {
            if ea.intersect(&eb) {
                return true;
            }
        }
    }
    let pa = a.hull[0];
    let pb = b.hull[0];
    (bb.touches(&BBox { x1: pa.x, y1: pa.y, x2: pa.x, y2: pa.y }) && point_in_poly2(b, 2 * pa.x as i64, 2 * pa.y as i64) >= 0)
        || (ab.touches(&BBox { x1: pb.x, y1: pb.y, x2: pb.x, y2: pb.y }) && point_in_poly2(a, 2 * pb.x as i64, 2 * pb.y as i64) >= 0)
}

/// number of polygons of `b` each polygon of `a` interacts with (touching included)
pub fn interaction_counts(a: &[Poly], b: &[Poly]) -> Vec<u32> {
    let grid = PolyGrid::new(b);
    let mut scratch = Vec::new();
    a.iter()
        .map(|p| {
            grid.query(&p.bbox(), &mut scratch);
            scratch.iter().filter(|&&j| polys_interact(p, &b[j as usize])).count() as u32
        })
        .collect()
}

pub fn interacting(a: &[Poly], b: &[Poly]) -> Region {
    if a.is_empty() || b.is_empty() {
        return vec![];
    }
    let c = interaction_counts(a, b);
    a.iter().zip(c).filter(|(_, n)| *n > 0).map(|(p, _)| p.clone()).collect()
}
pub fn interacting_min(a: &[Poly], b: &[Poly], min: u32) -> Region {
    if a.is_empty() || b.is_empty() {
        return vec![];
    }
    let c = interaction_counts(a, b);
    a.iter().zip(c).filter(|(_, n)| *n >= min).map(|(p, _)| p.clone()).collect()
}
pub fn not_interacting(a: &[Poly], b: &[Poly]) -> Region {
    if b.is_empty() {
        return a.to_vec();
    }
    let c = interaction_counts(a, b);
    a.iter().zip(c).filter(|(_, n)| *n == 0).map(|(p, _)| p.clone()).collect()
}

/// flags: polygon of `a` shares interior area with `b`
pub fn overlap_flags(a: &[Poly], b: &[Poly]) -> Vec<bool> {
    let mut flags = vec![false; a.len()];
    if a.is_empty() || b.is_empty() {
        return flags;
    }
    let both = and(a, b);
    let grid = PolyGrid::new(a);
    let mut scratch = Vec::new();
    for r in &both {
        // a point strictly inside r: midpoint of the first hull edge moved half a dbu to the right
        let e = Edge::new(r.hull[0], r.hull[1]);
        let (mx, my) = ((e.p1.x + e.p2.x) as i64, (e.p1.y + e.p2.y) as i64);
        let (sx, sy) = (e.dy().signum(), -e.dx().signum());
        let (px, py) = (mx + sx, my + sy);
        grid.query(&r.bbox(), &mut scratch);
        for &i in &scratch {
            if !flags[i as usize] && point_in_poly2(&a[i as usize], px, py) > 0 {
                flags[i as usize] = true;
            }
        }
    }
    flags
}
pub fn overlapping(a: &[Poly], b: &[Poly]) -> Region {
    let f = overlap_flags(a, b);
    a.iter().zip(f).filter(|(_, x)| *x).map(|(p, _)| p.clone()).collect()
}
/// polygons of a entirely outside b (touching allowed)
pub fn outside(a: &[Poly], b: &[Poly]) -> Region {
    let f = overlap_flags(a, b);
    a.iter().zip(f).filter(|(_, x)| !*x).map(|(p, _)| p.clone()).collect()
}
pub fn not_outside(a: &[Poly], b: &[Poly]) -> Region {
    overlapping(a, b)
}

/// Manhattan sizing by (dx, dy) (same sign); corners stay square. Result is merged.
pub fn sized(a: &[Poly], dx: i32, dy: i32) -> Region {
    if a.is_empty() || (dx == 0 && dy == 0) {
        return a.to_vec();
    }
    debug_assert!(a.iter().all(|p| p.is_manhattan()));
    let (ax, ay) = (dx.abs(), dy.abs());
    let mut rects = Vec::new();
    for p in a {
        for e in p.edges() {
            let b = e.bbox();
            // only edges that move: horizontal edges move with dy, vertical with dx
            let horizontal = e.p1.y == e.p2.y;
            if (horizontal && ay == 0) || (!horizontal && ax == 0) {
                continue;
            }
            rects.push(Poly::rect(b.x1 - ax, b.y1 - ay, b.x2 + ax, b.y2 + ay));
        }
    }
    if dx >= 0 && dy >= 0 {
        let mut all = a.to_vec();
        all.extend(rects);
        merge(&all)
    } else {
        not(a, &merge(&rects))
    }
}

pub fn edges_of(a: &[Poly]) -> Vec<Edge> {
    a.iter().flat_map(|p| p.edges()).collect()
}

pub fn is_manhattan(a: &[Poly]) -> bool {
    a.iter().all(|p| p.is_manhattan())
}
