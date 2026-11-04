//! Challenge tests: the file against the committed level, the re-anchor tool, and each kind's
//! attempt with synthetic drives (GAME_SYSTEMS CH-2..CH-5, CH-7).

use super::*;

const DT: f32 = 1.0 / 60.0;

fn def(id: u32, kind: ChallengeKind, gates: Vec<f32>, thresholds: [f32; 3]) -> ChallengeDef {
    ChallengeDef { id, kind, name: format!("test {id}"), road: 0, gates, thresholds, reference: thresholds[2], at: Vec2::ZERO, to: None }
}

/// A straight road along +Z (x = 0), 6 m wide: gates at stations = z.
fn straight(d: ChallengeDef) -> Placed {
    let g = |s: f32| Gate { p: Vec3::new(0.0, 0.0, s), dir: Vec2::Y, half: 3.0, s };
    let mut gates = vec![g(d.gates[0])];
    let mut length = 0.0;
    if d.kind.section() {
        let (a, b) = (d.gates[0], d.gates[1]);
        length = b - a;
        if d.kind == ChallengeKind::PaceSection {
            for k in 1..=PACE_MIDS {
                gates.push(g(a + length * k as f32 / (PACE_MIDS + 1) as f32));
            }
        }
        gates.push(g(b));
    }
    Placed { def: d, gates, length }
}

/// A car on the straight road, driven by the test.
struct Drive {
    t: Tracker,
    f: ChallengeFrame,
    out: Vec<Outcome>,
}

impl Drive {
    fn at(z: f32, x: f32) -> Drive {
        let f = ChallengeFrame {
            dt: DT,
            live: true,
            pos: Vec3::new(x, 0.5, z),
            vel: Vec3::ZERO,
            fwd: Vec3::Z,
            up_y: 1.0,
            contacts: 4,
            hits: 0,
            body_contact: false,
            slide_meter: 0.0,
            road: Some((0, x.abs())),
        };
        Drive { t: Tracker::default(), f, out: Vec::new() }
    }

    fn step(&mut self, placed: &[Placed]) {
        self.f.pos += self.f.vel * DT;
        self.f.road = Some((0, self.f.pos.x.abs()));
        self.t.step(&self.f, placed, &mut self.out);
    }

    /// Drives at `kmh` along +Z (negative: -Z) for `secs`.
    fn run(&mut self, placed: &[Placed], kmh: f32, secs: f32) {
        self.f.vel = Vec3::Z * kmh / 3.6;
        for _ in 0..(secs / DT).round() as usize {
            self.step(placed);
        }
    }

    fn finished(&self) -> Vec<f32> {
        self.out
            .iter()
            .filter_map(|o| match o {
                Outcome::Finished { value, .. } => Some(*value),
                _ => None,
            })
            .collect()
    }

    fn voids(&self) -> Vec<&'static str> {
        self.out
            .iter()
            .filter_map(|o| match o {
                Outcome::Void { why, .. } => Some(*why),
                _ => None,
            })
            .collect()
    }
}

// --- CH-2 Radar Post ---

#[test]
fn radar_records_the_speed_at_the_gate_both_ways() {
    let placed = vec![straight(def(1, ChallengeKind::RadarPost, vec![100.0], [115.0, 140.0, 160.0]))];
    let mut d = Drive::at(0.0, 0.0);
    d.run(&placed, 120.0, 5.0);
    let v = d.finished();
    assert_eq!(v.len(), 1);
    assert!((v[0] - 120.0).abs() <= 1.0, "{v:?}");
    // The other way.
    let mut d = Drive::at(200.0, 1.0);
    d.run(&placed, -90.0, 6.0);
    let v = d.finished();
    assert_eq!(v.len(), 1);
    assert!((v[0] - 90.0).abs() <= 1.0, "{v:?}");
    // Off the road (5 m past the edge): nothing.
    let mut d = Drive::at(0.0, 9.0);
    d.run(&placed, 120.0, 5.0);
    assert!(d.finished().is_empty());
}

// --- CH-3 Pace Section ---

fn pace() -> Vec<Placed> {
    vec![straight(def(5, ChallengeKind::PaceSection, vec![100.0, 1100.0], [70.0, 85.0, 97.0]))]
}

#[test]
fn pace_is_length_over_time() {
    let placed = pace();
    let mut d = Drive::at(50.0, 0.0);
    d.run(&placed, 90.0, 45.0);
    let v = d.finished();
    assert_eq!(v.len(), 1, "{:?}", d.out);
    assert!((v[0] - 90.0).abs() < 90.0 * 0.005, "{v:?}");
    let checkpoints = d.out.iter().filter(|o| matches!(o, Outcome::Checkpoint { .. })).count();
    assert_eq!(checkpoints, PACE_MIDS);
    // The live readout runs while in it.
    let mut d = Drive::at(50.0, 0.0);
    d.run(&placed, 72.0, 10.0);
    let live = d.t.live(&placed, &d.f).expect("running");
    assert!((live.value - 72.0).abs() < 2.0, "{live:?}");
    assert!(live.done > 0.1 && live.done < 0.2);
}

#[test]
fn pace_voids_on_a_skipped_gate_a_recover_or_reversing() {
    let placed = pace();
    // Round a mid-gate (20 m off the centre line) then back on.
    let mut d = Drive::at(50.0, 0.0);
    d.run(&placed, 90.0, 4.0);
    d.f.pos.x = 25.0;
    d.run(&placed, 90.0, 6.0);
    d.f.pos.x = 0.0;
    d.run(&placed, 90.0, 40.0);
    assert!(d.finished().is_empty());
    assert!(!d.voids().is_empty(), "{:?}", d.out);
    // A recover (jump forward).
    let mut d = Drive::at(50.0, 0.0);
    d.run(&placed, 90.0, 5.0);
    d.f.pos.z += 30.0;
    d.run(&placed, 90.0, 40.0);
    assert_eq!(d.voids(), vec!["Recovered"]);
    assert!(d.finished().is_empty());
    // Reversing.
    let mut d = Drive::at(50.0, 0.0);
    d.run(&placed, 90.0, 5.0);
    d.run(&placed, -10.0, 2.0);
    assert_eq!(d.voids(), vec!["Reversed"]);
}

#[test]
fn pace_restarts_at_its_start_gate() {
    let placed = pace();
    let mut d = Drive::at(50.0, 0.0);
    d.run(&placed, 30.0, 10.0);
    // Back before the start (a teleport would void; drive round instead: put it behind
    // without a jump by stepping the position in small moves).
    for _ in 0..200 {
        d.f.vel = Vec3::ZERO;
        d.f.pos.z -= 1.0;
        d.step(&placed);
    }
    let starts = |d: &Drive| d.out.iter().filter(|o| matches!(o, Outcome::Started { .. })).count();
    assert_eq!(starts(&d), 1);
    d.run(&placed, 100.0, 45.0);
    assert_eq!(starts(&d), 2);
    let v = d.finished();
    assert_eq!(v.len(), 1);
    assert!((v[0] - 100.0).abs() < 0.5, "the restart times from the second start: {v:?}");
}

// --- CH-4 Slide Section ---

#[test]
fn slide_section_counts_raw_slide_points_and_a_hit_voids_it() {
    let placed = vec![straight(def(8, ChallengeKind::SlideSection, vec![100.0, 400.0], [3000.0, 6000.0, 10000.0]))];
    let mut d = Drive::at(50.0, 0.0);
    d.f.slide_meter = 500.0;
    d.run(&placed, 60.0, 5.0);
    d.f.slide_meter = 4500.0;
    d.run(&placed, 60.0, 20.0);
    assert_eq!(d.finished(), vec![4000.0]);
    // A damaging hit inside.
    let mut d = Drive::at(50.0, 0.0);
    d.run(&placed, 60.0, 5.0);
    d.f.hits += 1;
    d.run(&placed, 60.0, 20.0);
    assert!(d.finished().is_empty());
    assert_eq!(d.voids(), vec!["Hit something"]);
}

// --- CH-5 Yump Board ---

/// Takes off `from` m past the crest at 25 m/s, lands after `air` s.
fn yump(from: f32, air: f32, land: impl FnOnce(&mut ChallengeFrame)) -> Drive {
    let placed = vec![straight(def(11, ChallengeKind::YumpBoard, vec![300.0], [14.0, 22.0, 30.0]))];
    let mut d = Drive::at(300.0 + from - 50.0, 0.0);
    d.run(&placed, 90.0, 2.0);
    d.f.contacts = 0;
    d.run(&placed, 90.0, air);
    d.f.contacts = 4;
    land(&mut d.f);
    d.run(&placed, 90.0, 1.0);
    d
}

#[test]
fn yump_measures_take_off_to_touchdown() {
    let d = yump(0.0, 0.8, |_| {});
    let v = d.finished();
    assert_eq!(v.len(), 1, "{:?}", d.out);
    // 0.8 s at 25 m/s = 20 m (± one step's travel, 0.42 m, at either end).
    assert!((v[0] - 20.0).abs() <= 0.5, "{v:?}");
    // Far from the crest: not an attempt.
    let d = yump(12.0, 0.8, |_| {});
    assert!(d.finished().is_empty() && d.voids().is_empty());
    // A bad landing voids it.
    let d = yump(0.0, 0.8, |f| f.hits += 1);
    assert!(d.finished().is_empty());
    assert_eq!(d.voids(), vec!["Bad landing"]);
    let d = yump(0.0, 0.8, |f| f.up_y = 0.5);
    assert_eq!(d.voids(), vec!["Bad landing"]);
}

#[test]
fn yump_distance_is_within_a_fifth_of_a_metre() {
    // Take-off and touchdown exactly on steps: the measure matches the flight.
    let placed = vec![straight(def(11, ChallengeKind::YumpBoard, vec![300.0], [14.0, 22.0, 30.0]))];
    let mut d = Drive::at(290.0, 0.0);
    d.f.vel = Vec3::Z * 20.0;
    for _ in 0..30 {
        d.step(&placed);
    }
    let take_off = d.f.pos;
    d.f.contacts = 0;
    for _ in 0..42 {
        d.step(&placed);
    }
    d.f.contacts = 4;
    for _ in 0..60 {
        d.step(&placed);
    }
    let flight = (42.0 + 1.0) * DT * 20.0;
    let v = d.finished();
    assert_eq!(v.len(), 1);
    assert!((v[0] - flight).abs() <= 0.2, "{v:?} vs {flight} (take-off {take_off})");
}

// --- CH-7 rewards ---

#[test]
fn each_star_pays_once_and_a_best_pays_50() {
    let d = def(3, ChallengeKind::RadarPost, vec![0.0], [115.0, 140.0, 160.0]);
    let mut b = Bests::default();
    let r = reward(&mut b, &d, 120.0);
    assert_eq!((r.result.stars, r.notes, r.result.new_best), (1, 400, true));
    let r = reward(&mut b, &d, 118.0);
    assert_eq!((r.result.stars, r.notes, r.result.new_best), (1, 0, false));
    let r = reward(&mut b, &d, 125.0);
    assert_eq!((r.notes, r.result.new_best), (50, true));
    let r = reward(&mut b, &d, 170.0);
    assert_eq!((r.result.stars, r.notes), (3, 600 + 1000));
    let r = reward(&mut b, &d, 165.0);
    assert_eq!(r.notes, 0);
    assert_eq!(b.get(3).unwrap().stars, 3);
    assert_eq!(b.get(3).unwrap().value, 170.0);
    assert_eq!(b.total_stars(), 3);
    // A first run with no star is still a best.
    let mut b = Bests::default();
    assert_eq!(reward(&mut b, &d, 60.0).notes, 50);
    // All stars on every challenge: 2,000 each.
    assert_eq!(STAR_NOTES.iter().sum::<u32>(), 2_000);
}

#[test]
fn stars_by_threshold() {
    let t = [14.0, 22.0, 30.0];
    assert_eq!(stars(13.9, &t), 0);
    assert_eq!(stars(14.0, &t), 1);
    assert_eq!(stars(29.0, &t), 2);
    assert_eq!(stars(31.0, &t), 3);
}

// --- The file on the committed level (DECISIONS C3) ---

fn committed() -> (Vec<ChallengeDef>, super::super::OpenWorld) {
    let l = super::super::level::load(&super::super::level::source()).expect("the committed level loads");
    let o = super::super::OpenWorld::from_data_with(l.data, Some(l.heights)).expect("the committed level builds");
    let defs = load(&file_path()).expect("challenges.ron parses");
    (defs, o)
}

#[test]
fn the_13_challenges_sit_on_their_roads() {
    let (defs, o) = committed();
    assert_eq!(defs.len(), 13);
    let count = |k| defs.iter().filter(|d| d.kind == k).count();
    assert_eq!([count(ChallengeKind::RadarPost), count(ChallengeKind::PaceSection), count(ChallengeKind::SlideSection), count(ChallengeKind::YumpBoard)], [4, 3, 3, 3]);
    let bad = check(&defs, &o.net, 3.0);
    assert!(bad.is_empty(), "off their roads (re-anchor after a re-bake: `PACENOTE_CHALLENGES=reanchor cargo test --lib write_challenges -- --ignored`):\n{}", bad.join("\n"));
    let placed = place(&defs, &o.net);
    assert_eq!(placed.len(), 13, "in order");
    assert!(placed.iter().map(|p| p.def.id).collect::<Vec<_>>().windows(2).all(|w| w[0] < w[1]));
    // Same-kind spacing ≥ 500 m (GAME_SYSTEMS §3).
    for a in &placed {
        for b in &placed {
            if a.def.id < b.def.id && a.def.kind == b.def.kind {
                let d = a.first().p.distance(b.first().p);
                assert!(d >= 500.0, "{} and {} are {d:.0} m apart", a.def.name, b.def.name);
            }
        }
    }
    // Sections are long enough (Pace 1.2-2.5 km, Slide 300-800 m).
    for p in &placed {
        match p.def.kind {
            ChallengeKind::PaceSection => assert!((1200.0..=2500.0).contains(&p.length), "{}: {}", p.def.name, p.length),
            ChallengeKind::SlideSection => assert!((300.0..=800.0).contains(&p.length), "{}: {}", p.def.name, p.length),
            _ => {}
        }
    }
}

#[test]
fn reanchoring_finds_the_stations_again() {
    let (defs, o) = committed();
    let mut moved = defs.clone();
    for d in &mut moved {
        for s in &mut d.gates {
            *s += 40.0;
        }
        d.road = 99;
    }
    let lost = reanchor(&mut moved, &o.net);
    assert!(lost.is_empty(), "{lost:?}");
    for (a, b) in defs.iter().zip(&moved) {
        assert_eq!(a.road, b.road, "{}", a.name);
        for (x, y) in a.gates.iter().zip(&b.gates) {
            assert!((x - y).abs() <= 2.0, "{}: {x} vs {y}", a.name);
        }
    }
}

/// The tool: rewrites `challenges.ron` from the committed level.
/// - `PACENOTE_CHALLENGES=fill`: the x,z checks from the stations (after editing stations);
/// - `PACENOTE_CHALLENGES=reanchor`: the roads and stations from the x,z (after A's re-bake).
#[test]
#[ignore]
fn write_challenges() {
    let (mut defs, o) = committed();
    match std::env::var("PACENOTE_CHALLENGES").as_deref() {
        Ok("fill") => fill_positions(&mut defs, &o.net),
        Ok("reanchor") => {
            let lost = reanchor(&mut defs, &o.net);
            assert!(lost.is_empty(), "no road near {lost:?}");
            fill_positions(&mut defs, &o.net);
        }
        _ => panic!("set PACENOTE_CHALLENGES=fill or reanchor"),
    }
    for d in &defs {
        eprintln!("{:>2} {:<13} {:<22} road {} gates {:?} at {:?} to {:?}", d.id, d.kind.label(), d.name, d.road, d.gates, d.at, d.to);
    }
    std::fs::write(file_path(), to_ron(&defs)).unwrap();
}

