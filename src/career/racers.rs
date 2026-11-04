//! Rival drivers: five AI cars on the same deterministic sim, the race field and its result,
//! and the list of other cars that style scoring and the HUD read.
//!
//! **Owner: stream F (GAMEPLAY).** Design: `docs/festival/GAME_SYSTEMS.md` §5 and AI-1..AI-8;
//! rulings `docs/festival/DECISIONS.md` R1-R7; determinism `docs/festival/TECH_PASS.md` §5.
//!
//! Rules that keep ghosts exact: only a run started in [`EventMode::Race`] has rivals; Race
//! runs record no ghost and no board time. Racers have their own [`Racer`] component (never
//! `drive::DrivenCar`) and their own `FixedUpdate` system after `drive::SimulateSet`, stepped in
//! `slot` order. The player sees them through [`RacerSolids`] (hooked into
//! `world::StageWorld::obstacles` in wave 0).

use std::sync::{Arc, RwLock};

use bevy::math::Vec2;
use bevy::prelude::*;

use crate::pilot::{self, Pilot, PilotParams};
use crate::stage::{Corner, Road};
use crate::vehicle::{self, Car, DamageMode, Ground, Input, Obstacle, Spec, Wall};

use super::garage::{self, CARS, CarDef, Class, Paint, UpgradeKind, Upgrades};

/// Registers the race flow (`career::race_flow`).
pub struct RacersPlugin;

impl Plugin for RacersPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(super::race_flow::RaceFlowPlugin);
    }
}

/// Race difficulty (GAME_SYSTEMS §5 table).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Difficulty {
    Rookie,
    #[default]
    Pro,
    Ace,
    Legend,
}

impl Difficulty {
    pub const ALL: [Difficulty; 4] = [Difficulty::Rookie, Difficulty::Pro, Difficulty::Ace, Difficulty::Legend];

    pub fn label(self) -> &'static str {
        match self {
            Difficulty::Rookie => "Rookie",
            Difficulty::Pro => "Pro",
            Difficulty::Ace => "Ace",
            Difficulty::Legend => "Legend",
        }
    }
    /// 0 Rookie .. 3 Legend (saves, menus).
    pub fn index(self) -> u8 {
        self as u8
    }
    pub fn from_index(i: u8) -> Difficulty {
        Difficulty::ALL[(i as usize).min(3)]
    }
    /// Share of the surface's grip used in corners (GAME_SYSTEMS §5).
    pub fn pace(self) -> f32 {
        [0.55, 0.68, 0.80, 0.92][self as usize]
    }
    /// Share of grip used braking.
    pub fn brake(self) -> f32 {
        [0.50, 0.55, 0.60, 0.65][self as usize]
    }
    /// Chance of a mistake at each corner of grade ≤ 2.
    pub fn mistake(self) -> f32 {
        [0.15, 0.08, 0.04, 0.01][self as usize]
    }
    /// Chance a rival lets the player past when caught (Legend never: it defends the centre).
    pub fn yields(self) -> f32 {
        [1.0, 1.0, 0.5, 0.0][self as usize]
    }
    /// The player's start slot, 1-based (1 = front).
    pub fn player_slot(self) -> u8 {
        [3, 4, 5, 6][self as usize]
    }
    /// The purse multiplier (GAME_SYSTEMS §4).
    pub fn mult(self) -> f32 {
        [0.75, 1.0, 1.5, 2.5][self as usize]
    }
    /// The entry fee as a share of the purse (Ace 10 %, Legend 25 %).
    pub fn fee_share(self) -> f32 {
        [0.0, 0.0, 0.10, 0.25][self as usize]
    }
}

/// How an event run is driven (DECISIONS R1). Lap events are Time Trial only.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EventMode {
    /// Solo, ghost, records (today's events).
    #[default]
    TimeTrial,
    /// Against 5 rivals.
    Race(Difficulty),
}

/// Start format, decided by the road's width over the first 400 m (GAME_SYSTEMS §5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartFormat {
    /// 2 wide, 3 rows, one launch (≥ 6 m wide).
    Grid,
    /// Single file, released 1.5 s apart (narrower).
    Pursuit,
}

/// One AI car in a race: its own car, spec and pilot, and how its race is going.
#[derive(Component, Clone, Debug)]
pub struct Racer {
    /// Start slot (1 = front) and step order.
    pub slot: u8,
    pub car: Car,
    pub spec: Spec,
    pub pilot: Pilot,
    /// Pose before the last step (interpolation).
    pub prev: (Vec3, Quat),
    /// Invented driver name.
    pub name: &'static str,
    /// Roster car (`garage::CarDef::id`) and its paint.
    pub model: &'static str,
    pub paint: Paint,
    /// Metres along the route (best so far), the step it is released on, how it ended.
    pub progress: f32,
    pub release: u32,
    pub finish: Option<Finish>,
    /// Progress at every [`HIST_STEPS`] steps since the start (the gaps).
    pub hist: Vec<f32>,
    /// Seconds stuck (no progress) / off the road, for putting it back.
    stuck: f32,
    off: f32,
    /// Seconds the player has been right behind it, and the yield in progress (s left, the
    /// speed cap); one roll per catch.
    caught: f32,
    yield_left: f32,
    yield_cap: f32,
    rolled_yield: bool,
    /// Its seeded pace spread (±3 %).
    pub spread: f32,
    /// Times it was put back on the road (the dev state).
    pub resets: u32,
    /// Seconds stopped right behind a stopped car, and seconds left passing through it (a
    /// stalled car on a narrow road would block it for ever: it ghosts past, not solid).
    blocked: f32,
    pub ghost: f32,
    /// The side it is passing on (+1 left, −1 right, 0 not passing): kept until the pass ends.
    pass_side: f32,
}

/// The racers' poses after the last step, `(position, forward)` per car (the same shape as
/// `open::traffic::TrafficSolids`, DECISIONS R4). Shared by `Arc` with the `StageWorld`.
#[derive(Resource, Clone, Default)]
pub struct RacerSolids(pub Arc<RwLock<Vec<(Vec3, Vec3)>>>);

impl RacerSolids {
    /// Collision cylinders for racers within `r` of `p`, appended to `out` (the traffic shape:
    /// three circles along the car).
    pub fn obstacles(&self, p: Vec2, r: f32, out: &mut Vec<Obstacle>) {
        if let Ok(poses) = self.0.read() {
            for pose in poses.iter() {
                crate::open::traffic::push_solids(*pose, p, r, out);
            }
        }
    }
}

/// Who an entrant is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Who {
    Player,
    /// A rival by start slot.
    Rival(u8),
}

/// One entrant's metadata (the sim state is on the `Racer`).
#[derive(Clone, Debug, PartialEq)]
pub struct Entrant {
    pub who: Who,
    /// Invented driver name ("A. Varenne").
    pub name: String,
    /// `garage::CarDef::id`.
    pub car: String,
    /// Fixed step it is released on (pursuit starts).
    pub start_step: u32,
    pub finish: Option<Finish>,
}

/// How an entrant ended.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Finish {
    /// Race clock (s); estimated for rivals still running when the player finishes.
    Time(f32),
    Dnf,
}

/// The race in progress (absent outside Race runs).
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct RaceField {
    pub event: u32,
    pub difficulty: Difficulty,
    pub format: StartFormat,
    pub entrants: Vec<Entrant>,
}

/// A race finished (the player crossed the line or retired). Economy, results screen, career.
#[derive(Message, Clone, Debug, PartialEq)]
pub struct RaceResult {
    pub event: u32,
    pub difficulty: Difficulty,
    /// Finishing order, first to last.
    pub order: Vec<(Who, Finish)>,
    /// The player's position, 1-based.
    pub player_pos: u8,
    /// No logged contact with a rival (the clean bonus).
    pub clean: bool,
}

/// What another car is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OtherKind {
    Traffic,
    Rival(u8),
}

/// Another car near the player, as style scoring, the HUD and the minimap see it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OtherCar {
    pub id: u32,
    pub kind: OtherKind,
    pub pos: Vec3,
    pub fwd: Vec3,
    /// m/s.
    pub speed: f32,
}

/// Every other car (traffic and rivals), rebuilt by F each fixed step (DECISIONS R5).
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct OtherCars(pub Vec<OtherCar>);

// ---------------------------------------------------------------- the race (pure)

/// Rivals in a race (6 cars with the player; DECISIONS R2).
pub const RIVALS: usize = 5;
/// The progress history is sampled every this many fixed steps (0.25 s at 60 Hz).
pub const HIST_STEPS: u32 = 15;
/// Pursuit starts: the gap between releases (1.5 s at 60 Hz).
pub const RELEASE_STEPS: u32 = 90;
/// A Recover in a race holds the car this many steps (5 s) instead of a time penalty.
pub const RECOVER_HOLD: u32 = 300;
/// Contact at or above this closing speed (m/s, 15 km/h) is logged (the clean bonus).
pub const CONTACT_MS: f32 = 15.0 / 3.6;
/// Fixed step.
pub const DT: f32 = 1.0 / 60.0;

/// Invented rival names (GAME_SYSTEMS §5; UX checks none is a real driver before shipping).
pub const NAMES: [&str; 8] = ["A. Varenne", "T. Okkonen", "R. Castell", "M. Duvane", "I. Strand", "K. Belloc", "J. Marr", "S. Ilves"];

/// Start format by the route's minimum width over its first 400 m.
pub fn start_format(road: &Road) -> StartFormat {
    let n = road.station(400.0);
    let min = road.width[..=n.min(road.len() - 1)].iter().copied().fold(f32::MAX, f32::min);
    if min >= 6.0 { StartFormat::Grid } else { StartFormat::Pursuit }
}

/// Where start slot `slot` (1 = front, 6 = back) stands: metres along the route, lateral
/// offset (m, left +), and the step it is released on.
pub fn slot_spot(format: StartFormat, slot: u8) -> (f32, f32, u32) {
    let k = slot.clamp(1, 6) as u32 - 1;
    match format {
        // 2 wide (±1.5 m), 3 rows 8 m apart; one launch.
        StartFormat::Grid => (2.0 + 8.0 * (2 - k / 2) as f32, if k % 2 == 0 { 1.5 } else { -1.5 }, 0),
        // Single file 10 m apart, released 1.5 s apart from the front.
        StartFormat::Pursuit => (2.0 + 10.0 * (5 - k) as f32, 0.0, k * RELEASE_STEPS),
    }
}

/// A pose on the route: `s` metres along, `lateral` m to the left, on the ground; heading.
pub fn route_pose(road: &Road, ground: &impl Ground, s: f32, lateral: f32) -> (Vec3, f32) {
    let i = road.station(s);
    let d = road.dir[i];
    let p = road.pts[i] + Vec3::new(d.y, 0.0, -d.x) * lateral;
    (Vec3::new(p.x, ground.height(p.x, p.z), p.z), libm::atan2f(d.x, d.y))
}

/// The light rubber band (GAME_SYSTEMS §5): `gap_s` > 0 when the rival is ahead of the player.
/// +2 % when > 6 s behind, −2 % when > 6 s ahead, never beyond ±3 %, and 0 in the last 15 %
/// of the route (`frac` = its progress share).
pub fn rubber_band(gap_s: f32, frac: f32) -> f32 {
    if frac >= 0.85 {
        return 0.0;
    }
    let m = if gap_s < -6.0 {
        0.02
    } else if gap_s > 6.0 {
        -0.02
    } else {
        0.0
    };
    f32::clamp(m, -0.03, 0.03)
}

/// The rivals' cars for a player car of `player_pi`: roster cars of the player's class (by
/// stock PI, falling back to the nearest below), each upgraded in a seeded order until its PI
/// is within ±20 of the player's (GAME_SYSTEMS §5).
pub fn pick_cars(player_pi: u16, seed: u64) -> Vec<(&'static CarDef, Upgrades)> {
    let class = Class::of(player_pi);
    let stock = |d: &CarDef| garage::pi(&(d.base)());
    // The player's class, plus cars up to ~120 PI below that upgrades can bring up (variety).
    let mut pool: Vec<&CarDef> = CARS.iter().filter(|d| stock(d) <= player_pi + 20 && (Class::of(stock(d)) == class || stock(d) + 120 >= player_pi)).collect();
    if pool.is_empty() {
        pool = CARS.iter().filter(|d| stock(d) <= player_pi + 20).collect();
        pool.sort_by_key(|d| std::cmp::Reverse(stock(d)));
        pool.truncate(2);
    }
    if pool.is_empty() {
        pool = vec![CARS.iter().min_by_key(|d| stock(d)).unwrap()];
    }
    (0..RIVALS)
        .map(|k| {
            let d = pool[(pilot::unit(seed, 100 + k as u64) * pool.len() as f32) as usize % pool.len()];
            let mut u = Upgrades::default();
            let order: Vec<UpgradeKind> = {
                let r = (pilot::unit(seed, 200 + k as u64) * 7.0) as usize;
                (0..7).map(|i| UpgradeKind::ALL[(i + r) % 7]).collect()
            };
            'grow: for _ in 0..3 {
                for kind in &order {
                    if garage::pi(&garage::build_spec(d, &u)) + 20 >= player_pi {
                        break 'grow;
                    }
                    let l = u.get(*kind);
                    if l < kind.max_level() {
                        u.set(*kind, l + 1);
                        if garage::pi(&garage::build_spec(d, &u)) > player_pi + 20 {
                            u.set(*kind, l);
                        }
                    }
                }
            }
            (d, u)
        })
        .collect()
}

/// The ground a rival drives on: the world (without the race's own solids) plus every other
/// car of the race as solids.
pub struct RivalGround<'a, G: Ground> {
    pub base: &'a G,
    pub others: &'a [(Vec3, Vec3)],
}

impl<G: Ground> Ground for RivalGround<'_, G> {
    fn height(&self, x: f32, z: f32) -> f32 {
        self.base.height(x, z)
    }
    fn normal(&self, x: f32, z: f32) -> Vec3 {
        self.base.normal(x, z)
    }
    fn surface(&self, x: f32, z: f32) -> crate::stage::Surface {
        self.base.surface(x, z)
    }
    fn obstacles(&self, p: Vec2, r: f32, out: &mut Vec<Obstacle>) {
        self.base.obstacles(p, r, out);
        for pose in self.others {
            crate::open::traffic::push_solids(*pose, p, r, out);
        }
    }
    fn walls(&self, p: Vec2, r: f32, out: &mut Vec<Wall>) {
        self.base.walls(p, r, out);
    }
}

/// A car's solid pose: (centre on the ground, forward).
pub fn solid_pose(car: &Car) -> (Vec3, Vec3) {
    (car.pos - car.up() * 0.45, car.forward())
}

/// The player as the race sees it each step.
#[derive(Clone, Copy, Debug)]
pub struct PlayerNow<'a> {
    pub car: &'a Car,
    pub progress: f32,
    /// The player's race is over (finished or retired): rivals stop yielding.
    pub done: bool,
}

/// The race itself, pure: the rivals, the clock and the player's history. The ECS shell
/// (`career::race_flow`) feeds it the player's car each fixed step.
#[derive(Clone, Debug)]
pub struct RaceSim {
    pub event: u32,
    pub difficulty: Difficulty,
    pub format: StartFormat,
    pub attempt: u32,
    pub seed: u64,
    pub rivals: Vec<Racer>,
    /// The player's start slot (1-based) and release step.
    pub player_slot: u8,
    pub player_release: u32,
    /// Steps since the start (the race clock: `step / 60` s); counts once the countdown is over.
    pub step: u32,
    armed: bool,
    /// The player's progress every [`HIST_STEPS`].
    pub player_hist: Vec<f32>,
    /// No contact ≥ 15 km/h between the player and a rival so far.
    pub clean: bool,
    /// Contacts logged (for the HUD / tests).
    pub contacts: u32,
    touching: [bool; RIVALS],
    /// Route length (m).
    pub len: f32,
}

impl RaceSim {
    /// A race on `road` (the event's route) for a player in a car of PI `player_pi`.
    pub fn new(event: u32, difficulty: Difficulty, attempt: u32, road: &Road, ground: &impl Ground, player_pi: u16, damage: DamageMode) -> RaceSim {
        let seed = pilot::seed(&[event as u64, difficulty.index() as u64, attempt as u64]);
        let format = start_format(road);
        let player_slot = difficulty.player_slot();
        let cars = pick_cars(player_pi, seed);
        // Names: 5 of the 8, seeded.
        let mut names: Vec<&'static str> = NAMES.to_vec();
        for k in (1..names.len()).rev() {
            let j = (pilot::unit(seed, 300 + k as u64) * (k + 1) as f32) as usize % (k + 1);
            names.swap(k, j);
        }
        let mut rivals = Vec::with_capacity(RIVALS);
        let slots: Vec<u8> = (1..=6u8).filter(|s| *s != player_slot).collect();
        for (k, slot) in slots.into_iter().enumerate() {
            let (d, u) = cars[k];
            let mut spec = garage::build_spec(d, &u);
            spec.damage = damage;
            spec.abs = true;
            spec.traction_assist = true;
            let (s, lat, release) = slot_spot(format, slot);
            let (pos, heading) = route_pose(road, ground, s, lat);
            let car = Car::new(pos + Vec3::Y * 0.9, heading);
            let spread = (pilot::unit(seed, 400 + k as u64) * 2.0 - 1.0) * 0.03;
            let params = PilotParams {
                pace: difficulty.pace() * (1.0 + spread),
                brake: difficulty.brake(),
                line_offset: 0.0,
                seed: pilot::seed(&[seed, slot as u64]),
                mistake: difficulty.mistake(),
                ..PilotParams::default()
            };
            // Distinct, strong colours (not the white of the default Saker).
            const RIVAL_PAINTS: [u8; 6] = [3, 4, 5, 6, 2, 1];
            let colour = RIVAL_PAINTS[((pilot::unit(seed, 500) * 6.0) as usize + k) % 6];
            rivals.push(Racer {
                slot,
                prev: (car.pos, car.rot),
                car,
                spec,
                // From its grid spot it eases to the racing line.
                pilot: {
                    let mut p = Pilot::new(params);
                    p.offset = lat;
                    p
                },
                name: names[k],
                model: d.id,
                paint: Paint { colour, metallic: k % 2 == 1 },
                progress: s,
                release,
                finish: None,
                hist: Vec::new(),
                stuck: 0.0,
                off: 0.0,
                caught: 0.0,
                yield_left: 0.0,
                yield_cap: 0.0,
                rolled_yield: false,
                spread,
                resets: 0,
                blocked: 0.0,
                ghost: 0.0,
                pass_side: 0.0,
            });
        }
        let player_release = slot_spot(format, player_slot).2;
        RaceSim { event, difficulty, format, attempt, seed, rivals, player_slot, player_release, step: 0, armed: false, player_hist: Vec::new(), clean: true, contacts: 0, touching: [false; RIVALS], len: road.length() }
    }

    /// Where the player starts: position on the ground and heading.
    pub fn player_start(&self, road: &Road, ground: &impl Ground) -> (Vec3, f32) {
        let (s, lat, _) = slot_spot(self.format, self.player_slot);
        route_pose(road, ground, s, lat)
    }

    /// Race clock (s).
    pub fn clock(&self) -> f32 {
        self.step as f32 * DT
    }

    /// Has the start (the countdown) gone?
    pub fn started(&self) -> bool {
        self.armed
    }

    /// Is the player still held at the start (a pursuit release)?
    pub fn player_held(&self) -> bool {
        !self.armed || self.step < self.player_release
    }

    /// The finish line (m along the route).
    pub fn finish_line(&self) -> f32 {
        self.len - 4.0
    }

    /// One fixed step of every rival, after the player's. `counting_down`: the start hasn't
    /// gone yet (everyone held). Each rival sees the player and the others as they were at the
    /// start of this step.
    pub fn step(&mut self, ground: &impl Ground, road: &Road, corners: &[Corner], player: PlayerNow, counting_down: bool) {
        if !counting_down {
            if self.armed {
                self.step += 1;
            } else {
                self.armed = true;
            }
        }
        if self.armed && self.step.is_multiple_of(HIST_STEPS) && self.player_hist.len() as u32 <= self.step / HIST_STEPS {
            self.player_hist.push(player.progress);
        }
        let poses: Vec<(Vec3, Vec3, bool)> = self.rivals.iter().map(|r| (solid_pose(&r.car).0, solid_pose(&r.car).1, r.ghost > 0.0)).collect();
        let mut others = Vec::with_capacity(RIVALS);
        let player_pose = solid_pose(player.car);
        let snapshot: Vec<(f32, f32, f32, bool)> = self.rivals.iter().map(|r| (r.progress, r.car.speed(), lateral_on(road, &r.car), r.finish.is_some())).collect();
        let player_lat = lateral_on(road, player.car);
        for k in 0..self.rivals.len() {
            others.clear();
            if !poses[k].2 {
                others.push(player_pose);
                others.extend(poses.iter().enumerate().filter(|(j, p)| *j != k && !p.2).map(|(_, p)| (p.0, p.1)));
            }
            let g = RivalGround { base: ground, others: &others };
            let held = !self.armed || self.step < self.rivals[k].release || self.rivals[k].finish.is_some();
            let r = &mut self.rivals[k];
            r.prev = (r.car.pos, r.car.rot);
            let input = if held {
                Input { handbrake: true, ..Input::default() }
            } else {
                racecraft(r, k, &snapshot, (player.progress, player.car.speed(), player_lat, player.done), self.difficulty, road, self.seed, self.len);
                r.pilot.input(&r.car, &r.spec, road, corners, ground, r.progress, DT)
            };
            vehicle::step(&mut r.car, &r.spec, input, &g, DT);
            #[cfg(test)]
            if let Ok(t) = std::env::var("RACE_TRACE_SLOT")
                && t.parse::<u8>().ok() == Some(r.slot)
                && self.step % 6 == 0
                && (std::env::var("FROM").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0)..std::env::var("TO").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(1e9)).contains(&r.progress)
            {
                let rp = road.nearest(Vec2::new(r.car.pos.x, r.car.pos.z), 40.0);
                eprintln!("slot {} step {} s {:.0} v {:.1} lat {:+.1} off_t {:?} off {:+.2} cap {:?} pm {:.3} st {:+.2} th {:.2} br {:.2} ghost {:.1} {}", r.slot, self.step, r.progress, r.car.speed() * 3.6, rp.map_or(99.0, |x| x.lateral), r.pilot.offset_target, r.pilot.offset, r.pilot.speed_cap.map(|c| c * 3.6), r.pilot.pace_mod, input.steer, input.throttle, input.brake, r.ghost, r.pilot.doing);
            }
            if held {
                continue;
            }
            // Progress: near the road and forwards only (as the player's).
            let near = road.nearest(Vec2::new(r.car.pos.x, r.car.pos.z), 40.0);
            if let Some(rp) = near
                && rp.s > r.progress
                && rp.s < r.progress + 40.0
            {
                if rp.s > r.progress + 0.05 {
                    r.stuck = 0.0;
                }
                r.progress = rp.s;
            }
            r.stuck += DT;
            // Off the road, on its roof or side, or turned round (after a spin).
            let wrong_way = near.is_some_and(|rp| {
                let d = road.dir[rp.i];
                let f = r.car.forward();
                f.x * d.x + f.z * d.y < -0.3 && r.car.speed().abs() < 8.0
            });
            let off = near.is_none_or(|rp| rp.lateral.abs() > road.half_width(&rp) + 1.5) || r.car.up().y < 0.3 || wrong_way;
            r.off = if off { r.off + DT } else { 0.0 };
            // A broken part retires it (Simulation damage); stuck or off the road, the
            // marshals put it back where it was (deterministic).
            if r.spec.damage == DamageMode::Simulation && r.car.damage.wrecked() {
                r.finish = Some(Finish::Dnf);
                continue;
            }
            if r.off > 3.0 || r.stuck > 10.0 {
                #[cfg(test)]
                if std::env::var("RACE_TRACE").is_ok() {
                    let rp = road.nearest(Vec2::new(r.car.pos.x, r.car.pos.z), 40.0);
                    eprintln!("reset slot {} at {:.0} m (step {}): off {:.1} stuck {:.1} up {:.2} lat {:?} half {:?} v {:.1} ghost {:.1} doing {} dmg {:.2}", r.slot, r.progress, self.step, r.off, r.stuck, r.car.up().y, rp.map(|x| x.lateral), rp.map(|x| road.half_width(&x)), r.car.speed(), r.ghost, r.pilot.doing, r.car.damage.total());
                }
                let i = road.station(r.progress);
                let d = road.dir[i];
                vehicle::recover(&mut r.car, road.pts[i], libm::atan2f(d.x, d.y));
                r.prev = (r.car.pos, r.car.rot);
                r.off = 0.0;
                r.stuck = 0.0;
                r.resets += 1;
            }
            if r.progress >= self.len - 4.0 && r.finish.is_none() {
                r.finish = Some(Finish::Time(self.step as f32 * DT));
            }
        }
        // History and contact with the player.
        let step = self.step;
        for (k, r) in self.rivals.iter_mut().enumerate() {
            if self.armed && step.is_multiple_of(HIST_STEPS) && r.hist.len() as u32 <= step / HIST_STEPS {
                r.hist.push(r.progress);
            }
            let close = touching(&r.car, player.car);
            let closing = (r.car.vel - player.car.vel).length();
            if close && !self.touching[k] && closing >= CONTACT_MS && !player.done && r.ghost <= 0.0 {
                self.clean = false;
                self.contacts += 1;
            }
            self.touching[k] = close;
        }
    }

    /// Seconds `a` is behind `b` from `b`'s history: now − when `b` was where `a` is. Positive
    /// when `a` is behind.
    pub fn gap(hist_b: &[f32], s_a: f32, now: f32) -> Option<f32> {
        let k = hist_b.iter().position(|&s| s >= s_a)?;
        let t = if k == 0 {
            0.0
        } else {
            let (s0, s1) = (hist_b[k - 1], hist_b[k]);
            let f = if s1 > s0 { (s_a - s0) / (s1 - s0) } else { 1.0 };
            (k as f32 - 1.0 + f) * HIST_STEPS as f32 * DT
        };
        Some(now - t)
    }

    /// Seconds a rival is ahead of the player (+) or behind (−), from the histories.
    pub fn gap_to_player(&self, k: usize, player_progress: f32) -> f32 {
        let r = &self.rivals[k];
        let now = self.clock();
        if r.progress >= player_progress {
            RaceSim::gap(&r.hist, player_progress, now).unwrap_or(0.0)
        } else {
            -RaceSim::gap(&self.player_hist, r.progress, now).unwrap_or(0.0)
        }
    }

    /// Standings now: (who, progress, finish), first to last. Finished cars by time, then the
    /// running ones by progress, retirements last.
    pub fn standings(&self, player_progress: f32, player_finish: Option<Finish>) -> Vec<(Who, f32, Option<Finish>)> {
        let mut v: Vec<(Who, f32, Option<Finish>)> = self.rivals.iter().map(|r| (Who::Rival(r.slot), r.progress, r.finish)).collect();
        v.push((Who::Player, player_progress, player_finish));
        let key = |e: &(Who, f32, Option<Finish>)| match e.2 {
            Some(Finish::Time(t)) => (0, t, 0.0),
            None => (1, 0.0, -e.1),
            Some(Finish::Dnf) => (2, 0.0, -e.1),
        };
        v.sort_by(|a, b| {
            let (ka, kb) = (key(a), key(b));
            ka.0.cmp(&kb.0).then(ka.1.total_cmp(&kb.1)).then(ka.2.total_cmp(&kb.2))
        });
        v
    }

    /// The result when the player's race ends: `player` is the player's race time, or `None`
    /// for a retirement (DNF). Rivals still running get their elapsed time plus the remaining
    /// metres at their mean speed so far.
    pub fn result(&self, player: Option<f32>) -> RaceResult {
        let now = self.clock();
        let mut order: Vec<(Who, Finish)> = self
            .rivals
            .iter()
            .map(|r| {
                let f = r.finish.unwrap_or_else(|| {
                    let run = (now - r.release as f32 * DT).max(1.0);
                    let start = slot_spot(self.format, r.slot).0;
                    let mean = ((r.progress - start) / run).max(1.0);
                    Finish::Time(now + (self.len - 4.0 - r.progress).max(0.0) / mean)
                });
                (Who::Rival(r.slot), f)
            })
            .collect();
        order.push((Who::Player, player.map_or(Finish::Dnf, Finish::Time)));
        order.sort_by(|a, b| match (a.1, b.1) {
            (Finish::Time(x), Finish::Time(y)) => x.total_cmp(&y),
            (Finish::Time(_), Finish::Dnf) => std::cmp::Ordering::Less,
            (Finish::Dnf, Finish::Time(_)) => std::cmp::Ordering::Greater,
            // Two retirements: the player last (the rival got further, or it's a wash).
            (Finish::Dnf, Finish::Dnf) => (a.0 == Who::Player).cmp(&(b.0 == Who::Player)),
        });
        let player_pos = order.iter().position(|e| e.0 == Who::Player).unwrap_or(5) as u8 + 1;
        RaceResult { event: self.event, difficulty: self.difficulty, order, player_pos, clean: self.clean }
    }
}

/// Signed lateral offset of a car from the route's centreline (0 off the road).
fn lateral_on(road: &Road, car: &Car) -> f32 {
    road.nearest(Vec2::new(car.pos.x, car.pos.z), 40.0).map_or(0.0, |r| r.lateral)
}

/// Do two cars touch (their 3-circle solids overlap)?
fn touching(a: &Car, b: &Car) -> bool {
    if a.pos.distance_squared(b.pos) > 25.0 {
        return false;
    }
    let (pa, pb) = (solid_pose(a), solid_pose(b));
    let circles = |p: (Vec3, Vec3)| {
        let c = Vec2::new(p.0.x, p.0.z);
        let f = Vec2::new(p.1.x, p.1.z).normalize_or_zero();
        [c - f * 1.3, c, c + f * 1.3]
    };
    let (ca, cb) = (circles(pa), circles(pb));
    ca.iter().any(|x| cb.iter().any(|y| x.distance(*y) < 0.95 * 2.0 + 0.05))
}

/// The race's say in how rival `k` drives this step: the rubber band, gap keeping behind the
/// car ahead, overtaking where the road is wide, yielding to the player when caught.
/// `cars[j]`: (progress, speed, lateral, finished) of rival `j` at the start of the step;
/// `player`: (progress, speed, lateral, done).
#[allow(clippy::too_many_arguments)]
fn racecraft(r: &mut Racer, k: usize, cars: &[(f32, f32, f32, bool)], player: (f32, f32, f32, bool), difficulty: Difficulty, road: &Road, seed: u64, len: f32) {
    let (me_s, me_v) = (r.progress, r.car.speed());
    let me_lat = lateral_on(road, &r.car);
    let gap_s = {
        // Seconds ahead of the player (+) or behind (−), from progress and speed (the
        // histories are the HUD's; this is cheap and smooth enough for the band).
        let v = me_v.max(player.1).max(8.0);
        (me_s - player.0) / v
    };
    r.pilot.pace_mod = 1.0 + rubber_band(gap_s, me_s / len.max(1.0));
    r.pilot.speed_cap = None;
    r.pilot.offset_target = None;

    // The car ahead within 50 m on the route (rivals and the player).
    let mut ahead: Option<(f32, f32, f32)> = None;
    let mut consider = |s: f32, v: f32, lat: f32| {
        let d = s - me_s;
        if d > 0.0 && d < 120.0 && ahead.is_none_or(|a| d < a.0) {
            ahead = Some((d, v, lat));
        }
    };
    for (j, c) in cars.iter().enumerate() {
        if j != k && !c.3 {
            consider(c.0, c.1, c.2);
        }
    }
    if !player.3 {
        consider(player.0, player.1, player.2);
    }

    // Width of the road over the next 60 m.
    let i0 = road.station(me_s);
    let i1 = road.station(me_s + 60.0);
    let min_w = road.width[i0..=i1.max(i0)].iter().copied().fold(f32::MAX, f32::min);
    let straight = road.curv[i0..=i1.max(i0)].iter().all(|c| c.abs() < 1.0 / 120.0);
    // Passing needs room and no real corner in the next 80 m.
    let i2 = road.station(me_s + 80.0);
    let gentle = road.curv[i0..=i2.max(i0)].iter().all(|c| c.abs() < 1.0 / 150.0);

    // Passing through a stalled car (see `Racer::ghost`).
    if r.ghost > 0.0 {
        r.ghost -= DT;
        return;
    }
    let stalled = ahead.is_some_and(|(d, v, _)| d < 15.0 && v < 2.0);
    r.blocked = if stalled && me_v < 2.0 { r.blocked + DT } else { 0.0 };
    if r.blocked > 3.0 {
        r.blocked = 0.0;
        r.ghost = 4.0;
        return;
    }

    if let Some((d, v_ahead, lat_ahead)) = ahead {
        let gap_m = (d - 4.5).max(0.0);
        let wants = me_v.max(v_ahead) >= v_ahead * 1.03 || gap_m < 0.6 * me_v;
        if min_w >= 4.5 && wants && gap_m < 25.0 && (gentle || r.pass_side != 0.0 && straight) {
            // Overtake: the side away from the car ahead (kept once chosen).
            let room = min_w * 0.5 - 1.1;
            if r.pass_side == 0.0 {
                r.pass_side = if lat_ahead >= 0.0 { -1.0 } else { 1.0 };
            }
            r.pilot.offset_target = Some(r.pass_side * room);
            // Not alongside yet: don't run into its back (creep, so it can still pull out).
            if (me_lat - lat_ahead).abs() < 1.7 && gap_m < 6.0 {
                r.pilot.speed_cap = Some(v_ahead + (gap_m * 0.6).max(0.8));
            }
        } else {
            r.pass_side = 0.0;
            // Follow at a 0.6 s headway: the speed it can still brake from to the car ahead's
            // speed within the gap left (≈ 4 m/s², all surfaces).
            let want_gap = 2.0 + 0.6 * me_v;
            let room = gap_m - want_gap;
            let cap = if room >= 0.0 { (v_ahead * v_ahead + 2.0 * 4.0 * room).sqrt() } else { (v_ahead + room * 0.5).max(0.0) };
            r.pilot.speed_cap = Some(cap);
        }
    }

    if ahead.is_none() {
        r.pass_side = 0.0;
    }
    // A car alongside (the grid, a pass in progress): hold this side of the road.
    let alongside = cars.iter().enumerate().filter(|(j, c)| *j != k && !c.3).map(|(_, c)| (c.0, c.2)).chain((!player.3).then_some((player.0, player.2))).any(|(s, lat)| (s - me_s).abs() < 6.5 && (lat - me_lat).abs() < 2.8);
    if alongside && r.pilot.offset_target.is_none() {
        let room = (min_w * 0.5 - 1.1).max(0.0);
        r.pilot.offset_target = Some(me_lat.clamp(-room, room));
    }

    // Yielding: the player right behind (within 1.2 s) for 2 s, on a straight ≥ 3.8 m wide.
    let behind_s = me_s - player.0;
    let caught = !player.3 && behind_s > 0.0 && behind_s < 1.2 * player.1.max(5.0) && player.1 > me_v * 0.9;
    r.caught = if caught { r.caught + DT } else { 0.0 };
    if !caught {
        r.rolled_yield = false;
    }
    if r.yield_left <= 0.0 && r.caught >= 2.0 && straight && min_w >= 3.8 && !r.rolled_yield {
        r.rolled_yield = true;
        if pilot::unit(seed, 600 + r.slot as u64 * 7919 + (me_s as u64)) < difficulty.yields() {
            r.yield_left = 3.0;
            r.yield_cap = me_v * 0.85;
        }
    }
    if r.yield_left > 0.0 {
        r.yield_left -= DT;
        let room = (min_w * 0.5 - 1.1).max(0.0);
        r.pilot.offset_target = Some(if player.2 >= 0.0 { -room } else { room });
        r.pilot.speed_cap = Some(r.pilot.speed_cap.map_or(r.yield_cap, |c| c.min(r.yield_cap)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stage::Surface;

    #[test]
    fn rubber_band_is_light_and_off_at_the_end() {
        assert_eq!(rubber_band(-10.0, 0.5), 0.02, "far behind: a little faster");
        assert_eq!(rubber_band(10.0, 0.5), -0.02, "far ahead: a little slower");
        assert_eq!(rubber_band(3.0, 0.5), 0.0);
        assert_eq!(rubber_band(-30.0, 0.86), 0.0, "never in the last 15 %");
        for g in [-100.0, -6.1, -1.0, 0.0, 6.1, 100.0] {
            for f in [0.0, 0.5, 0.84, 0.9] {
                assert!(rubber_band(g, f).abs() <= 0.03);
            }
        }
    }

    #[test]
    fn start_slots() {
        // Grid: 2 wide, 3 rows 8 m apart, one launch.
        assert_eq!(slot_spot(StartFormat::Grid, 1), (18.0, 1.5, 0));
        assert_eq!(slot_spot(StartFormat::Grid, 2), (18.0, -1.5, 0));
        assert_eq!(slot_spot(StartFormat::Grid, 6), (2.0, -1.5, 0));
        // Pursuit: single file 10 m apart, released 1.5 s (90 steps) apart from the front.
        assert_eq!(slot_spot(StartFormat::Pursuit, 1), (52.0, 0.0, 0));
        assert_eq!(slot_spot(StartFormat::Pursuit, 4), (22.0, 0.0, 270));
        let gaps: Vec<u32> = (1..6).map(|k| slot_spot(StartFormat::Pursuit, k + 1).2 - slot_spot(StartFormat::Pursuit, k).2).collect();
        assert!(gaps.iter().all(|g| *g == RELEASE_STEPS));
        assert_eq!(Difficulty::Legend.player_slot(), 6);
    }

    #[test]
    fn rival_cars_match_the_players_pi() {
        for d in CARS {
            let p = garage::pi(&(d.base)());
            for seed in 0..4u64 {
                let cars = pick_cars(p, seed);
                assert_eq!(cars.len(), RIVALS);
                for (c, u) in cars {
                    let q = garage::pi(&garage::build_spec(c, &u));
                    assert!(q.abs_diff(p) <= 20 || q < p, "{}: rival {} PI {q} vs {p}", d.id, c.id);
                    assert!(q + 60 >= p, "{}: rival {} PI {q} far below {p}", d.id, c.id);
                }
            }
        }
    }

    #[test]
    fn standings_and_result_order() {
        let road = Road::from_centreline(&(0..500).map(|i| Vec2::new(0.0, i as f32 * 2.0)).collect::<Vec<_>>(), &[0.0; 500], vec![7.0; 500], vec![Surface::Tarmac; 500]);
        let g = crate::vehicle::Flat(0.0, Surface::Tarmac);
        let mut sim = RaceSim::new(1, Difficulty::Pro, 0, &road, &g, 570, DamageMode::Simulation);
        assert_eq!(sim.format, StartFormat::Grid);
        sim.step = 600;
        for (k, r) in sim.rivals.iter_mut().enumerate() {
            r.progress = 100.0 + 100.0 * k as f32;
        }
        sim.rivals[4].finish = Some(Finish::Time(55.0));
        sim.rivals[4].progress = sim.len;
        sim.rivals[0].finish = Some(Finish::Dnf);
        // The player at 350 m, running.
        let st = sim.standings(350.0, None);
        assert_eq!(st[0].0, Who::Rival(sim.rivals[4].slot), "the finisher leads");
        assert_eq!(st[1].0, Who::Rival(sim.rivals[3].slot));
        assert_eq!(st[2].0, Who::Player);
        assert_eq!(st[5].0, Who::Rival(sim.rivals[0].slot), "the retirement last");
        // Player crosses first (before the finisher's 55 s and the running rivals' estimates,
        // ≥ 25 s at their pace so far): P1.
        let r = sim.result(Some(20.0));
        assert_eq!(r.player_pos, 1);
        // Player crosses last: rivals still running get estimates ahead of a slow time.
        let r = sim.result(Some(10_000.0));
        assert_eq!(r.player_pos, 5, "behind the 4 running or finished, ahead of the retirement: {:?}", r.order);
        // Retired: DNF, behind everyone.
        let r = sim.result(None);
        assert_eq!(r.player_pos, 6);
        assert!(r.clean);
    }

    #[test]
    fn gaps_from_history() {
        // b went 0, 10, 20, 30 m at 0.25 s steps; a is at 15 m now (t = 1.0 s): b was there
        // at 0.375 s, so a is 0.625 s behind.
        let g = RaceSim::gap(&[0.0, 10.0, 20.0, 30.0], 15.0, 1.0).unwrap();
        assert!((g - 0.625).abs() < 1e-4, "{g}");
    }
}

/// Headless races on the open world's event routes (slow in debug: the full world loads).
#[cfg(test)]
mod race_tests {
    use super::*;
    use crate::open::events;

    struct Run {
        finished: usize,
        resets: u32,
        dnf: usize,
        worst_off: f32,
        poses: Vec<(Vec3, Quat)>,
        secs: f32,
    }

    /// Races event `id` at `d` with the player parked (held) on its slot; `steps` max.
    fn race(base: &crate::world::StageWorld, id: u32, d: Difficulty, steps: u32, stop_at: Option<u32>) -> Run {
        let ev = events::build(&base.open.as_ref().unwrap().net, &base.open.as_ref().unwrap().data.events);
        let e = ev.iter().find(|e| e.id == id).unwrap();
        let mut w = base.clone();
        events::on_route(&mut w, e);
        let mut sim = RaceSim::new(id, d, 1, &w.road, &w, 570, DamageMode::Simulation);
        let (p, h) = sim.player_start(&w.road, &w);
        let mut player = Car::new(p + Vec3::Y * 0.9, h);
        let mut worst_off = 0.0f32;
        let t0 = std::time::Instant::now();
        let mut n = 0;
        let player_s = w.road.nearest(Vec2::new(p.x, p.z), 10.0).map_or(0.0, |r| r.s);
        for i in 0..steps {
            // The player parked on the handbrake (stepped, so it settles like a real car).
            vehicle::step(&mut player, &Spec::default(), Input { handbrake: true, ..Input::default() }, &w, DT);
            sim.step(&w, &w.road, &w.corners, PlayerNow { car: &player, progress: player_s, done: false }, i < 180);
            n += 1;
            for r in &sim.rivals {
                if r.finish.is_none()
                    && let Some(rp) = w.road.nearest(Vec2::new(r.car.pos.x, r.car.pos.z), 40.0)
                {
                    worst_off = worst_off.max(rp.lateral.abs() - w.road.half_width(&rp));
                }
            }
            if stop_at == Some(i) || sim.rivals.iter().all(|r| r.finish.is_some()) {
                break;
            }
        }
        let secs = t0.elapsed().as_secs_f32();
        eprintln!("event {id} {:?}: {n} steps in {secs:.1} s ({:.3} ms per step for 5 rivals)", d, secs * 1000.0 / n as f32);
        for r in &sim.rivals {
            eprintln!("  slot {} {:10} {:7}: {:?} at {:.0} m, resets {}, damage {:.2}", r.slot, r.name, r.model, r.finish, r.progress, r.resets, r.car.damage.total());
        }
        Run {
            finished: sim.rivals.iter().filter(|r| matches!(r.finish, Some(Finish::Time(_)))).count(),
            resets: sim.rivals.iter().map(|r| r.resets).sum(),
            dnf: sim.rivals.iter().filter(|r| r.finish == Some(Finish::Dnf)).count(),
            worst_off,
            poses: sim.rivals.iter().map(|r| (r.car.pos, r.car.rot)).collect(),
            secs,
        }
    }

    fn world() -> crate::world::StageWorld {
        crate::world::generate(&crate::open::def())
    }

    /// AI-4: at Pro, with the player parked, all 5 rivals finish every Rally and Sprint route
    /// without retiring or being put back on the road.
    #[test]
    #[ignore = "slow (the open world + every route, ~minutes in debug): cargo test -- --ignored rivals_finish"]
    fn rivals_finish_every_route_at_pro() {
        let w = world();
        let ev = events::build(&w.open.as_ref().unwrap().net, &w.open.as_ref().unwrap().data.events);
        let only: Option<u32> = std::env::var("RACE_EVENT").ok().and_then(|v| v.parse().ok());
        for e in ev.iter().filter(|e| e.kind != events::EventKind::Lap && only.is_none_or(|o| o == e.id)) {
            let r = race(&w, e.id, Difficulty::Pro, 60 * 1500, None);
            assert_eq!(r.finished, RIVALS, "{}: {} of 5 finished", e.name, r.finished);
            assert_eq!(r.dnf, 0, "{}", e.name);
            assert_eq!(r.resets, 0, "{}: rivals had to be put back on the road", e.name);
            assert!(r.worst_off < 1.5, "{}: a rival went {:.1} m off the road", e.name, r.worst_off);
        }
    }

    /// One pilot alone down each route at a range of paces: metres, offs, time (tuning aid).
    #[test]
    #[ignore = "tuning aid"]
    fn pace_sweep() {
        let w = world();
        let ev = events::build(&w.open.as_ref().unwrap().net, &w.open.as_ref().unwrap().data.events);
        let only: Option<u32> = std::env::var("RACE_EVENT").ok().and_then(|v| v.parse().ok());
        for e in ev.iter().filter(|e| e.kind != events::EventKind::Lap && only.is_none_or(|o| o == e.id)) {
            let mut we = w.clone();
            events::on_route(&mut we, e);
            for pace in [0.55f32, 0.62, 0.68, 0.75, 0.8, 0.92] {
                let mut pilot = Pilot::new(PilotParams { pace, brake: 0.55, ..PilotParams::default() });
                let (p, h) = we.road.start();
                let spec = Spec::default();
                let mut car = Car::new(p + Vec3::Y * 0.9, h);
                let (mut prog, mut offs, mut off, mut t) = (0.0f32, 0, false, 0.0f32);
                let mut where_off = Vec::new();
                while prog < we.road.length() - 4.0 && t < 900.0 {
                    let i = pilot.input(&car, &spec, &we.road, &we.corners, &we, prog, DT);
                    vehicle::step(&mut car, &spec, i, &we, DT);
                    t += DT;
                    let rp = we.road.nearest(Vec2::new(car.pos.x, car.pos.z), 40.0);
                    if let Some(rp) = rp
                        && rp.s > prog
                        && rp.s < prog + 40.0
                    {
                        prog = rp.s;
                    }
                    let now = rp.is_none_or(|r| r.lateral.abs() > we.road.half_width(&r) + 1.5);
                    if now && !off {
                        offs += 1;
                        where_off.push(prog as i32);
                        // Put it back (as the race would) to see the rest of the route.
                        let i = we.road.station(prog);
                        let d = we.road.dir[i];
                        vehicle::recover(&mut car, we.road.pts[i], libm::atan2f(d.x, d.y));
                    }
                    off = now;
                }
                eprintln!("{:24} pace {pace:.2}: {:5.0} m in {t:5.1} s, offs {offs} {:?}, damage {:.2}", e.name, prog, &where_off[..where_off.len().min(8)], car.damage.total());
            }
        }
    }

    /// Traces one pilot through a stretch of a route (tuning aid): RACE_EVENT, PACE, FROM, TO.
    #[test]
    #[ignore = "tuning aid"]
    fn pace_trace() {
        let env = |k: &str, d: f32| std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d);
        let w = world();
        let ev = events::build(&w.open.as_ref().unwrap().net, &w.open.as_ref().unwrap().data.events);
        let e = ev.iter().find(|e| e.id == env("RACE_EVENT", 100.0) as u32).unwrap();
        let mut we = w.clone();
        events::on_route(&mut we, e);
        let (a, b) = (env("FROM", 800.0), env("TO", 900.0));
        for c in we.corners.iter().filter(|c| c.end > a - 100.0 && c.start < b) {
            eprintln!("corner {:.0}-{:.0} grade {} {:?} {:.0}° crest {} narrows {}", c.start, c.end, c.grade, c.turn, c.angle, c.crest, c.narrows);
        }
        for cr in we.crests.iter().filter(|c| **c > a - 100.0 && **c < b) {
            eprintln!("crest at {cr:.0}");
        }
        let mut pilot = Pilot::new(PilotParams { pace: env("PACE", 0.68), brake: 0.55, ..PilotParams::default() });
        let (p, h) = we.road.start();
        let spec = std::env::var("CAR").ok().and_then(|c| garage::def(&c)).map_or_else(Spec::default, |d| (d.base)());
        let mut car = Car::new(p + Vec3::Y * 0.9, h);
        let mut prog = 0.0f32;
        for k in 0..60 * 900 {
            let i = pilot.input(&car, &spec, &we.road, &we.corners, &we, prog, DT);
            let v0 = car.speed();
            vehicle::step(&mut car, &spec, i, &we, DT);
            if v0 - car.speed() > 4.0 {
                let mut obs = Vec::new();
                we.obstacles(Vec2::new(car.pos.x, car.pos.z), 4.0, &mut obs);
                let mut walls = Vec::new();
                we.walls(Vec2::new(car.pos.x, car.pos.z), 4.0, &mut walls);
                eprintln!("HIT at s {prog:.0} pos {:?}: obstacles {:?} walls {:?}", car.pos, obs.iter().filter(|o| o.pos.distance(Vec2::new(car.pos.x, car.pos.z)) < 4.0).collect::<Vec<_>>(), walls);
            }
            let rp = we.road.nearest(Vec2::new(car.pos.x, car.pos.z), 40.0);
            if let Some(rp) = rp
                && rp.s > prog
                && rp.s < prog + 40.0
            {
                prog = rp.s;
            }
            if prog > a && k % 6 == 0 {
                let st = we.road.station(prog);
                let air = car.wheels.iter().filter(|w| !w.contact).count();
                let beta = car.vel.dot(pilot::left_of(&car)).atan2(car.speed().abs().max(1.0)).to_degrees();
                eprintln!("s {prog:5.0} v {:5.1} km/h curv {:+.4} w {:.1} {:?} lat {:+5.1} β {beta:+5.1} st {:+.2} th {:.2} br {:.2} air {air} grade {:+.2} {}", car.speed() * 3.6, we.road.curv[st], we.road.width[st], we.road.surface[st], rp.map_or(99.0, |r| r.lateral), i.steer, i.throttle, i.brake, (we.road.pts[(st + 2).min(we.road.len() - 1)].y - we.road.pts[st.saturating_sub(2)].y) / 8.0, pilot.doing);
            }
            if prog > b {
                break;
            }
        }
    }

    /// AI-2: two headless races with the same inputs give identical rival poses at step N.
    #[test]
    #[ignore = "slow (loads the open world): cargo test -- --ignored rivals_are_deterministic"]
    fn rivals_are_deterministic() {
        let w = world();
        // AI-3: the start format by width (the Hillclimb is a B-road, Fernhollow a forest track).
        let ev = events::build(&w.open.as_ref().unwrap().net, &w.open.as_ref().unwrap().data.events);
        let fmt = |name: &str| start_format(&ev.iter().find(|e| e.name == name).unwrap().route);
        assert_eq!(fmt("Hillclimb"), StartFormat::Grid);
        assert_eq!(fmt("Fernhollow escarpment"), StartFormat::Pursuit);
        // No car overlaps another at spawn.
        for e in ev.iter().filter(|e| e.kind != events::EventKind::Lap) {
            let mut we = w.clone();
            events::on_route(&mut we, e);
            let sim = RaceSim::new(e.id, Difficulty::Pro, 1, &we.road, &we, 570, DamageMode::Simulation);
            let (p, h) = sim.player_start(&we.road, &we);
            let mut cars: Vec<Car> = sim.rivals.iter().map(|r| r.car.clone()).collect();
            cars.push(Car::new(p + Vec3::Y * 0.9, h));
            for i in 0..cars.len() {
                for j in i + 1..cars.len() {
                    assert!(!touching(&cars[i], &cars[j]), "{}: cars {i} and {j} overlap on the grid", e.name);
                }
            }
        }
        let a = race(&w, events::FIRST_ID + 4, Difficulty::Ace, 60 * 40, Some(60 * 40 - 1));
        let b = race(&w, events::FIRST_ID + 4, Difficulty::Ace, 60 * 40, Some(60 * 40 - 1));
        assert_eq!(a.poses, b.poses);
        eprintln!("{:.1} s", a.secs);
    }
}
