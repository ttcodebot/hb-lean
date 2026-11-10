#!/usr/bin/env python3
"""compare.py RUN_DIR REPLAY_LOG [REPLAY_WALL_S]: LibreLane per-step wall time vs the single-process replay."""
import glob, os, re, sys

run, log = sys.argv[1], sys.argv[2]
def rt(d):
    try:
        h, m, s = open(os.path.join(d, "runtime.txt")).read().strip().split(":")
        return int(h) * 3600 + int(m) * 60 + float(s)
    except OSError:
        return 0.0
steps = sorted(d for d in glob.glob(os.path.join(run, "[0-9]*-*")) if os.path.isdir(d))
total = sum(rt(d) for d in steps)
replay = dict(re.findall(r"REPLAY_STEP (\S+) (\d+) ms", open(log).read()))
print("| step | LibreLane s | in-process s |")
print("| --- | --: | --: |")
ll = rp = 0.0
for d in steps:
    n = os.path.basename(d)
    if n in replay:
        ll += rt(d); rp += int(replay[n]) / 1000
        print(f"| {n} | {rt(d):.1f} | {int(replay[n]) / 1000:.1f} |")
print(f"| **replayed steps** | **{ll:.1f}** | **{rp:.1f}** |")
if len(sys.argv) > 3:
    print(f"| replay process wall (incl. start, read, write) | | {float(sys.argv[3]):.1f} |")
print(f"\nAll LibreLane steps: {total:.1f} s in {len(steps)} steps; other steps: {total - ll:.1f} s\n")
groups = {}
for d in steps:
    n = os.path.basename(d)
    if n in replay: continue
    tool = n.split("-")[1]
    groups[tool] = groups.get(tool, 0) + rt(d)
print("Not replayed, by tool: " + ", ".join(f"{k} {v:.1f}" for k, v in sorted(groups.items(), key=lambda kv: -kv[1])))
