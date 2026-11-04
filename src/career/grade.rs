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

/// Banked Style to reach grade 2; grade `g` needs `XP_STEP × (g − 1)²` in all. A skilled
/// Rhythm banks ~5,000 Style a minute, so grade 2 comes in the first minute, grade 5 in about
/// 10 minutes and grade 30 after ~8 hours (when the top car's full build does too).
pub const XP_STEP: u32 = 3_000;

impl Grade {
    /// 1..=30.
    pub fn level(&self) -> u32 {
        let g = 1 + ((self.xp as f64 / XP_STEP as f64).sqrt().floor() as u32);
        g.min(MAX_GRADE)
    }

    /// Total xp at which `grade` starts.
    pub fn xp_for(grade: u32) -> u32 {
        let g = grade.clamp(1, MAX_GRADE) - 1;
        XP_STEP * g * g
    }

    /// Progress through the current grade, 0..1 (1 at the top grade).
    pub fn progress(&self) -> f32 {
        let g = self.level();
        if g >= MAX_GRADE {
            return 1.0;
        }
        let (a, b) = (Grade::xp_for(g), Grade::xp_for(g + 1));
        (self.xp - a) as f32 / (b - a) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grade_curve() {
        assert_eq!(Grade { xp: 0 }.level(), 1);
        assert_eq!(Grade { xp: 2_999 }.level(), 1);
        assert_eq!(Grade { xp: 3_000 }.level(), 2);
        assert_eq!(Grade { xp: Grade::xp_for(5) }.level(), 5);
        assert_eq!(Grade { xp: u32::MAX }.level(), MAX_GRADE);
        let mut last = 0;
        for xp in (0..3_000_000).step_by(997) {
            let l = Grade { xp }.level();
            assert!(l >= last);
            last = l;
        }
        assert!((Grade { xp: 4_500 }.progress() - 1_500.0 / 9_000.0).abs() < 1e-4);
    }
}
