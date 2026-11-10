//! Edge collection operations (KLayout "Edges" semantics): merge / and / not of collinear
//! edges with signed direction counts, interaction selections, edge-to-edge checks.
//! Selections and checks expect merged inputs (edges of merged polygons and boolean results are).

use crate::check::{check_edges, CheckOpts, EdgeGrid};
use crate::geom::*;
use crate::region::{point_in_poly2, PolyGrid};
use std::collections::HashMap;

fn gcd(a: i64, b: i64) -> i64 {
    if b == 0 { a.abs() } else { gcd(b, a % b) }
}

/// line key: canonical direction (coprime, dx > 0 or dx == 0 && dy > 0) and offset
#[derive(Hash, PartialEq, Eq, Clone, Copy)]
struct Line {
    dx: i64,
    dy: i64,
    c: i64,
}

fn line_of(e: &Edge) -> (Line, i64, i64) {
    let (mut dx, mut dy) = (e.dx(), e.dy());
    let g = gcd(dx, dy);
    dx /= g;
    dy /= g;
    let fwd = dx > 0 || (dx == 0 && dy > 0);
    if !fwd {
        dx = -dx;
        dy = -dy;
    }
    let c = e.p1.x as i64 * dy - e.p1.y as i64 * dx;
    let t1 = e.p1.x as i64 * dx + e.p1.y as i64 * dy;
    let t2 = e.p2.x as i64 * dx + e.p2.y as i64 * dy;
    (Line { dx, dy, c }, t1, t2)
}

fn point_on(l: &Line, t: i64) -> Pt {
    // solve x*dx + y*dy = t, x*dy - y*dx = c
    let n = l.dx * l.dx + l.dy * l.dy;
    Pt::new(((t * l.dx + l.c * l.dy) / n) as i32, ((t * l.dy - l.c * l.dx) / n) as i32)
}

#[derive(Clone, Copy, PartialEq)]
enum Op {
    Or,
    And,
    Not,
}

fn boolean(a: &[Edge], b: &[Edge], op: Op) -> Vec<Edge> {
    let mut lines: HashMap<Line, Vec<(i64, i64, i32, u8)>> = HashMap::new();
    let mut out = Vec::new();
    // `b` first; for AND/NOT, edges of `a` on a line without any `b` edge are decided directly
    // (`a` is a merged edge set)
    for (which, set) in [(1u8, b), (0u8, a)] {
        for e in set {
            if e.is_degenerate() {
                continue;
            }
            let (l, t1, t2) = line_of(e);
            let (lo, hi, s) = if t1 < t2 { (t1, t2, 1) } else { (t2, t1, -1) };
            if which == 0 && op != Op::Or {
                match lines.get_mut(&l) {
                    Some(v) => v.push((lo, hi, s, which)),
                    None => {
                        if op == Op::Not {
                            out.push(*e);
                        }
                    }
                }
            } else {
                lines.entry(l).or_default().push((lo, hi, s, which));
            }
        }
    }
    let mut keys: Vec<&Line> = lines.keys().collect();
    keys.sort_by_key(|l| (l.dx, l.dy, l.c));
    for l in keys {
        let segs = &lines[l];
        if op != Op::Or && !segs.iter().any(|s| s.3 == 0) {
            continue;
        }
        let mut ts: Vec<i64> = segs.iter().flat_map(|s| [s.0, s.1]).collect();
        ts.sort_unstable();
        ts.dedup();
        let mut run: Option<(i64, i64, i32)> = None; // start, end, sign
        let mut flush = |run: &mut Option<(i64, i64, i32)>, out: &mut Vec<Edge>| {
            if let Some((s, e, sign)) = run.take() {
                let (p, q) = (point_on(l, s), point_on(l, e));
                out.push(if sign > 0 { Edge::new(p, q) } else { Edge::new(q, p) });
            }
        };
        for w in ts.windows(2) {
            let (lo, hi) = (w[0], w[1]);
            let (mut wa, mut wb) = (0i32, 0i32);
            for s in segs {
                if s.0 <= lo && s.1 >= hi {
                    if s.3 == 0 { wa += s.2 } else { wb += s.2 }
                }
            }
            let v = match op {
                Op::Or => (wa + wb).signum(),
                Op::And => if wb != 0 { wa.signum() } else { 0 },
                Op::Not => if wb != 0 { 0 } else { wa.signum() },
            };
            match (&mut run, v) {
                (Some(r), v) if v != 0 && r.2 == v && r.1 == lo => r.1 = hi,
                (_, 0) => flush(&mut run, &mut out),
                _ => {
                    flush(&mut run, &mut out);
                    run = Some((lo, hi, v));
                }
            }
        }
        flush(&mut run, &mut out);
    }
    out
}

pub fn merge(a: &[Edge]) -> Vec<Edge> {
    boolean(a, &[], Op::Or)
}
pub fn and(a: &[Edge], b: &[Edge]) -> Vec<Edge> {
    boolean(a, b, Op::And)
}
pub fn not(a: &[Edge], b: &[Edge]) -> Vec<Edge> {
    boolean(a, b, Op::Not)
}

fn grid(b: &[Edge]) -> EdgeGrid {
    EdgeGrid::new(b.iter().map(|e| (*e, 0)).collect(), 2000)
}

/// edges of `a` touching or crossing any edge of `b`
pub fn interacting(a: &[Edge], b: &[Edge]) -> Vec<Edge> {
    if a.is_empty() || b.is_empty() {
        return vec![];
    }
    let a = a.to_vec();
    let g = grid(b);
    let mut scratch = Vec::new();
    a.into_iter()
        .filter(|e| {
            g.query(&e.bbox(), &mut scratch);
            scratch.iter().any(|&j| e.intersect(&g.edges[j as usize].0))
        })
        .collect()
}

fn contains_excl(e: &Edge, p: Pt) -> bool {
    e.contains(p) && p != e.p1 && p != e.p2
}

fn edge_is_outside(a: &Edge, b: &Edge) -> bool {
    if a.parallel(b) {
        !a.coincident(b)
    } else {
        match a.intersect_point(b) {
            None => true,
            Some(pt) => !contains_excl(b, pt) || !contains_excl(a, pt),
        }
    }
}

/// edges of `a` which are not outside (collinear overlap or proper crossing with) some edge of `b`
pub fn not_outside(a: &[Edge], b: &[Edge]) -> Vec<Edge> {
    if a.is_empty() || b.is_empty() {
        return vec![];
    }
    let a = a.to_vec();
    let g = grid(b);
    let mut scratch = Vec::new();
    a.into_iter()
        .filter(|e| {
            g.query(&e.bbox(), &mut scratch);
            scratch.iter().any(|&j| !edge_is_outside(e, &g.edges[j as usize].0))
        })
        .collect()
}

/// Edges#extended_in(d): a polygon per edge, reaching d to the inside (right of the edge)
pub fn extended_in(a: &[Edge], d: i32) -> Vec<Poly> {
    a.iter()
        .filter(|e| !e.is_degenerate())
        .filter_map(|e| {
            let f = d as f64 / e.len();
            let (nx, ny) = (rounded(e.dy() as f64 * f), rounded(-e.dx() as f64 * f));
            let p = Poly::from_ring(vec![e.p1, e.p2, Pt::new(e.p2.x + nx, e.p2.y + ny), Pt::new(e.p1.x + nx, e.p1.y + ny)]);
            if p.hull.is_empty() { None } else { Some(p) }
        })
        .collect()
}

fn edge_interacts_poly(e: &Edge, p: &Poly) -> bool {
    if p.edges().any(|pe| pe.intersect(e)) {
        return true;
    }
    point_in_poly2(p, 2 * e.p1.x as i64, 2 * e.p1.y as i64) >= 0
}

/// edges of `a` touching or inside any polygon of `r`
pub fn interacting_region(a: &[Edge], r: &[Poly]) -> Vec<Edge> {
    if a.is_empty() || r.is_empty() {
        return vec![];
    }
    let g = PolyGrid::new(r);
    let mut scratch = Vec::new();
    a.iter()
        .copied()
        .filter(|e| {
            g.query(&e.bbox(), &mut scratch);
            scratch.iter().any(|&j| edge_interacts_poly(e, &r[j as usize]))
        })
        .collect()
}

/// polygons of `r` touching any edge of `a`
pub fn region_interacting_edges(r: &[Poly], a: &[Edge]) -> Vec<Poly> {
    if a.is_empty() || r.is_empty() {
        return vec![];
    }
    let g = grid(a);
    let mut scratch = Vec::new();
    r.iter()
        .filter(|p| {
            g.query(&p.bbox(), &mut scratch);
            scratch.iter().any(|&j| edge_interacts_poly(&g.edges[j as usize].0, p))
        })
        .cloned()
        .collect()
}

/// single-collection edge check (Edges#width etc. without `other`)
pub fn self_check(o: &CheckOpts, a: &[Edge]) -> Vec<EdgePair> {
    let g = grid(a);
    let mut scratch = Vec::new();
    let mut out = Vec::new();
    for (i, (e, _)) in g.edges.iter().enumerate() {
        g.query(&e.bbox().enlarged(o.d), &mut scratch);
        for &j in &scratch {
            if (j as usize) > i {
                if let Some(ep) = check_edges(o, e, &g.edges[j as usize].0) {
                    out.push(ep);
                }
            }
        }
    }
    out
}
