//! The economy: the one place Notes move. Applies `Payout` / `Charge`, handles the UI's
//! `CareerRequest`s (buy, fit, select, paint, number, service, sell), pays races and Time
//! Trials, puts the active car's build on the road (`drive::CarSpec`), and keeps
//! `career.json` (load at startup, debounced save on every change, save on quit).
//!
//! **Owner: stream F (GAMEPLAY).** Rules: `docs/festival/GAME_SYSTEMS.md` §4 and EC-1..EC-7;
//! rulings `docs/festival/DECISIONS.md` E1-E12, R7. Pure rules (purse, shares, fees) are free
//! functions with table tests; the systems are thin.

use bevy::prelude::*;
use engine::flow::AppState;
use engine::paths::DataDir;
use engine::ui::prelude::Toast;

use super::garage::{self, CARS, Garage, OwnedCar, Refusal, UpgradeKind, round100};
use super::grade::Grade;
use super::racers::{Difficulty, Finish, RaceResult};
use super::save::{self, CareerFile, CareerProgress, Loaded};
use super::style::StyleEvent;
use super::wallet::Wallet;
use super::{CAREER_FILE, CareerRequest, Charge, LastPayout, MoneyEarned, PayReason, Payout, notes};
use crate::drive::{CarSpec, DrivenCar, StageRun};
use crate::world::StageWorld;

/// Seconds a change waits before the save is written (more changes in that time join it).
pub const SAVE_DEBOUNCE: f32 = 2.0;

// ---------------------------------------------------------------- rules (pure)

/// The race purse for a route of `km`: `round100(300 + 350 × km)` (GAME_SYSTEMS §4).
pub fn purse(km: f32) -> u32 {
    round100(300.0 + 350.0 * km)
}

/// The share of the purse for finishing `pos` (1-based); 0 past 6th.
pub fn share(pos: u8) -> f32 {
    match pos {
        1 => 1.0,
        2 => 0.55,
        3 => 0.35,
        4 => 0.20,
        5 => 0.10,
        6 => 0.05,
        _ => 0.0,
    }
}

/// The entry fee for a race at `d` on a purse of `p` (Ace 10 %, Legend 25 %; lost on a DNF).
pub fn entry_fee(p: u32, d: Difficulty) -> u32 {
    round100(p as f32 * d.fee_share())
}

/// What a race pays, line by line (the results card), and the total.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RacePay {
    pub lines: Vec<(String, u32)>,
    pub total: u32,
}

/// Race pay: the position's share of the purse × difficulty, +10 % for a clean race (no
/// contact with a rival), plus the first-win bonus (P × difficulty, once per event per
/// difficulty). A DNF (`pos` `None`) pays nothing. The fee was charged at the start.
pub fn race_pay(p: u32, d: Difficulty, pos: Option<u8>, clean: bool, first_win: bool) -> RacePay {
    let Some(pos) = pos else {
        return RacePay { lines: vec![("Retired: no purse".into(), 0)], total: 0 };
    };
    let base = p as f32 * d.mult() * share(pos);
    let with_clean = if clean { base * 1.1 } else { base };
    let mut lines = vec![(format!("P{pos} \u{b7} {:.0} % of {} \u{d7} {} {:.2}", share(pos) * 100.0, notes(p), d.label(), d.mult()), base.round() as u32)];
    if clean && base > 0.0 {
        lines.push(("Clean race +10 %".into(), (with_clean - base).round() as u32));
    }
    let mut total = with_clean.round() as u32;
    if first_win && pos == 1 {
        let bonus = (p as f32 * d.mult()).round() as u32;
        lines.push((format!("First win at {}", d.label()), bonus));
        total += bonus;
    }
    RacePay { lines, total }
}

/// Time Trial pay: 15 % of the purse per finish, +10 % for a new personal best.
pub fn trial_pay(p: u32, personal_best: bool) -> RacePay {
    let finish = (p as f32 * 0.15).round() as u32;
    let mut lines = vec![("Time Trial finish".to_string(), finish)];
    let mut total = finish;
    if personal_best {
        let pb = (p as f32 * 0.10).round() as u32;
        lines.push(("New personal best".into(), pb));
        total += pb;
    }
    RacePay { lines, total }
}

/// A ledger label for a reason.
pub fn label(r: &PayReason) -> String {
    match r {
        PayReason::Rhythm => "Rhythm banked".into(),
        PayReason::ChallengeStar { stars, .. } => format!("Challenge {}", "\u{2605}".repeat(*stars as usize)),
        PayReason::ChallengeBest { .. } => "Challenge best".into(),
        PayReason::Race { pos, difficulty, .. } => format!("Race P{pos} ({})", difficulty.label()),
        PayReason::TimeTrial { .. } => "Time Trial".into(),
        PayReason::Sale { car } => format!("Sold {}", garage::def(car).map_or(*car, |d| d.name)),
        PayReason::Purchase { car } => format!("Bought {}", garage::def(car).map_or(*car, |d| d.name)),
        PayReason::Upgrade { kind, level } => format!("{} {}", kind.name(), garage::level_name(*level)),
        PayReason::EntryFee { .. } => "Entry fee".into(),
        PayReason::Service => "Service".into(),
    }
}

/// Applies a UI request to the career. `Ok(what happened)` for the toast; the wallet is only
/// touched when the whole request can go through. `in_event`: an event run is on (no car
/// swaps then).
pub fn apply_request(req: &CareerRequest, w: &mut Wallet, g: &mut Garage, log: &mut CareerProgress, in_event: bool) -> Result<String, Refusal> {
    match req {
        CareerRequest::BuyCar(id) => {
            let price = g.can_buy(w, id)?;
            let d = garage::def(id).ok_or(Refusal::Unknown)?;
            w.debit(price)?;
            g.owned.push(OwnedCar::stock(d));
            log.log(label(&PayReason::Purchase { car: d.id }), -(price as i64));
            Ok(format!("{} is in your garage", d.name))
        }
        CareerRequest::FitPart { kind, level } => {
            let cost = g.can_fit(w, *kind, *level)?;
            w.debit(cost)?;
            let i = g.active.min(g.owned.len() - 1);
            let car = &mut g.owned[i];
            if *level > car.bought.get(*kind) {
                car.bought.set(*kind, *level);
            }
            car.fitted.set(*kind, *level);
            if cost > 0 {
                log.log(label(&PayReason::Upgrade { kind: *kind, level: *level }), -(cost as i64));
            }
            Ok(format!("{} {} fitted", kind.name(), garage::level_name(*level)))
        }
        CareerRequest::SetActiveCar(i) => {
            if *i >= g.owned.len() {
                return Err(Refusal::Unknown);
            }
            if in_event {
                return Err(Refusal::Busy);
            }
            g.active = *i;
            Ok(format!("Driving the {}", g.active_car().def().map_or("car", |d| d.name)))
        }
        CareerRequest::Paint(p) => {
            let i = g.active.min(g.owned.len() - 1);
            g.owned[i].paint = garage::Paint { colour: p.colour % garage::PAINTS, metallic: p.metallic };
            Ok("Repainted".into())
        }
        CareerRequest::SetNumber(n) => {
            let i = g.active.min(g.owned.len() - 1);
            g.owned[i].number = (*n).clamp(1, 99);
            Ok(format!("Number {}", g.owned[i].number))
        }
        CareerRequest::Service => {
            let cost = g.service_cost();
            w.debit(cost)?;
            let i = g.active.min(g.owned.len() - 1);
            g.owned[i].damage = 0.0;
            if cost > 0 {
                log.log(label(&PayReason::Service), -(cost as i64));
            }
            Ok("Serviced: good as new".into())
        }
        CareerRequest::SellCar(i) => {
            let price = g.can_sell(*i)?;
            let car = g.owned.remove(*i);
            if g.active > *i || g.active >= g.owned.len() {
                g.active = g.active.saturating_sub(1);
            }
            w.credit(price);
            let id = car.def().map_or(garage::SAKER, |d| d.id);
            log.log(label(&PayReason::Sale { car: id }), price as i64);
            Ok(format!("Sold the {} for {} N", car.def().map_or("car", |d| d.name), notes(price)))
        }
    }
}

// ---------------------------------------------------------------- ECS

/// Save bookkeeping.
#[derive(Resource, Default)]
pub struct SaveState {
    /// Seconds since the first unsaved change (`None`: nothing to save).
    pub dirty: Option<f32>,
    /// The file was read (or found missing) at startup; never write before that.
    pub loaded: bool,
    /// Set when the save was corrupt and kept aside: the notice to show once.
    pub notice: Option<String>,
}

/// The last refused request's reason, for E's screens (also toasted).
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct LastRefusal(pub Option<String>);

/// The event being run as a Time Trial was finished and paid (so a run pays once).
#[derive(Resource, Default)]
struct TrialPaid(bool);

pub struct EconomyPlugin;

impl Plugin for EconomyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CareerProgress>()
            .init_resource::<SaveState>()
            .init_resource::<LastRefusal>()
            .init_resource::<TrialPaid>()
            .add_systems(Startup, load_career)
            .add_systems(Update, (apply_money, handle_requests, pay_race, pay_trial, grade_from_style, sync_car_spec, track_car, mark_changes, save_debounced).chain())
            .add_systems(Last, save_on_exit);
        #[cfg(feature = "dev")]
        dev::register(app);
    }
}

fn path(dir: &DataDir) -> std::path::PathBuf {
    dir.file(CAREER_FILE)
}

/// Reads `career.json` into the resources.
pub fn load_career(world: &mut World) {
    let Some(dir) = world.get_resource::<DataDir>().cloned() else { return };
    let f = match save::load(&path(&dir)) {
        Loaded::Ok(f) => f,
        Loaded::Fresh => CareerFile::default(),
        Loaded::Bad(why) => {
            world.resource_mut::<SaveState>().notice = Some(why);
            CareerFile::default()
        }
    };
    info!("career: {} N, {} car(s), grade {}", f.wallet.balance, f.garage.owned.len(), f.grade.level());
    put(world, f);
    world.resource_mut::<SaveState>().loaded = true;
}

/// Replaces the career resources with `f`.
pub fn put(world: &mut World, f: CareerFile) {
    world.insert_resource(f.wallet);
    world.insert_resource(f.garage);
    world.insert_resource(f.grade);
    world.insert_resource(f.progress);
}

/// The career as it stands, as a file.
pub fn snapshot(world: &World) -> CareerFile {
    CareerFile {
        version: save::VERSION,
        wallet: world.resource::<Wallet>().clone(),
        garage: world.resource::<Garage>().clone(),
        grade: *world.resource::<Grade>(),
        progress: world.resource::<CareerProgress>().clone(),
    }
}

/// Writes the save now (no-op before it was loaded).
pub fn save_now(world: &mut World) {
    if !world.resource::<SaveState>().loaded {
        return;
    }
    let Some(dir) = world.get_resource::<DataDir>().cloned() else { return };
    let f = snapshot(world);
    match save::save(&path(&dir), &f) {
        Ok(()) => world.resource_mut::<SaveState>().dirty = None,
        Err(e) => {
            warn!("career save: {e}");
            crate::telemetry::handled(world, &e, "career.save");
            world.resource_mut::<SaveState>().dirty = None;
        }
    }
}

fn mark(save: &mut SaveState) {
    if save.dirty.is_none() {
        save.dirty = Some(0.0);
    }
}

/// `Payout` / `Charge` → the wallet (and the ledger); `MoneyEarned` after a credit.
fn apply_money(mut pay: MessageReader<Payout>, mut charge: MessageReader<Charge>, mut wallet: ResMut<Wallet>, mut progress: ResMut<CareerProgress>, mut save: ResMut<SaveState>, mut earned: MessageWriter<MoneyEarned>) {
    for p in pay.read() {
        if p.amount == 0 {
            continue;
        }
        wallet.credit(p.amount);
        progress.log(label(&p.reason), p.amount as i64);
        earned.write(MoneyEarned { amount: p.amount });
        mark(&mut save);
    }
    for c in charge.read() {
        match wallet.debit(c.amount) {
            Ok(()) => {
                progress.log(label(&c.reason), -(c.amount as i64));
                mark(&mut save);
            }
            Err(r) => warn!("charge refused ({}): {}", label(&c.reason), r.reason()),
        }
    }
}

/// Is an event run on (the clock started, or counting down at the line)?
fn in_event(stage: &Option<Res<StageWorld>>) -> bool {
    stage.as_ref().is_some_and(|s| s.event.is_some() || !s.free_roam() && s.open.is_none())
}

#[allow(clippy::too_many_arguments)]
fn handle_requests(
    mut reqs: MessageReader<CareerRequest>,
    mut wallet: ResMut<Wallet>,
    mut garage: ResMut<Garage>,
    mut progress: ResMut<CareerProgress>,
    mut save: ResMut<SaveState>,
    mut refusal: ResMut<LastRefusal>,
    mut toast: MessageWriter<Toast>,
    stage: Option<Res<StageWorld>>,
    state: Option<Res<State<AppState>>>,
) {
    let busy = state.is_some_and(|s| *s.get() == AppState::InGame) && in_event(&stage);
    for r in reqs.read() {
        match apply_request(r, &mut wallet, &mut garage, &mut progress, busy) {
            Ok(msg) => {
                refusal.0 = None;
                info!("career: {msg}");
                toast.write(Toast::new(msg).secs(2.5));
                mark(&mut save);
            }
            Err(why) => {
                let text = why.reason();
                info!("career: refused {r:?}: {text}");
                toast.write(Toast::new("Can't do that").body(text.clone()));
                refusal.0 = Some(text);
            }
        }
    }
}

/// The purse of event `id` (from its route length), if it is loaded.
pub fn event_purse(world_events: Option<&crate::open::events::Events>, id: u32) -> Option<u32> {
    world_events?.0.iter().find(|e| e.id == id).map(|e| purse(e.route.length() / 1000.0))
}

/// `RaceResult` → the purse, the first-win bonus, the clean bonus; records the best finish.
#[allow(clippy::too_many_arguments)]
fn pay_race(
    mut results: MessageReader<RaceResult>,
    events: Option<Res<crate::open::events::Events>>,
    mut progress: ResMut<CareerProgress>,
    mut last: ResMut<LastPayout>,
    mut pay: MessageWriter<Payout>,
    mut save: ResMut<SaveState>,
    fee: Option<Res<super::race_flow::FeePaid>>,
) {
    for r in results.read() {
        let p = event_purse(events.as_deref(), r.event).unwrap_or(1_000);
        let pos = r.order.iter().find(|(w, _)| *w == super::racers::Who::Player).and_then(|(_, f)| matches!(f, Finish::Time(_)).then_some(r.player_pos));
        let rec = progress.races.entry(r.event).or_default();
        let first_win = pos == Some(1) && !rec.won(r.difficulty);
        if let Some(pos) = pos {
            let best = rec.best_mut(r.difficulty);
            if *best == 0 || pos < *best {
                *best = pos;
            }
        }
        progress.stats.races += 1;
        if pos == Some(1) {
            progress.stats.wins += 1;
        }
        let mut out = race_pay(p, r.difficulty, pos, r.clean, first_win);
        if let Some(f) = fee.as_deref().filter(|f| f.amount > 0) {
            out.lines.push((format!("Entry fee (paid at the start) \u{2212}{} N", notes(f.amount)), 0));
        }
        info!("race pay: event {} {:?} P{:?} clean {} first win {}: {} N", r.event, r.difficulty, pos, r.clean, first_win, out.total);
        last.0 = out.lines.clone();
        last.0.push(("Total".into(), out.total));
        if out.total > 0 {
            pay.write(Payout { amount: out.total, reason: PayReason::Race { event: r.event, pos: pos.unwrap_or(0), difficulty: r.difficulty } });
        }
        mark(&mut save);
    }
}

/// A finished Time Trial in an event pays 15 % of its purse (+10 % for a team best).
fn pay_trial(
    run: Option<Res<StageRun>>,
    stage: Option<Res<StageWorld>>,
    events: Option<Res<crate::open::events::Events>>,
    result: Option<Res<crate::meta::LastResult>>,
    field: Option<Res<super::racers::RaceField>>,
    mut paid: ResMut<TrialPaid>,
    mut last: ResMut<LastPayout>,
    mut pay: MessageWriter<Payout>,
) {
    let finished = run.as_ref().is_some_and(|r| r.finished.is_some());
    if !finished {
        paid.0 = false;
        return;
    }
    let Some(event) = stage.as_ref().and_then(|s| s.event.clone()) else { return };
    if paid.0 || field.is_some() {
        return;
    }
    // Wait for the board write (`meta::finish_run`) so a personal best is known.
    let Some(result) = result.filter(|r| r.is_changed() && r.record.stage == event.id) else { return };
    paid.0 = true;
    let p = event_purse(events.as_deref(), event.id).unwrap_or(1_000);
    let out = trial_pay(p, result.inserted.team_best);
    last.0 = out.lines.clone();
    last.0.push(("Total".into(), out.total));
    pay.write(Payout { amount: out.total, reason: PayReason::TimeTrial { event: event.id } });
}

/// Banked Style is grade xp 1:1 (DECISIONS S5), and feeds the Logbook stats.
fn grade_from_style(mut style: MessageReader<StyleEvent>, mut grade: ResMut<Grade>, mut progress: ResMut<CareerProgress>, mut save: ResMut<SaveState>) {
    for e in style.read() {
        if let StyleEvent::Bank { points, .. } = *e {
            grade.xp = grade.xp.saturating_add(points);
            progress.stats.style_banked += points as u64;
            progress.stats.best_rhythm = progress.stats.best_rhythm.max(points);
            mark(&mut save);
        }
    }
}

/// The active car's build → `CarSpec` (keeping the assists and damage mode `prefs` set), but
/// only when no run is under way (menus, loading, free roam, or on the line before the clock
/// runs), so a car never changes mid-stage. Also when the garage changes.
#[allow(clippy::too_many_arguments)]
fn sync_car_spec(
    garage: Res<Garage>,
    spec: Option<ResMut<CarSpec>>,
    state: Option<Res<State<AppState>>>,
    run: Option<Res<StageRun>>,
    stage: Option<Res<StageWorld>>,
    loading: Option<Res<crate::loading::Loading>>,
    mut applied: Local<Option<String>>,
) {
    let Some(mut spec) = spec else { return };
    let key = garage::car_key(garage.active_car());
    if applied.as_deref() == Some(key.as_str()) {
        return;
    }
    let in_game = state.is_some_and(|s| *s.get() == AppState::InGame);
    let on_the_line = run.as_ref().is_some_and(|r| r.countdown > 0.0 && r.time == 0.0);
    let roaming = stage.as_ref().is_some_and(|s| s.free_roam());
    if in_game && loading.is_none() && !on_the_line && !roaming {
        return;
    }
    let mut s = garage::spec_of(garage.active_car());
    s.abs = spec.0.abs;
    s.traction_assist = spec.0.traction_assist;
    s.damage = spec.0.damage;
    info!("car: {key} (PI {})", garage::pi(&s));
    spec.0 = s;
    *applied = Some(key);
}

/// Free roam: the driven car's damage is the owned car's (Service repairs it), and its pose is
/// remembered for Continue. On a fresh spawn in free roam the stored damage comes back.
fn track_car(
    mut cars: Query<&mut DrivenCar>,
    added: Query<(), Added<DrivenCar>>,
    stage: Option<Res<StageWorld>>,
    mut garage: ResMut<Garage>,
    mut progress: ResMut<CareerProgress>,
    mut save: ResMut<SaveState>,
    time: Res<Time>,
    mut every: Local<f32>,
) {
    if !stage.as_ref().is_some_and(|s| s.free_roam()) {
        return;
    }
    let Ok(mut c) = cars.single_mut() else { return };
    let i = garage.active.min(garage.owned.len() - 1);
    if !added.is_empty() {
        let d = garage.owned[i].damage;
        if d > 0.0 {
            let dm = &mut c.car.damage;
            dm.engine = dm.engine.max(d);
            dm.steering = dm.steering.max(d);
            dm.suspension = dm.suspension.max(d);
        }
        return;
    }
    // A Service in free roam repairs the car on the road too.
    if garage.is_changed() && garage.owned[i].damage == 0.0 && c.car.damage.total() > 0.0 {
        crate::vehicle::repair(&mut c.car);
    }
    let d = (c.car.damage.total() * 100.0).round() / 100.0;
    if (garage.owned[i].damage - d).abs() > 0.009 {
        garage.owned[i].damage = d;
        mark(&mut save);
    }
    *every += time.delta_secs();
    if *every > 5.0 {
        *every = 0.0;
        let heading = heading_of(c.car.forward());
        progress.bypass_change_detection().world = Some(save::WorldPose { pos: c.car.pos.to_array(), heading });
    }
}

/// Any change to the career's resources (from here or another stream) gets saved.
fn mark_changes(wallet: Res<Wallet>, garage: Res<Garage>, grade: Res<Grade>, progress: Res<CareerProgress>, mut save: ResMut<SaveState>) {
    let changed = |c: bool, a: bool| c && !a;
    if changed(wallet.is_changed(), wallet.is_added()) || changed(garage.is_changed(), garage.is_added()) || changed(grade.is_changed(), grade.is_added()) || changed(progress.is_changed(), progress.is_added()) {
        mark(&mut save);
    }
}

fn save_debounced(world: &mut World) {
    let dt = world.resource::<Time>().delta_secs();
    let due = {
        let mut s = world.resource_mut::<SaveState>();
        match s.dirty.as_mut() {
            Some(t) => {
                *t += dt;
                *t >= SAVE_DEBOUNCE
            }
            None => false,
        }
    };
    if let Some(why) = world.resource_mut::<SaveState>().notice.take() {
        world.write_message(Toast::new("Career save couldn't be read").body(format!("{why}. It was kept as {} and a new career started.", save::BAD_FILE)).secs(8.0));
    }
    if due {
        save_now(world);
    }
}

fn save_on_exit(world: &mut World) {
    let quitting = world.get_resource::<Messages<AppExit>>().is_some_and(|m| !m.is_empty());
    if quitting {
        save_now(world);
    }
}

/// The car's pose heading as `Car::new` takes it, from a forward vector.
pub fn heading_of(fwd: Vec3) -> f32 {
    libm::atan2f(fwd.x, fwd.z)
}

#[cfg(feature = "dev")]
mod dev {
    use engine::devtools::{AddControlCommand, params};
    use serde_json::{Value, json};

    use super::*;

    pub fn register(app: &mut App) {
        app.add_control_command("career.state", "Career: wallet, garage (active car, PI), grade, races", params(&[]), |w, _| Ok(state(w)))
            .add_control_command(
                "career.request",
                "Send a CareerRequest as the UI would",
                params(&[("buy", "string", "car id"), ("fit", "string", "part name"), ("level", "integer", "0-3"), ("select", "integer", "garage index"), ("sell", "integer", "garage index"), ("service", "boolean", ""), ("paint", "integer", "0-6")]),
                |w, v: Value| {
                    let req = if let Some(id) = v.get("buy").and_then(Value::as_str) {
                        CareerRequest::BuyCar(id.into())
                    } else if let Some(p) = v.get("fit").and_then(Value::as_str) {
                        let kind = UpgradeKind::ALL.into_iter().find(|k| k.name().eq_ignore_ascii_case(p)).ok_or("fit: a part name")?;
                        CareerRequest::FitPart { kind, level: v.get("level").and_then(Value::as_u64).unwrap_or(1) as u8 }
                    } else if let Some(i) = v.get("select").and_then(Value::as_u64) {
                        CareerRequest::SetActiveCar(i as usize)
                    } else if let Some(i) = v.get("sell").and_then(Value::as_u64) {
                        CareerRequest::SellCar(i as usize)
                    } else if v.get("service").is_some() {
                        CareerRequest::Service
                    } else if let Some(c) = v.get("paint").and_then(Value::as_u64) {
                        CareerRequest::Paint(garage::Paint { colour: c as u8, metallic: v.get("metallic").and_then(Value::as_bool).unwrap_or(false) })
                    } else {
                        return Err("one of buy / fit / select / sell / service / paint".into());
                    };
                    w.write_message(req);
                    Ok(json!({"sent": true}))
                },
            )
            .add_control_command("career.give", "Dev: add Notes", params(&[("notes", "integer", "amount")]), |w, v: Value| {
                let n = v.get("notes").and_then(Value::as_u64).unwrap_or(10_000) as u32;
                w.write_message(Payout { amount: n, reason: PayReason::Rhythm });
                Ok(json!({"notes": n}))
            });
    }

    fn state(w: &mut World) -> Value {
        let wallet = w.resource::<Wallet>().clone();
        let g = w.resource::<Garage>().clone();
        let cars: Vec<Value> = g
            .owned
            .iter()
            .map(|c| {
                let s = garage::spec_of(c);
                json!({"model": c.model, "key": garage::car_key(c), "pi": garage::pi(&s), "class": garage::Class::of(garage::pi(&s)).letter(), "damage": c.damage, "paint": [c.paint.colour, c.paint.metallic]})
            })
            .collect();
        let p = w.resource::<CareerProgress>().clone();
        let spec = w.get_resource::<CarSpec>().map(|s| garage::pi(&s.0));
        json!({
            "balance": wallet.balance, "earned": wallet.earned, "spent": wallet.spent,
            "active": g.active, "cars": cars, "car_spec_pi": spec,
            "grade": w.resource::<Grade>().level(), "xp": w.resource::<Grade>().xp,
            "races": p.races.iter().map(|(k, r)| (k.to_string(), json!([r.rookie, r.pro, r.ace, r.legend]))).collect::<serde_json::Map<_, _>>(),
            "ledger": p.ledger.iter().rev().take(10).map(|l| json!([l.what, l.amount])).collect::<Vec<_>>(),
            "last_payout": w.resource::<LastPayout>().0,
            "roster": CARS.iter().map(|d| d.id).collect::<Vec<_>>(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::career::garage::{Paint, def};

    #[test]
    fn purses_match_the_doc() {
        // GAME_SYSTEMS §4: River crossings 1,300, Hillclimb / Quarry 2,100, Fernhollow 2,700,
        // Blackpine 4,800 (from their km).
        assert_eq!(purse(2.86), 1_300);
        assert_eq!(purse(5.1), 2_100);
        assert_eq!(purse(6.9), 2_700);
        assert_eq!(purse(12.9), 4_800);
        assert_eq!(entry_fee(4_800, Difficulty::Legend), 1_200);
        assert_eq!(entry_fee(4_800, Difficulty::Ace), 500);
        assert_eq!(entry_fee(4_800, Difficulty::Pro), 0);
    }

    #[test]
    fn race_pay_table() {
        // EC-7's example: Blackpine, Legend, P2, clean = 4,800 × 2.5 × 0.55 × 1.1 = 7,260.
        assert_eq!(race_pay(4_800, Difficulty::Legend, Some(2), true, false).total, 7_260);
        // Each row of the shares at Pro, not clean.
        for (pos, want) in [(1, 2_100), (2, 1_155), (3, 735), (4, 420), (5, 210), (6, 105)] {
            assert_eq!(race_pay(2_100, Difficulty::Pro, Some(pos), false, false).total, want, "P{pos}");
        }
        // Difficulty ×.
        assert_eq!(race_pay(2_100, Difficulty::Rookie, Some(1), false, false).total, 1_575);
        assert_eq!(race_pay(2_100, Difficulty::Ace, Some(1), false, false).total, 3_150);
        // First win: + P × difficulty, only for a win.
        assert_eq!(race_pay(2_100, Difficulty::Pro, Some(1), false, true).total, 4_200);
        assert_eq!(race_pay(2_100, Difficulty::Pro, Some(2), false, true).total, 1_155);
        // DNF pays nothing.
        assert_eq!(race_pay(4_800, Difficulty::Legend, None, true, true).total, 0);
        // Time Trial: 15 %, +10 % for a personal best.
        assert_eq!(trial_pay(2_100, false).total, 315);
        assert_eq!(trial_pay(2_100, true).total, 525);
    }

    #[test]
    fn requests_buy_fit_select_sell_service() {
        let (mut w, mut g, mut log) = (Wallet::default(), Garage::default(), CareerProgress::default());
        // Too little money: refused, nothing changes.
        assert_eq!(apply_request(&CareerRequest::BuyCar("vale".into()), &mut w, &mut g, &mut log, false), Err(Refusal::Notes { need: 18_000 }));
        assert_eq!((w.balance, g.owned.len()), (12_000, 1));
        // Buy the Wren: debited, in the garage, not active.
        apply_request(&CareerRequest::BuyCar("wren".into()), &mut w, &mut g, &mut log, false).unwrap();
        assert_eq!((w.balance, g.owned.len(), g.active), (3_000, 2, 0));
        assert_eq!(apply_request(&CareerRequest::BuyCar("wren".into()), &mut w, &mut g, &mut log, false), Err(Refusal::Owned));
        // Fit Engine Gravel on the Saker: buys L1 + L2 (10,000): refused.
        assert_eq!(apply_request(&CareerRequest::FitPart { kind: UpgradeKind::Engine, level: 2 }, &mut w, &mut g, &mut log, false), Err(Refusal::Notes { need: 7_000 }));
        // Brakes Street (1,500), then back to stock (free), then Street again (free: owned).
        apply_request(&CareerRequest::FitPart { kind: UpgradeKind::Brakes, level: 1 }, &mut w, &mut g, &mut log, false).unwrap();
        apply_request(&CareerRequest::FitPart { kind: UpgradeKind::Brakes, level: 0 }, &mut w, &mut g, &mut log, false).unwrap();
        apply_request(&CareerRequest::FitPart { kind: UpgradeKind::Brakes, level: 1 }, &mut w, &mut g, &mut log, false).unwrap();
        assert_eq!(w.balance, 1_500);
        assert_eq!((g.owned[0].fitted.brakes, g.owned[0].bought.brakes), (1, 1));
        // Swap cars: not during an event.
        assert_eq!(apply_request(&CareerRequest::SetActiveCar(1), &mut w, &mut g, &mut log, true), Err(Refusal::Busy));
        apply_request(&CareerRequest::SetActiveCar(1), &mut w, &mut g, &mut log, false).unwrap();
        assert_eq!(g.active_car().model, "wren");
        apply_request(&CareerRequest::Paint(Paint { colour: 9, metallic: true }), &mut w, &mut g, &mut log, false).unwrap();
        assert_eq!(g.active_car().paint, Paint { colour: 2, metallic: true });
        // Service: 1,500 × 0.4 × 0.6 (class D) = 400.
        g.owned[1].damage = 0.4;
        apply_request(&CareerRequest::Service, &mut w, &mut g, &mut log, false).unwrap();
        assert_eq!((w.balance, g.owned[1].damage), (1_100, 0.0));
        // Sell the Saker (60 % of 45k + half its 1,500 brakes); the active Wren stays active.
        apply_request(&CareerRequest::SellCar(0), &mut w, &mut g, &mut log, false).unwrap();
        assert_eq!(w.balance, 1_100 + 27_800);
        assert_eq!((g.owned.len(), g.active_car().model.as_str()), (1, "wren"));
        assert_eq!(apply_request(&CareerRequest::SellCar(0), &mut w, &mut g, &mut log, false), Err(Refusal::LastCar));
        assert_eq!(w.balance, w.earned + 12_000 - w.spent, "the books balance");
        assert!(log.ledger.len() >= 5);
        let _ = def("wren");
    }
}
