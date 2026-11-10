# Flow animation: native Rust renderer (prototype)

Renders the `flow-anim/` scene JSON without a browser: animated WebP, GIF, poster and keyframes natively, MP4 by piping raw frames to `ffmpeg`.
`core` is a port of `flow-anim/player.js` and also compiles to `wasm32-unknown-unknown` (`wasm/`, `web/index.html`).

`compare-07-drt.png`: browser reference (left) against this renderer (right), difference x4 below.
