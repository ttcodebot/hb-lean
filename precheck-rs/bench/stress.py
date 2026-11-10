"""Stress copy of the reference GDS: seeded random damage to real geometry, so violations
interact with fragmented wires and standard-cell shapes.  usage: stress.py clean.gds stress.gds [seed]"""
import random, sys
import klayout.db as k

rnd = random.Random(int(sys.argv[3]) if len(sys.argv) > 3 else 1)
ly = k.Layout(); ly.read(sys.argv[1]); top = ly.top_cell()
L = lambda l, d=0: ly.layer(l, d)
bb = top.bbox()
g5 = lambda v: int(round(v / 5.0)) * 5
# 1. nudge / delete a share of top-level routing shapes and vias
for layer, pmove, pdel in ((34, .05, .02), (36, .08, .03), (42, .08, .03), (46, .1, .0), (35, .03, .02), (38, .03, .02), (40, .03, .02)):
    for s in list(top.shapes(L(layer)).each()):
        r = rnd.random()
        if r < pdel:
            s.delete()
        elif r < pdel + pmove:
            s.transform(k.Trans(g5(rnd.uniform(-150, 150)), g5(rnd.uniform(-150, 150))))
# 2. sprinkle random small shapes over the standard cells
def sprinkle(layer, n, smin, smax, square=False):
    for _ in range(n):
        w = g5(rnd.uniform(smin, smax)); h = w if square else g5(rnd.uniform(smin, smax))
        x = g5(rnd.uniform(bb.left + 2000, bb.right - 2000)); y = g5(rnd.uniform(bb.bottom + 2000, bb.top - 2000))
        top.shapes(L(layer)).insert(k.Box(x, y, x + w, y + h))
sprinkle(34, 300, 150, 600)          # metal1 blobs
sprinkle(36, 150, 150, 800)          # metal2 blobs
sprinkle(42, 100, 150, 800)
sprinkle(33, 300, 220, 220, True)    # contacts anywhere
sprinkle(33, 30, 200, 260)           # odd-sized contacts
sprinkle(35, 150, 260, 260, True)    # via1 anywhere
sprinkle(38, 100, 260, 260, True)
sprinkle(35, 20, 240, 300)
# 3. a 5x5 via2 array at 0.30 pitch spacing (V2.2b) and a contact array (CO.2b) above the die
x0, y0 = 20000, bb.top + 10000
top.shapes(L(36)).insert(k.Box(x0 - 500, y0 - 500, x0 + 4000, y0 + 4000)); top.shapes(L(42)).insert(k.Box(x0 - 500, y0 - 500, x0 + 4000, y0 + 4000))
for i in range(5):
    for j in range(5):
        top.shapes(L(38)).insert(k.Box(x0 + i * 560, y0 + j * 560, x0 + i * 560 + 260, y0 + j * 560 + 260))
x0 = 40000
top.shapes(L(22)).insert(k.Box(x0 - 500, y0 - 500, x0 + 4000, y0 + 4000)); top.shapes(L(34)).insert(k.Box(x0 - 500, y0 - 500, x0 + 4000, y0 + 4000))
for i in range(6):
    for j in range(6):
        top.shapes(L(33)).insert(k.Box(x0 + i * 470, y0 + j * 470, x0 + i * 470 + 220, y0 + j * 470 + 220))
# 4. 45-degree and off-grid shapes
x0 = 60000
top.shapes(L(36)).insert(k.Polygon([k.Point(x0, y0), k.Point(x0 + 1000, y0 + 1000), k.Point(x0 + 1200, y0 + 800), k.Point(x0 + 200, y0 - 200)]))
top.shapes(L(42)).insert(k.Polygon([k.Point(x0 + 3000, y0), k.Point(x0 + 3000, y0 + 1000), k.Point(x0 + 4003, y0 + 1000), k.Point(x0 + 4500, y0 + 300), k.Point(x0 + 4000, y0)]))
ly.write(sys.argv[2])
