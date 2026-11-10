//! Renderer core for the hardening-flow animation: scene model, canvas wrapper and the
//! drawing logic ported from player.js. No I/O, no threads: builds for native and wasm32.
pub mod canvas;
pub mod player;
pub mod scene;

pub use canvas::Canvas;
pub use player::Player;
pub use scene::Scene;

pub const W: u32 = 1200;
pub const H: u32 = 630;
