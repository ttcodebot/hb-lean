//! Port of player.js: stage timeline, per-cell animation and the draw routine.
//! Kept in the same order and with the same constants as the JavaScript, so the two can be diffed.
use crate::canvas::{hex, Canvas, Font, Rgb};
use crate::scene::Scene;
use std::collections::HashMap;

const W: f64 = 1200.0;
const H: f64 = 630.0;
struct Vp {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}
const VP: Vp = Vp { x: 16.0, y: 16.0, w: 868.0, h: 528.0 };
const PANEL_X: f64 = 904.0;
const PANEL_W: f64 = 280.0;

const C_BG: Rgb = hex(0x0b0f16);
const C_DIE: Rgb = hex(0x111722);
const C_ROW: Rgb = hex(0x1a2230);
const C_TEXT: Rgb = hex(0xe6edf3);
const C_DIM: Rgb = hex(0x7d8ba0);
const C_ACCENT: Rgb = hex(0xffd166);
const C_COMB: Rgb = hex(0x4f7fd9);
const C_SEQ: Rgb = hex(0xf0a23a);
const C_BUF: Rgb = hex(0x2fbfae);
const C_CLK: Rgb = hex(0xff4fd8);
const C_TIE: Rgb = hex(0x77839a);
const C_DIODE: Rgb = hex(0xa78bfa);
const C_PHYS: Rgb = hex(0x4a5d80);
const C_FILL: Rgb = hex(0x22304a);
const C_PIN: Rgb = hex(0xe6edf3);
const C_GREEN: Rgb = hex(0x3fb950);
const C_RED: Rgb = hex(0xff7b72);
const C_WHITE: Rgb = hex(0xffffff);
const LAYER_COL: [Rgb; 6] = [hex(0xa594ff), hex(0x4cc9f0), hex(0xff5d8f), hex(0x7ae582), hex(0xffd166), hex(0xff9f1c)];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Cat {
    Comb,
    Seq,
    Buf,
    Clk,
    Tie,
    Diode,
    Phys,
}

impl Cat {
    fn color(self) -> Rgb {
        match self {
            Cat::Comb => C_COMB,
            Cat::Seq => C_SEQ,
            Cat::Buf => C_BUF,
            Cat::Clk => C_CLK,
            Cat::Tie => C_TIE,
            Cat::Diode => C_DIODE,
            Cat::Phys => C_PHYS,
        }
    }
}

pub struct Stage {
    pub id: &'static str,
    pub label: &'static str,
    max_snap: i64,
    pub dur: f64,
    pub rt: f64,
    pub t0: f64,
}

struct Cell {
    w: f64,
    h: f64,
    cat: Cat,
    born: usize,
    pos: Vec<Option<[f64; 2]>>,
    death: usize, // 0 = never (as in the JS, where stage 0 is falsy)
    r: f64,
    r2: f64,
}

#[derive(Clone, Copy)]
struct Cam {
    cx: f64,
    cy: f64,
    s: f64,
}

const NONE: usize = usize::MAX;

pub struct Player {
    s: Scene,
    pub stages: Vec<Stage>,
    pub duration: f64,
    si_pdn: usize,
    si_gpl: usize,
    si_cts: usize,
    si_grt: usize,
    si_drt: usize,
    si_fill: usize,
    has_cts: bool,
    cells: Vec<Cell>,
    n_logic: usize,
    cx0: f64,
    cy0: f64,
    die_w: f64,
    die_h: f64,
    full_cam: Cam,
    zoom_cam: Cam,
    zoom_on: bool,
    wires: Vec<[f64; 6]>,
    vias: Vec<[f64; 4]>,
    used_layers: Vec<usize>,
    layer_range: Vec<(usize, usize)>, // per used layer: first index, count
    via_range: Vec<(usize, usize)>,
    layer_t: Vec<(f64, f64)>,
    wire_um: f64,
    fills: Vec<[f64; 4]>,
}

fn hash(i: usize) -> f64 {
    let mut x = (i as u32).wrapping_add(1).wrapping_mul(2654435761);
    x ^= x >> 15;
    x = x.wrapping_mul(2246822519);
    x ^= x >> 13;
    x as f64 / 4294967296.0
}
fn clamp(x: f64) -> f64 {
    x.max(0.0).min(1.0)
}
fn ease(x: f64) -> f64 {
    let x = clamp(x);
    if x < 0.5 {
        4.0 * x * x * x
    } else {
        1.0 - (-2.0 * x + 2.0).powi(3) / 2.0
    }
}
fn ease_out(x: f64) -> f64 {
    1.0 - (1.0 - clamp(x)).powi(3)
}
fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}
/// toLocaleString('en-US') with a fixed number of fraction digits.
fn fmt(x: f64, d: usize) -> String {
    let s = format!("{:.*}", d, x.abs());
    let (int, frac) = match s.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (s.as_str(), None),
    };
    let mut out = String::new();
    if x < 0.0 && s.chars().any(|c| c != '0' && c != '.') {
        out.push('-');
    }
    for (i, ch) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    if let Some(f) = frac {
        out.push('.');
        out.push_str(f);
    }
    out
}

impl Player {
    pub fn new(s: Scene) -> Player {
        let die = s.die;
        let (die_w, die_h) = (die[2] - die[0], die[3] - die[1]);
        let snap_idx: HashMap<&str, i64> = s.snaps.iter().enumerate().map(|(i, n)| (n.as_str(), i as i64)).collect();
        let defs: [(&str, &str, &[&str], f64); 9] = [
            ("floorplan", "Floorplan", &["floorplan", "io"], 0.9),
            ("pdn", "Power grid + taps", &["tap", "pdn"], 0.9),
            ("gpl", "Global placement", &["gpl"], 1.7),
            ("dpl", "Repair + legalise", &["rsz", "dpl"], 1.1),
            ("cts", "Clock tree", &["cts", "rsz2"], 1.2),
            ("grt", "Global routing", &["grt"], 0.9),
            ("drt", "Detailed routing", &["drt"], 2.4),
            ("fill", "Fill insertion", &["fill"], 0.8),
            ("final", "Final layout", &[], 1.3),
        ];
        let mut stages: Vec<Stage> = defs
            .iter()
            .map(|d| {
                let max_snap = d.2.iter().filter_map(|n| snap_idx.get(n)).copied().max().unwrap_or(-1);
                let rt = d.2.iter().map(|n| s.runtime.get(*n).copied().unwrap_or(0.0)).sum();
                Stage { id: d.0, label: d.1, max_snap, dur: d.3, rt, t0: 0.0 }
            })
            .collect();
        let si_of = |n: &str| snap_idx.get(n).copied();
        stages[0].max_snap = si_of("floorplan").unwrap_or(0);
        stages[1].max_snap = si_of("io").unwrap_or(0).max(si_of("pdn").unwrap_or(0)).max(si_of("tap").unwrap_or(0));
        stages.last_mut().unwrap().max_snap = 1_000_000_000;
        let cts_snaps: Vec<i64> = ["cts", "rsz2"].iter().filter_map(|n| si_of(n)).collect();
        let has_cts = s.cells.iter().any(|c| cts_snaps.contains(&c.1));
        if !has_cts {
            stages.retain(|st| st.id != "cts");
        }
        let mut acc = 0.0;
        for st in stages.iter_mut() {
            st.t0 = acc;
            acc += st.dur;
        }
        let duration = acc;
        let si = |id: &str| stages.iter().position(|st| st.id == id).unwrap_or(NONE);
        let (si_pdn, si_gpl, si_cts, si_grt, si_drt, si_fill) = (si("pdn"), si("gpl"), si("cts"), si("grt"), si("drt"), si("fill"));
        let cell_stage = |snap: i64| stages.iter().position(|st| st.max_snap >= snap).unwrap_or(stages.len() - 1);

        let cells: Vec<Cell> = s
            .cells
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let m = &s.macros[c.0];
                let born = cell_stage(c.1);
                let pos = stages
                    .iter()
                    .map(|st| {
                        let mut p = None;
                        for h in &c.3 {
                            if h[0] as i64 <= st.max_snap {
                                p = Some([h[1], h[2]]);
                            }
                        }
                        p
                    })
                    .collect();
                let cat = match m.3.as_str() {
                    "clkbuf" => {
                        if has_cts && born == si_cts {
                            Cat::Clk
                        } else {
                            Cat::Buf
                        }
                    }
                    "seq" => Cat::Seq,
                    "buf" => Cat::Buf,
                    "clk" => Cat::Clk,
                    "tie" => Cat::Tie,
                    "diode" => Cat::Diode,
                    "phys" => Cat::Phys,
                    _ => Cat::Comb,
                };
                Cell { w: m.1, h: m.2, cat, born, pos, death: if c.4 != 0 { cell_stage(c.4) } else { 0 }, r: hash(i), r2: hash(i + 7777) }
            })
            .collect();
        let is_logic = |c: &Cell| c.cat != Cat::Phys && c.death == 0;
        let n_logic = cells.iter().filter(|c| is_logic(c)).count();
        let (mut cx0, mut cy0, mut nn) = (0.0, 0.0, 0usize);
        for c in &cells {
            if c.born == si_gpl {
                if let Some(p) = c.pos[si_gpl] {
                    cx0 += p[0];
                    cy0 += p[1];
                    nn += 1;
                }
            }
        }
        if nn > 0 {
            cx0 /= nn as f64;
            cy0 /= nn as f64;
        } else {
            cx0 = die_w / 2.0;
            cy0 = die_h / 2.0;
        }

        let mut bb = [1e12f64, 1e12, -1e12, -1e12];
        let mut grow = |x: f64, y: f64| {
            bb[0] = bb[0].min(x);
            bb[1] = bb[1].min(y);
            bb[2] = bb[2].max(x);
            bb[3] = bb[3].max(y);
        };
        for c in cells.iter().filter(|c| is_logic(c)) {
            for p in c.pos.iter().flatten() {
                grow(p[0], p[1]);
                grow(p[0] + c.w, p[1] + c.h);
            }
        }
        for w in &s.wires {
            grow(w[1], w[2]);
            grow(w[3], w[4]);
        }
        let fit_cam = |x1: f64, y1: f64, x2: f64, y2: f64, pad: f64| {
            let (w, h) = ((x2 - x1) * (1.0 + 2.0 * pad), (y2 - y1) * (1.0 + 2.0 * pad));
            let mut sc = (VP.w / w).min(VP.h / h);
            sc = sc.min((VP.w / die_w) * 6.0);
            let (mut cx, mut cy) = ((x1 + x2) / 2.0, (y1 + y2) / 2.0);
            let (hw, hh) = (VP.w / sc / 2.0, VP.h / sc / 2.0);
            cx = if 2.0 * hw < die_w { cx.max(die[0] + hw).min(die[2] - hw) } else { (die[0] + die[2]) / 2.0 };
            cy = if 2.0 * hh < die_h { cy.max(die[1] + hh).min(die[3] - hh) } else { (die[1] + die[3]) / 2.0 };
            Cam { cx, cy, s: sc }
        };
        let full_cam = fit_cam(die[0], die[1], die[2], die[3], 0.02);
        let mut zoom_cam = fit_cam(bb[0], bb[1], bb[2], bb[3], 0.05);
        let zoom_on = zoom_cam.s > full_cam.s * 1.35;
        if !zoom_on {
            zoom_cam = full_cam;
        }

        let mut wires = s.wires.clone();
        wires.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[5].total_cmp(&b[5])));
        let mut vias = s.vias.clone();
        vias.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[3].total_cmp(&b[3])));
        let mut used_layers: Vec<usize> = wires.iter().map(|w| w[0] as usize).collect();
        used_layers.dedup();
        let layer_range: Vec<(usize, usize)> = used_layers
            .iter()
            .map(|&l| (wires.iter().position(|w| w[0] as usize == l).unwrap_or(0), wires.iter().filter(|w| w[0] as usize == l).count()))
            .collect();
        let via_range: Vec<(usize, usize)> = used_layers
            .iter()
            .map(|&l| (vias.iter().position(|v| v[0] as usize == l).unwrap_or(0), vias.iter().filter(|v| v[0] as usize == l).count()))
            .collect();
        let share: Vec<f64> = layer_range.iter().map(|r| (r.1 as f64).powf(0.6)).collect();
        let tot: f64 = share.iter().sum();
        let mut a0 = 0.0;
        let layer_t = share
            .iter()
            .map(|sh| {
                let r = (a0 / tot, (a0 + sh) / tot);
                a0 += sh;
                r
            })
            .collect();
        let wire_um = s.wires.iter().map(|w| (w[3] - w[1]).abs() + (w[4] - w[2]).abs()).sum::<f64>() / s.dbu;
        let fills = s.fills.iter().map(|f| [f.1, f.2, s.macros[f.0].1, s.macros[f.0].2]).collect();

        Player {
            s, stages, duration, si_pdn, si_gpl, si_cts, si_grt, si_drt, si_fill, has_cts, cells, n_logic, cx0, cy0, die_w, die_h,
            full_cam, zoom_cam, zoom_on, wires, vias, used_layers, layer_range, via_range, layer_t, wire_um, fills,
        }
    }

    pub fn design(&self) -> &str {
        &self.s.design
    }

    /// Timestamp of the reference key frame of stage `i` (same fractions as render.js).
    pub fn keyframe_time(&self, i: usize) -> f64 {
        let st = &self.stages[i];
        let frac = match st.id {
            "final" => 0.99,
            "gpl" => 0.35,
            _ => 0.7,
        };
        st.t0 + st.dur * frac
    }

    fn stage_at(&self, t: f64) -> (usize, f64) {
        let t = t.max(0.0).min(self.duration - 1e-6);
        let mut i = self.stages.len() - 1;
        while i > 0 && self.stages[i].t0 > t {
            i -= 1;
        }
        (i, (t - self.stages[i].t0) / self.stages[i].dur)
    }

    fn cam_at(&self, si: usize, u: f64) -> Cam {
        if !self.zoom_on {
            return self.full_cam;
        }
        let k = if si < self.si_gpl {
            0.0
        } else if si == self.si_gpl {
            ease(u / 0.45)
        } else if si < self.si_fill {
            1.0
        } else if si == self.si_fill {
            1.0 - ease(u / 0.8)
        } else {
            0.0
        };
        let (f, z) = (self.full_cam, self.zoom_cam);
        Cam { cx: lerp(f.cx, z.cx, k), cy: lerp(f.cy, z.cy, k), s: lerp(f.s.ln(), z.s.ln(), k).exp() }
    }

    /// [x, y, alpha, pop]
    fn cell_pos(&self, c: &Cell, si: usize, u: f64) -> Option<[f64; 4]> {
        if c.born > si {
            return None;
        }
        if c.death != 0 && c.death <= si {
            if c.death < si {
                return None;
            }
            let p = c.pos[si - 1]?;
            return Some([p[0], p[1], 1.0 - clamp(u / 0.25), 0.0]);
        }
        let p = c.pos[si]?;
        if c.born == si {
            if si == self.si_gpl {
                let d = c.r * 0.25;
                let k = ease_out((u - d) / 0.62);
                let (jx, jy) = ((c.r - 0.5) * self.die_w * 0.05, (c.r2 - 0.5) * self.die_w * 0.05);
                return Some([lerp(self.cx0 + jx, p[0], k), lerp(self.cy0 + jy, p[1], k), clamp(u / 0.08), 0.0]);
            }
            if c.cat == Cat::Phys {
                let d = ((p[0] - self.s.die[0]) / self.die_w) * 0.55;
                return Some([p[0], p[1], clamp((u - d) / 0.15), 0.0]);
            }
            let d = 0.1 + c.r * 0.5;
            let k = clamp((u - d) / 0.18);
            if k <= 0.0 {
                return None;
            }
            return Some([p[0], p[1], 1.0, 1.0 - k]);
        }
        if si > 0 {
            if let Some(q) = c.pos[si - 1] {
                if q[0] != p[0] || q[1] != p[1] {
                    let k = ease((u - c.r * 0.2) / 0.6);
                    return Some([lerp(q[0], p[0], k), lerp(q[1], p[1], k), 1.0, 0.0]);
                }
            }
        }
        Some([p[0], p[1], 1.0, 0.0])
    }

    pub fn draw(&self, cv: &mut Canvas, t: f64) {
        let s = &self.s;
        let die = s.die;
        let (die_w, die_h) = (self.die_w, self.die_h);
        let (si, u) = self.stage_at(t);
        let st = &self.stages[si];
        let cam = self.cam_at(si, u);
        let sc = cam.s;
        let fx = |x: f64| VP.x + VP.w / 2.0 + (x - cam.cx) * cam.s;
        let fy = |y: f64| VP.y + VP.h / 2.0 - (y - cam.cy) * cam.s;
        let mut segs: Vec<f32> = Vec::new();

        cv.clear(C_BG);
        // --- die viewport (clipped to VP in the JS; here the area outside VP is repainted below) ---

        // die + rows
        let fp = if si == 0 { ease(u / 0.5) } else { 1.0 };
        cv.fill_rect(fx(die[0]), fy(die[3]), die_w * sc, die_h * sc, C_DIE, fp);
        let nrows = if si == 0 { (s.rows.len() as f64 * clamp((u - 0.15) / 0.5)).floor() as usize } else { s.rows.len() };
        for r in &s.rows[..nrows] {
            segs.extend_from_slice(&[fx(r[0]) as f32, fy(r[1] + s.row_h) as f32, (r[2] * sc) as f32, (s.row_h * sc) as f32]);
        }
        cv.stroke_rects(&segs, C_ROW, 1.0, 1.0);

        let routed = if si > self.si_drt {
            1.0
        } else if si == self.si_drt {
            u
        } else {
            0.0
        };
        let dim_cells = if si >= self.si_drt { lerp(1.0, 0.55, clamp(routed * 3.0)) } else { 1.0 };

        // fill cells
        if si >= self.si_fill {
            let k = if si == self.si_fill { u } else { 1.0 };
            segs.clear();
            for f in &self.fills {
                if (f[0] - die[0]) / die_w > k * 1.15 {
                    continue;
                }
                let (x, y, w, h) = (fx(f[0]) + 0.5, fy(f[1] + f[3]) + 0.5, f[2] * sc - 1.0, f[3] * sc - 1.0);
                cv.fill_rect(x, y, w, h, C_FILL, 1.0);
                segs.extend_from_slice(&[x as f32, y as f32, w as f32, h as f32]);
            }
            if sc * s.row_h > 6.0 {
                cv.stroke_rects(&segs, C_BG, 1.0, 1.0);
            }
        }

        // PDN
        if si >= self.si_pdn {
            let k = if si == self.si_pdn { clamp((u - 0.3) / 0.6) } else { 1.0 };
            for p in &s.pdn {
                let horiz = p[2] == p[4];
                let a = (if horiz { 0.16 } else { 0.2 }) * (if si >= self.si_drt { lerp(1.0, 0.8, routed) } else { 1.0 });
                let col = if p[6] != 0.0 { hex(0x6ea8ff) } else { C_RED };
                let (mut x1, mut x2, mut y1, mut y2) = (p[1].min(p[3]), p[1].max(p[3]), p[2].min(p[4]), p[2].max(p[4]));
                if horiz {
                    x2 = lerp(x1, x2, k);
                    y1 -= p[5] / 2.0;
                    y2 += p[5] / 2.0;
                } else {
                    y2 = lerp(y1, y2, k);
                    x1 -= p[5] / 2.0;
                    x2 += p[5] / 2.0;
                }
                cv.fill_rect(fx(x1), fy(y2), ((x2 - x1) * sc).max(1.0), ((y2 - y1) * sc).max(1.0), col, a);
            }
        }

        // cells
        let mut cur: Vec<Option<[f64; 4]>> = Vec::with_capacity(self.cells.len());
        let mut n_logic = 0usize;
        let mut pops: Vec<([f64; 5], Cat)> = Vec::new();
        for c in &self.cells {
            let p = self.cell_pos(c, si, u);
            cur.push(p);
            let Some(p) = p else { continue };
            if c.cat != Cat::Phys && !(c.death != 0 && c.death <= si) {
                n_logic += 1;
            }
            let (x, y, w, h) = (fx(p[0]), fy(p[1] + c.h), c.w * sc, c.h * sc);
            if c.cat == Cat::Phys {
                cv.fill_rect(x + 0.5, y + 0.5, (w - 1.0).max(1.5), h - 1.0, C_PHYS, p[2]);
            } else {
                cv.fill_rect(x + 0.5, y + 0.5, (w - 1.0).max(1.5), (h - 1.0).max(1.5), c.cat.color(), p[2] * dim_cells);
                if p[3] > 0.0 {
                    pops.push(([x, y, w, h, p[3]], c.cat));
                }
            }
        }

        // IO pins
        if si > 0 || u > 0.45 {
            let k = if si == 0 { clamp((u - 0.45) / 0.4) } else { 1.0 };
            let np = (s.pins.len() as f64 * k).floor() as usize;
            for p in &s.pins[..np] {
                cv.fill_rect(fx(p[0]) - 1.0, fy(p[1] + p[3]) - 1.0, (p[2] * sc + 2.0).max(3.0), (p[3] * sc + 2.0).max(3.0), C_PIN, 1.0);
            }
        }

        // ratsnest
        let mut rat_a = 0.0;
        if si >= self.si_gpl && si < self.si_drt {
            rat_a = if si == self.si_gpl { clamp(u / 0.15) } else { 1.0 };
        } else if si == self.si_drt {
            rat_a = 1.0 - clamp(u / 0.35);
        }
        if si == self.si_grt {
            rat_a = lerp(1.0, 0.5, u);
        }
        let cts_now = self.has_cts && si == self.si_cts;
        if rat_a > 0.0 {
            let big = (self.n_logic as f64 * 0.05).max(24.0);
            let mut pts: Vec<f64> = Vec::new();
            for pass in 0..2 {
                segs.clear();
                for (ni, n) in s.nets.iter().enumerate() {
                    if (n.1 == 1) != (pass == 1) {
                        continue;
                    }
                    if pass == 0 && n.0.len() as f64 > big {
                        continue;
                    }
                    if pass == 1 && cts_now && hash(ni) * 0.5 > u {
                        continue;
                    }
                    let (mut sx, mut sy, mut k) = (0.0, 0.0, 0usize);
                    pts.clear();
                    for &e in &n.0 {
                        let (px, py);
                        if e < 0 {
                            let p = &s.pins[(-1 - e) as usize];
                            px = p[0] + p[2] / 2.0;
                            py = p[1] + p[3] / 2.0;
                        } else {
                            let Some(p) = cur[e as usize] else { continue };
                            let c = &self.cells[e as usize];
                            px = p[0] + c.w / 2.0;
                            py = p[1] + c.h / 2.0;
                        }
                        pts.push(px);
                        pts.push(py);
                        sx += px;
                        sy += py;
                        k += 1;
                    }
                    if k < 2 {
                        continue;
                    }
                    if k == 2 {
                        segs.extend_from_slice(&[fx(pts[0]) as f32, fy(pts[1]) as f32, fx(pts[2]) as f32, fy(pts[3]) as f32]);
                        continue;
                    }
                    sx /= k as f64;
                    sy /= k as f64;
                    for j in (0..pts.len()).step_by(2) {
                        segs.extend_from_slice(&[fx(sx) as f32, fy(sy) as f32, fx(pts[j]) as f32, fy(pts[j + 1]) as f32]);
                    }
                }
                if pass == 0 {
                    cv.stroke_lines(&segs, C_WHITE, 0.22 * rat_a, 1.0, false);
                } else {
                    cv.stroke_lines(&segs, C_CLK, (if cts_now { 0.75 } else { 0.4 }) * rat_a, if cts_now { 1.4 } else { 1.0 }, false);
                }
            }
        }

        // global-route guides
        if si == self.si_grt || (si == self.si_drt && u < 0.3) {
            let k = if si == self.si_grt { clamp(u / 0.75) } else { 1.0 };
            let a = if si == self.si_grt { 1.0 } else { 1.0 - u / 0.3 };
            let n = (s.guides.len() as f64 * k).floor() as usize;
            for g in &s.guides[..n] {
                cv.fill_rect(fx(g[1]), fy(g[4]), (g[3] - g[1]) * sc, (g[4] - g[2]) * sc, LAYER_COL[g[0] as usize % 6], 0.11 * a);
            }
        }

        // routed wires, layer by layer
        let mut cur_layer: Option<usize> = None;
        if routed > 0.0 {
            for (li, &l) in self.used_layers.iter().enumerate() {
                let (ta, tb) = self.layer_t[li];
                let k = clamp((routed / 0.92 - ta) / (tb - ta));
                if k <= 0.0 {
                    continue;
                }
                if k < 1.0 {
                    cur_layer = Some(l);
                }
                let (w0, wn) = self.layer_range[li];
                let n = (wn as f64 * k).floor() as usize;
                let lw = (s.layer_w[l] * sc).max(1.3);
                let mut emit = |cv: &mut Canvas, from: usize, to: usize, col: Rgb| {
                    segs.clear();
                    for w in &self.wires[from..to] {
                        segs.extend_from_slice(&[fx(w[1]) as f32, fy(w[2]) as f32, fx(w[3]) as f32, fy(w[4]) as f32]);
                    }
                    cv.stroke_lines(&segs, col, 0.95, lw, true);
                };
                emit(cv, w0, w0 + n, LAYER_COL[l % 6]);
                // growing head
                if k < 1.0 {
                    let head = ((wn as f64 * 0.02).ceil() as usize).max(2);
                    emit(cv, (w0 + n).saturating_sub(head).max(w0), w0 + n, C_WHITE);
                }
                let (v0, vn) = self.via_range[li];
                let nv = (vn as f64 * k).floor() as usize;
                let vs = (s.layer_w[l] * sc * 1.5).max(2.0);
                for v in &self.vias[v0..v0 + nv] {
                    cv.fill_rect(fx(v[1]) - vs / 2.0, fy(v[2]) - vs / 2.0, vs, vs, hex(0xf5f7fa), 0.8);
                }
            }
        }

        // pop highlights for newly inserted cells
        for (p, cat) in &pops {
            let g = 6.0 * p[4];
            let col = if *cat == Cat::Clk { hex(0xffd0f5) } else { C_WHITE };
            cv.stroke_rect(p[0] - g, p[1] - g, p[2] + 2.0 * g, p[3] + 2.0 * g, col, p[4], 2.0);
        }

        // die outline
        cv.stroke_rect(fx(die[0]), fy(die[3]), die_w * sc, die_h * sc, hex(0x4a5a78), fp, 1.5);

        // end of clip: repaint everything outside the viewport
        cv.fill_rect(0.0, 0.0, W, VP.y, C_BG, 1.0);
        cv.fill_rect(0.0, VP.y + VP.h, W, H - VP.y - VP.h, C_BG, 1.0);
        cv.fill_rect(0.0, VP.y, VP.x, VP.h, C_BG, 1.0);
        cv.fill_rect(VP.x + VP.w, VP.y, W - VP.x - VP.w, VP.h, C_BG, 1.0);

        // ---- side panel ----
        let px = PANEL_X;
        cv.fill_text("TINY TAPEOUT  \u{b7}  HARDENING", px, 34.0, Font::SansBold, 13.0, C_ACCENT, 1.0);
        let mut name: String = s.design.clone();
        while cv.measure(&name, Font::SansBold, 22.0) > PANEL_W && name.chars().count() > 4 {
            name.pop();
            name.pop();
        }
        if name != s.design {
            name.push('\u{2026}');
        }
        cv.fill_text(&name, px, 64.0, Font::SansBold, 22.0, C_TEXT, 1.0);
        let sub = format!("{}  \u{b7}  {} \u{d7} {} \u{b5}m", s.pdk.as_deref().unwrap_or(""), fmt(die_w / s.dbu, 0), fmt(die_h / s.dbu, 0));
        cv.fill_text(&sub, px, 86.0, Font::Sans, 14.0, C_DIM, 1.0);

        let mut y = 122.0;
        for (i, sg) in self.stages.iter().enumerate() {
            let (done, now) = (i < si, i == si);
            let dot = if now { C_ACCENT } else if done { C_GREEN } else { hex(0x3a4558) };
            cv.fill_circle(px + 7.0, y - 6.0, if now { 6.0 } else { 4.5 }, dot);
            let col = if now { C_TEXT } else if done { hex(0x9fb0c7) } else { hex(0x56637a) };
            cv.fill_text(sg.label, px + 24.0, y, if now { Font::SansBold } else { Font::Sans }, 17.0, col, 1.0);
            if sg.rt != 0.0 && (done || now) {
                let tx = format!("{} s", fmt(sg.rt, 1));
                let w = cv.measure(&tx, Font::Mono, 13.0);
                cv.fill_text(&tx, px + PANEL_W - w, y, Font::Mono, 13.0, if now { C_ACCENT } else { hex(0x6b7a92) }, 1.0);
            }
            y += 31.0;
        }

        // stats
        y = (y + 12.0).max(418.0);
        let mut stat = |cv: &mut Canvas, k: &str, v: &str, col: Option<Rgb>| {
            cv.fill_text(k, px, y, Font::Sans, 14.0, C_DIM, 1.0);
            let w = cv.measure(v, Font::MonoBold, 19.0);
            cv.fill_text(v, px + PANEL_W - w, y + 1.0, Font::MonoBold, 19.0, col.unwrap_or(C_TEXT), 1.0);
            y += 30.0;
        };
        stat(cv, "Cells placed", &fmt(n_logic as f64, 0), None);
        if si <= self.si_gpl || s.drt_log.is_empty() || si < self.si_grt {
            let gl = &s.gpl_log;
            let g = if si == self.si_gpl && !gl.is_empty() {
                Some(gl[(gl.len() - 1).min((clamp(u / 0.85) * gl.len() as f64).floor() as usize)])
            } else if si > self.si_gpl && !gl.is_empty() {
                Some(gl[gl.len() - 1])
            } else {
                None
            };
            let dash = "\u{2013}".to_string();
            stat(cv, "Est. wirelength", &g.map(|g| format!("{} \u{b5}m", fmt(g[2], 0))).unwrap_or(dash.clone()), None);
            stat(cv, "Overlap", &g.map(|g| format!("{} %", fmt(g[1] * 100.0, 0))).unwrap_or(dash), g.filter(|g| g[1] < 0.12).map(|_| C_GREEN));
        } else {
            let dl = &s.drt_log;
            let d = if si == self.si_drt {
                Some(dl[(dl.len() - 1).min((clamp(u / 0.9) * dl.len() as f64).floor() as usize)])
            } else if si > self.si_drt {
                Some(dl[dl.len() - 1])
            } else {
                None
            };
            let dash = "\u{2013}".to_string();
            let wr = if si >= self.si_drt {
                format!("{} \u{b5}m", fmt(self.wire_um * (if si > self.si_drt { 1.0 } else { clamp(routed / 0.92) }), 0))
            } else {
                dash.clone()
            };
            stat(cv, "Wire routed", &wr, None);
            stat(cv, "DRC violations", &d.map(|d| format!("{}", d[1])).unwrap_or(dash), d.map(|d| if d[1] == 0.0 { C_GREEN } else { C_RED }));
        }
        // layer legend while routing
        if si >= self.si_grt {
            let mut lx = px;
            for &l in &self.used_layers {
                let on = si > self.si_drt || (si == self.si_drt && cur_layer.map_or(true, |c| l <= c));
                let a = if on { 1.0 } else { 0.35 };
                cv.fill_rect(lx, y - 4.0, 12.0, 12.0, LAYER_COL[l % 6], a);
                let w = cv.fill_text(&s.layers[l], lx + 16.0, y + 7.0, Font::Sans, 13.0, C_TEXT, a);
                lx += 22.0 + w + 8.0;
            }
        }

        // ---- caption + progress ----
        let cap = format!("{}/{}  {}", si + 1, self.stages.len(), st.label);
        let cw = cv.fill_text(&cap, VP.x, 590.0, Font::SansBold, 30.0, C_TEXT, 1.0);
        if let (true, Some(cl)) = (si == self.si_drt, cur_layer) {
            cv.fill_text(&s.layers[cl], VP.x + cw + 18.0, 590.0, Font::SansBold, 24.0, LAYER_COL[cl % 6], 1.0);
        }
        let (bx, bw, by, bh) = (VP.x, W - 2.0 * VP.x, 606.0, 8.0);
        for (i, sg) in self.stages.iter().enumerate() {
            let (x1, w1) = (bx + (sg.t0 / self.duration) * bw, (sg.dur / self.duration) * bw - 3.0);
            cv.fill_rect(x1, by, w1, bh, hex(0x222b3a), 1.0);
            let k = if i < si {
                1.0
            } else if i == si {
                u
            } else {
                0.0
            };
            if k > 0.0 {
                cv.fill_rect(x1, by, w1 * k, bh, if i == si { C_ACCENT } else { C_GREEN }, 1.0);
            }
        }
    }
}
