//! The guide's info panel (right of the map): link status, stage clock, speed, progress,
//! the next call in big type, the next [`NOTES`] notes counting down, splits. Plus the map
//! overlays: waiting / link lost messages, countdown, scale bar, controls hint.
//!
//! Text is only rewritten when what it shows changes (format into a scratch string, compare).
//!
//! On the in-car tablet ([`GuideScreen::InCar`]) the same panel renders into the tablet's
//! texture (`UiTargetCamera` = the tablet camera), with fewer rows so the type stays big. Under
//! the next call sits the call-timing cue (`coach.rs`): a bar filling as the note comes into
//! reach, its green window the time to call, and the word for the last call.

use std::fmt::Write;

use bevy::prelude::*;
use engine::ui::theme::{display, label};

use bevy::ui::UiTargetCamera;

use super::coach::{self, Grade, GuideCalls};
use super::map::{CREST, MapCamera, MapView, NOTES, grade_color, panel_px};
use super::{GuideFeed, GuideScreen, GuideStage, GuideView, Note, upcoming, view_wanted};
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
    Cue,
    CallGrade,
    Calls,
    Damage,
}

/// The car's damage on the panel, coloured like the driver's HUD bars (under 35 % plain, under
/// 70 % warning, then bad); empty while the car is whole.
fn damage(out: &mut String, d: &super::sync::NetDamage) -> Color {
    let parts = d.parts();
    let worst = d.worst();
    if worst <= 0.0 && d.detached == 0 {
        return TEXT;
    }
    out.push_str("DAMAGE");
    for (name, v) in ["ENGINE", "STEERING", "SUSPENSION"].into_iter().zip(parts) {
        if v > 0.0 {
            let _ = write!(out, "  {name} {:.0}%", v * 100.0);
        }
    }
    let off = d.detached.count_ones();
    if off > 0 {
        let _ = write!(out, "  {off} PART{} OFF", if off == 1 { "" } else { "S" });
    }
    if worst >= 0.7 {
        BAD
    } else if worst >= 0.35 {
        WARN
    } else {
        TEXT
    }
}

/// Rows in the coming-up list on this screen.
fn rows(screen: GuideScreen) -> usize {
    match screen {
        GuideScreen::Map => NOTES - 1,
        GuideScreen::InCar => 4,
    }
}

#[derive(Component)]
struct CueFill;

#[derive(Component)]
struct CueWindow;

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

fn spawn(mut commands: Commands, screen: Res<GuideScreen>, cams: Query<Entity, With<MapCamera>>) {
    let screen = *screen;
    // On the tablet everything goes to its camera (spawned by the map first).
    let target = match screen {
        GuideScreen::Map => None,
        GuideScreen::InCar => match cams.iter().next() {
            Some(c) => Some(UiTargetCamera(c)),
            None => return,
        },
    };
    let panel_w = panel_px(screen);
    let n_rows = rows(screen);
    // Right panel.
    let mut root = commands.spawn_empty();
    if let Some(t) = &target {
        root.insert(t.clone());
    }
    root
        .insert((
            Name::new("guide panel"),
            GuideView,
            PanelRoot,
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(0.0),
                top: Val::Px(0.0),
                bottom: Val::Px(0.0),
                width: Val::Px(panel_w),
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
            p.spawn(t("", label(16.0), TEXT, T::Damage));
            rule(p);
            eyebrow(p, "NEXT");
            p.spawn(Node { column_gap: Val::Px(16.0), align_items: AlignItems::Baseline, ..default() }).with_children(|r| {
                r.spawn(t("", display(54.0), TEXT, T::NextCall));
                r.spawn(t("", display(34.0), DIM, T::NextDist));
            });
            p.spawn(t("", label(18.0), CREST, T::NextMods));
            // Call cue: the bar fills as the next note comes into reach; green = call it now.
            let (lo, hi) = coach::window();
            p.spawn((Node { width: Val::Percent(100.0), height: Val::Px(12.0), margin: UiRect::top(Val::Px(8.0)), ..default() }, BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.08)))).with_children(|b| {
                b.spawn((
                    CueWindow,
                    Node { position_type: PositionType::Absolute, left: Val::Percent(lo * 100.0), width: Val::Percent((hi - lo) * 100.0), height: Val::Percent(100.0), ..default() },
                    BackgroundColor(GOOD.with_alpha(0.28)),
                ));
                b.spawn((CueFill, Node { position_type: PositionType::Absolute, left: Val::Percent(0.0), width: Val::Px(5.0), top: Val::Px(-4.0), bottom: Val::Px(-4.0), ..default() }, BackgroundColor(TEXT)));
            });
            p.spawn(Node { column_gap: Val::Px(18.0), align_items: AlignItems::Baseline, ..default() }).with_children(|r| {
                r.spawn(t("", label(17.0), DIM, T::Cue));
                r.spawn(t("", display(24.0), DIM, T::CallGrade));
            });
            rule(p);
            eyebrow(p, "COMING UP");
            // Three columns of equal-height cells, so the different type sizes line up.
            p.spawn(Node { column_gap: Val::Px(14.0), ..default() }).with_children(|cols| {
                let col = |cols: &mut ChildSpawnerCommands, width: Option<f32>, end: bool, font: &dyn Fn() -> TextFont, color: Color, tag: fn(usize) -> T| {
                    cols.spawn(Node { flex_direction: FlexDirection::Column, width: width.map_or(Val::Auto, Val::Px), align_items: if end { AlignItems::FlexEnd } else { AlignItems::FlexStart }, ..default() })
                        .with_children(|c| {
                            for i in 0..n_rows {
                                c.spawn(Node { height: Val::Px(38.0), align_items: AlignItems::Center, ..default() }).with_child(t("", font(), color, tag(i)));
                            }
                        });
                };
                col(cols, Some(62.0), true, &|| display(22.0), DIM, T::RowDist);
                col(cols, Some(110.0), false, &|| display(28.0), TEXT, T::RowCall);
                col(cols, None, false, &|| label(15.0), DIM, T::RowMods);
            });
            rule(p);
            eyebrow(p, "SPLITS");
            for (i, name) in ["S1", "S2", "S3", "FINISH"].into_iter().enumerate() {
                p.spawn(Node { column_gap: Val::Px(14.0), align_items: AlignItems::Baseline, ..default() }).with_children(|r| {
                    r.spawn((Text::new(name), display(24.0), TextColor(DIM), Node { width: Val::Px(90.0), ..default() }));
                    r.spawn(t("", display(24.0), DIM, T::Split(i)));
                });
            }
            p.spawn(t("", label(15.0), DIM, T::Calls));
        });
    // Map overlays.
    let mut overlay = commands.spawn((
        GuideView,
        t("", display(40.0), TEXT, T::Overlay),
        Node { position_type: PositionType::Absolute, left: Val::Px(0.0), right: Val::Px(panel_w), top: Val::Percent(40.0), justify_content: JustifyContent::Center, ..default() },
        TextLayout::justify(Justify::Center),
    ));
    if let Some(t) = &target {
        overlay.insert(t.clone());
    }
    let mut countdown = commands.spawn((
        GuideView,
        t("", display(120.0), Color::srgb(1.0, 0.85, 0.3), T::Countdown),
        Node { position_type: PositionType::Absolute, left: Val::Px(0.0), right: Val::Px(panel_w), top: Val::Percent(12.0), justify_content: JustifyContent::Center, ..default() },
        TextLayout::justify(Justify::Center),
    ));
    if let Some(t) = &target {
        countdown.insert(t.clone());
    }
    let hint = match screen {
        GuideScreen::Map => "WHEEL / + -  ZOOM    DRAG  PAN    F  FOLLOW    M  WHOLE STAGE    H  HEADING UP",
        GuideScreen::InCar => "HOLD SPACE / RIGHT MOUSE  RAISE    E  CALL    WHEEL  ZOOM",
    };
    let mut foot = commands.spawn((GuideView, Node { position_type: PositionType::Absolute, left: Val::Px(28.0), bottom: Val::Px(24.0), flex_direction: FlexDirection::Column, row_gap: Val::Px(6.0), ..default() }));
    if let Some(t) = &target {
        foot.insert(t.clone());
    }
    foot.with_children(|p| {
        p.spawn(Node { column_gap: Val::Px(10.0), align_items: AlignItems::Center, ..default() }).with_children(|r| {
            r.spawn((ScaleBar, Node { width: Val::Px(100.0), height: Val::Px(4.0), ..default() }, BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.7))));
            r.spawn(t("100 m", label(15.0), DIM, T::Scale));
        });
        p.spawn((Text::new(hint), label(14.0), TextColor(Color::srgba(1.0, 1.0, 1.0, 0.4))));
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

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn update(
    feed: Res<GuideFeed>,
    session: Res<Session>,
    stage: Option<Res<GuideStage>>,
    view: Res<MapView>,
    calls: Option<Res<GuideCalls>>,
    mut texts: Query<(&T, &mut Text, &mut TextColor)>,
    mut fill: Query<&mut Node, (With<ProgressFill>, Without<ScaleBar>, Without<CueFill>)>,
    mut bar: Query<&mut Node, (With<ScaleBar>, Without<ProgressFill>, Without<CueFill>)>,
    mut cue_q: Query<(&mut Node, &mut BackgroundColor), (With<CueFill>, Without<ProgressFill>, Without<ScaleBar>)>,
    mut notes: Local<Vec<Note>>,
    mut s: Local<String>,
) {
    // No more notes once the run is over.
    match &stage {
        Some(g) if feed.run.finished.is_none() => upcoming(g, feed.s, NOTES, &mut notes),
        _ => notes.clear(),
    }
    let run = feed.run;
    let car = feed.car;
    let speed = car.map_or(0.0, |c| c.speed());
    let cue = calls.as_ref().filter(|_| run.running()).and_then(|c| c.coach.cue(feed.s, speed));
    // The last call's word stays up for 2.5 s of run time.
    let last = calls.as_ref().and_then(|c| c.coach.last).filter(|(_, at)| (feed.clock - at) < 2.5 && feed.clock >= *at);
    let tally = calls.as_ref().map(|c| c.coach.tally).unwrap_or_default();
    for (tag, mut text, mut color) in &mut texts {
        s.clear();
        let mut c = color.0;
        match *tag {
            T::Stage => match &stage {
                Some(g) => {
                    let _ = write!(s, "STAGE {}  {}  {:.1} KM  {:?}  {:?} {:?}", g.def.id, g.def.name.to_uppercase(), g.road.length() / 1000.0, g.def.surface, g.def.weather, g.def.time);
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
                        s.push('R');
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
                } else if let Some(f) = run.finished {
                    s.push_str("FINISH  ");
                    clock(&mut s, f);
                    if let Some(score) = tally.score() {
                        let _ = write!(s, "\nCALL TIMING {score}%");
                    }
                    c = TEXT;
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
            T::Damage => c = damage(&mut s, &feed.damage),
            T::Cue => {
                // Name the note the cue is for: it may be the one after the corner in progress.
                if let (Some(q), Some(g), Some(cl)) = (cue, &stage, &calls)
                    && let Some(n) = cl.coach.notes().get(q.note)
                {
                    if q.now {
                        s.push_str("CALL ");
                        call(&mut s, g, n);
                        s.push_str(" NOW");
                        c = GOOD;
                    } else if q.to_go < coach::LATE_S {
                        s.push_str("LATE: CALL ");
                        call(&mut s, g, n);
                        c = BAD;
                    } else {
                        call(&mut s, g, n);
                        let _ = write!(s, " IN {:.0} S", (q.to_go - coach::EARLY_S).max(0.0).ceil());
                        c = DIM;
                    }
                }
            }
            T::CallGrade => {
                if let Some((g, _)) = last {
                    s.push_str(g.word());
                    c = match g {
                        Grade::OnTime => GOOD,
                        Grade::Early => WARN,
                        Grade::Late | Grade::Missed => BAD,
                    };
                }
            }
            T::Calls => {
                if let Some(score) = tally.score() {
                    let _ = write!(s, "CALL TIMING {score}%   {} ON TIME  {} EARLY  {} LATE  {} MISSED", tally.on_time, tally.early, tally.late, tally.missed);
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
    if let Ok((mut n, mut bg)) = cue_q.single_mut() {
        let (x, col) = match cue {
            Some(q) => (q.fill, if q.now { GOOD } else if q.to_go < coach::LATE_S { BAD } else { TEXT }),
            None => (0.0, Color::NONE),
        };
        let left = Val::Percent(x * 100.0);
        if n.left != left {
            n.left = left;
        }
        if bg.0 != col {
            bg.0 = col;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guide::sync::NetDamage;

    #[test]
    fn damage_reads_like_the_hud() {
        let mut s = String::new();
        assert_eq!(damage(&mut s, &NetDamage::default()), TEXT);
        assert!(s.is_empty(), "whole car: nothing shown");
        let d = NetDamage { detached: 0b101, engine: 0, steering: 8, suspension: 2 };
        let c = damage(&mut s, &d);
        assert_eq!(s, "DAMAGE  STEERING 40%  SUSPENSION 10%  2 PARTS OFF");
        assert_eq!(c, WARN);
        s.clear();
        assert_eq!(damage(&mut s, &NetDamage { engine: 18, ..default() }), BAD);
    }
}
