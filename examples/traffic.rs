//! Traffic from above: runs the open world's traffic around a stand-in player driving the lanes
//! and writes top-down SVG frames (lanes grey, junction turns amber, cars in their paint with a
//! yellow nose; green outline = holds its turn at a junction, red = stopped after a hit; the
//! blue ring is the player, blue dots its trail). Also prints the step cost.
//!
//! ```sh
//! cargo run --release --example traffic -- --out /tmp/traffic [--cars 60] [--minutes 3]
//!     [--every 30] [--half 250] [--at x,z] [--speed 13] [--png]
//! ```
//! `--at` fixes the view (default: follows the player). `--png` also converts each frame with
//! `rsvg-convert` (Homebrew `librsvg`).

use std::time::Instant;

use bevy::math::Vec2;
use pacenote::open::traffic::{LaneGraph, LaneWalker, Traffic, TrafficConfig};
use pacenote::open::{SEED, roads};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let out = arg("--out").unwrap_or_else(|| "traffic_frames".into());
    let cars: usize = arg("--cars").map_or(60, |v| v.parse().unwrap());
    let minutes: f32 = arg("--minutes").map_or(3.0, |v| v.parse().unwrap());
    let every: f32 = arg("--every").map_or(30.0, |v| v.parse().unwrap());
    let half: f32 = arg("--half").map_or(250.0, |v| v.parse().unwrap());
    let speed: f32 = arg("--speed").map_or(13.0, |v| v.parse().unwrap());
    let at: Option<Vec2> = arg("--at").map(|v| {
        let p: Vec<f32> = v.split(',').map(|x| x.parse().unwrap()).collect();
        Vec2::new(p[0], p[1])
    });
    let png = args.iter().any(|a| a == "--png");
    std::fs::create_dir_all(&out).unwrap();

    let t0 = Instant::now();
    let net = roads::generate(SEED);
    let t1 = Instant::now();
    let graph = std::sync::Arc::new(LaneGraph::build(&net));
    let turns = graph.lanes.iter().filter(|l| l.is_turn()).count();
    println!(
        "roads {:.0} ms, lane graph {:.0} ms: {} lanes ({} turns), {} nodes",
        (t1 - t0).as_secs_f32() * 1e3,
        t1.elapsed().as_secs_f32() * 1e3,
        graph.lanes.len(),
        turns,
        graph.nodes.len()
    );
    let mut traffic = Traffic::with_graph(graph.clone(), TrafficConfig { cars, ..Default::default() });
    let start = at.unwrap_or_else(|| {
        let (p, _) = roads::spawn(&net);
        Vec2::new(p.x, p.z)
    });
    let mut walker = LaneWalker::new(&graph, start, speed, 5);
    let dt = 1.0 / 60.0;
    let steps = (minutes * 3600.0) as usize;
    let mut trail: Vec<Vec2> = Vec::new();
    let (mut spent, mut worst) = (0.0f64, 0.0f64);
    for k in 0..steps {
        walker.step(&graph, dt);
        let p = walker.player(&graph);
        let t = Instant::now();
        traffic.step(dt, Some(p));
        let e = t.elapsed().as_secs_f64();
        spent += e;
        worst = worst.max(e);
        if k % 30 == 0 {
            trail.push(Vec2::new(p.pos.x, p.pos.z));
        }
        if k % ((every * 60.0) as usize).max(1) == 0 {
            let centre = at.unwrap_or(Vec2::new(p.pos.x, p.pos.z));
            let svg = traffic.svg(&net, centre, half, Some(p.pos), &trail);
            let name = format!("{out}/traffic_{:04}.svg", k / 60);
            std::fs::write(&name, svg).unwrap();
            if png {
                let _ = std::process::Command::new("rsvg-convert").arg("-o").arg(name.replace(".svg", ".png")).arg(&name).status();
            }
        }
    }
    println!("{} steps, {} cars at the end, longest wait now {:.1} s", steps, traffic.len(), traffic.longest_wait());
    println!("step: mean {:.1} µs, worst {:.1} µs", spent / steps as f64 * 1e6, worst * 1e6);
}
