//! Pacenote: a two-person rally game on `engine` (see docs/DESIGN.md).

pub mod art;
pub mod audio;
pub mod codriver;
pub mod demo;
pub mod drive;
pub mod guide;
pub mod hud;
pub mod loading;
pub mod meta;
pub mod net_session;
pub mod stage;
pub mod telemetry;
pub mod vehicle;
pub mod voice;
pub mod world;

use bevy::prelude::*;
use engine::prelude::*;

pub fn info() -> GameInfo {
    // Our own assets/ wins over ./assets when run from another folder (the engine's scripts).
    let info = GameInfo::new("pacenote", if demo::ACTIVE { "Pacenote Demo" } else { "Pacenote" }).tagline("One drives. One reads the road.");
    // Dev builds only: shipped builds must not carry a path into the source tree.
    #[cfg(feature = "dev")]
    let info = info.assets_fallback(concat!(env!("CARGO_MANIFEST_DIR"), "/assets"));
    info
}

/// Which stage to load (`--stage N`, 1..=20).
#[derive(Resource, Clone, Copy, Debug)]
pub struct StageChoice(pub u32);

pub struct Pacenote;

impl Plugin for Pacenote {
    fn build(&self, app: &mut App) {
        let n = demo::clamp_stage(engine::cli::arg_value("stage").and_then(|v| v.parse().ok()).unwrap_or(1u32).clamp(1, 20));
        app.insert_resource(StageChoice(n))
            .insert_resource(Time::<Fixed>::from_hz(60.0))
            .add_plugins((drive::DrivePlugin, hud::HudPlugin, guide::GuidePlugin))
            .add_plugins(audio::RallyAudioPlugin)
            .add_plugins(meta::MetaPlugin)
            .add_plugins(voice::VoicePlugin)
            .add_plugins(art::ArtPlugin)
            .add_plugins(loading::LoadingPlugin)
            .add_plugins(codriver::CoDriverPlugin)
            .add_plugins(telemetry::TelemetryPlugin)
            .add_systems(OnEnter(engine::flow::AppState::InGame), load_stage);
        #[cfg(feature = "dev")]
        app.add_systems(Update, keep_game_focused);
    }
}

/// The engine's dev metrics window opens at start beside the game; on a laptop screen it lands
/// on top and macOS gives it focus, and the unfocused game drops to 8 updates/s. Whenever any
/// other window of ours gains focus, hand it straight back to the game window (the metrics
/// window still scrolls under the mouse without focus). F4 still toggles it.
#[cfg(feature = "dev")]
fn keep_game_focused(
    mut focus: MessageReader<bevy::window::WindowFocused>,
    mut primary: Query<(Entity, &mut Window), With<bevy::window::PrimaryWindow>>,
) {
    let Ok((game, mut window)) = primary.single_mut() else {
        focus.clear();
        return;
    };
    if focus.read().last().is_some_and(|e| e.focused && e.window != game) {
        // Bevy synced `focused = false` from the event; setting it back calls focus_window.
        window.focused = true;
    }
}

fn load_stage(world: &mut World) {
    // The guide builds its map from the driver's stage instead (guide::GuideStage).
    if world.get_resource::<net_session::Role>() == Some(&net_session::Role::Guide) {
        return;
    }
    let n = world.resource::<StageChoice>().0;
    let def = stage::stages().into_iter().find(|d| d.id == n).expect("stage");
    // Generation, terrain, road, dressing, trees, sky, camera and car, behind the loading screen.
    loading::begin(world, def);
}
