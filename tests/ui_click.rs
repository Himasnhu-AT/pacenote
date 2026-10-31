//! Mouse clicks on the pause menu, with Bevy's real UI layout and focus system: the full game
//! (renderer, render scale, HUD, stage) but no winit, so no window is ever shown. The cursor
//! is placed on a button through the `Window` component and the left button pressed, exactly
//! what winit does for a real click. Silent (no audio device, co-driver off). Needs a GPU, so
//! it is ignored by default: `cargo test --test ui_click -- --ignored`.

use bevy::input::ButtonState;
use bevy::input::mouse::MouseButtonInput;
use bevy::prelude::*;
use bevy::window::{PrimaryWindow, WindowResolution};
use engine::flow::AppState;
use engine::prelude::*;
use engine::ui::menus::MenuAction;
use engine::ui::prelude::ScreenRequest;
use engine::ui::widgets::{ActionButton, UiAction};

fn app() -> App {
    let dir = std::env::temp_dir().join(format!("pacenote-ui-click-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let info = pacenote::info();
    let assets = engine::paths::assets_dir(&info);
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .build()
            .disable::<bevy::winit::WinitPlugin>()
            // Silent: no audio device, and the co-driver's OS voice off (below).
            .disable::<bevy::audio::AudioPlugin>()
            .set(WindowPlugin {
                primary_window: Some(Window { resolution: WindowResolution::new(1700, 1000).with_scale_factor_override(2.0), ..default() }),
                exit_condition: bevy::window::ExitCondition::DontExit,
                ..default()
            })
            .set(AssetPlugin { file_path: assets.to_string_lossy().into_owned(), ..default() }),
    );
    let options = EngineOptions { headless: false, skip_menu: true, data_dir: Some(dir), assets_dir: Some(assets), ui: true, devtools: false, trace: false, worker_threads: None };
    app.add_plugins(EnginePlugin { info, options });
    app.add_plugins(pacenote::Pacenote);
    app.finish();
    app.cleanup();
    app.world_mut().resource_mut::<pacenote::codriver::CoDriver>().enabled = false;
    app
}

fn state(app: &App) -> AppState {
    *app.world().resource::<State<AppState>>().get()
}

/// Centre (physical px) of the button that does `action`.
fn button_centre(app: &mut App, action: UiAction) -> Option<Vec2> {
    let w = app.world_mut();
    let mut q = w.query::<(&ActionButton, &bevy::ui::UiGlobalTransform)>();
    q.iter(w).find(|(b, _)| b.0 == action).map(|(_, t)| t.translation)
}

fn click_at(app: &mut App, at: Vec2) {
    let w = app.world_mut();
    let window = w.query_filtered::<Entity, With<PrimaryWindow>>().single(w).unwrap();
    w.entity_mut(window).get_mut::<Window>().unwrap().set_physical_cursor_position(Some(at.as_dvec2()));
    app.update();
    let w = app.world_mut();
    w.write_message(MouseButtonInput { button: MouseButton::Left, state: ButtonState::Pressed, window });
    app.update();
    let w = app.world_mut();
    w.write_message(MouseButtonInput { button: MouseButton::Left, state: ButtonState::Released, window });
    for _ in 0..4 {
        app.update();
    }
}

fn dump(app: &mut App) {
    let w = app.world_mut();
    let mut cams = w.query::<(Entity, &Camera, &bevy::camera::RenderTarget, Option<&Name>, Has<bevy::ui::IsDefaultUiCamera>)>();
    for (e, c, t, n, ui) in cams.iter(w) {
        eprintln!("camera {e} {:?} order {} active {} target {:?} ui {ui} size {:?}", n.map(|n| n.as_str()), c.order, c.is_active, t, c.physical_target_size());
    }
    let mut q = w.query::<(&ActionButton, &Interaction, &bevy::ui::UiGlobalTransform, &ComputedNode, &bevy::ui::ComputedUiTargetCamera)>();
    for (b, i, t, n, cam) in q.iter(w) {
        eprintln!("button {:?} {:?} at {:?} size {:?} cam {:?}", b.0, i, t.translation, n.size(), cam.get());
    }
}

/// Loads the first stage through the loading screen and lets it settle.
fn load(app: &mut App) {
    for f in 0..3000 {
        app.update();
        if f > 30 && state(app) == AppState::InGame && app.world().contains_resource::<pacenote::world::StageWorld>() && !pacenote::loading::is_loading(app.world()) {
            break;
        }
    }
    assert!(!pacenote::loading::is_loading(app.world()), "stage loaded");
    for _ in 0..30 {
        app.update();
    }
}

/// The primary window's cursor: (captured, visible).
fn cursor(app: &mut App) -> (bool, bool) {
    let w = app.world_mut();
    let c = w.query_filtered::<&bevy::window::CursorOptions, With<PrimaryWindow>>().single(w).unwrap();
    (c.grab_mode != bevy::window::CursorGrabMode::None, c.visible)
}

// These runs are not test-driven (no `--control`), so the engine's real cursor policy runs:
// the cursor is captured while driving, exactly as in a shipped build. (Without winit nothing
// happens to the OS cursor, so the clicks below go through either way: the cursor asserts are
// what catch a locked, hidden pointer.)

#[test]
#[ignore = "needs a GPU; run with --ignored"]
fn pause_menu_quit_to_menu_with_the_mouse() {
    let mut app = app();
    load(&mut app);
    assert_eq!(cursor(&mut app), (true, false), "driving: captured and hidden");
    app.world_mut().write_message(ScreenRequest::Push("pause"));
    for _ in 0..5 {
        app.update();
    }
    assert_eq!(cursor(&mut app), (false, true), "pause menu: pointer free and shown");
    dump(&mut app);
    let at = button_centre(&mut app, UiAction::Menu(MenuAction::MainMenu)).expect("Quit to menu button");
    click_at(&mut app, at);
    for _ in 0..10 {
        app.update();
    }
    assert_eq!(state(&app), AppState::MainMenu, "clicking Quit to menu leaves the stage");
}

#[test]
#[ignore = "needs a GPU; run with --ignored"]
fn results_screen_frees_the_pointer_and_menu_works_with_the_mouse() {
    let mut app = app();
    load(&mut app);
    assert_eq!(cursor(&mut app), (true, false), "driving: captured");
    // Cross the line.
    {
        let mut run = app.world_mut().resource_mut::<pacenote::drive::StageRun>();
        run.countdown = 0.0;
        run.time = 95.0;
        run.finished = Some(95.0);
    }
    for _ in 0..8 {
        app.update();
    }
    assert_eq!(app.world().resource::<engine::ui::prelude::Screens>().top(), Some("results"));
    assert_eq!(cursor(&mut app), (false, true), "results: pointer free and shown (the owner's \"arrow never comes\")");
    let at = button_centre(&mut app, UiAction::Menu(MenuAction::Custom("menu"))).expect("Menu button");
    click_at(&mut app, at);
    for _ in 0..10 {
        app.update();
    }
    assert_eq!(state(&app), AppState::MainMenu, "clicking Menu on the results leaves the stage");
}
