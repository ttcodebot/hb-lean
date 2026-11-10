//! Integer geometry primitives. Conventions follow KLayout so results can be compared 1:1:
//! polygon hulls are clockwise, holes counter-clockwise (interior is on the right of every edge).

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct Pt {
    pub x: i32,
    pub y: i32,
}

impl Pt {
    pub fn new(x: i32, y: i32) -> Pt {
        Pt { x, y }
    }
    pub fn dist(self, o: Pt) -> f64 {
        self.sq_dist(o).sqrt()
    }
    pub fn sq_dist(self, o: Pt) -> f64 {
        let (dx, dy) = ((self.x - o.x) as f64, (self.y - o.y) as f64);
        dx * dx + dy * dy
    }
}

/// KLayout's coord rounding (half away from zero)
pub fn rounded(v: f64) -> i32 {
    if v > 0.0 { (v + 0.5) as i32 } else { (v - 0.5) as i32 }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct BBox {
    pub x1: i32,
    pub y1: i32,
    pub x2: i32,
    pub y2: i32,
}

impl BBox {
    pub fn empty() -> BBox {
        BBox { x1: i32::MAX, y1: i32::MAX, x2: i32::MIN, y2: i32::MIN }
    }
    pub fn is_empty(&self) -> bool {
        self.x1 > self.x2 || self.y1 > self.y2
    }
    pub fn add(&mut self, p: Pt) {
        self.x1 = self.x1.min(p.x);
        self.y1 = self.y1.min(p.y);
        self.x2 = self.x2.max(p.x);
        self.y2 = self.y2.max(p.y);
    }
    pub fn join(&mut self, o: &BBox) {
        if !o.is_empty() {
            self.x1 = self.x1.min(o.x1);
            self.y1 = self.y1.min(o.y1);
            self.x2 = self.x2.max(o.x2);
            self.y2 = self.y2.max(o.y2);
        }
    }
    pub fn enlarged(&self, d: i32) -> BBox {
        BBox { x1: self.x1 - d, y1: self.y1 - d, x2: self.x2 + d, y2: self.y2 + d }
    }
    pub fn touches(&self, o: &BBox) -> bool {
        self.x1 <= o.x2 && o.x1 <= self.x2 && self.y1 <= o.y2 && o.y1 <= self.y2
    }
    pub fn overlaps(&self, o: &BBox) -> bool {
        self.x1 < o.x2 && o.x1 < self.x2 && self.y1 < o.y2 && o.y1 < self.y2
    }
    pub fn contains(&self, o: &BBox) -> bool {
        self.x1 <= o.x1 && self.y1 <= o.y1 && self.x2 >= o.x2 && self.y2 >= o.y2
    }
    pub fn width(&self) -> i32 {
        self.x2 - self.x1
    }
    pub fn height(&self) -> i32 {
        self.y2 - self.y1
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct Edge {
    pub p1: Pt,
    pub p2: Pt,
}

pub fn vprod(ax: i64, ay: i64, bx: i64, by: i64) -> i64 {
    ax * by - ay * bx
}

impl Edge {
    pub fn new(p1: Pt, p2: Pt) -> Edge {
        Edge { p1, p2 }
    }
    pub fn dx(&self) -> i64 {
        (self.p2.x - self.p1.x) as i64
    }
    pub fn dy(&self) -> i64 {
        (self.p2.y - self.p1.y) as i64
    }
    pub fn is_degenerate(&self) -> bool {
        self.p1 == self.p2
    }
    pub fn sq_len(&self) -> f64 {
        (self.dx() * self.dx() + self.dy() * self.dy()) as f64
    }
    pub fn len(&self) -> f64 {
        self.sq_len().sqrt()
    }
    /// integer length as KLayout reports it
    pub fn length(&self) -> i64 {
        rounded(self.len()) as i64
    }
    pub fn swapped(&self) -> Edge {
        Edge { p1: self.p2, p2: self.p1 }
    }
    pub fn bbox(&self) -> BBox {
        BBox { x1: self.p1.x.min(self.p2.x), y1: self.p1.y.min(self.p2.y), x2: self.p1.x.max(self.p2.x), y2: self.p1.y.max(self.p2.y) }
    }
    pub fn is_ortho(&self) -> bool {
        self.p1.x == self.p2.x || self.p1.y == self.p2.y
    }
    /// >0: p is left of the edge, <0: right, 0: on the line
    pub fn side_of(&self, p: Pt) -> i32 {
        if self.is_degenerate() {
            return 0;
        }
        vprod(self.dx(), self.dy(), (p.x - self.p1.x) as i64, (p.y - self.p1.y) as i64).signum() as i32
    }
    /// signed distance (positive = left), unrounded
    pub fn fdistance(&self, p: Pt) -> f64 {
        if self.is_degenerate() {
            return 0.0;
        }
        vprod(self.dx(), self.dy(), (p.x - self.p1.x) as i64, (p.y - self.p1.y) as i64) as f64 / self.len()
    }
    pub fn distance(&self, p: Pt) -> i32 {
        rounded(self.fdistance(p))
    }
    pub fn distance_abs(&self, p: Pt) -> i32 {
        rounded(self.fdistance(p).abs())
    }
    pub fn parallel(&self, o: &Edge) -> bool {
        vprod(self.dx(), self.dy(), o.dx(), o.dy()) == 0
    }
    pub fn sprod_sign(&self, o: &Edge) -> i32 {
        (self.dx() * o.dx() + self.dy() * o.dy()).signum() as i32
    }
    pub fn contains(&self, p: Pt) -> bool {
        if self.is_degenerate() {
            return self.p1 == p;
        }
        let s1 = (p.x - self.p1.x) as i64 * self.dx() + (p.y - self.p1.y) as i64 * self.dy();
        let s2 = (p.x - self.p2.x) as i64 * -self.dx() + (p.y - self.p2.y) as i64 * -self.dy();
        self.distance_abs(p) < 1 && s1 >= 0 && s2 >= 0
    }
    pub fn crossed_by(&self, e: &Edge) -> bool {
        let mut res = true;
        let vsa = self.side_of_raw(e.p1);
        if vsa < 0 {
            res = false;
        } else if vsa == 0 {
            return true;
        }
        let vsb = self.side_of_raw(e.p2);
        if vsb > 0 {
            res = !res;
        } else if vsb == 0 {
            return true;
        }
        res
    }
    fn side_of_raw(&self, p: Pt) -> i64 {
        vprod(self.dx(), self.dy(), (p.x - self.p1.x) as i64, (p.y - self.p1.y) as i64).signum()
    }
    pub fn intersect(&self, e: &Edge) -> bool {
        if self.is_degenerate() {
            e.contains(self.p1)
        } else if e.is_degenerate() {
            self.contains(e.p1)
        } else if !self.bbox().touches(&e.bbox()) {
            false
        } else if self.is_ortho() && e.is_ortho() {
            true
        } else {
            self.crossed_by(e) && e.crossed_by(self)
        }
    }
    /// both edges are collinear and share more than a point
    pub fn coincident(&self, e: &Edge) -> bool {
        if self.is_degenerate() || e.is_degenerate() || self.distance_abs(e.p1) >= 1 || self.distance_abs(e.p2) >= 1 {
            return false;
        }
        let sp = |p: Pt, a: Pt, b: Pt| ((p.x - a.x) as i64 * (b.x - a.x) as i64 + (p.y - a.y) as i64 * (b.y - a.y) as i64).signum();
        if self.sprod_sign(e) < 0 {
            sp(e.p2, self.p2, self.p1) > 0 && sp(e.p1, self.p1, self.p2) > 0
        } else {
            sp(e.p1, self.p2, self.p1) > 0 && sp(e.p2, self.p1, self.p2) > 0
        }
    }
    /// intersection of the line through `e` with this edge's line
    pub fn cut_point(&self, e: &Edge) -> Option<Pt> {
        let pa = vprod(self.dx(), self.dy(), e.dx(), e.dy());
        if pa == 0 {
            return None;
        }
        let pb = vprod((e.p1.x - self.p1.x) as i64, (e.p1.y - self.p1.y) as i64, e.dx(), e.dy());
        let t = pb as f64 / pa as f64;
        Some(Pt::new(self.p1.x + rounded(self.dx() as f64 * t), self.p1.y + rounded(self.dy() as f64 * t)))
    }
    pub fn intersect_point(&self, e: &Edge) -> Option<Pt> {
        if self.is_degenerate() {
            return if e.contains(self.p1) { Some(self.p1) } else { None };
        }
        if e.is_degenerate() {
            return if self.contains(e.p1) { Some(e.p1) } else { None };
        }
        let (b1, b2) = (self.bbox(), e.bbox());
        if !b1.touches(&b2) {
            return None;
        }
        if self.is_ortho() && e.is_ortho() {
            return Some(Pt::new(b1.x1.max(b2.x1), b1.y1.max(b2.y1)));
        }
        let mut res = true;
        let vxa = vprod(self.dx(), self.dy(), (e.p1.x - self.p1.x) as i64, (e.p1.y - self.p1.y) as i64);
        if vxa < 0 {
            res = false;
        } else if vxa == 0 {
            return if self.contains(e.p1) { Some(e.p1) } else if e.crossed_by(self) { Some(e.p1) } else { None };
        }
        let vxb = vprod(self.dx(), self.dy(), (e.p2.x - self.p1.x) as i64, (e.p2.y - self.p1.y) as i64);
        if vxb > 0 {
            res = !res;
        } else if vxb == 0 {
            return if e.crossed_by(self) { Some(e.p2) } else { None };
        }
        if res && e.crossed_by(self) {
            let (a, b) = (vxa.abs() as f64, vxb.abs() as f64);
            let t = a / (a + b);
            Some(Pt::new(e.p1.x + rounded(e.dx() as f64 * t), e.p1.y + rounded(e.dy() as f64 * t)))
        } else {
            None
        }
    }
    pub fn extended(&self, e: i32) -> (f64, f64, f64, f64) {
        let (x1, y1, x2, y2) = (self.p1.x as f64, self.p1.y as f64, self.p2.x as f64, self.p2.y as f64);
        if self.is_degenerate() {
            (x1 - e as f64, y1, x2 + e as f64, y2)
        } else {
            let f = e as f64 / self.len();
            let (dx, dy) = (self.dx() as f64 * f, self.dy() as f64 * f);
            (x1 - dx, y1 - dy, x2 + dx, y2 + dy)
        }
    }
}

#[derive(Clone, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct Poly {
    pub hull: Vec<Pt>,
    pub holes: Vec<Vec<Pt>>,
}

pub fn ring_area2(r: &[Pt]) -> i64 {
    // twice the signed area, positive = counter-clockwise
    let n = r.len();
    let mut a = 0i64;
    for i in 0..n {
        let (p, q) = (r[i], r[(i + 1) % n]);
        a += p.x as i64 * q.y as i64 - q.x as i64 * p.y as i64;
    }
    a
}

/// remove duplicate and collinear points, rotate so that the smallest point comes first
pub fn normalize_ring(r: &mut Vec<Pt>, clockwise: bool) {
    loop {
        let n = r.len();
        if n < 3 {
            r.clear();
            return;
        }
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            let (a, b, c) = (r[(i + n - 1) % n], r[i], r[(i + 1) % n]);
            if a == b {
                continue;
            }
            let v = vprod((b.x - a.x) as i64, (b.y - a.y) as i64, (c.x - b.x) as i64, (c.y - b.y) as i64);
            if v == 0 && b != c {
                // collinear: drop unless it is a spike reversal (keep geometry simple: drop anyway)
                continue;
            }
            if b == c {
                // duplicate handled when c is visited (a == b there)
                out.push(b);
                continue;
            }
            out.push(b);
        }
        out.dedup();
        if out.len() > 1 && out.first() == out.last() {
            out.pop();
        }
        let changed = out.len() != n;
        *r = out;
        if !changed {
            break;
        }
    }
    if r.len() < 3 {
        r.clear();
        return;
    }
    let ccw = ring_area2(r) > 0;
    if ccw == clockwise {
        r.reverse();
    }
    let k = (0..r.len()).min_by_key(|&i| r[i]).unwrap();
    r.rotate_left(k);
}

impl Poly {
    pub fn from_ring(mut pts: Vec<Pt>) -> Poly {
        normalize_ring(&mut pts, true);
        Poly { hull: pts, holes: vec![] }
    }
    pub fn rect(x1: i32, y1: i32, x2: i32, y2: i32) -> Poly {
        Poly { hull: vec![Pt::new(x1, y1), Pt::new(x1, y2), Pt::new(x2, y2), Pt::new(x2, y1)], holes: vec![] }
    }
    pub fn bbox(&self) -> BBox {
        let mut b = BBox::empty();
        for p in &self.hull {
            b.add(*p);
        }
        b
    }
    pub fn is_box(&self) -> bool {
        self.holes.is_empty() && self.hull.len() == 4 && self.hull.iter().enumerate().all(|(i, p)| {
            let q = self.hull[(i + 1) % 4];
            p.x == q.x || p.y == q.y
        })
    }
    pub fn edges(&self) -> impl Iterator<Item = Edge> + '_ {
        std::iter::once(&self.hull).chain(self.holes.iter()).flat_map(|r| {
            let n = r.len();
            (0..n).map(move |i| Edge::new(r[i], r[(i + 1) % n]))
        })
    }
    pub fn num_edges(&self) -> usize {
        self.hull.len() + self.holes.iter().map(|h| h.len()).sum::<usize>()
    }
    /// area in dbu^2
    pub fn area(&self) -> i64 {
        let mut a = ring_area2(&self.hull).abs();
        for h in &self.holes {
            a -= ring_area2(h).abs();
        }
        a / 2
    }
    pub fn area2(&self) -> i64 {
        let mut a = ring_area2(&self.hull).abs();
        for h in &self.holes {
            a -= ring_area2(h).abs();
        }
        a
    }
    pub fn is_manhattan(&self) -> bool {
        self.edges().all(|e| e.is_ortho())
    }
    pub fn canonical(&self) -> Poly {
        let mut p = self.clone();
        p.holes.sort();
        p
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct EdgePair {
    pub first: Edge,
    pub second: Edge,
}

impl EdgePair {
    pub fn normalized(&self) -> EdgePair {
        let mut f = self.first;
        let mut s = self.second;
        let vp = |a: Pt, b: Pt, c: Pt, d: Pt| vprod((a.x - b.x) as i64, (a.y - b.y) as i64, (c.x - d.x) as i64, (c.y - d.y) as i64);
        let mut a1 = vp(f.p2, s.p2, f.p1, s.p1);
        let a2 = vp(f.p1, s.p2, f.p2, s.p1);
        if a2.abs() > a1.abs() {
            f = f.swapped();
            a1 = a2;
        }
        if a1 < 0 {
            f = f.swapped();
            s = s.swapped();
        } else if a1 == 0 {
            if f.sprod_sign(&s) > 0 {
                f = f.swapped();
            }
            if f.side_of(s.p1) > 0 || s.side_of(f.p1) > 0 {
                f = f.swapped();
                s = s.swapped();
            }
        }
        EdgePair { first: f, second: s }
    }
    /// order-independent key for comparisons of symmetric edge pairs
    pub fn key(&self) -> (Edge, Edge) {
        let n = self.normalized();
        let a = (n.first.p1.min(n.first.p2), n.first.p1.max(n.first.p2));
        let b = (n.second.p1.min(n.second.p2), n.second.p1.max(n.second.p2));
        let (a, b) = if a <= b { (a, b) } else { (b, a) };
        (Edge::new(a.0, a.1), Edge::new(b.0, b.1))
    }
    pub fn bbox(&self) -> BBox {
        let mut b = self.first.bbox();
        b.join(&self.second.bbox());
        b
    }
    /// KLayout's EdgePair::to_polygon(e) on the normalized pair
    pub fn to_polygon(&self, e: i32) -> Option<Poly> {
        let n = self.normalized();
        let (mut e1, mut e2);
        let fl = |ed: &Edge| (ed.p1.x as f64, ed.p1.y as f64, ed.p2.x as f64, ed.p2.y as f64);
        e1 = fl(&n.first);
        e2 = fl(&n.second);
        if e != 0 {
            if !n.first.is_degenerate() {
                e1 = n.first.extended(e);
            }
            if !n.second.is_degenerate() {
                e2 = n.second.extended(e);
            }
            if n.first.is_degenerate() && n.second.is_degenerate() {
                if n.first.p1 == n.second.p1 {
                    e1 = n.first.extended(e);
                    let t = n.second.extended(e);
                    e2 = (t.2, t.3, t.0, t.1);
                } else {
                    e1 = (n.first.p1.x as f64, n.first.p1.y as f64, n.second.p1.x as f64, n.second.p1.y as f64);
                    e2 = (e1.2, e1.3, e1.0, e1.1);
                }
            }
            let shift = |ed: (f64, f64, f64, f64)| {
                let (dx, dy) = (ed.2 - ed.0, ed.3 - ed.1);
                let l = (dx * dx + dy * dy).sqrt();
                if l == 0.0 {
                    return ed;
                }
                let f = e as f64 / l;
                let (sx, sy) = (-dy * f, dx * f);
                (ed.0 + sx, ed.1 + sy, ed.2 + sx, ed.3 + sy)
            };
            e1 = shift(e1);
            e2 = shift(e2);
        }
        let r = |x: f64| rounded(x);
        let pts = vec![Pt::new(r(e1.0), r(e1.1)), Pt::new(r(e1.2), r(e1.3)), Pt::new(r(e2.0), r(e2.1)), Pt::new(r(e2.2), r(e2.3))];
        let p = Poly::from_ring(pts);
        if p.hull.is_empty() { None } else { Some(p) }
    }
}
