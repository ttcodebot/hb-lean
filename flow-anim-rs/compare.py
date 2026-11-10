#!/usr/bin/env python3
"""Per-stage difference between reference (browser) and Rust key frames, plus side-by-side images.
usage: compare.py REF_DIR RUST_DIR OUT_DIR"""
import sys, os
import numpy as np
from PIL import Image
ref, rs, out = sys.argv[1:4]
os.makedirs(out, exist_ok=True)
print(f"{'frame':16} {'MAE':>6} {'RMSE':>6} {'PSNR':>6} {'>32/255':>8} {'MAE die':>8} {'MAE panel':>9} {'MAE blur4':>9}")
for f in sorted(os.listdir(ref)):
    if not f.endswith('.png') or not os.path.exists(os.path.join(rs, f)): continue
    A = Image.open(os.path.join(ref, f)).convert('RGB'); B = Image.open(os.path.join(rs, f)).convert('RGB')
    a = np.asarray(A, dtype=np.float32); b = np.asarray(B, dtype=np.float32)
    d = np.abs(a - b)
    mse = float(((a - b) ** 2).mean())
    psnr = 10 * np.log10(255 ** 2 / mse) if mse else 99
    big = float((d.max(axis=2) > 32).mean() * 100)
    # structural difference without sub-pixel anti-aliasing noise: compare 4x box-downscaled images
    sa = np.asarray(A.resize((300, 157), Image.BOX), dtype=np.float32); sb = np.asarray(B.resize((300, 157), Image.BOX), dtype=np.float32)
    print(f"{f:16} {d.mean():6.2f} {mse ** 0.5:6.2f} {psnr:6.1f} {big:7.2f}% {d[16:544, 16:884].mean():8.2f} {d[:, 884:].mean():9.2f} {np.abs(sa - sb).mean():9.2f}")
    W, H = A.size
    side = Image.new('RGB', (W * 2, H * 2))
    side.paste(A, (0, 0)); side.paste(B, (W, 0))
    amp = Image.fromarray(np.clip(d * 4, 0, 255).astype(np.uint8))
    side.paste(amp, (0, H))
    # zoomed crops (panel text) ref | rust
    crop = (896, 16, 1196, 136)
    side.paste(A.crop(crop).resize((600, 240), Image.NEAREST), (W, H)); side.paste(B.crop(crop).resize((600, 240), Image.NEAREST), (W + 600, H))
    crop = (300, 200, 600, 320)
    side.paste(A.crop(crop).resize((600, 240), Image.NEAREST), (W, H + 250)); side.paste(B.crop(crop).resize((600, 240), Image.NEAREST), (W + 600, H + 250))
    side.save(os.path.join(out, f))
