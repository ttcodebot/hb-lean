//! A slice of the gf180mcu KLayout rule deck (PDK e8daeda, variant gf180mcuD:
//! metal_level=5LM, metal_top=11K, mim_option=B), ported rule by rule.
//! Rule names and semantics follow libs.tech/klayout/tech/drc/rule_decks/*.rb.

use crate::check::*;
use crate::edges as ed;
use crate::gds::{LayerKey, Library};
use crate::geom::*;
use crate::region::{self as rg, Region};
use crate::simple::SimpleResult;
use rayon::prelude::*;
use std::collections::HashMap;
use std::time::Instant;

pub enum Markers {
    Pairs(Vec<EdgePair>),
    Polys(Vec<Poly>),
    Edges(Vec<Edge>),
}

impl Markers {
    pub fn len(&self) -> usize {
        match self {
            Markers::Pairs(v) => v.len(),
            Markers::Polys(v) => v.len(),
            Markers::Edges(v) => v.len(),
        }
    }
    fn strings(&self) -> Vec<String> {
        let mut out: Vec<String> = match self {
            Markers::Pairs(v) => v
                .iter()
                .map(|ep| {
                    let a = (ep.first.p1.min(ep.first.p2), ep.first.p1.max(ep.first.p2));
                    let b = (ep.second.p1.min(ep.second.p2), ep.second.p1.max(ep.second.p2));
                    let (a, b) = if a <= b { (a, b) } else { (b, a) };
                    format!("ep:{},{},{},{}/{},{},{},{}", a.0.x, a.0.y, a.1.x, a.1.y, b.0.x, b.0.y, b.1.x, b.1.y)
                })
                .collect(),
            Markers::Polys(v) => v
                .iter()
                .map(|p| {
                    let ring = |r: &Vec<Pt>| r.iter().map(|q| format!("{},{}", q.x, q.y)).collect::<Vec<_>>().join(";");
                    let mut s = format!("poly:{}", ring(&p.hull));
                    let mut holes = p.holes.clone();
                    holes.sort();
                    for h in &holes {
                        s.push('/');
                        s.push_str(&ring(h));
                    }
                    s
                })
                .collect(),
            Markers::Edges(v) => v
                .iter()
                .map(|e| {
                    let (a, b) = (e.p1.min(e.p2), e.p1.max(e.p2));
                    format!("edge:{},{},{},{}", a.x, a.y, b.x, b.y)
                })
                .collect(),
        };
        out.sort();
        out
    }
}

pub struct RuleResult {
    pub name: String,
    pub markers: Markers,
    pub seconds: f64,
}

pub struct RunResult {
    pub rules: Vec<RuleResult>,
    pub phases: Vec<(String, f64)>,
}

/// layer name -> GDS layer/datatype (generic_layers.rb, 5LM)
pub const LAYERS: &[(&str, i16, i16)] = &[
    ("comp", 22, 0), ("dnwell", 12, 0), ("nwell", 21, 0), ("lvpwell", 204, 0), ("dualgate", 55, 0), ("poly2", 30, 0),
    ("nplus", 32, 0), ("pplus", 31, 0), ("sab", 49, 0), ("esd", 24, 0), ("resistor", 62, 0), ("fhres", 227, 0),
    ("fusetop", 75, 0), ("fusewindow_d", 96, 1), ("polyfuse", 220, 0), ("mvsd", 210, 0), ("mvpsd", 11, 39), ("nat", 5, 0),
    ("comp_dummy", 22, 4), ("poly2_dummy", 30, 4), ("schottky_diode", 241, 0), ("zener", 178, 0), ("res_mk", 110, 5),
    ("opc_drc", 124, 5), ("v5_xtor", 112, 1), ("cap_mk", 117, 5), ("mos_cap_mk", 166, 5), ("ind_mk", 151, 5),
    ("diode_mk", 115, 5), ("drc_bjt", 127, 5), ("lvs_bjt", 118, 5), ("mim_l_mk", 117, 10), ("latchup_mk", 137, 5),
    ("guard_ring_mk", 167, 5), ("otp_mk", 173, 5), ("mtpmark", 122, 5), ("neo_ee_mk", 88, 17), ("sramcore", 108, 5),
    ("lvs_rf", 100, 5), ("lvs_drain", 100, 7), ("hvpolyrs", 123, 5), ("lvs_io", 119, 5), ("probe_mk", 13, 17),
    ("esd_mk", 24, 5), ("lvs_source", 100, 8), ("well_diode_mk", 153, 51), ("ldmos_xtor", 226, 0), ("plfuse", 125, 5),
    ("efuse_mk", 80, 5), ("mcell_feol_mk", 11, 17), ("ymtp_mk", 86, 17), ("dev_wf_mk", 128, 17), ("comp_label", 22, 10),
    ("poly2_label", 30, 10), ("mdiode", 116, 5), ("ndmy", 111, 5), ("pmndmy", 152, 5), ("pad", 37, 0),
    ("ubmpperi", 183, 0), ("ubmparray", 184, 0), ("ubmeplate", 185, 0), ("pr_bndry", 0, 0), ("border", 63, 0),
    ("contact", 33, 0),
    ("metal1_drawn", 34, 0), ("metal1_slot", 34, 3), ("metal1_dummy", 34, 4), ("metal1_label", 34, 10), ("metal1_blk", 34, 5), ("metal1_res", 110, 11),
    ("via1", 35, 0),
    ("metal2_drawn", 36, 0), ("metal2_slot", 36, 3), ("metal2_dummy", 36, 4), ("metal2_blk", 36, 5), ("metal2_label", 36, 10), ("metal2_res", 110, 12),
    ("via2", 38, 0),
    ("metal3_drawn", 42, 0), ("metal3_slot", 42, 3), ("metal3_dummy", 42, 4), ("metal3_blk", 42, 5), ("metal3_label", 42, 10), ("metal3_res", 110, 13),
    ("via3", 40, 0),
    ("metal4_drawn", 46, 0), ("metal4_slot", 46, 3), ("metal4_dummy", 46, 4), ("metal4_blk", 46, 5), ("metal4_label", 46, 10), ("metal4_res", 110, 14),
    ("via4", 41, 0),
    ("metal5_drawn", 81, 0), ("metal5_slot", 81, 3), ("metal5_dummy", 81, 4), ("metal5_blk", 81, 5), ("metal5_label", 81, 10), ("metal5_res", 110, 15),
];

pub struct Ctx {
    layers: HashMap<&'static str, Region>,
    empty: Region,
}

impl Ctx {
    pub fn l(&self, name: &str) -> &Region {
        self.layers.get(name).unwrap_or(&self.empty)
    }
    fn set(&mut self, name: &'static str, r: Region) {
        self.layers.insert(name, r);
    }
}

fn um(v: f64) -> i32 {
    (v * 1000.0).round() as i32
}

type Out = Vec<(String, Markers)>;
type RuleFn = Box<dyn Fn(&Ctx) -> Out + Send + Sync>;

fn pairs(name: &str, v: Vec<EdgePair>) -> Out {
    vec![(name.to_string(), Markers::Pairs(v))]
}
fn polys(name: &str, v: Vec<Poly>) -> Out {
    vec![(name.to_string(), Markers::Polys(v))]
}
fn edges(name: &str, v: Vec<Edge>) -> Out {
    vec![(name.to_string(), Markers::Edges(v))]
}

fn width(r: &[Poly], d: f64) -> Vec<EdgePair> {
    single_layer_check(&CheckOpts::new(Relation::Width, um(d), Metrics::Euclidian), r)
}
fn space(r: &[Poly], d: f64) -> Vec<EdgePair> {
    single_layer_check(&CheckOpts::new(Relation::Space, um(d), Metrics::Euclidian), r)
}
fn separation(a: &[Poly], b: &[Poly], d: f64) -> Vec<EdgePair> {
    two_layer_check(&CheckOpts::new(Relation::Space, um(d), Metrics::Euclidian), a, b)
}
fn enclosed(a: &[Poly], b: &[Poly], d: f64) -> Vec<EdgePair> {
    two_layer_check(&CheckOpts::new(Relation::Inside, um(d), Metrics::Euclidian), a, b)
}
/// EdgePairs#polygons(enlargement)
fn ep_polygons(v: &[EdgePair], e: i32) -> Vec<Poly> {
    v.iter().filter_map(|ep| ep.to_polygon(e)).collect()
}
fn with_area_lt(r: &[Poly], max_um2: f64) -> Vec<Poly> {
    let max2 = (max_um2 * 1e6 * 2.0).round() as i64;
    r.iter().filter(|p| p.area2() < max2).cloned().collect()
}

// ---------------------------------------------------------------- geom.rb
fn offgrid_angle(ctx: &Ctx, layer: &str, name: &str) -> Out {
    let r = ctx.l(layer);
    let mut off = Vec::new();
    let mut acute = Vec::new();
    for p in r {
        for ring in std::iter::once(&p.hull).chain(p.holes.iter()) {
            for q in ring {
                if q.x % 5 != 0 || q.y % 5 != 0 {
                    off.push(EdgePair { first: Edge::new(*q, *q), second: Edge::new(*q, *q) });
                }
            }
        }
        for e in p.edges() {
            let (dx, dy) = (e.dx().abs(), e.dy().abs());
            if !(dx == 0 || dy == 0 || dx == dy) {
                acute.push(e);
            }
        }
    }
    vec![(format!("{name}_OFFGRID"), Markers::Pairs(off)), (format!("{name}_ACUTE"), Markers::Edges(acute))]
}

const GEOM_GROUPS: &[(&str, &[&str])] = &[
    ("geom_base_1", &["comp", "dnwell", "nwell", "lvpwell", "dualgate", "poly2", "nplus", "pplus", "sab", "esd", "resistor", "fhres", "fusetop", "fusewindow_d", "polyfuse", "mvsd", "mvpsd", "nat", "comp_dummy", "poly2_dummy", "schottky_diode", "zener", "res_mk", "opc_drc", "ndmy", "pmndmy", "v5_xtor", "cap_mk", "mos_cap_mk", "ind_mk", "diode_mk", "drc_bjt", "lvs_bjt"]),
    ("geom_base_2", &["mim_l_mk", "latchup_mk", "guard_ring_mk", "otp_mk", "mtpmark", "neo_ee_mk", "sramcore", "lvs_rf", "lvs_drain", "hvpolyrs", "lvs_io", "probe_mk", "esd_mk", "lvs_source", "well_diode_mk", "ldmos_xtor", "plfuse", "efuse_mk", "mcell_feol_mk", "ymtp_mk", "dev_wf_mk", "comp_label", "poly2_label", "mdiode"]),
    ("geom_m1m2", &["contact", "metal1", "metal1_slot", "metal1_blk", "metal1_label", "metal1_res", "via1", "metal2", "metal2_label", "metal2_slot", "metal2_blk", "metal2_res"]),
    ("geom_m3", &["via2", "metal3", "metal3_dummy", "metal3_label", "metal3_blk", "metal3_res"]),
    ("geom_m4", &["via3", "metal4", "metal4_dummy", "metal4_label", "metal4_blk", "metal4_res"]),
    ("geom_m5", &["via4", "metal5", "metal5_dummy", "metal5_label", "metal5_blk", "metal5_res"]),
    ("geom_topmarkers", &["pad", "ubmpperi", "ubmparray", "ubmeplate", "pr_bndry", "border"]),
];

// ---------------------------------------------------------------- metal.rb / metaltop.rb
fn metal_w(idx: usize) -> f64 {
    if idx == 1 { 0.23 } else { 0.28 }
}
fn metal_1(ctx: &Ctx, idx: usize) -> Out {
    let drawn = ctx.l(&format!("metal{idx}_drawn"));
    // M1.1: the deck subtracts 3.3V SRAM core cells (sramcore not under v5_xtor/dualgate)
    if idx == 1 {
        let sram_lv = rg::not_interacting(&rg::not_interacting(ctx.l("sramcore"), ctx.l("v5_xtor")), ctx.l("dualgate"));
        pairs("M1.1", width(&rg::not(drawn, &sram_lv), metal_w(idx)))
    } else {
        pairs(&format!("M{idx}.1"), width(drawn, metal_w(idx)))
    }
}
fn metal_2a(ctx: &Ctx, idx: usize) -> Out {
    pairs(&format!("M{idx}.2a"), space(ctx.l(&format!("metal{idx}_drawn")), metal_w(idx)))
}
fn metal_2b(ctx: &Ctx, idx: usize) -> Out {
    let drawn = ctx.l(&format!("metal{idx}_drawn"));
    // wide metal = opening with a 10um x 10um square (shrink x, shrink y, grow x, grow y), clipped.
    // Polygons are independent under this, and only those with a bbox > 10um both ways can survive.
    let cand: Region = drawn.iter().filter(|p| { let b = p.bbox(); b.width() > 10000 && b.height() > 10000 }).cloned().collect();
    let wide = rg::and(&rg::sized(&rg::sized(&rg::sized(&rg::sized(&cand, -5000, 0), 0, -5000), 5000, 0), 0, 5000), &cand);
    let mut o = CheckOpts::new(Relation::Space, um(0.3), Metrics::Euclidian);
    o.zero = ZeroMode::Never; // without_touching_edges
    pairs(&format!("M{idx}.2b"), two_layer_check(&o, drawn, &wide))
}
fn metal_3(ctx: &Ctx, idx: usize) -> Out {
    polys(&format!("M{idx}.3"), with_area_lt(ctx.l(&format!("metal{idx}_drawn")), 0.1444))
}

fn metaltop_rules(ctx: &Ctx) -> Out {
    let top = ctx.l("metal5");
    let mut out = Out::new();
    out.extend(pairs("MT.1", width(top, 0.44)));
    out.extend(pairs("MT.2a", space(top, 0.46)));
    let wide: Region = top.iter().filter(|p| p.edges().all(|e| e.length() >= 10005)).cloned().collect();
    out.extend(pairs("MT.2b", separation(top, &wide, 0.6)));
    out.extend(polys("MT.4", with_area_lt(top, 0.5625)));
    out
}

// ---------------------------------------------------------------- via.rb
fn via_names(idx: usize) -> (String, String, String) {
    let lower = format!("metal{idx}");
    let upper = format!("metal{}", idx + 1);
    (format!("via{idx}"), lower, upper)
}

fn via_rule_1(ctx: &Ctx, idx: usize) -> Out {
    let (v, _, _) = via_names(idx);
    edges(&format!("V{idx}.1"), rg::edges_of(ctx.l(&v)).into_iter().filter(|e| e.length() != 260).collect())
}
fn via_rule_2a(ctx: &Ctx, idx: usize) -> Out {
    let (v, _, _) = via_names(idx);
    pairs(&format!("V{idx}.2a"), space(ctx.l(&v), 0.26))
}
fn via_rule_3ab(ctx: &Ctx, idx: usize) -> Out {
    let (v, lower, _) = via_names(idx);
    let (via, lm) = (ctx.l(&v), ctx.l(&lower));
    let l1 = rg::not(via, lm);
    if idx == 1 {
        polys("V1.3a", l1)
    } else {
        let l2 = ep_polygons(&enclosed(via, lm, 0.01), 1);
        polys(&format!("V{idx}.3b"), rg::or(&l1, &rg::merge(&l2)))
    }
}
fn via_rule_4a(ctx: &Ctx, idx: usize) -> Out {
    let (v, _, upper) = via_names(idx);
    let (via, um_) = (ctx.l(&v), ctx.l(&upper));
    let l1 = ep_polygons(&enclosed(via, um_, 0.01), 1);
    let l2 = rg::not(via, um_);
    polys(&format!("V{idx}.4a"), rg::or(&rg::merge(&l1), &l2))
}

/// end-of-line overlap (V.3c / V.4b / CO.6a): metal narrower than 0.34 must extend `0.06` past the cut at its line end
fn eol_rule(cut: &[Poly], metal: &[Poly], min_len: f64) -> Vec<EdgePair> {
    let mut o = CheckOpts::new(Relation::Width, um(0.34) + 1, Metrics::Euclidian);
    o.zero = ZeroMode::WhenTouching;
    let cond: Vec<EdgePair> = single_layer_check(&o, metal).into_iter().filter(|ep| ep.first.length() >= um(min_len) as i64 && ep.second.length() >= um(min_len) as i64).collect();
    let e1: Vec<Edge> = ed::merge(&cond.iter().map(|ep| ep.first).collect::<Vec<_>>());
    let e2: Vec<Edge> = ed::merge(&cond.iter().map(|ep| ep.second).collect::<Vec<_>>());
    let short: Vec<Edge> = rg::edges_of(metal).into_iter().filter(|e| e.length() < um(0.34) as i64).collect();
    let eol = ed::interacting(&ed::interacting(&short, &e1), &e2);
    let eol = ed::not(&ed::not(&eol, &e1), &e2);
    let o = CheckOpts::new(Relation::Inside, um(0.06), Metrics::Projection);
    edges_check(&o, &rg::edges_of(cut), &eol)
}
fn via_rule_3c(ctx: &Ctx, idx: usize) -> Out {
    let (v, lower, _) = via_names(idx);
    pairs(&format!("V{idx}.3c"), eol_rule(ctx.l(&v), ctx.l(&lower), 0.28))
}
fn via_rule_4b(ctx: &Ctx, idx: usize) -> Out {
    let (v, _, upper) = via_names(idx);
    pairs(&format!("V{idx}.4b"), eol_rule(ctx.l(&v), ctx.l(&upper), 0.28))
}

/// "adjacent edge" rule (V.3d / V.4c / CO.6b): if the metal overlaps the cut by < 0.04 on one side,
/// the adjacent sides need 0.06.
fn adjacent_rule(cut: &[Poly], metal: &[Poly], polygons_enl: i32) -> Vec<Poly> {
    let cut_edges = rg::edges_of(cut);
    let o = CheckOpts::new(Relation::Inside, um(0.04), Metrics::Projection);
    let enc = two_layer_check(&o, cut, metal);
    let enc_edges: Vec<Edge> = enc.iter().flat_map(|ep| [ep.first, ep.second]).collect();
    // edges of the cut that are "not outside" the violating edge parts (collinear overlap)
    let cond_edges = ed::not_outside(&cut_edges, &enc_edges);
    let check_corner = ed::extended_in(&cond_edges, 2);
    let check = ed::not(&ed::interacting_region(&cut_edges, &check_corner), &cond_edges);
    // corners where two "cond" edges meet
    let mut ow = CheckOpts::new(Relation::Width, 2, Metrics::Euclidian);
    ow.ignore_angle = 135.0;
    ow.shielded = false;
    let cond_corner = ep_polygons(&ed::self_check(&ow, &cond_edges), polygons_enl);
    let o = CheckOpts::new(Relation::Inside, um(0.06), Metrics::Projection);
    let l1 = ep_polygons(&edges_check(&o, &check, &rg::edges_of(metal)), polygons_enl);
    let l2 = rg::interacting(cut, &cond_corner);
    rg::or(&rg::merge(&l1), &l2)
}
fn via_rule_3d(ctx: &Ctx, idx: usize) -> Out {
    let (v, lower, _) = via_names(idx);
    polys(&format!("V{idx}.3d"), adjacent_rule(ctx.l(&v), ctx.l(&lower), 0))
}
fn via_rule_4c(ctx: &Ctx, idx: usize) -> Out {
    let (v, _, upper) = via_names(idx);
    polys(&format!("V{idx}.4c"), adjacent_rule(ctx.l(&v), ctx.l(&upper), 0))
}

/// 4x4-or-larger array spacing (V.2b / CO.2b)
fn array_rule(cut: &[Poly], size: f64, grow: f64, sp: f64, d: f64, proj_min: Option<f64>) -> Vec<EdgePair> {
    let edge_len = um(size * 3.0 + 3.0 * sp);
    let g = um(grow);
    // grow, merge, shrink; a merged blob can only pass the bbox filter after the shrink if its
    // bbox is at least edge_len + 2g, so the (expensive) shrink is applied to those only
    let grown: Vec<Poly> = cut.iter().map(|p| { let b = p.bbox(); debug_assert!(p.is_box()); Poly::rect(b.x1 - g, b.y1 - g, b.x2 + g, b.y2 + g) }).collect();
    let cand = rg::merge_boxes_min_bbox(&grown, edge_len + 2 * g);
    let poss = rg::sized(&cand, -g, -g);
    let big: Region = poss.into_iter().filter(|p| { let b = p.bbox(); b.width().min(b.height()) >= edge_len }).collect();
    let all = rg::interacting_min(&big, cut, 16);
    let mut o = CheckOpts::new(Relation::Width, edge_len, Metrics::Euclidian);
    o.min_projection = edge_len as i64;
    o.max_projection = 1000 * edge_len as i64;
    let exc = rg::merge(&ep_polygons(&single_layer_check(&o, &all), 0));
    let loc = rg::not_interacting(&all, &exc);
    let selected = rg::interacting(cut, &loc);
    let mut o = CheckOpts::new(Relation::Space, um(d), Metrics::Euclidian);
    if let Some(p) = proj_min {
        o.min_projection = um(p) as i64;
    }
    single_layer_check(&o, &selected)
}
fn via_rule_2b(ctx: &Ctx, idx: usize) -> Out {
    let (v, _, _) = via_names(idx);
    pairs(&format!("V{idx}.2b"), array_rule(ctx.l(&v), 0.26, 0.2, 0.36, 0.36, Some(0.26)))
}

// ---------------------------------------------------------------- contact.rb
fn contact_rules() -> Vec<(&'static str, RuleFn)> {
    let mut v: Vec<(&'static str, RuleFn)> = Vec::new();
    v.push(("CO.1", Box::new(|c| edges("CO.1", rg::edges_of(c.l("contact")).into_iter().filter(|e| e.length() != 220).collect()))));
    v.push(("CO.2a", Box::new(|c| pairs("CO.2a", space(c.l("contact"), 0.25)))));
    v.push(("CO.2b", Box::new(|c| pairs("CO.2b", array_rule(c.l("contact"), 0.22, 0.16, 0.28, 0.28, None)))));
    v.push(("CO.3", Box::new(|c| {
        let main = rg::not(c.l("contact"), c.l("sramcore"));
        let l1 = ep_polygons(&enclosed(&main, c.l("poly2"), 0.07), 1);
        let l2 = rg::not(&rg::not_outside(&main, c.l("poly2")), c.l("poly2"));
        polys("CO.3", rg::or(&rg::merge(&l1), &l2))
    })));
    v.push(("CO.4", Box::new(|c| {
        let main = rg::not(c.l("contact"), c.l("sramcore"));
        let l1 = ep_polygons(&enclosed(&main, c.l("comp"), 0.07), 1);
        let l2 = rg::not(&rg::not_outside(&main, c.l("comp")), c.l("comp"));
        polys("CO.4", rg::or(&rg::merge(&l1), &l2))
    })));
    v.push(("CO.5a", Box::new(|c| {
        let butted = rg::outside(&rg::interacting(c.l("ncomp"), c.l("pcomp")), c.l("pcomp"));
        let chk = rg::interacting(c.l("contact"), &butted);
        pairs("CO.5a", enclosed(&chk, &butted, 0.1))
    })));
    v.push(("CO.5b", Box::new(|c| {
        let butted = rg::outside(&rg::interacting(c.l("pcomp"), c.l("ncomp")), c.l("ncomp"));
        let chk = rg::interacting(c.l("contact"), &butted);
        pairs("CO.5b", enclosed(&chk, &butted, 0.1))
    })));
    v.push(("CO.6", Box::new(|c| {
        let l1 = ep_polygons(&enclosed(c.l("contact"), c.l("metal1"), 0.005), 1);
        let l2 = rg::not(c.l("contact"), c.l("metal1"));
        polys("CO.6", rg::or(&rg::merge(&l1), &l2))
    })));
    v.push(("CO.6a", Box::new(|c| pairs("CO.6a", eol_rule(c.l("contact"), c.l("metal1"), 0.24)))));
    v.push(("CO.6b", Box::new(|c| {
        let main = rg::not(c.l("contact"), c.l("sramcore"));
        polys("CO.6b", adjacent_rule(&main, c.l("metal1"), 1))
    })));
    v.push(("CO.7", Box::new(|c| {
        let a = rg::not(&rg::and(c.l("contact"), c.l("comp")), c.l("otp_mk"));
        let b = rg::not(c.l("tgate"), c.l("otp_mk"));
        pairs("CO.7", separation(&a, &b, 0.15))
    })));
    v.push(("CO.8", Box::new(|c| pairs("CO.8", separation(&rg::and(c.l("contact"), c.l("poly2")), c.l("comp"), 0.17)))));
    v.push(("CO.9", Box::new(|c| {
        let butt = ed::and(&rg::edges_of(c.l("ncomp")), &rg::edges_of(c.l("pcomp")));
        polys("CO.9", ed::region_interacting_edges(c.l("contact"), &butt))
    })));
    v.push(("CO.10", Box::new(|c| polys("CO.10", rg::and(c.l("contact"), c.l("tgate"))))));
    v.push(("CO.11", Box::new(|c| polys("CO.11", rg::not(&rg::not(c.l("contact"), c.l("poly2")), c.l("comp"))))));
    v
}

// ---------------------------------------------------------------- comp.rb (latch-up style rule)
/// DF.13: every PCOMP in Nwell must be within 20um (LV) / 15um (MV) of an Nwell tap,
/// measured by growing the tap 0.5um at a time inside the Nwell.
fn df13(ctx: &Ctx) -> Out {
    let (ntap, nwell, pactive) = (ctx.l("ntap"), ctx.l("nwell"), ctx.l("pactive"));
    let grow = |steps: usize| {
        let mut r = ntap.clone();
        for _ in 0..steps {
            r = rg::and(&rg::sized(&r, 500, 500), nwell);
        }
        r
    };
    let by15 = grow(30);
    let mut by20 = by15.clone();
    for _ in 0..10 {
        by20 = rg::and(&rg::sized(&by20, 500, 500), nwell);
    }
    let lv = rg::not_interacting(&rg::not_interacting(pactive, ctx.l("v5_xtor")), ctx.l("dualgate"));
    let mv = rg::overlapping(pactive, ctx.l("dualgate"));
    vec![
        ("DF.13_LV".to_string(), Markers::Polys(rg::not_interacting(&lv, &by20))),
        ("DF.13_MV".to_string(), Markers::Polys(rg::not_interacting(&mv, &by15))),
    ]
}

// ---------------------------------------------------------------- driver
fn derive(ctx: &mut Ctx) {
    // generic_layers.rb: metalN = drawn + dummy, device layers
    for i in 1..=5 {
        let names: [&'static str; 5] = ["metal1", "metal2", "metal3", "metal4", "metal5"];
        let d = ctx.l(&format!("metal{i}_drawn")).clone();
        let dm = ctx.l(&format!("metal{i}_dummy")).clone();
        let m = if dm.is_empty() { d } else { rg::or(&d, &dm) };
        ctx.set(names[i - 1], m);
    }
    let dnwell_n = rg::not(ctx.l("dnwell"), ctx.l("lvpwell"));
    let all_nwell = rg::or(&dnwell_n, ctx.l("nwell"));
    let ncomp = rg::and(ctx.l("comp"), ctx.l("nplus"));
    let pcomp = rg::and(ctx.l("comp"), ctx.l("pplus"));
    let tgate = rg::not(&rg::and(ctx.l("poly2"), ctx.l("comp")), ctx.l("res_mk"));
    let pactive = rg::and(&pcomp, &all_nwell);
    let ntap = rg::not(&rg::and(&ncomp, &all_nwell), ctx.l("res_mk"));
    ctx.set("all_nwell", all_nwell);
    ctx.set("ncomp", ncomp);
    ctx.set("pcomp", pcomp);
    ctx.set("tgate", tgate);
    ctx.set("pactive", pactive);
    ctx.set("ntap", ntap);
}

fn rule_list() -> Vec<(String, RuleFn)> {
    let mut v: Vec<(String, RuleFn)> = Vec::new();
    for (g, layers) in GEOM_GROUPS {
        let layers: Vec<&'static str> = layers.to_vec();
        v.push((g.to_string(), Box::new(move |c| layers.iter().flat_map(|l| offgrid_angle(c, l, l)).collect())));
    }
    for idx in 1..=4usize {
        v.push((format!("M{idx}.1"), Box::new(move |c| metal_1(c, idx))));
        v.push((format!("M{idx}.2a"), Box::new(move |c| metal_2a(c, idx))));
        v.push((format!("M{idx}.2b"), Box::new(move |c| metal_2b(c, idx))));
        v.push((format!("M{idx}.3"), Box::new(move |c| metal_3(c, idx))));
    }
    v.push(("metaltop".to_string(), Box::new(metaltop_rules)));
    for idx in 1..=4usize {
        v.push((format!("V{idx}.1"), Box::new(move |c| via_rule_1(c, idx))));
        v.push((format!("V{idx}.2a"), Box::new(move |c| via_rule_2a(c, idx))));
        v.push((format!("V{idx}.2b"), Box::new(move |c| via_rule_2b(c, idx))));
        v.push((format!("V{idx}.3ab"), Box::new(move |c| via_rule_3ab(c, idx))));
        v.push((format!("V{idx}.3c"), Box::new(move |c| via_rule_3c(c, idx))));
        v.push((format!("V{idx}.3d"), Box::new(move |c| via_rule_3d(c, idx))));
        v.push((format!("V{idx}.4a"), Box::new(move |c| via_rule_4a(c, idx))));
        v.push((format!("V{idx}.4b"), Box::new(move |c| via_rule_4b(c, idx))));
        v.push((format!("V{idx}.4c"), Box::new(move |c| via_rule_4c(c, idx))));
    }
    for (n, f) in contact_rules() {
        v.push((n.to_string(), f));
    }
    v.push(("DF.13".to_string(), Box::new(df13)));
    v
}

pub fn run(lib: &Library, top: &str, threads: usize, only: Option<&[String]>) -> RunResult {
    let mut phases = Vec::new();
    let t = Instant::now();
    let flat = lib.flatten(top);
    phases.push(("flatten".to_string(), t.elapsed().as_secs_f64()));

    let t = Instant::now();
    let by_key: HashMap<LayerKey, &'static str> = LAYERS.iter().map(|(n, l, d)| ((*l, *d), *n)).collect();
    let work: Vec<(&'static str, &Vec<Poly>)> = flat.iter().filter_map(|(k, v)| by_key.get(k).map(|n| (*n, v))).collect();
    let merged: Vec<(&'static str, Region)> = if threads > 1 {
        work.par_iter().map(|(n, v)| (*n, rg::merge_boxes(v))).collect()
    } else {
        work.iter().map(|(n, v)| (*n, rg::merge_boxes(v))).collect()
    };
    let mut ctx = Ctx { layers: merged.into_iter().collect(), empty: vec![] };
    phases.push(("merge layers".to_string(), t.elapsed().as_secs_f64()));

    let t = Instant::now();
    derive(&mut ctx);
    phases.push(("derive layers".to_string(), t.elapsed().as_secs_f64()));

    let t = Instant::now();
    let list: Vec<(String, RuleFn)> = rule_list().into_iter().filter(|(n, _)| only.map(|o| o.iter().any(|x| n.starts_with(x.as_str()))).unwrap_or(true)).collect();
    let exec = |(_, f): &(String, RuleFn)| -> Vec<RuleResult> {
        let t = Instant::now();
        let out = f(&ctx);
        let dt = t.elapsed().as_secs_f64() / out.len().max(1) as f64;
        out.into_iter().map(|(name, markers)| RuleResult { name, markers, seconds: dt }).collect()
    };
    let rules: Vec<RuleResult> = if threads > 1 { list.par_iter().flat_map(exec).collect() } else { list.iter().flat_map(exec).collect() };
    phases.push(("rules".to_string(), t.elapsed().as_secs_f64()));
    RunResult { rules, phases }
}

pub fn to_json(res: &RunResult, simple: &SimpleResult) -> String {
    let mut rules = serde_json::Map::new();
    for r in &res.rules {
        rules.insert(r.name.clone(), serde_json::json!({ "count": r.markers.len(), "markers": r.markers.strings(), "ms": r.seconds * 1e3 }));
    }
    let checks: serde_json::Map<String, serde_json::Value> = simple.checks.iter().map(|(n, m)| (n.clone(), serde_json::Value::String(m.clone()))).collect();
    serde_json::to_string(&serde_json::json!({ "top": simple.top, "checks": checks, "rules": rules })).unwrap()
}
