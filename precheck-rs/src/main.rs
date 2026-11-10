//! precheck-rs: prototype of the Tiny Tapeout gf180mcuD precheck DRC in plain Rust.
//! Reads a GDS, flattens it, runs a slice of the gf180mcu KLayout rule deck natively and
//! writes per-rule violation markers as JSON.

mod check;
mod edges;
mod gds;
mod geom;
mod region;
mod rules;
mod simple;

use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: precheck-rs <gds> [--top NAME] [--out markers.json] [--threads N] [--only RULE,RULE] [--timing] [--repeat N]");
        std::process::exit(2);
    }
    let mut top = None;
    let mut out = None;
    let mut threads = 1usize;
    let mut only: Option<Vec<String>> = None;
    let mut timing = false;
    let mut repeat = 1usize;
    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--top" => { top = Some(args[i + 1].clone()); i += 1 }
            "--out" => { out = Some(args[i + 1].clone()); i += 1 }
            "--threads" => { threads = args[i + 1].parse().unwrap(); i += 1 }
            "--only" => { only = Some(args[i + 1].split(',').map(|s| s.to_string()).collect()); i += 1 }
            "--repeat" => { repeat = args[i + 1].parse().unwrap(); i += 1 }
            "--timing" => timing = true,
            a => { eprintln!("unknown argument {a}"); std::process::exit(2) }
        }
        i += 1;
    }
    rayon::ThreadPoolBuilder::new().num_threads(threads).build_global().unwrap();

    for rep in 0..repeat {
        let t0 = Instant::now();
        let data = std::fs::read(&args[1]).expect("cannot read GDS");
        let lib = gds::read(&data).expect("GDS parse error");
        let t_read = t0.elapsed();

        let simple = simple::run(&lib, top.as_deref());
        let top_name = simple.top.clone();
        let t_simple = t0.elapsed();

        let res = rules::run(&lib, &top_name, threads, only.as_deref());
        let t_rules = t0.elapsed();

        if rep + 1 == repeat {
            if let Some(path) = &out {
                std::fs::write(path, rules::to_json(&res, &simple)).expect("cannot write output");
            }
            let total: usize = res.rules.iter().map(|r| r.markers.len()).sum();
            for c in &simple.checks {
                println!("check {:28} {}", c.0, if c.1.is_empty() { "ok".to_string() } else { format!("FAIL: {}", c.1) });
            }
            let mut viol: Vec<_> = res.rules.iter().filter(|r| r.markers.len() > 0).collect();
            viol.sort_by(|a, b| a.name.cmp(&b.name));
            for r in viol {
                println!("rule  {:28} {:6} violation(s)", r.name, r.markers.len());
            }
            if timing {
                let mut t: Vec<_> = res.rules.iter().collect();
                t.sort_by(|a, b| b.seconds.partial_cmp(&a.seconds).unwrap());
                for r in t.iter().take(25) {
                    println!("time  {:28} {:9.3} ms", r.name, r.seconds * 1e3);
                }
                for (n, s) in &res.phases {
                    println!("phase {:28} {:9.3} ms", n, s * 1e3);
                }
            }
            println!("rules run: {}, violations: {}", res.rules.len(), total);
        }
        println!(
            "wall: total {:.1} ms (read {:.1} ms, non-deck checks {:.1} ms, deck slice {:.1} ms) threads={}",
            t_rules.as_secs_f64() * 1e3,
            t_read.as_secs_f64() * 1e3,
            (t_simple - t_read).as_secs_f64() * 1e3,
            (t_rules - t_simple).as_secs_f64() * 1e3,
            threads
        );
    }
}
