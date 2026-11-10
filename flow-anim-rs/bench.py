#!/usr/bin/env python3
"""Benchmark matrix: min wall of 3 runs per configuration. Peak RSS is the larger of the renderer and its ffmpeg child."""
import subprocess, os, time, json, sys
BIN = '/tmp/flow-anim-rs/target/release/flowanim'
reps = int(sys.argv[1]) if len(sys.argv) > 1 else 3
print('scene\tthreads\tmode\twall_s\tpeak_rss_mb\tdraw_wall_ms\tcpu_draw_ms_per_frame\tencode_wall_ms\tload1', flush=True)
for scene in ['gf', 'sky']:
    for j in [1, 4]:
        for mode in ['draw', 'png', 'webp', 'webp-stock', 'gif', 'mp4', 'all']:
            flags = {'draw': [], 'webp-stock': ['--webp', '--webp-stock']}.get(mode, ['--' + mode])
            best = None
            for _ in range(reps):
                t0 = time.time()
                p = subprocess.Popen([BIN, f'/tmp/flow-anim/out-{scene}/scene.json', '-o', f'/tmp/flow-anim-rs/bench-out/{scene}-{j}-{mode}', '-j', str(j)] + flags, stdout=subprocess.PIPE)
                out = p.stdout.read()
                _, status, ru = os.wait4(p.pid, 0)
                wall = time.time() - t0
                assert status == 0
                d = json.loads(out)
                row = (wall, ru.ru_maxrss / 1024, d['draw_wall_ms'], d['draw_ms_per_frame'], d['encode_wall_ms'])
                if best is None or row[0] < best[0]: best = row
            print(f"{scene}\t{j}\t{mode}\t{best[0]:.2f}\t{best[1]:.0f}\t{best[2]:.0f}\t{best[3]:.2f}\t{best[4]:.0f}\t{os.getloadavg()[0]:.1f}", flush=True)
