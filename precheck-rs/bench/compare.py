"""Compare two marker JSON files (precheck-rs vs reference). usage: compare.py a.json b.json [--counts]"""
import json, sys
a = json.load(open(sys.argv[1]))["rules"]; b = json.load(open(sys.argv[2]))["rules"]
names = sorted(set(a) & set(b)); only = sorted(set(a) ^ set(b))
same = cnt = diff = 0
for n in names:
    if a[n]["markers"] == b[n]["markers"]: same += 1
    elif a[n]["count"] == b[n]["count"]:
        cnt += 1; print(f"GEOM-DIFF {n}: count {a[n]['count']} equal, markers differ")
        if "-v" in sys.argv: print("   a-b", sorted(set(a[n]["markers"]) - set(b[n]["markers"]))[:4], "\n   b-a", sorted(set(b[n]["markers"]) - set(a[n]["markers"]))[:4])
    else:
        diff += 1; print(f"COUNT-DIFF {n}: {a[n]['count']} vs {b[n]['count']}")
        if "-v" in sys.argv: print("   a-b", sorted(set(a[n]["markers"]) - set(b[n]["markers"]))[:4], "\n   b-a", sorted(set(b[n]["markers"]) - set(a[n]["markers"]))[:4])
nz = sum(1 for n in names if b[n]["count"])
print(f"rules compared {len(names)} (with violations in reference: {nz}); identical markers {same}; same count/different geometry {cnt}; different count {diff}; not in both {len(only)} {only[:8]}")
