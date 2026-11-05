//! The loading screen: hides everything a stage needs before the countdown can start.
//!
//! Before this existed, entering a stage did all of it inside the first frames of play: the
//! stage was generated on the main thread, every material's GPU pipeline compiled on first draw
//! (synchronously on macOS: stalls of 0.5–5 s), and the scenery and trees streamed in over the
//! next ~30 s. The window stopped responding for up to 13 s while the clock was about to run.
//!
//! Now, on entering a stage:
//! 1. **Generate**: the stage (road, heights, corners, collision) is built on a worker thread
//!    while the loading screen draws.
//! 2. **Build**: terrain, road, dressing volume, sky, camera and car are spawned (one long frame,
//!    behind the screen).
//! 3. **Warm**: every material is drawn once ([`WarmPipelines`]) so its pipeline compiles now;
//!    textures and the car model finish loading; the dressing cells all evaluate (with more
//!    workers than in play) and the near trees spawn.
//! 4. **Settle**: a run of normal-length frames, so the first second of driving is smooth.
//!
//! The stage clock's countdown only starts once the screen is gone. Headless runs (tests,
//! servers) skip all of it: the stage is generated synchronously and nothing is drawn.

use std::time::Instant;

use bevy::asset::LoadState;
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, futures_lite::future};
use engine::flow::{AppState, PauseAllowed};
use engine::pcg::{PcgSettings, PcgStats};
use engine::render_budget::WarmPipelines;

use crate::stage::StageDef;
use crate::world::StageWorld;

/// Present while a stage is loading (removed when the countdown may start).
#[derive(Resource)]
pub struct Loading {
    phase: Phase,
    started: Instant,
    phase_at: Instant,
    def: StageDef,
    task: Option<Task<StageWorld>>,
    /// Frames in a row under the settle threshold.
    calm: u32,
    /// PCG limits to restore after loading.
    pcg: Option<PcgSettings>,
    /// Shown progress (0..1), only ever rises.
    shown: f32,
    /// Dressing cells within [`NEAR_SCENERY`] of the start: the ones to wait for.
    near_cells: u32,
    /// The stage's contour map for the screen, drawn on a worker (none for stage 1: the
    /// catalog already has it).
    map: Option<Task<crate::meta::stages::Raster>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Generate,
    Build,
    Warm,
    Settle,
}

impl Phase {
    /// 1-based step of the four, for "2/4".
    fn step(self) -> u32 {
        match self {
            Phase::Generate => 1,
            Phase::Build => 2,
            Phase::Warm => 3,
            Phase::Settle => 4,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Phase::Generate => "Surveying the roads",
            Phase::Build => "Building terrain and road",
            Phase::Warm => "Preparing scenery and shaders",
            Phase::Settle => "Final checks",
        }
    }
}

/// The overlay root.
#[derive(Component)]
struct LoadingRoot;

#[derive(Component)]
struct LoadingLabel;

#[derive(Component)]
struct LoadingBar;

/// The contour map picture (its image is swapped in when the worker finishes).
#[derive(Component)]
struct LoadingMap {
    /// Real time the picture last changed (it fades in from there).
    since: f32,
}

/// Size of the loading screen's contour map (px). The menu's is 1100; this one is drawn
/// while the stage loads, so a little smaller.
const MAP_PX: u32 = 900;

/// Frames under this long count as settled.
const CALM_MS: f32 = 40.0;
/// Settled frames needed in a row.
const CALM_FRAMES: u32 = 30;
/// Longest the settle phase waits for them (s).
const SETTLE_MAX_S: f32 = 2.5;
/// Give up waiting (and let the player drive) after this long.
const TIMEOUT_S: f32 = 60.0;
/// Scenery within this of the start line is in before the screen goes (m). The rest of the
/// stage (far tree cards, the far side of the map) streams in during play, nearest first.
const NEAR_SCENERY: f32 = 450.0;
/// Dressing cell size (m), as `art::spawn_stage` sets the volume up.
const PCG_CELL: f32 = 64.0;

/// Tips for the classic stages: (keyboard, gamepad). Keys are named for the device used last.
const TIPS: &[(&str, &str)] = &[
    ("Your co-driver reads the road ahead: listen for the corner grade (1 hairpin to 6 flat).", ""),
    ("Big hits damage the car. A damaged engine loses power; bent steering pulls to one side.", ""),
    ("Marker posts, signs and saplings snap if you clip them, and shrubs bend: none of them hurt the car.", ""),
    ("Press C to change camera: cockpit, bonnet or chase.", "Select changes camera: cockpit, bonnet or chase."),
    ("Stuck or upside down? R puts you back on the road for a 5 s penalty.", "Stuck or upside down? Y puts you back on the road for a 5 s penalty."),
    ("The handbrake (Space) helps the car rotate into hairpins.", "The handbrake (A) helps the car rotate into hairpins."),
];

/// Tips for the open world (no co-driver in free roam, no penalty for a recover there).
const OPEN_TIPS: &[(&str, &str)] = &[
    ("Events wait around the valley: stop inside a glowing column to start one, or pick it under Events and choose Go now.", ""),
    ("Challenges start as you drive through their first gate: Radar Posts, Pace and Slide Sections, Yump Boards.", ""),
    ("Link slides, air and close shaves into a Rhythm. The longer it runs, the more Style and Notes it banks.", ""),
    ("A hard hit breaks a Rhythm: bank it by easing off for a moment before the risky bit.", ""),
    ("M opens the map: pick an event and follow the route line there.", "D-pad up opens the map: pick an event and follow the route line there."),
    ("Stuck or on the roof? R puts you back on the road: free in free roam, +5 s in an event.", "Stuck or on the roof? Y puts you back on the road: free in free roam, +5 s in an event."),
    ("Press C to change camera: cockpit, bonnet or chase.", "Select changes camera: cockpit, bonnet or chase."),
    ("The handbrake (Space) helps the car rotate into hairpins.", "The handbrake (A) helps the car rotate into hairpins."),
    ("Street parts are cheap; a full Works build moves a car up a class. Upgrades are in the Service Park's Garage.", ""),
];

/// A new tip every this many seconds (UX_PASS §2), crossfading over [`TIP_FADE`].
const TIP_EVERY: f32 = 7.0;
const TIP_FADE: f32 = 0.3;
/// After this long the phase line says it's taking longer than usual.
const SLOW_S: f32 = 20.0;

/// The tips in a shuffled order and the one showing.
#[derive(Resource)]
struct TipCycle {
    tips: Vec<String>,
    at: usize,
    /// Real time the current tip came in.
    since: f32,
    /// A pending step (−1 / +1) halfway through its crossfade.
    step: Option<(i32, f32)>,
}

#[derive(Component)]
struct TipText;
#[derive(Component)]
struct TipCount;
#[derive(Component)]
struct PhaseStep;

/// The region map on the open world's screen: built once the painted map and the stage
/// are in; then the route (or the car's spot) is drawn on it.
#[derive(Component)]
struct RegionMap {
    built: Option<f32>,
}

/// A dot of the route line, shown in turn from start to finish.
#[derive(Component)]
struct RouteDot(f32);

/// The briefing's "Events finished" value (filled in once the world's events are known).
#[derive(Component)]
struct EventsLine;

/// Events finished and in the world: from the loaded world's events when they exist, else the
/// list remembered from the last drive. Finished = a time on the board or a race finish.
pub fn events_progress(world: &World) -> Option<(usize, usize)> {
    let ids: Vec<u32> = match world.get_resource::<crate::open::events::Events>() {
        Some(ev) if !ev.0.is_empty() => ev.0.iter().map(|e| e.id).collect(),
        _ => world.resource::<crate::meta::Profile>().events.iter().map(|e| e.id).collect(),
    };
    if ids.is_empty() {
        return None;
    }
    let p = world.resource::<crate::meta::Profile>();
    let me = crate::meta::board::team_key(&p.driver_name, "");
    let b = &world.resource::<crate::meta::Boards>().0;
    let races = world.get_resource::<crate::career::save::CareerProgress>().map(|c| c.races.clone()).unwrap_or_default();
    let done = ids.iter().filter(|id| b.records.iter().any(|r| r.stage == **id && r.team() == me) || races.get(id).is_some_and(|r| r.rookie + r.pro + r.ace + r.legend > 0)).count();
    Some((done, ids.len()))
}

/// "Morning" for 9.0 (the open world's start presets' words, by hour).
pub fn hour_name(h: f32) -> &'static str {
    match h.rem_euclid(24.0) {
        h if h < 5.0 => "Night",
        h if h < 8.0 => "Dawn",
        h if h < 11.5 => "Morning",
        h if h < 14.0 => "Noon",
        h if h < 18.5 => "Afternoon",
        h if h < 21.0 => "Sunset",
        _ => "Night",
    }
}

/// "09:00".
pub fn clock_text(h: f32) -> String {
    let m = (h.rem_euclid(24.0) * 60.0).round() as u32 % (24 * 60);
    format!("{:02}:{:02}", m / 60, m % 60)
}

/// Fades the screen into the stage after loading.
#[derive(Component)]
struct LoadingFade(f32);

/// Fades the screen in over the menu.
#[derive(Component)]
struct LoadingIntro(f32);

pub struct LoadingPlugin;

impl Plugin for LoadingPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (advance, draw, tips, region_map).chain().run_if(resource_exists::<Loading>))
            .add_systems(Update, (fade_in, fade_out))
            .add_systems(OnExit(AppState::InGame), abandon);
    }
}

/// Is a stage still loading? (Gameplay holds the car and the clock while it is.)
pub fn is_loading(world: &World) -> bool {
    world.contains_resource::<Loading>()
}

/// Starts loading `def`. Headless worlds build it right away (no screen); visual ones show the
/// loading screen and build it over the next frames.
pub fn begin(world: &mut World, def: StageDef) {
    if !crate::art::visual(world) {
        let s = crate::world::generate(&def);
        crate::world::spawn(world, &s);
        // MSAA off from the start: the engine turns it off a frame later, and a render-scaled
        // frame drawn in between fails GPU validation (MSAA depth, single-sample colour).
        world.spawn((engine::gfx::MainCamera, Camera3d::default(), Msaa::Off, Transform::from_xyz(0.0, 50.0, 0.0)));
        crate::drive::spawn_car(world);
        return;
    }
    let task = {
        let def = def.clone();
        AsyncComputeTaskPool::get().spawn(async move {
            let t0 = Instant::now();
            let s = crate::world::generate(&def);
            info!("stage {} \"{}\": {:.1} km, {} corners, generated in {:.2} s", def.id, def.name, s.road.length() / 1000.0, s.corners.len(), t0.elapsed().as_secs_f32());
            s
        })
    };
    // The picture for the screen, alongside the generation (stage 1's is the menu backdrop).
    // The open world has no stage map (and no single road to draw one from).
    let have_map = def.id == crate::open::ID || (def.id == 1 && world.resource::<crate::meta::stages::StageCatalog>().map.is_some());
    let map = (!have_map).then(|| {
        let def = def.clone();
        let a = world.resource::<engine::ui::theme::Theme>().accent.to_srgba();
        AsyncComputeTaskPool::get().spawn(async move { crate::meta::stages::contour_map(&def, &crate::stage::generate_road(&def), MAP_PX, [a.red, a.green, a.blue]) })
    });
    let now = Instant::now();
    world.insert_resource(Loading { phase: Phase::Generate, started: now, phase_at: now, def: def.clone(), task: Some(task), calm: 0, pcg: None, shown: 0.0, near_cells: 0, map });
    world.insert_resource(PauseAllowed(false));
    // Long frames are the point of this screen: no hitch / hang reports while it's up.
    #[cfg(feature = "telemetry")]
    world.insert_resource(engine::trace::TraceQuiet);
    spawn_screen(world, &def);
}

/// The screen: an event briefing. Left: kicker (kind and conditions), the name, the facts,
/// a briefing card (your best and the record for a stage or event; grade, Notes and the car
/// for free roam), the real loading phase as "n/4" and the bar; a rotating tip bottom left.
/// Right: the stage's contour map, or for the open world the painted region with the route
/// drawn start to finish. Everything here is already known (catalog, board, profile,
/// career); only the maps are drawn on workers.
fn spawn_screen(world: &mut World, def: &StageDef) {
    use crate::meta::hub::{self, Glyphs, glyph, notes};
    use crate::meta::screens::{biome_name, kind_color, kind_glyph, stat, surface_name, time_name, weather_name};
    use crate::meta::{Boards, PendingStart, Profile, RunMode, board};
    use engine::ui::prelude::*;

    let t = world.resource::<Theme>().clone();
    let gl = world.get_resource::<Glyphs>().cloned().unwrap_or_default();
    let open = def.id == crate::open::ID;
    let device = world.get_resource::<engine::actions::ActionState>().map_or(engine::actions::Device::KeyboardMouse, |a| a.last_device);
    let pad = device == engine::actions::Device::Gamepad;
    // Tips: the mode's pool, shuffled (seeded by the clock), keys named for the device.
    let mut pool: Vec<String> = (if open { OPEN_TIPS } else { TIPS }).iter().map(|(k, g)| if pad && !g.is_empty() { g.to_string() } else { k.to_string() }).collect();
    let mut seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(7, |d| d.as_nanos() as u64) | 1;
    for i in (1..pool.len()).rev() {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        pool.swap(i, (seed % (i as u64 + 1)) as usize);
    }
    let first_tip = pool.first().cloned().unwrap_or_default();
    let n_tips = pool.len();
    let now = world.resource::<Time<Real>>().elapsed_secs();
    world.insert_resource(TipCycle { tips: pool, at: 0, since: now, step: None });

    let cat = world.resource::<crate::meta::stages::StageCatalog>();
    let info = cat.info(def.id).cloned();
    let backdrop = cat.backdrop.clone();
    // Until the worker's map is in: stage 1's backdrop, else the catalog's road sketch.
    let (picture, is_map) = match (def.id == 1).then(|| cat.map.clone()).flatten() {
        Some(m) => (Some(m), true),
        None => (if open { None } else { cat.preview(def.id) }, false),
    };
    let mode = world.get_resource::<RunMode>().copied().unwrap_or_default();
    let p = world.resource::<Profile>().clone();
    let b = &world.resource::<Boards>().0;
    let me = board::team_key(&p.driver_name, "");
    let team = board::team_key(&p.driver_name, if mode == RunMode::Team { &p.guide_name } else { "" });
    // The event "Go now" loads at, if any.
    let pending = world.get_resource::<PendingStart>().copied();
    let event = match pending {
        Some(PendingStart::Event(id)) => p.events.iter().find(|e| e.id == id).cloned(),
        _ => None,
    };
    let route_to = match pending {
        Some(PendingStart::Route(id)) => p.events.iter().find(|e| e.id == id).cloned(),
        _ => None,
    };
    let pick = world.get_resource::<crate::meta::screens::EventPick>().copied().unwrap_or_default();
    // This weather and time of day's board (crate::meta::stages::board_hash).
    let (best, leader) = match (&event, &info) {
        (Some(e), _) => {
            let mut v: Vec<&board::Record> = b.records.iter().filter(|r| r.stage == e.id).collect();
            v.sort_by(|a, b| a.total().total_cmp(&b.total()));
            (b.records.iter().filter(|r| r.stage == e.id && r.team() == me).min_by(|a, b| a.total().total_cmp(&b.total())).cloned(), v.first().map(|r| (*r).clone()))
        }
        (None, Some(i)) => {
            let h = crate::meta::stages::board_hash(&i.hash, def, crate::meta::picked_assists(world));
            (b.best_of(def.id, &h, &team).cloned(), b.leader(def.id, &h).cloned())
        }
        _ => (None, None),
    };
    let surf = match &info {
        Some(i) => i
            .surfaces
            .iter()
            .filter(|(s, f)| *f >= 0.02 && !matches!(s, crate::stage::Surface::Verge | crate::stage::Surface::Rough))
            .map(|(s, f)| format!("{} {:.0}%", surface_name(*s), f * 100.0))
            .collect::<Vec<_>>()
            .join(", "),
        None => surface_name(def.surface).to_string(),
    };
    // The open world's clock starts where Settings › World says (daytime::start_hours); the
    // stages keep their own time of day.
    let conditions = if open {
        let h = crate::daytime::start_hours(&world.get_resource::<crate::daytime::DaySettings>().cloned().unwrap_or_default(), crate::daytime::wall_hours(), crate::daytime::night_available(world));
        format!("{} {}, {}", hour_name(h), clock_text(h), weather_name(def.weather))
    } else {
        format!("{}, {}", time_name(def.time), weather_name(def.weather))
    };
    let (kicker, name, facts) = match (&event, open) {
        (Some(e), _) => (format!("{} \u{b7} {}", e.kind, conditions), e.name.clone(), format!("{} \u{b7} {:.1} km of closed road \u{b7} start line, then the countdown", e.kind, e.km)),
        (None, true) => (
            format!("Free roam \u{b7} {conditions}"),
            hub::TOWN.to_string(),
            match (&route_to, p.last_pose.is_some()) {
                (Some(e), _) => format!("The valley, its town, farms and forest roads \u{b7} route set to {}", e.name),
                (None, true) => "The valley, its town, farms and forest roads \u{b7} back where you parked".into(),
                (None, false) => "The valley, its town, farms and forest roads \u{b7} starting at the service park".into(),
            },
        ),
        (None, false) => {
            let mut k = format!("Stage {:02} \u{b7} {}", def.id, if mode == RunMode::Team { "Team run" } else { "Practice" });
            if crate::meta::stages::conditions_key(def).is_some() {
                k.push_str(&format!(" \u{b7} {conditions}"));
            }
            (k, def.name.clone(), format!("{} \u{b7} {} \u{b7} {conditions}", biome_name(def.biome), surf))
        }
    };
    let events_line = events_progress(world).map_or_else(|| "\u{2014}".to_string(), |(d, n)| format!("{d} of {n}"));
    let chip = hub::car_chip(world.resource::<crate::career::garage::Garage>());
    let balance = world.resource::<crate::career::wallet::Wallet>().balance;
    let (grade, _, _) = hub::grade_span(world.resource::<crate::career::grade::Grade>().xp);
    let name_size = if name.chars().count() > 12 { 96.0 } else { 120.0 };
    let accent = event.as_ref().map_or(t.accent, |e| kind_color(&e.kind));
    world
        .commands()
        .spawn((
            LoadingRoot,
            Name::new("Loading screen"),
            DespawnOnExit(AppState::InGame),
            GlobalZIndex(1000),
            fullscreen(),
            BackgroundColor(t.bg),
        ))
        .with_children(|root| {
            // Right: the map, bleeding off the edges like the menu backdrop (classic stages),
            // or the region in a road-book frame (open world, filled in when it's painted).
            if open {
                root.spawn((Node { position_type: PositionType::Absolute, right: px(EDGE), top: px(0), bottom: px(0), justify_content: JustifyContent::Center, flex_direction: FlexDirection::Column, ..default() }, Pickable::IGNORE)).with_children(|r| {
                    r.spawn((
                        Node { width: px(760), height: px(760), border: UiRect::all(px(1)), overflow: Overflow::clip(), ..default() },
                        BorderColor::all(t.line_strong),
                        BackgroundColor(t.panel),
                        Pickable::IGNORE,
                    ))
                    .with_children(|m| {
                        // Until the region is painted: the contour backdrop, faint, and a note.
                        if let Some(img) = backdrop.clone() {
                            m.spawn((ImageNode { image: img, color: Color::srgba(1.0, 1.0, 1.0, 0.35), ..default() }, Node { position_type: PositionType::Absolute, left: px(0), top: px(0), width: percent(100), height: percent(100), ..default() }, Pickable::IGNORE));
                        }
                        m.spawn((Node { position_type: PositionType::Absolute, left: px(0), right: px(0), bottom: px(GAP_M), justify_content: JustifyContent::Center, ..default() }, Pickable::IGNORE)).with_children(|n| {
                            n.spawn(text("SURVEYING THE VALLEY", label(SIZE_LABEL), t.text_dim));
                        });
                        m.spawn((RegionMap { built: None }, ImageNode { color: Color::NONE, ..default() }, Node { position_type: PositionType::Absolute, left: px(0), top: px(0), width: percent(100), height: percent(100), ..default() }, Pickable::IGNORE));
                    });
                    r.spawn((Node { justify_content: JustifyContent::SpaceBetween, width: px(760), margin: UiRect::top(px(GAP_S)), ..default() }, Pickable::IGNORE)).with_children(|cap| {
                        cap.spawn(text("THE VALLEY \u{b7} 4 \u{d7} 4 KM", label(18.0), t.text_dim));
                        cap.spawn(text(if event.is_some() { "ROUTE, START TO FINISH" } else { "YOU ARE HERE" }, label(18.0), t.text_dim));
                    });
                });
            } else {
                let mut pic = root.spawn((
                    LoadingMap { since: now },
                    ImageNode { color: Color::NONE, ..default() },
                    Node { position_type: PositionType::Absolute, right: px(-140), top: percent(-14), height: percent(128), aspect_ratio: Some(1.0), ..default() },
                    Pickable::IGNORE,
                ));
                if let Some(h) = picture {
                    pic.insert(ImageNode { image: h, color: Color::NONE, ..default() });
                    if !is_map {
                        // The small road sketch: centred and smaller than the full map.
                        pic.insert(Node { position_type: PositionType::Absolute, right: percent(8), top: percent(18), height: percent(64), aspect_ratio: Some(1.0), ..default() });
                    }
                }
            }
            menu_backdrop(root, &t, 0.0, if open { 0.6 } else { 1.0 });
            root.spawn(menu_column(GAP_S)).insert(Node { position_type: PositionType::Absolute, left: px(EDGE), top: px(0), bottom: px(0), width: px(820), flex_direction: FlexDirection::Column, justify_content: JustifyContent::Center, row_gap: px(GAP_S), ..default() }).with_children(|c| {
                c.spawn((Node { column_gap: px(GAP_S + 4.0), align_items: AlignItems::Center, ..default() }, Pickable::IGNORE)).with_children(|k| {
                    match &event {
                        Some(e) => glyph(k, &gl, kind_glyph(&e.kind), 18.0, accent),
                        None => {
                            k.spawn((Node { width: px(36), height: px(3), ..default() }, BackgroundColor(t.accent), Pickable::IGNORE));
                        }
                    }
                    k.spawn(eyebrow(&kicker, accent));
                });
                c.spawn(title(&name, name_size, t.text));
                c.spawn((text(facts, body(SIZE_BODY + 2.0), t.text_muted), Node { max_width: px(780), ..default() }));
                c.spawn(spacer(GAP_M));
                // The briefing card.
                c.spawn(accent_panel(&t, accent, 22.0, GAP_S)).insert(Node { width: px(620), flex_direction: FlexDirection::Column, row_gap: px(GAP_XS + 3.0), padding: UiRect::all(px(22)), border: UiRect::left(px(3)), border_radius: BorderRadius::all(px(RADIUS)), ..default() }).with_children(|card| {
                    card.spawn((Node { justify_content: JustifyContent::SpaceBetween, ..default() }, Pickable::IGNORE)).with_children(|h| {
                        h.spawn(eyebrow("Briefing", t.text_dim));
                        h.spawn(text(format!("{} \u{b7} {} {}", chip.name.to_uppercase(), chip.class.letter(), chip.pi), label(SIZE_LABEL), t.text_dim));
                    });
                    let line = |card: &mut ChildSpawnerCommands, what: &str, value: String, color: Color| {
                        card.spawn((Node { justify_content: JustifyContent::SpaceBetween, column_gap: px(GAP_M), ..default() }, Pickable::IGNORE)).with_children(|row| {
                            row.spawn(text(what, body(SIZE_BODY), t.text_muted));
                            row.spawn(text(value, body_bold(SIZE_BODY), color));
                        });
                    };
                    let rec = |r: &Option<board::Record>| r.as_ref().map_or("No time yet".to_string(), |r| format!("{}   {}", board::clock(r.total()), r.team_label()));
                    if open && event.is_none() {
                        line(card, "Driver grade", format!("Grade {grade}"), t.text);
                        line(card, "Notes", notes(balance), t.text);
                        card.spawn((Node { justify_content: JustifyContent::SpaceBetween, column_gap: px(GAP_M), ..default() }, Pickable::IGNORE)).with_children(|row| {
                            row.spawn(text("Events finished", body(SIZE_BODY), t.text_muted));
                            row.spawn((EventsLine, text(events_line.clone(), body_bold(SIZE_BODY), t.text)));
                        });
                        if let Some(e) = &route_to {
                            line(card, "Route set", format!("{} \u{b7} {:.1} km", e.name, e.km), t.accent);
                        }
                    } else {
                        line(card, if mode == RunMode::Team { "Team best" } else { "Your best" }, rec(&best), if best.is_some() { t.text } else { t.text_dim });
                        line(card, if event.is_some() { "Event record" } else { "Stage record" }, rec(&leader), if leader.is_some() { t.accent } else { t.text_dim });
                        if event.is_some() {
                            let mode = match pick.mode {
                                crate::career::racers::EventMode::TimeTrial => "Time trial \u{b7} solo".to_string(),
                                crate::career::racers::EventMode::Race(d) => format!("Race \u{b7} 5 rivals, {d:?}"),
                            };
                            line(card, "Mode", mode, t.text);
                        } else if let Some(i) = &info {
                            card.spawn((Node { column_gap: px(GAP_L), margin: UiRect::top(px(GAP_XS)), ..default() }, Pickable::IGNORE)).with_children(|r| {
                                stat(r, &t, format!("{:.2}", i.length / 1000.0), "km", t.text, 34.0);
                                stat(r, &t, i.corners.to_string(), "Corners", t.text, 34.0);
                                stat(r, &t, i.tight.to_string(), "Hairpins & 2s", t.text, 34.0);
                                if i.crests > 0 {
                                    stat(r, &t, i.crests.to_string(), "Crests", t.text, 34.0);
                                }
                                stat(r, &t, format!("{:.0}", i.climb), "m climb", t.text, 34.0);
                            });
                        }
                    }
                });
                c.spawn(spacer(GAP_L));
                // Progress: the real phase, n/4, and a bar that only rises.
                c.spawn((Node { column_gap: px(GAP_S), align_items: AlignItems::Baseline, width: px(620), justify_content: JustifyContent::SpaceBetween, ..default() }, Pickable::IGNORE)).with_children(|r| {
                    r.spawn((LoadingLabel, text(Phase::Generate.label().to_uppercase(), label(SIZE_LABEL + 2.0), t.text)));
                    r.spawn((PhaseStep, text("1 / 4", label(SIZE_LABEL + 2.0), t.text_muted)));
                });
                c.spawn((Node { width: px(620), height: px(4), margin: UiRect::top(px(GAP_XS)), ..default() }, BackgroundColor(t.line), Pickable::IGNORE)).with_children(|b| {
                    b.spawn((LoadingBar, Node { width: percent(0), height: percent(100), ..default() }, BackgroundColor(t.accent), Pickable::IGNORE));
                });
            });
            // Tip, bottom left where the menus keep their key hints; Left / Right steps.
            root.spawn((Node { position_type: PositionType::Absolute, left: px(EDGE), bottom: px(FOOTER_BOTTOM), width: px(820), ..default() }, Pickable::IGNORE)).with_children(|w| {
                w.spawn(accent_panel(&t, t.line_strong, 18.0, GAP_XS + 2.0)).insert(Node { width: px(820), flex_direction: FlexDirection::Column, row_gap: px(GAP_XS + 2.0), padding: UiRect::all(px(18)), border: UiRect::left(px(3)), border_radius: BorderRadius::all(px(RADIUS)), ..default() }).with_children(|card| {
                    card.spawn((Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, ..default() }, Pickable::IGNORE)).with_children(|h| {
                        h.spawn((Node { column_gap: px(GAP_S), align_items: AlignItems::Center, ..default() }, Pickable::IGNORE)).with_children(|l| {
                            l.spawn(eyebrow("Pace note", t.accent));
                            l.spawn((TipCount, text(format!("1 / {n_tips}"), label(SIZE_LABEL), t.text_dim)));
                        });
                        h.spawn((Node { column_gap: px(GAP_S), align_items: AlignItems::Center, ..default() }, Pickable::IGNORE)).with_children(|k| {
                            k.spawn(key_cap(&t, if pad { "LB" } else { "Left" }));
                            k.spawn(key_cap(&t, if pad { "RB" } else { "Right" }));
                            k.spawn(text("More tips", body(18.0), t.text_dim));
                        });
                    });
                    card.spawn((TipText, text(first_tip, body(SIZE_BODY + 1.0), t.text), Node { min_height: px(52), ..default() }));
                });
            });
            // Fades in over the menu (200 ms).
            root.spawn((LoadingIntro(now), fullscreen(), BackgroundColor(t.bg), Pickable::IGNORE));
        });
    world.flush();
}

/// The worker's contour map is in: show it in place of the sketch.
fn take_map(world: &mut World) {
    let done = {
        let Some(mut l) = world.get_resource_mut::<Loading>() else { return };
        let Some(task) = l.map.as_mut() else { return };
        let r = block_on(future::poll_once(task));
        if r.is_some() {
            l.map = None;
        }
        r
    };
    let Some(raster) = done else { return };
    let Some(mut images) = world.get_resource_mut::<Assets<Image>>() else { return };
    let h = images.add(raster.image());
    let now = world.resource::<Time<Real>>().elapsed_secs();
    let mut q = world.query::<(&mut ImageNode, &mut Node, &mut LoadingMap)>();
    for (mut img, mut node, mut m) in q.iter_mut(world) {
        img.image = h.clone();
        img.color = Color::NONE;
        *node = Node { position_type: PositionType::Absolute, right: px(-140), top: percent(-14), height: percent(128), aspect_ratio: Some(1.0), ..default() };
        m.since = now;
    }
}

/// Steps the load along. Exclusive: building the stage needs the whole world.
fn advance(world: &mut World) {
    let phase = world.resource::<Loading>().phase;
    let elapsed = world.resource::<Loading>().started.elapsed().as_secs_f32();
    match phase {
        Phase::Generate => {
            let done = {
                let mut l = world.resource_mut::<Loading>();
                l.task.as_mut().and_then(|t| block_on(future::poll_once(t)))
            };
            if let Some(s) = done {
                world.resource_mut::<Loading>().task = None;
                world.insert_resource(Pending(s));
                set_phase(world, Phase::Build);
            }
        }
        Phase::Build => {
            // One frame after Generate so the label shows before the long frame.
            let Some(Pending(s)) = world.remove_resource::<Pending>() else { return };
            let t0 = Instant::now();
            let near = near_cells(&s);
            if world.get_resource::<crate::net_session::Role>() == Some(&crate::net_session::Role::Guide) {
                // The guide rides along: the scenery and a drawn car, never the simulation.
                crate::guide::seat::build_scene(world, &s);
            } else {
                crate::world::spawn(world, &s);
                // MSAA off from the start: the engine turns it off a frame later, and a render-scaled
                // frame drawn in between fails GPU validation (MSAA depth, single-sample colour).
                world.spawn((engine::gfx::MainCamera, Camera3d::default(), Msaa::Off, Transform::from_xyz(0.0, 50.0, 0.0)));
                crate::drive::spawn_car(world);
            }
            // Evaluate the dressing with every spare core while nobody is driving.
            let fast = {
                let mut pcg = world.resource_mut::<PcgSettings>();
                let old = pcg.clone();
                let cores = std::thread::available_parallelism().map_or(4, |n| n.get());
                pcg.in_flight = cores.saturating_sub(2).max(4);
                pcg.cells_per_frame = 32;
                old
            };
            {
                let mut l = world.resource_mut::<Loading>();
                l.pcg = Some(fast);
                l.near_cells = near;
            }
            info!("loading: stage built in {:.2} s", t0.elapsed().as_secs_f32());
            set_phase(world, Phase::Warm);
        }
        Phase::Warm => {
            let since = world.resource::<Loading>().phase_at.elapsed().as_secs_f32();
            // Warm once the camera has followed the car to the start line (first frame after
            // the build), and again later for materials that arrived with the scenery.
            let warm_at = [0.15f32, 2.0];
            let frame = world.resource::<Time>().delta_secs();
            for t in warm_at {
                if since >= t && since - frame < t {
                    world.write_message(WarmPipelines);
                }
            }
            // Once a second: what we're still waiting for.
            if (since - frame).floor() < since.floor() {
                let pcg = world.get_resource::<PcgStats>().map(|s| (s.cells, s.cells_pending));
                debug!(
                    "loading: {:.0} s: textures {} car {} pcg {:?} trees {}",
                    since,
                    textures_loaded(world),
                    car_ready(world),
                    pcg,
                    crate::art::trees::near_ready(world)
                );
            }
            if since > 2.5 && content_ready(world) {
                set_phase(world, Phase::Settle);
            }
        }
        Phase::Settle => {
            let ms = world.resource::<Time<Real>>().delta_secs() * 1000.0;
            let mut l = world.resource_mut::<Loading>();
            l.calm = if ms < CALM_MS { l.calm + 1 } else { 0 };
            // A slow or busy GPU may never give 30 calm frames in a row (frames around
            // 30-50 ms): don't hold the player on "Final checks" for long because of it.
            if l.calm >= CALM_FRAMES || l.phase_at.elapsed().as_secs_f32() > SETTLE_MAX_S {
                finish(world);
                return;
            }
        }
    }
    if elapsed > TIMEOUT_S && world.resource::<Loading>().phase != Phase::Generate {
        warn!("loading: still not settled after {TIMEOUT_S} s ({phase:?}); starting anyway");
        finish(world);
    }
}

/// The generated stage, between the worker and the build frame.
#[derive(Resource)]
struct Pending(StageWorld);

fn set_phase(world: &mut World, p: Phase) {
    let mut l = world.resource_mut::<Loading>();
    info!("loading: {:?} → {:?} at {:.2} s", l.phase, p, l.started.elapsed().as_secs_f32());
    l.phase = p;
    l.phase_at = Instant::now();
}

/// Textures loaded, the car model in, every dressing cell spawned and the near trees out.
fn content_ready(world: &mut World) -> bool {
    textures_loaded(world) && car_ready(world) && pcg_progress(world).is_some_and(|p| p >= 1.0) && crate::art::trees::near_ready(world)
}

fn textures_loaded(world: &World) -> bool {
    let (Some(server), Some(mats)) = (world.get_resource::<AssetServer>(), world.get_resource::<Assets<StandardMaterial>>()) else { return true };
    mats.iter().all(|(_, m)| {
        [&m.base_color_texture, &m.normal_map_texture, &m.metallic_roughness_texture, &m.occlusion_texture, &m.emissive_texture]
            .into_iter()
            .flatten()
            .all(|h| !matches!(server.get_load_state(h.id()), Some(LoadState::Loading)))
    })
}

/// The car's model has spawned and found its wheels (or there's no model to wait for).
fn car_ready(world: &mut World) -> bool {
    let mut q = world.query_filtered::<(), (With<crate::art::car::CarModel>, Without<crate::art::car::CarRig>)>();
    q.iter(world).next().is_none()
}

/// Dressing cells whose centre is within [`NEAR_SCENERY`] of the start line.
fn near_cells(s: &StageWorld) -> u32 {
    let half = crate::stage::half_size(&s.def);
    let (start, _) = s.road.start();
    let start = Vec2::new(start.x, start.z);
    let (lo, hi) = ((-half / PCG_CELL).floor() as i32, (half / PCG_CELL).ceil() as i32);
    let mut n = 0;
    for z in lo..hi {
        for x in lo..hi {
            let c = (Vec2::new(x as f32, z as f32) + 0.5) * PCG_CELL;
            if c.distance(start) < NEAR_SCENERY {
                n += 1;
            }
        }
    }
    n
}

/// Share of the near dressing cells spawned (None before the volume has its cells). Cells are
/// evaluated nearest the camera first, so the first `near_cells` done are about the near ones.
fn pcg_progress(world: &World) -> Option<f32> {
    let s = world.get_resource::<PcgStats>()?;
    if s.cells == 0 {
        return None;
    }
    let want = world.get_resource::<Loading>().map_or(s.cells, |l| l.near_cells.clamp(1, s.cells));
    Some(((s.cells - s.cells_pending) as f32 / want as f32).min(1.0))
}

fn finish(world: &mut World) {
    let Some(l) = world.remove_resource::<Loading>() else { return };
    if let Some(old) = l.pcg {
        world.insert_resource(old);
    }
    let mut q = world.query_filtered::<Entity, With<LoadingRoot>>();
    let roots: Vec<Entity> = q.iter(world).collect();
    for e in roots {
        world.entity_mut(e).despawn();
    }
    // The screen dissolves into the stage (visual only: the stage is live from now).
    if crate::art::visual(world) {
        let bg = world.resource::<engine::ui::theme::Theme>().bg;
        let now = world.resource::<Time<Real>>().elapsed_secs();
        world.spawn((LoadingFade(now), engine::ui::theme::fullscreen(), BackgroundColor(bg), GlobalZIndex(1000), Pickable::IGNORE, DespawnOnExit(AppState::InGame)));
    }
    world.remove_resource::<TipCycle>();
    world.insert_resource(PauseAllowed(true));
    #[cfg(feature = "telemetry")]
    world.remove_resource::<engine::trace::TraceQuiet>();
    info!("loading: stage {} ready in {:.2} s", l.def.id, l.started.elapsed().as_secs_f32());
}

/// Left the stage while it was loading: drop the work (and restore the PCG limits).
fn abandon(world: &mut World) {
    world.remove_resource::<Pending>();
    #[cfg(feature = "telemetry")]
    world.remove_resource::<engine::trace::TraceQuiet>();
    if let Some(l) = world.remove_resource::<Loading>()
        && let Some(old) = l.pcg
    {
        world.insert_resource(old);
    }
}

/// Progress bar and label.
fn draw(world: &mut World) {
    // Gone when `advance` just finished the load this frame.
    let Some((phase, since)) = world.get_resource::<Loading>().map(|l| (l.phase, l.phase_at.elapsed().as_secs_f32())) else { return };
    // Generate 0–15 %, build 15–25 %, warm 25–95 % (dressing cells), settle 95–100 %.
    let target = match phase {
        Phase::Generate => 0.15 * (1.0 - (-since * 2.0).exp()),
        Phase::Build => 0.2,
        Phase::Warm => 0.25 + 0.7 * pcg_progress(world).unwrap_or(0.0) * (since / 2.5).min(1.0),
        Phase::Settle => 0.95 + 0.05 * world.resource::<Loading>().calm as f32 / CALM_FRAMES as f32,
    };
    let shown = {
        let mut l = world.resource_mut::<Loading>();
        l.shown = l.shown.max(target);
        l.shown
    };
    let mut bars = world.query_filtered::<&mut Node, With<LoadingBar>>();
    for mut n in bars.iter_mut(world) {
        n.width = Val::Percent(shown * 100.0);
    }
    take_map(world);
    let now = world.resource::<Time<Real>>().elapsed_secs();
    let mut maps = world.query::<(&mut ImageNode, &LoadingMap)>();
    for (mut img, m) in maps.iter_mut(world) {
        let a = ((now - m.since) / 0.6).clamp(0.0, 1.0) * 0.85;
        if img.image != Handle::default() && (img.color.alpha() - a).abs() > 0.004 {
            img.color = Color::srgba(1.0, 1.0, 1.0, a);
        }
    }
    let elapsed = world.resource::<Loading>().started.elapsed().as_secs_f32();
    let want = if elapsed > SLOW_S { format!("{} \u{b7} taking longer than usual", phase.label()).to_uppercase() } else { phase.label().to_uppercase() };
    let mut labels = world.query_filtered::<&mut Text, With<LoadingLabel>>();
    for mut t in labels.iter_mut(world) {
        if t.0 != want {
            t.0 = want.clone();
        }
    }
    if let Some((d, n)) = events_progress(world) {
        let v = format!("{d} of {n}");
        let mut q = world.query_filtered::<&mut Text, With<EventsLine>>();
        for mut t in q.iter_mut(world) {
            if t.0 != v {
                t.0 = v.clone();
            }
        }
    }
    let step = format!("{} / 4", phase.step());
    let mut steps = world.query_filtered::<&mut Text, With<PhaseStep>>();
    for mut t in steps.iter_mut(world) {
        if t.0 != step {
            t.0 = step.clone();
        }
    }
}

/// Rotates the tip every 7 s (crossfade 300 ms); Left / Right (LB / RB) step by hand.
fn tips(world: &mut World) {
    let now = world.resource::<Time<Real>>().elapsed_secs();
    // Hand steps from the menu navigation keys (no screen is open while loading).
    let mut hand = 0;
    if let Some(mut q) = world.get_resource_mut::<engine::ui::widgets::NavQueue>() {
        q.0.retain(|i| match i {
            engine::ui::widgets::NavInput::Left | engine::ui::widgets::NavInput::PrevTab => {
                hand = -1;
                false
            }
            engine::ui::widgets::NavInput::Right | engine::ui::widgets::NavInput::NextTab => {
                hand = 1;
                false
            }
            _ => true,
        });
    }
    let Some(mut c) = world.get_resource_mut::<TipCycle>() else { return };
    if c.tips.is_empty() {
        return;
    }
    if c.step.is_none() {
        if hand != 0 {
            c.step = Some((hand, now));
        } else if now - c.since > TIP_EVERY {
            c.step = Some((1, now));
        }
    }
    // Fade out, swap at the middle, fade in.
    let mut alpha = 1.0;
    let mut swap = None;
    if let Some((dir, t0)) = c.step {
        let k = (now - t0) / TIP_FADE;
        if k >= 0.5 && c.since < t0 {
            let n = c.tips.len() as i32;
            c.at = (c.at as i32 + dir).rem_euclid(n) as usize;
            c.since = now;
            swap = Some((c.tips[c.at].clone(), format!("{} / {}", c.at + 1, n)));
        }
        alpha = if k < 0.5 { 1.0 - k * 2.0 } else { ((k - 0.5) * 2.0).min(1.0) };
        if k >= 1.0 {
            c.step = None;
            alpha = 1.0;
        }
    }
    if let Some((tip, count)) = swap {
        let mut q = world.query_filtered::<&mut Text, With<TipText>>();
        for mut t in q.iter_mut(world) {
            t.0 = tip.clone();
        }
        let mut q = world.query_filtered::<&mut Text, (With<TipCount>, Without<TipText>)>();
        for mut t in q.iter_mut(world) {
            t.0 = count.clone();
        }
    }
    let mut q = world.query_filtered::<&mut TextColor, With<TipText>>();
    for mut col in q.iter_mut(world) {
        if (col.0.alpha() - alpha).abs() > 0.01 {
            col.0.set_alpha(alpha);
        }
    }
}

/// The open world's region map: once the painted map and the stage exist, frame the route
/// (an event) or the car's spot and draw the route as a dotted line, start to finish over
/// 1.2 s (at once with reduce motion).
fn region_map(world: &mut World) {
    let now = world.resource::<Time<Real>>().elapsed_secs();
    let mut q = world.query::<(Entity, &RegionMap)>();
    let Some((map_e, built)) = q.iter(world).next().map(|(e, m)| (e, m.built)) else { return };
    if let Some(t0) = built {
        // Fade the picture in, then reveal the dots in order.
        let a = ((now - t0) / 0.6).clamp(0.0, 1.0);
        if let Some(mut img) = world.get_mut::<ImageNode>(map_e)
            && (img.color.alpha() - a).abs() > 0.004
        {
            img.color = Color::srgba(1.0, 1.0, 1.0, a);
        }
        let shown = if crate::meta::hub::reduce_motion(world) { 1.0 } else { ((now - t0 - 0.3) / 1.2).clamp(0.0, 1.0) };
        let mut dots = world.query::<(&RouteDot, &mut Visibility)>();
        for (d, mut v) in dots.iter_mut(world) {
            let want = if d.0 <= shown { Visibility::Inherited } else { Visibility::Hidden };
            if *v != want {
                *v = want;
            }
        }
        return;
    }
    let Some(map) = world.get_resource::<crate::map::MapArt>().and_then(|a| a.map.clone()) else { return };
    let Some(stage) = world.get_resource::<crate::world::StageWorld>().cloned() else { return };
    if stage.open.is_none() {
        return;
    }
    let car = world.query::<&crate::drive::DrivenCar>().iter(world).next().map(|c| Vec2::new(c.car.pos.x, c.car.pos.z));
    // Still waiting to be put on the event's line: wait a frame.
    if world.contains_resource::<crate::meta::PendingStart>() {
        return;
    }
    let route: Vec<Vec2> = if stage.event.is_some() { stage.road.pts.iter().map(|p| Vec2::new(p.x, p.z)).collect() } else { Vec::new() };
    // The window: the route's box (with a margin), or 1.4 km around the car.
    let (lo, hi) = if route.len() > 1 {
        let (mut lo, mut hi) = (route[0], route[0]);
        for p in &route {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
        let c = (lo + hi) * 0.5;
        let half = (hi - lo).max_element() * 0.5 * 1.18 + 120.0;
        (c - Vec2::splat(half), c + Vec2::splat(half))
    } else {
        let c = car.unwrap_or(Vec2::ZERO);
        (c - Vec2::splat(700.0), c + Vec2::splat(700.0))
    };
    let size = crate::map::paint::SIZE as f32;
    let (a, b) = (crate::map::paint::uv(lo) * size, crate::map::paint::uv(hi) * size);
    let rect = Rect::from_corners(a.max(Vec2::ZERO), b.min(Vec2::splat(size)));
    // A world point → percent of the frame.
    let to = |p: Vec2| {
        let q = crate::map::paint::uv(p) * size;
        ((q - rect.min) / rect.size()) * 100.0
    };
    let t = world.resource::<engine::ui::theme::Theme>().clone();
    let gl = world.get_resource::<crate::meta::hub::Glyphs>().cloned().unwrap_or_default();
    world.entity_mut(map_e).insert((RegionMap { built: Some(now) }, ImageNode { image: map, rect: Some(rect), color: Color::NONE, ..default() }));
    let parent = world.get::<ChildOf>(map_e).map(|c| c.parent());
    let Some(frame) = parent else { return };
    world.commands().entity(frame).with_children(|f| {
        let dot = |f: &mut ChildSpawnerCommands, p: Vec2, size: f32, color: Color, k: f32| {
            f.spawn((
                RouteDot(k),
                Node { position_type: PositionType::Absolute, left: percent(p.x), top: percent(p.y), width: px(size), height: px(size), margin: UiRect::new(px(-size / 2.0), px(0), px(-size / 2.0), px(0)), border_radius: BorderRadius::all(px(size)), ..default() },
                BackgroundColor(color),
                Visibility::Hidden,
                Pickable::IGNORE,
            ));
        };
        if route.len() > 1 {
            // ~140 dots along the route, a casing under each for contrast.
            let len: f32 = route.windows(2).map(|w| w[0].distance(w[1])).sum();
            let n = 140;
            let mut acc = 0.0;
            let mut next = 0.0;
            let mut k = 0;
            for w in route.windows(2) {
                let d = w[0].distance(w[1]);
                while next <= acc + d && k <= n {
                    let p = w[0].lerp(w[1], ((next - acc) / d.max(1e-3)).clamp(0.0, 1.0));
                    let frac = k as f32 / n as f32;
                    dot(f, to(p), 9.0, t.bg, frac);
                    dot(f, to(p), 5.0, t.accent, frac);
                    k += 1;
                    next = len * k as f32 / n as f32;
                }
                acc += d;
            }
            // Start and finish markers.
            let (s, e) = (to(route[0]), to(*route.last().unwrap_or(&route[0])));
            dot(f, s, 18.0, t.good, 0.0);
            f.spawn((RouteDot(1.0), Node { position_type: PositionType::Absolute, left: percent(e.x), top: percent(e.y), width: px(16), height: px(16), margin: UiRect::new(px(-8), px(0), px(-8), px(0)), border: UiRect::all(px(3)), ..default() }, BorderColor::all(t.text), BackgroundColor(t.bad), Visibility::Hidden, Pickable::IGNORE));
        } else if let Some(c) = car {
            // You are here: a ring and a dot.
            let p = to(c);
            f.spawn((RouteDot(0.0), Node { position_type: PositionType::Absolute, left: percent(p.x), top: percent(p.y), width: px(34), height: px(34), margin: UiRect::new(px(-17), px(0), px(-17), px(0)), ..default() }, Visibility::Hidden, Pickable::IGNORE)).with_children(|r| {
                crate::meta::hub::glyph(r, &gl, crate::meta::hub::Glyph::Ring, 34.0, t.accent);
            });
            dot(f, p, 10.0, t.accent, 0.0);
        }
    });
}

fn fade_in(mut commands: Commands, time: Res<Time<Real>>, mut q: Query<(Entity, &LoadingIntro, &mut BackgroundColor)>) {
    for (e, f, mut bg) in &mut q {
        let a = 1.0 - ((time.elapsed_secs() - f.0) / 0.2).clamp(0.0, 1.0);
        bg.0.set_alpha(a);
        if a <= 0.0 {
            commands.entity(e).despawn();
        }
    }
}

/// The loaded stage fades in from the screen's background (250 ms).
fn fade_out(mut commands: Commands, time: Res<Time<Real>>, mut q: Query<(Entity, &LoadingFade, &mut BackgroundColor)>) {
    for (e, f, mut bg) in &mut q {
        let a = 1.0 - ((time.elapsed_secs() - f.0) / 0.25).clamp(0.0, 1.0);
        bg.0.set_alpha(a);
        if a <= 0.0 {
            commands.entity(e).despawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_hours_read_as_words_and_a_clock() {
        assert_eq!((hour_name(9.0), clock_text(9.0)), ("Morning", "09:00".to_string()));
        assert_eq!((hour_name(16.0), clock_text(16.25)), ("Afternoon", "16:15".to_string()));
        assert_eq!(hour_name(23.0), "Night");
        assert_eq!(clock_text(23.999), "00:00");
    }
}
