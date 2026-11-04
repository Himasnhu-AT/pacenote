//! Front end + competition layer, headless: the menu reaches a stage (and the next, and back),
//! a driven run replays exactly from its recording, finishing writes the board and the
//! ghost, and the ghost then races in lockstep with the same inputs.

use bevy::prelude::*;
use engine::devkit::testing::Scenario;
use engine::flow::AppState;
use engine::ui::prelude::MenuEvent;
use pacenote::drive::{DrivenCar, StageRun};
use pacenote::meta::{self, Boards, GhostCar, LastResult, StageNow, ghost};
use pacenote::world::StageWorld;

/// Headless Pacenote.
fn scenario(menu: bool) -> Scenario {
    let b = Scenario::new(pacenote::info());
    let b = if menu { b.menu() } else { b };
    b.plugin(pacenote::Pacenote).build()
}

fn state(w: &World) -> AppState {
    *w.resource::<State<AppState>>().get()
}

fn stage_id(w: &World) -> Option<u32> {
    w.get_resource::<StageWorld>().map(|s| s.def.id)
}

#[test]
fn menu_reaches_a_stage_then_the_next_then_the_menu() {
    let mut s = scenario(true);
    s.wait_until(30, |w| state(w) == AppState::MainMenu).expect("main menu");
    assert!(s.world().get_resource::<StageWorld>().is_none());

    // Stage select row 3 (practice).
    s.world_mut().write_message(MenuEvent("stage:3"));
    s.wait_until(600, |w| state(w) == AppState::InGame && stage_id(w) == Some(3)).expect("stage 3 loads");
    s.wait_until(30, |w| w.get_resource::<StageNow>().is_some_and(|n| n.id == 3)).expect("stage noted");
    assert_eq!(s.count::<With<DrivenCar>>(), 1);

    // Results → "Next stage": InGame → InGame tears stage 3 down and loads 4.
    s.world_mut().write_message(MenuEvent("next"));
    s.wait_until(600, |w| stage_id(w) == Some(4)).expect("stage 4 loads");
    s.step(5);
    assert_eq!(state(s.world()), AppState::InGame);
    assert_eq!(s.count::<With<DrivenCar>>(), 1, "the old car is gone");
    assert_eq!(s.count::<With<engine::world::WorldRoot>>(), 1, "one terrain");
    assert_eq!(s.world().resource::<StageNow>().id, 4);

    // "Menu": everything of the stage goes.
    s.world_mut().write_message(MenuEvent("menu"));
    s.wait_until(60, |w| state(w) == AppState::MainMenu).expect("back to the menu");
    s.step(3);
    assert!(s.world().get_resource::<StageWorld>().is_none());
    assert_eq!(s.count::<With<DrivenCar>>(), 0);
    assert_eq!(s.count::<With<Mesh3d>>(), 0, "no stage meshes left");
}

/// Drives with a fixed key schedule (relative to the start of the run).
fn drive(s: &mut Scenario, recover_at: Option<u32>) {
    let plan: &[(u32, KeyCode, bool)] = &[
        (200, KeyCode::KeyW, true),
        (330, KeyCode::KeyD, true),
        (390, KeyCode::KeyD, false),
        (420, KeyCode::KeyA, true),
        (450, KeyCode::Space, true),
        (470, KeyCode::Space, false),
        (500, KeyCode::KeyA, false),
        (560, KeyCode::KeyW, false),
        (570, KeyCode::KeyS, true),
        (620, KeyCode::KeyS, false),
    ];
    // Keyed on the recorded step count, so a retry replays the same inputs on the same steps.
    let mut fired = vec![false; plan.len()];
    let mut recovered = false;
    loop {
        let f = s.world().resource::<ghost::Recorder>().steps;
        if f >= 660 {
            break;
        }
        for (k, (at, key, down)) in plan.iter().enumerate() {
            if !fired[k] && f >= *at {
                fired[k] = true;
                if *down { s.press(*key) } else { s.release(*key) }
            }
        }
        if !recovered && recover_at.is_some_and(|r| f >= r) {
            recovered = true;
            meta::recover(s.world_mut());
        }
        s.update();
    }
    for key in [KeyCode::KeyW, KeyCode::KeyA, KeyCode::KeyS, KeyCode::KeyD, KeyCode::Space] {
        s.release(key);
    }
}

#[test]
fn a_run_replays_exactly_and_its_ghost_races_in_lockstep() {
    let mut s = scenario(false);
    s.wait_until(600, |w| w.get_resource::<StageNow>().is_some()).expect("stage 1 loads");
    let road = s.world().resource::<StageWorld>().road.clone();
    let start = ghost::start_car(&road);
    {
        let car = &s.query::<&DrivenCar, _>(|mut q| q.next().unwrap().car.clone());
        assert!(car.pos.distance(start.pos) < 0.5, "start_car matches drive::spawn_car");
    }
    drive(&mut s, Some(520));
    let steps = s.world().resource::<ghost::Recorder>().steps;
    assert!(steps > 600, "recorded every fixed step ({steps})");
    let car = s.query::<&DrivenCar, _>(|mut q| q.next().unwrap().car.clone());
    assert!(car.pos.distance(start.pos) > 15.0, "the car drove ({:.1} m)", car.pos.distance(start.pos));
    assert_eq!(s.world().resource::<StageRun>().penalty, 5.0, "recover cost 5 s");

    // The recording replays to exactly the same car.
    let spec = s.world().resource::<pacenote::drive::CarSpec>().0.clone();
    let rec = s.world().resource::<ghost::Recorder>().clone();
    assert_eq!(rec.recovers.len(), 1);
    let g = rec.ghost(1, "h", &ghost::sim_hash(&spec, meta::HZ), "A", "", 1.0, 0, "t", meta::HZ);
    let stage = s.world().resource::<StageWorld>().clone();
    let replayed = ghost::replay(&g, start.clone(), &spec, &stage, 1.0 / meta::HZ as f32);
    assert_eq!(replayed, car, "ghost replay is bit-exact");

    // Finish: the board gets the run, the ghost is saved, results open.
    {
        let len = stage.road.length();
        let mut run = s.world_mut().resource_mut::<StageRun>();
        run.progress = len;
        let t = run.time + run.penalty;
        run.finished = Some(t);
    }
    s.wait_until(10, |w| w.get_resource::<LastResult>().is_some()).expect("results");
    let res = s.world().resource::<LastResult>().clone();
    assert!(res.inserted.team_best && res.ghost_saved);
    assert!(res.record.solo(), "practice runs are solo");
    assert_eq!(s.world().resource::<Boards>().0.records.len(), 1);
    assert!(s.data_dir().join(meta::BOARD_FILE).is_file());
    assert_eq!(meta::list_ghosts(s.world(), 1).len(), 1);

    // Retry: the saved ghost spawns and, driven the same way, stays exactly on the car.
    s.world_mut().write_message(MenuEvent("retry"));
    s.step(1);
    assert_eq!(s.count::<With<GhostCar>>(), 1, "the ghost races the retry");
    drive(&mut s, Some(520));
    let car = s.query::<&DrivenCar, _>(|mut q| q.next().unwrap().car.clone());
    let (gstep, gcar) = s.query::<&GhostCar, _>(|mut q| {
        let g = q.next().unwrap();
        (g.replay.step, g.replay.car.clone())
    });
    assert_eq!(gstep, s.world().resource::<ghost::Recorder>().steps, "ghost on the same fixed step");
    assert_eq!(gcar, car, "same inputs → the ghost is exactly where the car is");
}

#[test]
fn camera_view_follows_the_saved_choice_and_c_is_remembered() {
    use pacenote::drive::CameraView;
    use pacenote::meta::Profile;
    let mut s = scenario(true);
    s.wait_until(30, |w| state(w) == AppState::MainMenu).expect("main menu");
    // The saved choice (Settings / stage picker) sets the view...
    s.world_mut().resource_mut::<Profile>().camera = 2;
    s.step(2);
    assert_eq!(*s.world().resource::<CameraView>(), CameraView::Chase);
    // ...and a run starts in it.
    s.world_mut().write_message(MenuEvent("stage:3"));
    s.wait_until(600, |w| stage_id(w) == Some(3)).expect("stage 3 loads");
    s.step(3);
    assert_eq!(*s.world().resource::<CameraView>(), CameraView::Chase, "a run starts in the chosen view");
    // C in game (the view changes) is remembered.
    *s.world_mut().resource_mut::<CameraView>() = CameraView::Bonnet;
    s.step(2);
    assert_eq!(s.world().resource::<Profile>().camera, 1);
}

#[test]
fn the_pointer_is_free_once_the_run_is_over() {
    use engine::input::CaptureCursor;
    let mut s = scenario(false);
    s.wait_until(600, |w| w.get_resource::<StageWorld>().is_some()).expect("stage");
    s.step(2);
    assert!(s.world().resource::<CaptureCursor>().0, "driving: the game lets the engine capture");
    s.world_mut().resource_mut::<StageRun>().retired = true;
    s.step(2);
    assert!(!s.world().resource::<CaptureCursor>().0, "retired: pointer free");
    s.world_mut().resource_mut::<StageRun>().retired = false;
    s.step(2);
    assert!(s.world().resource::<CaptureCursor>().0);
}

#[test]
fn assists_apply_before_a_run_never_during_it() {
    use pacenote::drive::{CarSpec, SteerSettings};
    use pacenote::prefs::Prefs;
    let mut s = scenario(true);
    s.wait_until(30, |w| state(w) == AppState::MainMenu).expect("main menu");
    {
        let mut p = s.world_mut().resource_mut::<Prefs>();
        p.abs = false;
        p.steer_speed = 1.4;
    }
    s.step(2);
    assert!(!s.world().resource::<CarSpec>().0.abs, "picked in the menu: the next run has it");
    assert_eq!(s.world().resource::<SteerSettings>().keyboard_speed, 1.4);
    s.world_mut().write_message(MenuEvent("stage:3"));
    s.wait_until(600, |w| w.get_resource::<StageNow>().is_some_and(|n| n.id == 3)).expect("stage 3");
    assert!(s.world().resource::<StageNow>().hash.ends_with("@noabs"), "its own board: {}", s.world().resource::<StageNow>().hash);
    s.wait_until(600, |w| w.resource::<StageRun>().time > 0.5).expect("clock running");
    s.world_mut().resource_mut::<Prefs>().abs = true;
    s.step(5);
    assert!(!s.world().resource::<CarSpec>().0.abs, "never changes mid-run");
}

// ---------------------------------------------------------------- festival menus (stream E)

use engine::ui::prelude::{ScreenRequest, Screens};
use pacenote::career::CareerRequest;
use pacenote::career::garage::UpgradeKind;
use pacenote::meta::{PendingStart, Profile};

fn top(w: &World) -> Option<&'static str> {
    w.resource::<Screens>().top()
}

/// Every screen of the Service Park is reachable from the menu and builds (headless runs
/// don't draw screens, so each builder is also called by hand).
#[test]
fn every_service_park_screen_is_reachable_and_builds() {
    let mut s = scenario(true);
    s.wait_until(30, |w| state(w) == AppState::MainMenu).expect("main menu");
    let reg = s.world().resource::<engine::ui::screens::ScreenRegistry>().clone();
    for id in ["main", "events", "challenges", "logbook", "garage", "upgrades", "dealer", "settings", "together", "credits", "boards"] {
        if id != "main" {
            s.world_mut().write_message(ScreenRequest::Push(id));
            s.step(2);
            assert_eq!(top(s.world()), Some(id), "{id} opens");
        }
        let build = reg.get(id).unwrap_or_else(|| panic!("{id} registered"));
        let root = s.world_mut().spawn(Node::default()).id();
        build(s.world_mut(), root);
        s.world_mut().flush();
        assert!(s.world().get::<Children>(root).is_some_and(|c| !c.is_empty()), "{id} built something");
        s.world_mut().entity_mut(root).despawn();
        if id != "main" {
            s.world_mut().write_message(ScreenRequest::Pop);
            s.step(2);
            assert_eq!(top(s.world()), Some("main"), "back from {id}");
        }
    }
    // Quit asks first; Esc stays.
    s.world_mut().write_message(MenuEvent("quit_ask"));
    s.step(2);
    assert_eq!(top(s.world()), Some("confirm"));
    let build = reg.get("confirm").expect("confirm registered");
    let root = s.world_mut().spawn(Node::default()).id();
    build(s.world_mut(), root);
    s.world_mut().flush();
    s.world_mut().entity_mut(root).despawn();
    s.world_mut().write_message(ScreenRequest::Pop);
    s.step(2);
    assert_eq!(top(s.world()), Some("main"));
    assert_eq!(state(s.world()), AppState::MainMenu, "still running");
}

fn requests(s: &mut Scenario) -> Vec<CareerRequest> {
    s.world_mut().resource_mut::<Messages<CareerRequest>>().drain().collect()
}

/// Upgrades: a level asks first and then sends the request (F applies it); with too few
/// Notes the dialog says how many are missing and nothing is sent.
#[test]
fn buying_a_part_asks_first_and_too_few_notes_is_refused() {
    let mut s = scenario(true);
    s.wait_until(30, |w| state(w) == AppState::MainMenu).expect("main menu");
    s.world_mut().write_message(ScreenRequest::Push("upgrades"));
    s.step(2);
    // Brakes, Street: 1,500 N of the 12,000 a fresh career has.
    s.world_mut().resource_mut::<pacenote::meta::garage_ui::CarSel>().preview[3] = Some(1);
    s.world_mut().write_message(MenuEvent("part:3"));
    s.step(2);
    assert_eq!(top(s.world()), Some("confirm"));
    let c = s.world().resource::<pacenote::meta::confirm::Confirm>().clone();
    assert!(c.refused.is_none() && c.title.contains("Street brakes"), "{c:?}");
    requests(&mut s);
    s.world_mut().write_message(MenuEvent("confirm_yes"));
    s.step(1);
    assert_eq!(requests(&mut s), vec![CareerRequest::FitPart { kind: UpgradeKind::Brakes, level: 1 }]);
    s.step(2);
    assert_eq!(top(s.world()), Some("upgrades"), "the dialog closed");

    // Engine Works with 100 N: refused, with the shortfall.
    s.world_mut().resource_mut::<pacenote::career::wallet::Wallet>().balance = 100;
    s.world_mut().resource_mut::<pacenote::meta::garage_ui::CarSel>().preview[0] = Some(3);
    s.world_mut().write_message(MenuEvent("part:0"));
    s.step(2);
    let c = s.world().resource::<pacenote::meta::confirm::Confirm>().clone();
    assert_eq!(c.refused.as_deref(), Some("Need 23,900 N more"), "3,000 + 7,000 + 14,000 − 100");
    s.world_mut().write_message(MenuEvent("confirm_yes"));
    s.step(1);
    assert!(requests(&mut s).is_empty(), "nothing bought");
}

/// Continue starts the open world where the car was parked; Go now starts an event on its
/// start line.
#[test]
fn continue_restores_the_parked_pose_and_go_now_starts_on_the_line() {
    let mut s = scenario(true);
    s.wait_until(30, |w| state(w) == AppState::MainMenu).expect("main menu");
    // First drive: from the service park (no pose yet).
    s.world_mut().write_message(MenuEvent("continue"));
    s.wait_until(3000, |w| w.get_resource::<StageWorld>().is_some_and(|s| s.open.is_some())).expect("the open world loads");
    s.wait_until(60, |w| !w.resource::<Profile>().events.is_empty()).expect("events noted");
    let home = s.query::<&DrivenCar, _>(|mut q| q.next().map(|c| c.car.pos)).expect("a car");
    // Park somewhere else: 60 m east of home, facing north.
    let spot = [home.x + 60.0, home.y, home.z, 0.0];
    s.world_mut().write_message(MenuEvent("menu"));
    s.wait_until(60, |w| state(w) == AppState::MainMenu).expect("menu");
    s.world_mut().resource_mut::<Profile>().last_pose = Some(spot);
    s.world_mut().write_message(MenuEvent("continue"));
    s.wait_until(3000, |w| w.get_resource::<StageWorld>().is_some_and(|s| s.open.is_some()) && !w.contains_resource::<PendingStart>()).expect("loads and parks");
    s.step(2);
    let pos = s.query::<&DrivenCar, _>(|mut q| q.next().map(|c| c.car.pos)).expect("a car");
    assert!(Vec2::new(pos.x - spot[0], pos.z - spot[2]).length() < 1.5, "parked where it was left: {pos} vs {spot:?}");

    // Go now: the event's own start line, the event running.
    let ev = s.world().resource::<Profile>().events[0].clone();
    s.world_mut().write_message(MenuEvent("menu"));
    s.wait_until(60, |w| state(w) == AppState::MainMenu).expect("menu");
    s.world_mut().resource_mut::<pacenote::meta::screens::EventPick>().id = Some(ev.id);
    s.world_mut().write_message(MenuEvent("go_now"));
    s.wait_until(3000, |w| w.get_resource::<StageWorld>().is_some_and(|s| s.event.as_ref().is_some_and(|e| e.id == ev.id))).expect("the event starts");
    s.step(2);
    let start = s.world().resource::<StageWorld>().road.start().0;
    let pos = s.query::<&DrivenCar, _>(|mut q| q.next().map(|c| c.car.pos)).expect("a car");
    assert!(Vec2::new(pos.x - start.x, pos.z - start.z).length() < 3.0, "on the start line: {pos} vs {start}");
    assert!(s.world().resource::<StageRun>().countdown > 0.0, "countdown before the start");
}
