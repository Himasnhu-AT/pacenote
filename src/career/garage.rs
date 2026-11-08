//! The roster, upgrades, performance index and the player's garage. Pure (no ECS besides the
//! `Garage` resource derive), so the Dealership, the AI field and the challenge bake all use
//! the same numbers.
//!
//! **Owner: stream F (GAMEPLAY).** Design: `docs/festival/GAME_SYSTEMS.md` §4.1-§4.3 (roster,
//! upgrades, PI) and EC-2..EC-6; rulings `docs/festival/DECISIONS.md` E1-E12.
//!
//! Eight cars on one sim ([`CARS`]: a stock `Spec` each), seven upgradeable parts that scale
//! that `Spec` ([`build_spec`]), the performance index and class ([`pi`], [`Class`]), and the
//! player's [`Garage`] with the checks the Dealership and Upgrades screens grey buttons out
//! with (`can_*`). Upgrade prices go by the car's *stock* class, so they don't jump when a
//! build crosses into the next class.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::wallet::Wallet;
use crate::vehicle::Spec;

/// Body shape: the rally hatch is `assets/models/rallycar.glb`, the rest are
/// `art::traffic_car` shapes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Body {
    Rally,
    Hatch,
    Saloon,
    Estate,
    Suv,
}

/// Car class from the PI (GAME_SYSTEMS §4.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Class {
    D,
    C,
    B,
    A,
    S,
}

impl Class {
    /// D < 400 · C 400-499 · B 500-599 · A 600-699 · S ≥ 700.
    pub fn of(pi: u16) -> Class {
        match pi {
            0..=399 => Class::D,
            400..=499 => Class::C,
            500..=599 => Class::B,
            600..=699 => Class::A,
            _ => Class::S,
        }
    }
    /// Upgrade price factor.
    pub fn factor(self) -> f32 {
        match self {
            Class::D => 0.6,
            Class::C => 0.8,
            Class::B => 1.0,
            Class::A => 1.3,
            Class::S => 1.6,
        }
    }
    pub fn letter(self) -> &'static str {
        match self {
            Class::D => "D",
            Class::C => "C",
            Class::B => "B",
            Class::A => "A",
            Class::S => "S",
        }
    }
}

/// How the car pops on a lift (AUDIO_PASS §2.1, DECISIONS D2): `Rally` doubles the burst and
/// makes a Bang likelier. A fitted Turbo also selects `Rally`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PopProfile {
    Road,
    Rally,
}

/// How many paint colours there are (`art::traffic_car`'s: white, silver, black, red, blue,
/// green, yellow).
pub const PAINTS: u8 = 7;

/// The paint colours (sRGB), as `art::traffic_car`'s.
pub const PAINT_SRGB: [[f32; 3]; 7] = [[0.86, 0.86, 0.84], [0.56, 0.58, 0.61], [0.035, 0.037, 0.045], [0.56, 0.06, 0.05], [0.07, 0.17, 0.46], [0.07, 0.24, 0.14], [0.82, 0.62, 0.12]];

impl Paint {
    /// The colour for a livery tint (metallic a shade darker, as the traffic's).
    pub fn color(self) -> Color {
        let c = PAINT_SRGB[(self.colour % PAINTS) as usize];
        let k = if self.metallic { 0.8 } else { 1.0 };
        Color::srgb(c[0] * k, c[1] * k, c[2] * k)
    }
}

/// A paint: one of the 7 `art::traffic_car` paints, solid or metallic. Free to change.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct Paint {
    pub colour: u8,
    pub metallic: bool,
}

/// One car of the roster.
#[derive(Clone, Copy, Debug)]
pub struct CarDef {
    /// Stable id: saves, `car_key`, records ("saker", "wren", ...).
    pub id: &'static str,
    pub name: &'static str,
    pub body: Body,
    /// Dealership price (Notes).
    pub price: u32,
    /// The stock spec.
    pub base: fn() -> Spec,
    /// Showroom default paint.
    pub paint: Paint,
    pub pop_profile: PopProfile,
    /// The body model (`Body::Rally`): a glTF under `assets/` with `art::car`'s rig (the rally
    /// hatch or a converted look-alike, `tools/art/import_sketchfab.py`).
    pub model: &'static str,
}

/// The rally hatch's model, the default body.
pub const RALLY_MODEL: &str = "models/rallycar.glb";

/// The Saker R5's id: today's car, owned at the start (DECISIONS E1).
pub const SAKER: &str = "saker";

/// The roster, in Dealership order (GAME_SYSTEMS §4.1; DECISIONS E2). Index 0 stays the Saker.
pub static CARS: &[CarDef] = &[
    CarDef { id: SAKER, name: "Saker R5", body: Body::Rally, price: 45_000, base: Spec::default, paint: Paint { colour: 0, metallic: false }, pop_profile: PopProfile::Rally, model: RALLY_MODEL },
    CarDef { id: "wren", name: "Wren 1.4", body: Body::Hatch, price: 9_000, base: wren, paint: Paint { colour: 6, metallic: false }, pop_profile: PopProfile::Road, model: RALLY_MODEL },
    CarDef { id: "marten", name: "Marten Tourer", body: Body::Estate, price: 14_000, base: marten, paint: Paint { colour: 5, metallic: false }, pop_profile: PopProfile::Road, model: RALLY_MODEL },
    CarDef { id: "bruin", name: "Bruin Trail", body: Body::Suv, price: 24_000, base: bruin, paint: Paint { colour: 5, metallic: true }, pop_profile: PopProfile::Road, model: RALLY_MODEL },
    CarDef { id: "vale", name: "Vale GT", body: Body::Saloon, price: 30_000, base: vale, paint: Paint { colour: 3, metallic: false }, pop_profile: PopProfile::Road, model: RALLY_MODEL },
    CarDef { id: "harrier", name: "Harrier S", body: Body::Saloon, price: 62_000, base: harrier, paint: Paint { colour: 1, metallic: true }, pop_profile: PopProfile::Road, model: RALLY_MODEL },
    CarDef { id: "fennec", name: "Fennec Kit", body: Body::Rally, price: 85_000, base: fennec, paint: Paint { colour: 3, metallic: true }, pop_profile: PopProfile::Rally, model: RALLY_MODEL },
    CarDef { id: "ossian", name: "Ossian Evo", body: Body::Rally, price: 140_000, base: ossian, paint: Paint { colour: 2, metallic: true }, pop_profile: PopProfile::Rally, model: RALLY_MODEL },
    // Debadged look-alike of a real car (owner's ruling, 8 Nov 2026: no logos, badges, names or
    // sponsor liveries), its own model (`assets/models/car_<id>.glb`).
    CarDef { id: "brawn", name: "Brawn 70", body: Body::Rally, price: 38_000, base: brawn, paint: Paint { colour: 4, metallic: false }, pop_profile: PopProfile::Road, model: "models/car_brawn.glb" },
    // Original designs (concept image -> TripoSG; tools/art/concept_cars.py).
    CarDef { id: "lynx", name: "Lynx 1600", body: Body::Rally, price: 16_000, base: lynx, paint: Paint { colour: 2, metallic: false }, pop_profile: PopProfile::Rally, model: "models/car_lynx.glb" },
    CarDef { id: "kite", name: "Kite Coupe", body: Body::Rally, price: 21_000, base: kite, paint: Paint { colour: 1, metallic: false }, pop_profile: PopProfile::Road, model: "models/car_kite.glb" },
    CarDef { id: "tarn", name: "Tarn 4WD", body: Body::Rally, price: 52_000, base: tarn, paint: Paint { colour: 3, metallic: false }, pop_profile: PopProfile::Rally, model: "models/car_tarn.glb" },
];

/// A roster car from the Saker's spec: torque ×, mass (inertia scales with it), grip, front
/// drive share.
fn variant(torque: f32, mass: f32, grip: f32, front_split: f32) -> Spec {
    let s = Spec::default();
    Spec { torque: s.torque * torque, mass, inertia: s.inertia * (mass / s.mass), grip, front_split, ..s }
}

/// FWD 115 kW hatch, traction aid on, a soft diff.
fn wren() -> Spec {
    Spec { traction_assist: true, lsd: 20.0, ..variant(0.55, 1_050.0, 0.92, 1.0) }
}
/// RWD estate: soft springs, a big drag area.
fn marten() -> Spec {
    let s = variant(0.65, 1_320.0, 0.90, 0.0);
    Spec { spring: s.spring * 0.85, drag: 0.80, ..s }
}
/// AWD SUV: tall and long-travel (a yump monster).
fn bruin() -> Spec {
    let s = variant(0.90, 1_550.0, 0.90, 0.45);
    Spec { rest: s.rest + 0.06, travel: s.travel + 0.04, ..s }
}
/// RWD saloon with a tight diff: the drift car.
fn vale() -> Spec {
    Spec { lsd: 90.0, ..variant(0.85, 1_300.0, 0.95, 0.0) }
}
/// Road-going AWD saloon.
fn harrier() -> Spec {
    Spec { drag: 0.68, ..variant(1.15, 1_380.0, 1.0, 0.35) }
}
/// Light, tail-happy RWD kit car.
fn fennec() -> Spec {
    variant(1.05, 1_020.0, 1.02, 0.0)
}
/// RWD muscle coupe: big torque, heavy nose, loose diff.
fn brawn() -> Spec {
    let s = variant(1.15, 1_620.0, 0.9, 0.0);
    Spec { lsd: 35.0, drag: 0.76, ..s }
}
/// Light 1980s FWD rally hatch: eager, a firm diff.
fn lynx() -> Spec {
    Spec { lsd: 30.0, ..variant(0.55, 1_000.0, 0.93, 1.0) }
}
/// Light 1970s RWD coupe: balanced, a medium diff.
fn kite() -> Spec {
    Spec { lsd: 45.0, ..variant(0.68, 1_080.0, 0.92, 0.0) }
}
/// Early-1990s AWD rally saloon: grippy and planted.
fn tarn() -> Spec {
    Spec { lsd: 50.0, ..variant(1.05, 1_280.0, 0.98, 0.45) }
}
/// The top car: AWD, big brakes.
fn ossian() -> Spec {
    let s = variant(1.35, 1_230.0, 1.05, 0.45);
    Spec { drag: 0.78, brake_torque: s.brake_torque * 1.15, ..s }
}

/// The roster entry for `id`.
pub fn def(id: &str) -> Option<&'static CarDef> {
    CARS.iter().find(|c| c.id == id)
}

/// The seven upgradeable parts (DECISIONS E4). Levels: 0 stock, 1 Street, 2 Gravel, 3 Works.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UpgradeKind {
    Engine,
    Turbo,
    Tyres,
    Brakes,
    Suspension,
    Weight,
    Gearbox,
}

impl UpgradeKind {
    /// In the Upgrades screen's order (E's ids `part:0..part:6`).
    pub const ALL: [UpgradeKind; 7] =
        [UpgradeKind::Engine, UpgradeKind::Turbo, UpgradeKind::Tyres, UpgradeKind::Brakes, UpgradeKind::Suspension, UpgradeKind::Weight, UpgradeKind::Gearbox];

    /// Highest level: Turbo and Gearbox stop at 2.
    pub fn max_level(self) -> u8 {
        match self {
            UpgradeKind::Turbo | UpgradeKind::Gearbox => 2,
            _ => 3,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            UpgradeKind::Engine => "Engine",
            UpgradeKind::Turbo => "Turbo",
            UpgradeKind::Tyres => "Tyres",
            UpgradeKind::Brakes => "Brakes",
            UpgradeKind::Suspension => "Suspension",
            UpgradeKind::Weight => "Weight",
            UpgradeKind::Gearbox => "Gearbox",
        }
    }
}

/// Display name of a level.
pub fn level_name(level: u8) -> &'static str {
    ["Stock", "Street", "Gravel", "Works"][level.min(3) as usize]
}

/// A build: the level of each part (0 = stock).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct Upgrades {
    pub engine: u8,
    pub turbo: u8,
    pub tyres: u8,
    pub brakes: u8,
    pub suspension: u8,
    pub weight: u8,
    pub gearbox: u8,
}

impl Upgrades {
    pub fn get(&self, kind: UpgradeKind) -> u8 {
        match kind {
            UpgradeKind::Engine => self.engine,
            UpgradeKind::Turbo => self.turbo,
            UpgradeKind::Tyres => self.tyres,
            UpgradeKind::Brakes => self.brakes,
            UpgradeKind::Suspension => self.suspension,
            UpgradeKind::Weight => self.weight,
            UpgradeKind::Gearbox => self.gearbox,
        }
    }
    pub fn set(&mut self, kind: UpgradeKind, level: u8) {
        let l = level.min(kind.max_level());
        match kind {
            UpgradeKind::Engine => self.engine = l,
            UpgradeKind::Turbo => self.turbo = l,
            UpgradeKind::Tyres => self.tyres = l,
            UpgradeKind::Brakes => self.brakes = l,
            UpgradeKind::Suspension => self.suspension = l,
            UpgradeKind::Weight => self.weight = l,
            UpgradeKind::Gearbox => self.gearbox = l,
        }
    }
}

/// Price of one level (1..=max) of a part at class factor 1.0 (GAME_SYSTEMS §4.2).
fn base_price(kind: UpgradeKind, level: u8) -> Option<u32> {
    let row: &[u32] = match kind {
        UpgradeKind::Engine => &[3_000, 7_000, 14_000],
        UpgradeKind::Turbo => &[6_000, 12_000],
        UpgradeKind::Tyres => &[2_000, 5_000, 9_000],
        UpgradeKind::Brakes => &[1_500, 3_500, 6_000],
        UpgradeKind::Suspension => &[2_500, 5_000, 8_000],
        UpgradeKind::Weight => &[3_000, 7_000, 12_000],
        UpgradeKind::Gearbox => &[2_500, 6_000],
    };
    row.get((level as usize).checked_sub(1)?).copied()
}

/// Price of one level of a part for a car of `class`, rounded to 100 Notes. `None` for level 0
/// or past the part's top level.
pub fn part_price(kind: UpgradeKind, level: u8, class: Class) -> Option<u32> {
    base_price(kind, level).map(|p| ((p as f32 * class.factor() / 100.0).round() as u32) * 100)
}

/// A car in the player's garage.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OwnedCar {
    /// `CarDef::id`.
    pub model: String,
    /// The levels fitted now (what drives).
    pub fitted: Upgrades,
    /// The highest level bought of each part (refitting up to it is free).
    pub bought: Upgrades,
    pub paint: Paint,
    /// Race number 1-99.
    pub number: u8,
    /// `vehicle::Damage::total()` carried between drives (repaired by Service).
    pub damage: f32,
}

impl Default for OwnedCar {
    fn default() -> Self {
        OwnedCar::stock(&CARS[0])
    }
}

impl OwnedCar {
    /// A new car off the Dealership floor, in its showroom paint.
    pub fn stock(def: &CarDef) -> OwnedCar {
        OwnedCar { model: def.id.into(), fitted: Upgrades::default(), bought: Upgrades::default(), paint: def.paint, number: 27, damage: 0.0 }
    }
    pub fn def(&self) -> Option<&'static CarDef> {
        def(&self.model)
    }
}

/// Why a purchase or fitting is refused (E shows it; F toasts it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Not enough Notes: `need` more.
    Notes { need: u32 },
    /// The car is already in the garage.
    Owned,
    /// Past the part's top level.
    MaxLevel,
    /// No such car in the roster (or garage slot).
    Unknown,
    /// You can't sell your last car.
    LastCar,
    /// Not now: an event is running (car swaps happen at the Service Park or in the menus).
    Busy,
}

impl Refusal {
    /// The player-facing reason ("Need 3,000 more Notes").
    pub fn reason(self) -> String {
        match self {
            Refusal::Notes { need } => format!("Need {} more Notes", super::notes(need)),
            Refusal::Owned => "Already in your garage".into(),
            Refusal::MaxLevel => "Already at the top level".into(),
            Refusal::Unknown => "Not available".into(),
            Refusal::LastCar => "You can't sell your last car".into(),
            Refusal::Busy => "Not during an event".into(),
        }
    }
}

/// The player's cars. Saved in `career.json`. The active car lives here, not in
/// `meta::Profile` (DECISIONS E8).
#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Garage {
    pub owned: Vec<OwnedCar>,
    /// Index into `owned` of the car driven.
    pub active: usize,
}

impl Default for Garage {
    /// A fresh career: the stock Saker R5, active (DECISIONS E1).
    fn default() -> Self {
        Garage { owned: vec![OwnedCar::default()], active: 0 }
    }
}

impl Garage {
    /// The car driven (the first one if `active` is out of range; a garage is never empty).
    pub fn active_car(&self) -> &OwnedCar {
        self.owned.get(self.active).or(self.owned.first()).expect("a garage always holds a car")
    }

    /// Can `model` be bought? `Ok(price)`.
    pub fn can_buy(&self, w: &Wallet, model: &str) -> Result<u32, Refusal> {
        let d = def(model).ok_or(Refusal::Unknown)?;
        if self.owned.iter().any(|c| c.model == model) {
            return Err(Refusal::Owned);
        }
        afford(w, d.price)
    }

    /// Can `level` of `kind` be fitted to the active car? `Ok(cost)`: 0 when it was bought
    /// before, else the sum of the missing levels up to it (DECISIONS E5).
    pub fn can_fit(&self, w: &Wallet, kind: UpgradeKind, level: u8) -> Result<u32, Refusal> {
        if level > kind.max_level() {
            return Err(Refusal::MaxLevel);
        }
        afford(w, self.fit_cost(kind, level))
    }

    /// Notes to fit `level` of `kind` on the active car: the levels above the highest bought.
    pub fn fit_cost(&self, kind: UpgradeKind, level: u8) -> u32 {
        let car = self.active_car();
        let class = car.def().map_or(Class::B, stock_class);
        (car.bought.get(kind) + 1..=level.min(kind.max_level())).filter_map(|l| part_price(kind, l, class)).sum()
    }

    /// Can owned car `index` be sold? `Ok(what it fetches)`. Never the last car.
    pub fn can_sell(&self, index: usize) -> Result<u32, Refusal> {
        let car = self.owned.get(index).ok_or(Refusal::Unknown)?;
        if self.owned.len() <= 1 {
            return Err(Refusal::LastCar);
        }
        Ok(sale_price(car))
    }

    /// Repair cost of the active car (GAME_SYSTEMS §4: `round100(1,500 × damage × class
    /// factor)`).
    pub fn service_cost(&self) -> u32 {
        let car = self.active_car();
        let f = car.def().map_or(1.0, |d| stock_class(d).factor());
        round100(1_500.0 * car.damage.max(0.0) * f)
    }
}

/// Rounds to the nearest 100 Notes.
pub fn round100(x: f32) -> u32 {
    ((x.max(0.0) / 100.0).round() as u32) * 100
}

/// The class a car leaves the Dealership in (upgrade prices go by it).
pub fn stock_class(def: &CarDef) -> Class {
    Class::of(pi(&(def.base)()))
}

/// What the upgrades bought for `car` cost (every bought level).
pub fn upgrades_spent(car: &OwnedCar) -> u32 {
    let class = car.def().map_or(Class::B, stock_class);
    UpgradeKind::ALL.iter().flat_map(|k| (1..=car.bought.get(*k)).filter_map(move |l| part_price(*k, l, class))).sum()
}

/// What selling `car` fetches: 60 % of its price plus 50 % of its upgrades (GAME_SYSTEMS §4).
pub fn sale_price(car: &OwnedCar) -> u32 {
    let price = car.def().map_or(0, |d| d.price);
    round100(price as f32 * 0.6 + upgrades_spent(car) as f32 * 0.5)
}

fn afford(w: &Wallet, price: u32) -> Result<u32, Refusal> {
    if w.balance >= price { Ok(price) } else { Err(Refusal::Notes { need: price - w.balance }) }
}

/// Engine × Turbo torque is capped here (GAME_SYSTEMS §4.2).
pub const TORQUE_CAP: f32 = 1.35;

/// Effect of one level (0 = stock) of a part, as multipliers (GAME_SYSTEMS §4.2).
fn level_of<const N: usize>(table: [f32; N], level: u8) -> f32 {
    if level == 0 { 1.0 } else { table[(level as usize - 1).min(N - 1)] }
}

/// The spec of a build: the car's base × the upgrade multipliers (GAME_SYSTEMS §4.2). Pure,
/// and a stock build is the base spec exactly (bit for bit), so the stock Saker is today's car.
pub fn build_spec(def: &CarDef, u: &Upgrades) -> Spec {
    let mut s = (def.base)();
    if *u == Upgrades::default() {
        return s;
    }
    let engine = level_of([1.06, 1.12, 1.20], u.engine);
    let turbo = level_of([1.08, 1.16], u.turbo);
    s.torque *= (engine * turbo).min(TORQUE_CAP);
    if u.turbo > 0 {
        s.engine_brake *= 0.9;
    }
    s.grip *= level_of([1.03, 1.06, 1.09], u.tyres);
    let weight = level_of([0.97, 0.94, 0.90], u.weight);
    s.mass *= weight;
    s.inertia *= weight;
    // Brakes: the sim's ABS stops shortest with the brake torque matched to the car's weight
    // and grip (`abs_torque`; more lengthens the stop). So the part brings the foot brake up
    // to what the build can use (60 / 85 / 100 % of the way, never below stock), which pays
    // off on heavy cars and with Tyres fitted, and scales the handbrake ×1.10 / 1.20 / 1.30.
    // (GAME_SYSTEMS §4.2's flat ×1.1-1.3 on the foot brake made stops *longer*.)
    if u.brakes > 0 {
        let k = u.brakes.min(3) as usize - 1;
        let want = abs_torque(&s).max(s.brake_torque);
        s.brake_torque += (want - s.brake_torque) * [0.6, 0.85, 1.0][k];
        s.handbrake_torque *= [1.10, 1.20, 1.30][k];
    }
    if u.suspension > 0 {
        let k = u.suspension.min(3) as usize - 1;
        s.damper *= [1.10, 1.15, 1.20][k];
        s.travel += [0.01, 0.02, 0.03][k];
        let arb = [1.05, 1.10, 1.15][k];
        s.arb = [s.arb[0] * arb, s.arb[1] * arb];
    }
    s.shift_time *= level_of([0.8, 0.6], u.gearbox);
    s.final_drive *= level_of([1.04, 1.08], u.gearbox);
    s
}

/// The foot-brake torque that stops a car of this weight and grip shortest with ABS (the
/// Saker's 4,200 N·m scaled by mass and tyre grip; measured in `brake_explore`).
pub fn abs_torque(spec: &Spec) -> f32 {
    4_200.0 * (spec.mass / 1_250.0) * spec.grip
}

/// The spec an owned car drives with (before `prefs`' assist and damage overrides). Falls back
/// to the Saker for an unknown model.
pub fn spec_of(car: &OwnedCar) -> Spec {
    build_spec(car.def().unwrap_or(&CARS[0]), &car.fitted)
}

/// Performance index 100..=999 (GAME_SYSTEMS §4.3).
///
/// Note: the doc's table gives the Ossian Evo PI 728, which leaves out its brakes ×1.15
/// (+15); with the formula as written it is 742 (still class S).
pub fn pi(spec: &Spec) -> u16 {
    let kw = 0.583 * spec.torque;
    let pw = kw / (spec.mass / 1000.0);
    let drive = if spec.front_split > 0.2 && spec.front_split < 0.8 {
        40.0
    } else if spec.front_split <= 0.2 {
        15.0
    } else {
        0.0
    };
    let brakes = 100.0 * (spec.brake_torque / 4200.0 - 1.0);
    let shift = 50.0 * (1.0 - spec.shift_time / crate::vehicle::SHIFT_TIME);
    (100.0 + 2.2 * pw + 400.0 * (spec.grip - 0.85) + drive + brakes + shift).round().clamp(100.0, 999.0) as u16
}

/// Key of a build for records and ghost files (TECH_PASS §5, DECISIONS E12):
/// `"saker.e0t0y0b0s0w0g0"` (engine, turbo, tyres, brakes, suspension, weight, gearbox).
pub fn car_key(car: &OwnedCar) -> String {
    let u = &car.fitted;
    format!("{}.e{}t{}y{}b{}s{}w{}g{}", car.model, u.engine, u.turbo, u.tyres, u.brakes, u.suspension, u.weight, u.gearbox)
}

/// Stat bars for E's screens, each 0..1: power, grip, handling, braking, weight (lighter is
/// fuller). Scaled so the roster spans most of each bar and a full build tops it out.
pub fn stat_bars(spec: &Spec) -> [f32; 5] {
    let power = 0.583 * spec.torque / 320.0;
    let grip = (spec.grip - 0.85) / 0.30;
    // Handling: grip, roll stiffness per tonne and the drive layout.
    let arb = (spec.arb[0] + spec.arb[1]) / spec.mass / 20.0;
    let awd = if spec.front_split > 0.2 && spec.front_split < 0.8 { 0.15 } else { 0.0 };
    let handling = 0.45 * grip + 0.4 * arb + awd;
    let braking = spec.brake_torque / spec.mass / 5.0;
    let weight = 1.0 - (spec.mass - 850.0) / 800.0;
    [power, grip, handling, braking, weight].map(|v| v.clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stage::Surface;
    use crate::vehicle::{Car, Flat, Input, step};

    #[test]
    fn the_saker_is_todays_car_in_class_b() {
        let spec = spec_of(&OwnedCar::default());
        assert_eq!(format!("{spec:?}"), format!("{:?}", Spec::default()), "the stock Saker drives exactly as today");
        let p = pi(&spec);
        assert!((565..=575).contains(&p), "GAME_SYSTEMS §4.1: PI 570, got {p}");
        assert_eq!(Class::of(p), Class::B);
    }

    #[test]
    fn the_roster_hits_its_pis_and_classes() {
        // GAME_SYSTEMS §4.1 (Ossian: 742 with its brakes, see `pi`).
        let want = [("saker", 570, Class::B), ("wren", 370, Class::D), ("marten", 362, Class::D), ("bruin", 428, Class::C), ("vale", 456, Class::C), ("harrier", 585, Class::B), ("fennec", 659, Class::A), ("ossian", 742, Class::S), ("brawn", 463, Class::C), ("lynx", 386, Class::D), ("kite", 434, Class::C), ("tarn", 571, Class::B)];
        assert_eq!(CARS.len(), 12);
        for (id, p, class) in want {
            let d = def(id).unwrap();
            let got = pi(&(d.base)());
            assert!(got.abs_diff(p) <= 5, "{id}: PI {got}, want {p}");
            assert_eq!(Class::of(got), class, "{id}");
            assert_eq!(stock_class(d), class);
        }
        // Every car hashes differently (ghosts and records key by it).
        let mut hashes: Vec<String> = CARS.iter().map(|d| crate::meta::ghost::sim_hash(&(d.base)(), 60)).collect();
        hashes.sort();
        hashes.dedup();
        assert_eq!(hashes.len(), CARS.len());
        let mut ids: Vec<&str> = CARS.iter().map(|d| d.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), CARS.len());
    }

    #[test]
    fn each_level_matches_the_table() {
        let d = &CARS[0];
        let base = (d.base)();
        let with = |k: UpgradeKind, l: u8| {
            let mut u = Upgrades::default();
            u.set(k, l);
            build_spec(d, &u)
        };
        let close = |a: f32, b: f32| (a - b).abs() < 1e-3 * b.abs().max(1.0);
        for (l, m) in [(1, 1.06), (2, 1.12), (3, 1.20)] {
            assert!(close(with(UpgradeKind::Engine, l).torque, base.torque * m));
        }
        for (l, m) in [(1, 1.08), (2, 1.16)] {
            let s = with(UpgradeKind::Turbo, l);
            assert!(close(s.torque, base.torque * m) && close(s.engine_brake, base.engine_brake * 0.9));
        }
        for (l, m) in [(1, 1.03), (2, 1.06), (3, 1.09)] {
            assert!(close(with(UpgradeKind::Tyres, l).grip, m));
        }
        for (l, m) in [(1, 1.10), (2, 1.20), (3, 1.30)] {
            let s = with(UpgradeKind::Brakes, l);
            assert!(close(s.handbrake_torque, base.handbrake_torque * m) && s.brake_torque >= base.brake_torque);
        }
        // On a heavy car the foot brake grows towards what its grip can use.
        let bruin = def("bruin").unwrap();
        let b3 = build_spec(bruin, &Upgrades { brakes: 3, ..default() });
        assert!(close(b3.brake_torque, abs_torque(&b3)) && b3.brake_torque > (bruin.base)().brake_torque * 1.1);
        for (l, m, t, a) in [(1, 1.10, 0.01, 1.05), (2, 1.15, 0.02, 1.10), (3, 1.20, 0.03, 1.15)] {
            let s = with(UpgradeKind::Suspension, l);
            assert!(close(s.damper, base.damper * m) && close(s.travel, base.travel + t) && close(s.arb[0], base.arb[0] * a) && close(s.arb[1], base.arb[1] * a));
        }
        for (l, m) in [(1, 0.97), (2, 0.94), (3, 0.90)] {
            let s = with(UpgradeKind::Weight, l);
            assert!(close(s.mass, base.mass * m) && close(s.inertia.y, base.inertia.y * m));
        }
        for (l, m, f) in [(1, 0.8, 1.04), (2, 0.6, 1.08)] {
            let s = with(UpgradeKind::Gearbox, l);
            assert!(close(s.shift_time, base.shift_time * m) && close(s.final_drive, base.final_drive * f));
        }
        // Engine Works + Turbo L2 is clamped at ×1.35.
        let u = Upgrades { engine: 3, turbo: 2, ..default() };
        assert!(close(build_spec(d, &u).torque, base.torque * TORQUE_CAP));
    }

    #[test]
    fn pi_rises_with_every_upgrade_level() {
        for d in CARS {
            for k in UpgradeKind::ALL {
                let mut u = Upgrades::default();
                let mut last = pi(&build_spec(d, &u));
                for l in 1..=k.max_level() {
                    u.set(k, l);
                    let p = pi(&build_spec(d, &u));
                    // Suspension changes nothing the PI reads (it is handling, not pace), and
                    // Brakes only once the build can use more brake than it has.
                    if k == UpgradeKind::Suspension || k == UpgradeKind::Brakes {
                        assert!(p >= last, "{} {:?} L{l}: PI {p} after {last}", d.id, k);
                    } else {
                        assert!(p > last, "{} {:?} L{l}: PI {p} after {last}", d.id, k);
                    }
                    last = p;
                }
            }
        }
        // A full build takes the Saker to S; the Wren climbs out of D.
        let full = Upgrades { engine: 3, turbo: 2, tyres: 3, brakes: 3, suspension: 3, weight: 3, gearbox: 2 };
        assert_eq!(Class::of(pi(&build_spec(&CARS[0], &full))), Class::S, "{}", pi(&build_spec(&CARS[0], &full)));
        assert!(Class::of(pi(&build_spec(def("wren").unwrap(), &full))) >= Class::C);
    }

    #[test]
    fn a_fresh_garage_and_its_refusals() {
        let g = Garage::default();
        assert_eq!(g.active_car().model, SAKER);
        let w = Wallet::default();
        assert_eq!(g.can_buy(&w, SAKER), Err(Refusal::Owned));
        assert_eq!(g.can_buy(&w, "nope"), Err(Refusal::Unknown));
        assert_eq!(g.can_buy(&w, "wren"), Ok(9_000));
        assert_eq!(g.can_buy(&w, "marten"), Err(Refusal::Notes { need: 2_000 }));
        assert_eq!(g.can_fit(&w, UpgradeKind::Brakes, 1), Ok(1_500), "class B: factor 1.0");
        assert_eq!(g.can_fit(&w, UpgradeKind::Engine, 3), Err(Refusal::Notes { need: 3_000 + 7_000 + 14_000 - w.balance }));
        assert_eq!(g.can_fit(&w, UpgradeKind::Turbo, 3), Err(Refusal::MaxLevel));
        assert_eq!(g.can_fit(&w, UpgradeKind::Gearbox, 0), Ok(0), "back to stock is free");
        assert_eq!(g.can_sell(0), Err(Refusal::LastCar));
        assert_eq!(Refusal::Notes { need: 3_000 }.reason(), "Need 3,000 more Notes");
    }

    #[test]
    fn selling_and_service_prices() {
        let mut car = OwnedCar::stock(def("vale").unwrap());
        assert_eq!(sale_price(&car), 18_000, "60 % of 30k");
        // Class C (×0.8): engine L1 + L2 = 2,400 + 5,600; half back.
        car.bought.engine = 2;
        assert_eq!(upgrades_spent(&car), 8_000);
        assert_eq!(sale_price(&car), 22_000);
        let mut g = Garage { owned: vec![OwnedCar::default(), car], active: 1 };
        assert_eq!(g.can_sell(1), Ok(22_000));
        g.owned[1].damage = 0.5;
        assert_eq!(g.service_cost(), 600, "1,500 × 0.5 × 0.8");
    }

    #[test]
    fn prices_scale_by_class() {
        assert_eq!(part_price(UpgradeKind::Engine, 1, Class::D), Some(1_800));
        assert_eq!(part_price(UpgradeKind::Brakes, 1, Class::S), Some(2_400));
        assert_eq!(part_price(UpgradeKind::Turbo, 3, Class::B), None);
        assert_eq!(part_price(UpgradeKind::Tyres, 0, Class::B), None);
        let all_street: u32 = UpgradeKind::ALL.iter().filter_map(|k| part_price(*k, 1, Class::B)).sum();
        assert_eq!(all_street, 20_500, "DECISIONS E3's pacing check");
        assert_eq!(car_key(&OwnedCar::default()), "saker.e0t0y0b0s0w0g0");
    }

    // ---- what each upgrade does on the road (the direction the Upgrades screen claims) ----

    const DT: f32 = 1.0 / 60.0;

    /// 0-100 and 100-160 km/h (s), on flat tarmac. (Top speed is the limiter in 5th on the
    /// gravel gearing, ~195 km/h, so power shows as acceleration.)
    fn sprint(spec: &Spec) -> (f32, f32) {
        let g = Flat(0.0, Surface::Tarmac);
        let mut car = Car::new(Vec3::new(0.0, 0.6, 0.0), 0.0);
        for _ in 0..60 {
            step(&mut car, spec, Input::default(), &g, DT);
        }
        let (mut t, mut t100, mut t160) = (0.0, f32::NAN, f32::NAN);
        for _ in 0..(40 * 60) {
            step(&mut car, spec, Input { throttle: 1.0, ..default() }, &g, DT);
            t += DT;
            if t100.is_nan() && car.speed() * 3.6 >= 100.0 {
                t100 = t;
            }
            if t160.is_nan() && car.speed() * 3.6 >= 160.0 {
                t160 = t;
            }
        }
        (t100, t160 - t100)
    }

    /// Peak lateral acceleration (m/s²) turning in at ~70 km/h on gravel, and the braking
    /// distance (m) from 100 km/h on gravel.
    fn grip_and_brakes(spec: &Spec) -> (f32, f32) {
        grip_and_brakes_on(spec, Surface::Gravel)
    }

    fn grip_and_brakes_on(spec: &Spec, surface: Surface) -> (f32, f32) {
        let g = Flat(0.0, surface);
        let mut car = Car::new(Vec3::new(0.0, 0.6, 0.0), 0.0);
        for _ in 0..60 {
            step(&mut car, spec, Input::default(), &g, DT);
        }
        while car.speed() < 70.0 / 3.6 {
            step(&mut car, spec, Input { throttle: 1.0, ..default() }, &g, DT);
        }
        let mut peak = 0.0f32;
        for _ in 0..240 {
            let v0 = car.vel;
            step(&mut car, spec, Input { throttle: 0.35, steer: 0.6, ..default() }, &g, DT);
            let a = (car.vel - v0) / DT;
            peak = peak.max(a.dot(car.rot * Vec3::NEG_X).abs());
        }
        let mut car = Car::new(Vec3::new(0.0, 0.6, 0.0), 0.0);
        for _ in 0..60 {
            step(&mut car, spec, Input::default(), &g, DT);
        }
        while car.speed() < 100.0 / 3.6 {
            step(&mut car, spec, Input { throttle: 1.0, ..default() }, &g, DT);
        }
        let from = car.pos;
        while car.speed() > 0.5 {
            step(&mut car, spec, Input { brake: 1.0, ..default() }, &g, DT);
        }
        (peak, car.pos.distance(from))
    }

    #[test]
    #[ignore]
    fn brake_explore() {
        for id in ["saker", "bruin", "marten", "harrier", "wren", "ossian"] {
            for tyres in [0u8, 3] {
                for brakes in 0..=3u8 {
                    let s = build_spec(def(id).unwrap(), &Upgrades { tyres, brakes, ..default() });
                    let (_, t) = grip_and_brakes_on(&s, Surface::Tarmac);
                    let (_, g) = grip_and_brakes_on(&s, Surface::Gravel);
                    eprintln!("{id} tyres {tyres} brakes {brakes}: tarmac {t:.2} gravel {g:.2}");
                }
            }
        }
    }

    #[test]
    fn upgrades_change_the_car_the_way_they_claim() {
        let d = &CARS[0];
        let stock = build_spec(d, &Upgrades::default());
        let up = |f: fn(&mut Upgrades)| {
            let mut u = Upgrades::default();
            f(&mut u);
            build_spec(d, &u)
        };
        let (t0, v0) = sprint(&stock);
        let (g0, b0) = grip_and_brakes(&stock);
        eprintln!("stock: 0-100 {t0:.2} s, 100-160 {v0:.2} s, lateral {g0:.2} m/s², 100-0 {b0:.1} m");
        // Power: quicker to 100 and from 100 to 160.
        for (name, spec) in [("engine L3", up(|u| u.engine = 3)), ("turbo L2", up(|u| u.turbo = 2))] {
            let (t, v) = sprint(&spec);
            eprintln!("{name}: 0-100 {t:.2} s, 100-160 {v:.2} s");
            assert!(t < t0 - 0.1 && v < v0 - 0.3, "{name}: {t:.2} s / {v:.2} s vs {t0:.2} / {v0:.2}");
        }
        // Weight: quicker to 100.
        let (t, _) = sprint(&up(|u| u.weight = 3));
        eprintln!("weight L3: 0-100 {t:.2} s");
        assert!(t < t0 - 0.1, "weight: {t:.2} vs {t0:.2}");
        // Gearbox: faster shifts and shorter gears, quicker to 100.
        let (t, _) = sprint(&up(|u| u.gearbox = 2));
        eprintln!("gearbox L2: 0-100 {t:.2} s");
        assert!(t < t0 - 0.05, "gearbox: {t:.2} vs {t0:.2}");
        // Tyres: more cornering grip and shorter stops.
        let (g, b) = grip_and_brakes(&up(|u| u.tyres = 3));
        eprintln!("tyres L3: lateral {g:.2}, 100-0 {b:.1} m");
        assert!(g > g0 * 1.02 && b < b0 - 0.5, "tyres: {g:.2} / {b:.1} vs {g0:.2} / {b0:.1}");
        // Brakes: shorter stops on tarmac once the build has more grip or weight than the
        // stock brake suits (Saker on Works tyres; the heavy Bruin); never longer.
        for (car, tyres) in [("saker", 3u8), ("bruin", 0), ("harrier", 3)] {
            let d = def(car).unwrap();
            let (_, b0) = grip_and_brakes_on(&build_spec(d, &Upgrades { tyres, ..default() }), Surface::Tarmac);
            let mut last = b0;
            for brakes in 1..=3u8 {
                let (_, b) = grip_and_brakes_on(&build_spec(d, &Upgrades { tyres, brakes, ..default() }), Surface::Tarmac);
                eprintln!("{car} tyres {tyres} brakes L{brakes}: 100-0 {b:.2} m (L0 {b0:.2})");
                assert!(b <= last + 0.3, "{car} brakes L{brakes}: {b:.2} after {last:.2}");
                last = b;
            }
            assert!(last < b0 - 0.5, "{car}: brakes L3 {last:.2} vs {b0:.2}");
        }
        let (_, b) = grip_and_brakes_on(&up(|u| u.brakes = 3), Surface::Tarmac);
        assert!(b <= grip_and_brakes_on(&stock, Surface::Tarmac).1 + 0.05, "stock Saker: brakes never lengthen the stop");
    }
}
