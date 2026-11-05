//! Sky and atmosphere: the time of day (`daytime::DayClock`) sets the sun (solar formula), its
//! colour and strength, exposure, haze and fill light from keyframes; weather sets cloud cover
//! (gradient sky), fog distance and the wet look. Built on `engine::look`, as data.
//!
//! **Owner: stream C (LOOK).** Design: `docs/festival/ART_BIBLE.md` §2 (diagnosis, keyframes),
//! `docs/festival/TECH_PASS.md` §1 (write `Look` about once a second, env refresh every 20 s,
//! never switch `SkyMode` in play), `docs/festival/DECISIONS.md` L1-L4.
//!
//! The clock moves the look through [`drive_look`]: it rewrites `Look` only when the sun has
//! moved one 0.02° quantum (about once a real second at 5×), so `apply_look` runs ≈ 1 Hz and
//! the sky's env light refilters at most every 20 s (`LookConfig::env_refresh_hz`).
//!
//! Night: the sun light is re-aimed as the moon (opposite azimuth, 35° up, 0.6 lux, blue), so
//! the atmosphere stays on and goes dark by itself; stars come from the cloud shader. Between
//! sunset and the moon the light is held at −2.5° (just under the horizon) and dimmed with the
//! keyframes, which keeps the twilight glow in the atmosphere for the length of a real dusk.

use bevy::prelude::*;
use engine::look::{ExposureLook, FogMode, GradeSection, Look, LookConfig, Rgb, SkyMode, SunLook, ToneMap};

use crate::daytime::{DayClock, solar, stage_hours};
use crate::stage::{StageDef, Weather};
use crate::world::StageWorld;

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

/// One keyframe of the day (ART_BIBLE §2.3, tuned in game). The light is not keyed: the sun's
/// direction comes from the solar formula and its colour and strength from the atmosphere.
#[derive(Clone, Copy, Debug)]
struct Key {
    /// Clock hour.
    h: f32,
    /// Base EV100 (auto exposure adapts in a narrow window around it).
    ev: f32,
    /// Haze: fog visibility (m) and its colour (display-referred: what the far horizon looks
    /// like after exposure), and the colour towards the sun.
    vis: f32,
    haze: [f32; 3],
    glow: [f32; 3],
    /// Flat fill light (cd/m²) and its colour.
    amb: f32,
    amb_col: [f32; 3],
    /// Cloud coverage 0..1 (`art::sky`).
    cover: f32,
}

const NIGHT_KEY: Key = Key {
    h: 0.0,
    ev: 1.6,
    vis: 1600.0,
    haze: [0.022, 0.028, 0.045],
    glow: [0.05, 0.06, 0.09],
    amb: 0.3,
    amb_col: [0.45, 0.55, 1.0],
    cover: 0.3,
};

/// Sunlight above the atmosphere (lux) and its colour: Bevy's atmosphere reddens and dims it
/// for the sky and for every lit surface (transmittance), so the keys don't.
const RAW_SUN: f32 = 110_000.0;
const SUN_WHITE: [f32; 3] = [1.0, 0.98, 0.95];

/// The day, in clock order (wraps at 24 h). Exposure, haze and fill per time; the light itself
/// is the raw sun (or the moon, [`light_at`]).
const KEYS: [Key; 13] = [
    NIGHT_KEY,
    Key { h: 4.2, ..NIGHT_KEY },
    // Sun ≈ −7.5°: blue hour.
    Key { h: 4.8, ev: 3.6, vis: 2000.0, haze: [0.06, 0.065, 0.1], glow: [0.16, 0.12, 0.13], amb: 2.0, amb_col: [0.5, 0.58, 0.95], cover: 0.35 },
    // Sun ≈ −3.5°: dawn glow.
    Key { h: 5.35, ev: 7.6, vis: 3000.0, haze: [0.3, 0.3, 0.38], glow: [0.7, 0.5, 0.4], amb: 40.0, amb_col: [0.55, 0.62, 0.9], cover: 0.35 },
    // Sun ≈ +3.7°: low gold, a little valley mist.
    Key { h: 6.0, ev: 10.6, vis: 4000.0, haze: [0.72, 0.68, 0.68], glow: [1.0, 0.72, 0.5], amb: 150.0, amb_col: [0.66, 0.72, 0.95], cover: 0.35 },
    // Sun ≈ +8.5°.
    Key { h: 6.5, ev: 11.6, vis: 5000.0, haze: [0.74, 0.74, 0.76], glow: [1.0, 0.82, 0.62], amb: 230.0, amb_col: [0.7, 0.74, 0.95], cover: 0.35 },
    // Sun 34°.
    Key { h: 9.0, ev: 13.6, vis: 8000.0, haze: [0.7, 0.77, 0.88], glow: [1.0, 0.94, 0.82], amb: 450.0, amb_col: [0.75, 0.82, 1.0], cover: 0.38 },
    // Sun 60°.
    Key { h: 12.0, ev: 14.4, vis: 9000.0, haze: [0.72, 0.79, 0.9], glow: [1.0, 0.96, 0.88], amb: 600.0, amb_col: [0.78, 0.84, 1.0], cover: 0.5 },
    // Sun 45°.
    Key { h: 16.0, ev: 14.1, vis: 8000.0, haze: [0.72, 0.78, 0.88], glow: [1.0, 0.94, 0.84], amb: 550.0, amb_col: [0.78, 0.83, 1.0], cover: 0.45 },
    // Sun 13.5°.
    Key { h: 19.0, ev: 12.4, vis: 5500.0, haze: [0.76, 0.74, 0.74], glow: [1.0, 0.8, 0.58], amb: 300.0, amb_col: [0.68, 0.72, 0.95], cover: 0.4 },
    // Sun ≈ +3.7°: sunset.
    Key { h: 20.0, ev: 10.4, vis: 4000.0, haze: [0.72, 0.64, 0.62], glow: [1.0, 0.66, 0.42], amb: 140.0, amb_col: [0.62, 0.66, 0.95], cover: 0.4 },
    // Sun ≈ −1°: dusk glow.
    Key { h: 20.5, ev: 8.8, vis: 3000.0, haze: [0.38, 0.36, 0.44], glow: [0.8, 0.52, 0.4], amb: 70.0, amb_col: [0.55, 0.6, 0.95], cover: 0.35 },
    // Sun ≈ −7.5°: blue hour.
    Key { h: 21.3, ev: 3.6, vis: 2000.0, haze: [0.06, 0.065, 0.1], glow: [0.16, 0.12, 0.13], amb: 2.0, amb_col: [0.5, 0.58, 0.95], cover: 0.35 },
];

fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn log_lerp(a: f32, b: f32, t: f32) -> f32 {
    engine::look::desc::log_lerp(a.max(1e-4), b.max(1e-4), t)
}

/// The keyframes blended at clock hour `hours` (fill light and visibility in log space, the
/// rest linearly).
fn sample(hours: f32) -> Key {
    let h = hours.rem_euclid(24.0);
    let n = KEYS.len();
    let i = KEYS.iter().rposition(|k| k.h <= h).unwrap_or(n - 1);
    let (a, b) = (KEYS[i], KEYS[(i + 1) % n]);
    let span = (b.h - a.h).rem_euclid(24.0).max(1e-3);
    let t = ((h - a.h).rem_euclid(24.0) / span).clamp(0.0, 1.0);
    // Smooth the corners so colour and brightness don't kink at a key.
    let t = t * t * (3.0 - 2.0 * t);
    Key {
        h,
        ev: a.ev + (b.ev - a.ev) * t,
        vis: log_lerp(a.vis, b.vis, t),
        haze: lerp3(a.haze, b.haze, t),
        glow: lerp3(a.glow, b.glow, t),
        amb: log_lerp(a.amb, b.amb, t),
        amb_col: lerp3(a.amb_col, b.amb_col, t),
        cover: a.cover + (b.cover - a.cover) * t,
    }
}

/// Cloud coverage (0..1) at clock hour `hours` for `weather` (`art::sky`).
pub fn cloud_cover(hours: f32, weather: Weather) -> f32 {
    match weather {
        Weather::Clear => sample(hours).cover,
        _ => 0.9,
    }
}

/// Moon: the sun light below this sun elevation (degrees), fully in by [`MOON_FULL`].
const MOON_FROM: f32 = -10.0;
const MOON_FULL: f32 = -13.0;
/// Twilight: the light is held here (just under the horizon) while the sun is lower, and
/// dimmed a decade per [`TWILIGHT_DECADE`] degrees (the sky's brightness through dusk).
const TWILIGHT_EL: f32 = -2.5;
const TWILIGHT_DECADE: f32 = 1.9;
/// Moonlight (lux, colour; ART_BIBLE §2.3: ≈ 2× a real full moon, so night stays drivable).
pub const MOON_LUX: f32 = 0.6;
pub const MOON_COLOR: [f32; 3] = [0.62, 0.72, 1.0];

/// What the light is doing at sun elevation `el`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Light {
    pub azimuth: f32,
    pub elevation: f32,
    /// Lux the engine must be given (before its below-horizon fade); above the atmosphere.
    pub lux: f32,
    pub color: [f32; 3],
    /// 1 when the light is the moon.
    pub moon: f32,
}

/// The directional light at clock hour `hours`: the raw sun, held at the horizon and dimmed
/// through twilight, then the moon.
pub fn light_at(hours: f32) -> Light {
    let (az, el) = solar(hours);
    if el <= MOON_FROM {
        let m = ((MOON_FROM - el) / (MOON_FROM - MOON_FULL)).clamp(0.0, 1.0);
        // Opposite the sun, fixed height: a steady moon for the whole night.
        return Light { azimuth: (az + 180.0).rem_euclid(360.0), elevation: 35.0, lux: (MOON_LUX * m).max(1e-3), color: MOON_COLOR, moon: 1.0 };
    }
    let lit_el = el.max(TWILIGHT_EL);
    let dim = 10f32.powf((el.min(TWILIGHT_EL) - TWILIGHT_EL) / TWILIGHT_DECADE);
    // The engine fades the light out from 0° to −3°; give it back so the sky keeps its glow.
    let fade = ((lit_el + 3.0) / 3.0).clamp(0.05, 1.0);
    Light { azimuth: az, elevation: lit_el, lux: RAW_SUN * dim / fade, color: SUN_WHITE, moon: 0.0 }
}

/// The look at clock hour `hours` in `weather`, fogged at `vis` metres (see
/// [`view_distance`], [`quality_view`]).
pub fn day_look(hours: f32, weather: Weather, vis: f32) -> Look {
    let k = sample(hours);
    let light = light_at(hours);
    let (_, true_el) = solar(hours);
    let base = Look::clear_day();
    let low = light.moon == 0.0 && light.elevation < 8.0;
    let mut look = Look {
        name: format!("day {hours:.2}").into(),
        sun: SunLook {
            azimuth_deg: light.azimuth,
            elevation_deg: light.elevation,
            color: Rgb::new(light.color[0], light.color[1], light.color[2]),
            illuminance: light.lux,
            // Moon: a short shadow range; low sun: long shadows waste resolution.
            shadow_distance: if light.moon > 0.0 {
                60.0
            } else if low {
                110.0
            } else {
                160.0
            },
            cascades: if light.moon > 0.0 { 2 } else { 4 },
            first_cascade: 10.0,
            disk_size_deg: if light.moon > 0.0 { 0.55 } else { 0.53 },
            // The sun's disk sets with it; the moon's is the same disk, dimmer light.
            disk_intensity: if light.moon > 0.0 { 1.0 } else { ((true_el + 1.0) / 2.0).clamp(0.0, 1.0) },
            ..base.sun.clone()
        },
        ..base
    };
    look.ambient.brightness = k.amb;
    look.ambient.color = Rgb::new(k.amb_col[0], k.amb_col[1], k.amb_col[2]);
    look.ambient.env_intensity = if light.moon > 0.0 { 0.8 } else { 1.0 };
    look.exposure = ExposureLook {
        ev100: k.ev,
        auto: true,
        // Narrow window around the keyed exposure (ART_BIBLE §2.2): the histogram is of the
        // exposed image, so the window sits around the compensation target.
        auto_min: EXPOSURE_TARGET - 1.0,
        auto_max: EXPOSURE_TARGET + 1.5,
        speed_brighten: 1.5,
        speed_darken: 2.5,
        compensation: EXPOSURE_TARGET,
    };
    // Haze matched to the horizon colour of the time of day: the far ground melts into the sky
    // instead of turning grey under a blue sky.
    look.fog.mode = FogMode::Atmospheric;
    look.fog.visibility = vis.max(k.vis);
    look.fog.color = Rgb::new(k.haze[0], k.haze[1], k.haze[2]);
    // Extinction: blue goes first (aerial perspective tints distance blue, not grey).
    look.fog.extinction = Rgb::new(0.55, 0.62, 0.72);
    look.fog.sun_color = Rgb::new(k.glow[0], k.glow[1], k.glow[2]);
    look.fog.sun_amount = if light.moon > 0.0 { 0.15 } else { 0.55 };
    look.fog.sun_exponent = 18.0;
    // Khronos PBR Neutral keeps the hue of a bright sky: a deep blue zenith over a pale
    // horizon, where Tony McMapface and AgX wash it to grey-lavender (compared in game).
    look.tonemapping = ToneMap::KhronosPbrNeutral;
    look.grading.temperature = 0.01;
    look.grading.post_saturation = 1.0;
    look.grading.midtones = GradeSection { contrast: 1.06, ..GradeSection::default() };
    look.bloom.intensity = 0.1;
    if light.moon > 0.0 {
        // Night: cool and desaturated (scotopic), a touch of vignette.
        look.grading.temperature = -0.06;
        look.grading.post_saturation = 0.72;
        look.bloom.intensity = 0.16;
    }
    match weather {
        Weather::Clear => {}
        Weather::Overcast | Weather::Rain | Weather::Fog => overcast(&mut look, &k, weather, true_el),
    }
    look
}

/// Exposure compensation (EV) the auto exposure aims the scene's average at; the keyed EV100s
/// are set so this lands near them.
const EXPOSURE_TARGET: f32 = -2.2;

/// Grey weather on top of the keyed look: a flat gradient sky (no clouds, never switched in
/// play), weak diffuse sun, short fog.
fn overcast(look: &mut Look, k: &Key, weather: Weather, true_el: f32) {
    let (grey, bright, sun_k, ev) = match weather {
        Weather::Overcast => (0.72, 3600.0, 0.2, 12.2),
        Weather::Rain => (0.56, 2400.0, 0.1, 11.6),
        _ => (0.8, 3000.0, 0.12, 12.0),
    };
    // Dimmer as the day goes: the clear key's exposure relative to noon.
    let day = ((k.ev - 8.0) / (14.4 - 8.0)).clamp(0.05, 1.0);
    let dusk = true_el < 12.0;
    let warm = if dusk { Rgb::new(1.0, 0.82, 0.7) } else { Rgb::new(1.0, 1.0, 1.0) };
    let tint = |r: f32, g: f32, b: f32| Rgb::new(r * warm.0[0], g * warm.0[1], b * warm.0[2]);
    look.sky.mode = SkyMode::Gradient;
    look.sky.zenith = tint(grey * 0.82, grey * 0.85, grey * 0.9);
    look.sky.horizon = tint(grey, grey * 1.01, grey * 1.03);
    look.sky.ground = Rgb::new(0.28, 0.28, 0.27);
    look.sky.brightness = if dusk { bright * 0.35 } else { bright } * day;
    look.sky.sun_glow = 0.12;
    look.sun.illuminance *= sun_k;
    look.sun.disk_intensity = 0.0;
    look.sun.color = Rgb::new(0.95, 0.96, 1.0);
    // A high diffuse sun reads as cloud light; keep the low sun's direction.
    look.sun.elevation_deg = look.sun.elevation_deg.max(40.0);
    look.ambient.brightness = if dusk { 500.0 } else { 1400.0 } * day;
    look.fog.mode = FogMode::Exponential;
    look.fog.color = tint(grey * 0.95, grey * 0.97, grey);
    look.fog.sun_amount = 0.0;
    // After sunset it keeps darkening towards night.
    let night = ((2.0 - true_el) / 10.0).clamp(0.0, 1.0);
    look.exposure.ev100 = if dusk { ev - 1.6 } else { ev } - night * 8.0;
    look.exposure.auto = false;
    look.grading.post_saturation = 0.86;
    look.grading.temperature = -0.04;
    look.grading.midtones = GradeSection { contrast: 1.04, ..GradeSection::default() };
    look.bloom.intensity = 0.06;
}

/// The look for a stage at its authored time of day, fogged at `vis` metres. Free roam's
/// clock then moves it ([`drive_look`]).
pub fn stage_look(def: &StageDef, vis: f32) -> Look {
    day_look(stage_hours(def.time), def.weather, vis)
}

/// How bright fake fill lights tuned for daylight should be at clock hour `hours` (1 at
/// noon): the keyed exposure relative to noon's. A fixed-power fill (the cabin's) would
/// otherwise glare at dusk and night, when the exposure opens up by 10+ stops.
pub fn daylight_fill_scale(hours: f32) -> f32 {
    2f32.powf(sample(hours).ev - 14.4).clamp(1e-4, 1.0)
}

/// Clock hours per 0.02° of sun travel (15°/h): the look is rewritten once per step.
const QUANTUM_H: f32 = 0.02 / 15.0;

/// Look-writing bookkeeping (`look.get` shows the apply count; this counts our writes).
#[derive(Resource, Default, Debug)]
pub struct DayLookState {
    /// (stage id, `StageWorld` insert tick, clock quantum) last written.
    last: Option<(u32, u32, i64)>,
    pub writes: u64,
}

/// Rewrites the `Look` from the clock when the sun has moved a quantum (or the stage changed).
pub fn drive_look(
    clock: Res<DayClock>,
    sw: Option<Res<StageWorld>>,
    core: Option<Res<engine::settings::CoreSettings>>,
    look: Option<ResMut<Look>>,
    mut st: ResMut<DayLookState>,
) {
    let (Some(sw), Some(mut look)) = (sw, look) else { return };
    let q = (clock.hours / QUANTUM_H).floor() as i64;
    let key = (sw.def.id, sw.added().get(), q);
    // A look inserted after our last write (stage load) is replaced too.
    if st.last == Some(key) && !look.is_added() {
        return;
    }
    st.last = Some(key);
    st.writes += 1;
    let quality = core.map_or(2, |c| c.quality.index());
    let vis = quality_view(view_distance(&sw.def), quality);
    *look = day_look(q as f32 * QUANTUM_H, sw.def.weather, vis);
}

/// Sky-light refresh rate in play (Hz): once per 20 s (TECH_PASS §1).
const ENV_REFRESH_HZ: f32 = 0.05;

/// Until this real time the env light may refilter quickly (after a clock jump).
#[derive(Resource, Default)]
struct EnvBoost(f64);

/// Lets the sky light catch up with a jump of the clock (dev `look.time`) within a second.
pub fn refresh_env_soon(world: &mut World) {
    let now = world.get_resource::<Time<Real>>().map_or(0.0, |t| t.elapsed_secs_f64());
    if let Some(mut b) = world.get_resource_mut::<EnvBoost>() {
        b.0 = now + 3.0;
    }
}

fn env_cadence(time: Res<Time<Real>>, boost: Res<EnvBoost>, config: Option<ResMut<LookConfig>>) {
    let Some(mut config) = config else { return };
    let want = if time.elapsed_secs_f64() < boost.0 { 4.0 } else { ENV_REFRESH_HZ };
    if config.env_refresh_hz != want {
        config.env_refresh_hz = want;
    }
}

/// The clock → look systems (added by `art::sky::SkyPlugin`).
pub fn register(app: &mut App) {
    app.init_resource::<DayLookState>()
        .init_resource::<EnvBoost>()
        .add_systems(Update, (drive_look.after(crate::daytime::DayClockSet), env_cadence).before(engine::gfx::QualitySet::Look));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_stage_has_a_look_with_fog_inside_view() {
        for def in crate::stage::stages() {
            let l = stage_look(&def, view_distance(&def));
            assert!(l.sun.illuminance > 1000.0, "stage {}", def.id);
            assert!(l.exposure.ev100 > 9.0);
            // The look round-trips as data (it goes over the control port).
            let text = ron::to_string(&l).unwrap();
            let back: Look = ron::from_str(&text).unwrap();
            assert_eq!(back, l);
        }
    }

    #[test]
    fn the_day_is_smooth_and_never_switches_the_sky() {
        let mut prev = day_look(0.0, Weather::Clear, 1400.0);
        for i in 1..=24 * 60 {
            let h = i as f32 / 60.0;
            let l = day_look(h, Weather::Clear, 1400.0);
            assert_eq!(l.sky.mode, SkyMode::Atmosphere, "{h}");
            // No jumps bigger than a minute's worth of change, except the moon handover.
            let dev = (l.exposure.ev100 - prev.exposure.ev100).abs();
            assert!(dev < 0.25, "ev jump {dev} at {h}");
            assert!(l.ambient.brightness > 0.0 && l.sun.illuminance > 0.0);
            prev = l;
        }
    }

    #[test]
    fn fill_light_is_low_by_day_and_the_moon_rises_at_night() {
        let noon = day_look(12.0, Weather::Clear, 1400.0);
        assert!((350.0..=650.0).contains(&noon.ambient.brightness));
        assert!(noon.exposure.auto);
        let night = day_look(23.0, Weather::Clear, 1400.0);
        assert!((night.sun.illuminance - MOON_LUX).abs() < 1e-3);
        assert_eq!(night.sun.elevation_deg, 35.0);
        // Twilight: the light stays just under the horizon, dimmed.
        let dusk = light_at(21.0);
        assert_eq!(dusk.elevation, TWILIGHT_EL);
        assert!(dusk.moon == 0.0);
        assert_eq!(daylight_fill_scale(12.0), 1.0);
        assert!(daylight_fill_scale(23.0) < 1e-3);
    }
}
