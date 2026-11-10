#!/usr/bin/env python3
"""Extract a compact animation scene (JSON) from a LibreLane run directory.

Usage: extract.py RUN_DIR --lef cells.lef --lef tech.tlef -o scene.json

Only reads per-step DEF snapshots, the global-routing guide file and step
logs/runtime.txt. No OpenROAD/KLayout needed; standard library only.
"""

import argparse
import glob
import json
import os
import re
import sys
import time

SNAPS = [
    ("floorplan", "openroad-floorplan"),
    ("tap", "openroad-tapendcapinsertion"),
    ("pdn", "openroad-generatepdn"),
    ("io", "odb-applydeftemplate"),
    ("gpl", "openroad-globalplacement"),
    ("rsz", "openroad-repairdesignpostgpl"),
    ("dpl", "openroad-detailedplacement"),
    ("cts", "openroad-cts"),
    ("rsz2", "openroad-resizertimingpostcts"),
    ("grt", "openroad-globalrouting"),
    ("drt", "openroad-detailedrouting"),
    ("fill", "openroad-fillinsertion"),
]


def step_dir(run, suffix):
    c = sorted(glob.glob(os.path.join(run, "[0-9]*-" + suffix)))
    return c[0] if c else None


def def_file(d):
    c = glob.glob(os.path.join(d, "*.def")) if d else []
    return c[0] if c else None


def runtime_s(d):
    try:
        h, m, s = open(os.path.join(d, "runtime.txt")).read().strip().split(":")
        return int(h) * 3600 + int(m) * 60 + float(s)
    except Exception:
        return None


def sections(text):
    """Return dict section name -> list of statements (token lists)."""
    out = {}
    for name in ("COMPONENTS", "PINS", "NETS", "SPECIALNETS"):
        m = re.search(r"^%s \d+ ;(.*?)^END %s" % (name, name), text, re.S | re.M)
        stmts = []
        if m:
            for s in m.group(1).split("\n    - ")[1:]:
                toks = s.split()
                if toks and toks[-1] == ";":
                    toks.pop()
                stmts.append(toks)
        out[name] = stmts
    return out


def parse_header(text):
    dbu = int(re.search(r"UNITS DISTANCE MICRONS (\d+)", text).group(1))
    m = re.search(r"DIEAREA \( (-?\d+) (-?\d+) \) \( (-?\d+) (-?\d+) \)", text)
    die = [int(x) for x in m.groups()]
    rows = []
    for m in re.finditer(
        r"^ROW \S+ \S+ (-?\d+) (-?\d+) \S+ DO (\d+) BY (\d+) STEP (\d+) (\d+)", text, re.M
    ):
        x, y, nx, ny, sx, sy = map(int, m.groups())
        rows.append([x, y, nx * sx])
    design = re.search(r"^DESIGN (\S+)", text, re.M).group(1)
    return design, dbu, die, rows


def parse_components(stmts):
    comps = {}
    for t in stmts:
        name, macro = t[0], t[1]
        pos = None
        src = None
        for i, tok in enumerate(t):
            if tok in ("PLACED", "FIXED") and t[i + 1] == "(":
                pos = (int(t[i + 2]), int(t[i + 3]), t[i + 5])
            elif tok == "SOURCE":
                src = t[i + 1]
        comps[name] = (macro, pos, src)
    return comps


def parse_route(t, i, special):
    """Parse routing starting at t[i] (token after ROUTED/FIXED/COVER).
    Returns (segments, vias, next index). segments: (layer, width|None, x1,y1,x2,y2, shape)"""
    segs, vias = [], []
    n = len(t)
    while i < n:
        layer = t[i]
        i += 1
        width = None
        shape = None
        if special:
            width = int(t[i])
            i += 1
        pts_prev = None
        while i < n:
            tok = t[i]
            if tok == "NEW":
                i += 1
                break
            if tok == "+":
                if t[i + 1] == "SHAPE":
                    shape = t[i + 2]
                    i += 3
                    continue
                return segs, vias, i
            if tok == "(":
                j = t.index(")", i)
                xs, ys = t[i + 1], t[i + 2]
                x = pts_prev[0] if xs == "*" else int(xs)
                y = pts_prev[1] if ys == "*" else int(ys)
                if pts_prev is not None and (x, y) != pts_prev:
                    segs.append((layer, width, pts_prev[0], pts_prev[1], x, y, shape))
                pts_prev = (x, y)
                i = j + 1
            elif tok in ("MASK", "TAPERRULE", "STYLE"):
                i += 2
            elif tok in ("TAPER", "VIRTUAL"):
                i += 1
            elif tok == "RECT":
                j = t.index(")", i)
                i = j + 1
            else:  # via name
                if pts_prev is not None:
                    vias.append((layer, tok, pts_prev[0], pts_prev[1]))
                i += 1
        else:
            break
    return segs, vias, i


def parse_nets(stmts, special=False):
    nets = []
    for t in stmts:
        name = t[0]
        conns = []
        i = 1
        n = len(t)
        while i < n and t[i] == "(":
            conns.append((t[i + 1], t[i + 2]))
            i = t.index(")", i) + 1
        use = "SIGNAL"
        segs, vias = [], []
        while i < n:
            if t[i] == "+":
                k = t[i + 1]
                if k == "USE":
                    use = t[i + 2]
                    i += 3
                elif k in ("ROUTED", "FIXED", "COVER"):
                    s, v, i = parse_route(t, i + 2, special)
                    segs += s
                    vias += v
                else:
                    i += 2
            else:
                i += 1
        nets.append((name, conns, use, segs, vias))
    return nets


def parse_pins(stmts):
    pins = {}
    for t in stmts:
        name = t[0]
        if "POWER" in t or "GROUND" in t:
            continue
        layer = rect = pos = None
        for i, tok in enumerate(t):
            if tok == "LAYER" and rect is None:
                layer = t[i + 1]
                rect = [int(t[i + 3]), int(t[i + 4]), int(t[i + 7]), int(t[i + 8])]
            elif tok in ("PLACED", "FIXED") and t[i + 1] == "(":
                pos = (int(t[i + 2]), int(t[i + 3]), t[i + 5])
        if rect and pos:
            x1, y1, x2, y2 = rect
            o = pos[2]
            if o in ("S", "FS"):
                y1, y2 = -y2, -y1
            if o in ("S", "FN"):
                x1, x2 = -x2, -x1
            if o in ("E", "W", "FE", "FW"):
                x1, y1, x2, y2 = y1, x1, y2, x2
            pins[name] = [pos[0] + x1, pos[1] + y1, x2 - x1, y2 - y1, layer]
    return pins


def parse_lef(paths):
    macros, layers = {}, {}
    for p in paths:
        txt = open(p, errors="replace").read()
        for m in re.finditer(r"^MACRO (\S+)(.*?)^END \1", txt, re.S | re.M):
            s = re.search(r"SIZE ([\d.]+) BY ([\d.]+)", m.group(2))
            c = re.search(r"CLASS ([^;]+);", m.group(2))
            if s:
                macros[m.group(1)] = (float(s.group(1)), float(s.group(2)), c.group(1).strip() if c else "")
        for m in re.finditer(r"^LAYER (\S+)\s+TYPE ROUTING(.*?)^END \1", txt, re.S | re.M):
            w = re.search(r"^\s*WIDTH ([\d.]+)", m.group(2), re.M)
            if w:
                layers.setdefault(m.group(1), float(w.group(1)))
    return macros, layers


def categorize(macro, lef_class):
    n = macro.lower().split("__")[-1]
    if re.match(r"(fill|decap|tap|endcap|filltie|filldecap|filler)", n) or "SPACER" in lef_class or "WELLTAP" in lef_class:
        return "phys"
    if re.match(r"(antenna|diode)", n):
        return "diode"
    if re.match(r"(clkbuf|clkinv|clkdlybuf|clkdlyinv)", n):
        return "clkbuf"
    if re.match(r"(buf|inv|dly)", n):
        return "buf"
    if re.match(r"(s?df|edf|dlx|dlr|lat|icg|dlclk|sdlclk)", n):
        return "seq"
    if re.match(r"(conb|tie)", n):
        return "tie"
    return "comb"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("run")
    ap.add_argument("--lef", action="append", default=[])
    ap.add_argument("-o", "--out", required=True)
    a = ap.parse_args()
    t0 = time.time()

    macros_lef, layer_w = parse_lef(a.lef)
    snaps = {}
    runtimes = {}
    for key, suffix in SNAPS:
        d = step_dir(a.run, suffix)
        f = def_file(d)
        if d:
            runtimes[key] = runtime_s(d)
        if f:
            snaps[key] = open(f).read()
    order = [k for k, _ in SNAPS if k in snaps]
    last = order[-1]
    design, dbu, die, rows = parse_header(snaps[last])

    comps_by = {k: parse_components(sections(snaps[k])["COMPONENTS"]) for k in order}

    # macro table
    macro_idx, macro_tab = {}, []

    def midx(m):
        if m not in macro_idx:
            w, h, cls = macros_lef.get(m, (1.0, 2.72, ""))
            if m not in macros_lef:
                print("warning: macro not in LEF:", m, file=sys.stderr)
            macro_idx[m] = len(macro_tab)
            macro_tab.append([m, round(w * dbu), round(h * dbu), categorize(m, cls)])
        return macro_idx[m]

    # cells: track position changes across snapshots
    final = comps_by[last]
    cells, fills, cell_index = [], [], {}
    allnames = dict.fromkeys(n for k in order for n in comps_by[k])
    for name in allnames:
        hist = []
        born = None
        lastpos = None
        macro = None
        death = 0
        for si, k in enumerate(order):
            c = comps_by[k].get(name)
            if c is None:
                if born is not None and not death:
                    death = si
                continue
            if c[1] is None:
                continue
            macro = c[0]
            if born is None:
                born = si
            p = (c[1][0], c[1][1])
            if p != lastpos:
                hist.append([si, p[0], p[1]])
                lastpos = p
        if born is None:
            continue
        mi = midx(macro)
        if order[born] == "fill":
            fills.append([mi, hist[0][1], hist[0][2]])
            continue
        netlist = name in comps_by[order[0]]
        cell_index[name] = len(cells)
        cells.append({"m": mi, "b": born, "n": 1 if netlist else 0, "p": hist, "d": death})

    # pins, nets, wires from last routed snapshot
    sec = sections(snaps[last])
    pins = parse_pins(sec["PINS"])
    pin_names = list(pins)
    pin_index = {n: i for i, n in enumerate(pin_names)}
    layer_names = []

    def lidx(l):
        if l not in layer_names:
            layer_names.append(l)
        return layer_names.index(l)

    # fix a sensible layer order up front from tech lef
    for l in layer_w:
        lidx(l)

    nets_out, wires, vias = [], [], []
    for ni, (name, conns, use, segs, vs) in enumerate(parse_nets(sec["NETS"])):
        ends = []
        for inst, pin in conns:
            if inst == "PIN":
                if pin in pin_index:
                    ends.append(-1 - pin_index[pin])
            elif inst in cell_index:
                ends.append(cell_index[inst])
        nets_out.append([ends, 1 if use == "CLOCK" else 0])
        for layer, _w, x1, y1, x2, y2, _s in segs:
            wires.append([lidx(layer), x1, y1, x2, y2, ni])
        for layer, _vn, x, y in vs:
            vias.append([lidx(layer), x, y, ni])

    pdn = []
    for name, conns, use, segs, vs in parse_nets(sec["SPECIALNETS"], special=True):
        for layer, w, x1, y1, x2, y2, shape in segs:
            pdn.append([lidx(layer), x1, y1, x2, y2, w, 1 if use == "GROUND" else 0])

    # global route guides
    guides = []
    gd = step_dir(a.run, "openroad-globalrouting")
    gf = glob.glob(os.path.join(gd, "*.guide")) if gd else []
    if gf:
        for line in open(gf[0]):
            p = line.split()
            if len(p) == 5:
                guides.append([lidx(p[4]), int(p[0]), int(p[1]), int(p[2]), int(p[3])])

    # logs
    gpl_log, drt_log = [], []
    d = step_dir(a.run, "openroad-globalplacement")
    if d:
        for f in glob.glob(os.path.join(d, "*.log")):
            for m in re.finditer(r"^\s+(\d+) \|\s+([\d.]+) \|\s+([\d.e+]+) \|", open(f).read(), re.M):
                gpl_log.append([int(m.group(1)), float(m.group(2)), float(m.group(3))])
    d = step_dir(a.run, "openroad-detailedrouting")
    if d:
        for f in glob.glob(os.path.join(d, "*.log")):
            txt = open(f).read()
            v = re.findall(r"Number of violations = (\d+)", txt)
            w = re.findall(r"Total wire length = (\d+) um", txt)
            for i, x in enumerate(v):
                drt_log.append([i, int(x), int(w[i]) if i < len(w) else None])

    pdk = lib = None
    try:
        r = json.load(open(os.path.join(a.run, "resolved.json")))
        pdk, lib = r.get("PDK"), r.get("STD_CELL_LIBRARY")
    except Exception:
        pass

    scene = {
        "design": design,
        "pdk": pdk,
        "lib": lib,
        "dbu": dbu,
        "die": die,
        "rows": rows,
        "rowH": macro_tab[0][2] if macro_tab else 0,
        "snaps": order,
        "runtime": runtimes,
        "macros": macro_tab,
        "cells": [[c["m"], c["b"], c["n"], c["p"], c["d"]] for c in cells],
        "fills": fills,
        "pins": [pins[n][:4] for n in pin_names],
        "layers": layer_names,
        "layerW": [round(layer_w.get(l, 0.2) * dbu) for l in layer_names],
        "nets": nets_out,
        "wires": wires,
        "vias": vias,
        "pdn": pdn,
        "guides": guides,
        "gplLog": gpl_log,
        "drtLog": drt_log,
    }
    with open(a.out, "w") as f:
        json.dump(scene, f, separators=(",", ":"))
    print(
        "%s: %d cells, %d fill, %d nets, %d wire segs, %d vias, %d pdn, %d guides; %.2fs, %d bytes"
        % (design, len(cells), len(fills), len(nets_out), len(wires), len(vias), len(pdn), len(guides), time.time() - t0, os.path.getsize(a.out)),
        file=sys.stderr,
    )


if __name__ == "__main__":
    main()
