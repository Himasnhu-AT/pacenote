//! Pacenote: a two-person rally game on `engine` (see docs/DESIGN.md).

pub mod audio;
pub mod drive;
pub mod guide;
pub mod hud;
pub mod meta;
pub mod net_session;
pub mod stage;
pub mod vehicle;
pub mod voice;
pub mod world;

use bevy::prelude::*;
use engine::prelude::*;

pub fn info() -> GameInfo {
    GameInfo::new("pacenote", "Pacenote").tagline("One drives. One reads the road.")
}

/// Which stage to load (`--stage N`, 1..=20).
#[derive(Resource, Clone, Copy, Debug)]
pub struct StageChoice(pub u32);

pub struct Pacenote;

impl Plugin for Pacenote {
    fn build(&self, app: &mut App) {
        let n = engine::cli::arg_value("stage").and_then(|v| v.parse().ok()).unwrap_or(1u32).clamp(1, 20);
        app.insert_resource(StageChoice(n))
            .insert_resource(Time::<Fixed>::from_hz(60.0))
            .add_plugins((drive::DrivePlugin, hud::HudPlugin, guide::GuidePlugin))
            .add_plugins(audio::RallyAudioPlugin)
            .add_plugins(meta::MetaPlugin)
            .add_plugins(voice::VoicePlugin)
            .add_systems(OnEnter(engine::flow::AppState::InGame), load_stage);
    }
}

fn load_stage(world: &mut World) {
    // The guide builds its map from the driver's stage instead (guide::GuideStage).
    if world.get_resource::<net_session::Role>() == Some(&net_session::Role::Guide) {
        return;
    }
    let n = world.resource::<StageChoice>().0;
    let def = stage::stages().into_iter().find(|d| d.id == n).expect("stage");
    let t0 = std::time::Instant::now();
    let s = world::generate(&def);
    info!("stage {} \"{}\": {:.1} km, {} corners, generated in {:.2} s", def.id, def.name, s.road.length() / 1000.0, s.corners.len(), t0.elapsed().as_secs_f32());
    world::spawn(world, &s);
    // Light and sky: a plain sun for now (the look comes in milestone 4).
    world.spawn((
        DirectionalLight { illuminance: 25_000.0, shadow_maps_enabled: true, ..default() },
        Transform::default().looking_to(Vec3::new(-0.4, -0.8, -0.3), Vec3::Y),
        engine::gfx::Sun,
    ));
    world.spawn((MainCamera, Camera3d::default(), Transform::from_xyz(0.0, 50.0, 0.0)));
    drive::spawn_car(world);
}
