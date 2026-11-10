// Flow animation player. Expects a global SCENE (see extract.py) and a <canvas id="c">.
// Exposes window.flowAnim = { duration, renderAt(t), play(), pause() }.
(function () {
  const S = SCENE;
  const W = 1200, H = 630;
  const VP = { x: 16, y: 16, w: 868, h: 528 }; // die viewport
  const PANEL = { x: 904, w: 280 };
  const COL = {
    bg: '#0b0f16', die: '#111722', row: '#1a2230', text: '#e6edf3', dim: '#7d8ba0', accent: '#ffd166',
    comb: '#4f7fd9', seq: '#f0a23a', buf: '#2fbfae', clk: '#ff4fd8', tie: '#77839a', diode: '#a78bfa',
    phys: '#4a5d80', physStroke: '#34415a', fill: '#22304a', pin: '#e6edf3',
  };
  const LAYER_COL = ['#a594ff', '#4cc9f0', '#ff5d8f', '#7ae582', '#ffd166', '#ff9f1c'];
  const dbu = S.dbu;
  const die = S.die;
  const dieW = die[2] - die[0], dieH = die[3] - die[1];

  // ---- stages -------------------------------------------------------------
  const snapIdx = {};
  S.snaps.forEach((s, i) => (snapIdx[s] = i));
  const DEFS = [
    ['floorplan', 'Floorplan', ['floorplan', 'io'], 0.9],
    ['pdn', 'Power grid + taps', ['tap', 'pdn'], 0.9],
    ['gpl', 'Global placement', ['gpl'], 1.7],
    ['dpl', 'Repair + legalise', ['rsz', 'dpl'], 1.1],
    ['cts', 'Clock tree', ['cts', 'rsz2'], 1.2],
    ['grt', 'Global routing', ['grt'], 0.9],
    ['drt', 'Detailed routing', ['drt'], 2.4],
    ['fill', 'Fill insertion', ['fill'], 0.8],
    ['final', 'Final layout', [], 1.3],
  ];
  const cellStage = (snap) => {
    // stage index for snapshot index
    for (let i = 0; i < stages.length; i++) if (stages[i].maxSnap >= snap) return i;
    return stages.length - 1;
  };
  let stages = DEFS.map((d) => {
    const idx = d[2].filter((s) => s in snapIdx).map((s) => snapIdx[s]);
    const rt = d[2].reduce((a, s) => a + (S.runtime[s] || 0), 0);
    return { id: d[0], label: d[1], maxSnap: idx.length ? Math.max(...idx) : -1, dur: d[3], rt };
  });
  // "io" snapshot sits after pdn in the flow; pins are simply shown in the floorplan stage.
  stages[0].maxSnap = snapIdx.floorplan ?? 0;
  stages[1].maxSnap = Math.max(snapIdx.io ?? 0, snapIdx.pdn ?? 0, snapIdx.tap ?? 0);
  stages[stages.length - 1].maxSnap = 1e9;
  const ctsSnaps = [snapIdx.cts, snapIdx.rsz2].filter((x) => x !== undefined);
  const hasCts = S.cells.some((c) => ctsSnaps.includes(c[1]));
  if (!hasCts) stages = stages.filter((s) => s.id !== 'cts');
  let acc = 0;
  stages.forEach((s) => { s.t0 = acc; acc += s.dur; });
  const DURATION = acc;
  const SI = {};
  stages.forEach((s, i) => (SI[s.id] = i));

  // ---- precompute cells ---------------------------------------------------
  const hash = (i) => { let x = Math.imul(i + 1, 2654435761) >>> 0; x ^= x >>> 15; x = Math.imul(x, 2246822519) >>> 0; x ^= x >>> 13; return (x >>> 0) / 4294967296; };
  const cells = S.cells.map((c, i) => {
    const m = S.macros[c[0]];
    const born = cellStage(c[1]);
    const pos = [];
    for (let s = 0; s < stages.length; s++) {
      let p = null;
      for (const h of c[3]) if (h[0] <= stages[s].maxSnap) p = h;
      pos.push(p ? [p[1], p[2]] : null);
    }
    let cat = m[3];
    if (cat === 'clkbuf') cat = hasCts && born === SI.cts ? 'clk' : 'buf';
    return { w: m[1], h: m[2], cat, born, pos, netlist: c[2], death: c[4] ? cellStage(c[4]) : 0, r: hash(i), r2: hash(i + 7777) };
  });
  const logic = cells.filter((c) => c.cat !== 'phys' && !c.death);
  // cluster start for global placement (synthetic: OpenROAD's real start is not in the snapshots)
  let cx0 = 0, cy0 = 0, nn = 0;
  for (const c of cells) if (c.born === SI.gpl && c.pos[SI.gpl]) { cx0 += c.pos[SI.gpl][0]; cy0 += c.pos[SI.gpl][1]; nn++; }
  cx0 = nn ? cx0 / nn : dieW / 2; cy0 = nn ? cy0 / nn : dieH / 2;

  // active region for camera
  let bb = [1e12, 1e12, -1e12, -1e12];
  const grow = (x, y) => { bb[0] = Math.min(bb[0], x); bb[1] = Math.min(bb[1], y); bb[2] = Math.max(bb[2], x); bb[3] = Math.max(bb[3], y); };
  for (const c of logic) for (const p of c.pos) if (p) { grow(p[0], p[1]); grow(p[0] + c.w, p[1] + c.h); }
  for (const w of S.wires) { grow(w[1], w[2]); grow(w[3], w[4]); }
  const fullCam = fitCam(die[0], die[1], die[2], die[3], 0.02);
  let zoomCam = fitCam(bb[0], bb[1], bb[2], bb[3], 0.05);
  const zoomOn = zoomCam.s > fullCam.s * 1.35;
  if (!zoomOn) zoomCam = fullCam;
  function fitCam(x1, y1, x2, y2, pad) {
    const w = (x2 - x1) * (1 + 2 * pad), h = (y2 - y1) * (1 + 2 * pad);
    let s = Math.min(VP.w / w, VP.h / h);
    s = Math.min(s, (VP.w / dieW) * 6);
    let cx = (x1 + x2) / 2, cy = (y1 + y2) / 2;
    // keep the view inside the die where possible
    const hw = VP.w / s / 2, hh = VP.h / s / 2;
    if (2 * hw < dieW) cx = Math.min(Math.max(cx, die[0] + hw), die[2] - hw); else cx = (die[0] + die[2]) / 2;
    if (2 * hh < dieH) cy = Math.min(Math.max(cy, die[1] + hh), die[3] - hh); else cy = (die[1] + die[3]) / 2;
    return { cx, cy, s };
  }

  // wires ordered by layer then net
  const wires = S.wires.slice().sort((a, b) => a[0] - b[0] || a[5] - b[5]);
  const vias = S.vias.slice().sort((a, b) => a[0] - b[0] || a[3] - b[3]);
  const usedLayers = [...new Set(wires.map((w) => w[0]))].sort((a, b) => a - b);
  const layerRange = {}, viaRange = {};
  usedLayers.forEach((l) => {
    layerRange[l] = [wires.findIndex((w) => w[0] === l), wires.filter((w) => w[0] === l).length];
    viaRange[l] = [vias.findIndex((v) => v[0] === l), vias.filter((v) => v[0] === l).length];
  });
  let tot = 0;
  const share = usedLayers.map((l) => Math.pow(layerRange[l][1], 0.6));
  share.forEach((x) => (tot += x));
  const layerT = {}; let a0 = 0;
  usedLayers.forEach((l, i) => { layerT[l] = [a0 / tot, (a0 + share[i]) / tot]; a0 += share[i]; });
  const wireUm = (() => { let s = 0; for (const w of S.wires) s += Math.abs(w[3] - w[1]) + Math.abs(w[4] - w[2]); return s / dbu; })();
  const fills = S.fills.map((f) => { const m = S.macros[f[0]]; return [f[1], f[2], m[1], m[2]]; });

  // ---- helpers ------------------------------------------------------------
  const clamp = (x) => Math.max(0, Math.min(1, x));
  const ease = (x) => { x = clamp(x); return x < 0.5 ? 4 * x * x * x : 1 - Math.pow(-2 * x + 2, 3) / 2; };
  const easeOut = (x) => 1 - Math.pow(1 - clamp(x), 3);
  const lerp = (a, b, t) => a + (b - a) * t;
  const fmt = (x, d = 0) => x.toLocaleString('en-US', { maximumFractionDigits: d, minimumFractionDigits: d });

  const canvas = document.getElementById('c');
  const ctx = canvas.getContext('2d');
  function resize() {
    const dpr = window.devicePixelRatio || 1;
    canvas.width = W * dpr; canvas.height = H * dpr;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  }
  resize();

  function stageAt(t) {
    t = Math.max(0, Math.min(DURATION - 1e-6, t));
    let i = stages.length - 1;
    while (i > 0 && stages[i].t0 > t) i--;
    return [i, (t - stages[i].t0) / stages[i].dur];
  }

  function camAt(si, u) {
    if (!zoomOn) return fullCam;
    let k;
    if (si < SI.gpl) k = 0;
    else if (si === SI.gpl) k = ease(u / 0.45);
    else if (si < SI.fill) k = 1;
    else if (si === SI.fill) k = 1 - ease(u / 0.8);
    else k = 0;
    return { cx: lerp(fullCam.cx, zoomCam.cx, k), cy: lerp(fullCam.cy, zoomCam.cy, k), s: Math.exp(lerp(Math.log(fullCam.s), Math.log(zoomCam.s), k)) };
  }

  function cellPos(c, si, u) {
    // returns [x, y, alpha, pop]
    if (c.born > si) return null;
    if (c.death && c.death <= si) {
      if (c.death < si) return null;
      const p = c.pos[si - 1]; if (!p) return null;
      return [p[0], p[1], 1 - clamp(u / 0.25), 0];
    }
    const p = c.pos[si];
    if (!p) return null;
    if (c.born === si) {
      if (si === SI.gpl) {
        const d = c.r * 0.25;
        const k = easeOut((u - d) / 0.62);
        const jx = (c.r - 0.5) * dieW * 0.05, jy = (c.r2 - 0.5) * dieW * 0.05;
        return [lerp(cx0 + jx, p[0], k), lerp(cy0 + jy, p[1], k), clamp(u / 0.08), 0];
      }
      if (c.cat === 'phys') {
        const d = ((p[0] - die[0]) / dieW) * 0.55;
        return [p[0], p[1], clamp((u - d) / 0.15), 0];
      }
      const d = 0.1 + c.r * 0.5;
      const k = clamp((u - d) / 0.18);
      if (k <= 0) return null;
      return [p[0], p[1], 1, 1 - k];
    }
    const q = c.pos[si - 1];
    if (q && (q[0] !== p[0] || q[1] !== p[1])) {
      const k = ease((u - c.r * 0.2) / 0.6);
      return [lerp(q[0], p[0], k), lerp(q[1], p[1], k), 1, 0];
    }
    return [p[0], p[1], 1, 0];
  }

  function draw(t) {
    const [si, u] = stageAt(t);
    const st = stages[si];
    const cam = camAt(si, u);
    const X = (x) => VP.x + VP.w / 2 + (x - cam.cx) * cam.s;
    const Y = (y) => VP.y + VP.h / 2 - (y - cam.cy) * cam.s;
    const sc = cam.s;

    ctx.fillStyle = COL.bg; ctx.fillRect(0, 0, W, H);
    ctx.save();
    ctx.beginPath(); ctx.rect(VP.x, VP.y, VP.w, VP.h); ctx.clip();

    // die + rows
    const fp = si === 0 ? ease(u / 0.5) : 1;
    ctx.fillStyle = COL.die; ctx.globalAlpha = fp;
    ctx.fillRect(X(die[0]), Y(die[3]), dieW * sc, dieH * sc);
    ctx.globalAlpha = 1;
    const nrows = si === 0 ? Math.floor(S.rows.length * clamp((u - 0.15) / 0.5)) : S.rows.length;
    ctx.strokeStyle = COL.row; ctx.lineWidth = 1; ctx.beginPath();
    for (let i = 0; i < nrows; i++) { const r = S.rows[i]; ctx.rect(X(r[0]), Y(r[1] + S.rowH), r[2] * sc, S.rowH * sc); }
    ctx.stroke();

    const routed = si > SI.drt ? 1 : si === SI.drt ? u : 0;
    const dimCells = si >= SI.drt ? lerp(1, 0.55, clamp(routed * 3)) : 1;

    // fill cells
    if (si >= SI.fill) {
      const k = si === SI.fill ? u : 1;
      ctx.fillStyle = COL.fill; ctx.strokeStyle = '#0b0f16'; ctx.lineWidth = 1; ctx.beginPath();
      for (const f of fills) {
        if ((f[0] - die[0]) / dieW > k * 1.15) continue;
        ctx.rect(X(f[0]) + 0.5, Y(f[1] + f[3]) + 0.5, f[2] * sc - 1, f[3] * sc - 1);
      }
      ctx.fill(); if (sc * S.rowH > 6) ctx.stroke();
    }

    // PDN
    if (si >= SI.pdn) {
      const k = si === SI.pdn ? clamp((u - 0.3) / 0.6) : 1;
      for (const p of S.pdn) {
        const horiz = p[2] === p[4];
        ctx.globalAlpha = (horiz ? 0.16 : 0.2) * (si >= SI.drt ? lerp(1, 0.8, routed) : 1);
        ctx.fillStyle = p[6] ? '#6ea8ff' : '#ff7b72';
        let x1 = Math.min(p[1], p[3]), x2 = Math.max(p[1], p[3]), y1 = Math.min(p[2], p[4]), y2 = Math.max(p[2], p[4]);
        if (horiz) { x2 = lerp(x1, x2, k); y1 -= p[5] / 2; y2 += p[5] / 2; } else { y2 = lerp(y1, y2, k); x1 -= p[5] / 2; x2 += p[5] / 2; }
        ctx.fillRect(X(x1), Y(y2), Math.max(1, (x2 - x1) * sc), Math.max(1, (y2 - y1) * sc));
      }
      ctx.globalAlpha = 1;
    }

    // cells
    const cur = new Array(cells.length);
    let nLogic = 0;
    const pops = [];
    for (let i = 0; i < cells.length; i++) {
      const c = cells[i];
      const p = cellPos(c, si, u);
      cur[i] = p;
      if (!p) continue;
      if (c.cat !== 'phys' && !(c.death && c.death <= si)) nLogic++;
      const x = X(p[0]), y = Y(p[1] + c.h), w = c.w * sc, h = c.h * sc;
      if (c.cat === 'phys') {
        ctx.globalAlpha = p[2];
        ctx.fillStyle = COL.phys; ctx.fillRect(x + 0.5, y + 0.5, Math.max(1.5, w - 1), h - 1);
      } else {
        ctx.globalAlpha = p[2] * dimCells;
        ctx.fillStyle = COL[c.cat] || COL.comb;
        ctx.fillRect(x + 0.5, y + 0.5, Math.max(1.5, w - 1), Math.max(1.5, h - 1));
        if (p[3] > 0) pops.push([x, y, w, h, p[3], c.cat]);
      }
    }
    ctx.globalAlpha = 1;

    // IO pins
    if (si > 0 || u > 0.45) {
      const k = si === 0 ? clamp((u - 0.45) / 0.4) : 1;
      ctx.fillStyle = COL.pin;
      const np = Math.floor(S.pins.length * k);
      for (let i = 0; i < np; i++) { const p = S.pins[i]; ctx.fillRect(X(p[0]) - 1, Y(p[1] + p[3]) - 1, Math.max(3, p[2] * sc + 2), Math.max(3, p[3] * sc + 2)); }
    }

    // ratsnest
    let ratA = 0;
    if (si >= SI.gpl && si < SI.drt) ratA = si === SI.gpl ? clamp(u / 0.15) : 1;
    else if (si === SI.drt) ratA = 1 - clamp(u / 0.35);
    if (si === SI.grt) ratA = lerp(1, 0.5, u);
    const ctsNow = hasCts && si === SI.cts;
    if (ratA > 0) {
      const big = Math.max(24, logic.length * 0.05);
      for (let pass = 0; pass < 2; pass++) {
        ctx.beginPath();
        for (let ni = 0; ni < S.nets.length; ni++) {
          const n = S.nets[ni];
          if ((n[1] === 1) !== (pass === 1)) continue;
          if (pass === 0 && n[0].length > big) continue;
          if (pass === 1 && ctsNow && hash(ni) * 0.5 > u) continue;
          let sx = 0, sy = 0, k = 0; const pts = [];
          for (const e of n[0]) {
            let px, py;
            if (e < 0) { const p = S.pins[-1 - e]; px = p[0] + p[2] / 2; py = p[1] + p[3] / 2; }
            else { const p = cur[e]; if (!p) continue; px = p[0] + cells[e].w / 2; py = p[1] + cells[e].h / 2; }
            pts.push(px, py); sx += px; sy += py; k++;
          }
          if (k < 2) continue;
          if (k === 2) { ctx.moveTo(X(pts[0]), Y(pts[1])); ctx.lineTo(X(pts[2]), Y(pts[3])); continue; }
          sx /= k; sy /= k;
          for (let j = 0; j < pts.length; j += 2) { ctx.moveTo(X(sx), Y(sy)); ctx.lineTo(X(pts[j]), Y(pts[j + 1])); }
        }
        if (pass === 0) { ctx.strokeStyle = '#ffffff'; ctx.globalAlpha = 0.22 * ratA; ctx.lineWidth = 1; }
        else { ctx.strokeStyle = COL.clk; ctx.globalAlpha = (ctsNow ? 0.75 : 0.4) * ratA; ctx.lineWidth = ctsNow ? 1.4 : 1; }
        ctx.stroke();
      }
      ctx.globalAlpha = 1;
    }

    // global-route guides
    if (si === SI.grt || (si === SI.drt && u < 0.3)) {
      const k = si === SI.grt ? clamp(u / 0.75) : 1;
      const a = si === SI.grt ? 1 : 1 - u / 0.3;
      const n = Math.floor(S.guides.length * k);
      ctx.globalAlpha = 0.11 * a;
      for (let i = 0; i < n; i++) { const g = S.guides[i]; ctx.fillStyle = LAYER_COL[g[0] % 6]; ctx.fillRect(X(g[1]), Y(g[4]), (g[3] - g[1]) * sc, (g[4] - g[2]) * sc); }
      ctx.globalAlpha = 1;
    }

    // routed wires, layer by layer
    let curLayer = null;
    if (routed > 0) {
      ctx.lineCap = 'square';
      for (const l of usedLayers) {
        const [ta, tb] = layerT[l];
        const k = clamp((routed / 0.92 - ta) / (tb - ta));
        if (k <= 0) continue;
        if (k < 1) curLayer = l;
        const [w0, wn] = layerRange[l];
        const n = Math.floor(wn * k);
        ctx.strokeStyle = LAYER_COL[l % 6]; ctx.lineWidth = Math.max(1.3, S.layerW[l] * sc);
        ctx.globalAlpha = 0.95;
        ctx.beginPath();
        for (let i = w0; i < w0 + n; i++) { const w = wires[i]; ctx.moveTo(X(w[1]), Y(w[2])); ctx.lineTo(X(w[3]), Y(w[4])); }
        ctx.stroke();
        // growing head
        if (k < 1) {
          ctx.strokeStyle = '#ffffff'; ctx.beginPath();
          for (let i = Math.max(w0, w0 + n - Math.max(2, Math.ceil(wn * 0.02))); i < w0 + n; i++) { const w = wires[i]; ctx.moveTo(X(w[1]), Y(w[2])); ctx.lineTo(X(w[3]), Y(w[4])); }
          ctx.stroke();
        }
        const [v0, vn] = viaRange[l];
        const nv = Math.floor(vn * k);
        const vs = Math.max(2, S.layerW[l] * sc * 1.5);
        if (vs > 2.5 || true) {
          ctx.fillStyle = '#f5f7fa'; ctx.globalAlpha = 0.8;
          for (let i = v0; i < v0 + nv; i++) { const v = vias[i]; ctx.fillRect(X(v[1]) - vs / 2, Y(v[2]) - vs / 2, vs, vs); }
        }
      }
      ctx.globalAlpha = 1; ctx.lineCap = 'butt';
    }

    // pop highlights for newly inserted cells
    for (const p of pops) {
      ctx.globalAlpha = p[4]; ctx.strokeStyle = p[5] === 'clk' ? '#ffd0f5' : '#ffffff'; ctx.lineWidth = 2;
      const g = 6 * p[4];
      ctx.strokeRect(p[0] - g, p[1] - g, p[2] + 2 * g, p[3] + 2 * g);
    }
    ctx.globalAlpha = 1;

    // die outline
    ctx.strokeStyle = '#4a5a78'; ctx.lineWidth = 1.5; ctx.globalAlpha = fp;
    ctx.strokeRect(X(die[0]), Y(die[3]), dieW * sc, dieH * sc);
    ctx.globalAlpha = 1;
    ctx.restore();

    // ---- side panel ----
    const px = PANEL.x;
    ctx.textBaseline = 'alphabetic';
    ctx.fillStyle = COL.accent; ctx.font = '600 13px system-ui, sans-serif';
    ctx.fillText('TINY TAPEOUT  ·  HARDENING', px, 34);
    ctx.fillStyle = COL.text; ctx.font = '700 22px system-ui, sans-serif';
    let name = S.design; while (ctx.measureText(name).width > PANEL.w && name.length > 4) name = name.slice(0, -2);
    ctx.fillText(name === S.design ? name : name + '…', px, 64);
    ctx.fillStyle = COL.dim; ctx.font = '14px system-ui, sans-serif';
    ctx.fillText((S.pdk || '') + '  ·  ' + fmt(dieW / dbu) + ' × ' + fmt(dieH / dbu) + ' µm', px, 86);

    let y = 122;
    stages.forEach((s, i) => {
      const done = i < si, now = i === si;
      ctx.fillStyle = now ? COL.accent : done ? '#3fb950' : '#3a4558';
      ctx.beginPath(); ctx.arc(px + 7, y - 6, now ? 6 : 4.5, 0, 7); ctx.fill();
      ctx.fillStyle = now ? COL.text : done ? '#9fb0c7' : '#56637a';
      ctx.font = (now ? '700 ' : '500 ') + '17px system-ui, sans-serif';
      ctx.fillText(s.label, px + 24, y);
      if (s.rt && (done || now)) {
        ctx.font = '13px ui-monospace, monospace'; ctx.fillStyle = now ? COL.accent : '#6b7a92';
        const tx = fmt(s.rt, 1) + ' s'; ctx.fillText(tx, px + PANEL.w - ctx.measureText(tx).width, y);
      }
      y += 31;
    });

    // stats
    y = Math.max(y + 12, 418);
    const stat = (k, v, col) => {
      ctx.fillStyle = COL.dim; ctx.font = '14px system-ui, sans-serif'; ctx.fillText(k, px, y);
      ctx.fillStyle = col || COL.text; ctx.font = '600 19px ui-monospace, monospace';
      ctx.fillText(v, px + PANEL.w - ctx.measureText(v).width, y + 1); y += 30;
    };
    stat('Cells placed', fmt(nLogic));
    if (si <= SI.gpl || !S.drtLog.length || si < SI.grt) {
      let g = null;
      if (si === SI.gpl && S.gplLog.length) g = S.gplLog[Math.min(S.gplLog.length - 1, Math.floor(clamp(u / 0.85) * S.gplLog.length))];
      else if (si > SI.gpl && S.gplLog.length) g = S.gplLog[S.gplLog.length - 1];
      stat('Est. wirelength', g ? fmt(g[2]) + ' µm' : '–');
      stat('Overlap', g ? fmt(g[1] * 100) + ' %' : '–', g && g[1] < 0.12 ? '#3fb950' : null);
    } else {
      let d = null;
      if (si === SI.drt) d = S.drtLog[Math.min(S.drtLog.length - 1, Math.floor(clamp(u / 0.9) * S.drtLog.length))];
      else if (si > SI.drt) d = S.drtLog[S.drtLog.length - 1];
      stat('Wire routed', si >= SI.drt ? fmt(wireUm * (si > SI.drt ? 1 : clamp(routed / 0.92))) + ' µm' : '–');
      stat('DRC violations', d ? String(d[1]) : '–', d ? (d[1] === 0 ? '#3fb950' : '#ff7b72') : null);
    }
    // layer legend while routing
    if (si >= SI.grt) {
      let lx = px; ctx.font = '13px system-ui, sans-serif';
      for (const l of usedLayers) {
        const on = si > SI.drt || (si === SI.drt && (curLayer === null ? true : l <= curLayer));
        ctx.globalAlpha = on ? 1 : 0.35;
        ctx.fillStyle = LAYER_COL[l % 6]; ctx.fillRect(lx, y - 4, 12, 12);
        ctx.fillStyle = COL.text; ctx.fillText(S.layers[l], lx + 16, y + 7);
        lx += 22 + ctx.measureText(S.layers[l]).width + 8;
      }
      ctx.globalAlpha = 1;
    }

    // ---- caption + progress ----
    ctx.fillStyle = COL.text; ctx.font = '700 30px system-ui, sans-serif';
    const cap = (si + 1) + '/' + stages.length + '  ' + st.label;
    ctx.fillText(cap, VP.x, 590);
    if (si === SI.drt && curLayer !== null) {
      const cw = ctx.measureText(cap).width;
      ctx.fillStyle = LAYER_COL[curLayer % 6]; ctx.font = '600 24px system-ui, sans-serif';
      ctx.fillText(S.layers[curLayer], VP.x + cw + 18, 590);
    }
    const bx = VP.x, bw = W - 2 * VP.x, by = 606, bh = 8;
    stages.forEach((s, i) => {
      const x1 = bx + (s.t0 / DURATION) * bw, w1 = (s.dur / DURATION) * bw - 3;
      ctx.fillStyle = '#222b3a'; ctx.fillRect(x1, by, w1, bh);
      const k = i < si ? 1 : i === si ? u : 0;
      if (k > 0) { ctx.fillStyle = i === si ? COL.accent : '#3fb950'; ctx.fillRect(x1, by, w1 * k, bh); }
    });
  }

  // ---- playback -----------------------------------------------------------
  let playing = false, tcur = 0, last = 0, speed = 1, onTick = null;
  function frame(now) {
    if (!playing) return;
    tcur += ((now - last) / 1000) * speed; last = now;
    if (tcur >= DURATION + 1.2) tcur = 0;
    draw(Math.min(tcur, DURATION));
    if (onTick) onTick(tcur);
    requestAnimationFrame(frame);
  }
  window.flowAnim = {
    duration: DURATION, stages,
    renderAt(t) { tcur = t; draw(t); },
    play() { if (playing) return; playing = true; last = performance.now(); requestAnimationFrame(frame); },
    pause() { playing = false; },
    get playing() { return playing; },
    set speed(v) { speed = v; },
    set onTick(f) { onTick = f; },
    resize() { resize(); draw(Math.min(tcur, DURATION)); },
  };
  draw(0);
})();
