#!/usr/bin/env python3
"""PSNR of selected frames of animated files against lossless PNG frames (tmp/fNNN.png)."""
import sys
from PIL import Image
import numpy as np
idx=[0,30,75,100,139]
for f in sys.argv[2:]:
    im=Image.open(f); ps=[]
    for i in range(im.n_frames):
        im.seek(i)
        if i in idx:
            src=Image.open(f'{sys.argv[1]}/f{i:03d}.png').convert('RGB')
            if im.size!=src.size: src=src.resize(im.size, Image.BOX)
            a=np.asarray(im.convert('RGB'),dtype=float); b=np.asarray(src,dtype=float)
            ps.append(round(10*np.log10(255**2/((a-b)**2).mean()),1))
    import os
    print(f"{f:50} {os.path.getsize(f):>9} bytes  PSNR {ps}")
