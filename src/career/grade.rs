//! The driver grade (1-30), earned with banked Style.
//!
//! **Owner: stream F (GAMEPLAY).** Design: `docs/festival/UX_PASS.md` §1 (top bar) and
//! `docs/festival/DECISIONS.md` S5 (xp = banked Style 1:1). F owns the level curve; E shows it.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Highest grade.
pub const MAX_GRADE: u32 = 30;

#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Grade {
    pub xp: u32,
}

impl Grade {
    /// 1..=30. Wave 0 stub: a flat 10,000 xp per grade (F tunes the curve).
    pub fn level(&self) -> u32 {
        (1 + self.xp / 10_000).min(MAX_GRADE)
    }
}
