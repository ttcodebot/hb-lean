#!/usr/bin/env python3
"""prune.py USED.txt CLOSURE_KEEP.txt PDK_DIR -> writes keep.list (nix store paths the traced runs touched)
and deletes PDK views they did not touch. In `lib` views (timing corners, chosen by config) only touched files stay."""
import os, re, shutil, sys

used = [l.strip() for l in open(sys.argv[1]) if l.strip()]
closure = set(open(sys.argv[2]).read().split())
store = sorted({p.split("/")[3] for p in used if p.startswith("/nix/store/")} & closure)
open("keep.list", "w").write("".join(f"nix/store/{n}\n" for n in store))
print(f"nix: {len(store)} of {len(closure)} closure paths kept")

pdk = sys.argv[3]
views, files = set(), set()
for p in used:
    m = re.search(r"/(libs\.(?:ref|tech)/[^/]+(?:/[^/]+)?)(/.*)?$", p)
    if p.startswith("/pdk/") and m:
        parts = m.group(1).split("/")
        views.add("/".join(parts[:3]) if parts[0] == "libs.ref" else "/".join(parts[:2]))
        files.add(m.group(1) + (m.group(2) or ""))
for top in ("libs.ref", "libs.tech"):
    for a in sorted(os.listdir(os.path.join(pdk, top))):
        subs = [f"{top}/{a}"] if top == "libs.tech" else [f"{top}/{a}/{b}" for b in sorted(os.listdir(os.path.join(pdk, top, a)))]
        for v in subs:
            full = os.path.join(pdk, v)
            if v not in views:
                shutil.rmtree(full) if os.path.isdir(full) and not os.path.islink(full) else os.remove(full)
            elif v.endswith("/lib") and os.path.isdir(full):
                for f in os.listdir(full):
                    if f"{v}/{f}" not in files:
                        os.remove(os.path.join(full, f))
print("pdk views kept:", ", ".join(sorted(views)))
