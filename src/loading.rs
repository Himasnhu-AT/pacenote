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

/// Frames under this long count as settled.
const CALM_MS: f32 = 40.0;
/// Settled frames needed in a row.
const CALM_FRAMES: u32 = 30;
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
        world.spawn((engine::gfx::MainCamera, Camera3d::default(), Transform::from_xyz(0.0, 50.0, 0.0)));
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
    let now = Instant::now();
    world.insert_resource(Loading { phase: Phase::Generate, started: now, phase_at: now, def: def.clone(), task: Some(task), calm: 0, pcg: None, shown: 0.0, near_cells: 0 });
    world.insert_resource(PauseAllowed(false));
    spawn_screen(world, &def);
}

fn spawn_screen(world: &mut World, def: &StageDef) {
    let tip = TIPS[(def.seed as usize) % TIPS.len()];
    let title = format!("STAGE {}  -  {}", def.id, def.name.to_uppercase());
    let sub = format!("{:?}   /   {:?}   /   {:?}", def.surface, def.biome, def.weather).to_uppercase();
    let font = |px: f32| TextFont { font_size: bevy::text::FontSize::Px(px), ..default() };
    world
        .spawn((
            LoadingRoot,
            DespawnOnExit(AppState::InGame),
            GlobalZIndex(1000),
            Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), flex_direction: FlexDirection::Column, justify_content: JustifyContent::Center, align_items: AlignItems::Center, row_gap: Val::Px(18.0), ..default() },
            BackgroundColor(Color::srgb(0.035, 0.04, 0.045)),
        ))
        .with_children(|p| {
            p.spawn((Text::new("PACENOTE"), font(18.0), TextColor(Color::srgba(1.0, 0.85, 0.3, 0.9))));
            p.spawn((Text::new(title), font(44.0), TextColor(Color::WHITE)));
            p.spawn((Text::new(sub), font(16.0), TextColor(Color::srgba(1.0, 1.0, 1.0, 0.55))));
            // Progress bar.
            p.spawn((Node { width: Val::Px(520.0), height: Val::Px(6.0), margin: UiRect::top(Val::Px(28.0)), ..default() }, BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.12)))).with_children(|b| {
                b.spawn((LoadingBar, Node { width: Val::Percent(0.0), height: Val::Percent(100.0), ..default() }, BackgroundColor(Color::srgb(1.0, 0.85, 0.3))));
            });
            p.spawn((LoadingLabel, Text::new(Phase::Generate.label()), font(16.0), TextColor(Color::srgba(1.0, 1.0, 1.0, 0.7))));
            p.spawn((Text::new(tip), font(16.0), TextColor(Color::srgba(1.0, 1.0, 1.0, 0.45)), Node { margin: UiRect::top(Val::Px(40.0)), max_width: Val::Px(640.0), ..default() }, TextLayout::justify(Justify::Center)));
        });
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
            crate::world::spawn(world, &s);
            world.spawn((engine::gfx::MainCamera, Camera3d::default(), Transform::from_xyz(0.0, 50.0, 0.0)));
            crate::drive::spawn_car(world);
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
            if l.calm >= CALM_FRAMES {
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
    info!("loading: stage {} ready in {:.2} s", l.def.id, l.started.elapsed().as_secs_f32());
}

/// Left the stage while it was loading: drop the work (and restore the PCG limits).
fn abandon(world: &mut World) {
    world.remove_resource::<Pending>();
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
    let mut labels = world.query_filtered::<&mut Text, With<LoadingLabel>>();
    for mut t in labels.iter_mut(world) {
        if t.0 != phase.label() {
            t.0 = phase.label().to_string();
        }
    }
}
