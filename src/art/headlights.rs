//! Car lights: two low-beam spots and the 4-lamp pod (toggled by the lights key, on by
//! themselves once the sun is below 2°), emissive lenses, brake and tail lamps.
//!
//! **Owner: stream D (CAR FEEL).** Design: `docs/festival/ART_BIBLE.md` §2.3 "Night
//! requirements", `docs/festival/TECH_PASS.md` §4/§7 (the shadowed spot is spawned at load with
//! intensity 0 by day; never toggle `shadows_enabled`), `docs/festival/DECISIONS.md` L6-L7.
//!
//! The spots are children of the player's car, spawned once with the car: the left low beam
//! casts shadows on High and Ultra (one shadowed spot, decided at spawn and never toggled),
//! the right beam and the pod don't. Switching the lights only changes intensities. The lens
//! glow and the tail lamps are in `art::car` (`CarWear`), which reads [`Headlights`].

use bevy::input::gamepad::GamepadButton;
use bevy::prelude::*;
use engine::actions::{ActionDef, ActionState, AddAction, Binding};
use engine::render_budget::Actor;
use engine::settings::{FieldDef, FieldKind, RegisterSettings, add_settings_section};
use engine::ui::prelude::*;
use serde::{Deserialize, Serialize};

use crate::drive::{CarSpec, DrivenCar};

/// The car has working headlights. **Its existence is the capability flag**: C offers the
/// Night start time only when this resource exists (`daytime::night_available`).
/// [`HeadlightsPlugin`] inserts it.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Headlights {
    /// The lights are switched on.
    pub on: bool,
}

/// Settings › Driving › Headlights: Auto (on below a 2° sun), On, Off. Section `"car_lights"`.
#[derive(Resource, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LightsPrefs {
    /// 0 Auto, 1 On, 2 Off.
    pub mode: u32,
}

/// Low beams (each), the pod (one merged spot for its four lamps): lumens, range (m), outer
/// and inner half-angles (rad).
pub const LOW_BEAM: (f32, f32, f32, f32) = (12_000.0, 80.0, 0.45, 0.22);
pub const POD: (f32, f32, f32, f32) = (40_000.0, 160.0, 0.22, 0.09);
/// Halogen-white.
const BEAM_COLOUR: [f32; 3] = [1.0, 0.93, 0.82];
/// Lamps in the model's frame (origin at hub height, nose towards −Z): the headlamps' centres
/// and the pod on the bonnet.
const LAMP_L: Vec3 = Vec3::new(-0.55, 0.37, -2.1);
const LAMP_R: Vec3 = Vec3::new(0.55, 0.37, -2.1);
const POD_AT: Vec3 = Vec3::new(0.0, 0.66, -1.95);
/// The beams dip 1.5° (low beam), the pod 1°.
const BEAM_DIP: f32 = 0.026;
const POD_DIP: f32 = 0.017;
/// Sun elevation (°) the lights come on below, and go off above.
const AUTO_ON: f32 = 2.0;
const AUTO_OFF: f32 = 4.0;

pub struct HeadlightsPlugin;

impl Plugin for HeadlightsPlugin {
    fn build(&self, app: &mut App) {
        add_settings_section::<LightsPrefs>(app, "car_lights");
        app.settings_defaults::<LightsPrefs>()
            .insert_resource(Headlights::default())
            .add_action(ActionDef::button("lights").label("Lights").group("Driving").bind(Binding::key(KeyCode::KeyL)).bind(Binding::pad(GamepadButton::DPadRight)))
            .register_setting(FieldDef {
                key: "headlights",
                tab: "Driving",
                label: "Headlights",
                hint: "Auto turns them on at dusk and off after dawn. L (D-pad right) switches them while driving.",
                kind: FieldKind::Choice { options: vec!["Auto", "On", "Off"] },
                get: |w| w.resource::<LightsPrefs>().mode as f32,
                set: |w, v| w.resource_mut::<LightsPrefs>().mode = (v.round().max(0.0) as u32).min(2),
            })
            .add_systems(Update, (switch, spawn_lights, drive_lights).chain());
    }
}

/// Whether the lights should be on, given the mode, the sun and the last state (auto has a
/// little hysteresis so they don't flicker at 2°).
pub fn auto_on(mode: u32, sun_el: f32, was: bool) -> bool {
    match mode {
        1 => true,
        2 => false,
        _ if sun_el < AUTO_ON => true,
        _ if sun_el > AUTO_OFF => false,
        _ => was,
    }
}

#[derive(Default)]
struct Switch {
    /// The auto/setting state last frame, and the player's own switch on top of it (cleared
    /// when the auto state changes, e.g. at dusk).
    auto: Option<bool>,
    manual: Option<bool>,
}

/// The lights key, the setting and the sun → [`Headlights`].
fn switch(prefs: Option<Res<LightsPrefs>>, clock: Option<Res<crate::daytime::DayClock>>, actions: Option<Res<ActionState>>, mut lights: ResMut<Headlights>, mut st: Local<Switch>) {
    let mode = prefs.map_or(0, |p| p.mode);
    let el = clock.map_or(30.0, |c| c.sun().1);
    let auto = auto_on(mode, el, st.auto.unwrap_or(false));
    if st.auto != Some(auto) {
        st.auto = Some(auto);
        st.manual = None;
    }
    if actions.is_some_and(|a| a.just_pressed("lights")) {
        st.manual = Some(!lights.on);
    }
    let on = st.manual.unwrap_or(auto);
    if lights.on != on {
        lights.on = on;
    }
}

/// The car's lights (children of the car entity).
#[derive(Component, Clone, Copy, Debug)]
pub struct CarLights {
    pub beams: [Entity; 2],
    pub pod: Entity,
}

fn spot(colour: [f32; 3], (_, range, outer, inner): (f32, f32, f32, f32), shadows: bool) -> SpotLight {
    SpotLight {
        color: Color::srgb(colour[0], colour[1], colour[2]),
        intensity: 0.0,
        range,
        radius: 0.06,
        outer_angle: outer,
        inner_angle: inner,
        shadow_maps_enabled: shadows,
        ..default()
    }
}

/// Spawns the player car's spots once (intensity 0; the left beam shadowed on High+).
fn spawn_lights(mut commands: Commands, spec: Option<Res<CarSpec>>, core: Option<Res<engine::settings::CoreSettings>>, cars: Query<Entity, (With<DrivenCar>, Without<CarLights>)>) {
    let spec = spec.map(|s| s.0.clone()).unwrap_or_default();
    let high = core.is_some_and(|c| c.quality.index() >= 2);
    let off = crate::art::car::model_offset(&spec);
    for car in &cars {
        let aim = |at: Vec3, dip: f32| Transform::from_translation(at + off).looking_to(Vec3::new(0.0, -dip, -1.0), Vec3::Y);
        let beams = [(LAMP_L, high), (LAMP_R, false)].map(|(at, shadows)| commands.spawn((Name::new("low beam"), spot(BEAM_COLOUR, LOW_BEAM, shadows), aim(at, BEAM_DIP), Actor)).id());
        let pod = commands.spawn((Name::new("light pod"), spot(BEAM_COLOUR, POD, false), aim(POD_AT, POD_DIP), Actor)).id();
        commands.entity(car).add_children(&[beams[0], beams[1], pod]).insert(CarLights { beams, pod });
    }
}

/// Lights on or off: only the intensities change.
fn drive_lights(lights: Res<Headlights>, cars: Query<Ref<CarLights>>, mut spots: Query<&mut SpotLight>) {
    for c in &cars {
        if !lights.is_changed() && !c.is_added() {
            continue;
        }
        let k = if lights.on { 1.0 } else { 0.0 };
        for (e, lm) in [(c.beams[0], LOW_BEAM.0), (c.beams[1], LOW_BEAM.0), (c.pod, POD.0)] {
            if let Ok(mut s) = spots.get_mut(e) {
                s.intensity = lm * k;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_lights_follow_the_sun_without_flicker() {
        assert!(auto_on(0, -5.0, false));
        assert!(!auto_on(0, 20.0, true));
        // Between 2° and 4° it keeps what it was.
        assert!(auto_on(0, 3.0, true));
        assert!(!auto_on(0, 3.0, false));
        assert!(auto_on(1, 40.0, false), "always on");
        assert!(!auto_on(2, -20.0, true), "always off");
    }

    #[test]
    fn lamps_sit_at_the_nose() {
        let s = crate::vehicle::Spec::default();
        let off = crate::art::car::model_offset(&s);
        for at in [LAMP_L, LAMP_R, POD_AT] {
            assert!((at + off).z < s.mounts[0].z - 0.5, "ahead of the front axle: {at}");
        }
    }
}
