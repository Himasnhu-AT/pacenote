//! Prints every stage's road (plan and height profile, every 10 m), corners and crests as JSON,
//! for the website (`web/src/data/stages.json`) and the telemetry report's crash maps:
//! `memgate -- cargo run -q --example stage_dump > web/src/data/stages.json`.

use pacenote::stage;
use serde_json::{Value, json};

/// Metres between exported points (the road has a station every `stage::STEP` m).
const EVERY_M: f32 = 10.0;

fn main() {
    let r = |v: f32| (v * 10.0).round() / 10.0;
    let mut out = Vec::new();
    for def in stage::stages() {
        let road = stage::generate_road(&def);
        let corners = stage::corners(&road);
        let crests = stage::crests(&road);
        let every = ((EVERY_M / stage::STEP).round() as usize).max(1);
        let pts: Vec<Value> = road.pts.iter().step_by(every).map(|p| json!([r(p.x), r(p.z), r(p.y)])).collect();
        out.push(json!({
            "id": def.id,
            "name": def.name,
            "biome": format!("{:?}", def.biome),
            "surface": format!("{:?}", def.surface),
            "weather": format!("{:?}", def.weather),
            "time": format!("{:?}", def.time),
            "length_m": r(road.length()),
            "step_m": EVERY_M,
            "points": pts,
            "corners": corners.iter().map(|c| json!({
                "start": r(c.start), "end": r(c.end),
                "turn": if c.turn == stage::Turn::Left { "left" } else { "right" },
                "grade": c.grade, "angle": r(c.angle), "crest": c.crest, "narrows": c.narrows,
            })).collect::<Vec<_>>(),
            "crests": crests.iter().map(|&k| r(k)).collect::<Vec<_>>(),
        }));
    }
    println!("{}", serde_json::to_string(&json!({ "stages": out })).expect("json"));
}
