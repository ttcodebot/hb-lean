//! The cheap non-deck precheck logic (the parts of precheck.py that only need the GDS):
//! top cell name / uniqueness, zero-area shapes, forbidden layers, prBoundary presence,
//! cell names, pin labels overlapping drawing.

use crate::gds::Library;
use crate::geom::ring_area2;

pub struct SimpleResult {
    pub top: String,
    pub checks: Vec<(String, String)>, // (name, failure message or empty)
}

/// gf180mcuD layer numbers used by precheck.py (tech_data.py / gf180mcuD.lyp)
const PR_BOUNDARY: (i16, i16) = (0, 0);
// forbidden_layers["gf180mcuD"]: Metal5, Metal5_Label
const FORBIDDEN: [(i16, i16); 2] = [(81, 0), (81, 10)];

pub fn run(lib: &Library, expected_top: Option<&str>) -> SimpleResult {
    let mut checks = Vec::new();
    let tops = lib.top_cells();
    let top = tops.first().map(|c| c.name.clone()).unwrap_or_default();
    let mut msg = String::new();
    if tops.len() != 1 {
        msg = format!("GDS top level not unique ({} top cells)", tops.len());
    } else if let Some(e) = expected_top {
        if e != top {
            msg = format!("Top macro name mismatch: expected {e}, got {top}");
        }
    }
    checks.push(("top_cell".to_string(), msg));

    // zero-area shapes and zero-length paths (zeroarea.rb.drc)
    let mut zero = 0usize;
    for c in &lib.cells {
        for s in &c.shapes {
            match s.path {
                None => {
                    if s.pts.len() < 3 || ring_area2(&s.pts) == 0 {
                        zero += 1
                    }
                }
                Some((w, _)) => {
                    let len: i64 = s.pts.windows(2).map(|p| ((p[0].x - p[1].x).abs() + (p[0].y - p[1].y).abs()) as i64).sum();
                    if len == 0 || w == 0 {
                        zero += 1
                    }
                }
            }
        }
    }
    checks.push(("zero_area".to_string(), if zero > 0 { format!("{zero} zero-area objects") } else { String::new() }));

    // forbidden layers (shapes or texts)
    let forb = lib.cells.iter().any(|c| c.shapes.iter().any(|s| FORBIDDEN.contains(&s.layer)) || c.texts.iter().any(|t| FORBIDDEN.contains(&t.layer)));
    checks.push(("forbidden_layers".to_string(), if forb { "Forbidden layer Metal5 found".to_string() } else { String::new() }));

    // prBoundary layer must exist
    let has_bnd = lib.cells.iter().any(|c| c.shapes.iter().any(|s| s.layer == PR_BOUNDARY));
    checks.push(("pr_boundary".to_string(), if has_bnd { String::new() } else { "prBoundary (0/0) layer not found".to_string() }));

    // top-level prBoundary rectangle must equal the bbox of everything (boundary_check, minus the DEF size)
    if let Some(tc) = lib.index.get(&top).map(|&i| &lib.cells[i]) {
        let bb = lib.bbox(&top);
        let ok = tc.shapes.iter().any(|s| {
            s.layer == PR_BOUNDARY && s.path.is_none() && {
                let mut b = crate::geom::BBox::empty();
                for p in &s.pts {
                    b.add(*p);
                }
                b == bb && bb.x1 == 0 && bb.y1 == 0
            }
        });
        checks.push(("boundary".to_string(), if ok { String::new() } else { "no top-level prBoundary rectangle covering the layout bbox at (0,0)".to_string() }));

    }

    // cell names
    let badname = lib.cells.iter().filter(|c| c.name.contains('#') || c.name.contains('/')).count();
    checks.push(("cell_names".to_string(), if badname > 0 { format!("{badname} cell names with '#' or '/'") } else { String::new() }));

    SimpleResult { top, checks }
}
