//! Pacenote: a two-person rally game on `engine` (see docs/DESIGN.md).

pub mod art;
pub mod audio;
#[cfg(feature = "dev")]
pub mod autopilot;
#[cfg(feature = "dev")]
pub mod capture;
pub mod codriver;
pub mod demo;
pub mod drive;
pub mod guide;
pub mod hardware;
pub mod hud;
pub mod loading;
pub mod meta;
pub mod net_session;
pub mod prefs;
pub mod stage;
pub mod telemetry;
pub mod vehicle;
pub mod voice;
pub mod world;

use bevy::prelude::*;
use engine::prelude::*;

pub fn info() -> GameInfo {
    // Our own assets/ wins over ./assets when run from another folder (the engine's scripts).
    // The telemetry envelope: app "pacenote", this crate's version, and the git sha only in
    // packaged builds (packaging/common.sh sets PACENOTE_BUILD_SHA; dev builds send none, so
    // the server files them as test data).
    let info = GameInfo::new("pacenote", if demo::ACTIVE { "Pacenote Demo" } else { "Pacenote" })
        .tagline("One drives. One reads the road.")
        .version(env!("CARGO_PKG_VERSION"))
        .build(option_env!("PACENOTE_BUILD_SHA"));
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
            .add_plugins(prefs::PrefsPlugin)
            .add_plugins(voice::VoicePlugin)
            .add_plugins(art::ArtPlugin)
            .add_plugins(loading::LoadingPlugin)
            .add_plugins(codriver::CoDriverPlugin)
            .add_plugins(telemetry::TelemetryPlugin)
            .add_systems(OnEnter(engine::flow::AppState::InGame), load_stage);
        fullscreen_on_first_run(app);
        app.add_plugins(hardware::HardwarePlugin);
        #[cfg(feature = "dev")]
        app.add_plugins(capture::CapturePlugin);
        #[cfg(feature = "dev")]
        app.add_plugins(autopilot::AutopilotPlugin);
        #[cfg(feature = "dev")]
        app.add_systems(Update, full_rate_unfocused);
    }
}

/// A fresh install starts in borderless fullscreen (the engine's default is a window). Only when
/// the settings file has never stored a window mode, so the player's choice in Settings > Video
/// sticks. The engine never applies the window mode in test-driven runs (play.py, control port).
fn fullscreen_on_first_run(app: &mut App) {
    use engine::settings::{CoreSettings, SettingsStore, WindowModeSetting};
    let Some(path) = app.world().get_resource::<SettingsStore>().map(|s| s.path.clone()) else { return };
    if !core_setting_saved(std::fs::read_to_string(path).ok().as_deref(), "window_mode")
        && let Some(mut core) = app.world_mut().get_resource_mut::<CoreSettings>()
    {
        core.window_mode = WindowModeSetting::Borderless;
    }
}

/// Does this settings file (its text, if any) hold the engine setting `key`?
pub(crate) fn core_setting_saved(file: Option<&str>, key: &str) -> bool {
    file.and_then(|t| serde_json::from_str::<serde_json::Value>(t).ok())
        .is_some_and(|v| v.get("core").and_then(|c| c.get(key)).is_some())
}

#[cfg(test)]
mod tests {
    use super::core_setting_saved;

    fn window_mode_saved(file: Option<&str>) -> bool {
        core_setting_saved(file, "window_mode")
    }

    #[test]
    fn fullscreen_only_without_a_saved_window_mode() {
        assert!(!window_mode_saved(None), "fresh install");
        assert!(!window_mode_saved(Some("not json")));
        assert!(!window_mode_saved(Some(r#"{"game": {}}"#)));
        assert!(!window_mode_saved(Some(r#"{"core": {"vsync": true}}"#)));
        assert!(window_mode_saved(Some(r#"{"core": {"window_mode": "Windowed"}}"#)), "the player's choice wins");
    }
}

/// Unfocused, the engine paces menus at 8 updates/s to save power. In dev builds the metrics
/// viewer (its own process) is usually what has focus, and it would then show "Waiting (vsync /
/// cap)" and a frozen-looking game: dev menus keep their focused rate.
#[cfg(feature = "dev")]
fn full_rate_unfocused(pacing: Option<ResMut<engine::render_budget::FramePacing>>) {
    let Some(mut pacing) = pacing else { return };
    if pacing.menus.unfocused_mode != pacing.menus.focused_mode {
        pacing.menus.unfocused_mode = pacing.menus.focused_mode;
    }
}

fn load_stage(world: &mut World) {
    // The guide builds its map from the driver's stage instead (guide::GuideStage).
    if world.get_resource::<net_session::Role>() == Some(&net_session::Role::Guide) {
        return;
    }
    let n = world.resource::<StageChoice>().0;
    // With the weather and time of day picked for it (meta::stage_def).
    let def = meta::stage_def(world, n).expect("stage");
    // Generation, terrain, road, dressing, trees, sky, camera and car, behind the loading screen.
    loading::begin(world, def);
}
