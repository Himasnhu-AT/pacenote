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

impl Wallet {
    /// Adds `amount` (saturating), counting it as earned.
    pub fn credit(&mut self, amount: u32) {
        self.balance = self.balance.saturating_add(amount);
        self.earned = self.earned.saturating_add(amount);
    }

    /// Takes `amount` if the balance covers it; otherwise refuses with what's missing. The
    /// balance never goes negative.
    pub fn debit(&mut self, amount: u32) -> Result<(), super::garage::Refusal> {
        if self.balance < amount {
            return Err(super::garage::Refusal::Notes { need: amount - self.balance });
        }
        self.balance -= amount;
        self.spent = self.spent.saturating_add(amount);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credit_and_debit_never_go_negative() {
        let mut w = Wallet::default();
        assert_eq!(w.balance, 12_000);
        w.credit(500);
        assert_eq!((w.balance, w.earned), (12_500, 500));
        assert_eq!(w.debit(13_000), Err(super::super::garage::Refusal::Notes { need: 500 }));
        assert_eq!(w.balance, 12_500, "a refused charge changes nothing");
        w.debit(12_500).unwrap();
        assert_eq!((w.balance, w.spent), (0, 12_500));
    }
}
