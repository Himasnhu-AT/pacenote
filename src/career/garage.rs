//! The roster, upgrades, performance index and the player's garage. Pure (no ECS besides the
//! `Garage` resource derive), so the Dealership, the AI field and the challenge bake all use
//! the same numbers.
//!
//! **Owner: stream F (GAMEPLAY).** Design: `docs/festival/GAME_SYSTEMS.md` §4.1-§4.3 (roster,
//! upgrades, PI) and EC-2..EC-6; rulings `docs/festival/DECISIONS.md` E1-E12.
//!
//! Wave 0 stubs: [`CARS`] holds only the Saker R5 (today's car, `Spec::default()`), and
//! [`build_spec`] ignores the upgrades. F fills in the other seven cars and the upgrade
//! multipliers (after adding `Spec.grip` / `Spec.shift_time`, DECISIONS E6). The prices
//! ([`part_price`]), [`pi`], [`Class`] and the `can_*` checks are already real.

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
}

/// The Saker R5's id: today's car, owned at the start (DECISIONS E1).
pub const SAKER: &str = "saker";

/// The roster, in Dealership order. Wave 0: the Saker only; F adds the other seven
/// (GAME_SYSTEMS §4.1). Index 0 stays the Saker.
pub static CARS: &[CarDef] = &[CarDef {
    id: SAKER,
    name: "Saker R5",
    body: Body::Rally,
    price: 45_000,
    base: Spec::default,
    paint: Paint { colour: 0, metallic: false },
    pop_profile: PopProfile::Rally,
}];

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
    /// No such car in the roster.
    Unknown,
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
        let car = self.active_car();
        let class = Class::of(pi(&spec_of(car)));
        let have = car.bought.get(kind);
        let cost = (have + 1..=level).filter_map(|l| part_price(kind, l, class)).sum();
        afford(w, cost)
    }
}

fn afford(w: &Wallet, price: u32) -> Result<u32, Refusal> {
    if w.balance >= price { Ok(price) } else { Err(Refusal::Notes { need: price - w.balance }) }
}

/// The spec of a build: the car's base × the upgrade multipliers (GAME_SYSTEMS §4.2). Pure.
/// Wave 0 stub: returns the base spec (F adds the multipliers, EC-4).
pub fn build_spec(def: &CarDef, _upgrades: &Upgrades) -> Spec {
    (def.base)()
}

/// The spec an owned car drives with (before `prefs`' assist and damage overrides). Falls back
/// to the Saker for an unknown model.
pub fn spec_of(car: &OwnedCar) -> Spec {
    build_spec(car.def().unwrap_or(&CARS[0]), &car.fitted)
}

/// Performance index 100..=999 (GAME_SYSTEMS §4.3). Until `Spec.grip` and `Spec.shift_time`
/// exist (F, DECISIONS E6) their terms are at their defaults (grip 1.0, stock shift).
pub fn pi(spec: &Spec) -> u16 {
    let kw = 0.583 * spec.torque;
    let pw = kw / (spec.mass / 1000.0);
    let grip = 1.0;
    let drive = if spec.front_split > 0.2 && spec.front_split < 0.8 {
        40.0
    } else if spec.front_split <= 0.2 {
        15.0
    } else {
        0.0
    };
    let brakes = 100.0 * (spec.brake_torque / 4200.0 - 1.0);
    (100.0 + 2.2 * pw + 400.0 * (grip - 0.85) + drive + brakes).round().clamp(100.0, 999.0) as u16
}

/// Key of a build for records and ghost files (TECH_PASS §5, DECISIONS E12):
/// `"saker.e0t0y0b0s0w0g0"` (engine, turbo, tyres, brakes, suspension, weight, gearbox).
pub fn car_key(car: &OwnedCar) -> String {
    let u = &car.fitted;
    format!("{}.e{}t{}y{}b{}s{}w{}g{}", car.model, u.engine, u.turbo, u.tyres, u.brakes, u.suspension, u.weight, u.gearbox)
}

/// Stat bars for E's screens, each 0..1: power, grip, handling, braking, weight (lighter is
/// fuller). Wave 0 stub over today's fields; F tunes it once the roster exists.
pub fn stat_bars(spec: &Spec) -> [f32; 5] {
    let power = 0.583 * spec.torque / 300.0;
    let braking = spec.brake_torque / 6000.0;
    let weight = 1.0 - (spec.mass - 900.0) / 800.0;
    [power, 0.5, 0.5, braking, weight].map(|v| v.clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_saker_is_todays_car_in_class_b() {
        let spec = spec_of(&OwnedCar::default());
        assert_eq!(format!("{spec:?}"), format!("{:?}", Spec::default()), "the stock Saker drives exactly as today");
        let p = pi(&spec);
        assert!((565..=575).contains(&p), "GAME_SYSTEMS §4.1: PI 570, got {p}");
        assert_eq!(Class::of(p), Class::B);
    }

    #[test]
    fn a_fresh_garage_and_its_refusals() {
        let g = Garage::default();
        assert_eq!(g.active_car().model, SAKER);
        let w = Wallet::default();
        assert_eq!(g.can_buy(&w, SAKER), Err(Refusal::Owned));
        assert_eq!(g.can_buy(&w, "nope"), Err(Refusal::Unknown));
        assert_eq!(g.can_fit(&w, UpgradeKind::Brakes, 1), Ok(1_500), "class B: factor 1.0");
        assert_eq!(g.can_fit(&w, UpgradeKind::Engine, 3), Err(Refusal::Notes { need: 3_000 + 7_000 + 14_000 - w.balance }));
        assert_eq!(g.can_fit(&w, UpgradeKind::Turbo, 3), Err(Refusal::MaxLevel));
        assert_eq!(g.can_fit(&w, UpgradeKind::Gearbox, 0), Ok(0), "back to stock is free");
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
}
