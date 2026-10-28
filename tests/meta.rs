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
