//! Animated WebP. Two paths:
//! - `encode_delta` + `mux`: every frame is encoded on its own (lossy VP8 of the changed
//!   rectangle) so frames encode in parallel; the RIFF/ANMF container is written here.
//! - `Stock`: libwebp's WebPAnimEncoder, sequential; what ffmpeg's libwebp_anim uses.
use libwebp_sys as w;

pub struct Frame {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    dur: u32,
    /// image chunks (ALPH, VP8) with headers and padding; empty when nothing changed
    chunk: Vec<u8>,
}

fn config(q: f32) -> w::WebPConfig {
    let mut c = w::WebPConfig::new_with_preset(w::WebPPreset::WEBP_PRESET_DEFAULT, q).expect("webp config");
    c.method = 4;
    c
}

/// What to send for one frame: the changed rectangle as RGBA, unchanged 8x8 blocks transparent.
pub struct Plan {
    x: usize,
    y: usize,
    rw: usize,
    rh: usize,
    buf: Vec<u8>,
    any_clear: bool,
}

/// Sequential part of the delta encoder. Like libwebp's animation encoder, 8x8 blocks that are
/// (nearly) what the viewer already sees become transparent, so static regions cost almost nothing.
/// `shown` tracks the source pixels last sent per block, so slow fades are refreshed once they
/// drift past the tolerance instead of being dropped forever.
pub struct Planner {
    width: usize,
    height: usize,
    shown: Vec<u8>,
    first: bool,
    pub tol: u8,
}

impl Planner {
    pub fn new(width: usize, height: usize, q: f32) -> Planner {
        // libwebp anim_encode.c QualityToMaxDiff
        let v = (q / 100.0).sqrt();
        let tol = (31.0 * (1.0 - v) + v + 0.5) as u8;
        Planner { width, height, shown: vec![0; width * height * 4], first: true, tol }
    }

    /// None when no block changed. `exact` forces tolerance 0 (used for the last frame).
    pub fn plan(&mut self, cur: &[u8], exact: bool) -> Option<Plan> {
        let (w, h) = (self.width, self.height);
        let stride = w * 4;
        if self.first {
            self.first = false;
            self.shown.copy_from_slice(cur);
            return Some(Plan { x: 0, y: 0, rw: w, rh: h, buf: cur.to_vec(), any_clear: false });
        }
        let tol = if exact { 0 } else { self.tol };
        let (bw_n, bh_n) = (w.div_ceil(8), h.div_ceil(8));
        let mut changed = vec![false; bw_n * bh_n];
        let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
        for by in 0..bh_n {
            for bx in 0..bw_n {
                let bw = 8.min(w - bx * 8);
                let mut diff = false;
                for yy in by * 8..(by * 8 + 8).min(h) {
                    let o = yy * stride + bx * 32;
                    let (a, b) = (&cur[o..o + bw * 4], &self.shown[o..o + bw * 4]);
                    if a != b && (tol == 0 || a.iter().zip(b).any(|(p, q)| p.abs_diff(*q) > tol)) {
                        diff = true;
                        break;
                    }
                }
                if diff {
                    changed[by * bw_n + bx] = true;
                    (x0, y0, x1, y1) = (x0.min(bx), y0.min(by), x1.max(bx + 1), y1.max(by + 1));
                    for yy in by * 8..(by * 8 + 8).min(h) {
                        let o = yy * stride + bx * 32;
                        self.shown[o..o + bw * 4].copy_from_slice(&cur[o..o + bw * 4]);
                    }
                }
            }
        }
        if x1 == 0 {
            return None;
        }
        let (x, y, rw, rh) = (x0 * 8, y0 * 8, (x1 * 8).min(w) - x0 * 8, (y1 * 8).min(h) - y0 * 8);
        let mut buf = vec![0u8; rw * rh * 4];
        let mut any_clear = false;
        for by in y0..y1 {
            for bx in x0..x1 {
                let bw = 8.min(w - bx * 8);
                let rows = by * 8..(by * 8 + 8).min(h);
                let dst = |yy: usize| ((yy - y) * rw + bx * 8 - x) * 4;
                if changed[by * bw_n + bx] {
                    for yy in rows {
                        let o = yy * stride + bx * 32;
                        buf[dst(yy)..dst(yy) + bw * 4].copy_from_slice(&cur[o..o + bw * 4]);
                    }
                } else {
                    any_clear = true;
                    let mut acc = [0u32; 3];
                    for yy in rows.clone() {
                        let o = yy * stride + bx * 32;
                        for px in cur[o..o + bw * 4].chunks_exact(4) {
                            acc[0] += px[0] as u32;
                            acc[1] += px[1] as u32;
                            acc[2] += px[2] as u32;
                        }
                    }
                    let n = (bw * rows.len()) as u32;
                    let flat = [(acc[0] / n) as u8, (acc[1] / n) as u8, (acc[2] / n) as u8, 0];
                    for yy in rows {
                        for px in buf[dst(yy)..dst(yy) + bw * 4].chunks_exact_mut(4) {
                            px.copy_from_slice(&flat);
                        }
                    }
                }
            }
        }
        Some(Plan { x, y, rw, rh, buf, any_clear })
    }
}

/// Parallel part: lossy VP8 (+ lossless alpha) of one planned rectangle.
pub fn encode_plan(plan: Option<&Plan>, q: f32, dur: u32) -> Frame {
    let Some(&Plan { x, y, rw, rh, ref buf, any_clear }) = plan else {
        return Frame { x: 0, y: 0, w: 0, h: 0, dur, chunk: Vec::new() };
    };
    let cfg = config(q);
    unsafe {
        let mut pic = w::WebPPicture::new().expect("webp picture");
        pic.width = rw as i32;
        pic.height = rh as i32;
        let ok = if any_clear {
            w::WebPPictureImportRGBA(&mut pic, buf.as_ptr(), (rw * 4) as i32)
        } else {
            w::WebPPictureImportRGBX(&mut pic, buf.as_ptr(), (rw * 4) as i32)
        };
        assert!(ok != 0, "WebPPictureImport");
        let mut wr = std::mem::zeroed::<w::WebPMemoryWriter>();
        w::WebPMemoryWriterInit(&mut wr);
        pic.writer = Some(w::WebPMemoryWrite);
        pic.custom_ptr = &mut wr as *mut _ as *mut _;
        let ok = w::WebPEncode(&cfg, &mut pic);
        assert!(ok != 0, "WebPEncode: {:?}", pic.error_code);
        // RIFF....WEBP, then chunks: keep the image data (ALPH, VP8), drop the still-image VP8X header
        let out = std::slice::from_raw_parts(wr.mem, wr.size);
        let mut chunk = Vec::with_capacity(out.len());
        let mut o = 12;
        while o + 8 <= out.len() {
            let len = u32::from_le_bytes(out[o + 4..o + 8].try_into().unwrap()) as usize;
            let end = (o + 8 + len + (len & 1)).min(out.len());
            if &out[o..o + 4] != b"VP8X" {
                chunk.extend_from_slice(&out[o..end]);
                if (end - o) & 1 == 1 {
                    chunk.push(0);
                }
            }
            o = end;
        }
        w::WebPMemoryWriterClear(&mut wr);
        w::WebPPictureFree(&mut pic);
        Frame { x: x as u32, y: y as u32, w: rw as u32, h: rh as u32, dur, chunk }
    }
}

/// Append in display order; an unchanged frame extends the previous one.
pub fn push(frames: &mut Vec<Frame>, f: Frame) {
    match frames.last_mut() {
        Some(last) if f.chunk.is_empty() => last.dur += f.dur,
        _ => frames.push(f),
    }
}

fn u24(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_le_bytes()[..3]);
}

pub fn mux(frames: &[Frame], width: u32, height: u32) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(b"RIFF\0\0\0\0WEBPVP8X");
    v.extend_from_slice(&10u32.to_le_bytes());
    v.extend_from_slice(&[0x02 | 0x10, 0, 0, 0]); // animation + alpha flags
    u24(&mut v, width - 1);
    u24(&mut v, height - 1);
    v.extend_from_slice(b"ANIM");
    v.extend_from_slice(&6u32.to_le_bytes());
    v.extend_from_slice(&[0, 0, 0, 0, 0, 0]); // background colour, loop count 0 = forever
    for f in frames {
        v.extend_from_slice(b"ANMF");
        v.extend_from_slice(&(16 + f.chunk.len() as u32).to_le_bytes());
        u24(&mut v, f.x / 2);
        u24(&mut v, f.y / 2);
        u24(&mut v, f.w - 1);
        u24(&mut v, f.h - 1);
        u24(&mut v, f.dur);
        v.push(0x00); // alpha-blend over the previous canvas, do not dispose
        v.extend_from_slice(&f.chunk);
    }
    let size = (v.len() - 8) as u32;
    v[4..8].copy_from_slice(&size.to_le_bytes());
    v
}

pub struct Stock {
    enc: *mut w::WebPAnimEncoder,
    cfg: w::WebPConfig,
    width: u32,
    height: u32,
}

impl Stock {
    pub fn new(width: u32, height: u32, q: f32) -> Stock {
        unsafe {
            let mut opt = std::mem::zeroed::<w::WebPAnimEncoderOptions>();
            assert!(w::WebPAnimEncoderOptionsInitInternal(&mut opt, w::WEBP_MUX_ABI_VERSION as _) != 0);
            let enc = w::WebPAnimEncoderNewInternal(width as i32, height as i32, &opt, w::WEBP_MUX_ABI_VERSION as _);
            assert!(!enc.is_null());
            Stock { enc, cfg: config(q), width, height }
        }
    }
    pub fn add(&mut self, rgba: &[u8], ts: u32) {
        unsafe {
            let mut pic = w::WebPPicture::new().expect("webp picture");
            pic.width = self.width as i32;
            pic.height = self.height as i32;
            pic.use_argb = 1;
            assert!(w::WebPPictureImportRGBX(&mut pic, rgba.as_ptr(), (self.width * 4) as i32) != 0);
            assert!(w::WebPAnimEncoderAdd(self.enc, &mut pic, ts as i32, &self.cfg) != 0, "WebPAnimEncoderAdd");
            w::WebPPictureFree(&mut pic);
        }
    }
    pub fn finish(self, end_ts: u32) -> Vec<u8> {
        unsafe {
            assert!(w::WebPAnimEncoderAdd(self.enc, std::ptr::null_mut(), end_ts as i32, std::ptr::null()) != 0);
            let mut data = w::WebPData::default();
            assert!(w::WebPAnimEncoderAssemble(self.enc, &mut data) != 0, "WebPAnimEncoderAssemble");
            let out = std::slice::from_raw_parts(data.bytes, data.size).to_vec();
            w::WebPDataClear(&mut data);
            w::WebPAnimEncoderDelete(self.enc);
            out
        }
    }
}
