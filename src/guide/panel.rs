//! The guide's info panel (right of the map): link status, stage clock, speed, progress,
//! the next call in big type, the next [`NOTES`] notes counting down, splits. Plus the map
//! overlays: waiting / link lost messages, countdown, scale bar, controls hint.
//!
//! Text is only rewritten when what it shows changes (format into a scratch string, compare).

use std::fmt::Write;

use bevy::prelude::*;
use engine::ui::theme::{display, label};

use super::map::{CREST, MapView, NOTES, PANEL_PX, grade_color};
use super::{GuideFeed, GuideStage, GuideView, Note, upcoming, view_wanted};
use crate::net_session::Session;
use crate::stage::Turn;

const PANEL_BG: Color = Color::srgba(0.085, 0.1, 0.115, 0.96);
const LINE: Color = Color::srgba(1.0, 1.0, 1.0, 0.08);
const TEXT: Color = Color::srgb(0.92, 0.93, 0.95);
const DIM: Color = Color::srgb(0.55, 0.58, 0.62);
const GOOD: Color = Color::srgb(0.4, 0.85, 0.5);
const WARN: Color = Color::srgb(0.98, 0.72, 0.2);
const BAD: Color = Color::srgb(0.95, 0.35, 0.35);

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum T {
    Stage,
    Status,
    Clock,
    Speed,
    Gear,
    Km,
    NextDist,
    NextCall,
    NextMods,
    RowDist(usize),
    RowCall(usize),
    RowMods(usize),
    Split(usize),
    Overlay,
    Countdown,
    Scale,
}

#[derive(Component)]
struct ProgressFill;

#[derive(Component)]
struct ScaleBar;

pub(super) fn build(app: &mut App) {
    app.add_systems(Update, (spawn.run_if(no_panel), update).chain().run_if(view_wanted).after(super::map::MapSystems));
}

#[derive(Component)]
struct PanelRoot;

fn no_panel(q: Query<(), With<PanelRoot>>) -> bool {
    q.is_empty()
}

fn t(s: &str, font: TextFont, color: Color, tag: T) -> impl Bundle {
    (Text::new(s), font, TextColor(color), tag)
}

fn eyebrow(p: &mut ChildSpawnerCommands, s: &str) {
    p.spawn((Text::new(s), label(14.0), TextColor(DIM), Node { margin: UiRect::top(Val::Px(6.0)), ..default() }));
}

fn rule(p: &mut ChildSpawnerCommands) {
    p.spawn((Node { height: Val::Px(1.0), width: Val::Percent(100.0), margin: UiRect::vertical(Val::Px(4.0)), ..default() }, BackgroundColor(LINE)));
}

fn spawn(mut commands: Commands) {
    // Right panel.
    commands
        .spawn((
            Name::new("guide panel"),
            GuideView,
            PanelRoot,
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(0.0),
                top: Val::Px(0.0),
                bottom: Val::Px(0.0),
                width: Val::Px(PANEL_PX),
                flex_direction: FlexDirection::Column,
                padding: UiRect::axes(Val::Px(28.0), Val::Px(24.0)),
                row_gap: Val::Px(6.0),
                border: UiRect::left(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(PANEL_BG),
            BorderColor::all(LINE),
        ))
        .with_children(|p| {
            p.spawn(t("GUIDE", label(14.0), DIM, T::Stage));
            p.spawn(t("", label(15.0), WARN, T::Status));
            rule(p);
            p.spawn(t("0:00.000", display(64.0), TEXT, T::Clock));
            p.spawn(Node { column_gap: Val::Px(18.0), align_items: AlignItems::Baseline, ..default() }).with_children(|r| {
                r.spawn(t("0", display(40.0), TEXT, T::Speed));
                r.spawn((Text::new("KM/H"), label(15.0), TextColor(DIM)));
                r.spawn(t("", display(28.0), DIM, T::Gear));
            });
            p.spawn((Node { width: Val::Percent(100.0), height: Val::Px(6.0), margin: UiRect::top(Val::Px(6.0)), ..default() }, BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.1))))
                .with_child((ProgressFill, Node { width: Val::Percent(0.0), height: Val::Percent(100.0), ..default() }, BackgroundColor(Color::srgb(1.0, 0.75, 0.25))));
            p.spawn(t("", label(15.0), DIM, T::Km));
            rule(p);
            eyebrow(p, "NEXT");
            p.spawn(Node { column_gap: Val::Px(16.0), align_items: AlignItems::Baseline, ..default() }).with_children(|r| {
                r.spawn(t("", display(54.0), TEXT, T::NextCall));
                r.spawn(t("", display(34.0), DIM, T::NextDist));
            });
            p.spawn(t("", label(18.0), CREST, T::NextMods));
            rule(p);
            eyebrow(p, "COMING UP");
            for i in 0..NOTES - 1 {
                p.spawn(Node { column_gap: Val::Px(14.0), align_items: AlignItems::Baseline, height: Val::Px(34.0), ..default() }).with_children(|r| {
                    r.spawn((t("", display(22.0), DIM, T::RowDist(i)), Node { width: Val::Px(62.0), ..default() }, TextLayout::justify(Justify::Right)));
                    r.spawn((t("", display(28.0), TEXT, T::RowCall(i)), Node { width: Val::Px(110.0), ..default() }));
                    r.spawn(t("", label(15.0), DIM, T::RowMods(i)));
                });
            }
            rule(p);
            eyebrow(p, "SPLITS");
            for i in 0..4 {
                p.spawn(t("", display(24.0), DIM, T::Split(i)));
            }
        });
    // Map overlays.
    commands.spawn((
        GuideView,
        t("", display(40.0), TEXT, T::Overlay),
        Node { position_type: PositionType::Absolute, left: Val::Px(0.0), right: Val::Px(PANEL_PX), top: Val::Percent(44.0), justify_content: JustifyContent::Center, ..default() },
        TextLayout::justify(Justify::Center),
    ));
    commands.spawn((
        GuideView,
        t("", display(120.0), Color::srgb(1.0, 0.85, 0.3), T::Countdown),
        Node { position_type: PositionType::Absolute, left: Val::Px(0.0), right: Val::Px(PANEL_PX), top: Val::Percent(12.0), justify_content: JustifyContent::Center, ..default() },
        TextLayout::justify(Justify::Center),
    ));
    commands
        .spawn((GuideView, Node { position_type: PositionType::Absolute, left: Val::Px(28.0), bottom: Val::Px(24.0), flex_direction: FlexDirection::Column, row_gap: Val::Px(6.0), ..default() }))
        .with_children(|p| {
            p.spawn(Node { column_gap: Val::Px(10.0), align_items: AlignItems::Center, ..default() }).with_children(|r| {
                r.spawn((ScaleBar, Node { width: Val::Px(100.0), height: Val::Px(4.0), ..default() }, BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.7))));
                r.spawn(t("100 m", label(15.0), DIM, T::Scale));
            });
            p.spawn((Text::new("WHEEL / + -  ZOOM    DRAG  PAN    F  FOLLOW    M  WHOLE STAGE    H  HEADING UP"), label(14.0), TextColor(Color::srgba(1.0, 1.0, 1.0, 0.4))));
        });
}

fn clock(out: &mut String, t: f32) {
    let m = (t / 60.0).floor();
    let _ = write!(out, "{}:{:06.3}", m as u32, t - m * 60.0);
}

fn call(out: &mut String, g: &GuideStage, n: &Note) -> Color {
    match n.corner {
        Some(k) => {
            let c = &g.corners[k];
            let _ = write!(out, "{} {}", if c.turn == Turn::Left { "LEFT" } else { "RIGHT" }, c.grade);
            grade_color(c.grade)
        }
        None => {
            out.push_str("CREST");
            CREST
        }
    }
}

fn mods(out: &mut String, g: &GuideStage, n: &Note) {
    let Some(k) = n.corner else { return };
    let c = &g.corners[k];
    let sep = |o: &mut String, s: &str| {
        if !o.is_empty() {
            o.push_str("  ");
        }
        o.push_str(s);
    };
    if n.into {
        sep(out, "INTO");
    }
    if c.end - c.start > 70.0 {
        sep(out, "LONG");
    }
    if c.crest {
        sep(out, "OVER CREST");
    }
    if c.narrows {
        sep(out, "NARROWS");
    }
}

fn dist(out: &mut String, d: f32) {
    if d <= 0.0 {
        out.push_str("NOW");
    } else {
        // Round like a co-driver: 10 m steps under 200, 50 above.
        let step = if d < 200.0 { 10.0 } else { 50.0 };
        let _ = write!(out, "{}", ((d / step).round() * step) as u32);
    }
}

/// Rewrites `text` only if `s` differs.
fn put(text: &mut Text, s: &str) {
    if text.0 != s {
        text.0.clear();
        text.0.push_str(s);
    }
}

#[allow(clippy::too_many_arguments)]
fn update(
    feed: Res<GuideFeed>,
    session: Res<Session>,
    stage: Option<Res<GuideStage>>,
    view: Res<MapView>,
    mut texts: Query<(&T, &mut Text, &mut TextColor)>,
    mut fill: Query<&mut Node, (With<ProgressFill>, Without<ScaleBar>)>,
    mut bar: Query<&mut Node, (With<ScaleBar>, Without<ProgressFill>)>,
    mut notes: Local<Vec<Note>>,
    mut s: Local<String>,
) {
    if let Some(g) = &stage {
        upcoming(g, feed.s, NOTES, &mut notes);
    } else {
        notes.clear();
    }
    let run = feed.run;
    let car = feed.car;
    for (tag, mut text, mut color) in &mut texts {
        s.clear();
        let mut c = color.0;
        match *tag {
            T::Stage => match &stage {
                Some(g) => {
                    let _ = write!(s, "STAGE {}  {}  {:.1} KM  {:?}", g.def.id, g.def.name.to_uppercase(), g.road.length() / 1000.0, g.def.surface);
                    s.make_ascii_uppercase();
                }
                None => s.push_str("GUIDE"),
            },
            T::Status => {
                if stage.as_ref().is_some_and(|g| !g.hash_ok) {
                    s.push_str("STAGE DIFFERS FROM THE DRIVER'S: UPDATE THE GAME");
                    c = BAD;
                } else if session.connected {
                    let _ = write!(s, "DRIVER CONNECTED  {:.0} MS", session.rtt_ms);
                    c = GOOD;
                } else if session.was_connected {
                    s.push_str("LINK LOST  RECONNECTING");
                    c = BAD;
                } else {
                    s.push_str("CONNECTING TO ");
                    if let Some(a) = session.addr {
                        let _ = write!(s, "{a}");
                    }
                    c = WARN;
                }
            }
            T::Clock => clock(&mut s, feed.clock),
            T::Speed => {
                let _ = write!(s, "{:.0}", car.map_or(0.0, |c| c.speed().abs() * 3.6));
            }
            T::Gear => {
                if let Some(car) = car {
                    if car.gear < 0 {
                        s.push_str("R");
                    } else {
                        let _ = write!(s, "GEAR {}", car.gear);
                    }
                }
            }
            T::Km => {
                if let Some(g) = &stage {
                    let _ = write!(s, "{:.2} / {:.2} KM", feed.s.max(0.0) / 1000.0, g.road.length() / 1000.0);
                    if run.penalty > 0.0 {
                        let _ = write!(s, "    PENALTY +{:.0} S", run.penalty);
                    }
                }
            }
            T::NextCall | T::NextDist | T::NextMods => {
                if let (Some(g), Some(n)) = (&stage, notes.first()) {
                    match *tag {
                        T::NextCall => c = call(&mut s, g, n),
                        T::NextDist => {
                            dist(&mut s, n.at - feed.s);
                            if n.at > feed.s {
                                s.push_str(" M");
                            }
                        }
                        _ => mods(&mut s, g, n),
                    }
                } else if *tag == T::NextCall && stage.is_some() {
                    s.push_str("FINISH");
                    c = TEXT;
                }
            }
            T::RowDist(i) | T::RowCall(i) | T::RowMods(i) => {
                if let (Some(g), Some(n)) = (&stage, notes.get(i + 1)) {
                    match *tag {
                        T::RowDist(_) => dist(&mut s, n.at - feed.s),
                        T::RowCall(_) => {
                            let col = call(&mut s, g, n);
                            // Further notes fade a little.
                            c = col.with_alpha(1.0 - i as f32 * 0.07);
                        }
                        _ => mods(&mut s, g, n),
                    }
                }
            }
            T::Split(i) => {
                let v = if i < 3 { run.splits().get(i).copied() } else { run.finished };
                if i < 3 {
                    let _ = write!(s, "S{}        ", i + 1);
                } else {
                    s.push_str("FINISH  ");
                }
                match v {
                    Some(v) => {
                        clock(&mut s, v);
                        c = if i < 3 { TEXT } else { GOOD };
                    }
                    None => {
                        s.push_str("-:--.---");
                        c = DIM;
                    }
                }
            }
            T::Overlay => {
                if stage.is_none() {
                    if session.connected {
                        s.push_str("THE DRIVER IS CHOOSING A STAGE");
                    } else {
                        s.push_str("WAITING FOR THE DRIVER");
                        if let Some(a) = session.addr {
                            let _ = write!(s, "\n{a}");
                        }
                    }
                } else if !session.connected && session.was_connected {
                    s.push_str("LINK LOST  RECONNECTING");
                    c = BAD;
                } else {
                    c = TEXT;
                }
            }
            T::Countdown => {
                if session.connected && stage.is_some() {
                    if run.countdown > 0.0 {
                        let _ = write!(s, "{}", run.countdown.ceil() as u32);
                    } else if run.finished.is_none() && run.time < 1.0 && car.is_some() {
                        s.push_str("GO");
                    }
                }
            }
            T::Scale => {
                let (m, _) = scale_bar(view.scale);
                if m >= 1000.0 {
                    let _ = write!(s, "{:.0} KM", m / 1000.0);
                } else {
                    let _ = write!(s, "{m:.0} M");
                }
            }
        }
        put(&mut text, &s);
        if color.0 != c {
            color.0 = c;
        }
    }
    if let (Ok(mut n), Some(g)) = (fill.single_mut(), &stage) {
        let w = Val::Percent((feed.s / g.road.length()).clamp(0.0, 1.0) * 100.0);
        if n.width != w {
            n.width = w;
        }
    }
    if let Ok(mut n) = bar.single_mut() {
        let w = Val::Px(scale_bar(view.scale).1);
        if n.width != w {
            n.width = w;
        }
    }
}

/// A round distance whose bar is 70–180 px at `scale` m/px: (metres, pixels).
fn scale_bar(scale: f32) -> (f32, f32) {
    for m in [10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0] {
        if m / scale >= 70.0 {
            return (m, m / scale);
        }
    }
    (5000.0, 5000.0 / scale)
}
