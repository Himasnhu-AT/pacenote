//! Driver HUD in the game's UI theme (Barlow Condensed, map-paper text, rally-plate yellow):
//!
//! - bottom right: rev bar (segments light up to the red line), speed in km/h or mph, gear;
//! - bottom left: stage name, stage clock (+ penalties), progress bar with km done / total;
//! - top right: damage per part, once the car is hurt;
//! - centre: countdown, GO, FINISH with the time, and the RETIRED, NOT DRIVABLE and CAR
//!   WRECKED (one big crash) plates.
//!
//! The controls card, the event-start prompt, the roof hint and short notices are in
//! `meta::coach`.
//!
//! Festival additions (stream E, `docs/festival/UX_PASS.md` §3.4-§3.5): the gear box border
//! pulses accent ↔ white at 2 Hz at the shift point (steady with reduce motion), and in free
//! roam a Notes chip top right shows the balance and the change for 4 s after it moves.
//!
//! Each readout can be turned off in Settings > HUD ([`crate::prefs::Prefs`]). Cheap: values
//! are cached and text / colours / widths are only written when they change (no per-frame
//! formatting or allocation while nothing changes).

use std::fmt::Write;

use bevy::prelude::*;
use engine::ui::theme::*;

use crate::drive::{CarSpec, DrivenCar, Down, StageRun};
use crate::prefs::Prefs;
use crate::world::StageWorld;

/// Segments in the rev bar.
pub const REV_SEGMENTS: usize = 24;

#[derive(Component)]
struct HudRoot;

/// The gear box (its border pulses at the shift point).
#[derive(Component)]
struct GearBox;

/// The Notes chip (top right, free roam) and its two texts.
#[derive(Component)]
struct NotesChip;
#[derive(Component)]
struct NotesValue;
#[derive(Component)]
struct NotesDelta;

/// How long the Notes chip stays after a change (s).
const NOTES_CHIP_S: f32 = 4.0;
/// The shift point as a share of the redline.
const SHIFT_AT: f32 = 0.93;

/// Which value a text shows.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum HudText {
    Speed,
    Unit,
    Gear,
    Stage,
    Clock,
    Penalty,
    Progress,
    CenterBig,
    CenterSmall,
    DeadNote,
}

/// A group hidden by a HUD setting.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum HudGroup {
    Speed,
    Revs,
    Clock,
    Progress,
    Damage,
    Retired,
    Wrecked,
    Dead,
}

#[derive(Component)]
struct RevSeg(usize);

#[derive(Component)]
struct ProgressFill;

/// Damage bar fill / percentage of part 0 engine, 1 steering, 2 suspension.
#[derive(Component)]
struct DamageFill(usize);
#[derive(Component)]
struct DamagePct(usize);

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (spawn_hud, update_hud, shift_pulse, notes_chip, hide_under_screens).chain().run_if(resource_exists::<StageWorld>));
    }
}

/// The HUD hides while a screen (pause, results, settings) is over the game: the results'
/// own big time and the HUD's FINISH time no longer show on top of each other.
fn hide_under_screens(screens: Option<Res<engine::ui::prelude::Screens>>, mut q: Query<&mut Visibility, With<HudRoot>>) {
    let want = if screens.is_some_and(|s| s.top().is_some()) { Visibility::Hidden } else { Visibility::Inherited };
    for mut v in &mut q {
        if *v != want {
            *v = want;
        }
    }
}

/// Rev bar colours: paper white, then accent, then red near the limiter.
fn seg_color(t: &Theme, i: usize) -> Color {
    let f = i as f32 / REV_SEGMENTS as f32;
    if f < 0.62 {
        t.text
    } else if f < 0.86 {
        t.accent
    } else {
        t.bad
    }
}

fn seg_off() -> Color {
    Color::srgba(0.95, 0.93, 0.88, 0.13)
}

/// The rev bar (for the HUD and the Settings preview), `lit` = 0..1.
pub fn rev_bar_preview(p: &mut ChildSpawnerCommands, t: &Theme, lit: f32) {
    let n = (lit * REV_SEGMENTS as f32).round() as usize;
    p.spawn((Node { column_gap: px(3), ..default() }, Pickable::IGNORE)).with_children(|r| {
        for i in 0..REV_SEGMENTS {
            r.spawn((Node { width: px(9), height: px(14), ..default() }, BackgroundColor(if i < n { seg_color(t, i) } else { seg_off() }), Pickable::IGNORE));
        }
    });
}

fn plate() -> BackgroundColor {
    BackgroundColor(Color::srgba(0.02, 0.03, 0.025, 0.55))
}

fn spawn_hud(mut commands: Commands, theme: Res<Theme>, q: Query<(), With<HudRoot>>) {
    if !q.is_empty() {
        return;
    }
    let t = &*theme;
    commands
        .spawn((HudRoot, Name::new("HUD"), DespawnOnExit(engine::flow::AppState::InGame), fullscreen(), Pickable::IGNORE))
        .with_children(|root| {
            // Bottom right: revs, speed, gear.
            root.spawn((
                Node { position_type: PositionType::Absolute, right: px(40), bottom: px(34), flex_direction: FlexDirection::Column, align_items: AlignItems::FlexEnd, row_gap: px(4), padding: UiRect::new(px(18), px(18), px(12), px(8)), border_radius: BorderRadius::all(px(RADIUS)), ..default() },
                plate(),
                Pickable::IGNORE,
            ))
            .with_children(|c| {
                c.spawn((HudGroup::Revs, Node { column_gap: px(4), margin: UiRect::bottom(px(2)), ..default() }, Pickable::IGNORE)).with_children(|r| {
                    for i in 0..REV_SEGMENTS {
                        r.spawn((RevSeg(i), Node { width: px(11), height: px(18), ..default() }, BackgroundColor(seg_off()), Pickable::IGNORE));
                    }
                });
                c.spawn((HudGroup::Speed, Node { column_gap: px(14), align_items: AlignItems::Center, ..default() }, Pickable::IGNORE)).with_children(|r| {
                    r.spawn((HudText::Speed, text("0", display(92.0), t.text), Node { min_width: px(150), justify_content: JustifyContent::FlexEnd, ..default() }, TextLayout::justify(Justify::Right)));
                    r.spawn((Node { flex_direction: FlexDirection::Column, row_gap: px(0), ..default() }, Pickable::IGNORE)).with_children(|u| {
                        u.spawn((HudText::Unit, text("KM/H", label(SIZE_LABEL), t.text_muted)));
                        u.spawn(text("GEAR", label(SIZE_TINY), t.text_dim));
                    });
                    r.spawn((
                        GearBox,
                        Node { width: px(78), height: px(92), justify_content: JustifyContent::Center, align_items: AlignItems::Center, border: UiRect::all(px(2)), border_radius: BorderRadius::all(px(RADIUS)), ..default() },
                        BorderColor::all(t.accent),
                        Pickable::IGNORE,
                    ))
                    .with_children(|g| {
                        g.spawn((HudText::Gear, text("1", display(76.0), t.accent)));
                    });
                });
            });
            // Top right: the Notes chip (free roam, after a change).
            root.spawn((
                NotesChip,
                Node { position_type: PositionType::Absolute, right: px(40), top: px(34), column_gap: px(10), align_items: AlignItems::Baseline, padding: UiRect::new(px(16), px(16), px(6), px(8)), border_radius: BorderRadius::all(px(RADIUS)), display: Display::None, ..default() },
                plate(),
                Pickable::IGNORE,
            ))
            .with_children(|c| {
                c.spawn((NotesValue, text("0", display(40.0), t.text)));
                c.spawn(text("N", label(SIZE_LABEL + 4.0), t.text_muted));
                c.spawn((NotesDelta, text("", body_bold(SIZE_BODY + 2.0), t.good)));
            });
            // Bottom left: stage, clock, progress.
            root.spawn((
                Node { position_type: PositionType::Absolute, left: px(40), bottom: px(34), flex_direction: FlexDirection::Column, row_gap: px(4), padding: UiRect::new(px(18), px(18), px(12), px(14)), border_radius: BorderRadius::all(px(RADIUS)), ..default() },
                plate(),
                Pickable::IGNORE,
            ))
            .with_children(|c| {
                c.spawn((HudText::Stage, text("", label(SIZE_LABEL), t.accent)));
                c.spawn((HudGroup::Clock, Node { column_gap: px(12), align_items: AlignItems::Baseline, ..default() }, Pickable::IGNORE)).with_children(|r| {
                    r.spawn((HudText::Clock, text("0:00.000", display(56.0), t.text)));
                    r.spawn((HudText::Penalty, text("", body_bold(SIZE_BODY), t.bad)));
                });
                c.spawn((HudGroup::Progress, Node { flex_direction: FlexDirection::Column, row_gap: px(6), margin: UiRect::top(px(2)), ..default() }, Pickable::IGNORE)).with_children(|p| {
                    p.spawn((Node { width: px(360), height: px(4), ..default() }, BackgroundColor(seg_off()), Pickable::IGNORE)).with_children(|b| {
                        b.spawn((ProgressFill, Node { width: percent(0), height: percent(100), ..default() }, BackgroundColor(t.accent), Pickable::IGNORE));
                    });
                    p.spawn((HudText::Progress, text("", label(SIZE_TINY + 1.0), t.text_muted)));
                });
            });
            // Top right: damage.
            root.spawn((
                HudGroup::Damage,
                Node { position_type: PositionType::Absolute, right: px(40), top: px(96), flex_direction: FlexDirection::Column, row_gap: px(6), padding: UiRect::all(px(14)), border: UiRect::left(px(3)), border_radius: BorderRadius::all(px(RADIUS)), display: Display::None, ..default() },
                plate(),
                BorderColor::all(t.bad),
                Pickable::IGNORE,
            ))
            .with_children(|c| {
                c.spawn(text("DAMAGE", label(SIZE_LABEL), t.bad));
                for (k, name) in ["Engine", "Steering", "Suspension"].into_iter().enumerate() {
                    c.spawn((Node { column_gap: px(10), align_items: AlignItems::Center, ..default() }, Pickable::IGNORE)).with_children(|r| {
                        r.spawn((text(name.to_uppercase(), label(SIZE_TINY + 1.0), t.text_muted), Node { width: px(96), ..default() }));
                        r.spawn((Node { width: px(150), height: px(6), ..default() }, BackgroundColor(seg_off()), Pickable::IGNORE)).with_children(|b| {
                            b.spawn((DamageFill(k), Node { width: percent(0), height: percent(100), ..default() }, BackgroundColor(t.text), Pickable::IGNORE));
                        });
                        r.spawn((DamagePct(k), text("0%", label(SIZE_TINY + 1.0), t.text), Node { width: px(44), ..default() }, TextLayout::justify(Justify::Right)));
                    });
                }
            });
            // Centre: countdown / GO / FINISH.
            root.spawn((Node { position_type: PositionType::Absolute, top: percent(24), width: percent(100), flex_direction: FlexDirection::Column, align_items: AlignItems::Center, ..default() }, Pickable::IGNORE)).with_children(|c| {
                c.spawn((HudText::CenterSmall, text("", label(SIZE_LABEL + 4.0), t.accent)));
                c.spawn((HudText::CenterBig, text("", display(150.0), t.accent), TextShadow::default()));
            });
            // Centre: retired.
            root.spawn((
                HudGroup::Retired,
                Node { position_type: PositionType::Absolute, top: percent(30), left: percent(50), width: px(560), margin: UiRect::left(px(-280)), justify_content: JustifyContent::Center, display: Display::None, ..default() },
                Pickable::IGNORE,
            ))
            .with_children(|w| {
                w.spawn(accent_panel(t, t.bad, 26.0, GAP_S)).with_children(|c| {
                    c.spawn(eyebrow("Stage over", t.bad));
                    c.spawn(title("Retired", SIZE_H1, t.text));
                    c.spawn(text("The car is too damaged to continue.", body(SIZE_BODY), t.text_muted));
                    c.spawn(spacer(GAP_XS));
                    c.spawn((Node { column_gap: px(GAP_L), ..default() }, Pickable::IGNORE)).with_children(|k| {
                        key_hint(k, t, "Esc", "Restart or leave");
                    });
                });
            });
            // Centre: the car is broken (Simulation damage), waiting for a recover.
            root.spawn((
                HudGroup::Wrecked,
                Node { position_type: PositionType::Absolute, top: percent(30), left: percent(50), width: px(560), margin: UiRect::left(px(-280)), justify_content: JustifyContent::Center, display: Display::None, ..default() },
                Pickable::IGNORE,
            ))
            .with_children(|w| {
                w.spawn(accent_panel(t, t.bad, 26.0, GAP_S)).with_children(|c| {
                    c.spawn(eyebrow("Damage 100 %", t.bad));
                    c.spawn(title("Car no longer drivable", SIZE_H1, t.text));
                    c.spawn(text("Recover to put it back on the road here, fully repaired.", body(SIZE_BODY), t.text_muted));
                    c.spawn(spacer(GAP_XS));
                    c.spawn((Node { column_gap: px(GAP_L), ..default() }, Pickable::IGNORE)).with_children(|k| {
                        key_hint(k, t, "R", "Recover");
                        key_hint(k, t, "Esc", "Pause");
                    });
                });
            });
            // Centre: one crash took half the car or more.
            root.spawn((
                HudGroup::Dead,
                Node { position_type: PositionType::Absolute, top: percent(30), left: percent(50), width: px(560), margin: UiRect::left(px(-280)), justify_content: JustifyContent::Center, display: Display::None, ..default() },
                Pickable::IGNORE,
            ))
            .with_children(|w| {
                w.spawn(accent_panel(t, t.bad, 26.0, GAP_S)).with_children(|c| {
                    c.spawn(eyebrow("Big crash", t.bad));
                    c.spawn(title("Car wrecked", SIZE_H1, t.text));
                    c.spawn((HudText::DeadNote, text("", body(SIZE_BODY), t.text_muted)));
                });
            });
        });
}

/// What the HUD last showed (only changes are written).
#[derive(Default)]
struct Shown {
    speed: Option<i32>,
    mph: Option<bool>,
    gear: Option<i8>,
    lit: Option<usize>,
    clock_ms: Option<u32>,
    penalty: Option<u32>,
    progress: Option<(u32, u32)>,
    stage: Option<u32>,
    damage: Option<[u32; 3]>,
    center: Option<(u8, u32)>,
    groups: Option<[bool; 8]>,
    dead_route: Option<bool>,
}

fn set_text(t: &mut Text, f: impl FnOnce(&mut String)) {
    t.0.clear();
    f(&mut t.0);
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn update_hud(
    run: Res<StageRun>,
    stage: Res<StageWorld>,
    spec: Res<CarSpec>,
    prefs: Option<Res<Prefs>>,
    theme: Res<Theme>,
    cars: Query<&DrivenCar>,
    mut texts: Query<(&HudText, &mut Text, &mut TextColor)>,
    mut groups: Query<(&HudGroup, &mut Node), (Without<RevSeg>, Without<ProgressFill>, Without<DamageFill>)>,
    mut segs: Query<(&RevSeg, &mut BackgroundColor), Without<DamageFill>>,
    mut fill: Query<&mut Node, (With<ProgressFill>, Without<HudGroup>, Without<DamageFill>)>,
    mut dmg: Query<(&DamageFill, &mut Node, &mut BackgroundColor), (Without<HudGroup>, Without<ProgressFill>, Without<RevSeg>)>,
    mut pct: Query<(&DamagePct, &mut Text), Without<HudText>>,
    mut shown: Local<Shown>,
) {
    let Ok(c) = cars.single() else { return };
    let car = &c.car;
    let t = &*theme;
    let prefs = prefs.map(|p| p.clone()).unwrap_or_default();
    let d = car.damage;
    // Damage only shows in Simulation (Cosmetic and None drive as new).
    let damaged = d.worst() > 0.02 && spec.0.damage == crate::vehicle::DamageMode::Simulation;

    // Groups on / off (settings, damage, retired).
    // Free roam has no route: no clock or progress.
    let route = !stage.free_roam();
    let want = [prefs.hud_speed, prefs.hud_revs, prefs.hud_clock && route, prefs.hud_progress && route, prefs.hud_damage && damaged, run.retired, run.down == Some(Down::Wrecked), matches!(run.down, Some(Down::Dead(_)))];
    if shown.groups != Some(want) {
        shown.groups = Some(want);
        for (g, mut n) in &mut groups {
            let on = want[*g as usize];
            n.display = if on { Display::Flex } else { Display::None };
        }
    }

    // Speed, units, gear.
    let (v, unit) = prefs.speed(car.speed());
    let v = v.round() as i32;
    let changed_units = shown.mph != Some(prefs.mph);
    let speed_changed = shown.speed != Some(v);
    let gear_changed = shown.gear != Some(car.gear);
    // Revs.
    let span = (spec.0.redline - spec.0.idle_rpm).max(1.0);
    let lit = (((car.rpm - spec.0.idle_rpm) / span).clamp(0.0, 1.0) * REV_SEGMENTS as f32).round() as usize;
    if shown.lit != Some(lit) {
        shown.lit = Some(lit);
        for (s, mut bg) in &mut segs {
            let c = if s.0 < lit { seg_color(t, s.0) } else { seg_off() };
            if bg.0 != c {
                bg.0 = c;
            }
        }
    }
    // Clock (ms), penalty, progress.
    let total = run.time + run.penalty;
    let ms = (total.max(0.0) * 1000.0).round() as u32;
    let pen = run.penalty.round() as u32;
    let len = stage.road.length();
    let prog = ((run.progress / 10.0).round() as u32, (len / 10.0).round() as u32);
    let prog_changed = shown.progress != Some(prog);
    if prog_changed {
        shown.progress = Some(prog);
        for mut n in &mut fill {
            n.width = percent((run.progress / len.max(1.0) * 100.0).clamp(0.0, 100.0));
        }
    }
    // Damage.
    let parts = [d.engine, d.steering, d.suspension];
    let pcts = parts.map(|x| (x * 100.0).round() as u32);
    if shown.damage != Some(pcts) {
        shown.damage = Some(pcts);
        for (f, mut n, mut bg) in &mut dmg {
            let x = parts[f.0];
            n.width = percent(x * 100.0);
            bg.0 = if x < 0.35 { t.text } else if x < 0.7 { t.accent } else { t.bad };
        }
        for (p, mut txt) in &mut pct {
            set_text(&mut txt, |s| {
                let _ = write!(s, "{}%", pcts[p.0]);
            });
        }
    }
    // Centre: 1 countdown, 2 GO, 3 finish; 0 nothing.
    let center = if run.retired || run.down.is_some() {
        (0, 0)
    } else if run.countdown > 0.0 {
        (1, run.countdown.ceil() as u32)
    } else if let Some(f) = run.finished {
        (3, (f * 1000.0).round() as u32)
    } else if run.time < 1.0 {
        (2, 0)
    } else {
        (0, 0)
    };
    let center_changed = shown.center != Some(center);
    let clock_changed = shown.clock_ms != Some(ms) || shown.penalty != Some(pen);
    let stage_changed = shown.stage != Some(stage.route_id());

    for (kind, mut txt, mut col) in &mut texts {
        match kind {
            HudText::Speed if speed_changed => set_text(&mut txt, |s| {
                let _ = write!(s, "{v}");
            }),
            HudText::Unit if changed_units => set_text(&mut txt, |s| s.push_str(unit)),
            HudText::Gear if gear_changed => set_text(&mut txt, |s| {
                if car.gear < 0 {
                    s.push('R');
                } else if car.gear == 0 {
                    s.push('N');
                } else {
                    let _ = write!(s, "{}", car.gear);
                }
            }),
            HudText::Stage if stage_changed => set_text(&mut txt, |s| {
                match (&stage.event, stage.in_world()) {
                    (Some(e), _) => {
                        let _ = write!(s, "{}", e.name.to_uppercase());
                    }
                    (None, true) => s.push_str("FREE ROAM"),
                    (None, false) => {
                        let _ = write!(s, "{:02}  {}", stage.def.id, stage.def.name.to_uppercase());
                    }
                }
            }),
            HudText::Clock if clock_changed => set_text(&mut txt, |s| clock_into(s, total)),
            HudText::Penalty if clock_changed => set_text(&mut txt, |s| {
                if pen > 0 {
                    let _ = write!(s, "+{pen} s");
                }
            }),
            HudText::Progress if prog_changed => set_text(&mut txt, |s| {
                let _ = write!(s, "{:.2} / {:.2} KM", prog.0 as f32 / 100.0, prog.1 as f32 / 100.0);
            }),
            HudText::CenterBig if center_changed => {
                col.0 = t.accent;
                set_text(&mut txt, |s| match center.0 {
                    1 => {
                        let _ = write!(s, "{}", center.1);
                    }
                    2 => s.push_str("GO"),
                    3 => clock_into(s, center.1 as f32 / 1000.0),
                    _ => {}
                });
            }
            HudText::CenterSmall if center_changed => set_text(&mut txt, |s| {
                if center.0 == 3 {
                    s.push_str("FINISH");
                }
            }),
            HudText::DeadNote if shown.dead_route != Some(route) => set_text(&mut txt, |s| {
                s.push_str(if route { "Run over. Back to the start line\u{2026}" } else { "Towing you home\u{2026}" });
            }),
            _ => {}
        }
    }
    shown.speed = Some(v);
    shown.mph = Some(prefs.mph);
    shown.gear = Some(car.gear);
    shown.clock_ms = Some(ms);
    shown.penalty = Some(pen);
    shown.stage = Some(stage.route_id());
    shown.center = Some(center);
    shown.dead_route = Some(route);
}

/// `83.4` → "1:23.400".
/// The gear box border: accent ↔ white at 2 Hz at the shift point; accent otherwise (and
/// always with reduce motion).
fn shift_pulse(
    time: Res<Time<Real>>,
    spec: Res<CarSpec>,
    theme: Res<Theme>,
    a11y: Option<Res<engine::actions::Accessibility>>,
    cars: Query<&DrivenCar>,
    mut boxes: Query<&mut BorderColor, With<GearBox>>,
) {
    let Ok(c) = cars.single() else { return };
    let at_shift = c.car.gear > 0 && c.car.rpm >= spec.0.redline * SHIFT_AT && !a11y.is_some_and(|a| a.reduce_motion);
    let want = if at_shift {
        let k = (time.elapsed_secs() * std::f32::consts::TAU * 2.0).sin() * 0.5 + 0.5;
        engine::ui::widgets::mix(theme.accent, theme.text, k)
    } else {
        theme.accent
    };
    for mut b in &mut boxes {
        if b.top != want {
            *b = BorderColor::all(want);
        }
    }
}

/// The Notes chip: in free roam, for 4 s after the balance changes ("12,400 N +750").
#[allow(clippy::type_complexity)]
fn notes_chip(
    time: Res<Time<Real>>,
    stage: Res<StageWorld>,
    wallet: Option<Res<crate::career::wallet::Wallet>>,
    theme: Res<Theme>,
    mut chip: Query<&mut Node, With<NotesChip>>,
    mut value: Query<&mut Text, (With<NotesValue>, Without<NotesDelta>)>,
    mut delta: Query<(&mut Text, &mut TextColor), (With<NotesDelta>, Without<NotesValue>)>,
    mut last: Local<Option<(u32, f32)>>,
) {
    let Some(w) = wallet else { return };
    let now = time.elapsed_secs();
    let before = last.map(|(b, _)| b);
    if before != Some(w.balance) {
        // The first look only notes the balance; a change shows the chip.
        if let Some(b) = before {
            let d = w.balance as i64 - b as i64;
            for mut t in &mut value {
                t.0 = crate::meta::hub::thousands(w.balance);
            }
            for (mut t, mut c) in &mut delta {
                t.0 = if d >= 0 { format!("+{}", crate::meta::hub::thousands(d as u32)) } else { format!("\u{2212}{}", crate::meta::hub::thousands((-d) as u32)) };
                c.0 = if d >= 0 { theme.good } else { theme.bad };
            }
            *last = Some((w.balance, now));
        } else {
            *last = Some((w.balance, f32::NEG_INFINITY));
        }
    }
    let since = last.map_or(f32::NEG_INFINITY, |(_, t)| t);
    let show = stage.free_roam() && stage.event.is_none() && now - since < NOTES_CHIP_S;
    let want = if show { Display::Flex } else { Display::None };
    for mut n in &mut chip {
        if n.display != want {
            n.display = want;
        }
    }
}

fn clock_into(s: &mut String, t: f32) {
    let t = t.max(0.0);
    let m = (t / 60.0).floor();
    let _ = write!(s, "{}:{:06.3}", m as u32, t - m * 60.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_format() {
        let mut s = String::new();
        clock_into(&mut s, 83.4);
        assert_eq!(s, "1:23.400");
        s.clear();
        clock_into(&mut s, 5.05);
        assert_eq!(s, "0:05.050");
    }

    #[test]
    fn rev_bar_colours_run_white_amber_red() {
        let t = crate::meta::screens::theme();
        assert_eq!(seg_color(&t, 0), t.text);
        assert_eq!(seg_color(&t, REV_SEGMENTS * 3 / 4), t.accent);
        assert_eq!(seg_color(&t, REV_SEGMENTS - 1), t.bad);
    }
}
