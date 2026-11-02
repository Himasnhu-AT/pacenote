//! The front end and competition layer: menus, stage select, team names, leaderboards,
//! results, ghosts, pause menu, credits. See `docs/META.md`.
//!
//! - [`board`]: records, ranking, merge, exchange file (pure, tested).
//! - [`ghost`]: input recording and exact replay (pure, tested) + the ghost car.
//! - [`stages`]: per-stage menu facts and pictures, generated in the background.
//! - [`netlink`]: the menus' only contact with networking (host / join / leave / Role).
//! - `screens`: every screen on the engine UI kit; `textfield`: a text input widget.
//!
//! Flow: main menu → stage select → [`start_stage`] → `AppState::InGame` (the game's
//! `load_stage`) → finish → [`finish_run`] writes the record and the ghost → "results" →
//! retry ([`restart_stage`]) / next stage / menu. Leaving `InGame` runs [`teardown`], which
//! removes the stage so another can load.

pub mod board;
pub mod ghost;
pub mod netlink;
pub(crate) mod screens;
pub mod stages;
mod textfield;

use std::sync::Arc;

use bevy::prelude::*;
use engine::flow::{AppState, PauseAllowed};
use engine::paths::DataDir;
use engine::ui::prelude::*;
use serde::{Deserialize, Serialize};

use crate::StageChoice;
use crate::stage::{StageDef, TimeOfDay, Weather};
use crate::drive::{CameraView, CarSpec, DrivenCar, StageRun};
use crate::world::StageWorld;

use board::{Board, BoardFile, Inserted, Record, team_key};
use ghost::{Ghost, Recorder, Replay};
pub use netlink::Role;
use stages::StageCatalog;

/// Leaderboard file in the data dir.
pub const BOARD_FILE: &str = "leaderboard.json";
/// Ghost files: `<data>/ghosts/sNN-<team>.json`.
pub const GHOST_DIR: &str = "ghosts";
/// Exports go here; files dropped in `boards/inbox` are merged by "Import".
pub const EXPORT_DIR: &str = "boards";
pub const INBOX_DIR: &str = "boards/inbox";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Fixed steps per second of the car simulation (lib.rs sets `Time<Fixed>` to this).
pub const HZ: u32 = 60;

/// Who we are and what we last chose; section `"profile"` of settings.json.
#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub driver_name: String,
    pub guide_name: String,
    /// Port the driver hosts on.
    pub host_port: u16,
    /// Address the guide joins (host:port).
    pub join_address: String,
    pub last_stage: u32,
    /// 0 off, 1 our best, 2 fastest available, 3 the rival chosen on the leaderboard.
    pub ghost: u32,
    pub ghost_opacity: f32,
    /// Chosen rival team key per stage.
    pub rivals: Vec<(u32, String)>,
    /// Send anonymous gameplay stats (docs/TELEMETRY.md). On by default.
    pub share_stats: bool,
    /// Send crash and hang reports (docs/TELEMETRY.md). On by default.
    pub send_crash_reports: bool,
    /// The first-launch note about stats was shown.
    pub stats_notice_seen: bool,
    /// Weather and time of day picked per stage (only stages not at their defaults).
    pub conditions: Vec<(u32, Weather, TimeOfDay)>,
    /// Driver's camera: 0 cockpit, 1 bonnet, 2 chase ([`CAMERA_VIEWS`]). Runs start in it; C
    /// in game changes it (and it's remembered).
    pub camera: u32,
}

/// The camera views in [`Profile::camera`] order.
pub const CAMERA_VIEWS: [CameraView; 3] = [CameraView::Cockpit, CameraView::Bonnet, CameraView::Chase];

/// "Cockpit" / "Bonnet" / "Chase".
pub fn view_name(v: CameraView) -> &'static str {
    match v {
        CameraView::Cockpit => "Cockpit",
        CameraView::Bonnet => "Bonnet",
        CameraView::Chase => "Chase",
    }
}

pub fn view_index(v: CameraView) -> u32 {
    CAMERA_VIEWS.iter().position(|x| *x == v).unwrap_or(0) as u32
}

impl Default for Profile {
    fn default() -> Self {
        Profile {
            driver_name: "Driver".into(),
            guide_name: "Guide".into(),
            host_port: 7777,
            join_address: "127.0.0.1:7777".into(),
            last_stage: 1,
            ghost: 1,
            ghost_opacity: 0.45,
            rivals: Vec::new(),
            share_stats: true,
            send_crash_reports: true,
            stats_notice_seen: false,
            conditions: Vec::new(),
            camera: 0,
        }
    }
}

impl Profile {
    pub fn rival(&self, stage: u32) -> Option<&str> {
        self.rivals.iter().find(|(s, _)| *s == stage).map(|(_, t)| t.as_str())
    }
    pub fn set_rival(&mut self, stage: u32, team: String) {
        self.rivals.retain(|(s, _)| *s != stage);
        self.rivals.push((stage, team));
    }
    /// The weather and time picked for `def`'s stage (its own when none was picked).
    pub fn conditions(&self, def: &StageDef) -> (Weather, TimeOfDay) {
        self.conditions.iter().find(|(s, ..)| *s == def.id).map_or((def.weather, def.time), |(_, w, t)| (*w, *t))
    }
    /// Remember a pick (forgotten again when it's the stage's default).
    pub fn set_conditions(&mut self, def: &StageDef, weather: Weather, time: TimeOfDay) {
        self.conditions.retain(|(s, ..)| *s != def.id);
        if (weather, time) != (def.weather, def.time) {
            self.conditions.push((def.id, weather, time));
        }
    }
}

/// Stage `id` as it will be driven: the published stage with the weather and time picked for
/// it (Profile), or `--weather <clear|overcast|fog|rain>` / `--time <morning|noon|afternoon|dusk>`
/// on the command line (dev builds) for tests.
pub fn stage_def(world: &World, id: u32) -> Option<StageDef> {
    let def = stages::base_def(id)?;
    let (mut w, mut t) = world.get_resource::<Profile>().map_or((def.weather, def.time), |p| p.conditions(&def));
    if let Some(v) = engine::cli::dev_value("weather") {
        w = stages::WEATHERS.into_iter().find(|x| format!("{x:?}").eq_ignore_ascii_case(&v)).unwrap_or(w);
    }
    if let Some(v) = engine::cli::dev_value("time") {
        t = stages::TIMES.into_iter().find(|x| format!("{x:?}").eq_ignore_ascii_case(&v)).unwrap_or(t);
    }
    if !stages::weathers_for(&def).contains(&w) {
        w = def.weather;
    }
    Some(stages::with_conditions(def, w, t))
}

/// The assists picked in Settings (what the next run is driven with).
pub fn picked_assists(world: &World) -> stages::Assists {
    world.get_resource::<crate::prefs::Prefs>().map_or_else(stages::Assists::default, |p| p.assists())
}

/// The assists the loaded car actually has.
pub fn run_assists(world: &World) -> stages::Assists {
    world.get_resource::<CarSpec>().map_or_else(stages::Assists::default, |s| stages::Assists { abs: s.0.abs, traction_control: s.0.traction_assist })
}

/// The board hash of stage `id` as it will be driven (see [`stages::board_hash`]); None until
/// the catalog has the stage.
pub fn board_hash_for(world: &World, id: u32) -> Option<String> {
    let road = world.resource::<StageCatalog>().info(id)?.hash.clone();
    Some(stages::board_hash(&road, &stage_def(world, id)?, picked_assists(world)))
}

/// How the next / current run counts.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RunMode {
    /// Solo practice: own board section, own ghost.
    #[default]
    Practice,
    /// Driver with a guide: a team run.
    Team,
}

/// The loaded leaderboard.
#[derive(Resource, Clone, Debug, Default)]
pub struct Boards(pub Board);

/// The stage currently loaded, as the leaderboard sees it.
#[derive(Resource, Clone, Debug)]
pub struct StageNow {
    pub id: u32,
    pub name: String,
    pub hash: String,
}

/// The last finished run, for the results screen.
#[derive(Resource, Clone, Debug)]
pub struct LastResult {
    pub record: Record,
    pub inserted: Inserted,
    pub stage_name: String,
    /// The team's best and the stage leader before this run.
    pub best_before: Option<Record>,
    pub leader_before: Option<Record>,
    pub ghost_saved: bool,
}

/// The ghost car on screen.
#[derive(Component)]
pub struct GhostCar {
    pub ghost: Arc<Ghost>,
    pub replay: Replay,
    pub prev: (Vec3, Quat),
    /// The ghost's road and sim matched when it was recorded.
    pub current: bool,
}

#[derive(Component)]
struct GhostBody;

/// The ghost's copy of the car model, waiting to be dressed: its far level of detail only,
/// in the ghost material ([`dress_ghosts`]).
#[derive(Component)]
struct GhostModel(Handle<StandardMaterial>);

/// Menu events collected for the exclusive handler.
#[derive(Resource, Default)]
struct MenuQueue(Vec<&'static str>);

/// The run currently on screen was already written to the board.
#[derive(Resource, Default)]
pub(crate) struct Finished(pub(crate) bool);

pub struct MetaPlugin;

impl Plugin for MetaPlugin {
    fn build(&self, app: &mut App) {
        engine::settings::add_settings_section::<Profile>(app, "profile");
        app.init_resource::<RunMode>()
            .init_resource::<Recorder>()
            .init_resource::<MenuQueue>()
            .init_resource::<Finished>()
            .init_resource::<Role>()
            .init_resource::<StageCatalog>()
            .add_plugins((textfield::TextFieldPlugin, screens::ScreensPlugin))
            .add_systems(Startup, (load_board, start_catalog, import_from_cli).chain())
            .add_systems(Update, poll_catalog)
            .add_systems(Update, (sync_camera, free_pointer))
            .add_systems(Update, (collect_menu, apply_menu).chain().after(engine::ui::UiSet::Screens))
            .add_systems(Update, (stage_ready, watch_finish, follow_ghost, dress_ghosts).chain().run_if(resource_exists::<StageWorld>))
            .add_systems(Update, restart_after_death.run_if(resource_exists::<StageRun>))
            .add_systems(OnExit(AppState::InGame), teardown);
        #[cfg(feature = "dev")]
        dev::register(app);
    }
}

/// Keeps the driver's camera and `Profile::camera` the same, whichever changed: C in game
/// (toast naming the view), the setting, or the stage picker's chip.
fn sync_camera(
    view: Option<ResMut<CameraView>>,
    mut profile: ResMut<Profile>,
    state: Option<Res<State<AppState>>>,
    role: Option<Res<Role>>,
    mut last: Local<Option<(CameraView, u32)>>,
    mut toasts: MessageWriter<Toast>,
) {
    let Some(mut view) = view else { return };
    let want = CAMERA_VIEWS[(profile.camera as usize).min(CAMERA_VIEWS.len() - 1)];
    let mut toast = false;
    match *last {
        // First frame: the saved choice wins.
        None => {
            if *view != want {
                *view = want;
            }
        }
        Some((_, p)) if p != profile.camera => {
            if *view != want {
                *view = want;
                toast = true;
            }
        }
        Some((v, _)) if v != *view => {
            profile.camera = view_index(*view);
            toast = true;
        }
        _ => {}
    }
    // Name the view when it changes in game (C, or the setting from the pause menu).
    let in_game = state.is_some_and(|s| *s.get() == AppState::InGame) && role.is_none_or(|r| *r != Role::Guide);
    if toast && in_game {
        toasts.write(Toast::new(format!("{} camera", view_name(*view))).secs(1.6));
    }
    *last = Some((*view, profile.camera));
}

/// The mouse pointer is only captured while there's a car to drive or a head to turn: the guide
/// on the map screen (pan, zoom) and a finished or retired run keep it free; the guide in the
/// car looks around with it. (The engine also frees it whenever a screen such as the results is
/// open.)
fn free_pointer(run: Option<Res<StageRun>>, role: Option<Res<Role>>, screen: Option<Res<crate::guide::GuideScreen>>, capture: Option<ResMut<engine::input::CaptureCursor>>) {
    let Some(mut capture) = capture else { return };
    let guide = role.is_some_and(|r| *r == Role::Guide) && screen.is_none_or(|s| *s == crate::guide::GuideScreen::Map);
    let done = run.is_some_and(|r| r.finished.is_some() || r.retired);
    let want = !(guide || done);
    if capture.0 != want {
        capture.0 = want;
    }
}

fn load_board(mut commands: Commands, dir: Option<Res<DataDir>>) {
    let mut b: Board = dir.map(|d| d.load_json(BOARD_FILE)).unwrap_or_default();
    b.sort();
    commands.insert_resource(Boards(b));
}

fn start_catalog(mut cat: ResMut<StageCatalog>, theme: Res<Theme>) {
    let a = theme.accent.to_srgba();
    let m = theme.text_muted.to_srgba();
    cat.start([a.red, a.green, a.blue], [m.red, m.green, m.blue]);
}

fn poll_catalog(mut cat: ResMut<StageCatalog>, images: Option<ResMut<Assets<Image>>>, mut rebuild: MessageWriter<RebuildScreen>, screens: Option<Res<Screens>>) {
    if cat.ready() {
        return;
    }
    if cat.poll(images.map(|i| i.into_inner())) && matches!(screens.and_then(|s| s.top()), Some("main" | "stages" | "boards")) {
        rebuild.write(RebuildScreen);
    }
}

/// `--import-board <file.json>`: merge a friend's board at startup.
fn import_from_cli(world: &mut World) {
    if let Some(path) = engine::cli::arg_value("import-board") {
        match import_file(world, std::path::Path::new(&path)) {
            Ok(n) => info!("imported {path}: {n} records added or improved"),
            Err(e) => warn!("import {path}: {e}"),
        }
    }
}

fn collect_menu(mut events: MessageReader<MenuEvent>, mut q: ResMut<MenuQueue>) {
    q.0.extend(events.read().map(|e| e.0));
}

fn apply_menu(world: &mut World) {
    let ids = std::mem::take(&mut world.resource_mut::<MenuQueue>().0);
    for id in ids {
        screens::on_menu(world, id);
    }
}

// ---------------------------------------------------------------- flow

/// Loads stage `id` (1..=20) as practice or a team run (hosting when `Team`).
pub fn start_stage(world: &mut World, id: u32, mode: RunMode) {
    // The open world (id 0) or a stage (1..=20).
    let open = id == crate::open::ID;
    let id = if open { id } else { crate::demo::clamp_stage(id.clamp(1, crate::stage::stages().len() as u32)) };
    match mode {
        RunMode::Practice => {
            if netlink::role(world) != Role::Solo {
                netlink::leave(world);
            }
        }
        RunMode::Team => {
            if netlink::role(world) != Role::Driver {
                let port = world.resource::<Profile>().host_port;
                if let Err(e) = netlink::host(world, port) {
                    world.write_message(Toast::new("Couldn't host").body(e));
                    return;
                }
            }
        }
    }
    world.insert_resource(mode);
    world.insert_resource(StageChoice(id));
    if !open {
        world.resource_mut::<Profile>().last_stage = id;
    }
    world.write_message(ScreenRequest::Replace(None));
    // From a stage straight into another: InGame → InGame runs OnExit/OnEnter again.
    world.resource_mut::<NextState<AppState>>().set(AppState::InGame);
}

/// Back to the start line of the current stage, clock reset, ghost from the start.
pub fn restart_stage(world: &mut World) {
    let Some(stage) = world.get_resource::<StageWorld>().cloned() else { return };
    let start = ghost::start_car(&stage.road);
    let mut q = world.query::<&mut DrivenCar>();
    for mut c in q.iter_mut(world) {
        c.car = start.clone();
        c.prev = (start.pos, start.rot);
    }
    // Free roam has no countdown (nothing to race).
    let countdown = if stage.free_roam() { 0.0 } else { 3.0 };
    world.insert_resource(StageRun { countdown, ..default() });
    world.resource_mut::<Recorder>().reset();
    world.resource_mut::<Finished>().0 = false;
    // Race the best ghost again (the run just finished may have replaced it).
    let ghosts: Vec<Entity> = world.query_filtered::<Entity, With<GhostCar>>().iter(world).collect();
    for e in ghosts {
        world.entity_mut(e).despawn();
    }
    if netlink::role(world) != Role::Guide
        && let Some(hash) = world.get_resource::<StageNow>().map(|s| s.hash.clone())
        && let Some(g) = pick_ghost(world, stage.route_id(), &hash)
    {
        spawn_ghost(world, g, &stage.road, &hash);
    }
    world.insert_resource(PauseAllowed(true));
    world.write_message(ScreenRequest::Replace(None));
}

/// The recover button from the pause menu: the same as the in-game key (latched for the next
/// fixed step, which puts the car back on the road; +5 s in an event; a broken car is
/// repaired too).
pub fn recover(world: &mut World) {
    let run = world.resource::<StageRun>().clone();
    if run.countdown > 0.0 || run.finished.is_some() || run.retired {
        return;
    }
    if let Some(mut r) = world.get_resource_mut::<crate::drive::RecoverPress>() {
        r.0 = true;
    }
    world.write_message(ScreenRequest::Replace(None));
}

/// A fatal crash in an event: once "You're dead" has shown, the event restarts from its line.
/// (Free roam puts the car back at home in the fixed step.)
fn restart_after_death(world: &mut World) {
    let Some(stage) = world.get_resource::<StageWorld>() else { return };
    if stage.free_roam() {
        return;
    }
    let dead = world.get_resource::<StageRun>().is_some_and(|r| matches!(r.down, Some(crate::drive::Down::Dead(t)) if t <= 0.0));
    if dead {
        info!("fatal crash: restarting the event");
        restart_stage(world);
    }
}

/// Removes the loaded stage (terrain, road, dressing, car, lights, cameras) so the menus
/// show and another stage can load. Entities with `DespawnOnExit(InGame)` go by themselves.
pub fn teardown(world: &mut World) {
    engine::world::despawn_world(world);
    let mut roots: Vec<Entity> = Vec::new();
    {
        let mut q = world.query_filtered::<Entity, (
            Without<ChildOf>,
            Or<(
                With<Mesh3d>,
                With<DirectionalLight>,
                With<PointLight>,
                With<SpotLight>,
                With<Camera3d>,
                With<DrivenCar>,
                With<crate::guide::seat::GuideCar>,
                With<engine::pcg::PcgVolume>,
                With<engine::pcg::PcgCellRoot>,
            )>,
        )>();
        roots.extend(q.iter(world));
    }
    for e in roots {
        if let Ok(mut em) = world.get_entity_mut(e) {
            // A level's foliage entity stays (the level owns it); only its volume goes.
            if em.contains::<engine::level::LevelEntity>() {
                em.remove::<(engine::pcg::PcgVolume, engine::pcg::PcgStreaming, crate::art::trees::TreeVolume, crate::art::DressingVolume)>();
            } else {
                em.despawn();
            }
        }
    }
    world.remove_resource::<StageWorld>();
    world.remove_resource::<StageNow>();
    world.insert_resource(StageRun::default());
    world.resource_mut::<Recorder>().reset();
    world.resource_mut::<Finished>().0 = false;
    world.insert_resource(PauseAllowed(true));
}

/// A stage just loaded: note its hash, pick and spawn the ghost.
fn stage_ready(world: &mut World) {
    let stage = world.resource::<StageWorld>();
    // Keyed by the route: the stage, or the open world's running event (its own records).
    if world.get_resource::<StageNow>().is_some_and(|s| s.id == stage.route_id()) {
        return;
    }
    let (mut def, road) = (stage.def.clone(), stage.road.clone());
    if let Some(e) = &stage.event {
        def.id = e.id;
        def.name = e.name.clone();
    }
    let road_hash = world
        .resource::<StageCatalog>()
        .info(def.id)
        .filter(|i| stages::with_conditions(i.def.clone(), def.weather, def.time) == def)
        .map(|i| i.hash.clone())
        .unwrap_or_else(|| stages::stage_hash(&def, &road));
    // Each weather / time of day has its own board and ghosts.
    let hash = stages::board_hash(&road_hash, &def, run_assists(world));
    world.insert_resource(StageNow { id: def.id, name: def.name.clone(), hash: hash.clone() });
    world.resource_mut::<Finished>().0 = false;
    if netlink::role(world) == Role::Guide {
        return;
    }
    let Some(g) = pick_ghost(world, def.id, &hash) else { return };
    spawn_ghost(world, g, &road, &hash);
}

/// The ghost to race on this stage, per the profile's choice.
pub fn pick_ghost(world: &World, stage: u32, hash: &str) -> Option<Ghost> {
    let p = world.resource::<Profile>();
    let mode = world.get_resource::<RunMode>().copied().unwrap_or_default();
    // Only ghosts driven in this weather and time of day.
    let cond = board::split_hash(hash).1;
    // …and with this car: a ghost replays its inputs through the sim, so one recorded with
    // other handling would drive somewhere else.
    let sim = ghost::sim_hash(&world.resource::<CarSpec>().0, HZ);
    let ghosts: Vec<Ghost> = list_ghosts(world, stage).into_iter().filter(|g| board::split_hash(&g.stage_hash).1 == cond && g.sim_hash == sim).collect();
    let solo_key = team_key(&p.driver_name, "");
    let team = team_key(&p.driver_name, &p.guide_name);
    let ours = |g: &&Ghost| if mode == RunMode::Team { g.team() == team } else { g.team() == solo_key };
    let rank = |a: &&Ghost, b: &&Ghost| (a.stage_hash != hash).cmp(&(b.stage_hash != hash)).then(a.total.total_cmp(&b.total));
    let pick = match p.ghost {
        0 => None,
        1 => ghosts.iter().filter(ours).min_by(rank),
        2 => ghosts.iter().min_by(rank),
        _ => p.rival(stage).and_then(|t| ghosts.iter().find(|g| g.team() == t)).or_else(|| ghosts.iter().filter(ours).min_by(rank)),
    };
    pick.cloned()
}

/// Every ghost saved for a stage.
pub fn list_ghosts(world: &World, stage: u32) -> Vec<Ghost> {
    let Some(dir) = world.get_resource::<DataDir>() else { return Vec::new() };
    let prefix = format!("s{stage:02}-");
    let Ok(rd) = std::fs::read_dir(dir.file(GHOST_DIR)) else { return Vec::new() };
    let mut out = Vec::new();
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with(&prefix) && name.ends_with(".json") {
            let g: Ghost = engine::paths::load_json(&e.path());
            if g.stage == stage && !g.inputs.is_empty() {
                out.push(g);
            }
        }
    }
    out
}

fn spawn_ghost(world: &mut World, g: Ghost, road: &crate::stage::Road, hash: &str) {
    let spec = world.resource::<CarSpec>().0.clone();
    let current = g.stage_hash == hash && g.sim_hash == ghost::sim_hash(&spec, HZ);
    let start = ghost::start_car(road);
    let replay = Replay::new(&g, start.clone());
    let opacity = world.resource::<Profile>().ghost_opacity.clamp(0.05, 1.0);
    info!("ghost: {} on stage {} ({} steps{})", board::team_label(&g.driver, &g.guide), g.stage, g.steps(), if current { "" } else { ", stale" });
    let label = board::team_label(&g.driver, &g.guide);
    let ghost = Arc::new(g);
    let tf = Transform::from_translation(start.pos).with_rotation(start.rot);
    let id = world
        .spawn((
            Name::new(format!("ghost: {label}")),
            GhostCar { ghost, replay, prev: (start.pos, start.rot), current },
            GhostBody,
            tf,
            Visibility::default(),
            DespawnOnExit(AppState::InGame),
        ))
        .id();
    let (Some(_), Some(_)) = (world.get_resource::<Assets<Mesh>>(), world.get_resource::<Assets<StandardMaterial>>()) else { return };
    let c = Color::srgba(0.45, 0.85, 1.0, opacity);
    let mat = world.resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial {
        base_color: c,
        emissive: LinearRgba::rgb(0.05, 0.12, 0.2),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });
    // The player's car model at its far level of detail (~2k triangles), placed like the
    // driven car's (`art::car::model_offset`), all in the ghost material.
    let Some(assets) = world.get_resource::<AssetServer>() else { return };
    let scene = assets.load(GltfAssetLabel::Scene(0).from_asset(crate::art::car::MODEL));
    let tf = Transform::from_translation(crate::art::car::model_offset(&spec));
    world.spawn((Name::new("ghost model"), GhostModel(mat), WorldAssetRoot(scene), tf, Visibility::default(), ChildOf(id)));
}

/// Once a ghost's model is in: keeps its far level of detail (and the parts drawn at every
/// distance) in the ghost material, drops the rest.
fn dress_ghosts(
    mut commands: Commands,
    models: Query<(Entity, &GhostModel)>,
    children: Query<&Children>,
    meshes: Query<Option<&bevy::gltf::GltfMeshName>, With<Mesh3d>>,
) {
    for (e, look) in &models {
        let mut found = false;
        for d in children.iter_descendants(e) {
            let Ok(name) = meshes.get(d) else { continue };
            found = true;
            match name.and_then(|n| crate::art::car::lod_of(n)) {
                Some(2) | None if name.is_some() => {
                    commands.entity(d).insert((MeshMaterial3d(look.0.clone()), bevy::light::NotShadowCaster));
                }
                _ => {
                    commands.entity(d).despawn();
                }
            }
        }
        if found {
            commands.entity(e).remove::<GhostModel>();
        }
    }
}

/// Keeps the ghost on the same fixed step as the driven car, drawn interpolated like it.
fn follow_ghost(
    fixed: Res<Time<Fixed>>,
    stage: Res<StageWorld>,
    spec: Res<CarSpec>,
    rec: Res<Recorder>,
    mut ghosts: Query<(&mut GhostCar, &mut Transform, &mut Visibility)>,
) {
    let dt = fixed.timestep().as_secs_f32();
    for (mut g, mut tf, mut vis) in &mut ghosts {
        let g = &mut *g;
        // A few steps per frame at most (a long hitch catches up over the next frames).
        let mut n = 0;
        while g.replay.step < rec.steps && n < 30 {
            g.prev = (g.replay.car.pos, g.replay.car.rot);
            let ghost = g.ghost.clone();
            g.replay.advance(&ghost, &spec.0, &*stage, dt);
            n += 1;
        }
        let a = fixed.overstep_fraction();
        tf.translation = g.prev.0.lerp(g.replay.car.pos, a);
        tf.rotation = g.prev.1.slerp(g.replay.car.rot, a);
        // Gone once it has finished and stopped (it only holds the handbrake after that).
        let want = if g.replay.done(&g.ghost) && g.replay.car.speed().abs() < 0.5 { Visibility::Hidden } else { Visibility::Inherited };
        if *vis != want {
            *vis = want;
        }
    }
}

fn watch_finish(world: &mut World) {
    let finished = world.get_resource::<StageRun>().is_some_and(|r| r.finished.is_some());
    if !finished || world.resource::<Finished>().0 || world.get_resource::<StageNow>().is_none() {
        return;
    }
    world.resource_mut::<Finished>().0 = true;
    if netlink::role(world) == Role::Guide {
        return;
    }
    finish_run(world);
    world.insert_resource(PauseAllowed(false));
    world.write_message(ScreenRequest::Replace(Some("results")));
}

/// Writes the finished run to the board (and its ghost when it's the team's best).
pub fn finish_run(world: &mut World) {
    let run = world.resource::<StageRun>().clone();
    let Some(total) = run.finished else { return };
    let now = world.resource::<StageNow>().clone();
    let p = world.resource::<Profile>().clone();
    let mode = world.get_resource::<RunMode>().copied().unwrap_or_default();
    let guide = match mode {
        RunMode::Practice => String::new(),
        RunMode::Team => netlink::peer_name(world).unwrap_or(p.guide_name.clone()),
    };
    let mut splits = run.splits.clone();
    splits.push(total);
    let rec = Record::new(now.id, &p.driver_name, &guide, total - run.penalty, run.penalty, splits, board::now(), VERSION, &now.hash);
    let team = rec.team();
    let (best_before, leader_before) = {
        let b = &world.resource::<Boards>().0;
        (b.best_of(now.id, &now.hash, &team).cloned(), b.ranking(now.id, &now.hash, rec.solo()).first().map(|r| (*r).clone()))
    };
    let inserted = world.resource_mut::<Boards>().0.insert(rec.clone());
    let mut ghost_saved = false;
    if let Some(dir) = world.get_resource::<DataDir>().cloned() {
        if let Err(e) = dir.save_json(BOARD_FILE, &world.resource::<Boards>().0) {
            warn!("leaderboard: {e}");
            crate::telemetry::handled(world, &e, "board.save");
        }
        if inserted.team_best {
            let spec = world.resource::<CarSpec>().0.clone();
            let g = world.resource::<Recorder>().ghost(now.id, &now.hash, &ghost::sim_hash(&spec, HZ), &rec.driver, &rec.guide, total, rec.date, VERSION, HZ);
            ghost_saved = match save_ghost(&dir, &g) {
                Ok(()) => true,
                Err(e) => {
                    warn!("ghost: {e}");
                    crate::telemetry::handled(world, &e, "ghost.save");
                    false
                }
            };
        }
    }
    info!("finish: stage {} {} {} (rank {}, team best {})", now.id, rec.team_label(), board::clock(total), inserted.rank, inserted.team_best);
    world.insert_resource(LastResult { record: rec, inserted, stage_name: now.name, best_before, leader_before, ghost_saved });
}

pub fn save_ghost(dir: &DataDir, g: &Ghost) -> std::io::Result<()> {
    std::fs::create_dir_all(dir.file(GHOST_DIR))?;
    dir.save_json(&format!("{GHOST_DIR}/{}", g.file_name()), g)
}

// ---------------------------------------------------------------- exchange

/// Writes the whole board and every ghost to `path` (default:
/// `<data>/boards/pacenote-board-<date>.json`). Returns the path.
pub fn export_board(world: &World, path: Option<std::path::PathBuf>) -> Result<std::path::PathBuf, String> {
    let dir = world.get_resource::<DataDir>().ok_or("no data dir")?;
    let path = path.unwrap_or_else(|| dir.file(&format!("{EXPORT_DIR}/pacenote-board-{}.json", board::date_text(board::now()))));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let records = world.resource::<Boards>().0.records.clone();
    let mut ghosts = Vec::new();
    for s in 1..=crate::stage::stages().len() as u32 {
        ghosts.extend(list_ghosts(world, s));
    }
    let f = BoardFile { exported: board::now(), records, ghosts, ..default() };
    engine::paths::write_atomic(&path, &f.to_json()).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Merges one exchange file: records (each team's best survives) and ghosts (kept when
/// faster than the one saved for that team). Returns records added or improved.
pub fn import_file(world: &mut World, path: &std::path::Path) -> Result<usize, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let f = BoardFile::from_json(&text)?;
    let n = world.resource_mut::<Boards>().0.merge(&f.board());
    let dir = world.get_resource::<DataDir>().cloned().ok_or("no data dir")?;
    dir.save_json(BOARD_FILE, &world.resource::<Boards>().0).map_err(|e| e.to_string())?;
    for g in f.ghosts {
        if g.inputs.is_empty() || !g.total.is_finite() {
            continue;
        }
        let existing: Ghost = engine::paths::load_json(&dir.file(&format!("{GHOST_DIR}/{}", g.file_name())));
        if existing.inputs.is_empty() || g.total < existing.total {
            save_ghost(&dir, &g).map_err(|e| e.to_string())?;
        }
    }
    Ok(n)
}

/// "Import": merges every `*.json` in `<data>/boards/inbox`, then moves them to
/// `inbox/merged/`. Returns (files, records changed).
pub fn import_inbox(world: &mut World) -> Result<(usize, usize), String> {
    let dir = world.get_resource::<DataDir>().cloned().ok_or("no data dir")?;
    let inbox = dir.file(INBOX_DIR);
    std::fs::create_dir_all(&inbox).map_err(|e| e.to_string())?;
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&inbox).map_err(|e| e.to_string())?.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
    files.sort();
    let (mut ok, mut changed) = (0, 0);
    let mut errors = Vec::new();
    for f in files {
        match import_file(world, &f) {
            Ok(n) => {
                ok += 1;
                changed += n;
                let done = inbox.join("merged");
                let _ = std::fs::create_dir_all(&done);
                let _ = std::fs::rename(&f, done.join(f.file_name().unwrap_or_default()));
            }
            Err(e) => errors.push(format!("{}: {e}", f.file_name().unwrap_or_default().to_string_lossy())),
        }
    }
    if ok == 0 && !errors.is_empty() {
        return Err(errors.join("; "));
    }
    Ok((ok, changed))
}

// ---------------------------------------------------------------- dev commands

#[cfg(feature = "dev")]
mod dev {
    use super::*;
    use engine::devtools::{AddControlCommand, params};
    use serde_json::{Value, json};

    pub fn register(app: &mut App) {
        app.add_control_command("meta.state", "Menus/leaderboard/ghost state", params(&[]), state)
            .add_control_command(
                "meta.profile",
                "Set profile fields",
                params(&[("driver", "string", "driver name"), ("guide", "string", "guide name"), ("address", "string", "join address"), ("ghost", "integer", "0 off, 1 our best, 2 fastest, 3 rival")]),
                set_profile,
            )
            .add_control_command("meta.start", "Load a stage", params(&[("stage", "integer", "1..=20"), ("mode", "string", "practice | team")]), start)
            .add_control_command("meta.finish", "Finish the current run now (tests, screenshots)", params(&[("time", "number", "clock seconds (default: current)")]), force_finish)
            .add_control_command("meta.retire", "Retire the car now (tests, screenshots of the RETIRED plate)", params(&[]), |w, _| {
                let mut run = w.get_resource_mut::<StageRun>().ok_or("no run")?;
                run.retired = true;
                Ok(json!({}))
            })
            .add_control_command("meta.restart", "Restart the current stage", params(&[]), |w, _| {
                restart_stage(w);
                Ok(json!({}))
            })
            .add_control_command("meta.recover", "Recover the car (+5 s)", params(&[]), |w, _| {
                recover(w);
                Ok(json!({}))
            })
            .add_control_command("board.export", "Export the leaderboard + ghosts", params(&[("path", "string", "file (default: data/boards/…)")]), |w, v| {
                let p = v.get("path").and_then(Value::as_str).map(std::path::PathBuf::from);
                export_board(w, p).map(|p| json!({"path": p}))
            })
            .add_control_command("board.import", "Merge an exchange file", params(&[("path", "string", "file")]), |w, v| {
                let p = v.get("path").and_then(Value::as_str).ok_or("path required")?;
                import_file(w, std::path::Path::new(p)).map(|n| json!({"changed": n}))
            })
            .add_control_command("ghost.verify", "Replay this run's recording and compare with the car", params(&[]), verify);
    }

    fn state(w: &mut World, _: Value) -> Result<Value, String> {
        let p = w.resource::<Profile>().clone();
        let screens = w.get_resource::<Screens>().map(|s| s.stack.clone()).unwrap_or_default();
        let ghost = {
            let mut q = w.query::<&GhostCar>();
            q.iter(w).next().map(|g| json!({"team": g.ghost.team(), "steps": g.ghost.steps(), "at": g.replay.step, "current": g.current}))
        };
        let last = w.get_resource::<LastResult>().map(|r| json!({"stage": r.record.stage, "total": r.record.total(), "rank": r.inserted.rank, "team_best": r.inserted.team_best, "stage_record": r.inserted.stage_record, "ghost_saved": r.ghost_saved}));
        Ok(json!({
            "screens": screens,
            "role": format!("{:?}", netlink::role(w)),
            "mode": format!("{:?}", w.resource::<RunMode>()),
            "driver": p.driver_name, "guide": p.guide_name, "address": p.join_address, "ghost_mode": p.ghost,
            "stage": w.get_resource::<StageNow>().map(|s| json!({"id": s.id, "hash": s.hash})),
            "records": w.resource::<Boards>().0.records.len(),
            "recorded_steps": w.resource::<Recorder>().steps,
            "ghost": ghost,
            "last": last,
            "catalog_ready": w.resource::<StageCatalog>().ready(),
        }))
    }

    fn set_profile(w: &mut World, v: Value) -> Result<Value, String> {
        let mut p = w.resource_mut::<Profile>();
        if let Some(s) = v.get("driver").and_then(Value::as_str) {
            p.driver_name = board::clean_name(s);
        }
        if let Some(s) = v.get("guide").and_then(Value::as_str) {
            p.guide_name = board::clean_name(s);
        }
        if let Some(s) = v.get("address").and_then(Value::as_str) {
            p.join_address = s.trim().into();
        }
        if let Some(g) = v.get("ghost").and_then(Value::as_u64) {
            p.ghost = g.min(3) as u32;
        }
        Ok(json!({"driver": p.driver_name, "guide": p.guide_name}))
    }

    fn start(w: &mut World, v: Value) -> Result<Value, String> {
        let id = v.get("stage").and_then(Value::as_u64).unwrap_or(1) as u32;
        let mode = if v.get("mode").and_then(Value::as_str) == Some("team") { RunMode::Team } else { RunMode::Practice };
        start_stage(w, id, mode);
        Ok(json!({"stage": id}))
    }

    fn force_finish(w: &mut World, v: Value) -> Result<Value, String> {
        let len = w.get_resource::<StageWorld>().ok_or("no stage")?.road.length();
        let mut run = w.resource_mut::<StageRun>();
        if let Some(t) = v.get("time").and_then(Value::as_f64) {
            run.time = t as f32;
        }
        let t = run.time.max(1.0);
        run.time = t;
        while run.splits.len() < 3 {
            let k = run.splits.len() as f32 + 1.0;
            let pen = run.penalty;
            run.splits.push(t * k / 4.0 + pen);
        }
        run.progress = len;
        run.countdown = 0.0;
        run.finished = Some(t + run.penalty);
        Ok(json!({"total": t + run.penalty}))
    }

    fn verify(w: &mut World, _: Value) -> Result<Value, String> {
        let stage = w.get_resource::<StageWorld>().cloned().ok_or("no stage")?;
        let spec = w.resource::<CarSpec>().0.clone();
        let g = w.resource::<Recorder>().ghost(stage.def.id, "", "", "", "", 0.0, 0, VERSION, HZ);
        let mut q = w.query::<&DrivenCar>();
        let car = q.iter(w).next().ok_or("no car")?.car.clone();
        let replayed = ghost::replay(&g, ghost::start_car(&stage.road), &spec, &stage, 1.0 / HZ as f32);
        Ok(json!({"steps": g.steps(), "runs": g.inputs.len(), "recovers": g.recovers.len(), "same": replayed == car, "pos": [car.pos.x, car.pos.y, car.pos.z], "replayed": [replayed.pos.x, replayed.pos.y, replayed.pos.z]}))
    }
}
