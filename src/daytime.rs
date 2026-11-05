//! The time of day: a clock that runs at 5× real time and moves the sun, and the World tab's
//! start-time settings.
//!
//! **Owner: stream C (LOOK).** Design: `docs/festival/ART_BIBLE.md` §2.3 (sun path, keyframes)
//! with `docs/festival/TECH_PASS.md` §1 (write `Look` about once a second, no `LookTimeline`,
//! never switch `SkyMode`) and `docs/festival/DECISIONS.md` L1-L8.
//!
//! Look-only: the sim never reads the clock, and it is not part of any event's condition key
//! (TECH_PASS §5). Free roam starts at the World › Start time and the clock runs (unless
//! World › Time passes is off); classic stages freeze it at their authored `TimeOfDay`.
//! `art::look::drive_look` turns the clock into the `Look`.

use bevy::prelude::*;
use engine::flow::AppState;
use engine::settings::{FieldDef, FieldKind, RegisterSettings, SettingsRegistry, add_settings_section};
use engine::ui::prelude::*;
use serde::{Deserialize, Serialize};

use crate::stage::TimeOfDay;
use crate::world::StageWorld;

/// Game seconds per real second (one day ≈ 4 h 48 min).
pub const DAY_RATE: f32 = 5.0;

/// The World › Start time choices, in `DaySettings::start` order: name and game hour (`None`:
/// the wall clock / the saved clock).
pub const START_PRESETS: [(&str, Option<f32>); 8] = [
    ("Dawn", Some(6.0)),
    ("Morning", Some(9.0)),
    ("Noon", Some(12.5)),
    ("Afternoon", Some(16.0)),
    ("Sunset", Some(19.5)),
    ("Night", Some(23.0)),
    ("Now", None),
    ("Last", None),
];
/// Index of "Night" in [`START_PRESETS`]: offered only when [`night_available`].
pub const NIGHT: u32 = 5;
/// Index of "Real clock" and "Last" in [`START_PRESETS`].
pub const REAL_CLOCK: u32 = 6;
pub const LAST: u32 = 7;
/// Where a night start goes while night isn't offered (Sunset).
const NIGHT_FALLBACK: f32 = 19.5;

/// Latitude of the valley (degrees north) and the sun's declination (late May: sunrise ≈
/// 05:37, sunset ≈ 20:23 with solar noon at 13:00 summer time).
const LATITUDE: f32 = 46.0;
const DECLINATION: f32 = 19.0;
/// Clock hour of solar noon (summer time).
const SOLAR_NOON: f32 = 13.0;

/// The game clock. Advanced on `Time<Virtual>` in `Update` (pause freezes it).
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct DayClock {
    /// 0..24.
    pub hours: f32,
    /// Game seconds per real second.
    pub rate: f32,
    pub running: bool,
}

impl Default for DayClock {
    fn default() -> Self {
        DayClock { hours: 16.0, rate: DAY_RATE, running: false }
    }
}

impl DayClock {
    /// Sun azimuth (0 = north = −Z, 90 = east = +X) and elevation, degrees, from the solar
    /// formula (46° N, declination 19°, solar noon 13:00).
    pub fn sun(&self) -> (f32, f32) {
        solar(self.hours)
    }
    /// 0 at night .. 1 in full day, from the sun's elevation (−6° .. +6°).
    pub fn daylight(&self) -> f32 {
        let (_, el) = self.sun();
        ((el + 6.0) / 12.0).clamp(0.0, 1.0)
    }
    pub fn is_night(&self) -> bool {
        self.daylight() < 0.05
    }
    /// Moves the clock on by `real_secs` real seconds (wraps at 24 h).
    pub fn advance(&mut self, real_secs: f32) {
        if self.running {
            self.hours = (self.hours + real_secs * self.rate / 3600.0).rem_euclid(24.0);
        }
    }
}

/// Sun (azimuth, elevation) in degrees at clock hour `hours`.
pub fn solar(hours: f32) -> (f32, f32) {
    let (lat, dec) = (LATITUDE.to_radians(), DECLINATION.to_radians());
    let h = (15.0 * (hours - SOLAR_NOON)).to_radians();
    let sin_el = lat.sin() * dec.sin() + lat.cos() * dec.cos() * h.cos();
    let el = sin_el.clamp(-1.0, 1.0).asin();
    // Azimuth from south, westwards; +180 makes it from north, clockwise (east = 90).
    let az = h.sin().atan2(h.cos() * lat.sin() - dec.tan() * lat.cos());
    ((az.to_degrees() + 180.0).rem_euclid(360.0), el.to_degrees())
}

/// The clock hour a classic stage freezes at (its authored light).
pub fn stage_hours(t: TimeOfDay) -> f32 {
    match t {
        TimeOfDay::Morning => 7.75,
        TimeOfDay::Noon => 13.0,
        TimeOfDay::Afternoon => 16.5,
        TimeOfDay::Dusk => 19.75,
    }
}

/// The World tab (section `"day"` of settings.json). C registers its rows (tab "World") in its
/// own plugin with `register_setting`; nothing in `prefs.rs` lists them.
#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DaySettings {
    /// Index into [`START_PRESETS`]; free roam only. Default Morning.
    pub start: u32,
    /// Off freezes the clock at the start time.
    pub time_passes: bool,
    /// The free-roam clock when last saved (for the "Last" start).
    pub last: f32,
}

impl Default for DaySettings {
    fn default() -> Self {
        DaySettings { start: 1, time_passes: true, last: 9.0 }
    }
}

/// Whether the Night start (and a night "Real clock" / "Last") may be offered: only once the
/// car has headlights (D inserts `art::headlights::Headlights`, DECISIONS L6). C's moon (the
/// sun light re-aimed, `art::look`) is in, so headlights are the only condition left.
pub fn night_available(world: &World) -> bool {
    world.contains_resource::<crate::art::headlights::Headlights>()
}

/// The hour a free-roam session starts at. `wall` is the local wall clock (hours), `night_ok`
/// whether night may be shown ([`night_available`]); a night start without it is Sunset.
pub fn start_hours(s: &DaySettings, wall: f32, night_ok: bool) -> f32 {
    let h = match s.start {
        REAL_CLOCK => wall,
        LAST => s.last,
        i => START_PRESETS.get(i as usize).and_then(|p| p.1).unwrap_or(9.0),
    }
    .rem_euclid(24.0);
    let night = DayClock { hours: h, ..default() }.is_night();
    if night && !night_ok { NIGHT_FALLBACK } else { h }
}

/// The local wall clock in hours (UTC where the platform gives no local time).
pub fn wall_hours() -> f32 {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let offset = local_offset(secs);
    ((secs + offset).rem_euclid(86_400)) as f32 / 3600.0
}

/// Seconds east of UTC for the local time zone at `secs`.
#[cfg(unix)]
fn local_offset(secs: i64) -> i64 {
    // `struct tm` starts with nine ints on every unix; `tm_gmtoff` (a long) follows on macOS,
    // Linux and the BSDs. The buffer is larger than any `struct tm`.
    #[repr(C)]
    struct Tm {
        ints: [i32; 9],
        gmtoff: std::os::raw::c_long,
        rest: [u64; 4],
    }
    unsafe extern "C" {
        fn localtime_r(t: *const i64, out: *mut Tm) -> *mut Tm;
    }
    let mut tm = Tm { ints: [0; 9], gmtoff: 0, rest: [0; 4] };
    // SAFETY: `localtime_r` reads `secs` and writes at most a `struct tm` into `tm`, which is larger.
    let ok = unsafe { !localtime_r(&secs, &mut tm).is_null() };
    // `c_long` is i64 on 64-bit targets, i32 on 32-bit ones.
    #[allow(clippy::useless_conversion)]
    let off = i64::from(tm.gmtoff);
    if ok { off } else { 0 }
}

#[cfg(not(unix))]
fn local_offset(_secs: i64) -> i64 {
    0
}

/// Where the clock is started and moved on (`art::look` reads it after).
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DayClockSet;

pub struct DayPlugin;

impl Plugin for DayPlugin {
    fn build(&self, app: &mut App) {
        add_settings_section::<DaySettings>(app, "day");
        app.settings_defaults::<DaySettings>().init_resource::<DayClock>();
        register(app);
        app.add_systems(Startup, offer_night)
            .add_systems(Update, (start_clock, tick).chain().in_set(DayClockSet))
            .add_systems(OnExit(AppState::InGame), save_last);
        #[cfg(feature = "dev")]
        dev::register(app);
    }
}

/// Settings value ↔ preset index while Night is hidden (the row then has 7 options).
fn shown_index(start: u32, night_ok: bool) -> u32 {
    if night_ok || start < NIGHT {
        start
    } else if start == NIGHT {
        NIGHT - 1
    } else {
        start - 1
    }
}

fn preset_index(shown: u32, night_ok: bool) -> u32 {
    if night_ok || shown < NIGHT { shown } else { shown + 1 }
}

fn day(w: &World) -> &DaySettings {
    w.resource::<DaySettings>()
}

fn register(app: &mut App) {
    let options: Vec<&'static str> = START_PRESETS.iter().enumerate().filter(|(i, _)| *i as u32 != NIGHT).map(|(_, p)| p.0).collect();
    app.register_setting(FieldDef {
        key: "day_start",
        tab: "World",
        label: "Start time",
        hint: "Time of day when free roam starts. Now follows your computer's clock; Last carries on from your last drive.",
        kind: FieldKind::Choice { options },
        get: |w| shown_index(day(w).start, night_available(w)) as f32,
        set: |w, v| {
            let i = preset_index(v.round().max(0.0) as u32, night_available(w)).min(START_PRESETS.len() as u32 - 1);
            w.resource_mut::<DaySettings>().start = i;
        },
    })
    .register_setting(FieldDef {
        key: "day_time_passes",
        tab: "World",
        label: "Time passes",
        hint: "The day moves on at 5x real speed: sunrise to sunset in about three hours. Off keeps the start time.",
        kind: FieldKind::Toggle,
        get: |w| if day(w).time_passes { 1.0 } else { 0.0 },
        set: |w, v| {
            let on = v > 0.5;
            w.resource_mut::<DaySettings>().time_passes = on;
            // Only the free-roam clock runs; a classic stage's stays frozen.
            let free = w.get_resource::<StageWorld>().is_some_and(|s| s.def.id == crate::open::ID);
            if let Some(mut c) = w.get_resource_mut::<DayClock>()
                && c.rate > 0.0
            {
                c.running = on && free;
            }
        },
    });
}

/// Adds "Night" to the Start time row once headlights exist (D's plugin inserts them at build).
fn offer_night(world: &mut World) {
    if !night_available(world) {
        return;
    }
    let Some(mut reg) = world.get_resource_mut::<SettingsRegistry>() else { return };
    if let Some(f) = reg.fields.iter_mut().find(|f| f.key == "day_start")
        && let FieldKind::Choice { options } = &mut f.kind
        && !options.contains(&START_PRESETS[NIGHT as usize].0)
    {
        options.insert(NIGHT as usize, START_PRESETS[NIGHT as usize].0);
    }
}

/// On each stage load: free roam starts at the setting's hour and runs; a classic stage freezes
/// at its authored time.
fn start_clock(world: &mut World, mut seen: Local<Option<(u32, u32)>>) {
    let Some(sw) = world.get_resource_ref::<StageWorld>() else {
        *seen = None;
        return;
    };
    // A new `StageWorld` (insert) is a new load, even of the same stage.
    let key = (sw.def.id, sw.added().get());
    if *seen == Some(key) {
        return;
    }
    *seen = Some(key);
    let (id, time) = (sw.def.id, sw.def.time);
    let settings = world.resource::<DaySettings>().clone();
    let clock = if id == crate::open::ID {
        let h = start_hours(&settings, wall_hours(), night_available(world));
        DayClock { hours: h, rate: DAY_RATE, running: settings.time_passes }
    } else {
        DayClock { hours: stage_hours(time), rate: DAY_RATE, running: false }
    };
    info!("daytime: clock {:.2} h, running {}", clock.hours, clock.running);
    world.insert_resource(clock);
}

/// Moves the clock on (virtual time: pause freezes it) and keeps the "Last" hour about once a
/// real minute.
fn tick(time: Res<Time<Virtual>>, real: Res<Time<Real>>, mut clock: ResMut<DayClock>, mut settings: ResMut<DaySettings>, sw: Option<Res<StageWorld>>, mut saved_at: Local<f64>) {
    if !clock.running {
        return;
    }
    clock.advance(time.delta_secs());
    let now = real.elapsed_secs_f64();
    if now - *saved_at > 60.0 && sw.is_some_and(|s| s.def.id == crate::open::ID) {
        *saved_at = now;
        settings.last = (clock.hours * 100.0).round() / 100.0;
    }
}

fn save_last(clock: Res<DayClock>, mut settings: ResMut<DaySettings>, sw: Option<Res<StageWorld>>) {
    if sw.is_some_and(|s| s.def.id == crate::open::ID) {
        settings.last = (clock.hours * 100.0).round() / 100.0;
    }
}

#[cfg(feature = "dev")]
mod dev {
    use bevy::prelude::*;
    use engine::devtools::{AddControlCommand, params};
    use serde_json::{Value, json};

    use super::DayClock;

    fn state(c: &DayClock) -> Value {
        let (az, el) = c.sun();
        json!({"hours": c.hours, "rate": c.rate, "running": c.running, "sun": {"azimuth": az, "elevation": el}, "daylight": c.daylight()})
    }

    /// `look.time {time?, speed?, stop?}`: replaces the engine's day-cycle timeline command
    /// (the game never uses `LookTimeline`).
    fn time(world: &mut World, p: Value) -> Result<Value, String> {
        let mut c = world.get_resource_mut::<DayClock>().ok_or("no clock")?;
        if let Some(h) = p.get("time").and_then(Value::as_f64) {
            c.hours = (h as f32).rem_euclid(24.0);
        }
        if let Some(s) = p.get("speed").and_then(Value::as_f64) {
            c.rate = s as f32;
            c.running = s > 0.0;
        }
        if p.get("stop").and_then(Value::as_bool) == Some(true) {
            c.running = false;
        }
        let out = state(&c);
        // A jump: refilter the sky light now rather than within 20 s.
        crate::art::look::refresh_env_soon(world);
        Ok(out)
    }

    /// `look.speed {x}`: game seconds per real second (0 freezes the clock).
    fn speed(world: &mut World, p: Value) -> Result<Value, String> {
        let x = p.get("x").or_else(|| p.get("speed")).and_then(Value::as_f64).ok_or("x: number")? as f32;
        let mut c = world.get_resource_mut::<DayClock>().ok_or("no clock")?;
        c.rate = x;
        c.running = x > 0.0;
        Ok(state(&c))
    }

    pub fn register(app: &mut App) {
        app.add_control_command(
            "look.time",
            "Game clock: jump to `time` (hours, 0..24), set `speed` (game s per real s; 0 freezes), `stop`",
            params(&[("time", "number", "clock hour 0..24"), ("speed", "number", "game seconds per real second (5 = normal)"), ("stop", "boolean", "freeze the clock")]),
            time,
        )
        .add_control_command("look.speed", "Clock speed: game seconds per real second (5 = normal, 0 = frozen)", params(&[("x", "number", "rate")]), speed)
        .add_control_command("day.get", "The game clock, the sun and daylight", params(&[]), |w, _| {
            Ok(w.get_resource::<DayClock>().map(state).unwrap_or(Value::Null))
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sun_follows_the_day() {
        let (az, el) = solar(13.0);
        assert!((az - 180.0).abs() < 0.5, "noon az {az}");
        assert!((el - 63.0).abs() < 0.5, "noon el {el}");
        // Morning in the east, evening in the west; sunrise ≈ 05:37, sunset ≈ 20:23.
        assert!((60.0..120.0).contains(&solar(7.0).0));
        assert!((240.0..300.0).contains(&solar(19.0).0));
        assert!(solar(5.5).1 < 0.0 && solar(5.75).1 > 0.0);
        assert!(solar(20.25).1 > 0.0 && solar(20.5).1 < 0.0);
        // Keyframe elevations of ART_BIBLE §2.3.
        for (h, want) in [(9.0, 33.0), (12.0, 60.0), (16.0, 45.0), (19.0, 13.0)] {
            assert!((solar(h).1 - want).abs() < 2.0, "{h}: {} vs {want}", solar(h).1);
        }
        assert!(solar(23.0).1 < -15.0);
    }

    #[test]
    fn clock_runs_at_five_times_and_wraps() {
        let mut c = DayClock { hours: 23.5, rate: DAY_RATE, running: true };
        // One real hour is five game hours.
        c.advance(3600.0);
        assert!((c.hours - 4.5).abs() < 1e-3, "{}", c.hours);
        let mut f = DayClock { running: false, ..c };
        f.advance(100.0);
        assert_eq!(f.hours, c.hours);
    }

    #[test]
    fn start_times() {
        let s = |start| DaySettings { start, ..default() };
        assert_eq!(start_hours(&DaySettings::default(), 0.0, false), 9.0);
        assert_eq!(start_hours(&s(0), 0.0, false), 6.0);
        // Night (and a night wall clock / saved clock) falls back to Sunset without headlights.
        assert_eq!(start_hours(&s(NIGHT), 0.0, false), NIGHT_FALLBACK);
        assert_eq!(start_hours(&s(NIGHT), 0.0, true), 23.0);
        assert_eq!(start_hours(&s(REAL_CLOCK), 2.0, false), NIGHT_FALLBACK);
        assert_eq!(start_hours(&s(REAL_CLOCK), 14.25, false), 14.25);
        assert_eq!(start_hours(&DaySettings { start: LAST, last: 17.5, ..default() }, 0.0, false), 17.5);
    }

    #[test]
    fn night_row_mapping_round_trips() {
        for ok in [false, true] {
            for i in 0..START_PRESETS.len() as u32 {
                if !ok && i == NIGHT {
                    continue;
                }
                assert_eq!(preset_index(shown_index(i, ok), ok), i);
            }
        }
        assert_eq!(shown_index(REAL_CLOCK, false), 5);
    }

    #[test]
    fn wall_clock_is_an_hour() {
        let h = wall_hours();
        assert!((0.0..24.0).contains(&h));
    }
}
