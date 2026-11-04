//! The race in the game: starts a [`RaceSim`] when an event is entered in Race mode, steps
//! the rivals each fixed step after the player, holds the player at a pursuit start and for 5 s
//! after a Recover, publishes the rivals as solids and in [`OtherCars`], sends the
//! [`RaceResult`] when the player's race ends, and draws the rival cars.
//!
//! **Owner: stream F (GAMEPLAY).** Design: `docs/festival/GAME_SYSTEMS.md` §5, AI-2..AI-8;
//! rulings `docs/festival/DECISIONS.md` R1-R7. Races record no ghost and no board time
//! (`meta::watch_finish` skips them).

use std::sync::Arc;

use bevy::prelude::*;
use engine::flow::AppState;
use engine::ui::prelude::Toast;

use super::garage;
use super::racers::{Difficulty, Entrant, Finish, OtherCar, OtherCars, OtherKind, PlayerNow, RaceField, RaceResult, RaceSim, RacerSolids, Who};
use super::wallet::Wallet;
use super::{Charge, PayReason, notes};
use crate::drive::{AutoInput, CarSpec, DrivenCar, SimulateSet, StageRun};
use crate::vehicle::{Car, Input};
use crate::world::StageWorld;

/// A race in progress (absent outside Race runs).
#[derive(Resource)]
pub struct RaceRun {
    pub sim: RaceSim,
    /// The world as the rivals see it: the event's route, no race solids (each rival adds the
    /// others itself).
    pub ground: StageWorld,
    /// Shared with `StageWorld::racers`: the player's view of the rivals.
    pub solids: Arc<RacerSolids>,
    /// The player's PI and damage mode at the start (a Restart rebuilds the same field).
    pub player_pi: u16,
    /// Steps the player is still held after a Recover; the penalty seen last (a Recover adds
    /// 5 s to it in `drive`).
    pub hold: u32,
    last_penalty: f32,
    /// This race set `AutoInput` (and must clear it).
    holding: bool,
    /// The result went out for this run.
    pub done: bool,
    /// The run was placed on the grid (a fresh `StageRun` re-places it: Restart, Retry).
    placed: bool,
    /// Rival race time / progress of the step before, for the HUD's live gaps.
    pub player_progress: f32,
}

/// The entry fee charged for the race in progress (Ace / Legend), so a Restart doesn't charge
/// it again and the results card can show it.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct FeePaid {
    pub event: u32,
    pub amount: u32,
}

/// The last race's result (the race HUD's results card).
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct LastRace(pub RaceResult);

/// A rival's car on screen.
#[derive(Component)]
pub struct RivalBody(pub u8);

/// Fresh entries into races this session (part of the seed: a Retry is the same race, a new
/// entry from the marker another).
#[derive(Resource, Default)]
struct Attempts(u32);

pub struct RaceFlowPlugin;

impl Plugin for RaceFlowPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Attempts>()
            .add_systems(FixedUpdate, race_pre.before(SimulateSet).run_if(resource_exists::<RaceRun>))
            .add_systems(FixedUpdate, (race_step.run_if(resource_exists::<RaceRun>), other_cars).chain().after(SimulateSet))
            .add_systems(Update, (draw_rivals, no_ghosts_in_races).run_if(resource_exists::<RaceRun>))
            .add_systems(Update, despawn_rivals.run_if(not(resource_exists::<RaceRun>)))
            .add_systems(OnExit(AppState::InGame), |w: &mut World| end(w));
        #[cfg(feature = "dev")]
        app.configure_sets(FixedUpdate, crate::autopilot::AutopilotSet.before(race_pre));
    }
}

/// Starts a race on the event now loaded as the stage's route (`StageWorld::event`): charges
/// the fee (refusing the race when the balance is short), builds the field and hooks the
/// rivals' solids into the world. Returns false when refused.
pub fn begin(world: &mut World, difficulty: Difficulty) -> bool {
    let Some(stage) = world.get_resource::<StageWorld>().cloned() else { return false };
    let Some(event) = stage.event.clone() else { return false };
    let purse = super::economy::purse(stage.road.length() / 1000.0);
    let fee = super::economy::entry_fee(purse, difficulty);
    if fee > 0 {
        let have = world.resource::<Wallet>().balance;
        if have < fee {
            world.write_message(Toast::new("Not enough Notes").body(format!("The {} entry fee is {} N: {} N short", difficulty.label(), notes(fee), notes(fee - have))));
            return false;
        }
        world.write_message(Charge { amount: fee, reason: PayReason::EntryFee { event: event.id } });
    }
    world.insert_resource(FeePaid { event: event.id, amount: fee });
    let attempt = {
        let mut a = world.resource_mut::<Attempts>();
        a.0 += 1;
        a.0
    };
    let spec = world.get_resource::<CarSpec>().map(|s| s.0.clone()).unwrap_or_default();
    let player_pi = garage::pi(&spec);
    let mut ground = stage.clone();
    ground.racers = None;
    let sim = RaceSim::new(event.id, difficulty, attempt, &ground.road, &ground, player_pi, spec.damage);
    info!("race: event {} at {} ({:?} start), player slot {} PI {}, fee {} N", event.id, difficulty.label(), sim.format, sim.player_slot, player_pi, fee);
    let solids = Arc::new(RacerSolids::default());
    world.resource_mut::<StageWorld>().racers = Some(solids.clone());
    world.insert_resource(field_of(&sim, event.id, world.resource::<super::garage::Garage>().active_car().model.clone()));
    world.insert_resource(RaceRun { sim, ground, solids, player_pi, hold: 0, last_penalty: 0.0, holding: false, done: false, placed: false, player_progress: 0.0 });
    world.remove_resource::<LastRace>();
    true
}

/// Ends the race (leaving the event, a Time Trial, leaving the game).
pub fn end(world: &mut World) {
    if world.remove_resource::<RaceRun>().is_some() {
        if let Some(mut a) = world.get_resource_mut::<AutoInput>() {
            a.0 = None;
        }
        info!("race: over");
    }
    world.remove_resource::<RaceField>();
    world.remove_resource::<FeePaid>();
    if let Some(mut s) = world.get_resource_mut::<StageWorld>()
        && s.racers.is_some()
    {
        s.racers = None;
    }
    if let Some(mut o) = world.get_resource_mut::<OtherCars>() {
        o.0.retain(|c| c.kind == OtherKind::Traffic);
    }
}

fn field_of(sim: &RaceSim, event: u32, player_car: String) -> RaceField {
    let mut entrants: Vec<Entrant> = sim.rivals.iter().map(|r| Entrant { who: Who::Rival(r.slot), name: r.name.into(), car: r.model.into(), start_step: r.release, finish: None }).collect();
    entrants.push(Entrant { who: Who::Player, name: "You".into(), car: player_car, start_step: sim.player_release, finish: None });
    entrants.sort_by_key(|e| match e.who {
        Who::Player => sim.player_slot,
        Who::Rival(s) => s,
    });
    RaceField { event, difficulty: sim.difficulty, format: sim.format, entrants }
}

/// Before the player's step: a fresh run (start, Restart, Retry) goes back on the grid; the
/// player is held at a pursuit start and for 5 s after a Recover.
fn race_pre(mut race: ResMut<RaceRun>, run: Res<StageRun>, mut cars: Query<&mut DrivenCar>, mut auto: ResMut<AutoInput>, spec: Option<Res<CarSpec>>, mut field: Option<ResMut<RaceField>>) {
    let race = &mut *race;
    let fresh = run.countdown >= 3.0 && run.time == 0.0 && run.finished.is_none();
    if fresh && (!race.placed || race.sim.started()) {
        // The same field as before (same seed): a Retry is the same race.
        let damage = spec.map_or(crate::vehicle::DamageMode::Simulation, |s| s.0.damage);
        race.sim = RaceSim::new(race.sim.event, race.sim.difficulty, race.sim.attempt, &race.ground.road, &race.ground, race.player_pi, damage);
        let (pos, heading) = race.sim.player_start(&race.ground.road, &race.ground);
        for mut c in &mut cars {
            c.car = Car::new(pos + Vec3::Y * 0.9, heading);
            c.prev = (c.car.pos, c.car.rot);
        }
        race.placed = true;
        race.done = false;
        race.hold = 0;
        race.last_penalty = 0.0;
        if let Some(f) = field.as_deref_mut() {
            for e in &mut f.entrants {
                e.finish = None;
            }
        }
    }
    // A Recover (drive adds +5 s to the penalty): held for 5 s instead (the clock is shared).
    if run.penalty > race.last_penalty {
        race.last_penalty = run.penalty;
        race.hold = super::racers::RECOVER_HOLD;
    }
    let hold_start = run.countdown <= 0.0 && race.sim.player_held();
    let hold = hold_start || race.hold > 0;
    race.hold = race.hold.saturating_sub(1);
    if hold {
        auto.0 = Some(Input { handbrake: true, ..Input::default() });
        race.holding = true;
    } else if race.holding {
        auto.0 = None;
        race.holding = false;
    }
}

/// After the player's step: every rival steps; solids, field and result go out.
fn race_step(world: &mut World) {
    if crate::loading::is_loading(world) {
        return;
    }
    let Some(run) = world.get_resource::<StageRun>().cloned() else { return };
    let mut q = world.query::<&DrivenCar>();
    let Some(player) = q.iter(world).next().map(|c| c.car.clone()) else { return };
    let Some(stage) = world.get_resource::<StageWorld>() else { return };
    let (road, corners) = (stage.road.clone(), stage.corners.clone());
    let t0 = std::time::Instant::now();
    let mut race = world.remove_resource::<RaceRun>().unwrap();
    let done = run.finished.is_some() || run.retired;
    race.sim.step(&race.ground, &road, &corners, PlayerNow { car: &player, progress: run.progress, done }, run.countdown > 0.0);
    race.player_progress = run.progress;
    if let Ok(mut v) = race.solids.0.write() {
        v.clear();
        v.extend(race.sim.rivals.iter().filter(|r| r.ghost <= 0.0).map(|r| super::racers::solid_pose(&r.car)));
    }
    let took = t0.elapsed().as_secs_f32() * 1000.0;
    if let Some(mut perf) = world.get_resource_mut::<RacePerf>() {
        perf.add(took);
    } else {
        world.insert_resource(RacePerf::default());
    }
    // The field's finishes (rivals as they cross; the player's when the result goes out).
    if let Some(mut f) = world.get_resource_mut::<RaceField>() {
        for r in &race.sim.rivals {
            if let Some(e) = f.entrants.iter_mut().find(|e| e.who == Who::Rival(r.slot))
                && e.finish != r.finish
            {
                e.finish = r.finish;
            }
        }
    }
    if done && !race.done {
        race.done = true;
        let time = if run.retired { None } else { run.finished.map(|t| t - run.penalty) };
        let result = race.sim.result(time);
        info!("race result: P{} of 6 ({}), clean {}, contacts {}", result.player_pos, time.map_or("DNF".into(), crate::meta::board::clock), result.clean, race.sim.contacts);
        if let Some(mut f) = world.get_resource_mut::<RaceField>() {
            for (who, fin) in &result.order {
                if let Some(e) = f.entrants.iter_mut().find(|e| e.who == *who) {
                    e.finish = Some(*fin);
                }
            }
        }
        world.insert_resource(LastRace(result.clone()));
        world.write_message(result);
    }
    world.insert_resource(race);
}

/// Time the rivals' step took (ms): the last and a running mean, for `perf` and the dev state.
#[derive(Resource, Default, Clone, Copy, Debug)]
pub struct RacePerf {
    pub last_ms: f32,
    pub mean_ms: f32,
    pub max_ms: f32,
    pub steps: u32,
}

impl RacePerf {
    fn add(&mut self, ms: f32) {
        self.last_ms = ms;
        self.steps += 1;
        self.mean_ms += (ms - self.mean_ms) / self.steps.min(600) as f32;
        self.max_ms = self.max_ms.max(ms);
    }
}

/// `OtherCars`: the traffic (free roam) and the rivals (races), rebuilt each fixed step.
fn other_cars(mut out: ResMut<OtherCars>, race: Option<Res<RaceRun>>, traffic: Option<Res<crate::open::traffic::Traffic>>) {
    out.0.clear();
    if let Some(t) = traffic {
        out.0.extend(t.cars().map(|c| OtherCar { id: c.id, kind: OtherKind::Traffic, pos: c.pos, fwd: c.fwd, speed: c.speed }));
    }
    if let Some(r) = race {
        out.0.extend(r.sim.rivals.iter().map(|r| OtherCar { id: 1_000_000 + r.slot as u32, kind: OtherKind::Rival(r.slot), pos: r.car.pos, fwd: r.car.forward(), speed: r.car.speed() }));
    }
}

/// An id for `art::traffic_car::spawn_parts` that picks body `shape` and the metallic paint
/// or not (it hashes the id to choose them).
fn art_id(shape: usize, metallic: bool) -> u32 {
    (0u32..).find(|&id| crate::art::traffic_car::shape_of(id) == shape && (((id >> 3) & 1) == 1) == metallic).unwrap_or(0)
}

/// The traffic body shape for a roster body (hatch, saloon, estate, SUV; the rally hatch uses
/// the hatch).
pub fn body_shape(b: garage::Body) -> usize {
    match b {
        garage::Body::Rally | garage::Body::Hatch => 0,
        garage::Body::Saloon => 1,
        garage::Body::Estate => 2,
        garage::Body::Suv => 3,
    }
}

/// Draws the rivals: traffic car bodies in their roster shape and paint, interpolated
/// between fixed steps.
#[allow(clippy::too_many_arguments)]
fn draw_rivals(
    mut commands: Commands,
    fixed: Res<Time<Fixed>>,
    race: Res<RaceRun>,
    art: Option<Res<crate::art::traffic_car::TrafficArt>>,
    meshes: Option<ResMut<Assets<Mesh>>>,
    mats: Option<ResMut<Assets<StandardMaterial>>>,
    mut bodies: Query<(&RivalBody, &mut Transform)>,
) {
    let (Some(mut meshes), Some(mut mats)) = (meshes, mats) else { return };
    let Some(art) = art else {
        commands.insert_resource(crate::art::traffic_car::TrafficArt::new(&mut meshes, &mut mats));
        return;
    };
    let a = fixed.overstep_fraction();
    // The body's origin is on the ground; the sim's is the centre of mass.
    let pose = |r: &super::racers::Racer| Transform::from_translation(r.prev.0.lerp(r.car.pos, a) - r.car.up() * 0.47).with_rotation(r.prev.1.slerp(r.car.rot, a));
    let mut have = [false; 7];
    for (b, mut tf) in &mut bodies {
        if let Some(r) = race.sim.rivals.iter().find(|r| r.slot == b.0) {
            *tf = pose(r);
            have[b.0 as usize % 7] = true;
        }
    }
    for r in race.sim.rivals.iter().filter(|r| !have[r.slot as usize % 7]) {
        let shape = garage::def(r.model).map_or(0, |d| body_shape(d.body));
        let id = art_id(shape, r.paint.metallic);
        commands
            .spawn((Name::new(format!("rival {}", r.name)), RivalBody(r.slot), pose(r), Visibility::default(), engine::render_budget::Actor, DespawnOnExit(AppState::InGame)))
            .with_children(|p| crate::art::traffic_car::spawn_parts(p, &art, id, r.paint.colour));
    }
}

fn despawn_rivals(mut commands: Commands, q: Query<Entity, With<RivalBody>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

/// No ghost car in a race (the rivals are the opposition).
fn no_ghosts_in_races(mut commands: Commands, q: Query<Entity, With<crate::meta::GhostCar>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

/// The race's live standings for the HUD: position (1-based) of the player, and per entrant
/// (who, name, progress, finish) first to last.
pub fn standings(race: &RaceRun, run: &StageRun) -> Vec<(Who, f32, Option<Finish>)> {
    let pf = if run.retired {
        Some(Finish::Dnf)
    } else {
        run.finished.map(|t| Finish::Time(t - run.penalty))
    };
    race.sim.standings(run.progress, pf)
}
