//! Roadside challenges: Radar Post, Pace Section, Slide Section, Yump Board. Drive-through
//! (no stop, no menu): the first gate starts the attempt.
//!
//! **Owner: stream F (GAMEPLAY, stream CHALLENGES).** Design: `docs/festival/GAME_SYSTEMS.md`
//! §3 and CH-1..CH-7 (rules, stars, rewards); the 13 anchors in `docs/festival/WORLD_PASS.md`
//! §5; rulings `docs/festival/DECISIONS.md` C1-C5 (13 now, the seeded placer later; stored in
//! [`CHALLENGES_FILE`] by road index + station, not in the level file A re-bakes).
//! Read-only observer of `drive::DrivenCar` after `SimulateSet`.
//!
//! Layers:
//! - data: [`ChallengeDef`] in [`CHALLENGES_FILE`] (road + stations, plus the world x,z of the
//!   first and last gate as a check, and to re-anchor after a re-bake: [`reanchor`]);
//! - geometry: [`place`] turns them into gate lines on the roads ([`Placed`], [`Gate`]);
//! - the attempt: [`Tracker::step`] is pure (a [`ChallengeFrame`] per fixed step in, [`Outcome`]s
//!   out); unit-tested with synthetic drives;
//! - results: [`stars`], [`reward`] and the player's bests ([`Bests`], `<DataDir>/`[`BESTS_FILE`]);
//! - the game: markers by the road (gate stripes, flag posts, kind boards, the radar camera that
//!   flashes), [`ChallengeResult`] / [`RadarFlash`] / [`RunCue`] / `career::Payout` messages,
//!   and dev commands `challenges.list`, `challenges.goto`, `challenges.state`.
//!
//! Starting a section (Pace or Slide) banks the Rhythm (GAME_SYSTEMS §2.3); Radar Posts and
//! Yump Boards don't (banking mid-jump would drop the Air).

use std::collections::BTreeMap;
use std::sync::Arc;

use bevy::math::Vec2;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::roads::RoadNet;

/// The challenges, under the assets dir (F's file; star thresholds baked by
/// `--bake-challenges` after A's re-bake).
pub const CHALLENGES_FILE: &str = "levels/open/challenges.ron";
/// The player's best result and stars per challenge, in the data dir.
pub const BESTS_FILE: &str = "challenges.json";

/// Notes for the first 1★, 2★, 3★ on a challenge (2,000 in all).
pub const STAR_NOTES: [u32; 3] = [400, 600, 1_000];
/// Notes for a new best with no new star.
pub const BEST_NOTES: u32 = 50;
/// Hidden mid-gates in a Pace Section.
pub const PACE_MIDS: usize = 4;
/// A Pace Section mid-gate counts within this many metres of the centreline.
const MID_REACH: f32 = 15.0;
/// Gates (start, end, radar) count within the half width plus this.
const GATE_SLACK: f32 = 1.0;
/// A Yump take-off counts within this many metres of the crest (along the road).
const CREST_REACH: f32 = 8.0;
/// Shorter hops over a Yump crest aren't attempts.
const YUMP_MIN_AIR: f32 = 0.15;
const LANDING_WINDOW: f32 = 0.6;
/// A section is abandoned after this long off its road (another road nearest, or more than
/// [`OFF_COURSE`] m from it), this long reversing, or below [`CRAWL_KMH`] on average.
const OFF_COURSE: f32 = 40.0;
const OFF_SECS: f32 = 3.0;
const REVERSE_SECS: f32 = 1.0;
const CRAWL_KMH: f32 = 15.0;
/// Approach cards: within this many metres, ahead within ±45°.
pub const APPROACH_M: f32 = 200.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ChallengeKind {
    /// Speed at the gate line (km/h).
    RadarPost,
    /// Mean speed between the start and end gates (km/h).
    PaceSection,
    /// Raw Slide + Switch points between the gates.
    SlideSection,
    /// Horizontal distance from take-off to touchdown (m).
    YumpBoard,
}

impl ChallengeKind {
    pub const ALL: [ChallengeKind; 4] = [ChallengeKind::RadarPost, ChallengeKind::PaceSection, ChallengeKind::SlideSection, ChallengeKind::YumpBoard];

    pub fn label(self) -> &'static str {
        match self {
            ChallengeKind::RadarPost => "Radar Post",
            ChallengeKind::PaceSection => "Pace Section",
            ChallengeKind::SlideSection => "Slide Section",
            ChallengeKind::YumpBoard => "Yump Board",
        }
    }

    /// The value's unit as the HUD writes it.
    pub fn unit(self) -> &'static str {
        match self {
            ChallengeKind::RadarPost | ChallengeKind::PaceSection => "km/h",
            ChallengeKind::SlideSection => "pts",
            ChallengeKind::YumpBoard => "m",
        }
    }

    /// A section: start and end gates, timed between.
    pub fn section(self) -> bool {
        matches!(self, ChallengeKind::PaceSection | ChallengeKind::SlideSection)
    }

    /// The kind's colour (boards, flags, HUD chip; always with its name or glyph).
    pub fn color(self) -> Color {
        match self {
            ChallengeKind::RadarPost => Color::srgb(0.93, 0.25, 0.2),
            ChallengeKind::PaceSection => Color::srgb(0.25, 0.6, 0.95),
            ChallengeKind::SlideSection => Color::srgb(0.95, 0.55, 0.12),
            ChallengeKind::YumpBoard => Color::srgb(0.98, 0.8, 0.1),
        }
    }
}

/// One challenge as stored in [`CHALLENGES_FILE`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChallengeDef {
    /// Stable id: saves and payouts key by it.
    pub id: u32,
    pub kind: ChallengeKind,
    /// Its name on the approach card ("Mill Straight").
    #[serde(default)]
    pub name: String,
    /// Road index in the `RoadNet` (stable through the re-bake, DECISIONS W1).
    pub road: u32,
    /// Gate stations (m along the road): one for a Radar Post / Yump crest, start and end for
    /// sections (a Pace Section's hidden mid-gates are spread evenly between them on load).
    /// Sections run towards increasing stations.
    pub gates: Vec<f32>,
    /// 1★, 2★, 3★ thresholds.
    pub thresholds: [f32; 3],
    /// The bake car's reference value R.
    pub reference: f32,
    /// World x,z of the first gate, as a check against the road (a test fails if they differ).
    pub at: Vec2,
    /// World x,z of the last gate of a section (the same check; re-anchoring uses both).
    #[serde(default)]
    pub to: Option<Vec2>,
}

/// A challenge attempt finished. Economy (stars pay once), style (the Challenge skill), HUD.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct ChallengeResult {
    pub id: u32,
    pub value: f32,
    /// Stars earned on this run, 0..=3.
    pub stars: u8,
    pub new_best: bool,
}

/// The car crossed a Radar Post gate: the flash and readout sounds (AUDIO_PASS §3; was
/// `SpeedTrap`).
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct RadarFlash {
    pub pos: Vec3,
    pub kmh: f32,
    pub target_beaten: bool,
    pub personal_best: bool,
}

/// Run cues for the festival sounds (AUDIO_PASS §3; was `ActivityCue`): event and race
/// countdowns, challenge checkpoints, finishes.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunCue {
    Countdown(u8),
    Go,
    Checkpoint,
    /// `place`: 1-based in a race, 0 when solo.
    Finish { place: u8, best: bool },
    Fail,
}

// --- Data -------------------------------------------------------------------------------

/// Where [`CHALLENGES_FILE`] is: beside the level the game plays (`assets/levels/...`).
pub fn file_path() -> std::path::PathBuf {
    let level = super::level::source();
    level.parent().map(|d| d.join("open").join("challenges.ron")).unwrap_or_else(|| CHALLENGES_FILE.into())
}

pub fn parse(text: &str) -> Result<Vec<ChallengeDef>, String> {
    let defs: Vec<ChallengeDef> = ron::from_str(text).map_err(|e| e.to_string())?;
    for d in &defs {
        let need = if d.kind.section() { 2 } else { 1 };
        if d.gates.len() != need {
            return Err(format!("challenge {}: {} gates, {need} expected", d.id, d.gates.len()));
        }
        if d.kind.section() && d.gates[1] <= d.gates[0] {
            return Err(format!("challenge {}: the end gate is before the start", d.id));
        }
        if !(d.thresholds[0] < d.thresholds[1] && d.thresholds[1] < d.thresholds[2]) {
            return Err(format!("challenge {}: thresholds must rise: {:?}", d.id, d.thresholds));
        }
    }
    let mut ids: Vec<u32> = defs.iter().map(|d| d.id).collect();
    ids.sort_unstable();
    ids.dedup();
    if ids.len() != defs.len() {
        return Err("duplicate challenge ids".into());
    }
    Ok(defs)
}

pub fn load(path: &std::path::Path) -> Result<Vec<ChallengeDef>, String> {
    parse(&std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?)
}

pub fn to_ron(defs: &[ChallengeDef]) -> String {
    let cfg = ron::ser::PrettyConfig::new().depth_limit(2).struct_names(false);
    ron::ser::to_string_pretty(&defs.to_vec(), cfg).expect("challenges serialise")
}

/// The world x,z of station `s` on a road.
fn xz_at(road: &crate::stage::Road, s: f32) -> Vec2 {
    let p = road.pts[road.station(s)];
    Vec2::new(p.x, p.z)
}

/// Sets `at` / `to` from the stations (after editing stations by hand).
pub fn fill_positions(defs: &mut [ChallengeDef], net: &RoadNet) {
    for d in defs {
        let Some(r) = net.roads.get(d.road as usize) else { continue };
        d.at = xz_at(&r.road, d.gates[0]);
        d.to = d.kind.section().then(|| xz_at(&r.road, d.gates[1]));
    }
}

/// Sets the road and stations from `at` / `to` (after a re-bake moved the roads: DECISIONS
/// C3). The road is the nearest one to `at`; a section's end is the nearest station of that
/// road to `to`. Returns the challenges it couldn't place (no road within 60 m).
pub fn reanchor(defs: &mut [ChallengeDef], net: &RoadNet) -> Vec<u32> {
    let mut lost = Vec::new();
    for d in defs {
        let Some(n) = net.nearest(d.at, 60.0) else {
            lost.push(d.id);
            continue;
        };
        d.road = n.road as u32;
        d.gates[0] = (n.at.s / 2.0).round() * 2.0;
        if let Some(to) = d.to {
            match net.roads[n.road].road.nearest(to, 60.0) {
                Some(e) => d.gates[1] = (e.s / 2.0).round() * 2.0,
                None => lost.push(d.id),
            }
        }
    }
    lost
}

/// Problems with the challenges on this network: a road that doesn't exist, a gate off its
/// road's length, or a gate more than `tol` m from where the file says it is.
pub fn check(defs: &[ChallengeDef], net: &RoadNet, tol: f32) -> Vec<String> {
    let mut bad = Vec::new();
    for d in defs {
        let Some(r) = net.roads.get(d.road as usize) else {
            bad.push(format!("{} {}: no road {}", d.id, d.name, d.road));
            continue;
        };
        for &s in &d.gates {
            if s < 0.0 || s > r.road.length() {
                bad.push(format!("{} {}: gate at {s} m off road {} ({:.0} m)", d.id, d.name, d.road, r.road.length()));
            }
        }
        let mut want = vec![(d.gates[0], d.at)];
        if let (Some(to), true) = (d.to, d.gates.len() > 1) {
            want.push((d.gates[1], to));
        }
        for (s, xz) in want {
            let got = xz_at(&r.road, s);
            if got.distance(xz) > tol {
                bad.push(format!("{} {}: gate at {s} m is {:.1} m from its x,z {xz}", d.id, d.name, got.distance(xz)));
            }
        }
    }
    bad
}

// --- Geometry ---------------------------------------------------------------------------

/// A gate line across a road.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gate {
    /// Centre on the road surface.
    pub p: Vec3,
    /// Unit direction of the road (XZ, towards increasing stations).
    pub dir: Vec2,
    pub half: f32,
    pub s: f32,
}

impl Gate {
    fn left(&self) -> Vec2 {
        Vec2::new(self.dir.y, -self.dir.x)
    }

    /// Signed distance of `p` past the line (along the road) and across it.
    pub fn local(&self, p: Vec3) -> (f32, f32) {
        let d = Vec2::new(p.x - self.p.x, p.z - self.p.z);
        (d.dot(self.dir), d.dot(self.left()))
    }

    /// Whether the move `a` → `b` crosses the line within `reach` of the centre: +1 forwards,
    /// -1 backwards, 0 not.
    pub fn crossed(&self, a: Vec3, b: Vec3, reach: f32) -> i8 {
        let (da, la) = self.local(a);
        let (db, lb) = self.local(b);
        if (da < 0.0) == (db < 0.0) {
            return 0;
        }
        let t = da / (da - db);
        if (la + (lb - la) * t).abs() > reach {
            return 0;
        }
        if db >= 0.0 { 1 } else { -1 }
    }
}

/// A challenge on the roads.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub def: ChallengeDef,
    /// Start, (Pace: the hidden mid-gates,) end; a Radar Post or Yump Board has one.
    pub gates: Vec<Gate>,
    /// Section length (m).
    pub length: f32,
}

impl Placed {
    pub fn first(&self) -> &Gate {
        &self.gates[0]
    }
}

fn gate(road: &crate::stage::Road, s: f32) -> Gate {
    let i = road.station(s);
    Gate { p: road.pts[i], dir: road.dir[i], half: road.width[i] * 0.5, s }
}

/// The challenges as gate lines on the network (those whose road exists).
pub fn place(defs: &[ChallengeDef], net: &RoadNet) -> Vec<Placed> {
    defs.iter()
        .filter_map(|d| {
            let road = &net.roads.get(d.road as usize)?.road;
            let mut gates = vec![gate(road, d.gates[0])];
            let mut length = 0.0;
            if d.kind.section() {
                let (a, b) = (d.gates[0], d.gates[1]);
                length = b - a;
                if d.kind == ChallengeKind::PaceSection {
                    for k in 1..=PACE_MIDS {
                        gates.push(gate(road, a + length * k as f32 / (PACE_MIDS + 1) as f32));
                    }
                }
                gates.push(gate(road, b));
            }
            Some(Placed { def: d.clone(), gates, length })
        })
        .collect()
}

// --- The attempt (pure) -----------------------------------------------------------------

/// One fixed step as the challenges see it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChallengeFrame {
    pub dt: f32,
    /// False in a countdown, a pause, while down or in an event (nothing starts or scores).
    pub live: bool,
    pub pos: Vec3,
    pub vel: Vec3,
    pub fwd: Vec3,
    pub up_y: f32,
    pub contacts: u8,
    pub hits: u32,
    pub body_contact: bool,
    /// [`crate::career::style::Rhythm::slide_meter`].
    pub slide_meter: f32,
    /// The nearest road (index) and the distance from its centreline, within 60 m.
    pub road: Option<(u32, f32)>,
}

/// What happened in a step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Outcome {
    /// A section's start gate (or its restart).
    Started { idx: usize },
    /// A hidden Pace gate passed (`k` from 1).
    Checkpoint { idx: usize, k: usize },
    /// A result. `pos`: where (the radar flash).
    Finished { idx: usize, value: f32, pos: Vec3 },
    Void { idx: usize, why: &'static str },
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Section {
    idx: usize,
    t: f32,
    /// The next gate to pass (index into `Placed::gates`).
    next: usize,
    meter0: f32,
    hits: u32,
    back: f32,
    off: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
enum Yump {
    #[default]
    Idle,
    Up { idx: usize, from: Vec3, t: f32, hits: u32 },
    Landing { idx: usize, dist: f32, window: f32, hits: u32 },
}

/// The attempts in progress. One section at a time; Radar Posts and Yump Boards work inside a
/// section too.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tracker {
    prev: Option<Vec3>,
    section: Option<Section>,
    yump: Yump,
}

/// The section in progress, for the HUD's live readout.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Live {
    pub idx: usize,
    pub t: f32,
    /// Share of the section done (0..1).
    pub done: f32,
    /// The value so far: Pace = mean speed so far (km/h), Slide = points so far.
    pub value: f32,
}

impl Tracker {
    pub fn live(&self, placed: &[Placed], f: &ChallengeFrame) -> Option<Live> {
        let s = self.section?;
        let p = placed.get(s.idx)?;
        let (along, _) = p.first().local(f.pos);
        let done = (along / p.length.max(1.0)).clamp(0.0, 1.0);
        let value = match p.def.kind {
            ChallengeKind::PaceSection => along.max(0.0) / s.t.max(0.1) * 3.6,
            _ => (f.slide_meter - s.meter0).max(0.0),
        };
        Some(Live { idx: s.idx, t: s.t, done, value })
    }

    /// Forgets the attempts (a new run, leaving the game).
    pub fn clear(&mut self) {
        *self = Tracker::default();
    }

    pub fn step(&mut self, f: &ChallengeFrame, placed: &[Placed], out: &mut Vec<Outcome>) {
        let Some(prev) = self.prev.replace(f.pos) else { return };
        let moved = Vec2::new(f.pos.x - prev.x, f.pos.z - prev.z).length();
        // A jump in position (recover, respawn): every attempt is off.
        if moved > 4.0 + f.vel.length() * f.dt * 2.0 {
            if let Some(s) = self.section.take() {
                out.push(Outcome::Void { idx: s.idx, why: "Recovered" });
            }
            if let Yump::Up { idx, .. } | Yump::Landing { idx, .. } = self.yump {
                out.push(Outcome::Void { idx, why: "Recovered" });
            }
            self.yump = Yump::Idle;
            return;
        }
        if !f.live {
            return;
        }
        self.sections(prev, f, placed, out);
        self.radars(prev, f, placed, out);
        self.yumps(prev, f, placed, out);
    }

    fn sections(&mut self, prev: Vec3, f: &ChallengeFrame, placed: &[Placed], out: &mut Vec<Outcome>) {
        // Start gates (forwards): a start, or a restart of the same section.
        for (idx, p) in placed.iter().enumerate() {
            if p.def.kind.section() {
                let g = p.first();
                if g.crossed(prev, f.pos, g.half + GATE_SLACK) == 1 {
                    if let Some(s) = self.section
                        && s.idx != idx
                    {
                        out.push(Outcome::Void { idx: s.idx, why: "Left the section" });
                    }
                    self.section = Some(Section { idx, t: 0.0, next: 1, meter0: f.slide_meter, hits: f.hits, back: 0.0, off: 0.0 });
                    out.push(Outcome::Started { idx });
                    return;
                }
            }
        }
        let Some(mut s) = self.section else { return };
        let p = &placed[s.idx];
        s.t += f.dt;
        let void = |out: &mut Vec<Outcome>, why| out.push(Outcome::Void { idx: s.idx, why });
        // Off course: off its road for a while, behind the start, reversing or crawling.
        let (along, _) = p.first().local(f.pos);
        let speed_along = Vec2::new(f.vel.x, f.vel.z).dot(p.first().dir);
        s.back = if speed_along < -2.0 { s.back + f.dt } else { 0.0 };
        let on_road = f.road.is_some_and(|(r, lat)| r == p.def.road && lat <= OFF_COURSE);
        s.off = if on_road { 0.0 } else { s.off + f.dt };
        if s.back > REVERSE_SECS {
            self.section = None;
            return void(out, "Reversed");
        }
        if s.off > OFF_SECS || along < -OFF_COURSE {
            self.section = None;
            return void(out, "Left the section");
        }
        if s.t > 20.0 && s.t > p.length / (CRAWL_KMH / 3.6) {
            self.section = None;
            return void(out, "Too slow");
        }
        if p.def.kind == ChallengeKind::SlideSection && f.hits > s.hits {
            self.section = None;
            return void(out, "Hit something");
        }
        // Gates ahead, in order: skipping one voids the run.
        let last = p.gates.len() - 1;
        for k in s.next..=last {
            let g = &p.gates[k];
            let reach = if k == last { g.half + GATE_SLACK } else { MID_REACH };
            if g.crossed(prev, f.pos, reach.max(MID_REACH)) != 1 {
                continue;
            }
            if k != s.next {
                self.section = None;
                return void(out, "Missed a gate");
            }
            if k < last {
                s.next += 1;
                out.push(Outcome::Checkpoint { idx: s.idx, k });
                continue;
            }
            // The finish line (within the road and its verge).
            self.section = None;
            if g.crossed(prev, f.pos, reach) != 1 {
                return void(out, "Missed the finish");
            }
            let value = match p.def.kind {
                ChallengeKind::PaceSection => {
                    // The exact time the line was crossed (this step's share of dt).
                    let (da, _) = g.local(prev);
                    let (db, _) = g.local(f.pos);
                    let t = s.t - f.dt * (db / (db - da).max(1e-6));
                    p.length / t.max(0.1) * 3.6
                }
                _ => (f.slide_meter - s.meter0).max(0.0).round(),
            };
            out.push(Outcome::Finished { idx: s.idx, value, pos: g.p });
            return;
        }
        self.section = Some(s);
    }

    fn radars(&mut self, prev: Vec3, f: &ChallengeFrame, placed: &[Placed], out: &mut Vec<Outcome>) {
        for (idx, p) in placed.iter().enumerate() {
            if p.def.kind == ChallengeKind::RadarPost {
                let g = p.first();
                if g.crossed(prev, f.pos, g.half + GATE_SLACK) != 0 {
                    let v = Vec2::new(f.vel.x, f.vel.z).length() * 3.6;
                    out.push(Outcome::Finished { idx, value: v, pos: g.p });
                }
            }
        }
    }

    fn yumps(&mut self, prev: Vec3, f: &ChallengeFrame, placed: &[Placed], out: &mut Vec<Outcome>) {
        let flat = |a: Vec3, b: Vec3| Vec2::new(a.x - b.x, a.z - b.z).length();
        self.yump = match self.yump {
            Yump::Idle if f.contacts == 0 => {
                // Take-off where the last wheel left (the step before): over a crest?
                placed
                    .iter()
                    .enumerate()
                    .find(|(_, p)| {
                        p.def.kind == ChallengeKind::YumpBoard && {
                            let (along, across) = p.first().local(prev);
                            along.abs() <= CREST_REACH && across.abs() <= p.first().half + GATE_SLACK
                        }
                    })
                    .map_or(Yump::Idle, |(idx, _)| Yump::Up { idx, from: prev, t: f.dt, hits: f.hits })
            }
            Yump::Idle => Yump::Idle,
            Yump::Up { idx, from, t, hits } if f.contacts == 0 => Yump::Up { idx, from, t: t + f.dt, hits },
            Yump::Up { idx, from, t, hits } => {
                if t < YUMP_MIN_AIR {
                    Yump::Idle
                } else if f.hits > hits || f.body_contact || f.up_y < 0.85 {
                    out.push(Outcome::Void { idx, why: "Bad landing" });
                    Yump::Idle
                } else {
                    Yump::Landing { idx, dist: flat(f.pos, from), window: 0.0, hits }
                }
            }
            Yump::Landing { idx, dist, window, hits } => {
                if f.hits > hits || f.body_contact || f.up_y < 0.85 {
                    out.push(Outcome::Void { idx, why: "Bad landing" });
                    Yump::Idle
                } else if window + f.dt >= LANDING_WINDOW - 1e-3 {
                    out.push(Outcome::Finished { idx, value: dist, pos: placed[idx].first().p });
                    Yump::Idle
                } else {
                    Yump::Landing { idx, dist, window: window + f.dt, hits }
                }
            }
        };
    }
}

// --- Results ----------------------------------------------------------------------------

/// Stars for a value (0..=3): each threshold reached is a star.
pub fn stars(value: f32, thresholds: &[f32; 3]) -> u8 {
    thresholds.iter().filter(|&&t| value >= t).count() as u8
}

/// The player's best per challenge.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Best {
    pub value: f32,
    pub stars: u8,
    pub runs: u32,
}

/// Every best, by challenge id (`<DataDir>/`[`BESTS_FILE`]).
#[derive(Resource, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Bests {
    pub version: u32,
    pub by_id: BTreeMap<u32, Best>,
}

impl Bests {
    pub fn get(&self, id: u32) -> Option<&Best> {
        self.by_id.get(&id)
    }

    /// Stars earned on all challenges.
    pub fn total_stars(&self) -> u32 {
        self.by_id.values().map(|b| b.stars as u32).sum()
    }
}

/// What a finished run gives: the result message and the Notes (with their reason).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reward {
    pub result: ChallengeResult,
    /// Notes for new stars (each star tier pays once), or for a new best without one.
    pub notes: u32,
    pub new_stars: bool,
}

/// Records a run in the bests and works out its reward. Higher is better for every kind.
pub fn reward(bests: &mut Bests, def: &ChallengeDef, value: f32) -> Reward {
    let earned = stars(value, &def.thresholds);
    let b = bests.by_id.entry(def.id).or_default();
    let new_best = b.runs == 0 || value > b.value;
    let before = b.stars;
    b.runs += 1;
    if new_best {
        b.value = value;
    }
    b.stars = b.stars.max(earned);
    let star_notes: u32 = (before..earned).map(|k| STAR_NOTES[k as usize]).sum();
    let notes = if star_notes > 0 {
        star_notes
    } else if new_best {
        BEST_NOTES
    } else {
        0
    };
    Reward { result: ChallengeResult { id: def.id, value, stars: earned, new_best }, notes, new_stars: star_notes > 0 }
}

// --- In the game ------------------------------------------------------------------------

/// The challenges of the world loaded now (empty outside the open world).
#[derive(Resource, Clone, Default)]
pub struct Challenges(pub Arc<Vec<Placed>>);

/// The attempts in progress and the last result (the HUD reads both).
#[derive(Resource, Clone, Debug, Default)]
pub struct ChallengeRun {
    pub tracker: Tracker,
    /// The section in progress.
    pub live: Option<Live>,
    /// The last result: (challenge index, reward, seconds since).
    pub last: Option<(usize, Reward, f32)>,
    /// The last void: (challenge index, why, seconds since).
    pub void: Option<(usize, &'static str, f32)>,
}

/// The challenges' step in `FixedUpdate`, after Style (Slide Sections read its meter).
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ChallengeSet;

#[derive(Component)]
struct Marker;

/// The radar camera's flash lamp (its material flashes on [`RadarFlash`]).
#[derive(Component)]
struct FlashLamp {
    idx: usize,
    mat: Handle<StandardMaterial>,
}

pub struct ChallengesPlugin;

impl Plugin for ChallengesPlugin {
    fn build(&self, app: &mut App) {
        use crate::world::StageWorld;
        app.add_message::<ChallengeResult>()
            .add_message::<RadarFlash>()
            .add_message::<RunCue>()
            .init_resource::<ChallengeRun>()
            .add_systems(Startup, load_bests)
            .configure_sets(FixedUpdate, ChallengeSet.after(crate::career::style::StyleSet))
            .add_systems(FixedUpdate, track.in_set(ChallengeSet).run_if(resource_exists::<StageWorld>).run_if(resource_exists::<Challenges>))
            .add_systems(Update, (setup, flash).run_if(resource_exists::<StageWorld>))
            .add_systems(OnExit(engine::flow::AppState::InGame), leave);
        #[cfg(feature = "dev")]
        dev::register(app);
    }
}

fn load_bests(mut commands: Commands, dir: Option<Res<engine::paths::DataDir>>) {
    let b: Bests = dir.map(|d| d.load_json(BESTS_FILE)).unwrap_or_default();
    commands.insert_resource(b);
}

fn leave(mut commands: Commands, mut run: ResMut<ChallengeRun>) {
    *run = ChallengeRun::default();
    commands.remove_resource::<Challenges>();
}

/// Once per world: load the challenges and put up their markers.
fn setup(world: &mut World) {
    let Some(stage) = world.get_resource::<crate::world::StageWorld>() else { return };
    let Some(open) = stage.open.clone() else { return };
    if world.contains_resource::<Challenges>() {
        return;
    }
    let path = file_path();
    let defs = match load(&path) {
        Ok(d) => d,
        Err(e) => {
            error!("challenges: {e}");
            Vec::new()
        }
    };
    let placed = place(&defs, &open.net);
    for msg in check(&defs, &open.net, 6.0) {
        warn!("challenges: {msg} (re-anchor: see open::challenges::reanchor)");
    }
    if crate::art::visual(world) {
        markers(world, &open, &placed);
    }
    info!("open world: {} challenges", placed.len());
    world.insert_resource(Challenges(Arc::new(placed)));
}

#[allow(clippy::too_many_arguments)]
fn track(
    time: Res<Time<Fixed>>,
    challenges: Res<Challenges>,
    stage: Res<crate::world::StageWorld>,
    run: Res<crate::drive::StageRun>,
    cars: Query<&crate::drive::DrivenCar>,
    rhythm: Res<crate::career::style::Rhythm>,
    (paused, loading, role): (Option<Res<engine::pause::Paused>>, Option<Res<crate::loading::Loading>>, Option<Res<crate::meta::Role>>),
    mut state: ResMut<ChallengeRun>,
    mut bests: ResMut<Bests>,
    dir: Option<Res<engine::paths::DataDir>>,
    (mut results, mut flashes, mut cues): (MessageWriter<ChallengeResult>, MessageWriter<RadarFlash>, MessageWriter<RunCue>),
    (mut pay, mut style): (MessageWriter<crate::career::Payout>, MessageWriter<crate::career::style::StyleCommand>),
    mut out: Local<Vec<Outcome>>,
) {
    let dt = time.delta_secs();
    if let Some((_, _, t)) = state.last.as_mut() {
        *t += dt;
    }
    if let Some((_, _, t)) = state.void.as_mut() {
        *t += dt;
    }
    if role.is_some_and(|r| *r == crate::meta::Role::Guide) {
        return;
    }
    let Ok(c) = cars.single() else { return };
    let car = &c.car;
    let f = ChallengeFrame {
        dt,
        // Free roam only: events have their own clock.
        live: stage.free_roam() && loading.is_none() && !paused.is_some_and(|p| p.0) && run.down.is_none(),
        pos: car.pos,
        vel: car.vel,
        fwd: car.forward(),
        up_y: car.up().y,
        contacts: car.wheels.iter().filter(|w| w.contact).count() as u8,
        hits: car.damage.hits,
        body_contact: car.body_contact,
        slide_meter: rhythm.slide_meter,
        road: stage.open.as_ref().and_then(|o| o.net.nearest(Vec2::new(car.pos.x, car.pos.z), 60.0)).map(|n| (n.road as u32, n.at.lateral.abs())),
    };
    out.clear();
    let placed = &challenges.0;
    state.tracker.step(&f, placed, &mut out);
    state.live = state.tracker.live(placed, &f);
    let mut save = false;
    for o in out.drain(..) {
        match o {
            Outcome::Started { .. } => {
                style.write(crate::career::style::StyleCommand::Bank);
                cues.write(RunCue::Go);
            }
            Outcome::Checkpoint { .. } => {
                cues.write(RunCue::Checkpoint);
            }
            Outcome::Void { idx, why } => {
                cues.write(RunCue::Fail);
                state.void = Some((idx, why, 0.0));
            }
            Outcome::Finished { idx, value, pos } => {
                let def = &placed[idx].def;
                let r = reward(&mut bests, def, value);
                save = true;
                info!("challenge {} {}: {value:.1} {} → {} stars{}, {} Notes", def.id, def.name, def.kind.unit(), r.result.stars, if r.result.new_best { ", best" } else { "" }, r.notes);
                if def.kind == ChallengeKind::RadarPost {
                    flashes.write(RadarFlash { pos, kmh: value, target_beaten: r.result.stars > 0, personal_best: r.result.new_best });
                } else {
                    cues.write(RunCue::Finish { place: 0, best: r.result.new_best });
                }
                if r.notes > 0 {
                    let reason = if r.new_stars { crate::career::PayReason::ChallengeStar { id: def.id, stars: r.result.stars } } else { crate::career::PayReason::ChallengeBest { id: def.id } };
                    pay.write(crate::career::Payout { amount: r.notes, reason });
                }
                results.write(r.result);
                state.last = Some((idx, r, 0.0));
                state.void = None;
            }
        }
    }
    if save
        && let Some(d) = dir
        && let Err(e) = d.save_json(BESTS_FILE, &*bests)
    {
        error!("challenges: can't save the bests: {e}");
    }
}

// --- Markers ----------------------------------------------------------------------------

/// A strip across the road at a gate, following the surface (`segs` pieces across).
fn stripe(open: &super::OpenWorld, g: &Gate, depth: f32) -> Mesh {
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, PrimitiveTopology};
    const SEGS: usize = 10;
    let left = g.left();
    let (mut pos, mut uv, mut idx) = (Vec::new(), Vec::new(), Vec::new());
    for k in 0..=SEGS {
        let u = k as f32 / SEGS as f32;
        let side = (u * 2.0 - 1.0) * g.half;
        for (j, along) in [-depth * 0.5, depth * 0.5].into_iter().enumerate() {
            let xz = Vec2::new(g.p.x, g.p.z) + left * side + g.dir * along;
            let y = open.ground_height(xz.x, xz.y) + 0.03;
            pos.push([xz.x - g.p.x, y - g.p.y, xz.y - g.p.z]);
            uv.push([u, j as f32]);
        }
    }
    for k in 0..SEGS as u32 {
        let a = k * 2;
        idx.extend_from_slice(&[a, a + 2, a + 1, a + 1, a + 2, a + 3]);
    }
    let n = pos.len();
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; n])
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
        .with_inserted_indices(Indices::U32(idx))
}

/// Boards, flag posts, gate stripes and the radar cameras (static; about 13 × 6 small meshes).
fn markers(world: &mut World, open: &super::OpenWorld, placed: &[Placed]) {
    use bevy::light::NotShadowCaster;
    use engine::flow::AppState;
    let (post, flag, board, cam, lamp) = {
        let mut m = world.resource_mut::<Assets<Mesh>>();
        (m.add(Cylinder::new(0.05, 2.6)), m.add(Cuboid::new(0.04, 0.45, 0.7)), m.add(Cuboid::new(1.3, 0.9, 0.05)), m.add(Cuboid::new(0.45, 0.35, 0.6)), m.add(Cuboid::new(0.3, 0.12, 0.05)))
    };
    let (white, grey, kinds) = {
        let mut m = world.resource_mut::<Assets<StandardMaterial>>();
        let white = m.add(StandardMaterial { base_color: Color::srgb(0.92, 0.92, 0.9), perceptual_roughness: 0.6, ..default() });
        let grey = m.add(StandardMaterial { base_color: Color::srgb(0.25, 0.26, 0.27), perceptual_roughness: 0.5, metallic: 0.3, ..default() });
        let kinds: Vec<Handle<StandardMaterial>> = ChallengeKind::ALL.iter().map(|k| m.add(StandardMaterial { base_color: k.color(), perceptual_roughness: 0.55, ..default() })).collect();
        (white, grey, kinds)
    };
    let stripe_mat = world.resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial {
        base_color: Color::srgba(0.95, 0.95, 0.92, 0.85),
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 0.8,
        depth_bias: 10.0,
        ..default()
    });
    for (idx, p) in placed.iter().enumerate() {
        let kind = p.def.kind;
        let km = kinds[ChallengeKind::ALL.iter().position(|&k| k == kind).unwrap_or(0)].clone();
        let name = format!("challenge {} {}", p.def.id, p.def.name);
        let spawn = |w: &mut World, mesh: &Handle<Mesh>, mat: &Handle<StandardMaterial>, t: Transform| {
            w.spawn((Name::new(name.clone()), Marker, Mesh3d(mesh.clone()), MeshMaterial3d(mat.clone()), t, NotShadowCaster, DespawnOnExit(AppState::InGame)))
                .id()
        };
        // Visible gates: start and end (not a Pace Section's hidden ones), the radar line, the crest.
        let shown: Vec<&Gate> = if kind.section() { vec![p.first(), p.gates.last().unwrap()] } else { vec![p.first()] };
        for (k, g) in shown.iter().enumerate() {
            let yaw = libm::atan2f(-g.dir.x, -g.dir.y);
            let rot = Quat::from_rotation_y(yaw);
            if kind != ChallengeKind::YumpBoard {
                let m = world.resource_mut::<Assets<Mesh>>().add(stripe(open, g, if kind == ChallengeKind::RadarPost { 0.35 } else { 0.6 }));
                spawn(world, &m, &stripe_mat, Transform::from_translation(g.p));
            }
            // Flag posts at both edges.
            for side in [1.0, -1.0] {
                let xz = Vec2::new(g.p.x, g.p.z) + g.left() * side * (g.half + 0.7);
                let base = Vec3::new(xz.x, open.ground_height(xz.x, xz.y), xz.y);
                spawn(world, &post, &white, Transform::from_translation(base + Vec3::Y * 1.3));
                spawn(world, &flag, &km, Transform::from_translation(base + Vec3::Y * 2.3 + rot * Vec3::new(0.0, 0.0, 0.0)).with_rotation(rot * Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)));
            }
            // The kind board on the right verge at the first gate, facing the traffic.
            if k == 0 {
                let xz = Vec2::new(g.p.x, g.p.z) - g.left() * (g.half + 2.2) - g.dir * 6.0;
                let base = Vec3::new(xz.x, open.ground_height(xz.x, xz.y), xz.y);
                spawn(world, &post, &white, Transform::from_translation(base + Vec3::Y * 1.3));
                spawn(world, &board, &km, Transform::from_translation(base + Vec3::Y * 2.2).with_rotation(rot));
                if kind == ChallengeKind::RadarPost {
                    // The camera on its own post across the road, looking down the road.
                    let xz = Vec2::new(g.p.x, g.p.z) + g.left() * (g.half + 2.0);
                    let base = Vec3::new(xz.x, open.ground_height(xz.x, xz.y), xz.y);
                    spawn(world, &post, &grey, Transform::from_translation(base + Vec3::Y * 1.3).with_scale(Vec3::new(1.6, 1.2, 1.6)));
                    spawn(world, &cam, &grey, Transform::from_translation(base + Vec3::Y * 3.25).with_rotation(rot));
                    let mat = world.resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial { base_color: Color::srgb(0.9, 0.9, 0.85), emissive: LinearRgba::BLACK, ..default() });
                    let e = spawn(world, &lamp, &mat, Transform::from_translation(base + Vec3::Y * 3.5 + rot * Vec3::new(0.0, 0.0, -0.31)).with_rotation(rot));
                    world.entity_mut(e).insert(FlashLamp { idx, mat });
                }
            }
        }
    }
}

/// The radar camera flashes (one white flash, 0.25 s; no strobing).
fn flash(time: Res<Time>, mut flashes: MessageReader<RadarFlash>, challenges: Option<Res<Challenges>>, lamps: Query<&FlashLamp>, mut mats: ResMut<Assets<StandardMaterial>>, mut lit: Local<Vec<(usize, f32)>>) {
    let dt = time.delta_secs();
    if let Some(ch) = challenges {
        for f in flashes.read() {
            if let Some(idx) = ch.0.iter().position(|p| p.def.kind == ChallengeKind::RadarPost && p.first().p.distance(f.pos) < 1.0) {
                lit.push((idx, 0.0));
            }
        }
    }
    if lit.is_empty() {
        return;
    }
    for l in lit.iter_mut() {
        l.1 += dt;
    }
    for lamp in &lamps {
        if let Some(&(_, t)) = lit.iter().find(|l| l.0 == lamp.idx)
            && let Some(mut m) = mats.get_mut(&lamp.mat)
        {
            let k = (1.0 - t / 0.25).clamp(0.0, 1.0);
            m.emissive = LinearRgba::rgb(60.0, 60.0, 55.0) * k * k;
        }
    }
    lit.retain(|l| l.1 < 0.3);
}

#[cfg(feature = "dev")]
mod dev {
    use engine::devtools::{AddControlCommand, params};
    use serde_json::{Value, json};

    use super::*;

    pub fn register(app: &mut App) {
        app.add_control_command("challenges.list", "Roadside challenges: id, kind, name, road, gates, thresholds, best", params(&[]), |w, _| {
            let ch = w.get_resource::<Challenges>().ok_or("no open world loaded")?;
            let bests = w.get_resource::<Bests>().cloned().unwrap_or_default();
            Ok(json!(ch.0.iter().map(|p| {
                let b = bests.get(p.def.id);
                json!({"id": p.def.id, "kind": p.def.kind.label(), "name": p.def.name, "road": p.def.road, "gates": p.def.gates, "at": [p.first().p.x, p.first().p.z], "thresholds": p.def.thresholds, "best": b.map(|b| b.value), "stars": b.map_or(0, |b| b.stars)})
            }).collect::<Vec<_>>()))
        })
        .add_control_command("challenges.state", "The challenge in progress, the last result and void", params(&[]), |w, _| {
            let r = w.get_resource::<ChallengeRun>().ok_or("no challenges")?;
            Ok(json!({
                "live": r.live.map(|l| json!({"idx": l.idx, "t": l.t, "done": l.done, "value": l.value})),
                "last": r.last.map(|(i, rw, t)| json!({"idx": i, "id": rw.result.id, "value": rw.result.value, "stars": rw.result.stars, "new_best": rw.result.new_best, "notes": rw.notes, "ago": t})),
                "void": r.void.map(|(i, why, t)| json!({"idx": i, "why": why, "ago": t})),
            }))
        })
        .add_control_command(
            "challenges.goto",
            "Dev: put the car `lead` m before a challenge's first gate, facing it, and its road under the autopilot",
            params(&[("id", "integer", "challenge id (challenges.list)"), ("lead", "number", "metres before the gate (default 150)"), ("kmh", "number", "start speed (default 0)")]),
            |w, p: Value| {
                let id = p.get("id").and_then(Value::as_u64).ok_or("`id` is required")? as u32;
                let lead = p.get("lead").and_then(Value::as_f64).unwrap_or(150.0) as f32;
                let kmh = p.get("kmh").and_then(Value::as_f64).unwrap_or(0.0) as f32;
                let open = w.get_resource::<crate::world::StageWorld>().and_then(|s| s.open.clone()).ok_or("not in the open world")?;
                let ch = w.get_resource::<Challenges>().ok_or("no challenges")?.0.clone();
                let pl = ch.iter().find(|p| p.def.id == id).ok_or("no such challenge")?;
                let road = open.net.roads[pl.def.road as usize].road.clone();
                let s = (pl.def.gates[0] - lead).max(0.0);
                let i = road.station(s);
                let (at, d) = (road.pts[i], road.dir[i]);
                let heading = libm::atan2f(d.x, d.y);
                {
                    let mut st = w.resource_mut::<crate::world::StageWorld>();
                    st.corners = Arc::new(crate::stage::corners(&road));
                    st.crests = Arc::new(crate::stage::crests(&road));
                    st.road = Arc::new(road);
                }
                // As `open.tour`: traffic isn't solid (the autopilot doesn't look out for it).
                open.traffic_solid.store(false, std::sync::atomic::Ordering::Relaxed);
                let radius = w.get_resource::<crate::drive::CarSpec>().map_or(0.32, |s| s.0.wheel_radius);
                let mut q = w.query::<&mut crate::drive::DrivenCar>();
                for mut c in q.iter_mut(w) {
                    crate::vehicle::recover(&mut c.car, at, heading);
                    let fwd = c.car.forward();
                    c.car.vel = fwd * kmh / 3.6;
                    for wh in &mut c.car.wheels {
                        wh.spin = kmh / 3.6 / radius;
                    }
                    c.prev = (c.car.pos, c.car.rot);
                }
                Ok(json!({"id": id, "at": [at.x, at.z], "station": s}))
            },
        );
    }
}

#[cfg(test)]
#[path = "challenges_tests.rs"]
mod tests;
