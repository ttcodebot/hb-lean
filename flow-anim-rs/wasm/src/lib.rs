//! WebAssembly wrapper around the renderer core: no wasm-bindgen, plain C ABI exports.
//! JS copies scene.json into memory from `alloc`, calls `init`, then `render(t)` per frame and
//! blits the RGBA buffer at `frame_ptr()` (1200x630) to a canvas with putImageData.
use flowanim_core::{Canvas, Player, Scene, H, W};

struct State {
    player: Player,
    canvas: Canvas,
}
static mut STATE: Option<State> = None;

#[no_mangle]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
    let mut v = Vec::<u8>::with_capacity(len);
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p
}

/// Returns the animation duration in seconds, or -1 if the scene does not parse.
#[no_mangle]
pub unsafe extern "C" fn init(ptr: *mut u8, len: usize) -> f64 {
    let bytes = Vec::from_raw_parts(ptr, len, len);
    let Ok(scene) = Scene::from_json(&bytes) else { return -1.0 };
    let player = Player::new(scene);
    let d = player.duration;
    *(&raw mut STATE) = Some(State { player, canvas: Canvas::new(W, H) });
    d
}

#[no_mangle]
pub unsafe extern "C" fn render(t: f64) {
    if let Some(s) = (*(&raw mut STATE)).as_mut() {
        s.player.draw(&mut s.canvas, t);
    }
}

#[no_mangle]
pub unsafe extern "C" fn frame_ptr() -> *const u8 {
    match (*(&raw const STATE)).as_ref() {
        Some(s) => s.canvas.data().as_ptr(),
        None => std::ptr::null(),
    }
}

#[no_mangle]
pub extern "C" fn width() -> u32 {
    W
}

#[no_mangle]
pub extern "C" fn height() -> u32 {
    H
}
