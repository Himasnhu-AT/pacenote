//! Recorded sound effects (Openverse, CC0 / CC BY, cooked by `tools/audio/cook_sfx.py` into
//! `assets/sounds/sfx/`, credits in `assets/sounds/sfx/index.json` and `assets/credits.txt`).
//!
//! - one-shots: the air horn and crowds at event starts and finishes (louder and longer on a
//!   win), the radar camera's shutter, money, the church bell on the hour, guard-rail scrapes;
//! - beds: looping ambience on the `ambience` bus, crossfaded by where and when the car is:
//!   the town (Fernhollow core), birds (dawn and morning), insects (night), wind in the pines
//!   (high ground).
//!
//! Clips load through the `AssetServer` (decoded on a background thread). The money clip is
//! loaded at startup (it plays in the Dealership); everything else only while driving in the
//! open world, and released when the game is left.
//!
//! Files were checked by ear-proxy (levels, spectrogram, title) before wiring: rejected and not
//! used are `birds_morning_2` (rain), `night_insects_2` (a band playing), `town_ambience_1` (a
//! snowstorm), `radar_shutter_1` (beeps before the click), `radar_shutter_3` (a phone),
//! `guardrail_scrape_3` (a 0.2 s tick), `wind_pines_2` (a step at the loop seam).

use bevy::prelude::*;
use engine::flow::AppState;
use engine::sound::{Attenuation, Rolloff, SoundCtl, SoundCue, SoundCues, Steal, VoiceId};

use crate::open::challenges::RunCue;
use crate::world::StageWorld;

/// (cue, files, bus, dB, spatial max distance (0 = 2D), cooldown s).
type OneShot = (&'static str, &'static [&'static str], &'static str, f32, f32, f32);

/// The one-shots loaded while driving.
const ONE_SHOTS: [OneShot; 11] = [
    ("rec.car_horn", &["car_horn_1", "car_horn_2", "car_horn_3"], "sfx", -6.0, 90.0, 3.0),
    ("rec.dog", &["dog_bark_1", "dog_bark_2"], "sfx", -10.0, 160.0, 6.0),
    ("rec.gravel", &["gravel_hit_1", "gravel_hit_2"], "sfx", -18.0, 0.0, 0.12),
    ("rec.horn", &["air_horn_1", "air_horn_2"], "sfx", -8.0, 0.0, 0.3),
    ("rec.horn.again", &["air_horn_2", "air_horn_1"], "sfx", -6.0, 0.0, 0.3),
    ("rec.crowd.start", &["start_crowd_1", "start_crowd_2"], "sfx", -14.0, 0.0, 2.0),
    ("rec.crowd.finish", &["finish_crowd_3"], "sfx", -15.0, 0.0, 2.0),
    ("rec.crowd.win", &["finish_crowd_1", "finish_crowd_2"], "sfx", -8.0, 0.0, 2.0),
    ("rec.shutter", &["radar_shutter_2"], "sfx", -4.0, 120.0, 0.2),
    ("rec.bell", &["church_bell_1"], "sfx", -2.0, 1400.0, 5.0),
    ("rec.scrape", &["guardrail_scrape_1", "guardrail_scrape_2"], "sfx", -8.0, 60.0, 0.9),
];
const MONEY: OneShot = ("rec.money", &["money_1"], "hud", -14.0, 0.0, 0.25);

/// The beds: (cue, file, dB).
pub const BEDS: [(&str, &str, f32); 5] = [("bed.town", "town_ambience_2", -6.0), ("bed.birds", "birds_morning_1", -6.0), ("bed.insects", "night_insects_1", -9.0), ("bed.wind", "wind_pines_1", -5.0), ("bed.service", "service_park_1", -8.0)];
/// The Service Park hub's bed (main menu), loaded at startup.
const MENU_BED: (&str, &str, f32) = ("bed.menu", "service_park_1", -16.0);
/// The loading screen: soft wind in the pines under the loading song (UX_PASS §2).
const LOADING_BED: (&str, &str, f32) = ("bed.loading", "wind_pines_1", -18.0);
const TOWN: usize = 0;
const BIRDS: usize = 1;
const INSECTS: usize = 2;
const WIND: usize = 3;
const SERVICE: usize = 4;
/// Crossfade time constant of the beds (s).
const BED_TAU: f32 = 1.5;

pub(super) fn register(app: &mut App) {
    app.init_resource::<Beds>()
        .add_systems(Startup, load_money)
        .add_systems(OnEnter(AppState::InGame), load_driving)
        .add_systems(OnExit(AppState::InGame), unload_driving)
        .add_systems(Update, (cues, purchases, beds, bell, scrape, trial_cues, horns, dogs, gravel))
        .add_systems(Update, (loading_bed_on.run_if(resource_added::<crate::loading::Loading>), loading_bed_off.run_if(resource_removed::<crate::loading::Loading>)))
        .add_systems(OnEnter(AppState::MainMenu), menu_bed_on)
        .add_systems(OnExit(AppState::MainMenu), menu_bed_off);
    #[cfg(feature = "dev")]
    {
        use engine::devtools::{AddControlCommand, params};
        app.add_control_command("festival.beds", "Recorded ambience beds: weights 0..1 and whether each is playing", params(&[]), |w, _| {
            let b = w.get_resource::<Beds>().ok_or("no beds")?;
            Ok(serde_json::json!(BEDS.iter().enumerate().map(|(i, (name, file, _))| serde_json::json!({"bed": name, "file": file, "weight": b.weights[i], "playing": b.voices[i].is_some()})).collect::<Vec<_>>()))
        });
        app.add_control_command("festival.voices", "Live voices, compact: cue, state, audible dB", params(&[]), |w, _| {
            let (Some(v), Some(cues)) = (w.get_resource::<engine::sound::Voices>(), w.get_resource::<SoundCues>()) else { return Err("no sound".into()) };
            Ok(serde_json::json!(v.iter().map(|i| format!("{} {} {:.0}dB", i.cue.map_or("-".into(), |c| cues.by_id(c).name.to_string()), i.state.as_str(), engine::sound::lin_to_db(i.audible))).collect::<Vec<_>>()))
        });
    }
}

fn cue(server: &AssetServer, (name, files, bus, db, reach, cooldown): OneShot) -> SoundCue {
    let mut c = SoundCue::new(name).bus(bus).volume(db).cooldown(cooldown).max_instances(1, Steal::Oldest);
    for f in files {
        c = c.clip(server.load(format!("sounds/sfx/{f}.ogg")));
    }
    if reach > 0.0 {
        c = c.spatial(Attenuation { min_distance: (reach * 0.05).max(3.0), max_distance: reach, rolloff: Rolloff::Logarithmic, occlusion: false, doppler: 0.0, ..default() });
    }
    c
}

fn load_money(server: Option<Res<AssetServer>>, cues: Option<ResMut<SoundCues>>) {
    if let (Some(server), Some(mut cues)) = (server, cues) {
        cues.add(cue(&server, MONEY));
        for (name, file, db) in [MENU_BED, LOADING_BED] {
            let mut c = SoundCue::new(name).clip(server.load(format!("sounds/sfx/{file}.ogg"))).bus("ambience").volume(db).max_instances(1, Steal::Oldest);
            c.looping = true;
            c.fade_in = 2.0;
            cues.add(c);
        }
    }
}

/// The Service Park hub: the workshop bed under the menu music.
fn menu_bed_on(ctl: Option<ResMut<SoundCtl>>) {
    if let Some(mut ctl) = ctl {
        ctl.play(MENU_BED.0);
    }
}

fn menu_bed_off(ctl: Option<ResMut<SoundCtl>>) {
    if let Some(mut ctl) = ctl {
        ctl.stop_cue(MENU_BED.0, 1.0);
    }
}

/// The loading screen's bed comes in with the screen and goes with it (the stage's own
/// ambience takes over).
fn loading_bed_on(ctl: Option<ResMut<SoundCtl>>) {
    if let Some(mut ctl) = ctl {
        ctl.play(LOADING_BED.0);
    }
}

fn loading_bed_off(ctl: Option<ResMut<SoundCtl>>) {
    if let Some(mut ctl) = ctl {
        ctl.stop_cue(LOADING_BED.0, 1.5);
    }
}

/// A small deterministic random stream for the "now and then" sounds (no `rand`).
fn chance(seed: &mut u32) -> f32 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 17;
    *seed ^= *seed << 5;
    *seed as f32 / u32::MAX as f32
}

/// Traffic honks: at the car just shaved past (60 % of Close Shaves), and at the player
/// sitting still in front of a stopped traffic car for 2 s (every 8 s at most, the cue's
/// cooldown spaces them).
fn horns(
    time: Res<Time>,
    mut style: MessageReader<crate::career::style::StyleEvent>,
    others: Option<Res<crate::career::racers::OtherCars>>,
    cars: Query<&crate::drive::DrivenCar>,
    ctl: Option<ResMut<SoundCtl>>,
    mut state: Local<(u32, f32)>,
) {
    let (Some(mut ctl), Some(others), Ok(c)) = (ctl, others, cars.single()) else {
        style.clear();
        return;
    };
    if state.0 == 0 {
        state.0 = 0x9e37_79b9;
    }
    let me = c.car.pos;
    let traffic = || others.0.iter().filter(|o| o.kind == crate::career::racers::OtherKind::Traffic);
    for e in style.read() {
        if let crate::career::style::StyleEvent::Tick { kind: crate::career::style::StyleKind::CloseShave, .. } = e
            && chance(&mut state.0) < 0.6
            && let Some(o) = traffic().min_by(|a, b| a.pos.distance(me).total_cmp(&b.pos.distance(me)))
            && o.pos.distance(me) < 60.0
        {
            ctl.play_at("rec.car_horn", o.pos);
        }
    }
    // Blocking: stopped, with a stopped traffic car a few metres behind facing us.
    let blocked = c.car.vel.length() < 1.0
        && traffic().any(|o| {
            let d = me - o.pos;
            o.speed < 1.0 && d.length() < 12.0 && d.normalize_or_zero().dot(o.fwd) > 0.8
        });
    state.1 = if blocked { state.1 + time.delta_secs() } else { 0.0 };
    if state.1 > 2.0 {
        state.1 = -6.0;
        if let Some(o) = traffic().min_by(|a, b| a.pos.distance(me).total_cmp(&b.pos.distance(me))) {
            ctl.play_at("rec.car_horn", o.pos);
        }
    }
}

/// A dog barks now and then at a farm or outskirts house the car passes (within 120 m, outside
/// the town's core): about one chance in 25 s while one is near.
fn dogs(time: Res<Time>, stage: Option<Res<StageWorld>>, cars: Query<&crate::drive::DrivenCar>, ctl: Option<ResMut<SoundCtl>>, mut seed: Local<u32>) {
    let (Some(stage), Some(mut ctl), Ok(c)) = (stage, ctl, cars.single()) else { return };
    let Some(open) = stage.open.as_ref().filter(|_| stage.free_roam()) else { return };
    if *seed == 0 {
        *seed = 0x2545_f491;
    }
    if chance(&mut seed) > time.delta_secs() / 25.0 {
        return;
    }
    let me = c.car.pos.xz();
    let near = open.buildings.buildings.iter().filter(|b| !crate::open::town::in_core(b.centre) && b.centre.distance(me) < 120.0).min_by(|a, b| a.centre.distance(me).total_cmp(&b.centre.distance(me)));
    if let Some(b) = near {
        ctl.play_at("rec.dog", Vec3::new(b.centre.x, open.ground_height(b.centre.x, b.centre.y) + 0.5, b.centre.y));
    }
}

/// Stones off the tyres on loose gravel and washouts: random hits on the underbody, more the
/// faster the car (≈ 1 a second at 60 km/h, 3 at 120), none below 30 km/h.
fn gravel(time: Res<Time>, cars: Query<&crate::drive::DrivenCar>, ctl: Option<ResMut<SoundCtl>>, mut seed: Local<u32>) {
    use crate::stage::Surface;
    let (Some(mut ctl), Ok(c)) = (ctl, cars.single()) else { return };
    let kmh = c.car.speed() * 3.6;
    let loose = c.car.wheels.iter().filter(|w| w.contact && matches!(w.surface, Some(Surface::Gravel | Surface::Loose))).count();
    if kmh < 30.0 || loose < 2 {
        return;
    }
    if *seed == 0 {
        *seed = 0x68e3_1da4;
    }
    let rate = (kmh - 30.0) / 30.0;
    if chance(&mut seed) < rate * time.delta_secs() {
        let v = 0.4 + 0.6 * chance(&mut seed);
        ctl.play_with("rec.gravel", engine::sound::PlayParams { volume: v, pitch: 0.85 + 0.3 * chance(&mut seed), ..default() });
    }
}

fn load_driving(server: Option<Res<AssetServer>>, cues: Option<ResMut<SoundCues>>) {
    let (Some(server), Some(mut cues)) = (server, cues) else { return };
    for s in ONE_SHOTS {
        cues.add(cue(&server, s));
    }
    for (name, file, db) in BEDS {
        let mut c = SoundCue::new(name).clip(server.load(format!("sounds/sfx/{file}.ogg"))).bus("ambience").volume(db).max_instances(1, Steal::Oldest);
        c.looping = true;
        c.fade_in = 1.0;
        cues.add(c);
    }
}

/// Leaving the game: the beds stop and every driving clip is released (the cues keep their
/// names with no clips, so a late `play` is a no-op).
fn unload_driving(cues: Option<ResMut<SoundCues>>, ctl: Option<ResMut<SoundCtl>>, mut beds: ResMut<Beds>) {
    if let Some(mut ctl) = ctl {
        for v in beds.voices.iter().flatten() {
            ctl.stop(*v, 0.5);
        }
    }
    *beds = Beds::default();
    if let Some(mut cues) = cues {
        for s in ONE_SHOTS {
            cues.add(SoundCue::new(s.0).bus(s.2));
        }
        for (name, _, _) in BEDS {
            cues.add(SoundCue::new(name).bus("ambience"));
        }
    }
}

/// Event start and finish: the crowd and the air horn (events only, not challenges; louder
/// and longer on a win or a best).
fn cues(mut run: MessageReader<RunCue>, ctl: Option<ResMut<SoundCtl>>, stage: Option<Res<StageWorld>>) {
    let Some(mut ctl) = ctl else {
        run.clear();
        return;
    };
    let event = stage.is_some_and(|s| !s.free_roam());
    for c in run.read() {
        if !event {
            continue;
        }
        match *c {
            RunCue::Countdown(3) => {
                ctl.play("rec.crowd.start");
            }
            RunCue::Go => {
                ctl.play("rec.horn");
            }
            RunCue::Finish { place, best } => {
                ctl.play("rec.horn");
                if place == 1 || best {
                    ctl.play_with("rec.horn.again", engine::sound::PlayParams { volume: 1.0, ..default() });
                    ctl.play("rec.crowd.win");
                } else {
                    ctl.play("rec.crowd.finish");
                }
            }
            _ => {}
        }
    }
}

/// A Dealership or Garage purchase (the wallet's `spent` grew): the money sound.
fn purchases(wallet: Option<Res<crate::career::wallet::Wallet>>, ctl: Option<ResMut<SoundCtl>>, mut last: Local<Option<u32>>) {
    let Some(w) = wallet else { return };
    if last.is_some_and(|s| w.spent > s)
        && let Some(mut ctl) = ctl
    {
        ctl.play("rec.money");
    }
    *last = Some(w.spent);
}

/// The looping beds and their weights (smoothed).
#[derive(Resource, Default)]
pub struct Beds {
    voices: [Option<VoiceId>; 5],
    /// Current (smoothed) weights, 0..1.
    pub weights: [f32; 5],
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// What each bed should be at: where the car is (`pos`), the hour and daylight (0..1).
pub fn bed_targets(pos: Vec3, hours: f32, daylight: f32, home: Vec2) -> [f32; 5] {
    use crate::open::land::{TOWN_HI, TOWN_LO};
    let p = Vec2::new(pos.x, pos.z);
    // Distance outside the town's core rectangle (0 inside).
    let lo = Vec2::new(TOWN_LO.x, TOWN_LO.y - 20.0);
    let d = (lo - p).max(p - TOWN_HI).max(Vec2::ZERO).length();
    let town = 1.0 - smoothstep(0.0, 220.0, d);
    let night = 1.0 - smoothstep(0.05, 0.3, daylight);
    let wind = smoothstep(190.0, 330.0, pos.y);
    let morning = smoothstep(4.5, 6.0, hours) * (1.0 - smoothstep(10.5, 12.0, hours));
    let mut w = [0.0; 5];
    // The service park (home): the workshop within ~120 m, gone by 300 m.
    w[SERVICE] = 1.0 - smoothstep(120.0, 300.0, p.distance(home));
    w[TOWN] = town * (1.0 - 0.5 * night);
    w[BIRDS] = morning * (1.0 - 0.7 * town) * (1.0 - 0.5 * wind);
    w[INSECTS] = night * (1.0 - 0.6 * town);
    w[WIND] = wind * (1.0 - 0.8 * town);
    w
}

fn beds(
    time: Res<Time>,
    stage: Option<Res<StageWorld>>,
    clock: Option<Res<crate::daytime::DayClock>>,
    cars: Query<&crate::drive::DrivenCar>,
    ctl: Option<ResMut<SoundCtl>>,
    mut beds: ResMut<Beds>,
) {
    let Some(mut ctl) = ctl else { return };
    let open = stage.as_ref().is_some_and(|s| s.in_world());
    let Ok(car) = cars.single() else { return };
    if !open {
        return;
    }
    let (hours, daylight) = clock.map_or((12.0, 1.0), |c| (c.hours, c.daylight()));
    let home = stage.as_ref().and_then(|s| s.open.as_ref()).map_or(Vec2::splat(1e6), |o| o.spawn().0.xz());
    let target = bed_targets(car.car.pos, hours, daylight, home);
    let k = 1.0 - (-time.delta_secs() / BED_TAU).exp();
    for (i, (name, _, _)) in BEDS.iter().enumerate() {
        let w = beds.weights[i] + (target[i] - beds.weights[i]) * k;
        beds.weights[i] = w;
        match beds.voices[i] {
            None if w > 0.01 => {
                // Full gain at the start; the weight is the live volume (`set_volume`
                // multiplies the start volume, so the weight goes in only once).
                let v = ctl.play(*name);
                ctl.set_volume(v, w);
                beds.voices[i] = Some(v);
            }
            None => {}
            Some(v) if w < 0.005 => {
                ctl.stop(v, 0.5);
                beds.voices[i] = None;
            }
            Some(v) => ctl.set_volume(v, w),
        }
    }
}

/// The church bell on the hour, 07:00-21:00, from the town square.
fn bell(stage: Option<Res<StageWorld>>, clock: Option<Res<crate::daytime::DayClock>>, ctl: Option<ResMut<SoundCtl>>, mut last: Local<Option<i32>>) {
    let (Some(stage), Some(clock), Some(mut ctl)) = (stage, clock, ctl) else { return };
    let Some(open) = stage.open.as_ref() else { return };
    let hour = clock.hours.floor() as i32;
    if last.is_some_and(|h| h != hour) && (7..=21).contains(&hour) {
        use crate::open::land::{TOWN_HI, TOWN_LO};
        let c = (TOWN_LO + TOWN_HI) * 0.5;
        ctl.play_at("rec.bell", Vec3::new(c.x, open.ground_height(c.x, c.y) + 18.0, c.y));
    }
    *last = Some(hour);
}

/// The car scraping a guard rail (B's `open::furniture` rails): its side within a hand of the
/// rail line while moving. One scrape per contact (the cue's cooldown spaces repeats).
fn scrape(stage: Option<Res<StageWorld>>, cars: Query<&crate::drive::DrivenCar>, ctl: Option<ResMut<SoundCtl>>, mut was: Local<bool>) {
    let (Some(stage), Some(mut ctl)) = (stage, ctl) else { return };
    let Some(open) = stage.open.as_ref() else { return };
    let Ok(c) = cars.single() else { return };
    let car = &c.car;
    let speed = car.vel.length();
    let p = Vec2::new(car.pos.x, car.pos.z);
    let touching = speed > 2.0 && rail_contact(&open.furniture.rails, p).is_some();
    if touching && !*was
        && let Some(at) = rail_contact(&open.furniture.rails, p)
    {
        ctl.play_with("rec.scrape", engine::sound::PlayParams { at: Some(Vec3::new(at.x, car.pos.y, at.y)), volume: (speed / 15.0).clamp(0.3, 1.0), ..default() });
    }
    *was = touching;
}

/// Half the car's width plus the rail's radius and a little: a side this close is touching.
const RAIL_TOUCH: f32 = 0.9 + 0.12 + 0.2;

/// The nearest point of a rail within touching distance of a car centred at `p`.
pub fn rail_contact(rails: &[crate::open::furniture::RailRun], p: Vec2) -> Option<Vec2> {
    for r in rails {
        for w in r.pts.windows(2) {
            let (a, b) = (Vec2::new(w[0].x, w[0].z), Vec2::new(w[1].x, w[1].z));
            if a.distance_squared(p) > 100.0 {
                continue;
            }
            let ab = b - a;
            let t = ((p - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
            let q = a + ab * t;
            if q.distance(p) <= RAIL_TOUCH {
                return Some(q);
            }
        }
    }
    None
}

/// Countdown, GO and finish cues for runs without rivals (time trials and the classic stages;
/// `career::race_flow` sends them for races).
fn trial_cues(run: Option<Res<crate::drive::StageRun>>, stage: Option<Res<StageWorld>>, race: Option<Res<crate::career::race_flow::RaceRun>>, mut out: MessageWriter<RunCue>, mut last: Local<(i32, bool)>) {
    let (Some(run), Some(stage)) = (run, stage) else { return };
    if race.is_some() || stage.free_roam() {
        *last = (0, false);
        return;
    }
    let count = run.countdown.max(0.0).ceil() as i32;
    let (prev, done) = *last;
    if count > 0 && count != prev && count <= 3 {
        out.write(RunCue::Countdown(count as u8));
    }
    if count == 0 && prev > 0 {
        out.write(RunCue::Go);
    }
    let finished = run.finished.is_some();
    if finished && !done {
        out.write(RunCue::Finish { place: 0, best: false });
    }
    *last = (count, finished);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beds_follow_place_and_time() {
        let town = Vec3::new(-300.0, 96.0, -1000.0);
        let pass = Vec3::new(-981.0, 420.0, 1560.0);
        let valley = Vec3::new(900.0, 100.0, -200.0);
        let home = Vec2::new(5000.0, 5000.0);
        let t = bed_targets(town, 14.0, 1.0, home);
        assert!(t[TOWN] > 0.99 && t[BIRDS] < 0.01 && t[INSECTS] < 0.01 && t[WIND] < 0.01, "{t:?}");
        let t = bed_targets(pass, 7.0, 0.8, home);
        assert!(t[WIND] > 0.99 && t[BIRDS] > 0.4 && t[TOWN] < 0.01, "{t:?}");
        let t = bed_targets(valley, 23.0, 0.0, home);
        assert!(t[INSECTS] > 0.99 && t[BIRDS] < 0.01, "{t:?}");
        let t = bed_targets(valley, 15.0, 1.0, home);
        assert!(t.iter().all(|&w| w < 0.01), "midday valley: the synth ambience alone {t:?}");
        let t = bed_targets(valley, 15.0, 1.0, Vec2::new(950.0, -200.0));
        assert!(t[SERVICE] > 0.99, "at the service park {t:?}");
    }

    #[test]
    fn wired_files_exist() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/sounds/sfx");
        let files = ONE_SHOTS.iter().chain([&MONEY]).flat_map(|s| s.1.iter().copied()).chain(BEDS.iter().map(|b| b.1)).chain([MENU_BED.1, LOADING_BED.1]);
        for f in files {
            assert!(dir.join(format!("{f}.ogg")).exists(), "{f}");
        }
    }

    #[test]
    fn a_rail_is_touched_at_the_car_side() {
        use crate::open::furniture::{RailKind, RailRun};
        let rail = RailRun { road: 0, side: 1, from: 0.0, to: 20.0, kind: RailKind::Steel, pts: (0..11).map(|k| Vec3::new(2.0, 0.0, k as f32 * 2.0)).collect() };
        assert!(rail_contact(std::slice::from_ref(&rail), Vec2::new(1.0, 10.0)).is_some());
        assert!(rail_contact(&[rail], Vec2::new(0.0, 10.0)).is_none());
    }
}
