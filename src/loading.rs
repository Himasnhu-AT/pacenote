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
    fn label(self) -> &'static str {
        match self {
            Phase::Generate => "Surveying the stage",
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

const TIPS: &[&str] = &[
    "Your co-driver reads the road ahead: listen for the corner grade (1 hairpin to 6 flat).",
    "Big hits damage the car. A damaged engine loses power; bent steering pulls to one side.",
    "Marker posts, signs and saplings snap if you clip them, and shrubs bend: none of them hurt the car.",
    "Press C to change camera: cockpit, bonnet or chase.",
    "Stuck or upside down? R puts you back on the road for a 5 s penalty.",
    "The handbrake (Space) helps the car rotate into hairpins.",
];

pub struct LoadingPlugin;

impl Plugin for LoadingPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (advance, draw).chain().run_if(resource_exists::<Loading>))
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

/// The screen: the stage's contour map on the right (as on the main menu), its name and
/// facts, your best and the stage record, progress, and a tip. Everything here is already
/// known (catalog, board, profile); only the map is drawn on a worker.
fn spawn_screen(world: &mut World, def: &StageDef) {
    use crate::meta::screens::{biome_name, stat, surface_name, time_name, weather_name};
    use crate::meta::{Boards, Profile, RunMode, board};
    use engine::ui::prelude::*;

    let t = world.resource::<Theme>().clone();
    let tip = TIPS[(def.seed as usize) % TIPS.len()];
    let cat = world.resource::<crate::meta::stages::StageCatalog>();
    let info = cat.info(def.id).cloned();
    // Until the worker's map is in: stage 1's backdrop, else the catalog's road sketch.
    let (picture, is_map) = match (def.id == 1).then(|| cat.map.clone()).flatten() {
        Some(m) => (Some(m), true),
        None => (cat.preview(def.id), false),
    };
    let mode = world.get_resource::<RunMode>().copied().unwrap_or_default();
    let p = world.resource::<Profile>().clone();
    let b = &world.resource::<Boards>().0;
    let team = board::team_key(&p.driver_name, if mode == RunMode::Team { &p.guide_name } else { "" });
    // This weather and time of day's board (crate::meta::stages::board_hash).
    let (best, leader) = match &info {
        Some(i) => {
            let h = crate::meta::stages::board_hash(&i.hash, def, crate::meta::picked_assists(world));
            (b.best_of(def.id, &h, &team).cloned(), b.leader(def.id, &h).cloned())
        }
        None => (None, None),
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
    let facts = format!("{} \u{b7} {} \u{b7} {}, {}", biome_name(def.biome), surf, time_name(def.time), weather_name(def.weather));
    let mut kicker = format!("Stage {:02} \u{b7} {}", def.id, if mode == RunMode::Team { "Team run" } else { "Practice" });
    if def.id == crate::open::ID {
        kicker = "Free roam".into();
    } else if crate::meta::stages::conditions_key(def).is_some() {
        kicker.push_str(&format!(" \u{b7} {} {}", time_name(def.time), weather_name(def.weather)));
    }
    let now = world.resource::<Time<Real>>().elapsed_secs();
    let name_size = if def.name.chars().count() > 12 { 96.0 } else { 120.0 };
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
            // The map, right, bleeding off the edges like the menu backdrop; the menu scrim
            // keeps the text side readable.
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
            menu_backdrop(root, &t, 0.0, 1.0);
            root.spawn(menu_column(GAP_S)).with_children(|c| {
                eyebrow_bar(c, &kicker, t.accent);
                c.spawn(title(&def.name, name_size, t.text));
                c.spawn((text(facts, body(SIZE_BODY + 2.0), t.text_muted), Node { max_width: px(760), ..default() }));
                c.spawn((Node { column_gap: px(GAP_L), margin: UiRect::vertical(px(GAP_M)), ..default() }, Pickable::IGNORE)).with_children(|r| {
                    let len = info.as_ref().map_or(def.length, |i| i.length);
                    stat(r, &t, format!("{:.2}", len / 1000.0), "km", t.text, 44.0);
                    if let Some(i) = &info {
                        stat(r, &t, i.corners.to_string(), "Corners", t.text, 44.0);
                        stat(r, &t, i.tight.to_string(), "Hairpins & 2s", t.text, 44.0);
                        if i.crests > 0 {
                            stat(r, &t, i.crests.to_string(), "Crests", t.text, 44.0);
                        }
                        stat(r, &t, format!("{:.0}", i.climb), "m climb", t.text, 44.0);
                    }
                });
                c.spawn((Node { flex_direction: FlexDirection::Column, row_gap: px(GAP_XS + 2.0), width: px(560), ..default() }, Pickable::IGNORE)).with_children(|l| {
                    let line = |l: &mut ChildSpawnerCommands, name: &str, r: Option<&board::Record>, color: Color| {
                        l.spawn((Node { justify_content: JustifyContent::SpaceBetween, ..default() }, Pickable::IGNORE)).with_children(|row| {
                            row.spawn(text(name, body(SIZE_BODY), t.text_muted));
                            match r {
                                Some(r) => row.spawn(text(format!("{}   {}", board::clock(r.total()), r.team_label()), body_bold(SIZE_BODY), color)),
                                None => row.spawn(text("no time yet", body(SIZE_BODY), t.text_dim)),
                            };
                        });
                    };
                    line(l, if mode == RunMode::Team { "Team best" } else { "Your best" }, best.as_ref(), t.text);
                    line(l, "Stage record", leader.as_ref(), t.accent);
                });
                c.spawn(spacer(GAP_L));
                // Progress.
                c.spawn((Node { width: px(560), height: px(4), ..default() }, BackgroundColor(t.line), Pickable::IGNORE)).with_children(|b| {
                    b.spawn((LoadingBar, Node { width: percent(0), height: percent(100), ..default() }, BackgroundColor(t.accent), Pickable::IGNORE));
                });
                c.spawn((LoadingLabel, text(Phase::Generate.label().to_uppercase(), label(SIZE_LABEL), t.text_muted)));
            });
            // Tip, bottom left where the menus keep their key hints.
            root.spawn((Node { position_type: PositionType::Absolute, left: px(EDGE), bottom: px(FOOTER_BOTTOM), width: px(620), ..default() }, Pickable::IGNORE)).with_children(|w| {
                w.spawn(accent_panel(&t, t.accent, 18.0, GAP_XS + 2.0)).with_children(|card| {
                    card.spawn(eyebrow("Pace note", t.accent));
                    card.spawn(text(tip, body(SIZE_BODY), t.text));
                });
            });
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
    let mut labels = world.query_filtered::<&mut Text, With<LoadingLabel>>();
    for mut t in labels.iter_mut(world) {
        if !t.0.eq_ignore_ascii_case(phase.label()) {
            t.0 = phase.label().to_uppercase();
        }
    }
}
