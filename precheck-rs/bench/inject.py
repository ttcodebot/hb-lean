"""Create a copy of the reference GDS with deliberate DRC violations.

usage: inject.py clean.gds injected.gds
Shapes go into the top cell. Most sit in a strip above the die (isolated from
the design, one violation type per 10 um slot); a few edits touch real routing.
"""
import sys
import klayout.db as k

ly = k.Layout()
ly.read(sys.argv[1])
top = ly.top_cell()
L = lambda l, d=0: ly.layer(l, d)
COMP, POLY, CONT, M1, V1, M2, V2, M3, V3, M4 = (L(22), L(30), L(33), L(34), L(35), L(36), L(38), L(42), L(40), L(46))
Y0 = top.bbox().top + 20000
slot = [0]


def at():
    x = 5000 + slot[0] * 10000
    slot[0] += 1
    return x, Y0


def box(layer, x, y, w, h):
    top.shapes(layer).insert(k.Box(x, y, x + w, y + h))


# --- metal width / space / area -------------------------------------------------
x, y = at(); box(M2, x, y, 200, 3000)                       # M2.1 width 0.20
x, y = at(); box(M2, x, y, 1000, 1000); box(M2, x + 1200, y, 1000, 1000)   # M2.2a space 0.20
x, y = at(); box(M2, x, y, 300, 300)                        # M2.3 area 0.09
x, y = at()                                                 # M3 notch 0.20 (same polygon)
top.shapes(M3).insert(k.Polygon([k.Point(x, y), k.Point(x, y + 2000), k.Point(x + 1000, y + 2000),
                                 k.Point(x + 1000, y + 1000), k.Point(x + 1200, y + 1000),
                                 k.Point(x + 1200, y + 2000), k.Point(x + 2200, y + 2000),
                                 k.Point(x + 2200, y)]))
x, y = at(); box(M1, x, y, 200, 2000)                       # M1.1 width 0.20
x, y = at(); box(M1, x, y, 1000, 1000); box(M1, x + 1200, y, 1000, 1000)   # M1.2a space 0.20
x, y = at(); box(M1, x, y, 1000, 1000); box(M1, x + 1150, y + 1150, 1000, 1000)  # M1.2a corner 0.212
x, y = at(); box(M3, x, y, 1000, 1000); box(M3, x + 1250, y + 300, 1000, 1000)  # M3.2a space 0.25, partial overlap
x, y = at(); box(M4, x, y, 250, 250)                        # M4.1 + M4.3
x, y = at(); box(M2, x, y, 12000, 12000); box(M2, x + 12290, y + 2000, 1000, 1000)  # M2.2b wide space 0.29
slot[0] += 1
# --- via rules ------------------------------------------------------------------
x, y = at(); box(M1, x, y, 1000, 1000); box(M2, x, y, 1000, 1000); box(V1, x + 300, y + 300, 300, 260)  # V1.1
x, y = at(); box(M1, x, y, 2000, 1000); box(M2, x, y, 2000, 1000)
box(V1, x + 300, y + 300, 260, 260); box(V1, x + 760, y + 300, 260, 260)                 # V1.2a space 0.20
x, y = at(); box(M2, x, y, 1000, 1000); box(V1, x + 300, y + 300, 260, 260)             # V1.3a no M1
x, y = at(); box(M1, x, y, 1000, 1000); box(V1, x + 300, y + 300, 260, 260)             # V1.4a no M2
x, y = at(); box(M2, x, y, 1000, 1000); box(M3, x, y, 1000, 1000); box(V2, x, y + 300, 260, 260)  # V2.3b flush
x, y = at(); box(M1, x, y, 300, 2000); box(M2, x - 300, y - 300, 1000, 1000)
box(V1, x + 20, y + 30, 260, 260)                                                       # V1.3c EOL 0.03
x, y = at(); box(M2, x, y, 300, 2000); box(M1, x - 300, y - 300, 1000, 1000)
box(V1, x + 20, y + 30, 260, 260)                                                       # V1.4b EOL 0.03
x, y = at(); box(M1, x, y, 1000, 1000); box(M2, x - 300, y - 300, 1600, 1600)
box(V1, x + 20, y + 30, 260, 260)                                                       # V1.3d 0.02 / 0.03
x, y = at(); box(M3, x, y, 1000, 1000); box(M4, x, y, 1000, 1000); box(V3, x + 300, y + 300, 260, 260)
box(V3, x + 300 + 500, y + 300 + 100, 260, 260)                                         # V3.2a space 0.24 offset
# --- contact / poly / comp ---------------------------------------------------------
x, y = at(); box(COMP, x, y, 1000, 1000); box(M1, x, y, 1000, 1000); box(CONT, x + 300, y + 300, 240, 220)  # CO.1
x, y = at(); box(COMP, x, y, 2000, 1000); box(M1, x, y, 2000, 1000)
box(CONT, x + 300, y + 300, 220, 220); box(CONT, x + 720, y + 300, 220, 220)             # CO.2a 0.20
x, y = at(); box(POLY, x, y, 1000, 1000); box(M1, x, y, 1000, 1000); box(CONT, x + 30, y + 300, 220, 220)   # CO.3 0.03
x, y = at(); box(COMP, x, y, 1000, 1000); box(M1, x, y, 1000, 1000); box(CONT, x + 30, y + 300, 220, 220)   # CO.4 0.03
x, y = at(); box(COMP, x, y, 1000, 1000); box(CONT, x + 300, y + 300, 220, 220)         # CO.6 no M1
x, y = at(); box(M1, x, y, 1000, 1000); box(CONT, x + 300, y + 300, 220, 220)           # CO.11 field
x, y = at(); box(COMP, x, y, 1000, 1000); box(M1, x + 298, y, 500, 1000); box(CONT, x + 300, y + 300, 220, 220)  # CO.6 0.002
x, y = at(); box(POLY, x, y, 100, 2000)                                                 # PL.1 width
x, y = at(); box(POLY, x, y, 500, 2000); box(POLY, x + 700, y, 500, 2000)               # PL.3a space 0.2
x, y = at(); box(COMP, x, y, 150, 2000)                                                 # DF.1a width
x, y = at(); box(COMP, x, y, 500, 2000); box(COMP, x + 700, y, 500, 2000)               # DF.3a space 0.2

# --- edits to real routing -----------------------------------------------------------
def nth(layer, n):
    shapes = sorted(top.shapes(layer).each(), key=lambda s: (s.bbox().left, s.bbox().bottom, s.bbox().right, s.bbox().top))
    return shapes[n]

for n in (400, 250, 100):                 # delete three M2 wires -> via1/via2 lose metal
    nth(M2, n).delete()
s = nth(M3, 200); s.transform(k.Trans(140, 140))    # nudge one M3 wire
s = nth(M2, 50); b = s.bbox()
box(M2, b.right + 200, b.bottom, 400, 400)          # blob 0.20 from a real M2 wire
ly.write(sys.argv[2])
print("slots", slot[0])
