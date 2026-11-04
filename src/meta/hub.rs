//! The Service Park: Pacenote's main menu (screen "main"), and the pieces every menu screen
//! shares.
//!
//! **Owner: stream E (UI).** Design: `docs/festival/UX_PASS.md` §1, `ART_BIBLE.md` §6,
//! `DECISIONS.md` U1-U4.
//!
//! - The hub: a sectioned list (DRIVE / CAR / YOU) on the left, a route card for the focused
//!   item bottom right, credits in the footer, the 3D showroom ([`super::showroom`]) behind.
//!   ↑/↓ wrap, Q/E (LB/RB) jump to the next or previous section, Esc does nothing (Quit is an
//!   item and asks first).
//! - [`top_bar`]: "PACENOTE · VALLEY TRIALS", the active car chip (name, number, class, PI),
//!   the driver grade with a 7-segment bar and the Notes balance, which counts to a new value
//!   over 0.6 s. Every menu screen spawns it.
//! - [`intro`]: the screen's content slides 24 px and fades in over 160 ms when the screen
//!   changes (not on a rebuild of the same screen); cut with reduce motion.
//! - Menu sounds: focus moves, activations, Back and screen changes send
//!   [`crate::audio::ui::UiSound`].
//!
//! E never changes career state: everything here reads `career::*`.

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::Button;
use engine::flow::AppState;
use engine::ui::prelude::*;
use engine::ui::widgets::{ActionButton, Focus, Focusable, KeepColor, LookBg, NavInput, NavQueue, UiActivated, nav_look, secondary_look};

use super::{Boards, Profile, board};
use crate::audio::ui::UiSound;
use crate::career::garage::{self, Class, Garage, UpgradeKind};
use crate::career::grade::{Grade, MAX_GRADE};
use crate::career::wallet::Wallet;

/// The festival's name (DECISIONS glossary).
pub const SERIES: &str = "Valley Trials";
/// The town the open world starts in (WORLD_PASS).
pub const TOWN: &str = "Fernhollow";

/// Height of the top bar band (design px). Screens keep their content below it.
pub const TOP_BAR_H: f32 = 76.0;

pub struct HubPlugin;

impl Plugin for HubPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, make_glyphs)
            .init_resource::<NotesTicker>()
            .init_resource::<HubCards>()
            .init_resource::<BuiltScreen>()
            .add_systems(Update, (hub_nav, super::confirm::sideways).before(engine::ui::UiSet::Input).run_if(engine::ui::ui_visible).run_if(resource_exists::<Focus>).run_if(resource_exists::<NavQueue>))
            .add_systems(Update, ui_sounds.after(engine::ui::UiSet::Screens).run_if(resource_exists::<Focus>).run_if(resource_exists::<Messages<UiActivated>>).run_if(resource_exists::<Messages<NavBack>>))
            .add_systems(Update, (info_card, animate_intro, tick_notes, refresh_on_career_change).after(engine::ui::UiSet::Paint));
    }
}

// ---------------------------------------------------------------- formatting

/// 48250 → "48,250".
pub fn thousands(n: u32) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// "12,400 N".
pub fn notes(n: u32) -> String {
    format!("{} N", thousands(n))
}

/// Reduce motion is on (engine accessibility setting).
pub fn reduce_motion(world: &World) -> bool {
    world.get_resource::<engine::actions::Accessibility>().is_some_and(|a| a.reduce_motion)
}

/// The grade's xp span: (grade, xp at its start, xp at the next grade's start; None at the
/// top). Works with any rising curve (F owns `Grade::level`).
pub fn grade_span(xp: u32) -> (u32, u32, Option<u32>) {
    let lvl = |x: u32| Grade { xp: x }.level();
    let g = lvl(xp);
    // Lowest xp with this grade.
    let (mut lo, mut hi) = (0u32, xp);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if lvl(mid) >= g { hi = mid } else { lo = mid + 1 }
    }
    let start = lo;
    if g >= MAX_GRADE {
        return (g, start, None);
    }
    let (mut lo, mut hi) = (xp, u32::MAX / 2);
    if lvl(hi) <= g {
        return (g, start, None);
    }
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if lvl(mid) > g { hi = mid } else { lo = mid + 1 }
    }
    (g, start, Some(lo))
}

/// The active car as the chip shows it: name, race number, class and PI.
pub struct CarChip {
    pub name: String,
    pub number: u8,
    pub class: Class,
    pub pi: u16,
}

pub fn car_chip(g: &Garage) -> CarChip {
    let car = g.active_car();
    let spec = garage::spec_of(car);
    let pi = garage::pi(&spec);
    CarChip { name: car.def().map_or_else(|| car.model.clone(), |d| d.name.to_string()), number: car.number, class: Class::of(pi), pi }
}

// ---------------------------------------------------------------- intro motion

/// Remembers which screen was built last, so a rebuild of the same screen (after a purchase)
/// doesn't replay the intro.
#[derive(Resource, Default)]
pub struct BuiltScreen(Option<(&'static str, usize)>);

/// Slides from `dx` px to 0 and fades from the veil, starting at `t0` (real seconds).
#[derive(Component, Clone, Copy)]
pub struct Intro {
    t0: f32,
    dx: f32,
}

/// A full-screen veil over a screen that fades out as it comes in.
#[derive(Component, Clone, Copy)]
struct Veil {
    t0: f32,
    from: f32,
}

/// Slide of the content and fade length (UX_PASS §1).
const INTRO_PX: f32 = 24.0;
const INTRO_S: f32 = 0.16;

/// Whether the screen being built should play its intro: it's another screen than last
/// time (not a rebuild) and reduce motion is off. Returns the intro component to put on the
/// content column (and spawns the veil under `root`).
pub fn intro(world: &mut World, root: Entity) -> Option<Intro> {
    if world.contains_resource::<super::confirm::BuildingUnder>() {
        return None;
    }
    let now = world.get_resource::<Screens>().and_then(|s| s.top().map(|t| (t, s.stack.len())));
    let last = std::mem::replace(&mut world.resource_mut::<BuiltScreen>().0, now);
    if now == last || now.is_none() || reduce_motion(world) {
        return None;
    }
    let back = matches!((now, last), (Some((_, a)), Some((_, b))) if a < b);
    let t0 = world.resource::<Time<Real>>().elapsed_secs();
    let bg = world.resource::<Theme>().bg;
    world.commands().entity(root).with_children(|p| {
        p.spawn((Veil { t0, from: 0.55 }, fullscreen(), BackgroundColor(bg.with_alpha(0.55)), GlobalZIndex(25), Pickable::IGNORE));
    });
    Some(Intro { t0, dx: if back { -INTRO_PX } else { INTRO_PX } })
}

fn ease_out(x: f32) -> f32 {
    1.0 - (1.0 - x.clamp(0.0, 1.0)).powi(3)
}

fn animate_intro(time: Res<Time<Real>>, mut q: Query<(&Intro, &mut UiTransform)>, mut veils: Query<(&Veil, &mut BackgroundColor)>) {
    let now = time.elapsed_secs();
    for (i, mut tf) in &mut q {
        let k = ease_out((now - i.t0) / INTRO_S);
        let x = i.dx * (1.0 - k);
        if tf.translation.x != Val::Px(x) {
            tf.translation.x = Val::Px(x);
        }
    }
    for (v, mut bg) in &mut veils {
        let a = v.from * (1.0 - ease_out((now - v.t0) / INTRO_S));
        if (bg.0.alpha() - a).abs() > 0.002 {
            bg.0.set_alpha(a);
        }
    }
}

// ---------------------------------------------------------------- top bar

/// The Notes figure in the top bar (counted by [`tick_notes`]).
#[derive(Component)]
pub struct NotesText;

/// The balance as the top bar shows it, counting to the wallet over 0.6 s.
#[derive(Resource, Default)]
pub struct NotesTicker {
    shown: f32,
    from: f32,
    to: u32,
    t0: f32,
    init: bool,
}

/// The shared top bar of every menu screen (only in the main menu, not in game).
pub fn top_bar(world: &mut World, root: Entity) {
    if world.get_resource::<State<AppState>>().is_some_and(|s| *s.get() != AppState::MainMenu) {
        return;
    }
    let t = world.resource::<Theme>().clone();
    let chip = car_chip(world.resource::<Garage>());
    let xp = world.resource::<Grade>().xp;
    let (g, lo, hi) = grade_span(xp);
    let shown = {
        let w = world.resource::<Wallet>().balance;
        let tk = world.resource::<NotesTicker>();
        if tk.init { tk.shown.round() as u32 } else { w }
    };
    world.commands().entity(root).with_children(|p| {
        p.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(EDGE),
                right: px(EDGE),
                top: px(0),
                height: px(TOP_BAR_H),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BorderColor::all(t.line),
            GlobalZIndex(24),
            Pickable::IGNORE,
        ))
        .with_children(|bar| {
            // Brand: PACENOTE · VALLEY TRIALS, with a road-book tick strip.
            bar.spawn((Node { column_gap: px(GAP_S + 4.0), align_items: AlignItems::Center, ..default() }, Pickable::IGNORE)).with_children(|b| {
                b.spawn((Node { width: px(36), height: px(3), ..default() }, BackgroundColor(t.accent), Pickable::IGNORE));
                b.spawn(text("PACENOTE", label(22.0), t.text));
                b.spawn(text("\u{b7}", label(22.0), t.text_dim));
                b.spawn(text(SERIES.to_uppercase(), label(22.0), t.accent));
            });
            bar.spawn((Node { column_gap: px(GAP_XL - 8.0), align_items: AlignItems::Center, ..default() }, Pickable::IGNORE)).with_children(|r| {
                // Active car chip: class plate, PI, name and number.
                r.spawn((Node { column_gap: px(GAP_S + 2.0), align_items: AlignItems::Center, ..default() }, Pickable::IGNORE)).with_children(|c| {
                    class_plate(c, &t, chip.class, chip.pi);
                    c.spawn(text(format!("{} #{}", chip.name.to_uppercase(), chip.number), label(20.0), t.text));
                });
                // Grade: number, 7 segments (filled / hollow), xp in this grade.
                r.spawn((Node { column_gap: px(GAP_S + 2.0), align_items: AlignItems::Center, ..default() }, Pickable::IGNORE)).with_children(|c| {
                    c.spawn(text(format!("GRADE {g}"), label(20.0), t.text));
                    let frac = hi.map_or(1.0, |h| (xp - lo) as f32 / (h - lo).max(1) as f32);
                    segments(c, &t, frac, 7);
                    let xp_line = match hi {
                        Some(h) => format!("{} / {} XP", thousands(xp - lo), thousands(h - lo)),
                        None => "TOP GRADE".into(),
                    };
                    c.spawn(text(xp_line, body(18.0), t.text_muted));
                });
                // Notes.
                r.spawn((Node { column_gap: px(6), align_items: AlignItems::Baseline, ..default() }, Pickable::IGNORE)).with_children(|c| {
                    c.spawn((NotesText, text(thousands(shown), display(34.0), t.text)));
                    c.spawn(text("N", label(20.0), t.text_muted));
                });
            });
        });
    });
}

/// A small class plate: the letter on a light plate and the PI beside it ("B 570").
pub fn class_plate(p: &mut ChildSpawnerCommands, t: &Theme, class: Class, pi: u16) {
    p.spawn((Node { align_items: AlignItems::Center, border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(3)), ..default() }, BorderColor::all(t.line_strong), Pickable::IGNORE)).with_children(|plate| {
        plate.spawn((Node { padding: UiRect::axes(px(7), px(1)), ..default() }, BackgroundColor(t.text), Pickable::IGNORE)).with_children(|l| {
            l.spawn((text(class.letter(), label(18.0), t.bg), KeepColor));
        });
        plate.spawn((Node { padding: UiRect::axes(px(7), px(1)), ..default() }, Pickable::IGNORE)).with_children(|l| {
            l.spawn((text(pi.to_string(), label(18.0), t.text), KeepColor));
        });
    });
}

/// `n` segments, the first `frac × n` filled, the rest hollow (shape carries it, not colour).
pub fn segments(p: &mut ChildSpawnerCommands, t: &Theme, frac: f32, n: usize) {
    let full = (frac.clamp(0.0, 1.0) * n as f32).floor() as usize;
    p.spawn((Node { column_gap: px(3), align_items: AlignItems::Center, ..default() }, Pickable::IGNORE)).with_children(|s| {
        for k in 0..n {
            let on = k < full;
            s.spawn((
                Node { width: px(16), height: px(8), border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(1)), ..default() },
                BackgroundColor(if on { t.text } else { Color::NONE }),
                BorderColor::all(if on { t.text } else { t.line_strong }),
                Pickable::IGNORE,
            ));
        }
    });
}

fn tick_notes(world: &mut World) {
    let Some(balance) = world.get_resource::<Wallet>().map(|w| w.balance) else { return };
    let now = world.resource::<Time<Real>>().elapsed_secs();
    let instant = reduce_motion(world);
    let shown = {
        let mut tk = world.resource_mut::<NotesTicker>();
        if !tk.init {
            *tk = NotesTicker { shown: balance as f32, from: balance as f32, to: balance, t0: now, init: true };
        }
        if tk.to != balance {
            tk.from = tk.shown;
            tk.to = balance;
            tk.t0 = now;
        }
        let k = if instant { 1.0 } else { ease_out((now - tk.t0) / 0.6) };
        tk.shown = tk.from + (tk.to as f32 - tk.from) * k;
        thousands(tk.shown.round() as u32)
    };
    let mut q = world.query_filtered::<&mut Text, With<NotesText>>();
    for mut txt in q.iter_mut(world) {
        if txt.0 != shown {
            txt.0 = shown.clone();
        }
    }
}

/// Career state changed while a menu showing it is up (F applied a request): rebuild it (the
/// same screen keeps its focus and skips the intro).
fn refresh_on_career_change(wallet: Option<Res<Wallet>>, garage: Option<Res<Garage>>, grade: Option<Res<Grade>>, screens: Option<Res<Screens>>, mut out: MessageWriter<RebuildScreen>) {
    let changed = wallet.is_some_and(|w| w.is_changed() && !w.is_added()) || garage.is_some_and(|g| g.is_changed() && !g.is_added()) || grade.is_some_and(|g| g.is_changed() && !g.is_added());
    if changed && matches!(screens.and_then(|s| s.top()), Some("main" | "garage" | "upgrades" | "dealer" | "logbook")) {
        out.write(RebuildScreen);
    }
}

// ---------------------------------------------------------------- the hub

/// A hub item's row.
#[derive(Component, Clone, Copy)]
pub struct HubRow {
    pub idx: usize,
    pub section: usize,
}

/// The route card slot (bottom right); `shown` is the item it shows.
#[derive(Component)]
struct InfoCard {
    shown: Option<usize>,
}

/// What the route card says for each item (filled when the hub is built).
#[derive(Resource, Default, Clone)]
pub struct HubCards(pub Vec<Card>);

#[derive(Clone, Debug, Default)]
pub struct Card {
    pub kicker: String,
    pub title: String,
    pub body: String,
    /// The next step, and whether it's good news (accent) or plain.
    pub next: Option<(String, bool)>,
}

/// Section names, in order (the footer link is in the last).
pub const SECTIONS: [&str; 3] = ["Drive", "Car", "You"];

/// One hub item.
pub struct Item {
    pub label: &'static str,
    pub section: usize,
    pub action: MenuAction,
}

/// The hub's items (DECISIONS U1), top to bottom. The demo's store button goes under
/// Continue.
pub fn items() -> Vec<Item> {
    let mut v = vec![
        Item { label: "Continue", section: 0, action: MenuAction::Custom("continue") },
        Item { label: "Events", section: 0, action: MenuAction::Open("events") },
        Item { label: "Challenges", section: 0, action: MenuAction::Open("challenges") },
        Item { label: "Play together", section: 0, action: MenuAction::Open("together") },
        Item { label: "Garage", section: 1, action: MenuAction::Open("garage") },
        Item { label: "Upgrades", section: 1, action: MenuAction::Open("upgrades") },
        Item { label: "Dealership", section: 1, action: MenuAction::Open("dealer") },
        Item { label: "Logbook", section: 2, action: MenuAction::Open("logbook") },
        Item { label: "Settings", section: 2, action: MenuAction::Settings },
        Item { label: "Quit", section: 2, action: MenuAction::Custom("quit_ask") },
    ];
    if crate::demo::ACTIVE {
        v.insert(1, Item { label: crate::demo::store_button(), section: 0, action: MenuAction::Custom("wishlist") });
    }
    v
}

/// Focus order of the Credits link (after Quit).
const CREDITS_ORDER: u32 = 40;

/// What the hub knows about the player, gathered once per build.
struct HubData {
    events: usize,
    events_done: usize,
    next_event: Option<String>,
    times: usize,
    roads: usize,
    car: CarChip,
    cars_owned: usize,
    affordable_parts: usize,
    cheapest_part: Option<(UpgradeKind, u8, u32)>,
    affordable_cars: usize,
    cheapest_car: Option<(&'static str, u32)>,
    balance: u32,
    parked: bool,
    challenges: usize,
}

fn gather(world: &World) -> HubData {
    let p = world.resource::<Profile>();
    let b = &world.resource::<Boards>().0;
    let me = board::team_key(&p.driver_name, "");
    let done: Vec<bool> = p.events.iter().map(|e| b.records.iter().any(|r| r.stage == e.id && r.team() == me)).collect();
    let g = world.resource::<Garage>();
    let w = world.resource::<Wallet>();
    let car = g.active_car();
    let mut parts: Vec<(UpgradeKind, u8, u32)> = UpgradeKind::ALL
        .iter()
        .filter_map(|k| {
            let next = car.bought.get(*k).max(car.fitted.get(*k)) + 1;
            if next > k.max_level() {
                return None;
            }
            let class = Class::of(garage::pi(&garage::spec_of(car)));
            garage::part_price(*k, next, class).map(|price| (*k, next, price))
        })
        .collect();
    parts.sort_by_key(|x| x.2);
    let affordable_parts = parts.iter().filter(|(k, l, _)| g.can_fit(w, *k, *l).is_ok()).count();
    let mut cars: Vec<(&'static str, u32)> = garage::CARS.iter().filter(|d| !g.owned.iter().any(|c| c.model == d.id)).map(|d| (d.name, d.price)).collect();
    cars.sort_by_key(|c| c.1);
    let affordable_cars = cars.iter().filter(|c| c.1 <= w.balance).count();
    let roads: std::collections::BTreeSet<u32> = b.records.iter().filter(|r| r.team() == me).map(|r| r.stage).collect();
    HubData {
        events: p.events.len(),
        events_done: done.iter().filter(|d| **d).count(),
        next_event: p.events.iter().zip(&done).find(|(_, d)| !**d).map(|(e, _)| e.name.clone()),
        times: b.records.iter().filter(|r| r.team() == me).count(),
        roads: roads.len(),
        car: car_chip(g),
        cars_owned: g.owned.len(),
        affordable_parts,
        cheapest_part: parts.first().copied(),
        affordable_cars,
        cheapest_car: cars.first().copied(),
        balance: w.balance,
        parked: p.last_pose.is_some(),
        challenges: super::screens::challenge_count(world),
    }
}

/// The muted number right of each item: its most useful fact, or nothing.
fn meta_line(label: &str, d: &HubData) -> String {
    match label {
        "Continue" => {
            if d.parked {
                "Free roam \u{b7} where you parked".into()
            } else {
                format!("Start in {TOWN}")
            }
        }
        "Events" if d.events > 0 => format!("{} / {} finished", d.events_done, d.events),
        "Challenges" if d.challenges > 0 => format!("{} by the road", d.challenges),
        "Play together" => "Driver + guide".into(),
        "Garage" => format!("{} #{}", d.car.name, d.car.number),
        "Upgrades" if d.affordable_parts > 0 => format!("{} part{} affordable", d.affordable_parts, if d.affordable_parts == 1 { "" } else { "s" }),
        "Upgrades" if d.cheapest_part.is_none() => "Fully built".into(),
        "Dealership" if d.affordable_cars > 0 => format!("{} car{} in reach", d.affordable_cars, if d.affordable_cars == 1 { "" } else { "s" }),
        "Dealership" => format!("{} car{}", garage::CARS.len(), if garage::CARS.len() == 1 { "" } else { "s" }),
        "Logbook" if d.times > 0 => format!("{} time{} set", d.times, if d.times == 1 { "" } else { "s" }),
        _ => String::new(),
    }
}

fn card_for(label: &str, d: &HubData) -> Card {
    let c = |kicker: &str, title: &str, body: &str, next: Option<(String, bool)>| Card { kicker: kicker.into(), title: title.into(), body: body.into(), next };
    match label {
        "Continue" if d.parked => c("Drive", "Free roam", "Back where you parked, in the car you drive now. The clock and the valley pick up where they were.", Some((format!("Driving the {} #{}", d.car.name, d.car.number), false))),
        "Continue" => c("Drive", "Free roam", &format!("Start at the service park in {TOWN}. Drive anywhere: events, challenges and rivals wait around the valley."), Some((format!("Driving the {} #{}", d.car.name, d.car.number), false))),
        "Events" => c(
            "Drive",
            "Events",
            "Rallies, sprints and laps on closed roads. Go now puts you on the start line; Set route shows the way on the map.",
            match (&d.next_event, d.events) {
                (_, 0) => Some(("Events appear as you discover them in the valley.".into(), false)),
                (Some(n), _) => Some((format!("Next: {n}"), true)),
                (None, _) => Some(("Every event finished. Chase your times.".into(), true)),
            },
        ),
        "Challenges" => c("Drive", "Challenges", "Radar Posts, Pace and Slide Sections and Yump Boards by the road. Drive through to start one; first stars pay Notes.", None),
        "Play together" => c("Drive", "Play together", "Two computers, one car. The driver sees the road but no map; the guide reads the map and calls the corners.", None),
        "Garage" => c("Car", "Garage", "Pick the car you drive, paint it and give it a race number.", Some((format!("{} car{} in the garage", d.cars_owned, if d.cars_owned == 1 { "" } else { "s" }), false))),
        "Upgrades" => c(
            "Car",
            "Upgrades",
            "Seven parts in three levels: Street, Gravel and Works. Owned levels refit for free.",
            d.cheapest_part.map(|(k, l, p)| (format!("{} {} {} \u{b7} you have {}", garage::level_name(l), k.name().to_lowercase(), notes(p), notes(d.balance)), p <= d.balance)),
        ),
        "Dealership" => c(
            "Car",
            "Dealership",
            "Every car in the Valley Trials, class D to S, with its price and how it drives.",
            Some(match d.cheapest_car {
                Some((name, price)) => (format!("{name} {} \u{b7} you have {}", notes(price), notes(d.balance)), price <= d.balance),
                None => ("You own every car.".into(), true),
            }),
        ),
        "Logbook" => c("You", "Logbook", "Your grade and totals, and the leaderboards with every time you've set.", (d.times > 0).then(|| (format!("{} times on {} roads", d.times, d.roads), false))),
        "Settings" => c("You", "Settings", "Driving, camera, the world's clock, HUD, audio, video and accessibility.", None),
        "Quit" => c("You", "Quit", "Notes and progress are saved as they change: nothing is lost.", None),
        "Credits" => c("Pacenote", "Credits", "Who made the game, and the open tools and assets it is built with.", None),
        other => c("Drive", other, "", None),
    }
}

/// The "main" screen.
pub fn build(world: &mut World, root: Entity) {
    let t = world.resource::<Theme>().clone();
    let d = gather(world);
    let list = items();
    let mut cards: Vec<Card> = list.iter().map(|i| card_for(i.label, &d)).collect();
    cards.push(card_for("Credits", &d));
    world.insert_resource(HubCards(cards));
    let metas: Vec<String> = list.iter().map(|i| meta_line(i.label, &d)).collect();
    let version = world.get_resource::<engine::app::GameInfo>().map_or("", |i| i.version);
    let hints = engine::ui::device_hints(world, &[("W / S", "Move"), ("Enter", "Select"), ("Q / E", "Section")], &[("D-pad", "Move"), ("A", "Select"), ("LB / RB", "Section")]);
    let intro = intro(world, root);
    world.commands().entity(root).with_children(|p| {
        menu_backdrop(p, &t, 0.0, 0.92);
        let mut col = p.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(EDGE - 26.0),
                top: px(TOP_BAR_H + 24.0),
                bottom: px(FOOTER_BOTTOM + 48.0),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                ..default()
            },
            UiTransform::default(),
            Pickable::IGNORE,
        ));
        if let Some(i) = intro {
            col.insert(i);
        }
        col.with_children(|c| {
            c.spawn((Node { margin: UiRect::new(px(26), px(0), px(0), px(GAP_S)), flex_direction: FlexDirection::Column, ..default() }, Pickable::IGNORE)).with_children(|h| {
                eyebrow_bar(h, &format!("{TOWN} \u{b7} {SERIES}"), t.accent);
                h.spawn(title("Service Park", 76.0, t.text));
            });
            c.spawn(spacer(GAP_S));
            let mut order = 1;
            for (s, name) in SECTIONS.iter().enumerate() {
                section_header(c, &t, s, name);
                for (k, item) in list.iter().enumerate().filter(|(_, i)| i.section == s) {
                    hub_row(c, &t, item, &metas[k], order, HubRow { idx: k, section: s });
                    order += 1;
                }
            }
        });
        // Route card, bottom right.
        p.spawn((
            Node { position_type: PositionType::Absolute, right: px(EDGE), bottom: px(FOOTER_BOTTOM + 44.0), width: px(470), flex_direction: FlexDirection::Column, ..default() },
            InfoCard { shown: None },
            Pickable::IGNORE,
        ));
        spawn_footer(p, &t, &hints);
        // Credits: a footer link, last in the focus order.
        p.spawn((Node { position_type: PositionType::Absolute, right: px(EDGE), bottom: px(FOOTER_BOTTOM - 6.0), column_gap: px(GAP_M), align_items: AlignItems::Center, ..default() }, Pickable::IGNORE)).with_children(|r| {
            if !version.is_empty() {
                r.spawn(text(format!("v{version}"), body(18.0), t.text_dim));
            }
            r.spawn((
                Button,
                Hovered::default(),
                ActionButton(MenuAction::Open("credits").into()),
                Focusable { order: CREDITS_ORDER },
                HubRow { idx: list.len(), section: SECTIONS.len() - 1 },
                secondary_look(&t),
                Node { height: px(32), padding: UiRect::horizontal(px(14)), align_items: AlignItems::Center, border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(RADIUS)), ..default() },
                BackgroundColor(Color::NONE),
                BorderColor::all(t.line),
            ))
            .with_children(|b| {
                b.spawn(text("CREDITS", label(18.0), t.text_muted));
            });
        });
    });
    world.flush();
    top_bar(world, root);
}

/// "01  DRIVE ─────": a road-book section label (not focusable).
fn section_header(p: &mut ChildSpawnerCommands, t: &Theme, s: usize, name: &str) {
    p.spawn((Node { column_gap: px(GAP_S + 2.0), align_items: AlignItems::Center, width: px(560), margin: UiRect::new(px(26), px(0), px(if s == 0 { 0.0 } else { GAP_M }), px(GAP_XS)), ..default() }, Pickable::IGNORE))
        .with_children(|h| {
            h.spawn(text(format!("{:02}", s + 1), label(18.0), t.accent));
            h.spawn(text(name.to_uppercase(), label(18.0), t.text_muted));
            h.spawn((Node { flex_grow: 1.0, height: px(1), ..default() }, BackgroundColor(t.line), Pickable::IGNORE));
        });
}

fn hub_row(p: &mut ChildSpawnerCommands, t: &Theme, item: &Item, meta: &str, order: u32, row: HubRow) {
    p.spawn((
        Button,
        Hovered::default(),
        ActionButton(item.action.into()),
        Focusable { order },
        row,
        nav_look(t),
        Node { width: px(586), height: px(50), align_items: AlignItems::Center, justify_content: JustifyContent::SpaceBetween, padding: UiRect::new(px(26), px(14), px(0), px(0)), ..default() },
        BackgroundColor(Color::NONE),
    ))
    .with_children(|b| {
        b.spawn((LookBg([Color::NONE, t.accent, Color::NONE]), Node { position_type: PositionType::Absolute, left: px(0), top: px(10), bottom: px(10), width: px(4), ..default() }, BackgroundColor(Color::NONE), Pickable::IGNORE));
        b.spawn((text(item.label.to_uppercase(), label(31.0), t.text_muted), TextLayout::no_wrap()));
        if !meta.is_empty() {
            b.spawn((text(meta, body(18.0), t.text_muted), KeepColor, TextLayout::no_wrap()));
        }
    });
}

/// Rebuilds the route card when the focused hub item changes.
fn info_card(world: &mut World, mut last: Local<Option<usize>>) {
    let (top, under) = world.get_resource::<Screens>().map_or((None, None), |s| (s.top(), s.stack.len().checked_sub(2).map(|i| s.stack[i])));
    let idx = match (top, under) {
        (Some("main"), _) => {
            let idx = world.get_resource::<Focus>().and_then(|f| f.0).and_then(|e| world.get::<HubRow>(e)).map(|r| r.idx);
            *last = idx;
            idx
        }
        // Under the confirm dialog: the card of the item that opened it.
        (Some(super::confirm::SCREEN), Some("main")) => *last,
        _ => return,
    };
    let mut q = world.query::<(Entity, &InfoCard)>();
    let Some((slot, shown)) = q.iter(world).next().map(|(e, c)| (e, c.shown)) else { return };
    if shown == idx {
        return;
    }
    if let Some(mut c) = world.get_mut::<InfoCard>(slot) {
        c.shown = idx;
    }
    world.entity_mut(slot).despawn_related::<Children>();
    let Some(card) = idx.and_then(|i| world.resource::<HubCards>().0.get(i).cloned()) else { return };
    let t = world.resource::<Theme>().clone();
    let glyphs = world.get_resource::<Glyphs>().cloned().unwrap_or_default();
    world.commands().entity(slot).with_children(|s| {
        s.spawn(accent_panel(&t, t.accent, 22.0, GAP_S)).with_children(|c| {
            c.spawn((Node { justify_content: JustifyContent::SpaceBetween, ..default() }, Pickable::IGNORE)).with_children(|h| {
                h.spawn(eyebrow(&card.kicker, t.text_dim));
                h.spawn(text("ROUTE CARD", label(SIZE_LABEL), t.text_dim));
            });
            c.spawn(text(card.title.to_uppercase(), display(36.0), t.text));
            if !card.body.is_empty() {
                c.spawn((text(card.body, body(SIZE_BODY - 1.0), t.text_muted), Node { max_width: px(420), ..default() }));
            }
            if let Some((next, good)) = card.next {
                c.spawn(rule(t.line));
                c.spawn((Node { column_gap: px(GAP_S), align_items: AlignItems::Center, ..default() }, Pickable::IGNORE)).with_children(|r| {
                    glyph(r, &glyphs, Glyph::Arrow, 16.0, if good { t.accent } else { t.text_dim });
                    r.spawn((text(next, body_bold(SIZE_BODY - 2.0), if good { t.text } else { t.text_muted }), Node { max_width: px(400), ..default() }));
                });
            }
        });
    });
}

/// Hub navigation on top of the engine's: ↑/↓ wrap, Q/E (LB/RB) jump sections.
fn hub_nav(screens: Option<Res<Screens>>, mut queue: ResMut<NavQueue>, mut focus: ResMut<Focus>, rows: Query<(Entity, &Focusable, Option<&HubRow>)>) {
    if screens.and_then(|s| s.top()) != Some("main") || queue.0.is_empty() {
        return;
    }
    let all: Vec<(Entity, u32, Option<HubRow>)> = rows.iter().map(|(e, f, r)| (e, f.order, r.copied())).collect();
    let (Some(first), Some(last)) = (all.iter().min_by_key(|x| x.1).copied(), all.iter().max_by_key(|x| x.1).copied()) else { return };
    let mut keep = Vec::with_capacity(queue.0.len());
    for input in std::mem::take(&mut queue.0) {
        let cur = focus.0.and_then(|e| all.iter().find(|x| x.0 == e).copied());
        match (input, cur) {
            (NavInput::Up, Some(c)) if c.1 == first.1 => focus.0 = Some(last.0),
            (NavInput::Down, Some(c)) if c.1 == last.1 => focus.0 = Some(first.0),
            (NavInput::NextTab | NavInput::PrevTab, c) => {
                let s = c.and_then(|c| c.2).map_or(0, |r| r.section);
                let n = SECTIONS.len();
                let to = if input == NavInput::NextTab { (s + 1) % n } else { (s + n - 1) % n };
                if let Some(t) = all.iter().filter(|x| x.2.is_some_and(|r| r.section == to && r.idx < usize::MAX)).filter(|x| x.1 < CREDITS_ORDER).min_by_key(|x| x.1) {
                    focus.0 = Some(t.0);
                }
            }
            _ => keep.push(input),
        }
    }
    queue.0 = keep;
}

/// Menu sounds from what the UI did this frame: a focus move (hover), an activation
/// (select), Back, and a screen pushed or popped (transition).
fn ui_sounds(
    screens: Option<Res<Screens>>,
    focus: Res<Focus>,
    orders: Query<&Focusable>,
    mut acts: MessageReader<UiActivated>,
    mut backs: MessageReader<NavBack>,
    mut last: Local<(Vec<&'static str>, Option<u32>)>,
    mut out: MessageWriter<UiSound>,
) {
    let stack = screens.map(|s| s.stack.clone()).unwrap_or_default();
    let activated = acts.read().any(|a| matches!(a.0, UiAction::Menu(_)));
    let backed = backs.read().count() > 0;
    // By focus order: a rebuild of the same screen keeps the order (new entities).
    let now_focus = focus.0.and_then(|e| orders.get(e).ok()).map(|f| f.order);
    let (prev, prev_focus) = std::mem::replace(&mut *last, (stack.clone(), now_focus));
    if stack.is_empty() {
        return;
    }
    if stack.len() > prev.len() && !prev.is_empty() {
        out.write(UiSound::Transition { forward: true });
    } else if stack.len() < prev.len() {
        out.write(UiSound::Transition { forward: false });
    }
    if activated {
        out.write(UiSound::Select);
    }
    if backed && stack.len() < prev.len() {
        out.write(UiSound::Back);
    }
    if stack == prev && prev_focus.is_some() && now_focus.is_some() && now_focus != prev_focus {
        out.write(UiSound::Hover);
    }
}

// ---------------------------------------------------------------- glyphs

/// Shapes the UI font (Barlow) doesn't have: drawn once into small images and tinted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Glyph {
    Check,
    Cross,
    StarFull,
    StarHollow,
    /// Rally.
    Diamond,
    /// Sprint.
    Play,
    /// Lap.
    Ring,
    Arrow,
    Lock,
    Up,
    Down,
}

impl Glyph {
    pub const ALL: [Glyph; 11] = [Glyph::Check, Glyph::Cross, Glyph::StarFull, Glyph::StarHollow, Glyph::Diamond, Glyph::Play, Glyph::Ring, Glyph::Arrow, Glyph::Lock, Glyph::Up, Glyph::Down];
    /// A text stand-in for runs without images (headless).
    fn fallback(self) -> &'static str {
        match self {
            Glyph::Check => "OK",
            Glyph::Cross | Glyph::Lock => "X",
            Glyph::StarFull => "*",
            Glyph::StarHollow => "-",
            Glyph::Diamond | Glyph::Play | Glyph::Ring => "",
            Glyph::Arrow => ">",
            Glyph::Up => "+",
            Glyph::Down => "-",
        }
    }
}

/// The glyph images, white with alpha (tint with the node colour).
#[derive(Resource, Clone, Default)]
pub struct Glyphs(pub Vec<(Glyph, Handle<Image>)>);

impl Glyphs {
    pub fn get(&self, g: Glyph) -> Option<Handle<Image>> {
        self.0.iter().find(|(k, _)| *k == g).map(|(_, h)| h.clone())
    }
}

/// Spawns glyph `g`, `size` px square, in `color`.
pub fn glyph(p: &mut ChildSpawnerCommands, glyphs: &Glyphs, g: Glyph, size: f32, color: Color) {
    match glyphs.get(g) {
        Some(h) => {
            p.spawn((ImageNode { image: h, color, ..default() }, Node { width: px(size), height: px(size), flex_shrink: 0.0, ..default() }, Pickable::IGNORE));
        }
        None => {
            p.spawn((text(g.fallback(), label(size * 0.8), color), KeepColor));
        }
    }
}

/// Five-point star outline (unit square, y down).
fn star_points() -> Vec<Vec2> {
    (0..10)
        .map(|i| {
            let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
            let r = if i % 2 == 0 { 0.46 } else { 0.19 };
            Vec2::new(0.5 + a.cos() * r, 0.53 + a.sin() * r)
        })
        .collect()
}

fn seg_dist(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-9)).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

fn poly_dist(p: Vec2, pts: &[Vec2]) -> f32 {
    let n = pts.len();
    let mut d = f32::MAX;
    let mut inside = false;
    for i in 0..n {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        d = d.min(seg_dist(p, a, b));
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
    }
    if inside { -d } else { d }
}

fn lines_dist(p: Vec2, lines: &[(Vec2, Vec2)], half: f32) -> f32 {
    lines.iter().map(|(a, b)| seg_dist(p, *a, *b)).fold(f32::MAX, f32::min) - half
}

/// Signed distance (unit square, y down) to glyph `g`: < 0 inside.
pub fn glyph_dist(g: Glyph, p: Vec2) -> f32 {
    let v = Vec2::new;
    match g {
        Glyph::Check => lines_dist(p, &[(v(0.16, 0.52), v(0.40, 0.76)), (v(0.40, 0.76), v(0.86, 0.24))], 0.075),
        Glyph::Cross => lines_dist(p, &[(v(0.22, 0.22), v(0.78, 0.78)), (v(0.78, 0.22), v(0.22, 0.78))], 0.075),
        Glyph::StarFull => poly_dist(p, &star_points()),
        Glyph::StarHollow => (poly_dist(p, &star_points()) + 0.04).abs() - 0.04,
        Glyph::Diamond => (p.x - 0.5).abs() + (p.y - 0.5).abs() - 0.42,
        Glyph::Play => poly_dist(p, &[v(0.22, 0.12), v(0.88, 0.5), v(0.22, 0.88)]),
        Glyph::Ring => (p.distance(v(0.5, 0.5)) - 0.33).abs() - 0.075,
        Glyph::Arrow => lines_dist(p, &[(v(0.12, 0.5), v(0.84, 0.5)), (v(0.56, 0.22), v(0.86, 0.5)), (v(0.86, 0.5), v(0.56, 0.78))], 0.07),
        Glyph::Lock => {
            let body = poly_dist(p, &[v(0.2, 0.46), v(0.8, 0.46), v(0.8, 0.9), v(0.2, 0.9)]);
            let shackle = if p.y < 0.46 { (p.distance(v(0.5, 0.42)) - 0.2).abs() - 0.065 } else { (p.x - 0.5).abs().max(0.0) + 1.0 };
            body.min(shackle)
        }
        Glyph::Up => poly_dist(p, &[v(0.5, 0.16), v(0.88, 0.8), v(0.12, 0.8)]),
        Glyph::Down => poly_dist(p, &[v(0.12, 0.2), v(0.88, 0.2), v(0.5, 0.84)]),
    }
}

/// White-with-alpha RGBA8 pixels of glyph `g`, `n`² (anti-aliased).
pub fn glyph_pixels(g: Glyph, n: u32) -> Vec<u8> {
    let mut px = Vec::with_capacity((n * n * 4) as usize);
    for y in 0..n {
        for x in 0..n {
            let p = Vec2::new((x as f32 + 0.5) / n as f32, (y as f32 + 0.5) / n as f32);
            let a = (0.5 - glyph_dist(g, p) * n as f32).clamp(0.0, 1.0);
            px.extend_from_slice(&[255, 255, 255, (a * 255.0).round() as u8]);
        }
    }
    px
}

fn make_glyphs(mut commands: Commands, images: Option<ResMut<Assets<Image>>>) {
    let Some(mut images) = images else {
        commands.insert_resource(Glyphs::default());
        return;
    };
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    let n = 64;
    let v = Glyph::ALL
        .iter()
        .map(|g| {
            let img = Image::new(Extent3d { width: n, height: n, depth_or_array_layers: 1 }, TextureDimension::D2, glyph_pixels(*g, n), TextureFormat::Rgba8UnormSrgb, bevy::asset::RenderAssetUsages::RENDER_WORLD);
            (*g, images.add(img))
        })
        .collect();
    commands.insert_resource(Glyphs(v));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyphs_are_drawn_inside_their_square() {
        for g in Glyph::ALL {
            let px = glyph_pixels(g, 32);
            let ink = px.chunks(4).filter(|c| c[3] > 128).count();
            assert!(ink > 20, "{g:?} has ink");
            // A clear border: the corners are empty.
            assert_eq!(px[3], 0, "{g:?} top-left corner");
        }
        // Filled vs hollow star: shape carries the meaning (the hollow one's middle is empty).
        let centre = |g: Glyph| glyph_pixels(g, 32)[((17 * 32 + 16) * 4 + 3) as usize];
        assert_eq!(centre(Glyph::StarFull), 255);
        assert_eq!(centre(Glyph::StarHollow), 0);
    }

    #[test]
    fn numbers_get_thousands_commas() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(12_000), "12,000");
        assert_eq!(thousands(48_250), "48,250");
        assert_eq!(thousands(1_234_567), "1,234,567");
        assert_eq!(notes(12_400), "12,400 N");
    }

    #[test]
    fn grade_span_brackets_the_xp() {
        for xp in [0, 1, 9_999, 10_000, 25_000, 123_456] {
            let (g, lo, hi) = grade_span(xp);
            assert_eq!(Grade { xp }.level(), g);
            assert!(lo <= xp);
            assert_eq!(Grade { xp: lo }.level(), g);
            if lo > 0 {
                assert!(Grade { xp: lo - 1 }.level() < g);
            }
            if let Some(h) = hi {
                assert!(h > xp && Grade { xp: h }.level() == g + 1 && Grade { xp: h - 1 }.level() == g);
            }
        }
        assert_eq!(grade_span(10_000_000).2, None, "top grade");
    }

    #[test]
    fn hub_items_are_decisions_u1() {
        let names: Vec<&str> = items().iter().filter(|i| !i.label.contains("Steam") && !i.label.contains("itch")).map(|i| i.label).collect();
        assert_eq!(names, ["Continue", "Events", "Challenges", "Play together", "Garage", "Upgrades", "Dealership", "Logbook", "Settings", "Quit"]);
        let sections: Vec<usize> = items().iter().map(|i| i.section).collect();
        assert!(sections.windows(2).all(|w| w[0] <= w[1]), "sections in order");
    }
}
