//! HUD: the race position block (top left) with the rivals on a progress strip, and the race
//! results card (positions, times, the purse) when the player's race ends.
//!
//! **Owner: stream F (GAMEPLAY).** Design: `docs/festival/GAME_SYSTEMS.md` §5-§6,
//! `docs/festival/UX_PASS.md` §3.3, `docs/festival/DECISIONS.md` R6.
//! A read-only view of F's state (`career::race_flow`); E owns `hud.rs` and the shared HUD
//! look. Despawns on leaving `InGame`; text is only rewritten when it changes, and the gaps
//! once a second (UX: "not every frame").
//!
//! Block: `P2/6 ▲1`, the car ahead and behind with the gap in seconds, a strip with a dot per
//! rival and the player, and a status line (pursuit release countdown, the Recover hold).
//! Results: Enter = Retry (the fee isn't charged again), F = Free roam. Settings › HUD ›
//! Race positions hides the block (the race runs anyway).

use std::fmt::Write;

use bevy::prelude::*;
use engine::flow::AppState;
use engine::settings::{FieldDef, FieldKind, RegisterSettings};
use engine::ui::theme::*;

use crate::career::race_flow::{LastRace, RaceRun};
use crate::career::racers::{Finish, Who};
use crate::career::{CareerSettings, LastPayout, notes};
use crate::drive::StageRun;

#[derive(Component)]
struct RaceHud;

#[derive(Component)]
struct ResultsCard;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum RaceText {
    Pos,
    Change,
    Ahead,
    Behind,
    Status,
}

/// A dot on the strip: rival slot, or 0 for the player.
#[derive(Component)]
struct StripDot(u8);

const STRIP_W: f32 = 300.0;

pub struct RaceHudPlugin;

impl Plugin for RaceHudPlugin {
    fn build(&self, app: &mut App) {
        app.register_setting(FieldDef {
            key: "race_positions",
            tab: "HUD",
            label: "Race positions",
            hint: "Your position, the gaps to the cars ahead and behind, and the rivals on a strip",
            kind: FieldKind::Toggle,
            get: |w| w.get_resource::<CareerSettings>().map_or(1.0, |s| s.race_positions as u8 as f32),
            set: |w, v| {
                if let Some(mut s) = w.get_resource_mut::<CareerSettings>() {
                    s.race_positions = v > 0.5;
                }
            },
        })
        .add_systems(Update, (spawn_block, update_block, results_card, results_keys).chain().run_if(resource_exists::<RaceRun>))
        .add_systems(Update, despawn_all.run_if(not(resource_exists::<RaceRun>)));
    }
}

fn plate() -> BackgroundColor {
    BackgroundColor(Color::srgba(0.02, 0.03, 0.025, 0.55))
}

fn spawn_block(mut commands: Commands, theme: Res<Theme>, q: Query<(), With<RaceHud>>, race: Res<RaceRun>) {
    if !q.is_empty() {
        return;
    }
    let t = &*theme;
    commands
        .spawn((
            RaceHud,
            Name::new("Race HUD"),
            DespawnOnExit(AppState::InGame),
            Node { position_type: PositionType::Absolute, left: px(40), top: px(34), flex_direction: FlexDirection::Column, row_gap: px(2), padding: UiRect::new(px(18), px(18), px(8), px(12)), border_radius: BorderRadius::all(px(RADIUS)), ..default() },
            plate(),
            Pickable::IGNORE,
        ))
        .with_children(|c| {
            c.spawn((Node { column_gap: px(12), align_items: AlignItems::Baseline, ..default() }, Pickable::IGNORE)).with_children(|r| {
                r.spawn((RaceText::Pos, text("P-/6", display(64.0), t.text)));
                r.spawn((RaceText::Change, text("", body_bold(SIZE_BODY), t.text_muted)));
            });
            c.spawn((RaceText::Ahead, text("", label(SIZE_LABEL), t.text)));
            c.spawn((RaceText::Behind, text("", label(SIZE_LABEL), t.text)));
            // The strip: rivals (paper) and the player (accent) along the route.
            c.spawn((Node { width: px(STRIP_W), height: px(10), margin: UiRect::vertical(px(6)), ..default() }, Pickable::IGNORE)).with_children(|s| {
                s.spawn((Node { position_type: PositionType::Absolute, top: px(4), width: percent(100), height: px(2), ..default() }, BackgroundColor(Color::srgba(0.95, 0.93, 0.88, 0.2)), Pickable::IGNORE));
                for r in &race.sim.rivals {
                    s.spawn((StripDot(r.slot), Node { position_type: PositionType::Absolute, left: px(0), width: px(8), height: px(8), top: px(1), border_radius: BorderRadius::all(px(4)), ..default() }, BackgroundColor(t.text_muted), Pickable::IGNORE));
                }
                s.spawn((StripDot(0), Node { position_type: PositionType::Absolute, left: px(0), width: px(10), height: px(10), top: px(0), border_radius: BorderRadius::all(px(5)), ..default() }, BackgroundColor(t.accent), Pickable::IGNORE));
            });
            c.spawn((RaceText::Status, text("", label(SIZE_TINY + 1.0), t.accent)));
        });
}

/// Cached values: only changed text is written.
#[derive(Default)]
struct Shown {
    pos: usize,
    last_pos: usize,
    change: i32,
    gaps_at: f32,
    ahead: String,
    behind: String,
    status: String,
}

#[allow(clippy::too_many_arguments)]
fn update_block(
    race: Res<RaceRun>,
    run: Res<StageRun>,
    settings: Option<Res<CareerSettings>>,
    screens: Option<Res<engine::ui::prelude::Screens>>,
    time: Res<Time>,
    mut shown: Local<Shown>,
    mut root: Query<&mut Visibility, With<RaceHud>>,
    mut texts: Query<(&RaceText, &mut Text)>,
    mut dots: Query<(&StripDot, &mut Node)>,
) {
    let on = settings.as_ref().is_none_or(|s| s.race_positions) && !screens.is_some_and(|s| s.top().is_some()) && !race.done;
    for mut v in &mut root {
        v.set_if_neq(if on { Visibility::Inherited } else { Visibility::Hidden });
    }
    if !on {
        return;
    }
    let st = crate::career::race_flow::standings(&race, &run);
    let pos = st.iter().position(|e| e.0 == Who::Player).map_or(6, |p| p + 1);
    let mut set = |which: RaceText, s: &str| {
        for (k, mut t) in &mut texts {
            if *k == which && t.0 != s {
                t.0 = s.into();
            }
        }
    };
    if pos != shown.pos {
        if shown.pos != 0 {
            shown.change = shown.pos as i32 - pos as i32;
        }
        shown.pos = pos;
        let mut s = String::new();
        let _ = write!(s, "P{pos}/{}", st.len());
        set(RaceText::Pos, &s);
        // (The HUD font has no ▲ / ▼: the sign carries it.)
        let c = match shown.change {
            0 => String::new(),
            n if n > 0 => format!("+{n}"),
            n => format!("\u{2212}{}", -n),
        };
        set(RaceText::Change, &c);
    }
    // Gaps once a second (and when the position changes).
    let now = time.elapsed_secs();
    if now - shown.gaps_at >= 1.0 || shown.last_pos != pos {
        shown.gaps_at = now;
        shown.last_pos = pos;
        let name = |w: Who| match w {
            Who::Player => "You".to_string(),
            Who::Rival(s) => race.sim.rivals.iter().find(|r| r.slot == s).map_or("?".into(), |r| r.name.to_string()),
        };
        let gap = |w: Who| match w {
            Who::Rival(s) => race.sim.rivals.iter().position(|r| r.slot == s).map(|k| race.sim.gap_to_player(k, run.progress).abs()),
            Who::Player => None,
        };
        let line = |i: Option<usize>, up: bool| -> String {
            let Some(e) = i.and_then(|i| st.get(i)) else { return String::new() };
            let label = if up { "AHEAD" } else { "BEHIND" };
            match (e.2, gap(e.0)) {
                (Some(Finish::Dnf), _) => format!("{label}  {}  out", name(e.0)),
                (_, Some(g)) => format!("{label}  {}  {}{:.1}", name(e.0), if up { "+" } else { "\u{2212}" }, g),
                _ => String::new(),
            }
        };
        let ahead = line(pos.checked_sub(2), true);
        let behind = line(Some(pos), false);
        if ahead != shown.ahead {
            set(RaceText::Ahead, &ahead);
            shown.ahead = ahead;
        }
        if behind != shown.behind {
            set(RaceText::Behind, &behind);
            shown.behind = behind;
        }
    }
    // The status line: the start format, the pursuit release, the Recover hold.
    let status = if run.countdown > 0.0 {
        format!("{} start \u{b7} you're {} of 6", if race.sim.format == crate::career::racers::StartFormat::Grid { "Grid" } else { "Pursuit" }, race.sim.player_slot)
    } else if race.sim.player_held() {
        format!("Released in {:.1} s", (race.sim.player_release as f32 - race.sim.step as f32) / 60.0)
    } else if race.hold > 0 {
        format!("Marshals: hold {:.1} s", race.hold as f32 / 60.0)
    } else {
        String::new()
    };
    if status != shown.status {
        set(RaceText::Status, &status);
        shown.status = status;
    }
    // Dots along the strip (a handful of nodes).
    let len = race.sim.len.max(1.0);
    for (d, mut n) in &mut dots {
        let s = if d.0 == 0 { run.progress } else { race.sim.rivals.iter().find(|r| r.slot == d.0).map_or(0.0, |r| r.progress) };
        let x = px((s / len).clamp(0.0, 1.0) * (STRIP_W - 8.0));
        if n.left != x {
            n.left = x;
        }
    }
}

/// The results card (right, under the damage panel: the centre keeps the HUD's FINISH time),
/// once the result and the economy's payout lines are in.
#[allow(clippy::too_many_arguments)]
fn results_card(
    mut commands: Commands,
    theme: Res<Theme>,
    race: Res<RaceRun>,
    last: Option<Res<LastRace>>,
    payout: Res<LastPayout>,
    wallet: Res<crate::career::wallet::Wallet>,
    field: Option<Res<crate::career::racers::RaceField>>,
    cards: Query<Entity, With<ResultsCard>>,
    mut built: Local<bool>,
) {
    if !race.done {
        for e in &cards {
            commands.entity(e).despawn();
        }
        *built = false;
        return;
    }
    let Some(last) = last else { return };
    // Rebuilt when the payout lines or the balance change (the purse lands a frame later).
    if *built && !payout.is_changed() && !wallet.is_changed() {
        return;
    }
    for e in &cards {
        commands.entity(e).despawn();
    }
    *built = true;
    let t = &*theme;
    let r = &last.0;
    let winner = match r.order.first() {
        Some((_, Finish::Time(x))) => *x,
        _ => 0.0,
    };
    commands
        .spawn((ResultsCard, Name::new("Race results"), DespawnOnExit(AppState::InGame), Node { position_type: PositionType::Absolute, right: px(40), top: px(236), ..default() }, Pickable::IGNORE))
        .with_children(|root| {
            root.spawn((
                Node { flex_direction: FlexDirection::Column, row_gap: px(4), padding: UiRect::all(px(26)), min_width: px(560), border: UiRect::left(px(4)), border_radius: BorderRadius::all(px(RADIUS)), ..default() },
                BackgroundColor(Color::srgba(0.02, 0.03, 0.025, 0.86)),
                BorderColor::all(t.accent),
                Pickable::IGNORE,
            ))
            .with_children(|c| {
                c.spawn(eyebrow(&format!("Race \u{b7} {}", r.difficulty.label()), t.accent));
                let head = match r.order.iter().find(|e| e.0 == Who::Player).map(|e| e.1) {
                    Some(Finish::Dnf) => "RETIRED".to_string(),
                    _ => format!("P{} OF 6", r.player_pos),
                };
                c.spawn(text(head, display(64.0), if r.player_pos == 1 { t.good } else { t.text }));
                for (i, (who, fin)) in r.order.iter().enumerate() {
                    let (name, car) = match who {
                        Who::Player => ("You".to_string(), field.as_ref().and_then(|f| f.entrants.iter().find(|e| e.who == Who::Player)).map_or(String::new(), |e| car_name(&e.car))),
                        Who::Rival(s) => race.sim.rivals.iter().find(|x| x.slot == *s).map_or(("?".into(), String::new()), |x| (x.name.to_string(), car_name(x.model))),
                    };
                    let time = match fin {
                        Finish::Dnf => "DNF".to_string(),
                        Finish::Time(x) if i == 0 => crate::meta::board::clock(*x),
                        Finish::Time(x) => format!("+{:.1}", x - winner),
                    };
                    let col = if *who == Who::Player { t.accent } else { t.text };
                    c.spawn((Node { column_gap: px(16), ..default() }, Pickable::IGNORE)).with_children(|row| {
                        row.spawn((text(format!("{}", i + 1), label(SIZE_LABEL), t.text_muted), Node { width: px(24), ..default() }));
                        row.spawn((text(name, body_bold(SIZE_BODY), col), Node { width: px(170), ..default() }));
                        row.spawn((text(car, body(SIZE_SMALL), t.text_muted), Node { width: px(160), ..default() }));
                        row.spawn(text(time, label(SIZE_LABEL), col));
                    });
                }
                c.spawn((Node { height: px(10), ..default() }, Pickable::IGNORE));
                for (what, n) in &payout.0 {
                    let total = what == "Total";
                    let size = if total { body_bold(SIZE_BODY) } else { body(SIZE_SMALL) };
                    c.spawn((Node { column_gap: px(16), justify_content: JustifyContent::SpaceBetween, ..default() }, Pickable::IGNORE)).with_children(|row| {
                        row.spawn(text(what.clone(), size.clone(), if total { t.text } else { t.text_muted }));
                        if *n > 0 || total {
                            row.spawn(text(format!("+{} N", notes(*n)), size, if total { t.good } else { t.text }));
                        }
                    });
                }
                c.spawn(text(format!("Balance {} N", notes(wallet.balance)), label(SIZE_LABEL), t.text));
                c.spawn((Node { height: px(6), ..default() }, Pickable::IGNORE));
                c.spawn(text("Enter  Retry      F  Free roam", label(SIZE_TINY + 1.0), t.text_dim));
            });
        });
}

fn car_name(id: &str) -> String {
    crate::career::garage::def(id).map_or(id.to_string(), |d| d.name.to_string())
}

/// Enter: Retry (same field, no new fee); F: back to free roam.
fn results_keys(world: &mut World) {
    if !world.resource::<RaceRun>().done || world.get_resource::<engine::ui::prelude::Screens>().is_some_and(|s| s.top().is_some()) {
        return;
    }
    let Some(keys) = world.get_resource::<ButtonInput<KeyCode>>() else { return };
    let (retry, roam) = (keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter), keys.just_pressed(KeyCode::KeyF));
    if retry {
        crate::meta::restart_stage(world);
    } else if roam {
        crate::open::events::leave(world);
    }
}

fn despawn_all(mut commands: Commands, q: Query<Entity, Or<(With<RaceHud>, With<ResultsCard>)>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}
