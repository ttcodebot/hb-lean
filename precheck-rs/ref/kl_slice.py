"""Reference for the ported slice using KLayout's own geometry engine (klayout.db wheel, flat mode).

The rule bodies are transcribed from the PDK's Ruby deck to the Region/Edges API that the DRC
layer wraps. Output: the same JSON marker format as precheck-rs, for exact comparison.
usage: kl_slice.py in.gds out.json [--deep]
"""
import json, sys, time
import klayout.db as k

R = k.Region
E = k.Metrics.Euclidian
P = k.Metrics.Projection
ZT = R.IncludeZeroDistanceWhenTouching
t00 = time.time()
ly = k.Layout(); ly.read(sys.argv[1]); top = ly.top_cell()
deep = "--deep" in sys.argv
dss = k.DeepShapeStore() if deep else None
t_read = time.time() - t00

def inp(l, d):
    li = ly.find_layer(l, d)
    if li is None:
        return R()
    it = top.begin_shapes_rec(li)
    r = R(it, dss) if deep else R(it)
    return r if deep else r.merged()

LAYERS = dict(comp=(22,0), dnwell=(12,0), nwell=(21,0), lvpwell=(204,0), dualgate=(55,0), poly2=(30,0), nplus=(32,0), pplus=(31,0),
  sab=(49,0), esd=(24,0), resistor=(62,0), fhres=(227,0), fusetop=(75,0), fusewindow_d=(96,1), polyfuse=(220,0), mvsd=(210,0),
  mvpsd=(11,39), nat=(5,0), comp_dummy=(22,4), poly2_dummy=(30,4), schottky_diode=(241,0), zener=(178,0), res_mk=(110,5),
  opc_drc=(124,5), v5_xtor=(112,1), cap_mk=(117,5), mos_cap_mk=(166,5), ind_mk=(151,5), diode_mk=(115,5), drc_bjt=(127,5),
  lvs_bjt=(118,5), mim_l_mk=(117,10), latchup_mk=(137,5), guard_ring_mk=(167,5), otp_mk=(173,5), mtpmark=(122,5),
  neo_ee_mk=(88,17), sramcore=(108,5), lvs_rf=(100,5), lvs_drain=(100,7), hvpolyrs=(123,5), lvs_io=(119,5), probe_mk=(13,17),
  esd_mk=(24,5), lvs_source=(100,8), well_diode_mk=(153,51), ldmos_xtor=(226,0), plfuse=(125,5), efuse_mk=(80,5),
  mcell_feol_mk=(11,17), ymtp_mk=(86,17), dev_wf_mk=(128,17), comp_label=(22,10), poly2_label=(30,10), mdiode=(116,5),
  ndmy=(111,5), pmndmy=(152,5), pad=(37,0), ubmpperi=(183,0), ubmparray=(184,0), ubmeplate=(185,0), pr_bndry=(0,0),
  border=(63,0), contact=(33,0), via1=(35,0), via2=(38,0), via3=(40,0), via4=(41,0))
for i, n in enumerate((34, 36, 42, 46, 81), 1):
    for suf, dt in (("drawn",0),("slot",3),("dummy",4),("label",10),("blk",5)):
        LAYERS[f"metal{i}_{suf}"] = (n, dt)
    LAYERS[f"metal{i}_res"] = (110, 10 + i)
t0 = time.time()
L = {n: inp(*ld) for n, ld in LAYERS.items()}
for i in range(1, 6):
    L[f"metal{i}"] = L[f"metal{i}_drawn"] + L[f"metal{i}_dummy"]
L["dnwell_n"] = L["dnwell"] - L["lvpwell"]
L["all_nwell"] = L["dnwell_n"] + L["nwell"]
L["ncomp"] = L["comp"] & L["nplus"]
L["pcomp"] = L["comp"] & L["pplus"]
L["tgate"] = (L["poly2"] & L["comp"]) - L["res_mk"]
L["pactive"] = L["pcomp"] & L["all_nwell"]
L["ntap"] = (L["ncomp"] & L["all_nwell"]) - L["res_mk"]
t_layers = time.time() - t0
um = lambda v: int(round(v * 1000))
out = {}
times = {}

def emit(name, obj):
    ms = []
    if isinstance(obj, k.EdgePairs):
        for ep in obj.each():
            a = tuple(sorted([(ep.first.p1.x, ep.first.p1.y), (ep.first.p2.x, ep.first.p2.y)]))
            b = tuple(sorted([(ep.second.p1.x, ep.second.p1.y), (ep.second.p2.x, ep.second.p2.y)]))
            a, b = sorted([a, b])
            ms.append("ep:%d,%d,%d,%d/%d,%d,%d,%d" % (*a[0], *a[1], *b[0], *b[1]))
    elif isinstance(obj, k.Edges):
        for e in obj.each():
            a = sorted([(e.p1.x, e.p1.y), (e.p2.x, e.p2.y)])
            ms.append("edge:%d,%d,%d,%d" % (*a[0], *a[1]))
    else:
        for p in obj.each():
            def ring(pts, cw):
                pts = [(q.x, q.y) for q in pts]
                a = sum(pts[i][0] * pts[(i + 1) % len(pts)][1] - pts[(i + 1) % len(pts)][0] * pts[i][1] for i in range(len(pts)))
                if (a > 0) == cw:
                    pts.reverse()
                i = pts.index(min(pts)); pts = pts[i:] + pts[:i]
                return ";".join("%d,%d" % q for q in pts)
            s = "poly:" + ring(list(p.each_point_hull()), True)
            for h in sorted(ring(list(p.each_point_hole(i)), False) for i in range(p.holes())):
                s += "/" + h
            ms.append(s)
    out[name] = {"count": len(ms), "markers": sorted(ms)}

import os
ONLY = os.environ.get('ONLY')
def timed(name, f):
    if ONLY and not any(name.startswith(o) for o in ONLY.split(',')): return
    t = time.time(); f(); times[name] = time.time() - t

def chk(kind, a, d, other=None, metrics=E, **kw):
    f = getattr(a, kind + "_check")
    args = dict(metrics=metrics, zero_distance_mode=kw.pop("zd", ZT)); args.update(kw)
    return f(other, d, **args) if other is not None else f(d, **args)

# ---- geom
GEOM = ["comp","dnwell","nwell","lvpwell","dualgate","poly2","nplus","pplus","sab","esd","resistor","fhres","fusetop","fusewindow_d",
 "polyfuse","mvsd","mvpsd","nat","comp_dummy","poly2_dummy","schottky_diode","zener","res_mk","opc_drc","ndmy","pmndmy","v5_xtor",
 "cap_mk","mos_cap_mk","ind_mk","diode_mk","drc_bjt","lvs_bjt","mim_l_mk","latchup_mk","guard_ring_mk","otp_mk","mtpmark","neo_ee_mk",
 "sramcore","lvs_rf","lvs_drain","hvpolyrs","lvs_io","probe_mk","esd_mk","lvs_source","well_diode_mk","ldmos_xtor","plfuse","efuse_mk",
 "mcell_feol_mk","ymtp_mk","dev_wf_mk","comp_label","poly2_label","mdiode","contact","metal1","metal1_slot","metal1_blk","metal1_label",
 "metal1_res","via1","metal2","metal2_label","metal2_slot","metal2_blk","metal2_res","via2","metal3","metal3_dummy","metal3_label",
 "metal3_blk","metal3_res","via3","metal4","metal4_dummy","metal4_label","metal4_blk","metal4_res","via4","metal5","metal5_dummy",
 "metal5_label","metal5_blk","metal5_res","pad","ubmpperi","ubmparray","ubmeplate","pr_bndry","border"]
def geom():
    for n in GEOM:
        emit(f"{n}_OFFGRID", L[n].grid_check(5, 5))
        e = L[n].edges()
        for a in (0, 45, 90, -45):
            e = e.with_angle(a, True)
        emit(f"{n}_ACUTE", e)
timed("geom", geom)

# ---- metal
def metal(i):
    d = L[f"metal{i}_drawn"]; w = um(0.23 if i == 1 else 0.28)
    if i == 1:
        sram_lv = L["sramcore"].not_interacting(L["v5_xtor"]).not_interacting(L["dualgate"])
        emit("M1.1", chk("width", d - sram_lv, w))
    else:
        emit(f"M{i}.1", chk("width", d, w))
    emit(f"M{i}.2a", chk("space", d, w))
    wide = d.sized(-5000, 0, 2).sized(0, -5000, 2).sized(5000, 0, 2).sized(0, 5000, 2) & d
    emit(f"M{i}.2b", chk("separation", d, um(0.3), wide, zd=R.NeverIncludeZeroDistance))
    emit(f"M{i}.3", d.with_area(None, 144400, False))
for i in range(1, 5):
    timed(f"metal{i}", lambda: metal(i))
def metaltop():
    t = L["metal5"]
    emit("MT.1", chk("width", t, um(0.44))); emit("MT.2a", chk("space", t, um(0.46)))
    wide = t.not_interacting(t.edges().with_length(None, 10005, False))
    emit("MT.2b", chk("separation", t, um(0.6), wide)); emit("MT.4", t.with_area(None, 562500, False))
timed("metaltop", metaltop)

# ---- shared helpers
def eol_rule(cut, metal, min_len):
    cond = chk("width", metal, um(0.34) + 1).with_length_both(um(min_len), None, False)
    e1 = cond.first_edges(); e2 = cond.second_edges()
    eol = metal.edges().with_length(None, um(0.34), False).interacting(e1).interacting(e2) - e1 - e2
    return cut.edges().enclosed_check(eol, um(0.06), metrics=P)

def adjacent_rule(cut, metal, enl):
    ce = cut.edges()
    cond = ce.not_outside(chk("enclosed", cut, um(0.04), metal, metrics=P).edges())
    corner = cond.extended_in(2)
    check = ce.interacting(corner) - cond
    cc = cond.width_check(2, ignore_angle=135).polygons(enl)
    l1 = check.enclosed_check(metal.edges(), um(0.06), metrics=P).polygons(enl)
    return l1 | cut.interacting(cc)

def array_rule(cut, size, grow, sp, d, proj_min):
    el = um(size * 3 + 3 * sp); g = um(grow)
    poss = cut.sized(g, g, 3).merged().sized(-g, -g, 3)
    allr = poss.with_bbox_min(el, None, False).interacting(cut, 16)
    exc = allr.width_check(el, min_projection=el, max_projection=1000 * el).polygons()
    sel = cut.interacting(allr.not_interacting(exc))
    kw = dict(min_projection=um(proj_min)) if proj_min else {}
    return chk("space", sel, um(d), **kw)

# ---- via
def via(i, rule):
    v = L[f"via{i}"]; lo = L[f"metal{i}"]; up = L[f"metal{i+1}"]
    if rule == "1": emit(f"V{i}.1", v.edges().with_length(260, True))
    elif rule == "2a": emit(f"V{i}.2a", chk("space", v, 260))
    elif rule == "2b": emit(f"V{i}.2b", array_rule(v, 0.26, 0.2, 0.36, 0.36, 0.26))
    elif rule == "3ab":
        l1 = v - lo
        if i == 1: emit("V1.3a", l1)
        else: emit(f"V{i}.3b", l1 | chk("enclosed", v, 10, lo).polygons(1))
    elif rule == "3c": emit(f"V{i}.3c", eol_rule(v, lo, 0.28))
    elif rule == "3d": emit(f"V{i}.3d", adjacent_rule(v, lo, 0))
    elif rule == "4a": emit(f"V{i}.4a", chk("enclosed", v, 10, up).polygons(1) | (v - up))
    elif rule == "4b": emit(f"V{i}.4b", eol_rule(v, up, 0.28))
    elif rule == "4c": emit(f"V{i}.4c", adjacent_rule(v, up, 0))
for i in range(1, 5):
    for r in ("1", "2a", "2b", "3ab", "3c", "3d", "4a", "4b", "4c"):
        timed(f"V{i}.{r}", lambda: via(i, r))

# ---- contact
c = L["contact"]; main = c - L["sramcore"]
timed("CO.1", lambda: emit("CO.1", c.edges().with_length(220, True)))
timed("CO.2a", lambda: emit("CO.2a", chk("space", c, 250)))
timed("CO.2b", lambda: emit("CO.2b", array_rule(c, 0.22, 0.16, 0.28, 0.28, None)))
timed("CO.3", lambda: emit("CO.3", chk("enclosed", main, 70, L["poly2"]).polygons(1) | (main.not_outside(L["poly2"]) - L["poly2"])))
timed("CO.4", lambda: emit("CO.4", chk("enclosed", main, 70, L["comp"]).polygons(1) | (main.not_outside(L["comp"]) - L["comp"])))
def co5(a, b, name):
    butted = L[a].interacting(L[b]).not_overlapping(L[b])
    emit(name, chk("enclosed", c.interacting(butted), 100, butted))
timed("CO.5a", lambda: co5("ncomp", "pcomp", "CO.5a")); timed("CO.5b", lambda: co5("pcomp", "ncomp", "CO.5b"))
timed("CO.6", lambda: emit("CO.6", chk("enclosed", c, 5, L["metal1"]).polygons(1) | (c - L["metal1"])))
timed("CO.6a", lambda: emit("CO.6a", eol_rule(c, L["metal1"], 0.24)))
timed("CO.6b", lambda: emit("CO.6b", adjacent_rule(main, L["metal1"], 1)))
timed("CO.7", lambda: emit("CO.7", chk("separation", (c & L["comp"]) - L["otp_mk"], 150, L["tgate"] - L["otp_mk"])))
timed("CO.8", lambda: emit("CO.8", chk("separation", c & L["poly2"], 170, L["comp"])))
timed("CO.9", lambda: emit("CO.9", c.interacting(L["ncomp"].edges() & L["pcomp"].edges())))
timed("CO.10", lambda: emit("CO.10", c & L["tgate"]))
timed("CO.11", lambda: emit("CO.11", c - L["poly2"] - L["comp"]))

# ---- DF.13
def df13():
    r = L["ntap"]
    for _ in range(30): r = r.sized(500, 2) & L["nwell"]
    by15 = r
    for _ in range(10): r = r.sized(500, 2) & L["nwell"]
    lv = L["pactive"].not_interacting(L["v5_xtor"]).not_interacting(L["dualgate"])
    mv = L["pactive"].overlapping(L["dualgate"])
    emit("DF.13_LV", lv.not_interacting(r)); emit("DF.13_MV", mv.not_interacting(by15))
timed("DF.13", df13)

json.dump({"rules": out, "times": times, "read_s": t_read, "layers_s": t_layers, "total_s": time.time() - t00}, open(sys.argv[2], "w"))
print("klayout.db %s %s: read %.3fs layers %.3fs rules %.3fs total %.3fs; violations %d" % (k.__version__, "deep" if deep else "flat", t_read, t_layers, sum(times.values()), time.time() - t00, sum(v["count"] for v in out.values())))
for n, t in sorted(times.items(), key=lambda x: -x[1])[:12]:
    print("  %-10s %.3f s" % (n, t))
