//! GIF at 800x420: 3:2 area downscale, one global palette (median cut + k-means on a
//! 15-bit histogram of the key frames), frame-difference rectangles with transparent
//! unchanged pixels, LZW per frame (parallelisable) through the `gif` crate.
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

pub const GW: usize = 800;
pub const GH: usize = 420;

/// 1200x630 RGBA -> 800x420 RGB, exact area average (every 3x3 block becomes 2x2).
pub fn downscale(rgba: &[u8]) -> Vec<u8> {
    const SW: usize = 1200;
    let mut out = vec![0u8; GW * GH * 3];
    // weights of the 3 source samples for the 2 destination samples, in thirds
    const WT: [[u32; 3]; 2] = [[2, 1, 0], [0, 1, 2]];
    for by in 0..GH / 2 {
        for bx in 0..GW / 2 {
            let mut src = [[[0u32; 3]; 3]; 3];
            for (sy, row) in src.iter_mut().enumerate() {
                let o = ((by * 3 + sy) * SW + bx * 3) * 4;
                for (sx, px) in row.iter_mut().enumerate() {
                    let p = &rgba[o + sx * 4..o + sx * 4 + 3];
                    *px = [p[0] as u32, p[1] as u32, p[2] as u32];
                }
            }
            for dy in 0..2 {
                for dx in 0..2 {
                    let mut acc = [0u32; 3];
                    for sy in 0..3 {
                        for sx in 0..3 {
                            let wgt = WT[dy][sy] * WT[dx][sx];
                            if wgt != 0 {
                                for c in 0..3 {
                                    acc[c] += wgt * src[sy][sx][c];
                                }
                            }
                        }
                    }
                    let o = ((by * 2 + dy) * GW + bx * 2 + dx) * 3;
                    for c in 0..3 {
                        out[o + c] = ((acc[c] + 4) / 9) as u8;
                    }
                }
            }
        }
    }
    out
}

pub struct Palette {
    pub colors: Vec<[u8; 3]>,
    lut: Vec<u8>, // rgb555 -> index
}

fn bin(r: u8, g: u8, b: u8) -> usize {
    ((r as usize >> 3) << 10) | ((g as usize >> 3) << 5) | (b as usize >> 3)
}

impl Palette {
    pub fn build(samples: &[Vec<u8>], n: usize) -> Palette {
        // histogram bins keep their true mean colour
        let mut sum = vec![[0u64; 4]; 32768];
        for s in samples {
            for p in s.chunks_exact(3) {
                let e = &mut sum[bin(p[0], p[1], p[2])];
                e[0] += p[0] as u64;
                e[1] += p[1] as u64;
                e[2] += p[2] as u64;
                e[3] += 1;
            }
        }
        // (mean colour, weight); sqrt weighting keeps thin bright features represented
        let pts: Vec<([f32; 3], f32)> = sum
            .iter()
            .filter(|e| e[3] > 0)
            .map(|e| ([e[0] as f32 / e[3] as f32, e[1] as f32 / e[3] as f32, e[2] as f32 / e[3] as f32], (e[3] as f32).sqrt()))
            .collect();
        // median cut
        let mut boxes: Vec<Vec<usize>> = vec![(0..pts.len()).collect()];
        while boxes.len() < n {
            let score = |b: &Vec<usize>| -> (f32, usize) {
                let mut best = (0.0, 0);
                for c in 0..3 {
                    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
                    for &i in b {
                        lo = lo.min(pts[i].0[c]);
                        hi = hi.max(pts[i].0[c]);
                    }
                    if hi - lo > best.0 {
                        best = (hi - lo, c);
                    }
                }
                best
            };
            let Some((bi, (_, axis))) = boxes
                .iter()
                .enumerate()
                .filter(|(_, b)| b.len() > 1)
                .map(|(i, b)| {
                    let (range, axis) = score(b);
                    let wsum: f32 = b.iter().map(|&i| pts[i].1).sum();
                    (i, (range * wsum.sqrt(), axis))
                })
                .max_by(|a, b| a.1 .0.total_cmp(&b.1 .0))
            else {
                break;
            };
            let mut b = boxes.swap_remove(bi);
            b.sort_by(|&x, &y| pts[x].0[axis].total_cmp(&pts[y].0[axis]));
            let total: f32 = b.iter().map(|&i| pts[i].1).sum();
            let (mut acc, mut cut) = (0.0, 1);
            for (k, &i) in b.iter().enumerate() {
                acc += pts[i].1;
                if acc >= total / 2.0 {
                    cut = (k + 1).clamp(1, b.len() - 1);
                    break;
                }
            }
            let hi = b.split_off(cut);
            boxes.push(b);
            boxes.push(hi);
        }
        let mean = |b: &[usize]| {
            let (mut m, mut wt) = ([0f32; 3], 0f32);
            for &i in b {
                for c in 0..3 {
                    m[c] += pts[i].0[c] * pts[i].1;
                }
                wt += pts[i].1;
            }
            [m[0] / wt, m[1] / wt, m[2] / wt]
        };
        let mut cent: Vec<[f32; 3]> = boxes.iter().map(|b| mean(b)).collect();
        let nearest = |cent: &[[f32; 3]], p: &[f32; 3]| {
            let mut best = (f32::MAX, 0);
            for (k, c) in cent.iter().enumerate() {
                let d = (c[0] - p[0]).powi(2) + (c[1] - p[1]).powi(2) + (c[2] - p[2]).powi(2);
                if d < best.0 {
                    best = (d, k);
                }
            }
            best.1
        };
        for _ in 0..4 {
            let mut groups: Vec<Vec<usize>> = vec![Vec::new(); cent.len()];
            for (i, p) in pts.iter().enumerate() {
                groups[nearest(&cent, &p.0)].push(i);
            }
            for (k, g) in groups.iter().enumerate() {
                if !g.is_empty() {
                    cent[k] = mean(g);
                }
            }
        }
        let colors: Vec<[u8; 3]> = cent.iter().map(|c| [c[0].round() as u8, c[1].round() as u8, c[2].round() as u8]).collect();
        let centers: Vec<[f32; 3]> = colors.iter().map(|c| [c[0] as f32, c[1] as f32, c[2] as f32]).collect();
        let mut lut = vec![0u8; 32768];
        for (i, e) in lut.iter_mut().enumerate() {
            let p = if sum[i][3] > 0 {
                [sum[i][0] as f32 / sum[i][3] as f32, sum[i][1] as f32 / sum[i][3] as f32, sum[i][2] as f32 / sum[i][3] as f32]
            } else {
                [((i >> 10) * 8 + 4) as f32, (((i >> 5) & 31) * 8 + 4) as f32, ((i & 31) * 8 + 4) as f32]
            };
            *e = nearest(&centers, &p) as u8;
        }
        Palette { colors, lut }
    }

    pub fn quantize(&self, rgb: &[u8]) -> Vec<u8> {
        rgb.chunks_exact(3).map(|p| self.lut[bin(p[0], p[1], p[2])]).collect()
    }
}

pub struct Encoded {
    frame: gif::Frame<'static>,
}

pub struct Writer {
    pub palette: Palette,
    enc: gif::Encoder<BufWriter<File>>,
    delay: u16,
    transparent: u8,
}

impl Writer {
    pub fn new(path: &Path, palette: Palette, delay_cs: u16) -> Writer {
        let size = (palette.colors.len() + 1).next_power_of_two();
        let mut table = vec![0u8; size * 3];
        for (i, c) in palette.colors.iter().enumerate() {
            table[i * 3..i * 3 + 3].copy_from_slice(c);
        }
        let transparent = palette.colors.len() as u8;
        let f = BufWriter::new(File::create(path).expect("create gif"));
        let mut enc = gif::Encoder::new(f, GW as u16, GH as u16, &table).expect("gif header");
        enc.set_repeat(gif::Repeat::Infinite).expect("gif loop");
        Writer { palette, enc, delay: delay_cs, transparent }
    }

    /// Crop to the changed rectangle, mark unchanged pixels transparent, LZW-compress.
    pub fn encode(&self, idx: &[u8], prev: Option<&[u8]>) -> Encoded {
        let (mut x0, mut y0, mut x1, mut y1) = (0, 0, GW, GH);
        if let Some(p) = prev {
            let row = |y: usize| (&idx[y * GW..(y + 1) * GW], &p[y * GW..(y + 1) * GW]);
            match (0..GH).find(|&y| row(y).0 != row(y).1) {
                None => (x0, y0, x1, y1) = (0, 0, 1, 1),
                Some(first) => {
                    y0 = first;
                    y1 = (0..GH).rfind(|&y| row(y).0 != row(y).1).unwrap() + 1;
                    (x0, x1) = (GW, 0);
                    for y in y0..y1 {
                        let (a, b) = row(y);
                        if let Some(f) = a.iter().zip(b).position(|(p, q)| p != q) {
                            x0 = x0.min(f);
                            x1 = x1.max(GW - a.iter().rev().zip(b.iter().rev()).position(|(p, q)| p != q).unwrap());
                        }
                    }
                }
            }
        }
        let (w, h) = (x1 - x0, y1 - y0);
        let mut buf = Vec::with_capacity(w * h);
        for y in y0..y1 {
            let cur = &idx[y * GW + x0..y * GW + x1];
            match prev {
                Some(p) => {
                    let pr = &p[y * GW + x0..y * GW + x1];
                    buf.extend(cur.iter().zip(pr).map(|(&c, &q)| if c == q { self.transparent } else { c }));
                }
                None => buf.extend_from_slice(cur),
            }
        }
        let mut frame = gif::Frame::from_indexed_pixels(w as u16, h as u16, buf, prev.map(|_| self.transparent));
        frame.left = x0 as u16;
        frame.top = y0 as u16;
        frame.delay = self.delay;
        frame.dispose = gif::DisposalMethod::Keep;
        frame.make_lzw_pre_encoded();
        Encoded { frame }
    }

    pub fn write(&mut self, e: &Encoded) {
        self.enc.write_lzw_pre_encoded_frame(&e.frame).expect("gif frame");
    }
}
