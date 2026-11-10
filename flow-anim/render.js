// Usage: node render.js OUT_DIR [fps]
// Reads OUT_DIR/scene.json, writes OUT_DIR/player.html, frames, mp4/gif/webp and key frames.
const fs = require('fs');
const path = require('path');
const { execFileSync, spawn } = require('child_process');
const { chromium } = require('playwright');

(async () => {
  const out = path.resolve(process.argv[2]);
  const fps = parseInt(process.argv[3] || '25', 10);
  const T = {};
  let t0 = Date.now();
  const scene = fs.readFileSync(path.join(out, 'scene.json'), 'utf8');
  const design = JSON.parse(scene).design;
  const html = fs.readFileSync(path.join(__dirname, 'template.html'), 'utf8')
    .replace('__DESIGN__', () => design)
    .replace('__SCENE__', () => scene)
    .replace('__PLAYER__', () => fs.readFileSync(path.join(__dirname, 'player.js'), 'utf8'));
  const htmlPath = path.join(out, 'player.html');
  fs.writeFileSync(htmlPath, html);
  T.html = Date.now() - t0;

  t0 = Date.now();
  const browser = await chromium.launch();
  const page = await browser.newPage({ viewport: { width: 1200, height: 630 }, deviceScaleFactor: 1 });
  page.on('console', (m) => console.log('[page]', m.text()));
  page.on('pageerror', (e) => { console.error('[pageerror]', e.message); process.exitCode = 1; });
  await page.goto('file://' + htmlPath + '#capture');
  const { duration, stages } = await page.evaluate(() => ({ duration: flowAnim.duration, stages: flowAnim.stages.map((s) => [s.id, s.t0, s.dur]) }));
  T.launch = Date.now() - t0;

  // key frames
  const kdir = path.join(out, 'keyframes');
  fs.mkdirSync(kdir, { recursive: true });
  const canvas = page.locator('#c');
  for (const [i, [id, s0, dur]] of stages.entries()) {
    const frac = id === 'final' ? 0.99 : id === 'gpl' ? 0.35 : 0.7;
    await page.evaluate((t) => flowAnim.renderAt(t), s0 + dur * frac);
    await canvas.screenshot({ path: path.join(kdir, `${String(i + 1).padStart(2, '0')}-${id}.png`) });
  }

  // frames -> ffmpeg (PNG pipe)
  t0 = Date.now();
  const n = Math.round(duration * fps);
  const mp4 = path.join(out, 'flow.mp4');
  const ff = spawn('ffmpeg', ['-v', 'error', '-y', '-f', 'image2pipe', '-framerate', String(fps), '-c:v', 'png', '-i', '-',
    '-c:v', 'libx264', '-preset', 'medium', '-crf', '23', '-pix_fmt', 'yuv420p', '-movflags', '+faststart', mp4], { stdio: ['pipe', 'inherit', 'inherit'] });
  const fdir = path.join(out, 'frames');
  fs.rmSync(fdir, { recursive: true, force: true });
  fs.mkdirSync(fdir, { recursive: true });
  for (let i = 0; i < n; i++) {
    const b64 = await page.evaluate((t) => { flowAnim.renderAt(t); return document.getElementById('c').toDataURL('image/png').slice(22); }, i / fps);
    const buf = Buffer.from(b64, 'base64');
    fs.writeFileSync(path.join(fdir, `f${String(i).padStart(4, '0')}.png`), buf);
    if (!ff.stdin.write(buf)) await new Promise((r) => ff.stdin.once('drain', r));
  }
  ff.stdin.end();
  await new Promise((r) => ff.on('close', r));
  T.frames_mp4 = Date.now() - t0;
  await browser.close();

  const inp = ['-v', 'error', '-y', '-framerate', String(fps), '-i', path.join(fdir, 'f%04d.png')];
  t0 = Date.now();
  // GIF: 800 px wide, half frame rate, 128-colour global palette, frame-diff rectangles
  execFileSync('ffmpeg', [...inp, '-filter_complex',
    `fps=${fps / 2},scale=800:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=128:stats_mode=diff[p];[b][p]paletteuse=dither=none:diff_mode=rectangle`,
    '-loop', '0', path.join(out, 'flow.gif')]);
  T.gif = Date.now() - t0;
  t0 = Date.now();
  execFileSync('ffmpeg', [...inp, '-vf', `fps=${fps / 2}`, '-c:v', 'libwebp_anim', '-q:v', '50', '-compression_level', '4', '-loop', '0', path.join(out, 'flow.webp')]);
  T.webp = Date.now() - t0;
  fs.copyFileSync(path.join(fdir, `f${String(n - 1).padStart(4, '0')}.png`), path.join(out, 'poster.png'));
  if (!process.env.KEEP_FRAMES) fs.rmSync(fdir, { recursive: true, force: true });

  const sizes = {};
  for (const f of ['scene.json', 'player.html', 'flow.mp4', 'flow.gif', 'flow.webp', 'poster.png']) sizes[f] = fs.statSync(path.join(out, f)).size;
  console.log(JSON.stringify({ design, duration, fps, frames: n, ms: T, bytes: sizes }, null, 1));
})();
