//! Every Pacenote screen on the engine UI kit: main menu (with the stage contour map behind
//! it and a crew card), stage select, join, team names, leaderboards, results, credits, the
//! pause menu header, and the game's settings rows.
//!
//! Buttons send `MenuEvent`s with the ids below; [`on_menu`] is the one place they're
//! handled.

use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use engine::flow::AppState;
use engine::settings::{FieldDef, FieldKind, RegisterSettings, format_percent};
use engine::ui::prelude::*;
use engine::ui::widgets::{ActionButton, Focus, Focusable, KeepColor, LookBg, ScrollList, nav_look};

use super::board::{self, Record};
use super::stages::StageCatalog;
use super::textfield::{self, TextKey, text_field};
use super::{Boards, LastResult, Profile, RunMode, StageNow, netlink};
use crate::drive::StageRun;
use crate::stage::{Biome, Surface, TimeOfDay, Weather};

/// Menu ids of the 20 stage rows.
const STAGE_IDS: [&str; 20] = [
    "stage:1", "stage:2", "stage:3", "stage:4", "stage:5", "stage:6", "stage:7", "stage:8", "stage:9", "stage:10", "stage:11", "stage:12", "stage:13", "stage:14", "stage:15",
    "stage:16", "stage:17", "stage:18", "stage:19", "stage:20",
];
/// Menu ids of the leaderboard rows.
const ROW_IDS: [&str; 12] = ["row:0", "row:1", "row:2", "row:3", "row:4", "row:5", "row:6", "row:7", "row:8", "row:9", "row:10", "row:11"];

/// Pacenote's look: night-forest greens, map-paper text, rally-plate yellow.
pub fn theme() -> Theme {
    Theme {
        bg: Color::srgb(0.043, 0.063, 0.055),
        panel: Color::srgba(0.07, 0.095, 0.085, 0.92),
        raised: Color::srgba(0.13, 0.16, 0.145, 0.96),
        line: Color::srgba(0.95, 0.93, 0.88, 0.10),
        line_strong: Color::srgba(0.95, 0.93, 0.88, 0.22),
        text: Color::srgb(0.95, 0.93, 0.88),
        text_muted: Color::srgb(0.66, 0.68, 0.62),
        text_dim: Color::srgb(0.44, 0.47, 0.43),
        accent: Color::srgb(1.0, 0.77, 0.12),
        on_accent: Color::srgb(0.07, 0.06, 0.03),
        good: Color::srgb(0.42, 0.88, 0.55),
        bad: Color::srgb(1.0, 0.4, 0.32),
        info: Color::srgb(0.52, 0.8, 1.0),
    }
}

/// What the stage list starts (practice or a hosted team run) and which stage it shows.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct StageSel {
    pub mode: RunMode,
    shown: Option<u32>,
}

/// The stage the leaderboard screen shows.
#[derive(Resource, Clone, Copy, Debug)]
pub struct BoardSel(pub u32);

/// Last message from joining (shown on the join screen).
#[derive(Resource, Clone, Debug, Default)]
pub struct NetNote(pub Option<String>);

#[derive(Component, Clone, Copy)]
struct StageRow(u32);

#[derive(Component)]
struct StageDetails;

#[derive(Component)]
struct JoinStatus;

/// Text that pulses between two colours (the results celebration).
#[derive(Component, Clone, Copy)]
struct Pulse(Color, Color);

pub struct ScreensPlugin;

impl Plugin for ScreensPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(theme())
            .init_resource::<StageSel>()
            .insert_resource(BoardSel(1))
            .init_resource::<NetNote>()
            .insert_resource(MenuConfig {
                eyebrow: "Two-person rally",
                main_items: vec![
                    ("Practice", MenuAction::Custom("practice")),
                    ("Host as driver", MenuAction::Custom("host")),
                    ("Join as guide", MenuAction::Open("join")),
                    ("Stage select", MenuAction::Custom("select")),
                    ("Leaderboards", MenuAction::Custom("boards")),
                    ("Team names", MenuAction::Open("team")),
                    ("Settings", MenuAction::Settings),
                    ("Credits", MenuAction::Open("credits")),
                    ("Quit", MenuAction::Quit),
                ],
                pause_items: vec![
                    ("Resume", MenuAction::Resume),
                    ("Restart stage", MenuAction::Custom("restart")),
                    ("Recover car (+5 s)", MenuAction::Custom("recover")),
                    ("Settings", MenuAction::Settings),
                    ("Quit to menu", MenuAction::MainMenu),
                ],
                pause_note: "Restarting or quitting drops this run.",
                main_panel: Some(crew_card),
                pause_header: Some(pause_header),
                pause_eyebrow: Some(pause_eyebrow),
            })
            .register_screen("main", build_main)
            .register_screen("stages", build_stages)
            .register_screen("join", build_join)
            .register_screen("team", build_team)
            .register_screen("boards", build_boards)
            .register_screen("credits", build_credits)
            .register_sticky_screen("results", build_results)
            .settings_defaults::<Profile>()
            .add_systems(Update, (stage_details, join_status, pulse).after(engine::ui::UiSet::Paint));
        register_settings(app);
    }
}

fn register_settings(app: &mut App) {
    fn p(w: &World) -> &Profile {
        w.resource::<Profile>()
    }
    app.register_setting(FieldDef {
        key: "ghost",
        tab: "Game",
        label: "Ghost car",
        hint: "Race a recorded run: your own best, the fastest ghost you have, or the rival picked on the leaderboard.",
        kind: FieldKind::Choice { options: vec!["Off", "Our best", "Fastest", "Rival"] },
        get: |w| p(w).ghost as f32,
        set: |w, v| w.resource_mut::<Profile>().ghost = (v.round().max(0.0) as u32).min(3),
    })
    .register_setting(FieldDef {
        key: "ghost_opacity",
        tab: "Game",
        label: "Ghost opacity",
        hint: "Applies from the next stage.",
        kind: FieldKind::Slider { min: 0.1, max: 1.0, step: 0.05, format: format_percent },
        get: |w| p(w).ghost_opacity,
        set: |w, v| w.resource_mut::<Profile>().ghost_opacity = v.clamp(0.1, 1.0),
    });
}

// ---------------------------------------------------------------- events

/// Handles one menu event (from any screen).
pub fn on_menu(world: &mut World, id: &'static str) {
    if let Some(n) = id.strip_prefix("stage:").and_then(|n| n.parse::<u32>().ok()) {
        let mode = world.resource::<StageSel>().mode;
        super::start_stage(world, n, mode);
        return;
    }
    if let Some(k) = id.strip_prefix("row:").and_then(|n| n.parse::<usize>().ok()) {
        pick_rival(world, k);
        return;
    }
    if let Some(key) = TextKey::from_action(id) {
        textfield::begin(world, key);
        return;
    }
    match id {
        "practice" => {
            let n = world.resource::<Profile>().last_stage;
            super::start_stage(world, n, RunMode::Practice);
        }
        "host" | "select" => {
            let mode = if id == "host" { RunMode::Team } else { RunMode::Practice };
            *world.resource_mut::<StageSel>() = StageSel { mode, shown: None };
            world.write_message(ScreenRequest::Push("stages"));
        }
        "boards" => {
            let n = world.get_resource::<StageNow>().map_or(world.resource::<Profile>().last_stage, |s| s.id);
            world.insert_resource(BoardSel(n));
            world.write_message(ScreenRequest::Push("boards"));
        }
        "board_prev" | "board_next" => {
            let n = stage_count();
            let cur = world.resource::<BoardSel>().0;
            let next = if id == "board_next" { cur % n + 1 } else { (cur + n - 2) % n + 1 };
            world.insert_resource(BoardSel(next));
            world.write_message(RebuildScreen);
        }
        "connect" => {
            let addr = world.resource::<Profile>().join_address.clone();
            let note = match netlink::join(world, &addr) {
                Ok(()) => format!("Connecting to {addr}…"),
                Err(e) => e,
            };
            world.insert_resource(NetNote(Some(note)));
        }
        "export" => match super::export_board(world, None) {
            Ok(p) => {
                world.write_message(Toast::new("Board exported").body(p.display().to_string()).secs(6.0));
            }
            Err(e) => {
                world.write_message(Toast::new("Export failed").body(e));
            }
        },
        "import" => {
            let inbox = world.get_resource::<engine::paths::DataDir>().map(|d| d.file(super::INBOX_DIR).display().to_string()).unwrap_or_default();
            match super::import_inbox(world) {
                Ok((0, _)) => {
                    world.write_message(Toast::new("Nothing to import").body(format!("Put friends' board files in {inbox}")).secs(6.0));
                }
                Ok((files, n)) => {
                    world.write_message(Toast::new(format!("Merged {files} board file{}", if files == 1 { "" } else { "s" })).body(format!("{n} times added or improved")));
                    world.write_message(RebuildScreen);
                }
                Err(e) => {
                    world.write_message(Toast::new("Import failed").body(e));
                }
            }
        }
        "retry" | "restart" => super::restart_stage(world),
        "recover" => super::recover(world),
        "next" => {
            let cur = world.get_resource::<StageNow>().map_or(1, |s| s.id);
            let mode = world.get_resource::<RunMode>().copied().unwrap_or_default();
            super::start_stage(world, cur % stage_count() + 1, mode);
        }
        "menu" => {
            world.resource_mut::<NextState<AppState>>().set(AppState::MainMenu);
        }
        _ => {}
    }
}

fn stage_count() -> u32 {
    crate::stage::stages().len() as u32
}

/// Leaderboard row `k` was picked: race that team's ghost on this stage.
fn pick_rival(world: &mut World, k: usize) {
    let stage = world.resource::<BoardSel>().0;
    let hash = world.resource::<StageCatalog>().info(stage).map(|i| i.hash.clone()).unwrap_or_default();
    let Some((rec, _)) = world.resource::<Boards>().0.listing(stage, &hash).get(k).map(|(r, s)| ((*r).clone(), *s)) else { return };
    let has_ghost = super::list_ghosts(world, stage).iter().any(|g| g.team() == rec.team());
    if !has_ghost {
        world.write_message(Toast::new("No ghost for this run").body("Import the team's board file to race them."));
        return;
    }
    {
        let mut p = world.resource_mut::<Profile>();
        p.set_rival(stage, rec.team());
        p.ghost = 3;
    }
    world.write_message(Toast::new("Rival ghost set").body(format!("{} on stage {stage}", rec.team_label())));
    world.write_message(RebuildScreen);
}

// ---------------------------------------------------------------- small parts

fn hrow(gap: f32) -> impl Bundle {
    (Node { column_gap: px(gap), align_items: AlignItems::Baseline, ..default() }, Pickable::IGNORE)
}

fn col(gap: f32) -> impl Bundle {
    (Node { flex_direction: FlexDirection::Column, row_gap: px(gap), ..default() }, Pickable::IGNORE)
}

fn stat(p: &mut ChildSpawnerCommands, t: &Theme, value: impl Into<String>, name: &str, color: Color, size: f32) {
    p.spawn(col(0.0)).with_children(|s| {
        s.spawn(text(value, display(size), color));
        s.spawn(text(name.to_uppercase(), label(SIZE_TINY), t.text_dim));
    });
}

/// A fixed-width table cell.
fn cell(p: &mut ChildSpawnerCommands, s: impl Into<String>, font: TextFont, color: Color, width: f32) {
    p.spawn((Node { width: px(width), flex_shrink: 0.0, ..default() }, Pickable::IGNORE)).with_children(|c| {
        c.spawn((text(s, font, color), KeepColor, TextLayout::no_wrap()));
    });
}

/// A small rally-plate tag ("GHOST", "OLD ROAD").
fn tag(p: &mut ChildSpawnerCommands, s: &str, fg: Color, bg: Color) {
    p.spawn((
        Node { padding: UiRect::axes(px(6), px(1)), border_radius: BorderRadius::all(px(2)), ..default() },
        BackgroundColor(bg),
        Pickable::IGNORE,
    ))
    .with_children(|c| {
        c.spawn((text(s.to_uppercase(), label(SIZE_TINY), fg), KeepColor));
    });
}

pub fn biome_name(b: Biome) -> &'static str {
    match b {
        Biome::PineForest => "Pine forest",
        Biome::BirchHills => "Birch hills",
        Biome::Scrub => "Dry scrub",
        Biome::Moor => "Highland moor",
        Biome::SnowForest => "Snow forest",
    }
}

pub fn surface_name(s: Surface) -> &'static str {
    match s {
        Surface::Gravel => "Gravel",
        Surface::Tarmac => "Tarmac",
        Surface::Mud => "Mud",
        Surface::Snow => "Snow",
        Surface::Verge => "Verge",
        Surface::Rough => "Rough",
    }
}

fn time_name(t: TimeOfDay) -> &'static str {
    match t {
        TimeOfDay::Morning => "Morning",
        TimeOfDay::Noon => "Midday",
        TimeOfDay::Afternoon => "Afternoon",
        TimeOfDay::Dusk => "Dusk",
    }
}

fn weather_name(w: Weather) -> &'static str {
    match w {
        Weather::Clear => "clear",
        Weather::Overcast => "overcast",
        Weather::Fog => "fog",
        Weather::Rain => "rain",
    }
}

fn my_team(p: &Profile, mode: RunMode) -> String {
    board::team_key(&p.driver_name, if mode == RunMode::Team { &p.guide_name } else { "" })
}

// ---------------------------------------------------------------- main

fn build_main(world: &mut World, root: Entity) {
    let map = world.resource::<StageCatalog>().map.clone();
    if let Some(map) = map {
        world.commands().entity(root).with_children(|p| {
            p.spawn((
                ImageNode { image: map, color: Color::srgba(1.0, 1.0, 1.0, 0.85), ..default() },
                Node { position_type: PositionType::Absolute, right: px(-140), top: percent(-14), height: percent(128), aspect_ratio: Some(1.0), ..default() },
                Pickable::IGNORE,
            ));
        });
    }
    engine::ui::menus::build_main(world, root);
}

/// Main menu, bottom right: the crew and their tally.
fn crew_card(world: &mut World, slot: Entity) {
    let t = world.resource::<Theme>().clone();
    let p = world.resource::<Profile>().clone();
    let b = world.resource::<Boards>().0.clone();
    let team = board::team_key(&p.driver_name, &p.guide_name);
    let cat = world.resource::<StageCatalog>();
    let current = |r: &Record| cat.info(r.stage).is_none_or(|i| i.hash == r.stage_hash);
    let driven = b.records.iter().filter(|r| r.team() == team && current(r)).count();
    let leads = (1..=stage_count()).filter(|s| cat.info(*s).and_then(|i| b.leader(*s, &i.hash)).is_some_and(|r| r.team() == team)).count();
    let last = cat.info(p.last_stage).map(|i| i.def.name.clone()).unwrap_or_default();
    world.commands().entity(slot).with_children(|s| {
        s.spawn(accent_panel(&t, t.accent, 22.0, GAP_S)).with_children(|c| {
            c.spawn(eyebrow("Crew", t.accent));
            c.spawn(hrow(GAP_S)).with_children(|r| {
                r.spawn(text(p.driver_name.clone(), display(36.0), t.text));
                r.spawn(text("&", body(SIZE_BODY), t.text_dim));
                r.spawn(text(p.guide_name.clone(), display(36.0), t.text));
            });
            c.spawn(rule(t.line));
            c.spawn((Node { column_gap: px(GAP_L), ..default() }, Pickable::IGNORE)).with_children(|r| {
                stat(r, &t, format!("{driven}/{}", stage_count()), "Stages timed", t.text, 32.0);
                stat(r, &t, leads.to_string(), "Stage records", if leads > 0 { t.accent } else { t.text_dim }, 32.0);
            });
            if !last.is_empty() {
                c.spawn(text(format!("Practice starts on {} (stage {}).", last, p.last_stage), body(SIZE_SMALL), t.text_muted));
            }
        });
    });
}

// ---------------------------------------------------------------- stage select

fn build_stages(world: &mut World, root: Entity) {
    let t = world.resource::<Theme>().clone();
    let sel = *world.resource::<StageSel>();
    world.resource_mut::<StageSel>().shown = None;
    let cat = world.resource::<StageCatalog>();
    let b = &world.resource::<Boards>().0;
    let p = world.resource::<Profile>();
    let team = my_team(p, sel.mode);
    let defs = crate::stage::stages();
    let rows: Vec<(u32, String, String, Option<f32>)> = defs
        .iter()
        .map(|d| {
            let (len, best) = match cat.info(d.id) {
                Some(i) => (i.length, b.best_of(d.id, &i.hash, &team).map(|r| r.total())),
                None => (d.length, None),
            };
            (d.id, d.name.clone(), format!("{} \u{b7} {:.1} km \u{b7} {}", biome_name(d.biome), len / 1000.0, surface_name(d.surface)), best)
        })
        .collect();
    let (kicker, verb) = match sel.mode {
        RunMode::Practice => ("Practice \u{b7} solo", "Drive"),
        RunMode::Team => ("Host as driver", "Host"),
    };
    world.commands().entity(root).with_children(|p| {
        menu_backdrop(p, &t, 0.25, 1.0);
        p.spawn((
            Node { position_type: PositionType::Absolute, left: px(EDGE), top: px(64), bottom: px(96), flex_direction: FlexDirection::Column, row_gap: px(GAP_S), ..default() },
            Pickable::IGNORE,
        ))
        .with_children(|c| {
            eyebrow_bar(c, kicker, t.accent);
            c.spawn(title("Choose a stage", SIZE_H1, t.text));
            c.spawn(spacer(GAP_S));
            c.spawn((Node { flex_direction: FlexDirection::Column, row_gap: px(2), margin: UiRect::left(px(-14)), ..default() }, Pickable::IGNORE)).with_children(|list| {
                for (k, (id, name, info, best)) in rows.iter().enumerate() {
                    list.spawn((
                        Button,
                        ActionButton(MenuAction::Custom(STAGE_IDS[k]).into()),
                        Focusable { order: k as u32 + 1 },
                        StageRow(*id),
                        nav_look(&t),
                        Node { width: px(700), height: px(37), align_items: AlignItems::Center, padding: UiRect::horizontal(px(14)), column_gap: px(14), ..default() },
                        BackgroundColor(Color::NONE),
                    ))
                    .with_children(|r| {
                        r.spawn((
                            LookBg([Color::NONE, t.accent, Color::NONE]),
                            Node { position_type: PositionType::Absolute, left: px(0), top: px(7), bottom: px(7), width: px(3), ..default() },
                            BackgroundColor(Color::NONE),
                            Pickable::IGNORE,
                        ));
                        cell(r, format!("{id:02}"), label(20.0), t.accent.with_alpha(0.8), 28.0);
                        r.spawn((Node { width: px(210), flex_shrink: 0.0, ..default() }, Pickable::IGNORE)).with_children(|c| {
                            c.spawn((text(name.to_uppercase(), label(25.0), t.text_muted), TextLayout::no_wrap()));
                        });
                        r.spawn((Node { flex_grow: 1.0, ..default() }, Pickable::IGNORE)).with_children(|c| {
                            c.spawn((text(info.clone(), body(15.0), t.text_dim), KeepColor, TextLayout::no_wrap()));
                        });
                        let (bt, bc) = match best {
                            Some(s) => (board::clock(*s), t.text),
                            None => ("\u{2014}".to_string(), t.text_dim),
                        };
                        r.spawn((text(bt, body_bold(17.0), bc), KeepColor));
                    });
                }
            });
        });
        p.spawn((
            StageDetails,
            Node { position_type: PositionType::Absolute, right: px(EDGE * 0.6), top: px(0), bottom: px(0), width: px(560), flex_direction: FlexDirection::Column, justify_content: JustifyContent::Center, ..default() },
            Pickable::IGNORE,
        ));
        spawn_footer(p, &t, &[("W / S", "Stage"), ("Enter", verb), ("Esc", "Back")]);
    });
}

/// Rebuilds the details panel when the focused stage row changes.
fn stage_details(world: &mut World) {
    if world.get_resource::<Screens>().and_then(|s| s.top()) != Some("stages") {
        return;
    }
    let Some(f) = world.get_resource::<Focus>().and_then(|f| f.0) else { return };
    let Some(id) = world.get::<StageRow>(f).map(|r| r.0) else { return };
    let mut q = world.query_filtered::<Entity, With<StageDetails>>();
    let Some(panel) = q.iter(world).next() else { return };
    let sel = *world.resource::<StageSel>();
    let fresh = world.get::<Children>(panel).is_none_or(|c| c.is_empty());
    if sel.shown == Some(id) && !fresh {
        return;
    }
    world.resource_mut::<StageSel>().shown = Some(id);
    world.entity_mut(panel).despawn_related::<Children>();
    let t = world.resource::<Theme>().clone();
    let cat = world.resource::<StageCatalog>();
    let Some(def) = crate::stage::stages().into_iter().find(|d| d.id == id) else { return };
    let info = cat.info(id).cloned();
    let img = cat.preview(id);
    let b = &world.resource::<Boards>().0;
    let p = world.resource::<Profile>().clone();
    let (team_best, solo_best, leader) = match &info {
        Some(i) => (
            b.best_of(id, &i.hash, &board::team_key(&p.driver_name, &p.guide_name)).cloned(),
            b.best_of(id, &i.hash, &board::team_key(&p.driver_name, "")).cloned(),
            b.leader(id, &i.hash).cloned(),
        ),
        None => (None, None, None),
    };
    let ghosts = super::list_ghosts(world, id);
    let verb = if sel.mode == RunMode::Team { "Host & drive" } else { "Drive" };
    world.commands().entity(panel).with_children(|c| {
        c.spawn(panel_box(&t)).with_children(|c| {
            c.spawn((Node { width: px(512), height: px(360), justify_content: JustifyContent::Center, ..default() }, BackgroundColor(t.bg.with_alpha(0.6)), Pickable::IGNORE)).with_children(|m| {
                match img {
                    Some(h) => {
                        m.spawn((ImageNode::new(h), Node { width: px(360), height: px(360), ..default() }, Pickable::IGNORE));
                    }
                    None => {
                        m.spawn((text("Surveying the road\u{2026}", body(SIZE_BODY), t.text_dim), Node { align_self: AlignSelf::Center, ..default() }));
                    }
                }
            });
            c.spawn(hrow(GAP_M)).with_children(|r| {
                r.spawn(text(format!("{id:02}"), display(40.0), t.accent));
                r.spawn(text(def.name.to_uppercase(), display(40.0), t.text));
            });
            let surf = match &info {
                Some(i) => i.surfaces.iter().filter(|(s, f)| *f >= 0.02 && !matches!(s, Surface::Verge | Surface::Rough)).map(|(s, f)| format!("{} {:.0}%", surface_name(*s), f * 100.0)).collect::<Vec<_>>().join(", "),
                None => surface_name(def.surface).to_string(),
            };
            c.spawn(text(format!("{} \u{b7} {} \u{b7} {}, {}", biome_name(def.biome), surf, time_name(def.time), weather_name(def.weather)), body(SIZE_BODY), t.text_muted));
            c.spawn((Node { column_gap: px(GAP_L), margin: UiRect::vertical(px(GAP_XS)), ..default() }, Pickable::IGNORE)).with_children(|r| {
                let len = info.as_ref().map_or(def.length, |i| i.length);
                stat(r, &t, format!("{:.2}", len / 1000.0), "km", t.text, 34.0);
                if let Some(i) = &info {
                    stat(r, &t, i.corners.to_string(), "Corners", t.text, 34.0);
                    stat(r, &t, i.tight.to_string(), "Hairpins & 2s", t.text, 34.0);
                    if i.crests > 0 {
                        stat(r, &t, i.crests.to_string(), "Crests", t.text, 34.0);
                    }
                    stat(r, &t, format!("{:.0}", i.climb), "m climb", t.text, 34.0);
                }
            });
            c.spawn(rule(t.line));
            let line = |c: &mut ChildSpawnerCommands, name: &str, r: Option<&Record>, color: Color| {
                c.spawn((Node { justify_content: JustifyContent::SpaceBetween, ..default() }, Pickable::IGNORE)).with_children(|l| {
                    l.spawn(text(name, body(SIZE_BODY), t.text_muted));
                    match r {
                        Some(r) => l.spawn(text(format!("{}   {}", board::clock(r.total()), r.team_label()), body_bold(SIZE_BODY), color)),
                        None => l.spawn(text("no time yet", body(SIZE_BODY), t.text_dim)),
                    };
                });
            };
            line(c, "Stage record", leader.as_ref(), t.accent);
            line(c, "Team best", team_best.as_ref(), t.text);
            line(c, "Practice best", solo_best.as_ref(), t.text);
            let ghost_line = match p.ghost {
                0 => "Ghost off (Settings > Game)".to_string(),
                _ if ghosts.is_empty() => "No ghost yet: finish the stage to record one.".to_string(),
                _ => format!("{} ghost{} saved", ghosts.len(), if ghosts.len() == 1 { "" } else { "s" }),
            };
            c.spawn(text(ghost_line, body(SIZE_SMALL), t.text_dim));
            c.spawn(spacer(GAP_S));
            action_button(c, &t, verb, MenuAction::Custom(STAGE_IDS[(id - 1) as usize]), None, true, 512.0);
        });
    });
}

fn panel_box(t: &Theme) -> impl Bundle {
    panel(t, 24.0, GAP_S + 2.0)
}

// ---------------------------------------------------------------- join / team

fn build_join(world: &mut World, root: Entity) {
    let t = world.resource::<Theme>().clone();
    let p = world.resource::<Profile>().clone();
    world.insert_resource(NetNote(None));
    world.commands().entity(root).with_children(|r| {
        menu_backdrop(r, &t, 0.25, 1.0);
        r.spawn(menu_column(GAP_M)).with_children(|c| {
            eyebrow_bar(c, "Join as guide", t.accent);
            c.spawn(title("Read the road", SIZE_H1, t.text));
            c.spawn((
                text("You see the stage map and the car on it; the driver sees only the road. Ask the driver for their address (shown when they host) and connect.", body(SIZE_BODY), t.text_muted),
                Node { max_width: px(620), ..default() },
            ));
            c.spawn(spacer(GAP_S));
            text_field(c, &t, "Driver's address", TextKey::Address, &p.join_address, 1, 520.0);
            text_field(c, &t, "Your name", TextKey::Guide, &p.guide_name, 2, 520.0);
            c.spawn(spacer(GAP_S));
            c.spawn((Node { column_gap: px(GAP_M), align_items: AlignItems::Center, ..default() }, Pickable::IGNORE)).with_children(|b| {
                action_button(b, &t, "Connect", MenuAction::Custom("connect"), Some(3), true, 300.0);
                action_button(b, &t, "Back", UiAction::Back, Some(4), false, 160.0);
            });
            c.spawn((JoinStatus, text("", body(SIZE_BODY), t.text_muted)));
        });
        spawn_footer(r, &t, &[("Enter", "Edit / select"), ("Esc", "Back")]);
    });
}

fn join_status(world: &mut World) {
    if world.get_resource::<Screens>().and_then(|s| s.top()) != Some("join") {
        return;
    }
    let t = world.resource::<Theme>().clone();
    let (msg, color) = match netlink::status(world) {
        netlink::NetStatus::Connecting(a) => (format!("Connecting to {a}\u{2026}"), t.text_muted),
        netlink::NetStatus::Connected(a) => (format!("Connected to {a}. Waiting for the driver to start a stage."), t.good),
        netlink::NetStatus::Failed(e) => (e, t.bad),
        _ => (world.resource::<NetNote>().0.clone().unwrap_or_default(), t.text_muted),
    };
    let mut q = world.query_filtered::<(&mut Text, &mut TextColor), With<JoinStatus>>();
    for (mut txt, mut c) in q.iter_mut(world) {
        if txt.0 != msg {
            txt.0 = msg.clone();
        }
        if c.0 != color {
            c.0 = color;
        }
    }
}

fn build_team(world: &mut World, root: Entity) {
    let t = world.resource::<Theme>().clone();
    let p = world.resource::<Profile>().clone();
    world.commands().entity(root).with_children(|r| {
        menu_backdrop(r, &t, 0.25, 1.0);
        r.spawn(menu_column(GAP_M)).with_children(|c| {
            eyebrow_bar(c, "Crew", t.accent);
            c.spawn(title("Team names", SIZE_H1, t.text));
            c.spawn((
                text("Both names go on every leaderboard time. When a guide joins over the network, their own name is used.", body(SIZE_BODY), t.text_muted),
                Node { max_width: px(620), ..default() },
            ));
            c.spawn(spacer(GAP_S));
            text_field(c, &t, "Driver", TextKey::Driver, &p.driver_name, 1, 460.0);
            text_field(c, &t, "Guide", TextKey::Guide, &p.guide_name, 2, 460.0);
            c.spawn(spacer(GAP_S));
            action_button(c, &t, "Done", UiAction::Back, Some(3), true, 240.0);
        });
        spawn_footer(r, &t, &[("Enter", "Edit / done"), ("Esc", "Back")]);
    });
}

// ---------------------------------------------------------------- leaderboards

fn build_boards(world: &mut World, root: Entity) {
    let t = world.resource::<Theme>().clone();
    let stage = world.resource::<BoardSel>().0;
    let def = crate::stage::stages().into_iter().find(|d| d.id == stage);
    let cat = world.resource::<StageCatalog>();
    let hash = cat.info(stage).map(|i| i.hash.clone());
    let b = world.resource::<Boards>().0.clone();
    let p = world.resource::<Profile>().clone();
    let listing: Vec<(Record, bool)> = match &hash {
        Some(h) => b.listing(stage, h).into_iter().map(|(r, s)| (r.clone(), s)).collect(),
        None => b.records.iter().filter(|r| r.stage == stage).map(|r| (r.clone(), false)).collect(),
    };
    let ghosts: Vec<String> = super::list_ghosts(world, stage).iter().map(|g| g.team()).collect();
    let rival = p.rival(stage).map(str::to_string);
    let me = [board::team_key(&p.driver_name, &p.guide_name), board::team_key(&p.driver_name, "")];
    let inbox = world.get_resource::<engine::paths::DataDir>().map(|d| d.file(super::INBOX_DIR).display().to_string()).unwrap_or_default();
    let total = b.records.len();
    world.commands().entity(root).with_children(|r| {
        menu_backdrop(r, &t, 0.45, 1.0);
        r.spawn((
            Node { position_type: PositionType::Absolute, left: px(EDGE), right: px(EDGE), top: px(64), bottom: px(96), flex_direction: FlexDirection::Column, row_gap: px(GAP_S), ..default() },
            Pickable::IGNORE,
        ))
        .with_children(|c| {
            eyebrow_bar(c, "Leaderboards", t.accent);
            c.spawn((Node { column_gap: px(GAP_L), align_items: AlignItems::Center, ..default() }, Pickable::IGNORE)).with_children(|h| {
                h.spawn(title(&format!("{stage:02} {}", def.as_ref().map_or("", |d| d.name.as_str())), SIZE_H1, t.text));
                action_button(h, &t, "< Prev", MenuAction::Custom("board_prev"), Some(1), false, 130.0);
                action_button(h, &t, "Next >", MenuAction::Custom("board_next"), Some(2), false, 130.0);
            });
            if let Some(d) = &def {
                c.spawn(text(format!("{} \u{b7} {} \u{b7} {:.1} km \u{b7} {} times on this machine across all stages", biome_name(d.biome), surface_name(d.surface), d.length / 1000.0, total), body(SIZE_BODY), t.text_muted));
            }
            c.spawn(spacer(GAP_S));
            // Header.
            c.spawn((Node { padding: UiRect::horizontal(px(14)), column_gap: px(GAP_M), ..default() }, Pickable::IGNORE)).with_children(|h| {
                for (s, w) in [("#", 48.0), ("Team", 470.0), ("Time", 150.0), ("Penalty", 100.0), ("Date", 140.0), ("Version", 100.0), ("", 220.0)] {
                    cell(h, s.to_uppercase(), label(SIZE_LABEL), t.text_dim, w);
                }
            });
            c.spawn(rule(t.line));
            if listing.is_empty() {
                c.spawn((text("No times on this stage yet. Drive it, or import a friend's board.", body(SIZE_BODY), t.text_muted), Node { margin: UiRect::top(px(GAP_M)), ..default() }));
            }
            let mut team_rank = 0;
            let mut solo_rank = 0;
            for (k, (rec, stale)) in listing.iter().take(ROW_IDS.len()).enumerate() {
                let rank = if *stale {
                    "\u{2014}".to_string()
                } else if rec.solo() {
                    solo_rank += 1;
                    format!("P{solo_rank}")
                } else {
                    team_rank += 1;
                    team_rank.to_string()
                };
                let mine = me.contains(&rec.team());
                let color = if *stale { t.text_dim } else if team_rank == 1 && !rec.solo() { t.accent } else if mine { t.info } else { t.text };
                c.spawn((
                    Button,
                    ActionButton(MenuAction::Custom(ROW_IDS[k]).into()),
                    Focusable { order: 10 + k as u32 },
                    nav_look(&t),
                    Node { padding: UiRect::axes(px(14), px(6)), column_gap: px(GAP_M), align_items: AlignItems::Center, ..default() },
                    BackgroundColor(Color::NONE),
                ))
                .with_children(|row| {
                    row.spawn((
                        LookBg([Color::NONE, t.accent, Color::NONE]),
                        Node { position_type: PositionType::Absolute, left: px(0), top: px(6), bottom: px(6), width: px(3), ..default() },
                        BackgroundColor(Color::NONE),
                        Pickable::IGNORE,
                    ));
                    cell(row, rank, display(28.0), color, 48.0);
                    cell(row, rec.team_label(), body_bold(22.0), color, 470.0);
                    cell(row, board::clock(rec.total()), display(28.0), color, 150.0);
                    cell(row, if rec.penalty > 0.0 { format!("+{:.0} s", rec.penalty) } else { "\u{2014}".into() }, body(SIZE_BODY), if rec.penalty > 0.0 { t.bad } else { t.text_dim }, 100.0);
                    cell(row, board::date_text(rec.date), body(SIZE_BODY), t.text_muted, 140.0);
                    cell(row, rec.version.clone(), body(SIZE_BODY), t.text_muted, 100.0);
                    row.spawn((Node { column_gap: px(GAP_XS + 2.0), width: px(220), ..default() }, Pickable::IGNORE)).with_children(|tags| {
                        if *stale {
                            tag(tags, "Old road", t.text_muted, t.raised);
                        }
                        if ghosts.contains(&rec.team()) {
                            if rival.as_deref() == Some(rec.team().as_str()) {
                                tag(tags, "Rival ghost", t.on_accent, t.accent);
                            } else {
                                tag(tags, "Ghost", t.text, t.raised);
                            }
                        }
                        if mine {
                            tag(tags, "You", t.bg, t.info);
                        }
                    });
                });
            }
            c.spawn((Node { flex_grow: 1.0, ..default() }, Pickable::IGNORE));
            c.spawn((Node { column_gap: px(GAP_M), align_items: AlignItems::Center, ..default() }, Pickable::IGNORE)).with_children(|b| {
                action_button(b, &t, "Export board", MenuAction::Custom("export"), Some(40), false, 240.0);
                action_button(b, &t, "Import boards", MenuAction::Custom("import"), Some(41), false, 240.0);
                action_button(b, &t, "Back", UiAction::Back, Some(42), true, 200.0);
            });
            c.spawn(text(format!("Pick a time with a ghost to race it. Import merges every board file in {inbox} (each team's best is kept)."), body(SIZE_SMALL), t.text_dim));
        });
        spawn_footer(r, &t, &[("W / S", "Move"), ("Enter", "Race ghost / select"), ("Esc", "Back")]);
    });
}

// ---------------------------------------------------------------- results

fn build_results(world: &mut World, root: Entity) {
    let t = world.resource::<Theme>().clone();
    let Some(res) = world.get_resource::<LastResult>().cloned() else { return };
    let mode = world.get_resource::<RunMode>().copied().unwrap_or_default();
    let r = &res.record;
    let (headline, hcolor, celebrate) = if res.inserted.stage_record {
        ("Stage record", t.accent, true)
    } else if res.inserted.team_best && res.best_before.is_some() {
        (if r.solo() { "New personal best" } else { "New team best" }, t.good, true)
    } else if res.inserted.team_best {
        ("First time set", t.good, false)
    } else {
        ("Stage complete", t.text, false)
    };
    let rank_line = if r.solo() {
        format!("Practice \u{b7} P{} of your solo times on this road", res.inserted.rank)
    } else {
        format!("P{} on the stage \u{b7} {}", res.inserted.rank, r.team_label())
    };
    let names = ["Split 1", "Split 2", "Split 3", "Finish"];
    let next_name = crate::stage::stages().into_iter().find(|d| d.id == r.stage % stage_count() + 1).map(|d| d.name).unwrap_or_default();
    world.commands().entity(root).with_children(|p| {
        menu_backdrop(p, &t, 0.72, 1.0);
        p.spawn(menu_column(GAP_S)).with_children(|c| {
            eyebrow_bar(c, &format!("Stage {} \u{b7} {} \u{b7} {}", r.stage, res.stage_name, if mode == RunMode::Team { "team run" } else { "practice" }), t.accent);
            let mut h = c.spawn(title(headline, SIZE_H1, hcolor));
            if celebrate {
                h.insert(Pulse(hcolor, t.text));
            }
            c.spawn(hrow(GAP_M)).with_children(|row| {
                row.spawn(text(board::clock(r.total()), display(120.0), t.text));
                if r.penalty > 0.0 {
                    row.spawn(text(format!("incl. +{:.0} s", r.penalty), body_bold(SIZE_BODY + 2.0), t.bad));
                }
            });
            c.spawn(text(rank_line, body(SIZE_BODY + 2.0), t.text_muted));
            if let Some(prev) = &res.best_before {
                let d = r.total() - prev.total();
                c.spawn(text(format!("{} vs your previous best {}", board::delta(d), board::clock(prev.total())), body_bold(SIZE_BODY), if d < 0.0 { t.good } else { t.bad }));
            }
            c.spawn(spacer(GAP_S));
            // Splits table.
            c.spawn(panel(&t, 18.0, GAP_XS + 2.0)).with_children(|tb| {
                tb.spawn(hrow(GAP_M)).with_children(|h| {
                    for (s, w) in [("", 110.0), ("Time", 160.0), ("vs best", 150.0), ("vs leader", 150.0)] {
                        cell(h, s.to_uppercase(), label(SIZE_LABEL), t.text_dim, w);
                    }
                });
                for (i, s) in r.splits.iter().enumerate().take(4) {
                    tb.spawn(hrow(GAP_M)).with_children(|row| {
                        cell(row, names.get(i).copied().unwrap_or("Split"), label(22.0), t.text_muted, 110.0);
                        cell(row, board::clock(*s), body_bold(22.0), t.text, 160.0);
                        for other in [&res.best_before, &res.leader_before] {
                            match other.as_ref().and_then(|o| o.splits.get(i)) {
                                Some(o) => {
                                    let d = s - o;
                                    cell(row, board::delta(d), body_bold(22.0), if d <= 0.0 { t.good } else { t.bad }, 150.0);
                                }
                                None => cell(row, "\u{2014}", body(22.0), t.text_dim, 150.0),
                            }
                        }
                    });
                }
            });
            if res.ghost_saved {
                c.spawn(text("Ghost saved: race it next time (Settings > Game > Ghost car).", body(SIZE_SMALL), t.text_dim));
            }
            c.spawn(spacer(GAP_M));
            c.spawn((Node { column_gap: px(GAP_M), align_items: AlignItems::Center, ..default() }, Pickable::IGNORE)).with_children(|b| {
                action_button(b, &t, "Retry", MenuAction::Custom("retry"), Some(1), true, 220.0);
                action_button(b, &t, &format!("Next: {next_name}"), MenuAction::Custom("next"), Some(2), false, 300.0);
                action_button(b, &t, "Menu", MenuAction::Custom("menu"), Some(3), false, 160.0);
            });
        });
        spawn_footer(p, &t, &[("A / D", "Choose"), ("Enter", "Select")]);
    });
}

fn pulse(time: Res<Time<Real>>, mut q: Query<(&Pulse, &mut TextColor)>) {
    let k = (time.elapsed_secs() * 3.2).sin() * 0.5 + 0.5;
    for (p, mut c) in &mut q {
        let (a, b) = (p.0.to_srgba(), p.1.to_srgba());
        let m = Color::srgb(a.red + (b.red - a.red) * k * 0.6, a.green + (b.green - a.green) * k * 0.6, a.blue + (b.blue - a.blue) * k * 0.6);
        c.0 = m;
    }
}

// ---------------------------------------------------------------- pause

fn pause_eyebrow(world: &World) -> String {
    match world.get_resource::<StageNow>() {
        Some(s) => format!("Stage {} \u{b7} {}", s.id, s.name),
        None => "Pacenote".into(),
    }
}

fn pause_header(world: &mut World, slot: Entity) {
    let t = world.resource::<Theme>().clone();
    let Some(run) = world.get_resource::<StageRun>().cloned() else { return };
    let len = world.get_resource::<crate::world::StageWorld>().map_or(1.0, |s| s.road.length());
    world.commands().entity(slot).with_children(|p| {
        p.spawn(hrow(GAP_M)).with_children(|r| {
            r.spawn(text(board::clock(run.time + run.penalty), display(40.0), t.text));
            if run.penalty > 0.0 {
                r.spawn(text(format!("+{:.0} s", run.penalty), body_bold(SIZE_BODY), t.bad));
            }
            r.spawn(text(format!("{:.0}% of the stage", (run.progress / len * 100.0).clamp(0.0, 100.0)), body(SIZE_SMALL), t.text_muted));
        });
    });
}

// ---------------------------------------------------------------- credits

const ENGINE_CREDITS: &str = "\
engine: code-first game shell (Rust)
Bevy 0.19: bevyengine.org, MIT / Apache-2.0
avian3d 0.7: Joona Aalto and contributors, MIT / Apache-2.0
ashlar: control port, MCP and trace tooling
Barlow and Barlow Condensed: Jeremy Tribby, SIL Open Font License 1.1";

/// The game's credits file (written by the art stream): `assets/credits.txt`. Lines starting
/// with `#` are comments, a line ending in `:` is a heading, anything else a paragraph.
pub fn game_credits(world: &World) -> String {
    let path = world
        .get_resource::<engine::paths::AssetsDir>()
        .map(|a| a.0.join("credits.txt"))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/credits.txt")));
    std::fs::read_to_string(path).unwrap_or_else(|_| "Pacenote\n(credits.txt is missing)".into())
}

fn build_credits(world: &mut World, root: Entity) {
    let t = world.resource::<Theme>().clone();
    let game = game_credits(world);
    world.commands().entity(root).with_children(|r| {
        menu_backdrop(r, &t, 0.45, 1.0);
        r.spawn((
            Node { position_type: PositionType::Absolute, left: px(EDGE), right: px(EDGE), top: px(64), bottom: px(96), flex_direction: FlexDirection::Column, row_gap: px(GAP_M), ..default() },
            Pickable::IGNORE,
        ))
        .with_children(|c| {
            eyebrow_bar(c, "Pacenote", t.accent);
            c.spawn(title("Credits", SIZE_H1, t.text));
            c.spawn((Node { column_gap: px(GAP_XL), flex_grow: 1.0, min_height: px(0), ..default() }, Pickable::IGNORE)).with_children(|cols| {
                cols.spawn((
                    ScrollList,
                    RelativeCursorPosition::default(),
                    Node { flex_direction: FlexDirection::Column, row_gap: px(GAP_XS), width: px(900), overflow: Overflow::scroll_y(), ..default() },
                ))
                .with_children(|g| {
                    for line in game.lines() {
                        let l = line.trim_end();
                        if l.is_empty() {
                            g.spawn(spacer(GAP_S));
                        } else if l.starts_with('#') {
                            // Comment.
                        } else if let Some(h) = l.strip_suffix(':') {
                            g.spawn((eyebrow(h.trim(), t.accent), Node { margin: UiRect::top(px(GAP_S)), ..default() }));
                        } else {
                            g.spawn((text(l.trim().to_string(), body(18.0), t.text), Node { max_width: px(880), ..default() }));
                        }
                    }
                });
                cols.spawn(col(GAP_XS)).with_children(|e| {
                    e.spawn(eyebrow("Built with", t.accent));
                    for l in ENGINE_CREDITS.lines() {
                        e.spawn(text(l, body(18.0), t.text_muted));
                    }
                });
            });
            action_button(c, &t, "Back", UiAction::Back, Some(1), true, 200.0);
        });
        spawn_footer(r, &t, &[("Wheel", "Scroll"), ("Esc", "Back")]);
    });
}
