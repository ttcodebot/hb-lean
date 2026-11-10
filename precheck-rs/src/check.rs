//! Edge-to-edge DRC checks: a port of KLayout's EdgeRelationFilter (dbEdgePairRelations.cc)
//! and the edge pair bookkeeping around it (shielding), evaluated flat with a uniform grid.

use crate::geom::*;
use std::collections::{HashMap, HashSet};

const EPS: f64 = 1e-5; // db::epsilon

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Relation {
    Width,
    Space,
    Overlap,
    Inside, // "enclosed": first inside second
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Metrics {
    Euclidian,
    Square,
    Projection,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ZeroMode {
    Never,
    Always,
    WhenTouching,
    WhenCollinearAndTouching,
    WhenOverlapping,
}

#[derive(Clone, Copy, Debug)]
pub struct CheckOpts {
    pub rel: Relation,
    pub d: i32,
    pub metrics: Metrics,
    pub ignore_angle: f64,
    pub min_projection: i64,
    pub max_projection: i64,
    pub zero: ZeroMode,
    pub shielded: bool,
    pub whole_edges: bool,
}

impl CheckOpts {
    pub fn new(rel: Relation, d: i32, metrics: Metrics) -> CheckOpts {
        CheckOpts { rel, d, metrics, ignore_angle: 90.0, min_projection: 0, max_projection: i64::MAX, zero: ZeroMode::WhenTouching, shielded: true, whole_edges: false }
    }
}

fn edge_projection(a: &Edge, b: &Edge) -> i64 {
    if a.is_degenerate() || b.is_degenerate() {
        return 0;
    }
    let al = a.sq_len();
    let sp = |p: Pt| ((p.x - a.p1.x) as i64 * a.dx() + (p.y - a.p1.y) as i64 * a.dy()) as f64 / al;
    let l1 = sp(b.p1).clamp(0.0, 1.0);
    let l2 = sp(b.p2).clamp(0.0, 1.0);
    rounded(a.len() * (l2 - l1).abs()) as i64
}

fn include_zero_flag(zd: ZeroMode, a: &Edge, b: &Edge) -> bool {
    match zd {
        ZeroMode::Always => true,
        ZeroMode::Never => false,
        _ => {
            let s1 = a.side_of(b.p1);
            let s2 = a.side_of(b.p2);
            if s1 == 0 && s2 == 0 {
                if zd == ZeroMode::WhenTouching || zd == ZeroMode::WhenCollinearAndTouching {
                    return a.intersect(b);
                } else if zd == ZeroMode::WhenOverlapping {
                    return a.coincident(b);
                }
            } else if (s1 == 0 || s2 == 0) && a.p1 != b.p2 && a.p2 != b.p1 {
                if zd == ZeroMode::WhenTouching {
                    return a.intersect(b);
                }
            }
            false
        }
    }
}

fn pt_along(g: &Edge, l: f64) -> Pt {
    Pt::new(g.p1.x + rounded(g.dx() as f64 * l), g.p1.y + rounded(g.dy() as f64 * l))
}

fn fv(ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    ax * by - ay * bx
}

/// part of `other` on the inside (right) side of `e` within distance d; Euclidian metrics
fn euclidian_near_part(zd: ZeroMode, d: i32, e: &Edge, other: &Edge) -> Option<Edge> {
    let mut g = *other;
    let (s1, s2);
    if e.is_degenerate() {
        if g.contains(e.p1) {
            s1 = 0;
            s2 = 0;
        } else {
            s1 = -1;
            s2 = -1;
        }
    } else {
        s1 = e.side_of(g.p1);
        s2 = e.side_of(g.p2);
    }
    let thr = if include_zero_flag(zd, e, &g) { 0 } else { -1 };
    if s1 > thr && s2 > thr {
        return None;
    } else if s2 > thr {
        g = Edge::new(g.p1, g.cut_point(e)?);
    } else if s1 > thr {
        g = Edge::new(g.cut_point(e)?, g.p2);
    }
    let d_f = d as f64;
    if g.is_degenerate() {
        if e.is_degenerate() {
            return if e.p1.dist(g.p1) < d_f { Some(g) } else { None };
        }
        let o = g.p1;
        if e.side_of(o) > thr {
            return None;
        }
        let a = e.sq_len();
        let b = ((e.p1.x - o.x) as i64 * e.dx() + (e.p1.y - o.y) as i64 * e.dy()) as f64 / a;
        let c = (e.p1.sq_dist(o) - d_f * d_f) / a;
        let s = b * b - c;
        if s >= -EPS {
            let sq = s.max(0.0).sqrt();
            let l1 = (0.0f64).max(-b - sq);
            let l2 = (1.0f64).min(-b + sq);
            if l1 <= l2 {
                return Some(g);
            }
        }
        return None;
    }

    let (mut l1, mut l2) = (f64::MAX, -f64::MAX);
    if e.parallel(&g) {
        if (e.distance(g.p1) as f64).abs() >= d_f {
            return None;
        }
    } else {
        let ef = 1.0 / e.len();
        let (enx, eny) = (ef * e.dy() as f64, -ef * e.dx() as f64);
        let (e1x, e1y) = (e.p1.x as f64 + enx * d_f, e.p1.y as f64 + eny * d_f);
        let (gdx, gdy, edx, edy) = (g.dx() as f64, g.dy() as f64, e.dx() as f64, e.dy() as f64);
        let det = fv(gdx, gdy, edx, edy);
        let mut lp1 = fv(e1x - g.p1.x as f64, e1y - g.p1.y as f64, edx, edy) / det;
        let mut lp2 = fv((e.p1.x - g.p1.x) as f64, (e.p1.y - g.p1.y) as f64, edx, edy) / det;
        if lp1 > lp2 {
            std::mem::swap(&mut lp1, &mut lp2);
        }
        if e.sprod_sign(&g) == 0 {
            if g.side_of(e.p1) * g.side_of(e.p2) <= 0 {
                l1 = lp1;
                l2 = lp2;
            }
        } else {
            let det = fv(gdx, gdy, enx, eny);
            let mut lt1 = fv((e.p1.x - g.p1.x) as f64, (e.p1.y - g.p1.y) as f64, enx, eny) / det;
            let mut lt2 = fv((e.p2.x - g.p1.x) as f64, (e.p2.y - g.p1.y) as f64, enx, eny) / det;
            if lt1 > lt2 {
                std::mem::swap(&mut lt1, &mut lt2);
            }
            let ll1 = lp1.max(lt1);
            let ll2 = lp2.min(lt2);
            if ll1 <= ll2 {
                l1 = ll1;
                l2 = ll2;
            }
        }
    }
    for i in 0..2 {
        let o = if i == 1 { e.p2 } else { e.p1 };
        let a = g.sq_len();
        let b = ((g.p1.x - o.x) as i64 * g.dx() + (g.p1.y - o.y) as i64 * g.dy()) as f64 / a;
        let c = (g.p1.sq_dist(o) - d_f * d_f) / a;
        let s = b * b - c;
        if s >= -EPS {
            let sq = s.sqrt(); // NaN for slightly negative s, as in the original
            let (c1, c2) = (-b - sq, -b + sq);
            if !c1.is_nan() {
                l1 = l1.min(c1);
                l2 = l2.max(c2);
            }
        }
    }
    l1 = l1.max(0.0);
    l2 = l2.min(1.0);
    if l1 > l2 - EPS {
        None
    } else {
        Some(Edge::new(pt_along(&g, l1), pt_along(&g, l2)))
    }
}

/// Projection (dd = 0) or Square (dd = d) metrics
fn var_near_part(zd: ZeroMode, d: i32, dd: i32, e: &Edge, other: &Edge) -> Option<Edge> {
    let mut g = *other;
    let (s1, s2);
    if e.is_degenerate() {
        if g.contains(e.p1) {
            s1 = 0;
            s2 = 0;
        } else {
            s1 = -1;
            s2 = -1;
        }
    } else {
        s1 = e.side_of(g.p1);
        s2 = e.side_of(g.p2);
    }
    let thr = if include_zero_flag(zd, e, &g) { 0 } else { -1 };
    if s1 > thr && s2 > thr {
        return None;
    } else if s2 > thr {
        g = Edge::new(g.p1, g.cut_point(e)?);
    } else if s1 > thr {
        g = Edge::new(g.cut_point(e)?, g.p2);
    }
    let (d_f, dd_f) = (d as f64, dd as f64);
    let sp = |ax: i64, ay: i64| (ax * e.dx() + ay * e.dy()) as f64;
    if g.is_degenerate() {
        if !e.is_degenerate() {
            if e.side_of(g.p1) > thr {
                return None;
            }
            if sp((g.p1.x - e.p1.x) as i64, (g.p1.y - e.p1.y) as i64) < -(dd_f * e.len()) {
                return None;
            }
            if sp((e.p2.x - g.p1.x) as i64, (e.p2.y - g.p1.y) as i64) < -(dd_f * e.len()) {
                return None;
            }
            if (e.distance(g.p1) as f64) <= -d_f {
                return None;
            }
        } else if e.p1.dist(g.p1) >= d_f {
            return None;
        }
        return Some(g);
    }
    let (mut l1, mut l2) = (f64::MIN, f64::MAX);
    let (mut epx, mut epy, mut enx, mut eny) = (0.0, 0.0, 0.0, 0.0);
    if !e.is_degenerate() {
        let ef = 1.0 / e.len();
        epx = ef * e.dx() as f64;
        epy = ef * e.dy() as f64;
        enx = ef * e.dy() as f64;
        eny = -ef * e.dx() as f64;
    }
    let (gdx, gdy, edx, edy) = (g.dx() as f64, g.dy() as f64, e.dx() as f64, e.dy() as f64);
    if e.parallel(&g) {
        if (e.distance(g.p1) as f64).abs() >= d_f {
            return None;
        }
    } else {
        let (e1x, e1y) = (e.p1.x as f64 + enx * d_f, e.p1.y as f64 + eny * d_f);
        let det = fv(gdx, gdy, edx, edy);
        let mut lp1 = fv(e1x - g.p1.x as f64, e1y - g.p1.y as f64, edx, edy) / det;
        let mut lp2 = fv((e.p1.x - g.p1.x) as f64, (e.p1.y - g.p1.y) as f64, edx, edy) / det;
        if lp1 > lp2 {
            std::mem::swap(&mut lp1, &mut lp2);
        }
        l1 = lp1;
        l2 = lp2;
    }
    let mut allow_zero_projection = false;
    if e.sprod_sign(&g) == 0 {
        if !g.is_degenerate() {
            if sp((g.p1.x - e.p1.x) as i64, (g.p1.y - e.p1.y) as i64) < -(dd_f * e.len())
                || sp((e.p2.x - g.p1.x) as i64, (e.p2.y - g.p1.y) as i64) < -(dd_f * e.len())
            {
                return None;
            }
            let l = ((e.p1.x - g.p1.x) as f64 * gdx + (e.p1.y - g.p1.y) as f64 * gdy) / g.sq_len();
            let dl = dd_f / g.len();
            l1 = l - dl;
            l2 = l + dl;
            allow_zero_projection = true;
        }
    } else {
        let det = fv(gdx, gdy, enx, eny);
        let mut lt1 = fv((e.p1.x - g.p1.x) as f64 - epx * dd_f, (e.p1.y - g.p1.y) as f64 - epy * dd_f, enx, eny) / det;
        let mut lt2 = fv((e.p2.x - g.p1.x) as f64 + epx * dd_f, (e.p2.y - g.p1.y) as f64 + epy * dd_f, enx, eny) / det;
        if lt1 > lt2 {
            std::mem::swap(&mut lt1, &mut lt2);
        }
        l1 = l1.max(lt1);
        l2 = l2.min(lt2);
    }
    l1 = l1.max(0.0);
    l2 = l2.min(1.0);
    let fail = if allow_zero_projection { l1 > l2 + EPS } else { l1 > l2 - EPS };
    if fail { None } else { Some(Edge::new(pt_along(&g, l1), pt_along(&g, l2))) }
}

/// EdgeRelationFilter::check
pub fn check_edges(o: &CheckOpts, a: &Edge, b: &Edge) -> Option<EdgePair> {
    if o.min_projection > 0 || o.max_projection < i64::MAX {
        let p = edge_projection(a, b);
        if !(p >= o.min_projection && p < o.max_projection) {
            let p = edge_projection(b, a);
            if !(p >= o.min_projection && p < o.max_projection) {
                return None;
            }
        }
    }
    let mut aa = *a;
    if o.rel == Relation::Overlap || o.rel == Relation::Inside {
        aa = aa.swapped();
    }
    if a.is_degenerate() || b.is_degenerate() {
        if a == b && matches!(o.zero, ZeroMode::Never | ZeroMode::WhenCollinearAndTouching | ZeroMode::WhenOverlapping) {
            return None;
        }
    } else if o.ignore_angle == 90.0 {
        if aa.sprod_sign(b) >= 0 {
            return None;
        }
    } else {
        let cosv = (o.ignore_angle * std::f64::consts::PI / 180.0).cos();
        let sp = (aa.dx() * b.dx() + aa.dy() * b.dy()) as f64;
        if -sp / (aa.len() * b.len()) < cosv + 1e-10 {
            return None;
        }
    }
    let mut an = *a;
    let mut bn = *b;
    if o.rel == Relation::Space || o.rel == Relation::Inside {
        an = an.swapped();
    }
    if o.rel == Relation::Space || o.rel == Relation::Overlap {
        bn = bn.swapped();
    }
    let (in2, in1) = match o.metrics {
        Metrics::Euclidian => (euclidian_near_part(o.zero, o.d, &an, &bn), euclidian_near_part(o.zero, o.d, &bn, &an)),
        Metrics::Square => (var_near_part(o.zero, o.d, o.d, &an, &bn), var_near_part(o.zero, o.d, o.d, &bn, &an)),
        Metrics::Projection => (var_near_part(o.zero, o.d, 0, &an, &bn), var_near_part(o.zero, o.d, 0, &bn, &an)),
    };
    let (mut first, mut second) = (in1?, in2?);
    if o.whole_edges {
        return Some(EdgePair { first: *a, second: *b });
    }
    if o.rel == Relation::Space || o.rel == Relation::Inside {
        first = first.swapped();
    }
    if o.rel == Relation::Space || o.rel == Relation::Overlap {
        second = second.swapped();
    }
    Some(EdgePair { first, second })
}

fn shields(ep: &EdgePair, q: &Edge) -> bool {
    let pe1 = Edge::new(ep.first.p1, ep.second.p2);
    let pe2 = Edge::new(ep.second.p1, ep.first.p2);
    match (pe1.intersect_point(q), pe2.intersect_point(q)) {
        (Some(i1), Some(i2)) => i1 != i2 || (pe1.side_of(q.p1) != 0 && pe2.side_of(q.p2) != 0),
        _ => false,
    }
}

/// Uniform grid over tagged edges.
pub struct EdgeGrid {
    cell: i32,
    x0: i32,
    y0: i32,
    nx: i32,
    ny: i32,
    buckets: Vec<Vec<u32>>,
    pub edges: Vec<(Edge, u32)>, // edge, owner polygon id
}

impl EdgeGrid {
    pub fn new(edges: Vec<(Edge, u32)>, cell: i32) -> EdgeGrid {
        let mut bb = BBox::empty();
        for (e, _) in &edges {
            bb.join(&e.bbox());
        }
        if bb.is_empty() {
            bb = BBox { x1: 0, y1: 0, x2: 1, y2: 1 };
        }
        let cell = cell.max(1);
        let nx = (bb.width() / cell + 1).max(1);
        let ny = (bb.height() / cell + 1).max(1);
        let mut buckets = vec![Vec::new(); (nx as usize) * (ny as usize)];
        for (i, (e, _)) in edges.iter().enumerate() {
            let b = e.bbox();
            for gy in ((b.y1 - bb.y1) / cell)..=((b.y2 - bb.y1) / cell) {
                for gx in ((b.x1 - bb.x1) / cell)..=((b.x2 - bb.x1) / cell) {
                    buckets[(gy * nx + gx) as usize].push(i as u32);
                }
            }
        }
        EdgeGrid { cell, x0: bb.x1, y0: bb.y1, nx, ny, buckets, edges }
    }
    /// indices of edges whose bbox touches `b` (may contain duplicates removed by caller-provided scratch)
    pub fn query(&self, b: &BBox, out: &mut Vec<u32>) {
        out.clear();
        let gx1 = ((b.x1 - self.x0).div_euclid(self.cell)).clamp(0, self.nx - 1);
        let gx2 = ((b.x2 - self.x0).div_euclid(self.cell)).clamp(0, self.nx - 1);
        let gy1 = ((b.y1 - self.y0).div_euclid(self.cell)).clamp(0, self.ny - 1);
        let gy2 = ((b.y2 - self.y0).div_euclid(self.cell)).clamp(0, self.ny - 1);
        for gy in gy1..=gy2 {
            for gx in gx1..=gx2 {
                for &i in &self.buckets[(gy * self.nx + gx) as usize] {
                    if self.edges[i as usize].0.bbox().touches(b) {
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

fn grid_cell_for(n: usize, d: i32) -> i32 {
    let _ = n;
    (d * 2).max(2000)
}

/// Apply shielding to edge pairs found between edges of grid(s).
/// `cands(i)` yields every edge that KLayout's scanner would also see next to the pair's edges.
fn apply_shielding(o: &CheckOpts, eps: Vec<(EdgePair, Edge, Edge)>, grids: &[&EdgeGrid]) -> Vec<EdgePair> {
    if !o.shielded {
        return eps.into_iter().map(|x| x.0).collect();
    }
    let mut out = Vec::with_capacity(eps.len());
    let mut scratch = Vec::new();
    'ep: for (ep, e1, e2) in eps {
        let n = ep.normalized();
        // a shielding edge has to cut both connecting edges of the pair, so it touches the pair's bbox
        // (and is then within d of both original edges, i.e. KLayout's scanner sees it too)
        let bb = n.bbox();
        for g in grids {
            g.query(&bb, &mut scratch);
            for &qi in &scratch {
                let q = &g.edges[qi as usize].0;
                if q == &e1 || q == &e2 {
                    continue;
                }
                if shields(&n, q) {
                    continue 'ep;
                }
            }
        }
        out.push(ep);
    }
    out
}

/// Single-layer check (width: same polygon only; space: any two edges).
pub fn single_layer_check(o: &CheckOpts, polys: &[Poly]) -> Vec<EdgePair> {
    let mut edges = Vec::new();
    for (pi, p) in polys.iter().enumerate() {
        for e in p.edges() {
            edges.push((e, pi as u32));
        }
    }
    let grid = EdgeGrid::new(edges, grid_cell_for(polys.len(), o.d));
    let mut found: Vec<(EdgePair, Edge, Edge)> = Vec::new();
    let mut scratch = Vec::new();
    let width = o.rel == Relation::Width;
    for (i, (a, pa)) in grid.edges.iter().enumerate() {
        let bb = a.bbox().enlarged(o.d);
        grid.query(&bb, &mut scratch);
        for &j in &scratch {
            if (j as usize) <= i {
                continue;
            }
            let (b, pb) = &grid.edges[j as usize];
            if width && pa != pb {
                continue;
            }
            if let Some(ep) = check_edges(o, a, b) {
                found.push((ep, *a, *b));
            }
        }
    }
    let res = apply_shielding(o, found, &[&grid]);
    dedup_pairs(res)
}

/// Two-layer check: subject edges (first) against other edges (second).
pub fn two_layer_check(o: &CheckOpts, subj: &[Poly], other: &[Poly]) -> Vec<EdgePair> {
    let mk = |polys: &[Poly]| {
        let mut edges = Vec::new();
        for (pi, p) in polys.iter().enumerate() {
            for e in p.edges() {
                edges.push((e, pi as u32));
            }
        }
        EdgeGrid::new(edges, grid_cell_for(polys.len(), o.d))
    };
    let gs = mk(subj);
    let go = mk(other);
    let mut found = Vec::new();
    let mut scratch = Vec::new();
    for (a, _) in gs.edges.iter() {
        let bb = a.bbox().enlarged(o.d);
        go.query(&bb, &mut scratch);
        for &j in &scratch {
            let (b, _) = &go.edges[j as usize];
            if let Some(ep) = check_edges(o, a, b) {
                found.push((ep, *a, *b));
            }
        }
    }
    let res = apply_shielding(o, found, &[&gs, &go]);
    dedup_pairs_ordered(res)
}

/// Edge-collection check (Edges vs Edges, no shielding, no polygons)
pub fn edges_check(o: &CheckOpts, subj: &[Edge], other: &[Edge]) -> Vec<EdgePair> {
    let go = EdgeGrid::new(other.iter().map(|e| (*e, 0)).collect(), grid_cell_for(other.len(), o.d));
    let mut out = Vec::new();
    let mut scratch = Vec::new();
    for a in subj {
        go.query(&a.bbox().enlarged(o.d), &mut scratch);
        for &j in &scratch {
            if let Some(ep) = check_edges(o, a, &go.edges[j as usize].0) {
                out.push(ep);
            }
        }
    }
    dedup_pairs_ordered(out)
}

fn dedup_pairs(v: Vec<EdgePair>) -> Vec<EdgePair> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for ep in v {
        // symmetric pairs: (first, second) == (second, first)
        let k = if ep.first <= ep.second { (ep.first, ep.second) } else { (ep.second, ep.first) };
        if seen.insert(k) {
            out.push(ep);
        }
    }
    out
}

fn dedup_pairs_ordered(v: Vec<EdgePair>) -> Vec<EdgePair> {
    let mut seen = HashSet::new();
    v.into_iter().filter(|ep| seen.insert(*ep)).collect()
}

#[allow(dead_code)]
pub fn count_by_poly(_m: &HashMap<u32, u32>) {}
