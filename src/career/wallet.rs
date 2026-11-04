//! The player's Notes.
//!
//! **Owner: stream F (GAMEPLAY).** Design: `docs/festival/GAME_SYSTEMS.md` §4 (faucets and
//! sinks), §4.4 (save), EC-1; rulings `docs/festival/DECISIONS.md` E1, E3, E7.
//! Only F changes a `Wallet`, by applying `career::Payout` / `career::Charge`; everything else
//! reads it.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// A fresh career's balance (GAME_SYSTEMS §4; DECISIONS E1).
pub const STARTING_BALANCE: u32 = 12_000;

/// Notes: what the player has, and the lifetime totals for the Logbook.
#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Wallet {
    pub balance: u32,
    pub earned: u32,
    pub spent: u32,
}

impl Default for Wallet {
    fn default() -> Self {
        Wallet { balance: STARTING_BALANCE, earned: 0, spent: 0 }
    }
}
