//! The career: Notes, the garage and its upgrades, the driver grade, Style and the Rhythm,
//! rivals and races.
//!
//! **Owner: stream F (GAMEPLAY)**; everything under `src/career/` is F's. Design:
//! `docs/festival/GAME_SYSTEMS.md` §2 (Style), §4 (economy, roster, upgrades, PI, save), §5
//! (rivals), §8 (interfaces). Names and rulings: `docs/festival/DECISIONS.md` §2.1-§2.4
//! (they override the other docs). Contracts: `docs/festival/TECH_PASS.md` §3.
//!
//! Wave 0 (the lead's contracts commit) landed the types and messages only, with stub bodies:
//! nothing here changes how the game plays yet. E (UI) builds screens against these types and
//! only ever *sends* [`CareerRequest`]s; only F mutates [`wallet::Wallet`] and
//! [`garage::Garage`]. One save: `<DataDir>/`[`CAREER_FILE`].

pub mod economy;
pub mod garage;
pub mod grade;
pub mod race_flow;
pub mod racers;
pub mod save;
pub mod style;
pub mod wallet;

use bevy::prelude::*;
use engine::settings::add_settings_section;
use engine::ui::prelude::*;
use serde::{Deserialize, Serialize};

use garage::{Paint, UpgradeKind};
use racers::Difficulty;

/// The career save, in the data dir (GAME_SYSTEMS §4.4 shape; settings stay in settings.json).
pub const CAREER_FILE: &str = "career.json";

/// "12,400": Notes with thousands commas (DECISIONS §1 number style). Add " N" or " Notes".
pub fn notes(n: u32) -> String {
    let d = n.to_string();
    let mut out = String::with_capacity(d.len() + d.len() / 3);
    for (i, ch) in d.chars().enumerate() {
        if i > 0 && (d.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// Why Notes moved (a ledger line). GAME_SYSTEMS §8 `PayReason`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PayReason {
    /// A banked Rhythm (Style / 25, capped).
    Rhythm,
    /// A new star on a challenge (`open::challenges::ChallengeDef::id`).
    ChallengeStar { id: u32, stars: u8 },
    /// A new best on a challenge with no new star.
    ChallengeBest { id: u32 },
    /// A race purse share (and its bonuses); `pos` 1-based.
    Race { event: u32, pos: u8, difficulty: Difficulty },
    /// A solo Time Trial finish.
    TimeTrial { event: u32 },
    /// A car sold / bought (`garage::CarDef::id`).
    Sale { car: &'static str },
    Purchase { car: &'static str },
    /// An upgrade level bought for the active car.
    Upgrade { kind: UpgradeKind, level: u8 },
    /// Ace / Legend entry fee.
    EntryFee { event: u32 },
    /// Repair at the Service Park or in the Garage.
    Service,
}

/// Credit Notes. Any stream may send it; only the economy (F) applies it to the wallet.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct Payout {
    pub amount: u32,
    pub reason: PayReason,
}

/// Debit Notes. Refused (and logged) when the balance is short: it never goes negative.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct Charge {
    pub amount: u32,
    pub reason: PayReason,
}

/// Sent by the economy *after* the wallet grew: the money sound (AUDIO_PASS §3) and the HUD's
/// Notes chip listen.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct MoneyEarned {
    pub amount: u32,
}

/// The last run's payout lines ("Purse P2", 1155), for E's results card.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct LastPayout(pub Vec<(String, u32)>);

/// UI → career. E sends these after its confirm modal; F applies them, saves, and toasts a
/// refusal (with `garage::Refusal`'s reason). Indices are into `Garage::owned`.
#[derive(Message, Clone, Debug, PartialEq)]
pub enum CareerRequest {
    /// Buy a car from the Dealership (`CarDef::id`); it joins the garage, not active.
    BuyCar(String),
    /// Fit `level` (0 = stock) of a part on the active car, buying any missing levels up to it
    /// (DECISIONS E5). Refitting an owned level is free.
    FitPart { kind: UpgradeKind, level: u8 },
    /// Drive this owned car (only at the Service Park, home or from the menu).
    SetActiveCar(usize),
    /// Repaint the active car (free).
    Paint(Paint),
    /// The active car's race number, 1-99.
    SetNumber(u8),
    /// Repair the active car.
    Service,
    /// Sell an owned car (60 % of its price + 50 % of its upgrades); never the last one.
    SellCar(usize),
}

/// F's settings (section `"career"` of settings.json). F registers its rows (tab "HUD") in its
/// own plugin with `register_setting`; nothing in `prefs.rs` lists them.
#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CareerSettings {
    /// HUD › Style HUD: 0 Full, 1 Compact (bank and break only), 2 Off. Scoring runs anyway.
    pub style_hud: u32,
    /// HUD › Challenge cards (approach, live readout, result).
    pub challenge_cards: bool,
    /// HUD › Race positions.
    pub race_positions: bool,
}

impl Default for CareerSettings {
    fn default() -> Self {
        CareerSettings { style_hud: 0, challenge_cards: true, race_positions: true }
    }
}

pub struct CareerPlugin;

impl Plugin for CareerPlugin {
    fn build(&self, app: &mut App) {
        add_settings_section::<CareerSettings>(app, "career");
        app.settings_defaults::<CareerSettings>()
            .init_resource::<wallet::Wallet>()
            .init_resource::<garage::Garage>()
            .init_resource::<grade::Grade>()
            .init_resource::<LastPayout>()
            .init_resource::<style::Rhythm>()
            .init_resource::<racers::OtherCars>()
            .add_message::<Payout>()
            .add_message::<Charge>()
            .add_message::<MoneyEarned>()
            .add_message::<CareerRequest>()
            .add_message::<style::StyleEvent>()
            .add_message::<racers::RaceResult>()
            .add_plugins((economy::EconomyPlugin, racers::RacersPlugin));
    }
}
