//! flowanim: render scene.json to animated WebP / GIF / MP4 / PNG key frames without a browser.
mod gifenc;
mod webpenc;

use flowanim_core::{Canvas, Player, Scene, H, W};
use rayon::prelude::*;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::time::Instant;

struct Args {
    scene: PathBuf,
    out: PathBuf,
    fps: u32,
    threads: usize,
    webp: bool,
    gif: bool,
    mp4: bool,
    png: bool,
    webp_stock: bool,
    webp_q: f32,
    webp_tol: Option<u8>,
    gif_colors: usize,
    fast_rects: bool,
    profile: bool,
    at: Vec<(f64, PathBuf)>,
}

fn parse_args() -> Args {
    let mut a = Args {
        scene: PathBuf::new(),
        out: PathBuf::from("."),
        fps: 25,
        threads: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
        webp: false,
        gif: false,
        mp4: false,
        png: false,
        webp_stock: false,
        webp_q: 50.0,
        webp_tol: None,
        gif_colors: 128,
        fast_rects: true,
        profile: false,
        at: Vec::new(),
    };
    let mut it = std::env::args().skip(1);
    while let Some(s) = it.next() {
        let mut val = || it.next().unwrap_or_else(|| usage());
        match s.as_str() {
            "-o" => a.out = val().into(),
            "--fps" => a.fps = val().parse().unwrap_or_else(|_| usage()),
            "--threads" | "-j" => a.threads = val().parse().unwrap_or_else(|_| usage()),
            "--webp" => a.webp = true,
            "--gif" => a.gif = true,
            "--mp4" => a.mp4 = true,
            "--png" => a.png = true,
            "--all" => (a.webp, a.gif, a.mp4, a.png) = (true, true, true, true),
            "--webp-stock" => a.webp_stock = true,
            "--webp-tol" => a.webp_tol = Some(val().parse().unwrap_or_else(|_| usage())),
            "--webp-q" => a.webp_q = val().parse().unwrap_or_else(|_| usage()),
            "--gif-colors" => a.gif_colors = val().parse().unwrap_or_else(|_| usage()),
            "--tiny-skia" => a.fast_rects = false,
            "--profile" => a.profile = true,
            "--at" => {
                let t = val().parse().unwrap_or_else(|_| usage());
                a.at.push((t, val().into()));
            }
            _ if a.scene.as_os_str().is_empty() && !s.starts_with('-') => a.scene = s.into(),
            _ => usage(),
        }
    }
    if a.scene.as_os_str().is_empty() {
        usage();
    }
    a
}

fn usage() -> ! {
    eprintln!(
        "usage: flowanim SCENE.json [-o DIR] [--webp] [--gif] [--mp4] [--png] [--all] [--fps 25] [-j N]\n\
         \x20      [--webp-stock] [--webp-q 50] [--webp-tol N] [--gif-colors 128] [--tiny-skia] [--at T FILE.png]...\n\
         With no output flag only the frames are drawn (timing run). --mp4 needs the ffmpeg binary."
    );
    std::process::exit(2)
}

fn write_png(path: &Path, rgba: &[u8], w: u32, h: u32) {
    let mut rgb = Vec::with_capacity((w * h * 3) as usize);
    for p in rgba.chunks_exact(4) {
        rgb.extend_from_slice(&p[..3]);
    }
    let f = std::io::BufWriter::new(std::fs::File::create(path).expect("create png"));
    let mut e = png::Encoder::new(f, w, h);
    e.set_color(png::ColorType::Rgb);
    e.set_depth(png::BitDepth::Eight);
    let mut wr = e.write_header().expect("png header");
    wr.write_image_data(&rgb).expect("png data");
}

fn ms(t: Instant) -> f64 {
    (t.elapsed().as_secs_f64() * 1e4).round() / 10.0
}

fn peak_rss_kb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| s.lines().find(|l| l.starts_with("VmHWM:")).and_then(|l| l.split_whitespace().nth(1)?.parse().ok()))
        .unwrap_or(0)
}

fn main() {
    let a = parse_args();
    let t_all = Instant::now();
    let mut tm: Vec<(&str, f64)> = Vec::new();

    let t = Instant::now();
    let bytes = std::fs::read(&a.scene).expect("read scene");
    let scene = Scene::from_json(&bytes).expect("parse scene");
    let player = Player::new(scene);
    tm.push(("load_ms", ms(t)));
    std::fs::create_dir_all(&a.out).expect("out dir");
    let pool = rayon::ThreadPoolBuilder::new().num_threads(a.threads).build().expect("pool");

    let fps = a.fps as f64;
    let n = (player.duration * fps).round() as usize;
    let new_canvas = || {
        let mut c = Canvas::new(W, H);
        c.fast = a.fast_rects;
        c
    };

    // ---- per-stage draw cost, single thread ----
    if a.profile {
        let mut cv = new_canvas();
        cv.prof_on = true;
        eprintln!("ms/frame      n   total   clear   rects   lines  srects circles    text");
        for st in &player.stages {
            let frames: Vec<f64> = (0..n).map(|i| i as f64 / fps).filter(|t| *t >= st.t0 && *t < st.t0 + st.dur).collect();
            let t = Instant::now();
            for ft in &frames {
                player.draw(&mut cv, *ft);
            }
            let k = 1e3 / frames.len().max(1) as f64;
            eprint!("{:10} {:3} {:7.2}", st.id, frames.len(), t.elapsed().as_secs_f64() * k);
            for p in cv.prof.iter_mut() {
                eprint!(" {:7.2}", *p * k);
                *p = 0.0;
            }
            eprintln!();
        }
        return;
    }

    // ---- single frames at given timestamps ----
    for (t, path) in &a.at {
        let mut cv = new_canvas();
        player.draw(&mut cv, *t);
        write_png(path, cv.data(), W, H);
    }

    // ---- key frames + poster (also the palette samples for the GIF) ----
    let mut key_times: Vec<(f64, String)> =
        (0..player.stages.len()).map(|i| (player.keyframe_time(i), format!("keyframes/{:02}-{}.png", i + 1, player.stages[i].id))).collect();
    key_times.push(((n - 1) as f64 / fps, "poster.png".into()));
    let mut samples: Vec<Vec<u8>> = Vec::new();
    if a.png || a.gif {
        let t = Instant::now();
        if a.png {
            std::fs::create_dir_all(a.out.join("keyframes")).expect("keyframes dir");
        }
        samples = pool.install(|| {
            key_times
                .par_iter()
                .map(|(t, name)| {
                    let mut cv = new_canvas();
                    player.draw(&mut cv, *t);
                    if a.png {
                        write_png(&a.out.join(name), cv.data(), W, H);
                    }
                    if a.gif {
                        gifenc::downscale(cv.data())
                    } else {
                        Vec::new()
                    }
                })
                .collect()
        });
        tm.push(("keyframes_png_ms", ms(t)));
    }

    // ---- encoders ----
    let mut gif = if a.gif {
        let t = Instant::now();
        let pal = gifenc::Palette::build(&samples, a.gif_colors - 1);
        tm.push(("gif_palette_ms", ms(t)));
        let delay_cs = (200.0 / fps).round() as u16; // every second frame
        Some(gifenc::Writer::new(&a.out.join("flow.gif"), pal, delay_cs))
    } else {
        None
    };
    drop(samples);
    let frame_ms = (2000.0 / fps).round() as u32;
    let mut webp_frames: Vec<webpenc::Frame> = Vec::new();
    let mut webp_stock = if a.webp && a.webp_stock { Some(webpenc::Stock::new(W, H, a.webp_q)) } else { None };
    let mut ff = if a.mp4 {
        let child = Command::new("ffmpeg")
            .args(["-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgba", "-s", &format!("{W}x{H}"), "-framerate", &a.fps.to_string(), "-i", "-"])
            .args(["-c:v", "libx264", "-preset", "medium", "-crf", "23", "-pix_fmt", "yuv420p", "-movflags", "+faststart"])
            .args(if a.threads == 1 { vec!["-threads", "1"] } else { vec![] })
            .arg(a.out.join("flow.mp4"))
            .stdin(Stdio::piped())
            .spawn()
            .expect("spawn ffmpeg");
        Some(child)
    } else {
        None
    };
    // frames go to ffmpeg from a writer thread so that x264 runs while the next batch is drawn
    let (ff_tx, ff_thread) = match ff.as_mut() {
        Some(child) => {
            let mut stdin = child.stdin.take().unwrap();
            let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(16);
            let th = std::thread::spawn(move || {
                for buf in rx {
                    stdin.write_all(&buf).expect("write to ffmpeg");
                }
            });
            (Some(tx), Some(th))
        }
        None => (None, None),
    };

    // ---- frame loop: draw a batch in parallel, encode the batch in parallel, emit in order ----
    let batch = ((a.threads * 4).max(2)) & !1;
    let mut canvases: Vec<Canvas> = (0..batch.min(n)).map(|_| new_canvas()).collect();
    let mut prev_idx: Option<Vec<u8>> = None;
    let (mut w_draw, mut w_enc, mut w_emit) = (0.0, 0.0, 0.0);
    let (cpu_draw, cpu_webp, cpu_gifq, cpu_giflzw) = (AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0));
    let add = |c: &AtomicU64, t: Instant| c.fetch_add(t.elapsed().as_nanos() as u64, Relaxed);
    let webp_par = a.webp && !a.webp_stock;
    let mut webp_plan = webp_par.then(|| webpenc::Planner::new(W as usize, H as usize, a.webp_q));
    if let (Some(p), Some(t)) = (webp_plan.as_mut(), a.webp_tol) {
        p.tol = t;
    }
    for start in (0..n).step_by(batch) {
        let cnt = batch.min(n - start);
        let t = Instant::now();
        pool.install(|| {
            canvases[..cnt].par_iter_mut().enumerate().for_each(|(j, cv)| {
                let t = Instant::now();
                player.draw(cv, (start + j) as f64 / fps);
                add(&cpu_draw, t);
            })
        });
        w_draw += t.elapsed().as_secs_f64();

        // half-rate outputs (WebP, GIF) take every second frame
        let t = Instant::now();
        let halves: Vec<usize> = (0..cnt).step_by(2).collect();
        // which blocks to send is decided in display order (cheap); the encoding runs in parallel
        let plans: Vec<Option<webpenc::Plan>> = match webp_plan.as_mut() {
            Some(pl) => halves.iter().map(|&j| pl.plan(canvases[j].data(), start + j + 2 >= n)).collect(),
            None => Vec::new(),
        };
        let stage_a: Vec<(Option<webpenc::Frame>, Option<Vec<u8>>)> = pool.install(|| {
            halves
                .par_iter()
                .enumerate()
                .map(|(h, &j)| {
                    let cur = canvases[j].data();
                    let w = if webp_par {
                        let t = Instant::now();
                        let f = webpenc::encode_plan(plans[h].as_ref(), a.webp_q, frame_ms);
                        add(&cpu_webp, t);
                        Some(f)
                    } else {
                        None
                    };
                    let g = gif.as_ref().map(|g| {
                        let t = Instant::now();
                        let idx = g.palette.quantize(&gifenc::downscale(cur));
                        add(&cpu_gifq, t);
                        idx
                    });
                    (w, g)
                })
                .collect()
        });
        let gif_frames: Vec<gifenc::Encoded> = match gif.as_ref() {
            Some(g) => pool.install(|| {
                (0..halves.len())
                    .into_par_iter()
                    .map(|h| {
                        let t = Instant::now();
                        let prev = if h > 0 { stage_a[h - 1].1.as_deref() } else { prev_idx.as_deref() };
                        let f = g.encode(stage_a[h].1.as_deref().unwrap(), prev);
                        add(&cpu_giflzw, t);
                        f
                    })
                    .collect()
            }),
            None => Vec::new(),
        };
        w_enc += t.elapsed().as_secs_f64();

        let t = Instant::now();
        if let Some(g) = gif.as_mut() {
            for f in &gif_frames {
                g.write(f);
            }
        }
        for (h, (w, g)) in stage_a.into_iter().enumerate() {
            if let Some(f) = w {
                webpenc::push(&mut webp_frames, f);
            }
            if h + 1 == halves.len() {
                prev_idx = g;
            }
        }
        if let Some(st) = webp_stock.as_mut() {
            for &j in &halves {
                st.add(canvases[j].data(), ((start + j) / 2) as u32 * frame_ms);
            }
        }
        if let Some(tx) = ff_tx.as_ref() {
            for cv in &canvases[..cnt] {
                tx.send(cv.data().to_vec()).expect("ffmpeg writer");
            }
        }
        w_emit += t.elapsed().as_secs_f64();
    }
    tm.push(("draw_wall_ms", (w_draw * 1e4).round() / 10.0));
    tm.push(("encode_wall_ms", (w_enc * 1e4).round() / 10.0));
    tm.push(("emit_wall_ms", (w_emit * 1e4).round() / 10.0));

    let t = Instant::now();
    drop(ff_tx);
    if let Some(th) = ff_thread {
        th.join().expect("ffmpeg writer thread");
    }
    if let Some(mut child) = ff {
        let st = child.wait().expect("ffmpeg");
        assert!(st.success(), "ffmpeg failed");
    }
    if a.webp {
        let nh = ((n + 1) / 2) as u32;
        let data = match webp_stock.take() {
            Some(st) => st.finish(nh * frame_ms),
            None => webpenc::mux(&webp_frames, W, H),
        };
        std::fs::write(a.out.join("flow.webp"), data).expect("write webp");
    }
    drop(gif);
    tm.push(("finish_ms", ms(t)));
    tm.push(("total_ms", ms(t_all)));

    let cpu = |c: &AtomicU64| (c.load(Relaxed) as f64 / 1e5).round() / 10.0;
    let mut out = format!(
        "{{\"design\":\"{}\",\"duration\":{:.1},\"fps\":{},\"frames\":{},\"threads\":{},\"batch\":{}",
        player.design(), player.duration, a.fps, n, a.threads, batch
    );
    for (k, v) in &tm {
        out += &format!(",\"{k}\":{v}");
    }
    out += &format!(
        ",\"cpu_draw_ms\":{},\"draw_ms_per_frame\":{:.2},\"cpu_webp_ms\":{},\"cpu_gif_quant_ms\":{},\"cpu_gif_lzw_ms\":{},\"peak_rss_kb\":{}",
        cpu(&cpu_draw), cpu(&cpu_draw) / n as f64, cpu(&cpu_webp), cpu(&cpu_gifq), cpu(&cpu_giflzw), peak_rss_kb()
    );
    for f in ["flow.webp", "flow.gif", "flow.mp4", "poster.png"] {
        if let Ok(m) = std::fs::metadata(a.out.join(f)) {
            out += &format!(",\"{f}\":{}", m.len());
        }
    }
    println!("{out}}}");
}
