"""Convert a KLayout report database (.lyrdb / XML) to the marker JSON used by compare.py.
usage: lyrdb2json.py report.xml out.json"""
import json, re, sys
import xml.etree.ElementTree as ET
root = ET.parse(sys.argv[1]).getroot()
rules = {}
def walk(cats, prefix=""):
    for c in cats.findall("category"):
        name = prefix + c.find("name").text
        sub = c.find("categories")
        if sub is not None and len(sub):
            walk(sub, name + ".")
        else:
            rules[name] = []
walk(root.find("categories"))
dbu = lambda s: int(round(float(s) * 1000))
def pts(s):
    return [tuple(dbu(v) for v in p.split(",")) for p in s.split(";")]
def ring(p, cw):
    a = sum(p[i][0] * p[(i + 1) % len(p)][1] - p[(i + 1) % len(p)][0] * p[i][1] for i in range(len(p)))
    if (a > 0) == cw: p = p[::-1]
    i = p.index(min(p)); p = p[i:] + p[:i]
    return ";".join("%d,%d" % q for q in p)
for it in root.find("items").findall("item"):
    cat = it.find("category").text.strip("'").replace("'.'", ".")
    for v in it.find("values").findall("value"):
        t = v.text
        if t.startswith("edge-pair:"):
            e = re.findall(r"\(([^)]*)\)", t)
            a, b = sorted(tuple(sorted(pts(x))) for x in e)
            m = "ep:%d,%d,%d,%d/%d,%d,%d,%d" % (*a[0], *a[1], *b[0], *b[1])
        elif t.startswith("edge:"):
            a = sorted(pts(re.findall(r"\(([^)]*)\)", t)[0]))
            m = "edge:%d,%d,%d,%d" % (*a[0], *a[1])
        elif t.startswith("polygon:"):
            parts = re.findall(r"\(([^)]*)\)", t)[0].split("/")
            m = "poly:" + ring(pts(parts[0]), True)
            for h in sorted(ring(pts(x), False) for x in parts[1:]): m += "/" + h
        else:
            m = t
        rules.setdefault(cat, []).append(m)
json.dump({"rules": {k: {"count": len(v), "markers": sorted(v)} for k, v in rules.items()}}, open(sys.argv[2], "w"))
print(sys.argv[1], "rules", len(rules), "items", sum(len(v) for v in rules.values()))
