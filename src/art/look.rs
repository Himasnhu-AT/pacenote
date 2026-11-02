//! Sky and atmosphere per stage: time of day sets the sun (angle, colour, strength) and the
//! sky; weather sets cloud cover (gradient sky), fog distance and the wet look. Built on
//! `engine::look` presets, as data.

use engine::look::{FogMode, GradeSection, Look, Rgb, SkyMode, SunLook};

use crate::stage::{StageDef, TimeOfDay, Weather};

/// Metres at which the scene is hidden by fog (also where scenery and trees stop drawing).
pub fn view_distance(def: &StageDef) -> f32 {
    match def.weather {
        Weather::Clear => 1400.0,
        Weather::Overcast => 900.0,
        Weather::Rain => 520.0,
        Weather::Fog => 210.0,
    }
}

/// The stage's view distance at graphics quality `q` (0 = Low … 3 = Ultra): Low and Medium
/// draw less of the far forest (fog closes in to match). Never below 350 m, or the stage's own
/// distance when that is shorter (fog stages).
pub fn quality_view(base: f32, q: usize) -> f32 {
    const SCALE: [f32; 4] = [0.45, 0.7, 1.0, 1.0];
    (base * SCALE[q.min(3)]).max(base.min(350.0))
}

/// The look for a stage, fogged at `vis` metres (see [`view_distance`], [`quality_view`]).
pub fn stage_look(def: &StageDef, vis: f32) -> Look {
    let (az, el, sun_col, lux) = match def.time {
        TimeOfDay::Morning => (105.0, 22.0, Rgb::new(1.0, 0.88, 0.74), 65_000.0),
        TimeOfDay::Noon => (170.0, 58.0, Rgb::new(1.0, 0.97, 0.93), 100_000.0),
        TimeOfDay::Afternoon => (235.0, 30.0, Rgb::new(1.0, 0.92, 0.82), 80_000.0),
        TimeOfDay::Dusk => (262.0, 9.0, Rgb::new(1.0, 0.7, 0.48), 30_000.0),
    };
    let base = match def.time {
        TimeOfDay::Dusk => Look::dusk(),
        _ => Look::clear_day(),
    };
    let mut look = Look {
        name: format!("stage {}", def.id).into(),
        sun: SunLook { azimuth_deg: az, elevation_deg: el, color: sun_col, illuminance: lux, shadow_distance: 140.0, cascades: 4, first_cascade: 12.0, ..base.sun.clone() },
        ..base
    };
    // Forest roads sit in shade most of the time: more sky light than the generic presets.
    look.ambient.brightness = match def.time {
        TimeOfDay::Dusk => 1100.0,
        _ => 2400.0,
    };
    if def.time == TimeOfDay::Dusk {
        // The engine's dusk is a postcard; a stage at dusk still needs to read.
        look.grading.temperature = 0.0;
        look.grading.post_saturation = 1.0;
        look.fog.color = Rgb::new(0.62, 0.56, 0.55);
        look.ambient.color = Rgb::new(0.6, 0.66, 0.9);
    }
    // Exposure follows the light (sunny 16 rule, a bit brighter for low sun).
    look.exposure.ev100 = match def.time {
        TimeOfDay::Noon => 14.6,
        TimeOfDay::Afternoon => 14.2,
        TimeOfDay::Morning => 13.1,
        TimeOfDay::Dusk => 10.8,
    };
    look.fog.visibility = vis;
    match def.weather {
        Weather::Clear => {
            look.fog.mode = FogMode::Atmospheric;
        }
        Weather::Overcast | Weather::Rain | Weather::Fog => {
            let (grey, bright, sun_k, ev) = match def.weather {
                Weather::Overcast => (0.72, 3600.0, 0.2, 12.2),
                Weather::Rain => (0.56, 2400.0, 0.1, 11.6),
                _ => (0.8, 3000.0, 0.12, 12.0),
            };
            let dusk = def.time == TimeOfDay::Dusk;
            let warm = if dusk { Rgb::new(1.0, 0.82, 0.7) } else { Rgb::new(1.0, 1.0, 1.0) };
            let tint = |r: f32, g: f32, b: f32| Rgb::new(r * warm.0[0], g * warm.0[1], b * warm.0[2]);
            look.sky.mode = SkyMode::Gradient;
            look.sky.zenith = tint(grey * 0.82, grey * 0.85, grey * 0.9);
            look.sky.horizon = tint(grey, grey * 1.01, grey * 1.03);
            look.sky.ground = Rgb::new(0.28, 0.28, 0.27);
            look.sky.brightness = if dusk { bright * 0.35 } else { bright };
            look.sky.sun_glow = 0.12;
            look.sun.illuminance = lux * sun_k;
            look.sun.disk_intensity = 0.0;
            look.sun.color = Rgb::new(0.95, 0.96, 1.0);
            // A high diffuse sun reads as cloud light; keep the low sun's direction.
            look.sun.elevation_deg = el.max(40.0);
            look.ambient.brightness = if dusk { 500.0 } else { 1400.0 };
            look.fog.mode = FogMode::Exponential;
            look.fog.color = tint(grey * 0.95, grey * 0.97, grey);
            look.fog.sun_amount = 0.0;
            look.exposure.ev100 = if dusk { ev - 1.6 } else { ev };
            look.grading.post_saturation = 0.86;
            look.grading.temperature = -0.04;
            look.grading.midtones = GradeSection { contrast: 1.04, ..GradeSection::default() };
            look.bloom.intensity = 0.06;
        }
    }
    look
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_stage_has_a_look_with_fog_inside_view() {
        for def in crate::stage::stages() {
            let l = stage_look(&def, view_distance(&def));
            assert!(l.fog.visibility <= view_distance(&def) + 1.0);
            assert!(l.sun.illuminance > 1000.0, "stage {}", def.id);
            assert!(l.exposure.ev100 > 9.0);
            // The look round-trips as data (it goes over the control port).
            let text = ron::to_string(&l).unwrap();
            let back: Look = ron::from_str(&text).unwrap();
            assert_eq!(back, l);
        }
    }
}
