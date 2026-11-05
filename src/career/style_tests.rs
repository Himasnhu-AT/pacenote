//! Style scoring tests: synthetic car states, one fixed step at a time (GAME_SYSTEMS SK-1..SK-6).

use super::*;
use crate::career::racers::{OtherCar, OtherKind};

const DT: f32 = 1.0 / 60.0;

/// A car on flat ground, driving along -Z, that the test steers by setting its state.
struct Sim {
    d: Detector,
    pos: Vec3,
    /// Forward speed (km/h) and slip angle (degrees).
    kmh: f32,
    beta: f32,
    contacts: u8,
    loose: bool,
    hits: u32,
    impacts: f32,
    broken: usize,
    body_contact: bool,
    up: Vec3,
    live: bool,
    road: Option<RoadInfo>,
    others: Vec<OtherCar>,
    progress: Option<f32>,
    rivals: Vec<(u32, f32)>,
    split: bool,
    in_event: bool,
    events: Vec<StyleEvent>,
    t: f32,
}

const FWD: Vec3 = Vec3::NEG_Z;
/// Left of a car facing -Z with up +Y.
const LEFT: Vec3 = Vec3::NEG_X;

impl Sim {
    fn new() -> Sim {
        Sim {
            d: Detector::default(),
            pos: Vec3::ZERO,
            kmh: 0.0,
            beta: 0.0,
            contacts: 4,
            loose: false,
            hits: 0,
            impacts: 0.0,
            broken: 0,
            body_contact: false,
            up: Vec3::Y,
            live: true,
            road: None,
            others: Vec::new(),
            progress: None,
            rivals: Vec::new(),
            split: false,
            in_event: false,
            events: Vec::new(),
            t: 0.0,
        }
    }

    fn vel(&self) -> Vec3 {
        // `kmh` is the forward speed; past 80° of slip (a spin) it is the whole speed.
        let b = self.beta.to_radians();
        let v = if self.beta.abs() < 80.0 { self.kmh / 3.6 / b.cos() } else { self.kmh / 3.6 };
        (FWD * b.cos() + LEFT * b.sin()) * v
    }

    fn step(&mut self) {
        self.pos += self.vel() * DT;
        for o in &mut self.others {
            o.pos += o.fwd * o.speed * DT;
        }
        if let Some(p) = self.progress.as_mut() {
            *p += self.kmh / 3.6 * DT;
        }
        let f = StyleFrame {
            dt: DT,
            live: self.live,
            pos: self.pos,
            vel: self.vel(),
            fwd: FWD,
            up: self.up,
            contacts: self.contacts,
            loose: self.loose,
            hits: self.hits,
            impacts: self.impacts,
            broken: self.broken,
            body_contact: self.body_contact,
            road: self.road,
            others: &self.others,
            progress: self.progress,
            rivals: &self.rivals,
            split: self.split,
            in_event: self.in_event,
        };
        self.d.step(&f, &mut self.events);
        self.split = false;
        self.t += DT;
    }

    fn run(&mut self, secs: f32) {
        let n = (secs / DT).round() as usize;
        for _ in 0..n {
            self.step();
        }
    }

    fn ticks(&self, kind: StyleKind) -> Vec<u32> {
        self.events
            .iter()
            .filter_map(|e| match e {
                StyleEvent::Tick { kind: k, points, .. } if *k == kind => Some(*points),
                _ => None,
            })
            .collect()
    }

    fn banks(&self) -> Vec<(u32, u32)> {
        self.events
            .iter()
            .filter_map(|e| match e {
                StyleEvent::Bank { points, notes, .. } => Some((*points, *notes)),
                _ => None,
            })
            .collect()
    }

    fn lost(&self) -> Vec<(u32, LostReason)> {
        self.events
            .iter()
            .filter_map(|e| match e {
                StyleEvent::Lost { points, why } => Some((*points, *why)),
                _ => None,
            })
            .collect()
    }
}

fn car(id: u32, pos: Vec3, fwd: Vec3, kmh: f32) -> OtherCar {
    OtherCar { id, kind: OtherKind::Traffic, pos, fwd, speed: kmh / 3.6 }
}

// --- SK-1 core ---

#[test]
fn same_frames_give_the_same_events() {
    let play = || {
        let mut s = Sim::new();
        s.kmh = 60.0;
        s.run(0.5);
        s.beta = 30.0;
        s.run(2.0);
        s.beta = -25.0;
        s.run(0.8);
        s.beta = 0.0;
        s.run(5.0);
        s.events
    };
    let a = play();
    assert!(!a.is_empty());
    assert_eq!(a, play());
}

#[test]
fn nothing_scores_while_not_live_and_the_timeout_holds() {
    let mut s = Sim::new();
    s.kmh = 60.0;
    s.live = false;
    s.beta = 30.0;
    s.run(3.0);
    s.beta = 0.0;
    s.run(1.0);
    assert!(s.events.is_empty(), "{:?}", s.events);
    // A paused Rhythm doesn't time out.
    s.live = true;
    s.d.inject(StyleKind::Air, 300, &mut s.events);
    s.live = false;
    s.run(10.0);
    assert!(s.banks().is_empty());
    assert!(s.d.rhythm.open);
    s.live = true;
    s.run(3.6);
    assert_eq!(s.banks().len(), 1);
}

// --- SK-2 Slide + Switch ---

#[test]
fn a_30_degree_slide_at_60_for_2_s_scores_192() {
    let mut s = Sim::new();
    s.kmh = 60.0;
    s.run(0.5);
    s.beta = 30.0;
    s.run(2.0);
    // Running: its points are live, not yet in P.
    assert!((185..=199).contains(&s.d.rhythm.live[LIVE_SLIDE]), "{:?}", s.d.rhythm);
    s.beta = 0.0;
    s.run(1.0);
    let t = s.ticks(StyleKind::Slide);
    assert_eq!(t.len(), 1, "{:?}", s.events);
    assert!((187..=197).contains(&t[0]), "{}", t[0]);
    assert_eq!(s.d.rhythm.live[LIVE_SLIDE], 0);
    assert!((s.d.rhythm.slide_meter - t[0] as f32).abs() < 0.5);
}

#[test]
fn a_14_degree_slip_never_starts_a_slide() {
    let mut s = Sim::new();
    s.kmh = 80.0;
    s.beta = 14.0;
    s.run(5.0);
    s.beta = 0.0;
    s.run(5.0);
    assert!(s.events.is_empty(), "{:?}", s.events);
}

#[test]
fn a_slide_needs_the_quarter_second_hold_and_two_wheels() {
    let mut s = Sim::new();
    s.kmh = 60.0;
    s.beta = 30.0;
    s.run(0.2);
    s.beta = 0.0;
    s.run(1.0);
    assert!(s.ticks(StyleKind::Slide).is_empty());
    s.contacts = 1;
    s.beta = 30.0;
    s.run(1.0);
    s.beta = 0.0;
    s.contacts = 4;
    s.run(1.0);
    assert!(s.ticks(StyleKind::Slide).is_empty());
}

#[test]
fn a_flick_inside_a_slide_scores_a_switch() {
    let mut s = Sim::new();
    s.kmh = 60.0;
    s.beta = 25.0;
    s.run(1.0);
    s.beta = 0.0;
    s.run(0.2);
    s.beta = -25.0;
    s.run(1.0);
    s.beta = 0.0;
    s.run(1.0);
    assert_eq!(s.ticks(StyleKind::Switch), vec![150]);
    // Two slides around the flick at most, but one Switch.
    assert!(!s.ticks(StyleKind::Slide).is_empty());
    // A slow change of side (more than 1 s straight between) is no Switch.
    let mut s = Sim::new();
    s.kmh = 60.0;
    s.beta = 25.0;
    s.run(1.0);
    s.beta = 0.0;
    s.run(1.5);
    s.beta = -25.0;
    s.run(1.0);
    s.beta = 0.0;
    s.run(1.0);
    assert!(s.ticks(StyleKind::Switch).is_empty());
}

#[test]
fn a_spin_voids_the_slide_but_keeps_the_rhythm() {
    let mut s = Sim::new();
    s.kmh = 60.0;
    s.d.inject(StyleKind::Air, 400, &mut s.events);
    s.beta = 30.0;
    s.run(1.5);
    s.beta = 110.0;
    s.kmh = 30.0;
    s.run(0.2);
    s.beta = 0.0;
    s.run(0.5);
    assert!(s.ticks(StyleKind::Slide).is_empty(), "{:?}", s.events);
    assert!(s.lost().is_empty());
    assert_eq!(s.d.rhythm.points, 400);
    assert!(s.d.rhythm.slide_meter.abs() < 0.5, "{}", s.d.rhythm.slide_meter);
}

// --- SK-3 Air ---

/// Up for `secs` at `mps` horizontal, then down; returns the sim after the landing window.
fn jump(secs: f32, mps: f32, land: impl FnOnce(&mut Sim)) -> Sim {
    let mut s = Sim::new();
    s.kmh = mps * 3.6;
    s.run(0.5);
    s.contacts = 0;
    s.run(secs);
    s.contacts = 4;
    land(&mut s);
    s.run(1.0);
    s
}

#[test]
fn air_scores_time_and_distance_at_a_clean_landing() {
    let s = jump(0.8, 25.0, |_| {});
    let t = s.ticks(StyleKind::Air);
    assert_eq!(t.len(), 1, "{:?}", s.events);
    // 250 × 0.8 + 6 × 20 = 320 (the take-off point is one step's worth uncertain: ±3).
    assert!((317..=323).contains(&t[0]), "{}", t[0]);
    // Big Air from 25 m.
    let big = jump(1.2, 25.0, |_| {});
    assert!(big.events.iter().any(|e| matches!(e, StyleEvent::Tick { kind: StyleKind::Air, tag: StyleTag::BigAir, .. })));
    assert!(!s.events.iter().any(|e| matches!(e, StyleEvent::Tick { tag: StyleTag::BigAir, .. })));
}

#[test]
fn a_hop_under_0_3_s_is_not_air() {
    let s = jump(0.29, 25.0, |_| {});
    assert!(s.ticks(StyleKind::Air).is_empty());
    assert!(!s.d.rhythm.open);
}

#[test]
fn a_crash_landing_voids_the_air_and_breaks_the_rhythm() {
    let mut s = jump(0.8, 25.0, |s| {
        s.d.inject(StyleKind::Slide, 500, &mut s.events);
        s.step();
        s.hits += 1;
    });
    assert!(s.ticks(StyleKind::Air).is_empty(), "{:?}", s.events);
    assert_eq!(s.lost(), vec![(500, LostReason::Hit)]);
    // A roof landing: no Air either.
    s = jump(0.8, 25.0, |s| s.up = Vec3::new(0.0, 0.5, 0.86).normalize());
    assert!(s.ticks(StyleKind::Air).is_empty());
    // Body on the ground: no Air.
    s = jump(0.8, 25.0, |s| s.body_contact = true);
    assert!(s.ticks(StyleKind::Air).is_empty());
}

// --- SK-4 Close Shave + Slipstream ---

/// Drives past a still car at `gap` m body gap and `kmh` (the closing speed).
fn pass(gap: f32, kmh: f32, oncoming: bool) -> Sim {
    let mut s = Sim::new();
    let (mine, theirs) = if oncoming { (kmh / 2.0, kmh / 2.0) } else { (kmh, 0.0) };
    s.kmh = mine;
    let fwd = if oncoming { -FWD } else { FWD };
    // 15 m ahead, `gap` + 1.8 to the left.
    s.others.push(car(7, FWD * 15.0 + LEFT * (gap + 1.8), fwd, theirs));
    s.run(4.0);
    s
}

#[test]
fn close_shaves_by_gap_and_direction() {
    assert_eq!(pass(0.5, 40.0, false).ticks(StyleKind::CloseShave), vec![250]);
    assert_eq!(pass(0.5, 40.0, true).ticks(StyleKind::CloseShave), vec![375]);
    assert_eq!(pass(1.0, 40.0, false).ticks(StyleKind::CloseShave), vec![120]);
    assert!(pass(1.3, 40.0, false).ticks(StyleKind::CloseShave).is_empty());
    // Too slow: 20 km/h closing.
    assert!(pass(0.5, 20.0, false).ticks(StyleKind::CloseShave).is_empty());
    // Overlapping bodies (a car driven through, e.g. non-solid dev traffic): no shave.
    assert!(pass(-0.8, 40.0, false).ticks(StyleKind::CloseShave).is_empty());
    let s = pass(0.5, 40.0, false);
    assert!(s.events.iter().any(|e| matches!(e, StyleEvent::Tick { tag: StyleTag::PaintScraper, .. })));
}

#[test]
fn the_same_car_shaves_once_in_5_s() {
    let mut s = Sim::new();
    s.kmh = 40.0;
    s.others.push(car(3, FWD * 10.0 + LEFT * 2.3, FWD, 0.0));
    s.run(2.0);
    // Put it ahead again 2 s later: no second shave.
    s.others[0].pos = s.pos + FWD * 10.0 + LEFT * 2.3;
    s.run(2.0);
    assert_eq!(s.ticks(StyleKind::CloseShave).len(), 1);
    // After 5 s it counts again.
    s.run(2.0);
    s.others[0].pos = s.pos + FWD * 10.0 + LEFT * 2.3;
    s.run(2.0);
    assert_eq!(s.ticks(StyleKind::CloseShave).len(), 2);
}

#[test]
fn a_shave_that_touches_breaks_instead() {
    let mut s = Sim::new();
    s.kmh = 40.0;
    s.d.inject(StyleKind::Air, 300, &mut s.events);
    s.others.push(car(3, FWD * 5.0 + LEFT * 2.3, FWD, 0.0));
    // Past it, then a knock with it beside.
    s.run(0.6);
    s.others[0].pos = s.pos + LEFT * 2.2;
    s.impacts += 2.0;
    s.step();
    s.run(2.0);
    assert!(s.ticks(StyleKind::CloseShave).is_empty());
    assert_eq!(s.lost(), vec![(300, LostReason::Contact)]);
}

#[test]
fn slipstream_scores_after_a_one_second_hold() {
    let mut s = Sim::new();
    s.kmh = 80.0;
    s.others.push(car(9, FWD * 10.0, FWD, 80.0));
    s.run(3.0);
    assert!((48..=52).contains(&s.d.rhythm.live[LIVE_DRAFT]), "{:?}", s.d.rhythm);
    s.others.clear();
    s.run(1.0);
    let t = s.ticks(StyleKind::Slipstream);
    assert_eq!(t.len(), 1);
    assert!((48..=52).contains(&t[0]), "{}", t[0]);
    // Too far behind: nothing.
    let mut s = Sim::new();
    s.kmh = 80.0;
    s.others.push(car(9, FWD * 25.0, FWD, 80.0));
    s.run(3.0);
    s.others.clear();
    s.run(1.0);
    assert!(s.ticks(StyleKind::Slipstream).is_empty());
}

// --- SK-5 Splinter, Switchback, Clean Line, Clean Pass, Flat Out ---

#[test]
fn splinters_count_twelve_per_rhythm() {
    let mut s = Sim::new();
    s.kmh = 90.0;
    s.step();
    for _ in 0..13 {
        s.broken += 1;
        s.impacts += 0.5;
        s.step();
    }
    let t = s.ticks(StyleKind::Splinter);
    assert_eq!(t.len(), 12);
    assert!(t.iter().all(|&p| p == 80));
    // Not a scuff.
    assert_eq!(s.d.rhythm.scuffs, 0);
    // Slow: 40.
    let mut s = Sim::new();
    s.kmh = 50.0;
    s.step();
    s.broken += 1;
    s.step();
    assert_eq!(s.ticks(StyleKind::Splinter), vec![40]);
}

#[test]
fn flat_out_at_165_on_tarmac_for_1_s_scores_75() {
    let mut s = Sim::new();
    s.kmh = 165.0;
    s.run(1.0);
    s.kmh = 100.0;
    s.run(1.0);
    let t = s.ticks(StyleKind::FlatOut);
    assert_eq!(t.len(), 1, "{:?}", s.events);
    // 1 s at 165 = 75, plus the 0.5 s tail below 145: nothing.
    assert!((74..=76).contains(&t[0]), "{}", t[0]);
    // 140 on tarmac: nothing; 140 on gravel: Flat Out.
    let mut s = Sim::new();
    s.kmh = 140.0;
    s.run(2.0);
    s.kmh = 50.0;
    s.run(1.0);
    assert!(s.ticks(StyleKind::FlatOut).is_empty());
    s.loose = true;
    s.kmh = 140.0;
    s.run(2.0);
    s.kmh = 50.0;
    s.run(1.0);
    assert_eq!(s.ticks(StyleKind::FlatOut).len(), 1);
}

fn hairpin(lateral: f32, kmh: f32) -> Sim {
    let mut s = Sim::new();
    s.kmh = kmh;
    // 30 m of straight, 20 m of hairpin, then straight again (stations follow the car).
    let mut at = 0.0;
    for (len, curv) in [(30.0, 0.0), (20.0, 0.1), (30.0, 0.0)] {
        let end = at + len;
        while at < end {
            s.road = Some(RoadInfo { road: 2, s: at, curv, half_width: 2.5, lateral });
            s.step();
            at += kmh / 3.6 * DT;
        }
    }
    s
}

#[test]
fn switchback_needs_the_road_and_30_kmh() {
    let t = hairpin(0.0, 50.0).ticks(StyleKind::Switchback);
    assert_eq!(t, vec![400]);
    assert!(hairpin(3.5, 50.0).ticks(StyleKind::Switchback).is_empty(), "off the road");
    assert!(hairpin(0.0, 25.0).ticks(StyleKind::Switchback).is_empty(), "too slow");
}

#[test]
fn clean_line_every_km_on_the_road() {
    let mut s = Sim::new();
    s.kmh = 100.0;
    s.road = Some(RoadInfo { road: 0, s: 0.0, curv: 0.0, half_width: 3.0, lateral: 0.0 });
    s.run(37.0);
    assert_eq!(s.ticks(StyleKind::CleanLine), vec![300]);
    // Off the road for a third of it: nothing.
    let mut s = Sim::new();
    s.kmh = 100.0;
    s.road = Some(RoadInfo { road: 0, s: 0.0, curv: 0.0, half_width: 3.0, lateral: 0.0 });
    s.run(20.0);
    s.road.as_mut().unwrap().lateral = 6.0;
    s.run(17.0);
    assert!(s.ticks(StyleKind::CleanLine).is_empty());
    // A hit starts the kilometre over.
    let mut s = Sim::new();
    s.kmh = 100.0;
    s.road = Some(RoadInfo { road: 0, s: 0.0, curv: 0.0, half_width: 3.0, lateral: 0.0 });
    s.run(30.0);
    s.hits += 1;
    s.run(10.0);
    assert!(s.ticks(StyleKind::CleanLine).is_empty());
}

#[test]
fn clean_line_at_event_splits() {
    let mut s = Sim::new();
    s.kmh = 100.0;
    s.in_event = true;
    s.road = Some(RoadInfo { road: 0, s: 0.0, curv: 0.0, half_width: 3.0, lateral: 0.0 });
    s.run(50.0);
    assert!(s.ticks(StyleKind::CleanLine).is_empty(), "no km rule in events");
    s.split = true;
    s.step();
    assert_eq!(s.ticks(StyleKind::CleanLine), vec![300]);
}

#[test]
fn clean_pass_holds_ahead_three_seconds_without_contact() {
    let mut s = Sim::new();
    s.kmh = 100.0;
    s.progress = Some(0.0);
    // The rival is 20 m ahead and slower.
    s.rivals = vec![(1, 20.0)];
    for _ in 0..(8.0 / DT) as usize {
        s.rivals[0].1 += 80.0 / 3.6 * DT;
        s.step();
    }
    assert_eq!(s.ticks(StyleKind::CleanPass), vec![300]);
    // Drop back behind before 3 s: no pass.
    let mut s = Sim::new();
    s.kmh = 100.0;
    s.progress = Some(0.0);
    s.rivals = vec![(1, 20.0)];
    for k in 0..(8.0 / DT) as usize {
        let rival = if k < 300 { 80.0 } else { 140.0 };
        s.rivals[0].1 += rival / 3.6 * DT;
        s.step();
    }
    assert!(s.ticks(StyleKind::CleanPass).is_empty());
}

// --- SK-6 Rhythm ---

#[test]
fn multiplier_steps_variety_and_cap() {
    let mut d = Detector::default();
    let mut out = Vec::new();
    d.inject(StyleKind::Air, 100, &mut out);
    assert!((d.rhythm.mult - 1.2).abs() < 1e-4);
    d.inject(StyleKind::Air, 100, &mut out);
    assert!((d.rhythm.mult - 1.4).abs() < 1e-4, "same kind: no variety");
    d.inject(StyleKind::Slide, 100, &mut out);
    assert!((d.rhythm.mult - 1.7).abs() < 1e-4, "different kind: +0.1");
    for k in 0..40 {
        d.inject(StyleKind::ALL[k % 4], 10, &mut out);
    }
    assert_eq!(d.rhythm.mult, MULT_CAP);
    assert_eq!(d.rhythm.chain, 43);
}

#[test]
fn a_rhythm_banks_after_3_5_s_and_pays_style_over_25() {
    let mut s = Sim::new();
    s.kmh = 50.0;
    s.step();
    s.d.inject(StyleKind::Air, 1000, &mut s.events);
    s.run(3.4);
    assert!(s.banks().is_empty());
    s.run(0.2);
    // 1000 × 1.2 = 1200 Style, 48 Notes.
    assert_eq!(s.banks(), vec![(1200, 48)]);
    assert!(!s.d.rhythm.open);
    assert_eq!(notes_for(100_000), NOTES_CAP);
    assert_eq!(notes_for(49), 1);
}

#[test]
fn a_running_slide_keeps_the_rhythm_alive() {
    let mut s = Sim::new();
    s.kmh = 60.0;
    s.step();
    s.d.inject(StyleKind::Air, 100, &mut s.events);
    s.beta = 30.0;
    s.run(6.0);
    assert!(s.banks().is_empty());
    // 6 s of slide: +0.2 three times on top of the Air's 1.2.
    assert!((s.d.rhythm.mult - 1.8).abs() < 1e-3, "{}", s.d.rhythm.mult);
    s.beta = 0.0;
    s.run(4.0);
    assert_eq!(s.banks().len(), 1);
}

#[test]
fn a_wall_brush_halves_the_multiplier() {
    let mut s = Sim::new();
    s.kmh = 50.0;
    s.step();
    for _ in 0..10 {
        s.d.inject(StyleKind::Air, 10, &mut s.events);
    }
    assert!((s.d.rhythm.mult - 3.0).abs() < 1e-4);
    s.impacts += 1.0;
    s.step();
    assert!((s.d.rhythm.mult - 1.5).abs() < 1e-4);
    assert_eq!(s.d.rhythm.points, 100, "P kept");
    // A scrape over a few steps is one scuff.
    s.impacts += 0.5;
    s.step();
    assert!((s.d.rhythm.mult - 1.5).abs() < 1e-4);
    assert!(s.lost().is_empty());
}

#[test]
fn breaks_lose_the_points() {
    // Recover (the car jumps back onto the road).
    let mut s = Sim::new();
    s.kmh = 60.0;
    s.step();
    s.d.inject(StyleKind::Air, 300, &mut s.events);
    s.beta = 30.0;
    s.run(1.0);
    s.pos += Vec3::new(30.0, 0.0, 0.0);
    s.step();
    let lost = s.lost();
    assert_eq!(lost.len(), 1);
    assert_eq!(lost[0].1, LostReason::Recover);
    assert!(lost[0].0 > 300, "the running slide is lost too: {}", lost[0].0);
    assert!(!s.d.rhythm.open);
    // Rolled.
    let mut s = Sim::new();
    s.kmh = 30.0;
    s.step();
    s.d.inject(StyleKind::Air, 300, &mut s.events);
    s.up = Vec3::new(1.0, 0.1, 0.0).normalize();
    s.step();
    assert_eq!(s.lost(), vec![(300, LostReason::Rolled)]);
    // A damaging hit.
    let mut s = Sim::new();
    s.kmh = 30.0;
    s.step();
    s.d.inject(StyleKind::Air, 300, &mut s.events);
    s.hits += 1;
    s.impacts += 9.0;
    s.step();
    assert_eq!(s.lost(), vec![(300, LostReason::Hit)]);
    // Nothing open: nothing lost.
    s.hits += 1;
    s.step();
    assert_eq!(s.lost().len(), 1);
}

#[test]
fn forced_bank_completes_running_skills() {
    let mut s = Sim::new();
    s.kmh = 60.0;
    s.beta = 30.0;
    s.run(2.0);
    s.d.bank(&mut s.events);
    let b = s.banks();
    assert_eq!(b.len(), 1);
    assert_eq!(s.ticks(StyleKind::Slide).len(), 1);
    assert!(!s.d.rhythm.open);
}

#[test]
fn a_challenge_scores_500_a_star() {
    let mut d = Detector::default();
    let mut out = Vec::new();
    d.challenge(2, &mut out);
    d.challenge(0, &mut out);
    assert_eq!(out, vec![StyleEvent::Tick { kind: StyleKind::Challenge, points: 1000, chain: 1, tag: StyleTag::None }]);
}

#[test]
fn beta_sign_and_range() {
    let mut s = Sim::new();
    s.kmh = 50.0;
    s.beta = 20.0;
    let f = StyleFrame { pos: Vec3::ZERO, vel: s.vel(), fwd: FWD, up: Vec3::Y, ..StyleFrame::of(&crate::vehicle::Car::new(Vec3::ZERO, 0.0), DT) };
    assert!((f.beta() - 20.0).abs() < 0.01, "{}", f.beta());
    assert!((f.kmh() - 50.0).abs() < 0.01);
}

// --- With the real car: the dev autopilot's drift style on a gravel stage (SK-2) ---

/// Drives `secs` of stage `id` with the dev autopilot and scores every step.
fn autopilot_run(id: u32, drift: bool, secs: f32) -> (Vec<StyleEvent>, f32) {
    use crate::autopilot::Autopilot;
    use crate::vehicle::{Car, Spec, step};
    let def = crate::stage::stages().into_iter().find(|d| d.id == id).unwrap();
    let w = crate::world::generate(&def);
    let mut ap = Autopilot::default();
    (ap.on, ap.drift, ap.pace) = (true, drift, 0.6);
    let (p, h) = w.road.start();
    let spec = Spec::default();
    let mut car = Car::new(p + Vec3::Y * 0.9, h);
    let mut d = Detector::default();
    let mut out = Vec::new();
    let mut progress = 0.0f32;
    for _ in 0..(secs * 60.0) as usize {
        let input = ap.input(&car, &spec, &w, progress, DT);
        step(&mut car, &spec, input, &w, DT);
        if let Some(rp) = w.road.nearest(Vec2::new(car.pos.x, car.pos.z), 40.0)
            && rp.s > progress
            && rp.s < progress + 40.0
        {
            progress = rp.s;
        }
        let mut f = StyleFrame::of(&car, DT);
        f.road = w.road.nearest(Vec2::new(car.pos.x, car.pos.z), 30.0).map(|at| RoadInfo { road: 0, s: at.s, curv: w.road.curv[at.i], half_width: w.road.half_width(&at), lateral: at.lateral });
        f.in_event = true;
        d.step(&f, &mut out);
    }
    (out, progress)
}

#[test]
fn the_autopilot_drifting_a_gravel_stage_scores_slides() {
    // Stage 15: early gravel hairpins (the autopilot's own drift report uses it).
    let (events, progress) = autopilot_run(15, true, 40.0);
    let slides: Vec<u32> = events.iter().filter_map(|e| match e {
        StyleEvent::Tick { kind: StyleKind::Slide, points, .. } => Some(*points),
        _ => None,
    }).collect();
    let banks = events.iter().filter(|e| matches!(e, StyleEvent::Bank { .. })).count();
    eprintln!("drift: {progress:.0} m, {} slides {:?}, {banks} banks, events {}", slides.len(), slides, events.len());
    assert!(!slides.is_empty(), "no Slide in {progress:.0} m of drifting: {events:?}");
    assert!(banks >= 1);
}
