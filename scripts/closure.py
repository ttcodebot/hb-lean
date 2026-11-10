#!/usr/bin/env python3
"""closure.py REFS.json CFG.json -> prints store names to keep (PATH roots closure minus build-only paths)."""
import json, re, sys
d = json.load(open(sys.argv[1])); refs, size = d["refs"], d["size"]
env = dict(e.split("=", 1) for e in json.load(open(sys.argv[2]))["config"]["Env"])
roots = [p.split("/")[3] for p in env["PATH"].split(":") if p.startswith("/nix/store")]
DROP = re.compile(r"^.{33}(clang-|llvm-|gcc-14[.0-9]*$|gcc-wrapper|cmake-|ghdl-bin|binutils-|iverilog|.*-source$|git-|channel|compiler-rt|nix-|neovim|zsh|.*-dev$|.*-doc$|.*-man$)")
seen, st = set(), [r for r in roots if not DROP.match(r)]
while st:
    n = st.pop()
    if n in seen or DROP.match(n): continue
    seen.add(n); st += refs[n]
if len(sys.argv) > 3:
    tot = sum(size[n] for n in seen); print(len(seen), "paths", round(tot / 1e6), "MB", file=sys.stderr)
    for n in sorted(seen, key=lambda n: -size[n])[:40]: print(f"{size[n]/1e6:6.0f} {n[33:]}", file=sys.stderr)
print("\n".join(sorted(seen)))
