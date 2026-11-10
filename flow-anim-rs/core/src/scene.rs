use serde::Deserialize;
use std::collections::HashMap;

/// scene.json as written by extract.py. Rows of heterogeneous arrays map to tuples.
#[derive(Deserialize)]
pub struct Scene {
    pub design: String,
    #[serde(default)]
    pub pdk: Option<String>,
    pub dbu: f64,
    pub die: [f64; 4],
    /// [x, y, width]
    pub rows: Vec<[f64; 3]>,
    #[serde(rename = "rowH")]
    pub row_h: f64,
    pub snaps: Vec<String>,
    pub runtime: HashMap<String, f64>,
    /// [name, w, h, category]
    pub macros: Vec<(String, f64, f64, String)>,
    /// [macro, born snapshot, in netlist, [[snapshot, x, y], ...], death snapshot]
    pub cells: Vec<(usize, i64, i64, Vec<[f64; 3]>, i64)>,
    /// [macro, x, y]
    pub fills: Vec<(usize, f64, f64)>,
    /// [x, y, w, h]
    pub pins: Vec<[f64; 4]>,
    pub layers: Vec<String>,
    #[serde(rename = "layerW")]
    pub layer_w: Vec<f64>,
    /// [[endpoint, ...], is_clock]; endpoint >= 0 is a cell index, < 0 is pin -1-e
    pub nets: Vec<(Vec<i64>, i64)>,
    /// [layer, x1, y1, x2, y2, net]
    pub wires: Vec<[f64; 6]>,
    /// [layer, x, y, net]
    pub vias: Vec<[f64; 4]>,
    /// [layer, x1, y1, x2, y2, width, is_ground]
    pub pdn: Vec<[f64; 7]>,
    /// [layer, x1, y1, x2, y2]
    pub guides: Vec<[f64; 5]>,
    #[serde(rename = "gplLog", default)]
    pub gpl_log: Vec<[f64; 3]>,
    #[serde(rename = "drtLog", default)]
    pub drt_log: Vec<[f64; 3]>,
}

impl Scene {
    pub fn from_json(bytes: &[u8]) -> Result<Scene, String> {
        serde_json::from_slice(bytes).map_err(|e| e.to_string())
    }
}
